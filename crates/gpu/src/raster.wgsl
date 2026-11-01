// The raster tile layer: grey tiles as textured quads in crop pixel
// coordinates. The camera is a uniform; the tiles are retained.

struct Camera {
    origin_zoom: vec4<f32>, // crop px at the screen's top-left, then screen px per crop px
    screen: vec4<f32>,      // screen size in px
    ink: vec4<f32>,
    paper: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var tile: texture_2d<f32>;
@group(1) @binding(1) var tile_sampler: sampler;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32, @location(0) rect: vec4<f32>) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[i];
    let px = mix(rect.xy, rect.zw, c);
    let s = (px - camera.origin_zoom.xy) * camera.origin_zoom.z;
    var out: Out;
    out.pos = vec4<f32>(
        s.x / camera.screen.x * 2.0 - 1.0,
        1.0 - s.y / camera.screen.y * 2.0,
        0.0, 1.0);
    out.uv = c;
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let g = textureSample(tile, tile_sampler, in.uv).r;
    return mix(camera.ink, camera.paper, g);
}
