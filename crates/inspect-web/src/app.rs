//! The Leptos chrome: mode buttons, the layers panel, the theme switch, the
//! status line (hovered node, FPS), the navigator and the pane canvases whose
//! pointer events become inspector messages. It holds no model of its own:
//! the [`Shell`] holds the inspector's, and the signals here only mirror what
//! the chrome shows of it.

use crate::gesture::{self, Fling, Gesture, TouchAction, Touches, Wheel, WheelMode};
use crate::shell::{FilterRow, LayerRow, Shell};
use crate::stats::FrameStats;
use leptos::prelude::*;
use leptos::task::spawn_local;
use lmsheets_inspector::detail::Detail;
use lmsheets_inspector::{
    ConstraintKind, ConstraintTrust, Layer, Layout, Mode, Msg, Pane, PinHead, Role, Sheet, Theme,
};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

type Handle = Rc<RefCell<Option<Shell>>>;
/// The fingers on the panes, the pane the gesture began on, and the momentum
/// still running from the last one (with the frame time it was last stepped).
type Fingers = Rc<RefCell<(Touches, Pane, Option<(Fling, f64)>)>>;

/// The About text (shared with the static fallback page, which fetches the
/// same file). Wording approved by the user, 2026-11-01.
const ABOUT: &str = include_str!("../about.html");

/// The page's layout: `?layout=mobile|desktop` decides; else a phone-sized
/// touch device (coarse pointer, narrow, or short: a phone turned on its
/// side) gets the mobile one.
fn layout_choice() -> Layout {
    match query("layout").as_deref() {
        Some("mobile") => return Layout::Mobile,
        Some("desktop") => return Layout::Desktop,
        _ => {}
    }
    let phone = window()
        .match_media(
            "(pointer: coarse) and (max-width: 820px), (pointer: coarse) and (max-height: 520px)",
        )
        .ok()
        .flatten()
        .is_some_and(|m| m.matches());
    if phone {
        Layout::Mobile
    } else {
        Layout::Desktop
    }
}

/// Whether the user asks for reduced motion: no momentum after a drag.
fn reduced_motion() -> bool {
    window()
        .match_media("(prefers-reduced-motion: reduce)")
        .ok()
        .flatten()
        .is_some_and(|m| m.matches())
}

/// A failure message with the build it happened in (index.html's
/// `LMS_BUILD`, set by site.sh), so a report names the version.
fn with_build(e: &str) -> String {
    let build = js_sys::Reflect::get(&window(), &"LMS_BUILD".into())
        .ok()
        .and_then(|b| b.as_string())
        .unwrap_or_else(|| "unknown".into());
    format!("{e} (build {build})")
}

/// Take down the loading indicator (index.html's `#boot`), once the sheet is
/// drawn or the viewer has failed and says why.
fn remove_boot() {
    if let Some(e) = window()
        .document()
        .and_then(|d| d.get_element_by_id("boot"))
    {
        e.remove();
    }
}

/// Go to the static page, saying why.
fn to_fallback(reason: &str) {
    let enc: String = js_sys::encode_uri_component(reason).into();
    let _ = window()
        .location()
        .replace(&format!("fallback.html?reason={enc}"));
}
/// The rAF callback, which re-arms itself.
type Tick = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

/// The `key=value` pairs of the URL's query.
fn query(key: &str) -> Option<String> {
    let search = window().location().search().unwrap_or_default();
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string())
}

/// Whether the system prefers a dark scheme.
fn prefers_dark() -> bool {
    window()
        .match_media("(prefers-color-scheme: dark)")
        .ok()
        .flatten()
        .is_some_and(|m| m.matches())
}

/// The CSS name of a theme (`data-theme` on the page).
fn css_name(t: Theme) -> &'static str {
    match t {
        Theme::WhiteLab => "white",
        Theme::Hud => "hud",
    }
}

fn webgpu_available() -> bool {
    let nav = window().navigator();
    js_sys::Reflect::get(&nav, &"gpu".into()).is_ok_and(|g| !g.is_undefined() && !g.is_null())
}

/// A scripted benchmark in flight.
struct Bench {
    frame: u32,
    frames: u32,
    stats: FrameStats,
    encode: FrameStats,
}

#[component]
fn PaneCanvas(
    pane: Pane,
    handle: Handle,
    node_ref: NodeRef<leptos::html::Canvas>,
    wheel_mode: RwSignal<WheelMode>,
    fingers: Fingers,
) -> impl IntoView {
    let dragging = Rc::new(Cell::new(false));
    // How far the pointer went since it went down: a press that stays put is
    // a click (it pins), one that moves is a drag (it pans).
    let travel = Rc::new(Cell::new(0.0f64));
    let px = |handle: &Handle, x: f64, y: f64| {
        let dpr = handle.borrow().as_ref().map_or(1.0, |s| s.dpr());
        (x * dpr, y * dpr)
    };
    let on_down = {
        let (dragging, travel, fingers) = (dragging.clone(), travel.clone(), fingers.clone());
        move |ev: web_sys::PointerEvent| {
            if let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
            if ev.pointer_type() == "touch" {
                let mut f = fingers.borrow_mut();
                f.1 = pane;
                f.2 = None;
                f.0.down(
                    ev.pointer_id(),
                    (ev.offset_x() as f64, ev.offset_y() as f64),
                    ev.time_stamp(),
                );
                return;
            }
            dragging.set(true);
            travel.set(0.0);
        }
    };
    let on_up = {
        let (dragging, travel, handle) = (dragging.clone(), travel.clone(), handle.clone());
        let fingers = fingers.clone();
        move |ev: web_sys::PointerEvent| {
            if ev.pointer_type() == "touch" {
                let acts = fingers.borrow_mut().0.up(ev.pointer_id(), ev.time_stamp());
                for a in acts {
                    match (a, handle.borrow_mut().as_mut()) {
                        (TouchAction::Tap(at), Some(s)) => {
                            s.touch(pane, (at.0 * s.dpr(), at.1 * s.dpr()), false);
                        }
                        (TouchAction::Fling(f), _) => {
                            fingers.borrow_mut().2 =
                                f.for_motion(reduced_motion()).map(|f| (f, 0.0));
                        }
                        _ => {}
                    }
                }
                return;
            }
            if dragging.replace(false) && travel.get() < 4.0 {
                let at = px(&handle, ev.offset_x() as f64, ev.offset_y() as f64);
                if let Some(s) = handle.borrow_mut().as_mut() {
                    s.click(pane, at);
                }
            }
        }
    };
    let on_move = {
        let (handle, dragging, travel) = (handle.clone(), dragging.clone(), travel.clone());
        let fingers = fingers.clone();
        move |ev: web_sys::PointerEvent| {
            if ev.pointer_type() == "touch" {
                let at = (ev.offset_x() as f64, ev.offset_y() as f64);
                let acts = {
                    let mut f = fingers.borrow_mut();
                    let acts = f.0.moved(ev.pointer_id(), at, ev.time_stamp());
                    if !acts.is_empty() {
                        f.2 = None;
                    }
                    acts
                };
                if let Some(s) = handle.borrow_mut().as_mut() {
                    let k = s.dpr();
                    for a in acts {
                        match a {
                            TouchAction::Pan(by) => s.send(Msg::Pan {
                                pane,
                                by: (by.0 * k, by.1 * k),
                            }),
                            TouchAction::Zoom { factor, about } => s.send(Msg::Zoom {
                                pane,
                                factor,
                                about: (about.0 * k, about.1 * k),
                            }),
                            _ => {}
                        }
                    }
                }
                return;
            }
            if dragging.get() {
                travel.set(travel.get() + (ev.movement_x() as f64).hypot(ev.movement_y() as f64));
            }
            let at = px(&handle, ev.offset_x() as f64, ev.offset_y() as f64);
            let by = px(&handle, ev.movement_x() as f64, ev.movement_y() as f64);
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.set_coarse(false);
                s.set_alt(ev.alt_key());
                s.set_shift(ev.shift_key());
                if dragging.get() {
                    s.send(Msg::Pan { pane, by });
                }
                s.hover(pane, at);
            }
        }
    };
    let on_leave = {
        let (handle, fingers) = (handle.clone(), fingers.clone());
        move |ev: web_sys::PointerEvent| {
            // A finger lifting is `pointerup`; only a cancel ends a touch here.
            if ev.pointer_type() == "touch" {
                if ev.type_() == "pointercancel" {
                    fingers.borrow_mut().0.cancel();
                }
                return;
            }
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.leave();
            }
        }
    };
    // Two-finger scroll pans, a pinch (ctrl + wheel) or ⌘ + scroll zooms about
    // the cursor; a mouse wheel zooms (see `gesture`).
    let on_wheel = {
        let handle = handle.clone();
        move |ev: web_sys::WheelEvent| {
            ev.prevent_default();
            let w = Wheel {
                dx: ev.delta_x(),
                dy: ev.delta_y(),
                mode: ev.delta_mode(),
                ctrl: ev.ctrl_key(),
                meta: ev.meta_key(),
            };
            let at = px(&handle, ev.offset_x() as f64, ev.offset_y() as f64);
            if let Some(s) = handle.borrow_mut().as_mut() {
                match gesture::wheel(w, wheel_mode.get_untracked()) {
                    Gesture::Pan(by) => {
                        let by = (by.0 * s.dpr(), by.1 * s.dpr());
                        s.send(Msg::Pan { pane, by });
                    }
                    Gesture::Zoom(factor) => s.send(Msg::Zoom {
                        pane,
                        factor,
                        about: at,
                    }),
                }
            }
        }
    };
    let on_dblclick = {
        let handle = handle.clone();
        move |_: web_sys::MouseEvent| {
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.send(Msg::Fit);
            }
        }
    };
    // Safari's pinch is a gesture, not a wheel: `scale` is cumulative since
    // `gesturestart`, so zoom by its change.
    {
        let (handle, canvas) = (handle.clone(), node_ref);
        let last = Rc::new(Cell::new(1.0f64));
        Effect::new(move |_| {
            let Some(canvas) = canvas.get() else {
                return;
            };
            let num = |ev: &web_sys::Event, k: &str| {
                js_sys::Reflect::get(ev, &k.into())
                    .ok()
                    .and_then(|v| v.as_f64())
            };
            let start = {
                let last = last.clone();
                Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
                    ev.prevent_default();
                    last.set(1.0);
                })
            };
            let change = {
                let (handle, last, canvas) = (handle.clone(), last.clone(), canvas.clone());
                Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
                    ev.prevent_default();
                    let scale = num(&ev, "scale").unwrap_or(1.0);
                    let factor = gesture::gesture_factor(scale, last.replace(scale));
                    let rect = canvas.get_bounding_client_rect();
                    let at = (
                        num(&ev, "clientX").unwrap_or(0.0) - rect.left(),
                        num(&ev, "clientY").unwrap_or(0.0) - rect.top(),
                    );
                    let at = px(&handle, at.0, at.1);
                    if let Some(s) = handle.borrow_mut().as_mut() {
                        s.send(Msg::Zoom {
                            pane,
                            factor,
                            about: at,
                        });
                    }
                })
            };
            let _ = canvas
                .add_event_listener_with_callback("gesturestart", start.as_ref().unchecked_ref());
            let _ = canvas
                .add_event_listener_with_callback("gesturechange", change.as_ref().unchecked_ref());
            start.forget();
            change.forget();
        });
    }
    let id = match pane {
        Pane::Redraw => "redraw",
        Pane::Scan => "scan",
    };
    view! {
        <canvas id=id class="pane" node_ref=node_ref
            on:pointerdown=on_down on:pointerup=on_up on:pointercancel=on_leave.clone()
            on:pointermove=on_move on:pointerleave=on_leave on:wheel=on_wheel
            on:dblclick=on_dblclick></canvas>
    }
}

/// The navigator: the sheet in a corner with the view boxed. A press or drag
/// centres the view under the pointer.
#[component]
fn Navigator(
    handle: Handle,
    node_ref: NodeRef<leptos::html::Canvas>,
    box_style: RwSignal<String>,
) -> impl IntoView {
    let down = Rc::new(Cell::new(false));
    let at = |ev: &web_sys::PointerEvent| (ev.offset_x() as f64, ev.offset_y() as f64);
    let on_down = {
        let (handle, down) = (handle.clone(), down.clone());
        move |ev: web_sys::PointerEvent| {
            down.set(true);
            if let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.nav_pointer(at(&ev));
            }
        }
    };
    let on_move = {
        let (handle, down) = (handle.clone(), down.clone());
        move |ev: web_sys::PointerEvent| {
            if down.get() {
                if let Some(s) = handle.borrow_mut().as_mut() {
                    s.nav_pointer(at(&ev));
                }
            }
        }
    };
    let on_up = move |_: web_sys::PointerEvent| down.set(false);
    view! {
        <div class="nav" title="Navigator: click or drag to move the view">
            <canvas id="nav" node_ref=node_ref
                on:pointerdown=on_down on:pointermove=on_move on:pointerup=on_up></canvas>
            <div class="nav-box" style=move || box_style.get()></div>
        </div>
    }
}

/// Where the sheet is and what it is called: `index.json` beside the page
/// (`{"sheets": [{"path": "sheets/X/shN/", "title": "..."}]}`, the first
/// sheet), else the development server's `sheet/`.
async fn default_sheet() -> (String, String) {
    let fallback = ("sheet/".to_string(), String::new());
    let Ok(text) = crate::net::text("index.json").await else {
        return fallback;
    };
    let Ok(json) = js_sys::JSON::parse(&text) else {
        return fallback;
    };
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &k.into()).ok();
    let first = get(&json, "sheets").and_then(|a| get(&a, "0"));
    let str_of = |k: &str| first.as_ref().and_then(|f| get(f, k)?.as_string());
    match str_of("path") {
        Some(p) => (p, str_of("title").unwrap_or_default()),
        None => fallback,
    }
}

/// The cards of a detail panel (the desktop drawer and the mobile sheet).
fn sections_view<F: Fn(Option<PinHead>) + 'static>(
    sections: Vec<lmsheets_inspector::detail::Section>,
    pin_link: StoredValue<F, LocalStorage>,
) -> impl IntoView {
    sections
        .into_iter()
        .map(|sec| {
            let rows = sec
                .rows
                .into_iter()
                .map(|r| {
                    let value = match (r.pin, r.href.clone()) {
                        (Some(pin), _) => view! {
                            <a class="pin" on:click=move |_| pin_link.with_value(|f| f(Some(pin)))>
                                {r.value.clone()}
                            </a>
                        }
                        .into_any(),
                        (None, Some(href)) => view! {
                            <a class="ext" href=href target="_blank" rel="noopener">{r.value.clone()}</a>
                        }
                        .into_any(),
                        _ => view! { <span>{r.value.clone()}</span> }.into_any(),
                    };
                    view! { <div class="drow"><span class="k">{r.key}</span><span class="v">{value}</span></div> }
                })
                .collect_view();
            view! { <section class="card"><h3>{sec.title}</h3>{rows}</section> }
        })
        .collect_view()
}

/// The whole window.
#[component]
pub fn App() -> impl IntoView {
    let handle: Handle = Rc::new(RefCell::new(None));
    let redraw_ref = NodeRef::<leptos::html::Canvas>::new();
    let scan_ref = NodeRef::<leptos::html::Canvas>::new();
    let nav_ref = NodeRef::<leptos::html::Canvas>::new();
    let stage_ref = NodeRef::<leptos::html::Div>::new();
    let mode = RwSignal::new(Mode::SideBySide);
    let layout = RwSignal::new(layout_choice());
    let wipe = RwSignal::new(0.5f64);
    let stacked = RwSignal::new(false);
    let sheet = RwSignal::new(Sheet::Closed);
    let menu_open = RwSignal::new(false);
    let about_open = RwSignal::new(false);
    let nav_on = RwSignal::new(layout.get_untracked() == Layout::Desktop);
    let summary = RwSignal::new(String::new());
    let title = RwSignal::new(String::new());
    let fingers: Fingers = Rc::new(RefCell::new((Touches::default(), Pane::Redraw, None)));
    let grip_from = Rc::new(Cell::new(0.0f64));
    let wipe_drag = Rc::new(Cell::new(false));
    // The look follows the system until the switch is used.
    let theme = RwSignal::new(Theme::for_scheme(prefers_dark()));
    let theme_chosen = Rc::new(Cell::new(false));
    let opacity = RwSignal::new(0.85f64);
    let hidden = RwSignal::new(BTreeSet::<Layer>::new());
    let rows = RwSignal::new(Vec::<LayerRow>::new());
    let nav_box = RwSignal::new(String::new());
    // What the constraints section mirrors of the model.
    let trust_rows = RwSignal::new(Vec::<FilterRow<ConstraintTrust>>::new());
    let kind_rows = RwSignal::new(Vec::<FilterRow<ConstraintKind>>::new());
    let trust_off = RwSignal::new(BTreeSet::<ConstraintTrust>::new());
    let kind_off = RwSignal::new(BTreeSet::<ConstraintKind>::new());
    let only_selection = RwSignal::new(false);
    let dimension_icons = RwSignal::new(false);
    let icon_count = RwSignal::new(0usize);
    // The panel of the pinned item, and which pin it was built for.
    let detail = RwSignal::new(None::<Detail>);
    let pin_seen = Rc::new(Cell::new((None::<PinHead>, false)));
    let pinned = RwSignal::new(false);
    let details_open = RwSignal::new(false);
    let constraints_on = RwSignal::new(false);
    let roles = RwSignal::new(Vec::<(Role, String)>::new());
    let wheel_mode = RwSignal::new(WheelMode::Auto);
    let hover = RwSignal::new(String::from("none"));
    let source = RwSignal::new(String::new());
    let zoom = RwSignal::new(String::new());
    let fps = RwSignal::new(String::from("no frames"));
    let bench_out = RwSignal::new(String::new());
    let fail = RwSignal::new(String::new());
    let status = RwSignal::new(String::from("loading the bundle"));
    let bench: Rc<RefCell<Option<Bench>>> = Rc::new(RefCell::new(None));

    let start_bench = {
        let bench = bench.clone();
        move || {
            *bench.borrow_mut() = Some(Bench {
                frame: 0,
                frames: 420,
                stats: FrameStats::new(1e9),
                encode: FrameStats::new(1e9),
            });
            bench_out.set("benchmarking".into());
        }
    };

    // Open the bundle once the canvases exist.
    {
        let (handle, start_bench) = (handle.clone(), start_bench.clone());
        Effect::new(move |_| {
            let (Some(redraw), Some(scan), Some(nav), Some(stage)) = (
                redraw_ref.get(),
                scan_ref.get(),
                nav_ref.get(),
                stage_ref.get(),
            ) else {
                return;
            };
            if handle.borrow().is_some() {
                return;
            }
            // `?nogpu=1` pretends there is none, to see the static page.
            if !webgpu_available() || query("nogpu").is_some() {
                to_fallback("WebGPU is not available in this browser.");
                fail.set("WebGPU is not available in this browser.".into());
                remove_boot();
                return;
            }
            let css = (stage.client_width() as f64, stage.client_height() as f64);
            let (handle, start_bench) = (handle.clone(), start_bench.clone());
            spawn_local(async move {
                let (base, name) = match query("bundle") {
                    Some(b) => (b, String::new()),
                    None => default_sheet().await,
                };
                title.set(name);
                let base = if base.ends_with('/') {
                    base
                } else {
                    base + "/"
                };
                let mobile = layout.get_untracked() == Layout::Mobile;
                match Shell::open(base, redraw, scan, nav, css, mobile).await {
                    Ok(mut shell) => {
                        shell.send(Msg::SetTheme(theme.get_untracked()));
                        shell.set_layout(layout.get_untracked());
                        shell.sync_state();
                        shell.layout_all();
                        rows.set(shell.layer_rows());
                        trust_rows.set(shell.trust_rows());
                        kind_rows.set(shell.kind_rows());
                        opacity.set(shell.model.opacity());
                        hidden.set(shell.model.hidden_layers().clone());
                        constraints_on.set(shell.model.constraints_shown());
                        roles.set(shell.role_rows());
                        *handle.borrow_mut() = Some(shell);
                        status.set("ready".into());
                        // `?mode=side|redraw|scan|overlay` starts in a mode.
                        let start = match query("mode").as_deref() {
                            Some("redraw") => Some(Mode::RedrawOnly),
                            Some("scan") => Some(Mode::ScanOnly),
                            Some("overlay") => Some(Mode::Overlay),
                            Some("wipe") => Some(Mode::Wipe),
                            Some("side" | "compare") => Some(Mode::SideBySide),
                            _ => None,
                        };
                        if let (Some(m), Some(s)) = (start, handle.borrow_mut().as_mut()) {
                            s.set_mode(m);
                            mode.set(m);
                            rows.set(s.layer_rows());
                        }
                        // `?look=X,Y,S`: paper point (mm) and scale (CSS px per mm).
                        if let Some(l) = query("look") {
                            let v: Vec<f64> = l.split(',').filter_map(|p| p.parse().ok()).collect();
                            if let (&[x, y, scale], Some(s)) =
                                (&v[..], handle.borrow_mut().as_mut())
                            {
                                s.look_at((x, y), scale);
                            }
                        }
                        // `?pin=kind:name` starts with that node pinned.
                        if let (Some(p), Some(s)) = (query("pin"), handle.borrow_mut().as_mut()) {
                            let p = p.replace("%3A", ":").replace("%2F", "/");
                            if let Some((kind, name)) = p.split_once(':') {
                                s.pin_key(kind, name);
                            }
                        }
                        if query("bench").is_some() {
                            start_bench();
                        }
                    }
                    Err(e) => {
                        // A GPU that will not open is the static page's case;
                        // a missing bundle is not.
                        if e.starts_with("WebGPU") {
                            to_fallback(&e);
                        }
                        fail.set(with_build(&e));
                        remove_boot();
                    }
                }
            });
        });
    }

    // Resize with the window; a phone turned wide stays a phone, but a
    // window that becomes one (or stops being one) changes layout.
    // Resize with the window.
    {
        let handle = handle.clone();
        let on_resize = Closure::<dyn FnMut()>::new(move || {
            if let (Some(stage), Some(s)) =
                (stage_ref.get_untracked(), handle.borrow_mut().as_mut())
            {
                let want = layout_choice();
                if want != layout.get_untracked() {
                    layout.set(want);
                    s.set_layout(want);
                }
                s.resize((stage.client_width() as f64, stage.client_height() as f64));
            }
        });
        let _ =
            window().add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
        on_resize.forget();
    }

    // `0` fits; Alt takes a single piece while it is held.
    {
        let handle = handle.clone();
        let key = move |down: bool| {
            let handle = handle.clone();
            Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
                if let Some(s) = handle.borrow_mut().as_mut() {
                    match ev.key().as_str() {
                        "0" if down && !ev.meta_key() && !ev.ctrl_key() => s.send(Msg::Fit),
                        // Escape closes the panel, then lets go of the pin.
                        "Escape" if down => {
                            if s.model.about_open() {
                                s.send(Msg::SetAbout(false));
                            } else if s.model.menu_open() {
                                s.send(Msg::SetMenu(false));
                            } else if s.model.details_open() {
                                s.send(Msg::SetDetails(false));
                            } else {
                                s.send(Msg::Unpin);
                            }
                        }
                        "i" if down && !ev.meta_key() && !ev.ctrl_key() => {
                            let open = s.model.details_open();
                            s.send(Msg::SetDetails(!open));
                        }
                        "Alt" => s.set_alt(down),
                        "Shift" => s.set_shift(down),
                        _ => {}
                    }
                }
            })
        };
        let (down, up) = (key(true), key(false));
        let _ = window().add_event_listener_with_callback("keydown", down.as_ref().unchecked_ref());
        let _ = window().add_event_listener_with_callback("keyup", up.as_ref().unchecked_ref());
        down.forget();
        up.forget();
    }

    // The frame loop: rAF, one draw per changed frame (every frame while
    // benchmarking), the FPS window, the status signals.
    {
        let (handle, bench, pin_seen) = (handle.clone(), bench.clone(), pin_seen.clone());
        let fingers = fingers.clone();
        let stats = Rc::new(RefCell::new(FrameStats::new(2000.0)));
        let last = Rc::new(Cell::new(None::<f64>));
        let shown_at = Rc::new(Cell::new(0.0f64));
        let booted = Cell::new(false);
        let tick: Tick = Rc::new(RefCell::new(None));
        let again = tick.clone();
        *tick.borrow_mut() = Some(Closure::new(move |t: f64| {
            if let Some(prev) = last.replace(Some(t)) {
                stats.borrow_mut().push(t, t - prev);
                if let Some(b) = bench.borrow_mut().as_mut() {
                    b.stats.push(t, t - prev);
                }
            }
            if let Some(s) = handle.borrow_mut().as_mut() {
                let benching = bench.borrow().is_some();
                if let Some(b) = bench.borrow_mut().as_mut() {
                    s.bench_step(b.frame);
                    b.frame += 1;
                }
                match s.frame(benching) {
                    Ok(Some(ms)) => {
                        // The first drawn frame: the sheet is up.
                        if !booted.replace(true) {
                            remove_boot();
                        }
                        if let Some(b) = bench.borrow_mut().as_mut() {
                            b.encode.push(t, ms);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        fail.set(with_build(&e));
                        remove_boot();
                    }
                }
                let seen = (s.model.pinned().map(|p| p.head), s.model.details_open());
                if seen != pin_seen.get() {
                    pin_seen.set(seen);
                    pinned.set(seen.0.is_some());
                    details_open.set(seen.1);
                    let d = if seen.0.is_some() { s.detail() } else { None };
                    summary.set(d.as_ref().map_or(String::new(), |d| {
                        let geometry = d
                            .section("Geometry")
                            .and_then(|s| s.rows.first())
                            .map(|r| format!(" \u{b7} {}", r.value));
                        format!("{}{}", d.title, geometry.unwrap_or_default())
                    }));
                    detail.set(d);
                }
                // The model's own state, mirrored for the chrome.
                let m = &s.model;
                if mode.get_untracked() != m.mode() {
                    mode.set(m.mode());
                    rows.set(s.layer_rows());
                }
                let m = &s.model;
                for (sig, v) in [
                    (menu_open, m.menu_open()),
                    (about_open, m.about_open()),
                    (nav_on, m.navigator()),
                ] {
                    if sig.get_untracked() != v {
                        sig.set(v);
                    }
                }
                if sheet.get_untracked() != m.sheet() {
                    sheet.set(m.sheet());
                }
                if stacked.get_untracked() != m.stacked() {
                    stacked.set(m.stacked());
                }
                if wipe.get_untracked() != m.wipe() {
                    wipe.set(m.wipe());
                }
                // Momentum after a touch drag or pinch: the same Pan and Zoom
                // messages, so both locked panes follow, until it dies away.
                {
                    let mut f = fingers.borrow_mut();
                    let pane = f.1;
                    if let Some((fling, last)) = f.2.as_mut() {
                        let dt = if *last == 0.0 {
                            16.0
                        } else {
                            (t - *last).clamp(1.0, 50.0)
                        };
                        *last = t;
                        let (pan, zoom) = fling.step(dt);
                        let k = s.dpr();
                        if pan != (0.0, 0.0) {
                            s.send(Msg::Pan {
                                pane,
                                by: (pan.0 * k, pan.1 * k),
                            });
                        }
                        if zoom != 1.0 {
                            let about = (fling.about.0 * k, fling.about.1 * k);
                            s.send(Msg::Zoom {
                                pane,
                                factor: zoom,
                                about,
                            });
                        }
                        if fling.done() {
                            f.2 = None;
                        }
                    }
                }
                // A finger held still on the paper is a long press.
                let held = fingers.borrow_mut().0.poll(t);
                let pane = fingers.borrow().1;
                for a in held {
                    if let TouchAction::LongPress(at) = a {
                        s.touch(pane, (at.0 * s.dpr(), at.1 * s.dpr()), true);
                    }
                }
                let (l, tp, w, h) = s.nav_box();
                let style = format!("left:{l:.1}px;top:{tp:.1}px;width:{w:.1}px;height:{h:.1}px");
                if nav_box.get_untracked() != style {
                    nav_box.set(style);
                }
                if t - shown_at.get() > 250.0 {
                    shown_at.set(t);
                    hover.set(s.hover_text());
                    source.set(if s.model.hovered().is_some() {
                        s.pick_source.to_string()
                    } else {
                        String::new()
                    });
                    icon_count.set(s.icons_shown());
                    let (scan_tiles, owner_tiles) = s.held();
                    let warped = s.overlay_stats().map_or(String::new(), |o| {
                        format!("  overlay {} to {} pieces in {:.0} ms", o.from, o.to, o.ms)
                    });
                    zoom.set(format!(
                        "scan x{:.2}  tiles {scan_tiles} + {owner_tiles} owner{warped}",
                        s.scan_zoom().unwrap_or(0.0)
                    ));
                    if let Some(e) = s.errors.last() {
                        status.set(format!("error: {e}"));
                    }
                    fps.set(stats.borrow().readout());
                }
            }
            let done = bench.borrow().as_ref().is_some_and(|b| b.frame >= b.frames);
            if done {
                if let Some(b) = bench.borrow_mut().take() {
                    // The whole run, tile loading included.
                    let text = format!(
                        "bench {} frames: frame time mean {:.2} ms p95 {:.2} ms ({:.0} fps); \
                         encode+submit mean {:.2} ms p95 {:.2} ms",
                        b.stats.len(),
                        b.stats.mean().unwrap_or(0.0),
                        b.stats.p95().unwrap_or(0.0),
                        1000.0 / b.stats.mean().unwrap_or(1.0),
                        b.encode.mean().unwrap_or(0.0),
                        b.encode.p95().unwrap_or(0.0),
                    );
                    bench_out.set(text);
                }
            }
            let _ = window()
                .request_animation_frame(again.borrow().as_ref().unwrap().as_ref().unchecked_ref());
        }));
        let _ = window()
            .request_animation_frame(tick.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }

    let set_mode = {
        let handle = handle.clone();
        move |m: Mode| {
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.set_mode(m);
                mode.set(m);
                rows.set(s.layer_rows());
            }
        }
    };
    let (m1, m2) = (set_mode.clone(), set_mode.clone());
    let (b1, b2, b3, b4, b5) = (
        set_mode.clone(),
        set_mode.clone(),
        set_mode.clone(),
        set_mode.clone(),
        set_mode,
    );

    // The look: the page's `data-theme`, and the shell's model.
    {
        let handle = handle.clone();
        Effect::new(move |_| {
            let t = theme.get();
            if let Some(root) = window().document().and_then(|d| d.document_element()) {
                let _ = root.set_attribute("data-theme", css_name(t));
            }
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.send(Msg::SetTheme(t));
                s.sync_state();
                s.layout_all();
                rows.set(s.layer_rows());
                trust_rows.set(s.trust_rows());
                roles.set(s.role_rows());
            }
        });
    }
    // Follow the system's scheme while the switch is untouched.
    {
        let chosen = theme_chosen.clone();
        if let Ok(Some(query)) = window().match_media("(prefers-color-scheme: dark)") {
            let on_change = Closure::<dyn FnMut()>::new(move || {
                if !chosen.get() {
                    theme.set(Theme::for_scheme(prefers_dark()));
                }
            });
            let _ = query
                .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
            on_change.forget();
        }
    }
    let toggle_theme = {
        let chosen = theme_chosen.clone();
        move |_| {
            chosen.set(true);
            theme.update(|t| *t = t.toggled());
        }
    };
    let set_opacity = {
        let handle = handle.clone();
        move |ev: web_sys::Event| {
            let value = event_target_value(&ev).parse::<f64>().unwrap_or(85.0) / 100.0;
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.send(Msg::SetOpacity(value));
                opacity.set(s.model.opacity());
            }
        }
    };
    // The constraint filters: send the message, mirror the model.
    let send_filter = {
        let handle = handle.clone();
        StoredValue::new_local(move |msg: Msg| {
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.send(msg);
                trust_off.set(s.model.hidden_constraint_trust().clone());
                kind_off.set(s.model.hidden_constraint_kinds().clone());
                only_selection.set(s.model.constraints_only_on_selection());
                dimension_icons.set(s.model.dimension_constraints());
                constraints_on.set(s.model.constraints_shown());
                icon_count.set(s.icons_shown());
            }
        })
    };
    // A link in the panel pins what it names; the cross closes the panel.
    let pin_link = {
        let handle = handle.clone();
        StoredValue::new_local(move |head: Option<PinHead>| {
            if let Some(s) = handle.borrow_mut().as_mut() {
                match head {
                    Some(h) => s.pin(h),
                    None => s.send(Msg::Unpin),
                }
            }
        })
    };
    let toggle_layer = {
        let handle = handle.clone();
        // The rows are built in a reactive list, which wants `Send` state.
        StoredValue::new_local(move |layer: Layer| {
            if let Some(s) = handle.borrow_mut().as_mut() {
                let visible = !s.model.layer_visible(layer);
                s.send(Msg::SetLayer { layer, visible });
                hidden.set(s.model.hidden_layers().clone());
            }
        })
    };

    let close_menu = move |_| send_filter.with_value(|f| f(Msg::SetMenu(false)));
    let close_about = move |_| send_filter.with_value(|f| f(Msg::SetAbout(false)));
    let open_about = move |_| send_filter.with_value(|f| f(Msg::SetAbout(true)));
    let open_menu =
        move |_| send_filter.with_value(|f| f(Msg::SetMenu(!menu_open.get_untracked())));
    let toggle_nav =
        move |_| send_filter.with_value(|f| f(Msg::SetNavigator(!nav_on.get_untracked())));
    // The wipe's divider: dragged along the stage.
    let wipe_to = {
        let (handle, stage) = (handle.clone(), stage_ref);
        move |ev: &web_sys::PointerEvent| {
            let Some(stage) = stage.get_untracked() else {
                return;
            };
            let r = stage.get_bounding_client_rect();
            let x = (ev.client_x() as f64 - r.left()) / r.width().max(1.0);
            if let Some(s) = handle.borrow_mut().as_mut() {
                s.send(Msg::SetWipe(x));
            }
        }
    };
    let (wipe_down, wipe_move, wipe_up) = {
        let (d1, d2, d3) = (wipe_drag.clone(), wipe_drag.clone(), wipe_drag);
        let (w1, w2) = (wipe_to.clone(), wipe_to);
        (
            move |ev: web_sys::PointerEvent| {
                d1.set(true);
                if let Some(el) = ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                {
                    let _ = el.set_pointer_capture(ev.pointer_id());
                }
                w1(&ev);
            },
            move |ev: web_sys::PointerEvent| {
                if d2.get() {
                    w2(&ev);
                }
            },
            move |_: web_sys::PointerEvent| d3.set(false),
        )
    };
    // The bottom sheet's grip: a drag up or down steps the sheet.
    let grip_from = StoredValue::new_local(grip_from);
    // The whole head of the sheet (grip and summary) is the handle; the
    // close button inside it keeps its own click.
    let on_button = |ev: &web_sys::PointerEvent| {
        ev.target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| el.closest("button").ok().flatten().is_some())
    };
    let grip_down = move |ev: web_sys::PointerEvent| {
        if on_button(&ev) {
            return;
        }
        grip_from.with_value(|g| g.set(ev.client_y() as f64));
        if let Some(el) = ev
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
    };
    let grip_up = move |ev: web_sys::PointerEvent| {
        if on_button(&ev) {
            return;
        }
        let from = grip_from.with_value(|g| g.get());
        let next = gesture::sheet_after_drag(sheet.get_untracked(), ev.client_y() as f64 - from);
        send_filter.with_value(|f| f(Msg::SetSheet(next)));
    };

    view! {
        <div class="shell" data-layout=move || if layout.get() == Layout::Mobile { "mobile" } else { "desktop" }
            data-mode=move || format!("{:?}", mode.get())
            data-split=move || if stacked.get() { "stacked" } else { "abreast" }>
            <div class="bar">
                <span class="doc-title"><b>{move || title.get()}</b></span>
                <span class="spacer"></span>
                <div class="group modes">
                    <button id="mode-side" class:on=move || mode.get() == Mode::SideBySide
                        on:click=move |_| b1(Mode::SideBySide)>
                        {move || if layout.get() == Layout::Mobile { "Compare" } else { "Side by side" }}
                    </button>
                    <button id="mode-redraw" class:on=move || mode.get() == Mode::RedrawOnly
                        on:click=move |_| b2(Mode::RedrawOnly)>"Redraw"</button>
                    <button id="mode-scan" class:on=move || mode.get() == Mode::ScanOnly
                        on:click=move |_| b3(Mode::ScanOnly)>"Scan"</button>
                    <button id="mode-overlay" class:on=move || mode.get() == Mode::Overlay
                        on:click=move |_| b4(Mode::Overlay)>"Overlay"</button>
                    <button id="mode-wipe" class:on=move || mode.get() == Mode::Wipe
                        on:click=move |_| b5(Mode::Wipe)>"Wipe"</button>
                </div>
                <label class="opacity" style:display=move || if mode.get() == Mode::Overlay { "flex" } else { "none" }
                    title="How opaque the redraw is over the scan">
                    "Opacity"
                    <input id="opacity" type="range" min="0" max="100" step="1"
                        prop:value=move || (opacity.get() * 100.0).round().to_string()
                        on:input=set_opacity />
                    <span class="n">{move || format!("{:.0}%", opacity.get() * 100.0)}</span>
                </label>
                <button class="desktop-only" title="What a plain mouse wheel does: auto guesses a mouse from a trackpad"
                    on:click=move |_| wheel_mode.update(|m| *m = m.toggled())>
                    "Wheel: " {move || wheel_mode.get().label()}
                </button>
                <button id="about" class="desktop-only" title="What this is, and where it comes from"
                    on:click=open_about>"About"</button>
                <button id="theme" class="desktop-only" title="Light and dark follow the system until this is used"
                    on:click=toggle_theme.clone()>{move || theme.get().label()}</button>
                <button id="menu" class="mobile-only" title="Layers, constraints, look, about"
                    class:on=move || menu_open.get()
                    on:click=open_menu>"Menu"</button>
            </div>
            <div class="stage" node_ref=stage_ref>
                <div class="panes" class:wipe=move || mode.get() == Mode::Wipe
                    class:stacked=move || stacked.get()>
                    <PaneCanvas pane=Pane::Redraw handle=handle.clone() node_ref=redraw_ref wheel_mode=wheel_mode fingers=fingers.clone() />
                    <PaneCanvas pane=Pane::Scan handle=handle.clone() node_ref=scan_ref wheel_mode=wheel_mode fingers=fingers.clone() />
                </div>
                <div class="wipe-handle" id="wipe-handle"
                    style:display=move || if mode.get() == Mode::Wipe { "block" } else { "none" }
                    style:left=move || format!("{:.3}%", wipe.get() * 100.0)
                    on:pointerdown=wipe_down on:pointermove=wipe_move on:pointerup=wipe_up>
                    <span class="wipe-knob">"\u{2194}"</span>
                </div>
                <div style:display=move || if nav_on.get() { "contents" } else { "none" }>
                    <Navigator handle=handle.clone() node_ref=nav_ref box_style=nav_box />
                </div>
                {move || (details_open.get() && layout.get() == Layout::Desktop).then(|| detail.get().map(|d| {
                    view! {
                        <div class="drawer" id="detail">
                            <div class="dhead">
                                <span class="dtitle">{d.title}</span>
                                <button class="close" title="Close (Esc)"
                                    on:click=move |_| send_filter.with_value(|f| f(Msg::SetDetails(false)))>"\u{d7}"</button>
                            </div>
                            {sections_view(d.sections, pin_link)}
                        </div>
                    }
                }))}
                {move || (layout.get() == Layout::Mobile && sheet.get() != Sheet::Closed).then(|| {
                    let open = sheet.get() > Sheet::Peek;

                    view! {
                        <div class="sheet" id="sheet" class:half=move || sheet.get() == Sheet::Half
                            class:full=move || sheet.get() == Sheet::Full>
                            <div class="sheet-head" on:pointerdown=grip_down on:pointerup=grip_up>
                                <div class="grip"><span class="grip-bar"></span></div>
                                <div class="peek">
                                    <span class="sum" id="sheet-summary">{move || summary.get()}</span>
                                    <button class="close" title="Let go of the pin"
                                        on:click=move |_| pin_link.with_value(|f| f(None))>"\u{d7}"</button>
                                </div>
                            </div>
                            {open.then(|| detail.get().map(|d| view! {
                                <div class="sheet-body" id="detail">{sections_view(d.sections, pin_link)}</div>
                            }))}
                        </div>
                    }
                })}
                <div class="fail" style:display=move || if fail.get().is_empty() { "none" } else { "flex" }>
                    <span id="fail">{move || fail.get()}</span>
                </div>
            </div>
            <div class="scrim" class:on=move || menu_open.get() || about_open.get()
                on:click=move |_| send_filter.with_value(|f| { f(Msg::SetMenu(false)); f(Msg::SetAbout(false)); })></div>
            <aside class="side" id="menu-sheet" class:open=move || menu_open.get()>
                <div class="menu-head mobile-only">
                    <b>"Menu"</b>
                    <button class="close" on:click=close_menu>"\u{d7}"</button>
                </div>
                <div class="mobile-only menu-actions">
                    <button id="theme-m" on:click=toggle_theme>{move || theme.get().label()}</button>
                    <button id="about-m" on:click=open_about>"About"</button>
                </div>
                <div class="mobile-only menu-actions" id="menu-views">
                    <button id="m-redraw" class:on=move || mode.get() == Mode::RedrawOnly
                        on:click=move |_| { m1(Mode::RedrawOnly); send_filter.with_value(|f| f(Msg::SetMenu(false))); }>"Redraw only"</button>
                    <button id="m-scan" class:on=move || mode.get() == Mode::ScanOnly
                        on:click=move |_| { m2(Mode::ScanOnly); send_filter.with_value(|f| f(Msg::SetMenu(false))); }>"Scan only"</button>
                </div>
                <h2>"Layers"</h2>
                <div id="layers">
                    <For each=move || rows.get() key=|r| (r.layer, r.colour.clone()) let:row>
                        {
                            let layer = row.layer;
                            view! {
                                <div class="row" id=format!("layer-{:?}", layer)
                                    class:off=move || hidden.get().contains(&layer)
                                    class:empty=row.count == 0
                                    on:click=move |_| toggle_layer.with_value(|t| t(layer))>
                                    <span class="tog"></span>
                                    <span class="dot" style:background=row.colour.clone()></span>
                                    <span class="name">{layer.label()}</span>
                                    <span class="n">{row.count}</span>
                                </div>
                            }
                        }
                    </For>
                </div>
                <h2 class="section">"Constraints"</h2>
                <div id="constraints">
                    <div class="row" id="c-on" class:off=move || !constraints_on.get()
                        on:click=move |_| send_filter.with_value(|f| f(Msg::SetConstraintsShown(!constraints_on.get_untracked())))>
                        <span class="tog"></span>
                        <span class="name">"Show constraint icons"</span>
                        <span class="n">{move || icon_count.get()}</span>
                    </div>
                    <details class="filters" id="filters">
                        <summary>"Filters"</summary>
                        <div class="row" id="c-only" class:off=move || !only_selection.get()
                            on:click=move |_| send_filter.with_value(|f| f(Msg::SetConstraintsOnSelection(!only_selection.get_untracked())))>
                            <span class="tog"></span>
                            <span class="name">"Only on the hovered group"</span>
                        </div>
                        <div class="row" id="c-dims" class:off=move || !dimension_icons.get()
                            on:click=move |_| send_filter.with_value(|f| f(Msg::SetDimensionConstraints(!dimension_icons.get_untracked())))>
                            <span class="tog"></span>
                            <span class="name">"Dimensions' own"</span>
                        </div>
                        <h3>"Trust"</h3>
                        <For each=move || trust_rows.get() key=|r| (r.key, r.colour.clone()) let:row>
                            {
                                let trust = row.key;
                                view! {
                                    <div class="row" id=format!("ctrust-{:?}", trust)
                                        class:off=move || trust_off.get().contains(&trust)
                                        class:empty=row.count == 0
                                        on:click=move |_| send_filter.with_value(|f| f(Msg::SetConstraintTrust {
                                            trust,
                                            visible: trust_off.get_untracked().contains(&trust),
                                        }))>
                                        <span class="tog"></span>
                                        <span class="dot" style:background=row.colour.clone()></span>
                                        <span class="name">{row.label}</span>
                                        <span class="n">{row.count}</span>
                                    </div>
                                }
                            }
                        </For>
                        <h3>"Type"</h3>
                        <For each=move || kind_rows.get() key=|r| r.key let:row>
                            {
                                let kind = row.key;
                                view! {
                                    <div class="row" id=format!("ckind-{:?}", kind)
                                        class:off=move || kind_off.get().contains(&kind)
                                        on:click=move |_| send_filter.with_value(|f| f(Msg::SetConstraintKind {
                                            kind,
                                            visible: kind_off.get_untracked().contains(&kind),
                                        }))>
                                        <span class="tog"></span>
                                        <span class="name">{row.label}</span>
                                        <span class="n">{row.count}</span>
                                    </div>
                                }
                            }
                        </For>
                    </details>
                </div>
                <h2 class="section">"View"</h2>
                <div class="row" id="nav-toggle" class:off=move || !nav_on.get() on:click=toggle_nav>
                    <span class="tog"></span>
                    <span class="name">"Navigator"</span>
                </div>
                <details class="legend" id="legend">
                    <summary>"Legend"</summary>
                    <p class="hint">"One colour per role: what is lit, tinted on the scan, drawn in the overlay and listed above."</p>
                    <For each=move || roles.get() key=|r| (r.0, r.1.clone()) let:r>
                        <div class="row plain">
                            <span class="dot" style:background=r.1.clone()></span>
                            <span class="name">{r.0.label()}</span>
                        </div>
                    </For>
                </details>
            </aside>
            <div class="modal" id="about-modal" role="dialog" aria-label="About" class:on=move || about_open.get()>
                <button class="close" id="about-close" title="Close (Esc)" on:click=close_about>"\u{d7}"</button>
                <div class="about-body" inner_html=ABOUT></div>
            </div>
            <div class="status">
                <span><span class="k">"Hover"</span><span class="v node" id="hover">{move || hover.get()}</span></span>
                <span class="hint" style:display=move || if hover.get() == "none" { "none" } else { "inline" }>
                    "Shift: connections \u{b7} Alt: piece \u{b7} click: pin"
                </span>
                <span class="hint">{move || source.get()}</span>
                <span class="hint">{move || zoom.get()}</span>
                <span class="bench" id="bench">{move || bench_out.get()}</span>
                <span class="spacer"></span>
                <button id="details-chip" class="chip" title="The pinned item's details (i)"
                    style:display=move || if pinned.get() { "inline-block" } else { "none" }
                    class:on=move || details_open.get()
                    on:click=move |_| send_filter.with_value(|f| f(Msg::SetDetails(!details_open.get_untracked())))>
                    "Details (i)"
                </button>
                <span class="hint">{move || status.get()}</span>
                <span class="live" id="fps"><span class="k">"FPS"</span>" "{move || fps.get()}</span>
            </div>
        </div>
    }
}
