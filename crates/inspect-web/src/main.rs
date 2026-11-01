//! lmsheets-inspect-web: the inspector in the browser. Leptos draws the
//! chrome (mode buttons, status line, FPS); `lmsheets-inspector` holds the
//! model and its pure `update`; `lmsheets-gpu` draws the two panes on
//! WebGPU. Load a bundle served next to the page (`?bundle=sheet/`).

#![warn(missing_docs)]

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod net;
#[cfg(target_arch = "wasm32")]
mod shell;
// Pure, so they are tested natively; only the wasm build uses them.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod gesture;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod icons;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod stats;

#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}

/// The inspector is a web page; natively, the GPU crate's example shows the
/// same panes: `cargo run -p lmsheets-gpu --example inspect`.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("lmsheets-inspect-web runs in the browser: crates/inspect-web/serve.sh BUNDLE_DIR");
}
