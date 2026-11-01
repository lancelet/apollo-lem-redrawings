//! The model, its messages, and the pure `update`.
//!
//! **One source of truth: the view in paper coordinates.** The model holds
//! the paper point at the centre of the view (mm) and the scale (screen px
//! per paper mm). The redraw pane draws the paper under exactly that. The
//! scan pane is never warped or resampled; its camera is *derived*:
//!
//! - its centre is the chain's image (paper to crop px) of the paper view's
//!   centre, and
//! - its zoom is the warp's local scale there ([`Chain::scan_per_mm`], from
//!   the Jacobian) times the paper scale.
//!
//! Rotation is not matched, by choice: 17619's sheet is turned by 0.008
//! degrees, far under a pixel across a pane, and a rotated scan pane would
//! make the tiles non-axis-aligned for no visible gain. (Recorded in
//! docs/redraw-plan.md, chunk 19.) A sheet with a real turn would need the
//! scan camera to carry an angle; that is a later change to the camera, not
//! to the model.
//!
//! Panning or zooming in the scan pane maps back through crop px to paper
//! mm and sets the paper view; the other pane follows its centre. Zooming
//! about the cursor keeps the point under the cursor fixed in the pane being
//! zoomed (in the scan pane to the grid's interpolation error, as the
//! derived centre goes through the grid).
//!
//! Everything is `f64` arithmetic with no clocks, randomness or hashing, so
//! a recorded `Vec<Msg>` replays to a bitwise-identical model.

use crate::chain::Chain;
use crate::constraints::{ConstraintKind, ConstraintTrust};
use crate::group::Highlight;
use crate::layers::Layer;
use crate::theme::Theme;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The paper scale limits, screen px per mm.
const SCALE_LIMITS: (f64, f64) = (0.05, 2000.0);
/// The scan zoom limits, screen px per crop px (as `ScanView` in lmsheets-gpu).
const SCAN_ZOOM_LIMITS: (f64, f64) = (0.02, 64.0);

/// A node of the drawing: an index into the bundle's `owners/owners.json`
/// node table (pixel value `v > 0` of an owner tile is `NodeId(v - 1)`).
/// The same namespace names the redraw's primitives, through
/// `lmsheets_render::nodes::node_key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// Which panes are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Redraw on the left, scan on the right.
    SideBySide,
    /// The redraw alone.
    RedrawOnly,
    /// The scan alone.
    ScanOnly,
    /// One pane, the scan, with the redraw drawn over it. The only mode in
    /// which the warp is applied, and then to our vectors, never to the
    /// scan. Its pane is the scan pane.
    Overlay,
    /// Both panes full size on top of each other, split by a divider: the
    /// redraw on its left, the scan revealed on its right (the mobile
    /// layout's wipe). Both cameras are the side-by-side ones, locked
    /// through the chain, so the same region fills both.
    Wipe,
}

/// How the chrome is laid out. The mobile layout shows one pane at a time,
/// keeps the navigator off and puts the details in a bottom sheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    /// Panels beside the stage, side by side available.
    #[default]
    Desktop,
    /// One pane at a time (redraw, scan, overlay or wipe), a bottom sheet.
    Mobile,
}

/// How far the bottom sheet (the mobile layout's details drawer) is up. It
/// is derived from the pin and the drawer's state, never set to `Peek` or
/// `Closed` directly: [`Msg::SetSheet`] with either closes the drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Sheet {
    /// Nothing is pinned.
    Closed,
    /// Pinned: a one-line summary.
    Peek,
    /// The details, about half the stage.
    Half,
    /// The details, the whole stage.
    Full,
}

/// A pane. (The overlay is the scan pane with the redraw drawn over it.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pane {
    /// The redraw, in paper coordinates.
    Redraw,
    /// The scan as scanned.
    Scan,
}

/// What an input event or the chrome asks for. Positions are in the pane's
/// own pixels, origin at its top-left; deltas are screen pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    /// The window (the area the panes share) changed size, px.
    Resize {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// Show other panes. The view is kept.
    SetMode(Mode),
    /// Drag a pane by a screen offset: the content follows the pointer.
    Pan {
        /// The pane dragged.
        pane: Pane,
        /// Pointer movement, px.
        by: (f64, f64),
    },
    /// Zoom a pane by `factor` about a pane pixel.
    Zoom {
        /// The pane zoomed.
        pane: Pane,
        /// Greater than 1 zooms in.
        factor: f64,
        /// The pane pixel that stays put.
        about: (f64, f64),
    },
    /// Fit the sheet in the pane the mode shows first (the redraw, or the
    /// scan when it is alone or under the overlay).
    Fit,
    /// Centre the view on a paper point, mm (the navigator's click and
    /// drag). The centre stays within the sheet's bounds.
    CentreOn {
        /// The paper point.
        at: (f64, f64),
    },
    /// How opaque the redraw is in the overlay, 0 to 1.
    SetOpacity(f64),
    /// Show or hide a layer of the redraw. Hidden parts are neither drawn
    /// nor picked.
    SetLayer {
        /// The layer.
        layer: Layer,
        /// Whether it is shown.
        visible: bool,
    },
    /// Change the look.
    SetTheme(Theme),
    /// The pointer is at `at` (pane pixels) in a pane, over `node`. The
    /// pick is the shell's ([`crate::pick::Picker::pick`], pure over the
    /// loaded owner tiles and the document's geometry) and rides in the
    /// message, so `update` does no lookup and a recorded log replays
    /// without the bundle. Re-send it after a pan or zoom, as the content
    /// moves under a still pointer.
    Hover {
        /// The pane the pointer is in.
        pane: Pane,
        /// The pointer, pane px.
        at: (f64, f64),
        /// What the pick found there.
        node: Option<NodeId>,
        /// What the shell decided to light for it: the node's group
        /// ([`crate::group::Groups::highlight`]), or the piece alone (the
        /// Alt modifier). `None` lights the picked node alone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        highlight: Option<Highlight>,
    },
    /// The pointer is over a constraint icon (pane pixels), which stands
    /// for constraints `items` (indices into the constraint list) that bind
    /// the nodes in `highlight`. Like [`Msg::Hover`], the pick is the
    /// shell's and rides in the message.
    HoverConstraints {
        /// The pane.
        pane: Pane,
        /// The pointer, pane px.
        at: (f64, f64),
        /// The constraints the icon stands for.
        items: Vec<usize>,
        /// What they bind.
        highlight: Highlight,
    },
    /// The pointer left the panes.
    Leave,
    /// Pin an item: the detail panel shows it and its group stays lit. The
    /// shell chose it (the hovered group or icon, or a link in the panel)
    /// and rides it in, as with a hover.
    Pin {
        /// What is pinned.
        head: PinHead,
        /// What is lit for it.
        highlight: Highlight,
    },
    /// Let go of the pin.
    Unpin,
    /// Show the connections of what is hovered or pinned (Shift held):
    /// its immediate constraints even with the icons off, joined to what
    /// they bind, which is lit in the bound colour.
    SetConnections(bool),
    /// Show or hide every constraint icon (the one switch).
    SetConstraintsShown(bool),
    /// Open or close the detail panel of the pinned item (a pin alone only
    /// lights it and offers the panel).
    SetDetails(bool),
    /// Show or hide the icons of a trust class.
    SetConstraintTrust {
        /// The class.
        trust: ConstraintTrust,
        /// Whether its icons are shown.
        visible: bool,
    },
    /// Show or hide the icons of a type.
    SetConstraintKind {
        /// The type.
        kind: ConstraintKind,
        /// Whether its icons are shown.
        visible: bool,
    },
    /// Show only the icons of constraints that bind the hovered group.
    SetConstraintsOnSelection(bool),
    /// Show the icons of dimensions' own constraints (off by default: the
    /// dimension is drawn).
    SetDimensionConstraints(bool),
    /// Choose the layout. The mobile one has no side by side (it becomes
    /// the redraw alone) and no navigator; the desktop one has no wipe (it
    /// becomes side by side) and has the navigator. The view is kept.
    SetLayout(Layout),
    /// Where the wipe's divider is, as a fraction of the width (0.02 to
    /// 0.98): the redraw is left of it, the scan right of it.
    SetWipe(f64),
    /// Raise or lower the bottom sheet. `Half` and `Full` open the details
    /// of the pinned item (nothing happens with no pin); `Peek` and `Closed`
    /// close them.
    SetSheet(Sheet),
    /// Open or close the menu sheet (layers, constraint icons, theme,
    /// about). Opening it closes the About panel.
    SetMenu(bool),
    /// Open or close the About panel. Opening it closes the menu.
    SetAbout(bool),
    /// Show or hide the navigator.
    SetNavigator(bool),
}

/// What a pin holds: a node of the drawing (its group lit), or a constraint
/// (an index into the constraint list).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PinHead {
    /// A node.
    Node(NodeId),
    /// A constraint.
    Constraint(usize),
}

/// A pinned item and what is lit for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    /// What is pinned.
    pub head: PinHead,
    /// What is lit for it, as for a hover.
    pub highlight: Highlight,
}

/// What the redraw pane shows: the paper point at its top-left, mm, the
/// scale and the pane size. The same shape as `lmsheets_gpu::View`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperCamera {
    /// The paper point at the pane's top-left, mm.
    pub origin: (f64, f64),
    /// Screen px per paper mm.
    pub zoom: f64,
    /// The pane, px.
    pub size: (u32, u32),
}

/// What the scan pane shows: the crop pixel at its top-left, screen px per
/// crop px and the pane size. The same shape as `lmsheets_gpu::raster::ScanView`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanCamera {
    /// The crop pixel at the pane's top-left.
    pub origin: (f64, f64),
    /// Screen px per crop px.
    pub zoom: f64,
    /// The pane, px.
    pub size: (u32, u32),
}

/// The map between paper mm and the navigator's thumbnail pixels: the
/// sheet's bounds fitted, centred, in a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavMap {
    /// The paper point at the thumbnail's top-left, mm.
    pub origin: (f64, f64),
    /// Thumbnail px per mm.
    pub scale: f64,
}

impl NavMap {
    /// The thumbnail pixel of a paper point.
    pub fn to_px(&self, mm: (f64, f64)) -> (f64, f64) {
        (
            (mm.0 - self.origin.0) * self.scale,
            (mm.1 - self.origin.1) * self.scale,
        )
    }

    /// The paper point under a thumbnail pixel.
    pub fn to_paper(&self, px: (f64, f64)) -> (f64, f64) {
        (
            self.origin.0 + px.0 / self.scale,
            self.origin.1 + px.1 / self.scale,
        )
    }
}

/// The redraw's opacity over the scan until the control says otherwise.
const OPACITY_DEFAULT: f64 = 0.85;

/// How much of the pane a fitted sheet fills, each way: a margin round it.
pub const FIT_FILL: f64 = 0.94;

/// The inspector's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    chain: Chain,
    mode: Mode,
    window: (u32, u32),
    /// The paper point at the view's centre, mm.
    centre: (f64, f64),
    /// Screen px per paper mm.
    scale: f64,
    /// The node under the pointer, as picked.
    hovered: Option<NodeId>,
    /// What is lit for it (empty with no hover).
    highlight: Highlight,
    /// The pane and pane pixel of the pointer, while it is over one.
    pointer: Option<(Pane, (f64, f64))>,
    /// The redraw's opacity in the overlay.
    opacity: f64,
    /// The layers switched off.
    hidden: BTreeSet<Layer>,
    theme: Theme,
    /// The constraints under the pointer's icon.
    hovered_constraints: Vec<usize>,
    /// Constraint classes and types switched off.
    trust_hidden: BTreeSet<ConstraintTrust>,
    kind_hidden: BTreeSet<ConstraintKind>,
    only_on_selection: bool,
    dimension_constraints: bool,
    pinned: Option<Pin>,
    constraints_shown: bool,
    details_open: bool,
    connections: bool,
    layout: Layout,
    /// The wipe divider, a fraction of the window's width.
    wipe: f64,
    /// How tall the sheet is while the details are open.
    sheet_height: Sheet,
    menu_open: bool,
    about_open: bool,
    navigator: bool,
}

/// The wipe divider's limits and start: never quite at an edge, so it can
/// always be grabbed.
const WIPE_LIMITS: (f64, f64) = (0.02, 0.98);

impl Model {
    /// A model of a `window` (px), showing both panes, the sheet fitted.
    pub fn new(chain: Chain, window: (u32, u32)) -> Self {
        let mut m = Self {
            chain,
            mode: Mode::SideBySide,
            window: (window.0.max(1), window.1.max(1)),
            centre: (0.0, 0.0),
            scale: 1.0,
            hovered: None,
            highlight: Highlight::default(),
            pointer: None,
            opacity: OPACITY_DEFAULT,
            // What is not on the original sheet starts hidden: the inferred
            // dimensions and the constructions are ours, shown on request.
            hidden: BTreeSet::from([Layer::Inferred, Layer::Constructions]),
            theme: Theme::default(),
            hovered_constraints: Vec::new(),
            trust_hidden: BTreeSet::new(),
            kind_hidden: BTreeSet::new(),
            only_on_selection: false,
            dimension_constraints: false,
            pinned: None,
            // So are the constraints (and any analysis overlay).
            constraints_shown: false,
            details_open: false,
            connections: false,
            layout: Layout::Desktop,
            wipe: 0.5,
            sheet_height: Sheet::Half,
            menu_open: false,
            about_open: false,
            navigator: true,
        };
        m.fit();
        m
    }

    /// The registration the panes are locked through.
    pub fn chain(&self) -> &Chain {
        &self.chain
    }

    /// The panes shown.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The node under the pointer.
    pub fn hovered(&self) -> Option<NodeId> {
        self.hovered
    }

    /// What the hover lights: the group, or the piece.
    pub fn highlight(&self) -> &Highlight {
        &self.highlight
    }

    /// The pane and pane pixel the pointer is at, if over a pane.
    pub fn pointer(&self) -> Option<(Pane, (f64, f64))> {
        self.pointer
    }

    /// The redraw's opacity in the overlay.
    pub fn opacity(&self) -> f64 {
        self.opacity
    }

    /// Whether a layer is shown.
    pub fn layer_visible(&self, layer: Layer) -> bool {
        !self.hidden.contains(&layer)
    }

    /// The layers switched off.
    pub fn hidden_layers(&self) -> &BTreeSet<Layer> {
        &self.hidden
    }

    /// Whether the constraint icons are shown at all.
    pub fn constraints_shown(&self) -> bool {
        self.constraints_shown
    }

    /// Whether the connections view is on (Shift held).
    pub fn connections(&self) -> bool {
        self.connections
    }

    /// Whether the detail panel is open (only ever with something pinned).
    pub fn details_open(&self) -> bool {
        self.details_open && self.pinned.is_some()
    }

    /// The layout.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The wipe divider, a fraction of the window's width.
    pub fn wipe(&self) -> f64 {
        self.wipe
    }

    /// Which pane a window pixel belongs to: in the wipe, the redraw left
    /// of the divider and the scan right of it; else the pane the mode
    /// puts there.
    pub fn pane_at(&self, px: (f64, f64)) -> Option<Pane> {
        match self.mode {
            Mode::Wipe => Some(if px.0 < self.wipe * self.window.0 as f64 {
                Pane::Redraw
            } else {
                Pane::Scan
            }),
            Mode::SideBySide => Some(if px.0 < (self.window.0 / 2) as f64 {
                Pane::Redraw
            } else {
                Pane::Scan
            }),
            Mode::RedrawOnly => Some(Pane::Redraw),
            Mode::ScanOnly | Mode::Overlay => Some(Pane::Scan),
        }
    }

    /// How far up the bottom sheet is.
    pub fn sheet(&self) -> Sheet {
        match (&self.pinned, self.details_open) {
            (None, _) => Sheet::Closed,
            (Some(_), false) => Sheet::Peek,
            (Some(_), true) => self.sheet_height,
        }
    }

    /// Whether the menu sheet is open.
    pub fn menu_open(&self) -> bool {
        self.menu_open
    }

    /// Whether the About panel is open.
    pub fn about_open(&self) -> bool {
        self.about_open
    }

    /// Whether the navigator is shown.
    pub fn navigator(&self) -> bool {
        self.navigator
    }

    /// Whether what [`Model::lit`] returns is the pin (nothing hovered).
    pub fn lit_is_pin(&self) -> bool {
        self.pinned.is_some() && self.highlight.is_empty()
    }

    /// The pinned item.
    pub fn pinned(&self) -> Option<&Pin> {
        self.pinned.as_ref()
    }

    /// What is lit: the hover's, or while nothing is hovered the pin's.
    pub fn lit(&self) -> &Highlight {
        match &self.pinned {
            Some(p) if self.highlight.is_empty() => &p.highlight,
            _ => &self.highlight,
        }
    }

    /// The constraints under the pointer's icon.
    pub fn hovered_constraints(&self) -> &[usize] {
        &self.hovered_constraints
    }

    /// The trust classes whose icons are off.
    pub fn hidden_constraint_trust(&self) -> &BTreeSet<ConstraintTrust> {
        &self.trust_hidden
    }

    /// The types whose icons are off.
    pub fn hidden_constraint_kinds(&self) -> &BTreeSet<ConstraintKind> {
        &self.kind_hidden
    }

    /// Whether icons of a trust class are shown.
    pub fn constraint_trust_visible(&self, t: ConstraintTrust) -> bool {
        !self.trust_hidden.contains(&t)
    }

    /// Whether icons of a type are shown.
    pub fn constraint_kind_visible(&self, k: ConstraintKind) -> bool {
        !self.kind_hidden.contains(&k)
    }

    /// Whether only constraints binding the hovered group show.
    pub fn constraints_only_on_selection(&self) -> bool {
        self.only_on_selection
    }

    /// Whether dimensions' own constraints show.
    pub fn dimension_constraints(&self) -> bool {
        self.dimension_constraints
    }

    /// The look.
    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// The window, px.
    pub fn window(&self) -> (u32, u32) {
        self.window
    }

    /// The paper point at the view's centre, mm.
    pub fn centre(&self) -> (f64, f64) {
        self.centre
    }

    /// The paper scale, screen px per mm.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// A pane's size, or none when the mode hides it.
    pub fn pane_size(&self, pane: Pane) -> Option<(u32, u32)> {
        let (w, h) = self.window;
        match (self.mode, pane) {
            (Mode::SideBySide, Pane::Redraw) => Some((w / 2, h)),
            (Mode::SideBySide, Pane::Scan) => Some((w - w / 2, h)),
            (Mode::RedrawOnly, Pane::Redraw) | (Mode::ScanOnly | Mode::Overlay, Pane::Scan) => {
                Some((w, h))
            }
            // Both full size, one over the other.
            (Mode::Wipe, _) => Some((w, h)),
            _ => None,
        }
    }

    /// A pane's top-left in the window, px, or none when hidden.
    pub fn pane_origin(&self, pane: Pane) -> Option<(u32, u32)> {
        self.pane_size(pane)?;
        Some(match (self.mode, pane) {
            (Mode::SideBySide, Pane::Scan) => (self.window.0 / 2, 0),
            _ => (0, 0),
        })
    }

    /// The redraw pane's camera, or none when hidden.
    pub fn redraw_camera(&self) -> Option<PaperCamera> {
        let size = self.pane_size(Pane::Redraw)?;
        Some(PaperCamera {
            origin: (
                self.centre.0 - size.0 as f64 / self.scale / 2.0,
                self.centre.1 - size.1 as f64 / self.scale / 2.0,
            ),
            zoom: self.scale,
            size,
        })
    }

    /// The crop pixel at the scan pane's centre: the chain's image of the
    /// paper view's centre.
    pub fn scan_centre(&self) -> (f64, f64) {
        self.chain.paper_to_scan(self.centre)
    }

    /// The scan zoom: the warp's local scale at the view's centre times
    /// the paper scale, screen px per crop px.
    pub fn scan_zoom(&self) -> f64 {
        self.scale / self.chain.scan_per_mm(self.centre)
    }

    /// The scan pane's camera, or none when hidden.
    pub fn scan_camera(&self) -> Option<ScanCamera> {
        let size = self.pane_size(Pane::Scan)?;
        let (c, zoom) = (self.scan_centre(), self.scan_zoom());
        Some(ScanCamera {
            origin: (
                c.0 - size.0 as f64 / zoom / 2.0,
                c.1 - size.1 as f64 / zoom / 2.0,
            ),
            zoom,
            size,
        })
    }

    /// The paper box `(min, max)`, mm, the primary pane shows: the redraw
    /// pane's, or the scan pane's size over the paper scale (the scan camera
    /// is the paper view at the warp's local scale, so the same extent).
    pub fn view_box(&self) -> ((f64, f64), (f64, f64)) {
        let size = self
            .pane_size(Pane::Redraw)
            .or(self.pane_size(Pane::Scan))
            .unwrap_or(self.window);
        let half = (
            size.0 as f64 / self.scale / 2.0,
            size.1 as f64 / self.scale / 2.0,
        );
        (
            (self.centre.0 - half.0, self.centre.1 - half.1),
            (self.centre.0 + half.0, self.centre.1 + half.1),
        )
    }

    /// The navigator's map for a thumbnail of `size` px: the sheet fitted.
    pub fn nav_map(&self, size: (f64, f64)) -> NavMap {
        let (lo, hi) = self.chain.paper_bounds();
        let scale = (size.0 / (hi.0 - lo.0)).min(size.1 / (hi.1 - lo.1));
        NavMap {
            origin: (
                (lo.0 + hi.0) / 2.0 - size.0 / scale / 2.0,
                (lo.1 + hi.1) / 2.0 - size.1 / scale / 2.0,
            ),
            scale,
        }
    }

    /// The paper point under a pixel of the redraw pane.
    pub fn paper_at(&self, px: (f64, f64)) -> Option<(f64, f64)> {
        let cam = self.redraw_camera()?;
        Some((
            cam.origin.0 + px.0 / cam.zoom,
            cam.origin.1 + px.1 / cam.zoom,
        ))
    }

    /// The crop pixel under a pixel of the scan pane.
    pub fn crop_at(&self, px: (f64, f64)) -> Option<(f64, f64)> {
        let cam = self.scan_camera()?;
        Some((
            cam.origin.0 + px.0 / cam.zoom,
            cam.origin.1 + px.1 / cam.zoom,
        ))
    }

    /// Set the view from the scan side: the crop pixel at the scan pane's
    /// centre and the scan zoom. The paper centre is that pixel's paper
    /// point, and the paper scale is chosen so the derived zoom is `zoom`
    /// there exactly.
    fn set_from_scan(&mut self, centre_px: (f64, f64), zoom: f64) {
        self.centre = self.chain.scan_to_paper(centre_px);
        // scan_zoom = scale / scan_per_mm, so scale = zoom * scan_per_mm.
        self.scale = zoom * self.chain.scan_per_mm(self.centre);
    }

    fn fit(&mut self) {
        let scan_first = matches!(self.mode, Mode::ScanOnly | Mode::Overlay);
        let size = if scan_first {
            self.pane_size(Pane::Scan)
        } else {
            self.pane_size(Pane::Redraw)
        }
        .unwrap_or(self.window);
        let (w, h) = (size.0.max(1) as f64, size.1.max(1) as f64);
        if scan_first {
            let e = self.chain.extent();
            let zoom = FIT_FILL * (w / e.0).min(h / e.1);
            self.set_from_scan((e.0 / 2.0, e.1 / 2.0), zoom);
        } else {
            let (lo, hi) = self.chain.paper_bounds();
            self.scale = FIT_FILL * (w / (hi.0 - lo.0)).min(h / (hi.1 - lo.1));
            self.centre = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
        }
    }
}

/// Apply one message. Pure and deterministic. Messages for a pane the mode
/// hides, and non-finite or non-positive zoom factors, change nothing.
pub fn update(m: &mut Model, msg: Msg) {
    match msg {
        Msg::Resize { width, height } => m.window = (width.max(1), height.max(1)),
        Msg::SetMode(mode) => {
            // The layouts' own modes: no side by side on a phone, no wipe
            // beside the panels.
            m.mode = match (m.layout, mode) {
                (Layout::Mobile, Mode::SideBySide) => Mode::RedrawOnly,
                (Layout::Desktop, Mode::Wipe) => Mode::SideBySide,
                (_, mode) => mode,
            };
            m.pointer = None;
            m.hovered_constraints.clear();
            m.hovered = None;
            m.highlight = Highlight::default();
        }
        Msg::Fit => m.fit(),
        Msg::CentreOn { at } => {
            if at.0.is_finite() && at.1.is_finite() {
                let (lo, hi) = m.chain.paper_bounds();
                m.centre = (at.0.clamp(lo.0, hi.0), at.1.clamp(lo.1, hi.1));
            }
        }
        Msg::SetOpacity(o) => {
            if o.is_finite() {
                m.opacity = o.clamp(0.0, 1.0);
            }
        }
        Msg::SetLayer { layer, visible } => {
            if visible {
                m.hidden.remove(&layer);
            } else {
                m.hidden.insert(layer);
            }
        }
        Msg::SetTheme(t) => m.theme = t,
        Msg::SetConstraintTrust { trust, visible } => {
            if visible {
                m.trust_hidden.remove(&trust);
            } else {
                m.trust_hidden.insert(trust);
            }
        }
        Msg::SetConstraintKind { kind, visible } => {
            if visible {
                m.kind_hidden.remove(&kind);
            } else {
                m.kind_hidden.insert(kind);
            }
        }
        Msg::Pin { head, highlight } => m.pinned = Some(Pin { head, highlight }),
        Msg::Unpin => {
            m.pinned = None;
            m.details_open = false;
        }
        Msg::SetLayout(layout) => {
            m.layout = layout;
            m.navigator = layout == Layout::Desktop;
            let mode = m.mode;
            update(m, Msg::SetMode(mode));
        }
        Msg::SetWipe(w) => {
            if w.is_finite() {
                m.wipe = w.clamp(WIPE_LIMITS.0, WIPE_LIMITS.1);
            }
        }
        Msg::SetSheet(sheet) => match sheet {
            Sheet::Closed | Sheet::Peek => m.details_open = false,
            Sheet::Half | Sheet::Full => {
                if m.pinned.is_some() {
                    m.details_open = true;
                    m.sheet_height = sheet;
                }
            }
        },
        Msg::SetMenu(open) => {
            m.menu_open = open;
            if open {
                m.about_open = false;
            }
        }
        Msg::SetAbout(open) => {
            m.about_open = open;
            if open {
                m.menu_open = false;
            }
        }
        Msg::SetNavigator(on) => m.navigator = on,
        Msg::SetConnections(on) => m.connections = on,
        Msg::SetConstraintsShown(on) => m.constraints_shown = on,
        Msg::SetDetails(open) => m.details_open = open && m.pinned.is_some(),
        Msg::SetConstraintsOnSelection(on) => m.only_on_selection = on,
        Msg::SetDimensionConstraints(on) => m.dimension_constraints = on,
        Msg::HoverConstraints {
            pane,
            at,
            items,
            highlight,
        } => {
            if m.pane_size(pane).is_none() || !(at.0.is_finite() && at.1.is_finite()) {
                return;
            }
            m.pointer = Some((pane, at));
            m.hovered = None;
            m.hovered_constraints = items;
            m.highlight = highlight;
        }
        Msg::Hover {
            pane,
            at,
            node,
            highlight,
        } => {
            if m.pane_size(pane).is_none() || !(at.0.is_finite() && at.1.is_finite()) {
                return;
            }
            m.pointer = Some((pane, at));
            m.hovered_constraints.clear();
            m.hovered = node;
            m.highlight = match (node, highlight) {
                (None, _) => Highlight::default(),
                (Some(n), None) => Highlight::piece(n),
                (Some(_), Some(h)) => h,
            };
        }
        Msg::Leave => {
            m.pointer = None;
            m.hovered_constraints.clear();
            m.hovered = None;
            m.highlight = Highlight::default();
        }
        Msg::Pan { pane, by } => {
            if m.pane_size(pane).is_none() || !(by.0.is_finite() && by.1.is_finite()) {
                return;
            }
            match pane {
                Pane::Redraw => {
                    m.centre = (m.centre.0 - by.0 / m.scale, m.centre.1 - by.1 / m.scale);
                }
                Pane::Scan => {
                    let c = m.scan_centre();
                    let z = m.scan_zoom();
                    m.set_from_scan((c.0 - by.0 / z, c.1 - by.1 / z), z);
                }
            }
        }
        Msg::Zoom {
            pane,
            factor,
            about,
        } => {
            let Some(size) = m.pane_size(pane) else {
                return;
            };
            if !(factor.is_finite() && factor > 0.0 && about.0.is_finite() && about.1.is_finite()) {
                return;
            }
            let half = (size.0 as f64 / 2.0, size.1 as f64 / 2.0);
            match pane {
                Pane::Redraw => {
                    let s = (m.scale * factor).clamp(SCALE_LIMITS.0, SCALE_LIMITS.1);
                    // The paper point under the cursor stays under it.
                    let anchor = (
                        m.centre.0 + (about.0 - half.0) / m.scale,
                        m.centre.1 + (about.1 - half.1) / m.scale,
                    );
                    m.centre = (
                        anchor.0 - (about.0 - half.0) / s,
                        anchor.1 - (about.1 - half.1) / s,
                    );
                    m.scale = s;
                }
                Pane::Scan => {
                    let (c, z) = (m.scan_centre(), m.scan_zoom());
                    let z2 = (z * factor).clamp(SCAN_ZOOM_LIMITS.0, SCAN_ZOOM_LIMITS.1);
                    // The crop pixel under the cursor stays under it.
                    let anchor = (c.0 + (about.0 - half.0) / z, c.1 + (about.1 - half.1) / z);
                    m.set_from_scan(
                        (
                            anchor.0 - (about.0 - half.0) / z2,
                            anchor.1 - (about.1 - half.1) / z2,
                        ),
                        z2,
                    );
                }
            }
        }
    }
}
