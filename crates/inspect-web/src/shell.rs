//! The shell around the inspector's pure core: it owns the GPU, the two
//! pane canvases, the bundle's loaded pieces and the tile fetches, turns
//! input into [`Msg`]s for [`update`], and draws what the model's cameras
//! say. The hover pick runs here (it reads loaded tiles) and rides into the
//! model in [`Msg::Hover`].

use crate::icons::{self, Connect, Emphasis};
use crate::net;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::Purpose;
use lmsheets_gpu::instances::{segment, warp, Instance};
use lmsheets_gpu::overlay::Overlay;
use lmsheets_gpu::raster::{OwnerLayer, OwnerLit, Pyramid, RasterLayer, ScanView, TileId};
use lmsheets_gpu::{Lit, Renderer, View};
use lmsheets_inspector::constraints::{cluster, pick_icon, singles, Cluster, ICON_PX};
use lmsheets_inspector::detail::{self, Detail};
use lmsheets_inspector::pick::{LIT_MIN_PX, OWNER_ZOOM, SERVING_MIN_PX};
use lmsheets_inspector::{
    update, Chain, ConstraintKind, ConstraintTrust, Constraints, Groups, Highlight, Layer, Mode,
    Model, Msg, OwnerTable, OwnerTile, Pane, Picker, PinHead, Role,
};
use lmsheets_render::conventions::Conventions;
use lmsheets_render::draw::draw;
use lmsheets_render::resolve::resolve;
use lmsheets_render::scene::Scene;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlCanvasElement;

/// Scan tile fetches in flight at once.
const MAX_IN_FLIGHT: usize = 8;
/// The same on a phone, and the most the scan tiles may hold (texture bytes),
/// and the device pixel ratio drawn at (a phone's 3 is more than the eye
/// gains and nine times the pixels of 1).
const MOBILE_IN_FLIGHT: usize = 4;
const MOBILE_TILE_BUDGET: u64 = 24 << 20;
const MOBILE_DPR_MAX: f64 = 2.0;
/// Scan tiles uploaded to the GPU in one frame.
const UPLOADS_PER_FRAME: usize = 4;
/// The longest piece the redraw is cut into when it is carried through the
/// warp for the overlay, paper mm.
const OVERLAY_STEP_MM: f64 = 0.5;
/// The navigator's largest size, CSS px.
const NAV_MAX: (f64, f64) = (210.0, 150.0);

/// A row of the constraints section: a class or a type, its colour (classes),
/// how many constraints it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterRow<T> {
    /// What it filters.
    pub key: T,
    /// The label.
    pub label: &'static str,
    /// How many constraints.
    pub count: usize,
    /// A CSS colour (the icon colour of a class; empty for a type).
    pub colour: String,
}

/// A row of the layers panel.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRow {
    /// The layer.
    pub layer: Layer,
    /// How many nodes it holds.
    pub count: usize,
    /// Its colour in the current look, a CSS colour.
    pub colour: String,
}

/// The navigator: the whole sheet in a corner, drawn once and again when
/// the look or the layers change.
struct Nav {
    surface: PaneSurface,
    renderer: Renderer,
    dirty: bool,
}

/// What the fetch tasks have finished, for the next frame to take.
#[derive(Default)]
struct Pending {
    scan: Vec<(TileId, (u32, u32), Vec<u8>)>,
    owners: Vec<((u32, u32), OwnerTile)>,
    scan_in_flight: BTreeSet<TileId>,
    owner_in_flight: BTreeSet<(u32, u32)>,
    errors: Vec<String>,
}

/// What building the overlay cost, for the status line.
#[derive(Clone, Copy, Debug)]
pub struct OverlayStats {
    /// Instances in the paper drawing.
    pub from: usize,
    /// Instances after the warp (pieces).
    pub to: usize,
    /// Milliseconds to warp.
    pub ms: f64,
}

/// The look and layers last put on the GPU.
#[derive(Clone, Debug, Default, PartialEq)]
struct Applied {
    theme: Option<lmsheets_inspector::Theme>,
    overlay: bool,
    hidden: BTreeSet<Layer>,
}

struct PaneSurface {
    canvas: HtmlCanvasElement,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// The inspector, loaded and on the GPU.
pub struct Shell {
    /// The pure model.
    pub model: Model,
    picker: Picker,
    groups: Groups,
    pyramid: Pyramid,
    base: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    redraw: PaneSurface,
    scan: PaneSurface,
    renderer: Renderer,
    /// The hovered group's geometry carried onto the scan (crop px), drawn
    /// where the owner map is out of reach.
    outline: Renderer,
    /// The redraw carried through the warp onto the scan (crop px), built
    /// when the overlay is first shown.
    overlay: Overlay,
    overlay_built: Option<OverlayStats>,
    nav: Nav,
    /// The constraint icons, as instances in the drawn pane's units.
    icons: Renderer,
    constraints: Constraints,
    scene: Scene,
    doc: Document,
    /// The redraw's colour of each styled item in the current look.
    colours: Vec<[f32; 4]>,
    /// What the GPU holds of the model's look and layers.
    applied: Applied,
    raster: RasterLayer,
    owners: OwnerLayer,
    owner_tiles: (u32, u32),
    pending: Rc<RefCell<Pending>>,
    /// The highlight the GPU holds, with the role it is lit in and whether it
    /// is the pin.
    lit: Highlight,
    lit_role: Role,
    lit_pin: bool,
    /// The nodes lit in the bound role besides the lit group (in the
    /// connections view): what the revealed constraints bind.
    lit_extra: Vec<lmsheets_inspector::NodeId>,
    /// The pinned group, lit in its own role besides a hover elsewhere.
    lit_pin_apart: Vec<lmsheets_inspector::NodeId>,
    /// The constraints the connections view shows for what is lit.
    reveal: Option<Vec<usize>>,
    /// A cluster the pointer opened: its members, shown apart, and where.
    expanded: Option<(Vec<usize>, (f64, f64))>,
    /// Whether Alt is held: hover takes the single piece.
    alt: bool,
    dirty: bool,
    /// Whether the phone's smaller budgets apply.
    mobile: bool,
    /// Whether the view is still the fit (a resize then refits).
    fitted: bool,
    dpr: f64,
    /// How the last hover was picked, for the status line.
    pub pick_source: &'static str,
    /// Errors from fetches and decodes, newest last.
    pub errors: Vec<String>,
}

fn acquire(p: &PaneSurface, device: &wgpu::Device) -> Result<wgpu::SurfaceTexture, String> {
    match p.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
            Ok(f)
        }
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            p.surface.configure(device, &p.config);
            match p.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f)
                | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Ok(f),
                other => Err(format!("no frame: {other:?}")),
            }
        }
        other => Err(format!("no frame: {other:?}")),
    }
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

impl Shell {
    /// Load the bundle under `base` (a URL ending in `/`), open WebGPU on
    /// the two canvases and fit the sheet in a window of `css` CSS px.
    pub async fn open(
        base: String,
        redraw: HtmlCanvasElement,
        scan: HtmlCanvasElement,
        nav: HtmlCanvasElement,
        css: (f64, f64),
        mobile: bool,
    ) -> Result<Self, String> {
        let url = |name: &str| format!("{base}{name}");
        let manifest = net::text(&url("manifest.json")).await?;
        let document = net::text(&url("document.json")).await?;
        let registration = net::bytes(&url("registration.bin")).await?;
        let owners_json = net::text(&url("owners/owners.json")).await?;
        let chain =
            Chain::from_bundle(&manifest, &document, &registration).map_err(|e| e.to_string())?;
        let pyramid = Pyramid::from_manifest(&manifest).map_err(|e| e.to_string())?;
        let doc = Document::parse(&document).map_err(|e| format!("document.json: {e:?}"))?;
        let scene = Scene::of(draw(&doc, Purpose::Screen));
        let theme = lmsheets_inspector::Theme::default();
        let styled = resolve(&scene, &doc, &theme.render(), Conventions::default());
        let table = OwnerTable::from_json(&owners_json)?;
        let groups = Groups::new(&doc, &scene.planes[0], &table);
        let constraints = Constraints::new(&doc, &scene.planes[0], &table);
        let picker = Picker::new(table, pyramid.tile, &scene.planes[0], &doc, &styled);

        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(desc);
        for c in [&redraw, &scan, &nav] {
            c.set_width(c.width().max(1));
            c.set_height(c.height().max(1));
        }
        let surface_of = |c: &HtmlCanvasElement| {
            instance
                .create_surface(wgpu::SurfaceTarget::Canvas(c.clone()))
                .map_err(|e| format!("WebGPU: no surface: {e}"))
        };
        let (s0, s1, s2) = (surface_of(&redraw)?, surface_of(&scan)?, surface_of(&nav)?);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&s0),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("WebGPU: not available in this browser: {e}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| format!("WebGPU: no device: {e}"))?;
        let caps = s0.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = |c: &HtmlCanvasElement| wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: c.width(),
            height: c.height(),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: Vec::new(),
            color_space: Default::default(),
            desired_maximum_frame_latency: 2,
        };
        let (c0, c1, c2) = (config(&redraw), config(&scan), config(&nav));
        s0.configure(&device, &c0);
        s1.configure(&device, &c1);
        s2.configure(&device, &c2);
        let mut renderer = Renderer::new(&device, format);
        renderer.set_scene(&device, &queue, &styled);
        let mut nav_renderer = Renderer::new(&device, format);
        nav_renderer.set_scene(&device, &queue, &styled);
        let outline = Renderer::new(&device, format);
        let overlay = Overlay::new(&device, format);
        let icons = Renderer::new(&device, format);
        let raster = RasterLayer::new(&device, format);
        let owners = OwnerLayer::new(&device, format);
        let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
        let dpr = if mobile { dpr.min(MOBILE_DPR_MAX) } else { dpr };
        let window = ((css.0 * dpr).max(1.0) as u32, (css.1 * dpr).max(1.0) as u32);
        let owner_tiles = pyramid.levels[0].tiles;
        let mut shell = Self {
            model: Model::new(chain, window),
            picker,
            groups,
            pyramid,
            base,
            device,
            queue,
            redraw: PaneSurface {
                canvas: redraw,
                surface: s0,
                config: c0,
            },
            scan: PaneSurface {
                canvas: scan,
                surface: s1,
                config: c1,
            },
            renderer,
            outline,
            overlay,
            overlay_built: None,
            icons,
            constraints,
            nav: Nav {
                surface: PaneSurface {
                    canvas: nav,
                    surface: s2,
                    config: c2,
                },
                renderer: nav_renderer,
                dirty: true,
            },
            scene,
            doc,
            colours: Vec::new(),
            applied: Applied::default(),
            raster,
            owners,
            owner_tiles,
            pending: Rc::default(),
            lit: Highlight::default(),
            lit_role: Role::Entity,
            lit_pin: false,
            lit_extra: Vec::new(),
            lit_pin_apart: Vec::new(),
            reveal: None,
            expanded: None,
            alt: false,
            dirty: true,
            mobile,
            fitted: true,
            dpr,
            pick_source: "",
            errors: Vec::new(),
        };
        shell.layout();
        shell.sync_state();
        Ok(shell)
    }

    fn max_in_flight(&self) -> usize {
        if self.mobile {
            MOBILE_IN_FLIGHT
        } else {
            MAX_IN_FLIGHT
        }
    }

    /// The device pixel ratio.
    pub fn dpr(&self) -> f64 {
        self.dpr
    }

    /// Apply a message, then re-pick under a still pointer (the content may
    /// have moved beneath it).
    pub fn send(&mut self, msg: Msg) {
        let moves = !matches!(msg, Msg::Hover { .. } | Msg::Leave);
        // The view stays the fitted one until the user moves it.
        match msg {
            Msg::Fit => self.fitted = true,
            Msg::Pan { .. } | Msg::Zoom { .. } | Msg::CentreOn { .. } => self.fitted = false,
            _ => {}
        }
        let relayout = matches!(msg, Msg::SetWipe(_) | Msg::SetLayout(_));
        update(&mut self.model, msg);
        self.dirty = true;
        if relayout {
            self.layout();
        }
        self.refresh_reveal();
        if moves {
            if let Some((pane, at)) = self.model.pointer() {
                self.hover(pane, at);
            }
        }
    }

    /// The pointer is at `at` (pane device px).
    pub fn hover(&mut self, pane: Pane, at: (f64, f64)) {
        // An icon is over the drawing: it wins the pointer.
        if let Some(c) = self.icon_at(pane, at) {
            let items = c.members.clone();
            let mut binds: Vec<_> = items
                .iter()
                .flat_map(|&i| self.constraints.items()[i].binds.clone())
                .filter(|n| self.picker.node_visible(&self.model, *n))
                .collect();
            binds.sort();
            binds.dedup();
            let before = (
                self.model.lit().clone(),
                self.model.hovered_constraints().to_vec(),
            );
            update(
                &mut self.model,
                Msg::HoverConstraints {
                    pane,
                    at,
                    items,
                    highlight: Highlight {
                        members: binds,
                        serving: Vec::new(),
                    },
                },
            );
            self.pick_source = "icon";
            self.refresh_reveal();
            // A merged icon opens to its members, which stay open while the
            // pointer is on them or near.
            if c.members.len() > 1 {
                self.expanded = Some((c.members.clone(), c.at));
            } else if !self
                .expanded
                .as_ref()
                .is_some_and(|(m, _)| m.contains(&c.members[0]))
            {
                self.expanded = None;
            }
            let after = (
                self.model.lit().clone(),
                self.model.hovered_constraints().to_vec(),
            );
            if before != after {
                self.dirty = true;
            }
            return;
        }
        // Off the icons: an opened cluster closes once the pointer is away.
        if let Some((_, anchor)) = &self.expanded {
            let far = self
                .icon_px(pane, *anchor)
                .is_none_or(|p| (p.0 - at.0).hypot(p.1 - at.1) > 4.0 * ICON_PX * self.dpr);
            if far {
                self.expanded = None;
            }
        }
        let node = self.picker.pick(&self.model, pane, at);
        self.pick_source = match (pane, self.model.scan_camera()) {
            (Pane::Scan, Some(c))
                if c.zoom >= OWNER_ZOOM
                    && self
                        .model
                        .crop_at(at)
                        .and_then(|p| self.picker.owners.value_at(p.0 as i64, p.1 as i64))
                        .is_some() =>
            {
                "owner map"
            }
            _ => "geometry",
        };
        // The group (or, with Alt, the piece) is the shell's decision.
        // Hidden layers are not lit.
        let highlight = node.map(|n| {
            self.groups
                .highlight(n, self.alt)
                .retaining(|x| self.picker.node_visible(&self.model, x))
        });
        let before = self.model.lit().clone();
        let had_icon = !self.model.hovered_constraints().is_empty();
        update(
            &mut self.model,
            Msg::Hover {
                pane,
                at,
                node,
                highlight,
            },
        );
        self.refresh_reveal();
        if before != *self.model.lit() || had_icon {
            self.dirty = true;
        }
    }

    /// The connections view: the constraints that bind the lit group (the
    /// constraint itself for a pinned constraint) shown one icon each even
    /// with the icons off. It is a pin's own view, and a hover's only with
    /// Shift held; it is kept while the pointer is on one of its icons.
    fn refresh_reveal(&mut self) {
        let new = self
            .constraints
            .connections(&self.model, self.reveal.as_deref());
        if new != self.reveal {
            self.reveal = new;
            self.dirty = true;
        }
    }

    /// The pinned group when something else is hovered: it stays lit in
    /// its own role. None when the hover is the pin (or there is none).
    fn pin_apart(&self) -> Vec<lmsheets_inspector::NodeId> {
        match self.model.pinned() {
            Some(p) if self.model.lit() != &p.highlight => p
                .highlight
                .members
                .iter()
                .copied()
                .filter(|n| self.picker.node_visible(&self.model, *n))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The pinned item's role colour.
    fn pin_colour(&self) -> [f32; 3] {
        let role = match self.model.pinned().map(|p| p.head) {
            Some(PinHead::Node(n)) => self.group_role(n),
            _ => Role::Bound,
        };
        self.model.theme().role(role)
    }

    /// What the revealed constraints bind beyond the lit group.
    fn extra_bound(&self) -> Vec<lmsheets_inspector::NodeId> {
        let Some(r) = self
            .reveal
            .as_ref()
            .filter(|_| self.model.hovered_constraints().is_empty())
        else {
            return Vec::new();
        };
        let lit = self.model.lit();
        let pin = self.model.pinned().map(|p| &p.highlight);
        let mut out: Vec<_> = r
            .iter()
            .flat_map(|&i| self.constraints.items()[i].binds.clone())
            .chain(
                self.constraints
                    .links_of(&self.connection_nodes())
                    .flat_map(|l| l.to.clone()),
            )
            .filter(|n| {
                !lit.contains(*n)
                    && !pin.is_some_and(|p| p.contains(*n))
                    && self.picker.node_visible(&self.model, *n)
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The icons the model shows, clustered for the current scale.
    fn clusters(&self) -> Vec<Cluster> {
        let scale = self.model.scale();
        let items = self.constraints.items();
        // The connections view: one icon each, spread apart.
        if let Some(r) = &self.reveal {
            let shown: Vec<usize> = r
                .iter()
                .copied()
                .filter(|&i| {
                    items[i].binds.is_empty()
                        || items[i]
                            .binds
                            .iter()
                            .any(|b| self.picker.node_visible(&self.model, *b))
                })
                .collect();
            return singles(items, &shown, scale);
        }
        let shown = self.constraints.visible(&self.model, self.picker.layers());
        let mut out = cluster(items, &shown, scale);
        if let Some((members, _)) = &self.expanded {
            if let Some(k) = out.iter().position(|c| &c.members == members) {
                out.remove(k);
                out.extend(singles(items, members, scale));
            }
        }
        out
    }

    /// Whether `pane` carries icons in this mode: the redraw pane, and the
    /// scan pane under the overlay.
    fn icons_on(&self, pane: Pane) -> bool {
        match pane {
            Pane::Redraw => true,
            Pane::Scan => self.model.mode() == Mode::Overlay,
        }
    }

    /// The pane pixel of a paper point, for icons: through the chain under
    /// the overlay.
    fn icon_px(&self, pane: Pane, p: (f64, f64)) -> Option<(f64, f64)> {
        match pane {
            Pane::Redraw => {
                let cam = self.model.redraw_camera()?;
                Some((
                    (p.0 - cam.origin.0) * cam.zoom,
                    (p.1 - cam.origin.1) * cam.zoom,
                ))
            }
            Pane::Scan => {
                let cam = self.model.scan_camera()?;
                let c = self.model.chain().paper_to_scan(p);
                Some((
                    (c.0 - cam.origin.0) * cam.zoom,
                    (c.1 - cam.origin.1) * cam.zoom,
                ))
            }
        }
    }

    /// The icon cluster under a pane pixel.
    fn icon_at(&self, pane: Pane, at: (f64, f64)) -> Option<Cluster> {
        if !self.icons_on(pane) || self.model.pane_size(pane).is_none() {
            return None;
        }
        let clusters = self.clusters();
        let i = pick_icon(
            &clusters,
            |p| self.icon_px(pane, p).unwrap_or((f64::MAX, f64::MAX)),
            at,
            self.dpr,
        )?;
        clusters.into_iter().nth(i)
    }

    /// Draw the icons into `target` over `pane`'s drawing, under `view`.
    fn draw_icons(
        &mut self,
        pane: Pane,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: &View,
    ) {
        let instances = self.icon_instances(pane, view.zoom);
        self.icons
            .set_instances(&self.device, &self.queue, &instances);
        self.icons.render_over(&self.queue, encoder, target, view);
    }

    /// The icons as instances for `pane`, in its drawing's units, `zoom`
    /// device px per unit.
    fn icon_instances(&self, pane: Pane, zoom: f64) -> Vec<lmsheets_gpu::instances::Instance> {
        let clusters = self.clusters();
        let lit = self.model.lit();
        let hovered = self.model.hovered_constraints();
        // The icon under the pointer, or the pinned constraint's, is
        // emphasised and draws its relation; with a group lit, the icons of
        // the constraints that bind it directly (a centre line counts as its
        // centres) are emphasised and the rest dimmed.
        let pinned = match self.model.pinned().map(|p| p.head) {
            Some(PinHead::Constraint(i)) if hovered.is_empty() => Some(i),
            _ => None,
        };
        let direct = if hovered.is_empty() && pinned.is_none() && !lit.is_empty() {
            Some(self.constraints.revealed(&lit.members))
        } else {
            None
        };
        let emphasis = |c: &Cluster| {
            if c.members
                .iter()
                .any(|m| hovered.contains(m) || pinned == Some(*m))
            {
                Emphasis::Hovered
            } else if let Some(d) = &direct {
                if c.members.iter().any(|m| d.contains(m)) {
                    Emphasis::Related
                } else {
                    Emphasis::Dim
                }
            } else {
                Emphasis::Plain
            }
        };
        let theme = self.model.theme();
        let mut out = match pane {
            Pane::Redraw => icons::build(
                &clusters,
                Connect {
                    items: self.constraints.items(),
                    all: self.reveal.is_some(),
                },
                &|p| p,
                zoom,
                self.dpr,
                theme,
                &emphasis,
            ),
            Pane::Scan => {
                let chain = self.model.chain();
                icons::build(
                    &clusters,
                    Connect {
                        items: self.constraints.items(),
                        all: self.reveal.is_some(),
                    },
                    &|p| chain.paper_to_scan(p),
                    zoom,
                    self.dpr,
                    theme,
                    &emphasis,
                )
            }
        };
        // What the pin or the connections view relates to without a
        // constraint, and a ring round a tiny lit item.
        let to_unit = |p: (f64, f64)| match pane {
            Pane::Redraw => p,
            Pane::Scan => self.model.chain().paper_to_scan(p),
        };
        if self.reveal.is_some() {
            let from = self.connection_nodes();
            out.extend(icons::link_connectors(
                self.constraints.links_of(&from),
                &to_unit,
                zoom,
                self.dpr,
                theme.role(Role::Bound),
            ));
        }
        let lit = self.model.lit().members.clone();
        for (nodes, colour) in [
            (lit, self.colour()),
            (self.lit_pin_apart.clone(), self.pin_colour()),
        ] {
            if let Some((centre, extent)) = self.lit_extent(&nodes) {
                out.extend(icons::halo(
                    to_unit(centre),
                    extent / self.dpr,
                    zoom,
                    self.dpr,
                    colour,
                ));
            }
        }
        out
    }

    /// The nodes whose connections are shown: the pin's group, else the
    /// lit one.
    fn connection_nodes(&self) -> Vec<lmsheets_inspector::NodeId> {
        match self.model.pinned() {
            Some(p) => p.highlight.members.clone(),
            None => self.model.lit().members.clone(),
        }
    }

    /// The paper centre and the screen extent (device px) of a group of nodes,
    /// from its items' ink bounds.
    fn lit_extent(&self, nodes: &[lmsheets_inspector::NodeId]) -> Option<((f64, f64), f64)> {
        let items = self.picker.items_in(nodes);
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for i in items {
            let Some(b) = self.picker.items()[i].primitive.ink_bounds() else {
                continue;
            };
            lo = (lo.0.min(b.min().x), lo.1.min(b.min().y));
            hi = (hi.0.max(b.max().x), hi.1.max(b.max().y));
        }
        (lo.0 <= hi.0).then(|| {
            let extent = (hi.0 - lo.0).max(hi.1 - lo.1) * self.model.scale();
            (((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0), extent)
        })
    }

    /// The trust classes of the constraints section, with counts and the
    /// icon colour of the current look.
    pub fn trust_rows(&self) -> Vec<FilterRow<ConstraintTrust>> {
        let theme = self.model.theme();
        ConstraintTrust::ALL
            .into_iter()
            .map(|t| {
                let c = theme.constraint(t);
                let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                FilterRow {
                    key: t,
                    label: t.label(),
                    count: self.constraints.count_trust(t),
                    colour: format!("rgb({} {} {})", b(c[0]), b(c[1]), b(c[2])),
                }
            })
            .collect()
    }

    /// The types of the constraints section, with counts (those with none
    /// are left out).
    pub fn kind_rows(&self) -> Vec<FilterRow<ConstraintKind>> {
        ConstraintKind::ALL
            .into_iter()
            .map(|k| FilterRow {
                key: k,
                label: k.label(),
                count: self.constraints.count_kind(k),
                colour: String::new(),
            })
            .filter(|r| r.count > 0)
            .collect()
    }

    /// How many constraints are drawn as icons now.
    pub fn icons_shown(&self) -> usize {
        self.constraints
            .visible(&self.model, self.picker.layers())
            .len()
    }

    /// A click on a pane at `at`: pin what is under the pointer (an icon, or
    /// the group), or let go of the pin over empty paper.
    pub fn click(&mut self, pane: Pane, at: (f64, f64)) {
        self.hover(pane, at);
        let lit = self.model.lit().clone();
        if let Some(&c) = self.model.hovered_constraints().first() {
            self.send(Msg::Pin {
                head: PinHead::Constraint(c),
                highlight: lit,
            });
        } else if let Some(n) = self.model.hovered() {
            self.send(Msg::Pin {
                head: PinHead::Node(n),
                highlight: lit,
            });
        } else {
            self.send(Msg::Unpin);
        }
    }

    /// A touch on a pane at `at`: pin what is under the finger (or let go of
    /// the pin over empty paper), as a click does; `piece` takes the single
    /// piece (a long press). The touch leaves no hover behind, only the pin.
    pub fn touch(&mut self, pane: Pane, at: (f64, f64), piece: bool) {
        let alt = self.alt;
        self.set_alt(piece);
        self.click(pane, at);
        self.set_alt(alt);
        self.leave();
    }

    /// Pin an item by its link (a row of the panel, or `?pin=`): a node's
    /// group stays lit, a constraint's bound nodes.
    pub fn pin(&mut self, head: PinHead) {
        let highlight = match head {
            PinHead::Node(n) => self.groups.highlight(n, false),
            PinHead::Constraint(i) => {
                let binds = self
                    .constraints
                    .items()
                    .get(i)
                    .map_or(Vec::new(), |c| c.binds.clone());
                Highlight {
                    members: binds,
                    serving: Vec::new(),
                }
            }
        };
        let highlight = highlight.retaining(|n| self.picker.node_visible(&self.model, n));
        self.send(Msg::Pin { head, highlight });
    }

    /// Pin the node `kind` `name` of the owner table, if it has it.
    pub fn pin_key(&mut self, kind: &str, name: &str) -> bool {
        match self.picker.table.id(kind, name) {
            Some(n) => {
                self.pin(PinHead::Node(n));
                true
            }
            None => false,
        }
    }

    /// The panel of the pinned item.
    pub fn detail(&self) -> Option<Detail> {
        let head = self.model.pinned()?.head;
        let chain = self.model.chain();
        let scale = |p: (f64, f64)| chain.scan_per_mm(p);
        let adapter = self.doc.residuals().map(detail::DocResiduals);
        Some(detail::detail(
            &detail::Source {
                doc: &self.doc,
                plane: &self.scene.planes[0],
                table: &self.picker.table,
                constraints: &self.constraints,
                layers: self.picker.layers(),
                groups: &self.groups,
                scan_per_mm: &scale,
                residuals: adapter.as_ref().map(|a| a as &dyn detail::Residuals),
            },
            head,
        ))
    }

    /// Alt went down or up: hover takes the single piece while it is down.
    pub fn set_alt(&mut self, alt: bool) {
        if self.alt != alt {
            self.alt = alt;
            if let Some((pane, at)) = self.model.pointer() {
                self.hover(pane, at);
            }
        }
    }

    /// Shift went down or up: the connections view of what is hovered or
    /// pinned.
    pub fn set_shift(&mut self, shift: bool) {
        if self.model.connections() != shift {
            self.send(Msg::SetConnections(shift));
        }
    }

    /// The pointer left a pane.
    pub fn leave(&mut self) {
        self.send(Msg::Leave);
    }

    /// Resize to a stage of `css` CSS px.
    pub fn resize(&mut self, css: (f64, f64)) {
        let w = (
            (css.0 * self.dpr).max(1.0) as u32,
            (css.1 * self.dpr).max(1.0) as u32,
        );
        self.send(Msg::Resize {
            width: w.0,
            height: w.1,
        });
        // A fitted view stays fitted to the new pane.
        if self.fitted {
            self.send(Msg::Fit);
        }
        self.layout();
    }

    /// Choose the layout (the phone's has one pane at a time), refitting.
    pub fn set_layout(&mut self, layout: lmsheets_inspector::Layout) {
        self.mobile = layout == lmsheets_inspector::Layout::Mobile;
        self.send(Msg::SetLayout(layout));
        self.send(Msg::Fit);
        self.layout();
    }

    /// Show other panes.
    pub fn set_mode(&mut self, mode: Mode) {
        self.send(Msg::SetMode(mode));
        self.send(Msg::Fit);
        self.layout();
    }

    /// Size each canvas to its pane (and hide the hidden ones).
    pub fn layout_all(&mut self) {
        self.layout();
    }

    fn layout(&mut self) {
        for (pane, p) in [
            (Pane::Redraw, &mut self.redraw),
            (Pane::Scan, &mut self.scan),
        ] {
            let hidden = self.model.pane_size(pane).is_none();
            let _ = p.canvas.class_list().toggle_with_force("hidden", hidden);
            let Some((w, h)) = self.model.pane_size(pane) else {
                continue;
            };
            let style = p.canvas.style();
            // In the wipe the redraw (on top) shows left of the divider only.
            let clip = if pane == Pane::Redraw && self.model.mode() == Mode::Wipe {
                format!("inset(0 {:.3}% 0 0)", 100.0 * (1.0 - self.model.wipe()))
            } else {
                "none".into()
            };
            let _ = style.set_property("clip-path", &clip);
            let _ = style.set_property("width", &format!("{}px", w as f64 / self.dpr));
            let _ = style.set_property("height", &format!("{}px", h as f64 / self.dpr));
            if (p.config.width, p.config.height) != (w, h) {
                p.canvas.set_width(w);
                p.canvas.set_height(h);
                p.config.width = w;
                p.config.height = h;
                p.surface.configure(&self.device, &p.config);
            }
        }
        // The navigator is the sheet's shape, as large as fits.
        let (lo, hi) = self.model.chain().paper_bounds();
        let k = (NAV_MAX.0 / (hi.0 - lo.0)).min(NAV_MAX.1 / (hi.1 - lo.1));
        let css = ((hi.0 - lo.0) * k, (hi.1 - lo.1) * k);
        let dev = (
            (css.0 * self.dpr).round().max(1.0) as u32,
            (css.1 * self.dpr).round().max(1.0) as u32,
        );
        let n = &mut self.nav.surface;
        let style = n.canvas.style();
        let _ = style.set_property("width", &format!("{}px", css.0));
        let _ = style.set_property("height", &format!("{}px", css.1));
        if (n.config.width, n.config.height) != dev {
            n.canvas.set_width(dev.0);
            n.canvas.set_height(dev.1);
            n.config.width = dev.0;
            n.config.height = dev.1;
            n.surface.configure(&self.device, &n.config);
            self.nav.dirty = true;
        }
        self.dirty = true;
    }

    /// The box of the view in the navigator, CSS px: left, top, width,
    /// height, held within the thumbnail (a view larger than the sheet
    /// shows as the whole thumbnail).
    pub fn nav_box(&self) -> (f64, f64, f64, f64) {
        let map = self.nav_map();
        let (lo, hi) = self.model.view_box();
        let (a, b) = (map.to_px(lo), map.to_px(hi));
        // Not clamped: the navigator clips its box, so a side beyond the
        // thumbnail is not drawn and every stroke shown is a real edge of
        // the view.
        let (x0, y0) = (a.0 / self.dpr, a.1 / self.dpr);
        let (x1, y1) = (b.0 / self.dpr, b.1 / self.dpr);
        (x0, y0, (x1 - x0 - 3.0).max(2.0), (y1 - y0 - 3.0).max(2.0))
    }

    fn nav_map(&self) -> lmsheets_inspector::NavMap {
        let c = &self.nav.surface.config;
        self.model
            .nav_map((f64::from(c.width), f64::from(c.height)))
    }

    /// The pointer is at `css` (navigator CSS px) while pressed: centre the
    /// view there.
    pub fn nav_pointer(&mut self, css: (f64, f64)) {
        let at = self
            .nav_map()
            .to_paper((css.0 * self.dpr, css.1 * self.dpr));
        self.send(Msg::CentreOn { at });
    }

    /// The legend: each role with its colour in the current look.
    pub fn role_rows(&self) -> Vec<(Role, String)> {
        let theme = self.model.theme();
        Role::ALL
            .into_iter()
            .map(|r| {
                let c = theme.role(r);
                let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                (r, format!("rgb({} {} {})", b(c[0]), b(c[1]), b(c[2])))
            })
            .collect()
    }

    /// The layers panel's rows in the current look.
    pub fn layer_rows(&self) -> Vec<LayerRow> {
        let css = |c: [f32; 4]| {
            let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            format!("rgb({} {} {})", b(c[0]), b(c[1]), b(c[2]))
        };
        let layers = self.picker.layers();
        Layer::ALL
            .into_iter()
            .map(|layer| LayerRow {
                layer,
                count: layers.count(layer),
                // The role colour of the layer: the same on the hover, the
                // pin, the scan's tint and the legend.
                colour: {
                    let c = self.model.theme().role(Role::of_layer(layer));
                    css([c[0], c[1], c[2], 1.0])
                },
            })
            .collect()
    }

    /// Put the model's look, mode and layers on the GPU where they differ
    /// from what it holds.
    pub fn sync_state(&mut self) {
        let want = Applied {
            theme: Some(self.model.theme()),
            overlay: self.model.mode() == Mode::Overlay,
            hidden: self.model.hidden_layers().clone(),
        };
        if want == self.applied {
            return;
        }
        let queue = &self.queue;
        let theme = self.model.theme();
        if self.applied.theme != want.theme || self.colours.is_empty() {
            // The redraw in the look's colours: no re-resolve (the dashes do
            // not change), only each item's colour.
            let rt = theme.render();
            let to = |c: lmsheets_document::value::style::Rgba| {
                [c.r as f32, c.g as f32, c.b as f32, c.a as f32]
            };
            self.colours = self
                .scene
                .items()
                .map(|i| to(rt.colour(&self.doc, i.tint)))
                .collect();
            let cols = &self.colours;
            self.renderer.set_paper(rt.paper);
            self.renderer.set_colours(queue, |i| cols[i]);
            self.nav.renderer.set_paper(rt.paper);
            self.nav.renderer.set_colours(queue, |i| cols[i]);
            self.nav.dirty = true;
            // The highlight's colours are the look's.
            self.lit = Highlight::default();
        }
        if self.applied.theme != want.theme || self.applied.overlay != want.overlay {
            let (ink, paper) = theme.scan_palette(want.overlay);
            self.raster.ink = ink;
            self.raster.paper = paper;
            self.raster.background = paper;
            let layers = self.picker.layers();
            let accent = |inferred: bool| {
                let c = theme.overlay(inferred);
                [c[0], c[1], c[2], 1.0]
            };
            self.overlay.strokes.set_colours(queue, |i| {
                accent(layers.of_item(i) == Some(Layer::Inferred))
            });
        }
        if self.applied.hidden != want.hidden {
            let hidden: Vec<usize> = want
                .hidden
                .iter()
                .flat_map(|l| self.picker.layers().items_in(*l))
                .collect();
            self.renderer.set_hidden(queue, &hidden);
            self.nav.renderer.set_hidden(queue, &hidden);
            self.overlay.strokes.set_hidden(queue, &hidden);
            self.nav.dirty = true;
        }
        self.applied = want;
        self.dirty = true;
    }

    /// Carry the redraw through the warp, the first time the overlay is
    /// shown (the chain does not change, so once).
    fn ensure_overlay(&mut self) {
        if self.overlay_built.is_some() {
            return;
        }
        let t0 = now();
        let chain = self.model.chain();
        let (instances, owners) = warp(
            self.renderer.base(),
            self.renderer.owners(),
            &|p| chain.paper_to_scan(p),
            OVERLAY_STEP_MM,
        );
        let to = instances.len();
        self.overlay
            .strokes
            .set_owned(&self.device, &self.queue, instances, owners);
        self.overlay_built = Some(OverlayStats {
            from: self.renderer.base().len(),
            to,
            ms: now() - t0,
        });
        // Colours and hidden layers apply to the new instances too.
        self.applied = Applied::default();
        self.sync_state();
    }

    /// What building the overlay cost, once it is built.
    pub fn overlay_stats(&self) -> Option<OverlayStats> {
        self.overlay_built
    }

    /// A one-line description of the hovered node.
    pub fn hover_text(&self) -> String {
        if let Some(&first) = self.model.hovered_constraints().first() {
            let items = self.constraints.items();
            let c = &items[first];
            let all = self.model.hovered_constraints();
            let level = c
                .level
                .as_deref()
                .map_or(String::new(), |l| format!(" ({l})"));
            if all.len() > 1 {
                // A merged icon: list what it stands for; hover opens it.
                let list: Vec<String> = all
                    .iter()
                    .take(4)
                    .map(|&i| format!("{} {}", items[i].kind.label(), items[i].id))
                    .collect();
                let more = all.len().saturating_sub(4);
                return format!(
                    "{} constraints here: {}{}",
                    all.len(),
                    list.join("; "),
                    if more > 0 {
                        format!("; +{more} more")
                    } else {
                        String::new()
                    }
                );
            }
            let detail: String = c.detail.chars().take(90).collect();
            let residual = self
                .doc
                .residuals()
                .map(detail::DocResiduals)
                .and_then(|r| detail::Residuals::constraint(&r, c.view.as_deref(), &c.id))
                .map_or(String::new(), |r| {
                    format!("  ink residual {:.2} px, pull {:.2}", r.ink_px, r.pull)
                });
            return format!(
                "{} {}  {}{}{}  {}",
                c.relation,
                c.id,
                c.trust.label(),
                level,
                residual,
                detail
            );
        }
        match self.model.hovered() {
            Some(n) => {
                let more = self.model.lit().members.len().saturating_sub(1);
                let group = if more > 0 {
                    format!("  +{more} in group")
                } else {
                    String::new()
                };
                match self.picker.table.get(n) {
                    Some((kind, name)) => format!("{kind}  {name}  (node {}){group}", n.0),
                    None => format!("node {}", n.0),
                }
            }
            None => "none".into(),
        }
    }

    /// The scan camera's zoom, for the status line.
    pub fn scan_zoom(&self) -> Option<f64> {
        self.model.scan_camera().map(|c| c.zoom)
    }

    /// How many scan and owner tiles are held.
    pub fn held(&self) -> (usize, usize) {
        (self.raster.held().0, self.owners.len())
    }

    fn request_tiles(&mut self) {
        let Some(cam) = self.model.scan_camera() else {
            return;
        };
        let view = ScanView {
            origin: cam.origin,
            zoom: cam.zoom,
            size: cam.size,
        };
        for id in self.raster.missing(&self.pyramid, &view) {
            let mut p = self.pending.borrow_mut();
            if p.scan_in_flight.len() >= self.max_in_flight() || !p.scan_in_flight.insert(id) {
                continue;
            }
            drop(p);
            let (url, pending) = (
                format!("{}{}", self.base, self.pyramid.tile_path(id)),
                self.pending.clone(),
            );
            spawn_local(async move {
                match net::grey_tile(&url).await {
                    Ok((size, grey)) => pending.borrow_mut().scan.push((id, size, grey)),
                    Err(e) => pending.borrow_mut().errors.push(e),
                }
            });
        }
        if cam.zoom >= OWNER_ZOOM {
            for (x, y) in self
                .owners
                .missing(self.pyramid.tile, self.owner_tiles, &view)
            {
                let mut p = self.pending.borrow_mut();
                if p.owner_in_flight.len() >= self.max_in_flight()
                    || !p.owner_in_flight.insert((x, y))
                {
                    continue;
                }
                drop(p);
                let (url, pending) = (
                    format!("{}owners/{x}_{y}.png", self.base),
                    self.pending.clone(),
                );
                spawn_local(async move {
                    let tile = net::bytes(&url)
                        .await
                        .and_then(|b| OwnerTile::from_png(&b).map_err(|e| format!("{url}: {e}")));
                    match tile {
                        Ok(t) => pending.borrow_mut().owners.push(((x, y), t)),
                        Err(e) => pending.borrow_mut().errors.push(e),
                    }
                });
            }
        }
    }

    /// On a phone: keep the scan tiles within the budget, never the ones the
    /// view shows nor the coarsest level (the overview).
    fn evict(&mut self) {
        let Some(cam) = self.model.scan_camera() else {
            return;
        };
        let view = ScanView {
            origin: cam.origin,
            zoom: cam.zoom,
            size: cam.size,
        };
        let level = self.pyramid.pick_level(view.zoom);
        let mut keep = self.pyramid.visible_tiles(level, &view);
        let coarsest = self.pyramid.levels.len() - 1;
        keep.extend(self.pyramid.visible_tiles(
            coarsest,
            &ScanView {
                origin: (0.0, 0.0),
                zoom: 1.0,
                size: (1 << 20, 1 << 20),
            },
        ));
        self.raster.evict_to(MOBILE_TILE_BUDGET, &keep);
    }

    /// Take what the fetches finished: upload it, drop it from flight.
    fn drain(&mut self) {
        let (scan, owners, errors) = {
            let mut p = self.pending.borrow_mut();
            let n = p.scan.len().min(UPLOADS_PER_FRAME);
            let scan: Vec<_> = p.scan.drain(..n).collect();
            for (id, ..) in &scan {
                p.scan_in_flight.remove(id);
            }
            let owners: Vec<_> = p.owners.drain(..).collect();
            for (at, _) in &owners {
                p.owner_in_flight.remove(at);
            }
            (scan, owners, std::mem::take(&mut p.errors))
        };
        self.errors.extend(errors);
        if !scan.is_empty() || !owners.is_empty() {
            self.dirty = true;
        }
        let uploaded = !scan.is_empty();
        for (id, size, grey) in scan {
            self.raster
                .insert(&self.device, &self.queue, &self.pyramid, id, size, &grey);
        }
        if uploaded && self.mobile {
            self.evict();
        }
        for ((x, y), t) in owners {
            self.owners.insert(
                &self.device,
                &self.queue,
                self.pyramid.tile,
                (x, y),
                (t.width, t.height),
                &t.values,
            );
            self.picker.owners.insert(x, y, t);
            // The pointer may be waiting on this tile.
            if let Some((pane, at)) = self.model.pointer() {
                self.hover(pane, at);
            }
        }
    }

    /// One frame: take loaded tiles, ask for the ones the cameras need,
    /// sync the highlight and draw if anything changed (or `force`).
    /// Returns the CPU time spent encoding and submitting, ms, when it drew.
    pub fn frame(&mut self, force: bool) -> Result<Option<f64>, String> {
        if self.model.mode() == Mode::Overlay {
            self.ensure_overlay();
        }
        self.sync_state();
        self.drain();
        self.request_tiles();
        let (role, extra, apart) = (self.lit_role(), self.extra_bound(), self.pin_apart());
        // Hovering the pinned item is the pin: drawn as one.
        let pin = self.model.lit_is_pin()
            || self
                .model
                .pinned()
                .is_some_and(|p| &p.highlight == self.model.lit());
        if self.lit_pin_apart != apart
            || self.lit != *self.model.lit()
            || self.lit_role != role
            || self.lit_pin != pin
            || self.lit_extra != extra
        {
            self.lit_extra = extra;
            self.lit_pin_apart = apart;
            self.lit = self.model.lit().clone();
            self.lit_role = role;
            self.lit_pin = pin;
            self.sync_highlight();
            self.dirty = true;
        }
        if !(self.dirty || force) {
            return Ok(None);
        }
        self.dirty = false;
        let start = now();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut frames = Vec::new();
        if let Some(cam) = self.model.redraw_camera() {
            let f = acquire(&self.redraw, &self.device)?;
            let target = f.texture.create_view(&Default::default());
            self.renderer.render(
                &self.queue,
                &mut encoder,
                &target,
                &View {
                    origin: cam.origin,
                    zoom: cam.zoom,
                    size: cam.size,
                    min_width: 1.0,
                },
            );
            let view = View {
                origin: cam.origin,
                zoom: cam.zoom,
                size: cam.size,
                min_width: 1.0,
            };
            self.draw_icons(Pane::Redraw, &mut encoder, &target, &view);
            frames.push(f);
        }
        if self.nav.dirty {
            self.nav.dirty = false;
            let c = &self.nav.surface.config;
            let size = (c.width, c.height);
            let map = self.nav_map();
            let f = acquire(&self.nav.surface, &self.device)?;
            let target = f.texture.create_view(&Default::default());
            self.nav.renderer.render(
                &self.queue,
                &mut encoder,
                &target,
                &View {
                    origin: map.origin,
                    zoom: map.scale,
                    size,
                    min_width: 1.0,
                },
            );
            frames.push(f);
        }
        if let Some(cam) = self.model.scan_camera() {
            let f = acquire(&self.scan, &self.device)?;
            let target = f.texture.create_view(&Default::default());
            let view = ScanView {
                origin: cam.origin,
                zoom: cam.zoom,
                size: cam.size,
            };
            self.raster.render(
                &self.device,
                &self.queue,
                &mut encoder,
                &target,
                &self.pyramid,
                &view,
            );
            let crop_view = View {
                origin: cam.origin,
                zoom: cam.zoom,
                size: cam.size,
                min_width: 1.0,
            };
            // The redraw over the scan: our vectors through the warp.
            let overlaid = self.model.mode() == Mode::Overlay;
            if overlaid {
                self.overlay.render(
                    &self.device,
                    &self.queue,
                    &mut encoder,
                    &target,
                    &crop_view,
                    self.model.opacity() as f32,
                );
            }
            // The exact pixels where the owner map reaches; else (zoomed out,
            // where an ink pixel is far under a screen pixel, or its tiles
            // are still coming) the group's geometry through the chain.
            let owned = cam.zoom >= OWNER_ZOOM;
            if owned {
                self.owners.render(
                    &self.device,
                    &self.queue,
                    &mut encoder,
                    &target,
                    &view,
                    self.owner_lit().as_ref(),
                );
            }
            // Under the overlay the lit group's vectors are drawn in full
            // strength over the translucent redraw, too.
            if overlaid
                || !owned
                || !self
                    .owners
                    .missing(self.pyramid.tile, self.owner_tiles, &view)
                    .is_empty()
            {
                self.outline
                    .render_over(&self.queue, &mut encoder, &target, &crop_view);
            }
            if overlaid {
                self.draw_icons(Pane::Scan, &mut encoder, &target, &crop_view);
            }
            frames.push(f);
        }
        self.queue.submit([encoder.finish()]);
        for f in frames {
            self.queue.present(f);
        }
        Ok(Some(now() - start))
    }

    /// The role of a node, from its layer category.
    fn role_of(&self, n: lmsheets_inspector::NodeId) -> Role {
        self.picker
            .layers()
            .of_node(n)
            .map_or(Role::Entity, Role::of_layer)
    }

    /// The role of the group a node is in: its head's (a dimension hovered
    /// by its witness line is the dimension's).
    fn group_role(&self, n: lmsheets_inspector::NodeId) -> Role {
        let head = self.groups.groups_of(n).next().map_or(n, |g| g.head);
        self.role_of(head)
    }

    /// The role the lit group is drawn in: the constraint's bound entities,
    /// or the hovered (else pinned) node's group.
    fn lit_role(&self) -> Role {
        if !self.model.hovered_constraints().is_empty() {
            return Role::Bound;
        }
        if let Some(n) = self.model.hovered() {
            return self.group_role(n);
        }
        match self.model.pinned().map(|p| p.head) {
            Some(PinHead::Constraint(_)) => Role::Bound,
            Some(PinHead::Node(n)) => self.group_role(n),
            None => Role::Entity,
        }
    }

    /// The lit group's colour.
    fn colour(&self) -> [f32; 3] {
        self.model.theme().role(self.lit_role())
    }

    /// The colour of a node that only serves the lit group: its own
    /// role's.
    fn serving_colour(&self, n: lmsheets_inspector::NodeId) -> [f32; 3] {
        self.model.theme().role(self.role_of(n))
    }

    /// Light the highlight in the redraw pane's instances and rebuild the
    /// scan pane's outline of it. Members are lit fully in the group's role
    /// colour, at least [`LIT_MIN_PX`] wide (the pin a little wider);
    /// serving lines lightly in their own role's colour, [`SERVING_MIN_PX`]
    /// wide, at every zoom.
    fn sync_highlight(&mut self) {
        let c = self.colour();
        let dpr = self.dpr as f32;
        let member_px = (LIT_MIN_PX + if self.lit_pin { 2.0 } else { 0.0 }) * dpr;
        let members = self.picker.items_in(&self.lit.members);
        // Serving nodes by role, each role its own colour.
        let mut by_role: std::collections::BTreeMap<Role, Vec<lmsheets_inspector::NodeId>> =
            Default::default();
        for n in &self.lit.serving {
            by_role.entry(self.role_of(*n)).or_default().push(*n);
        }
        let serving: Vec<(Vec<usize>, [f32; 3])> = by_role
            .iter()
            .map(|(r, ns)| (self.picker.items_in(ns), self.model.theme().role(*r)))
            .collect();
        let mut lits: Vec<Lit> = serving
            .iter()
            .map(|(items, colour)| Lit {
                items,
                colour: *colour,
                alpha: 0.55,
                min_px: SERVING_MIN_PX * dpr,
            })
            .collect();
        let bound = self.model.theme().role(Role::Bound);
        let extra_items = self.picker.items_in(&self.lit_extra);
        lits.push(Lit {
            items: &extra_items,
            colour: bound,
            alpha: 0.9,
            min_px: LIT_MIN_PX * dpr,
        });
        let pin_colour = self.pin_colour();
        let pin_items = self.picker.items_in(&self.lit_pin_apart);
        lits.push(Lit {
            items: &pin_items,
            colour: pin_colour,
            alpha: 1.0,
            min_px: (LIT_MIN_PX + 2.0) * dpr,
        });
        lits.push(Lit {
            items: &members,
            colour: c,
            alpha: 1.0,
            min_px: member_px,
        });
        self.renderer.set_highlight(&self.queue, &lits);
        let chain = self.model.chain();
        let mut instances: Vec<Instance> = Vec::new();
        for n in &self.lit.serving {
            let k = self.serving_colour(*n);
            for [a, b] in self.picker.outline(std::slice::from_ref(n), chain) {
                instances.push(segment(
                    a,
                    b,
                    1e-4,
                    [k[0], k[1], k[2], 0.55],
                    SERVING_MIN_PX * dpr,
                ));
            }
        }
        for [a, b] in self.picker.outline(&self.lit_extra, chain) {
            instances.push(segment(
                a,
                b,
                1e-4,
                [bound[0], bound[1], bound[2], 0.8],
                LIT_MIN_PX * dpr,
            ));
        }
        for [a, b] in self.picker.outline(&self.lit_pin_apart, chain) {
            instances.push(segment(
                a,
                b,
                1e-4,
                [pin_colour[0], pin_colour[1], pin_colour[2], 0.9],
                (LIT_MIN_PX + 2.0) * dpr,
            ));
        }
        for [a, b] in self.picker.outline(&self.lit.members, chain) {
            instances.push(segment(a, b, 1e-4, [c[0], c[1], c[2], 0.9], member_px));
        }
        self.outline
            .set_instances(&self.device, &self.queue, &instances);
    }

    /// The owner layer's lit values for the lit group.
    fn owner_lit(&self) -> Option<OwnerLit> {
        let h = self.model.lit();
        (!h.is_empty() || !self.lit_pin_apart.is_empty()).then(|| OwnerLit {
            colour: self.colour(),
            pin_colour: self.pin_colour(),
            pin: self.lit_pin_apart.iter().map(|n| n.0 + 1).collect(),
            serving_colour: h
                .serving
                .first()
                .map(|n| self.serving_colour(*n))
                .or_else(|| {
                    (!self.lit_extra.is_empty()).then(|| self.model.theme().role(Role::Bound))
                }),
            members: h.members.iter().map(|n| n.0 + 1).collect(),
            serving: h
                .serving
                .iter()
                .chain(&self.lit_extra)
                .map(|n| n.0 + 1)
                .collect(),
        })
    }

    /// Centre the redraw pane on paper point `at` (mm) at `scale` screen px
    /// per mm: a pan, then a zoom about the centre, both plain messages.
    pub fn look_at(&mut self, at: (f64, f64), scale: f64) {
        let Some(size) = self.model.pane_size(Pane::Redraw) else {
            return;
        };
        let mid = (size.0 as f64 / 2.0, size.1 as f64 / 2.0);
        let (here, s) = (self.model.centre(), self.model.scale());
        self.send(Msg::Pan {
            pane: Pane::Redraw,
            by: ((here.0 - at.0) * s, (here.1 - at.1) * s),
        });
        let factor = scale * self.dpr / self.model.scale();
        self.send(Msg::Zoom {
            pane: Pane::Redraw,
            factor,
            about: mid,
        });
    }

    /// One scripted step of the benchmark: zoom in on the view's centre for
    /// the first 90 frames (loading full-resolution tiles and owner maps),
    /// then pan back and forth and breathe the zoom, in both panes' locked
    /// cameras. Deterministic in `i`.
    pub fn bench_step(&mut self, i: u32) {
        let t = i as f64;
        let (pane, size) = match self.model.pane_size(Pane::Redraw) {
            Some(s) => (Pane::Redraw, s),
            None => (
                Pane::Scan,
                self.model.pane_size(Pane::Scan).unwrap_or((1, 1)),
            ),
        };
        let mid = (size.0 as f64 / 2.0, size.1 as f64 / 2.0);
        if i < 90 {
            self.send(Msg::Zoom {
                pane,
                factor: 1.035,
                about: mid,
            });
        } else {
            self.send(Msg::Pan {
                pane,
                by: (14.0 * (t * 0.05).cos(), 9.0 * (t * 0.037).sin()),
            });
            self.send(Msg::Zoom {
                pane,
                factor: 1.0 + 0.012 * (t * 0.09).sin(),
                about: mid,
            });
        }
        // A moving pointer, so the hover pick runs every frame too.
        let at = (
            mid.0 + 120.0 * (t * 0.07).sin(),
            mid.1 + 80.0 * (t * 0.11).cos(),
        );
        self.hover(pane, at);
    }
}
