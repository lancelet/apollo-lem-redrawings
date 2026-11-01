// Lays an offscreen layer over the target at one opacity. The layer holds
// premultiplied colour (the strokes were blended over transparent), so
// overlapping pieces of one stroke are drawn once, however translucent the
// layer is laid.

struct Params {
    opacity: vec4<f32>, // x: the layer's opacity
};

@group(0) @binding(0) var layer: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle that covers the target.
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - vec2<f32>(1.0), 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(layer, vec2<i32>(pos.xy), 0);
    return c * params.opacity.x;
}
