//! Fetching the bundle's files, and decoding images with the browser.
//!
//! WebP scan tiles are decoded by the browser (`createImageBitmap`, off the
//! main thread) and read back as grey. The owner tiles are 16-bit PNG,
//! which browsers decode to 8 bits, so they are fetched as bytes and decoded
//! by `lmsheets-inspector`'s `png` (a Rust decode on the main thread, only
//! for the few tiles in view when zoomed in).

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    ColorSpaceConversion, ImageBitmap, ImageBitmapOptions, OffscreenCanvas,
    OffscreenCanvasRenderingContext2d, PremultiplyAlpha, Response,
};

fn js(e: JsValue) -> String {
    let field = |k: &str| js_sys::Reflect::get(&e, &k.into()).ok()?.as_string();
    match (field("name"), field("message")) {
        (Some(name), Some(message)) => format!("{name}: {message}"),
        (None, Some(message)) => message,
        _ => e.as_string().unwrap_or_else(|| format!("{e:?}")),
    }
}

async fn response(url: &str) -> Result<Response, String> {
    let window = web_sys::window().ok_or("no window")?;
    let r: Response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("{url}: {}", js(e)))?
        .dyn_into()
        .map_err(|_| format!("{url}: not a response"))?;
    if r.ok() {
        Ok(r)
    } else {
        Err(format!("{url}: HTTP {}", r.status()))
    }
}

/// A file's bytes.
pub async fn bytes(url: &str) -> Result<Vec<u8>, String> {
    let r = response(url).await?;
    let buf = JsFuture::from(r.array_buffer().map_err(js)?)
        .await
        .map_err(|e| format!("{url}: reading: {}", js(e)))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// A file's text.
pub async fn text(url: &str) -> Result<String, String> {
    String::from_utf8(bytes(url).await?).map_err(|e| format!("{url}: {e}"))
}

/// A WebP or PNG tile decoded by the browser, as 8-bit grey (the red
/// channel: the tiles are grey), with its size.
pub async fn grey_tile(url: &str) -> Result<((u32, u32), Vec<u8>), String> {
    let r = response(url).await?;
    let blob: web_sys::Blob = JsFuture::from(r.blob().map_err(js)?)
        .await
        .map_err(js)?
        .dyn_into()
        .map_err(|_| "not a blob")?;
    let opts = ImageBitmapOptions::new();
    opts.set_color_space_conversion(ColorSpaceConversion::None);
    opts.set_premultiply_alpha(PremultiplyAlpha::None);
    let window = web_sys::window().ok_or("no window")?;
    let bitmap: ImageBitmap = JsFuture::from(
        window
            .create_image_bitmap_with_blob_and_image_bitmap_options(&blob, &opts)
            .map_err(js)?,
    )
    .await
    .map_err(|e| format!("{url}: {}", js(e)))?
    .dyn_into()
    .map_err(|_| "not a bitmap")?;
    let (w, h) = (bitmap.width(), bitmap.height());
    let canvas = OffscreenCanvas::new(w, h).map_err(js)?;
    let ctx: OffscreenCanvasRenderingContext2d = canvas
        .get_context("2d")
        .map_err(js)?
        .ok_or("no 2d context")?
        .dyn_into()
        .map_err(|_| "not a 2d context")?;
    ctx.draw_image_with_image_bitmap(&bitmap, 0.0, 0.0)
        .map_err(js)?;
    bitmap.close();
    let data = ctx
        .get_image_data(0.0, 0.0, w as f64, h as f64)
        .map_err(js)?
        .data();
    Ok(((w, h), data.0.chunks_exact(4).map(|p| p[0]).collect()))
}
