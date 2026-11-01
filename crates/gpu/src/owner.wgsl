// The owner layer: the owner map's 16-bit tiles drawn over the scan as
// quads in crop pixel coordinates. A texel equal to one of the highlighted
// nodes' values is tinted (fully for a member of the hovered group, lightly
// for a node that only serves it); every other texel is discarded. Tiles are R16Uint, read
// with textureLoad (never filtered, so a pixel stays a pixel).

struct Camera {
    origin_zoom: vec4<f32>, // crop px at the screen's top-left, then screen px per crop px
    screen: vec4<f32>,      // screen size in px
    tint: vec4<f32>,        // the lit group's colour (rgb) and opacity
    serving_tint: vec4<f32>, // the colour of the nodes that only serve it
    pin_tint: vec4<f32>,     // the colour of the pinned group, where lit apart from the hover
    counts: vec4<u32>,      // x: member values, y: serving values, z: pinned values
    values: array<vec4<u32>, 16>, // tile values (node index + 1): members, then serving
};

const SERVING_ALPHA: f32 = 0.45;

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var tile: texture_2d<u32>;

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
    let size = vec2<f32>(textureDimensions(tile));
    let texel = vec2<i32>(min(in.uv * size, size - vec2<f32>(1.0)));
    let v = textureLoad(tile, texel, 0).r;
    var a = 0.0;
    var rgb = camera.tint.rgb;
    var op = camera.tint.a;
    if (v != 0u) {
        let n = camera.counts.x + camera.counts.y + camera.counts.z;
        for (var i = 0u; i < n; i = i + 1u) {
            if (camera.values[i >> 2u][i & 3u] == v) {
                let serving = i >= camera.counts.x;
                a = select(1.0, SERVING_ALPHA, serving);
                if (i >= camera.counts.x + camera.counts.y) {
                    a = 1.0;
                    rgb = camera.pin_tint.rgb;
                    op = camera.pin_tint.a;
                } else if (serving) {
                    rgb = camera.serving_tint.rgb;
                    op = camera.serving_tint.a;
                }
                break;
            }
        }
    }
    if (a == 0.0) {
        discard;
    }
    return vec4<f32>(rgb, op * a);
}
