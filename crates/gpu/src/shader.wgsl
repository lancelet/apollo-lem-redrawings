// Analytic strokes and fills: every instance is one quad covering its
// primitive plus a pixel of margin, and the fragment shader computes the
// exact distance to the primitive, so lines and arcs are true curves at
// any zoom. Lines thinner than a pixel are drawn a pixel wide at
// proportionally lower alpha, which is their true coverage.

struct View {
    // The paper point at the screen's top-left corner, mm; pixels per
    // mm; the device pixel ratio.
    origin_zoom: vec4<f32>,
    // The screen size, px; the thinnest line, px; unused.
    screen: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;

struct Inst {
    // kind (0 segment, 1 arc, 2 triangle, 3 disc), a.x, a.y, weight mm
    @location(0) k_a: vec4<f32>,
    // segment: b.x, b.y, -, floor; arc: radius, start, sweep, floor;
    // triangle: b.x, b.y, c.x, c.y; disc: radius, -, -, -.
    // The floor (segments and arcs only) is a minimum width in px for this
    // instance alone, drawn at full opacity: how a highlight stays visible.
    @location(1) b: vec4<f32>,
    @location(2) rgba: vec4<f32>,
};

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) p: vec2<f32>,
    @location(1) @interpolate(flat) k_a: vec4<f32>,
    @location(2) @interpolate(flat) b: vec4<f32>,
    @location(3) @interpolate(flat) rgba: vec4<f32>,
};

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    let s = (p - view.origin_zoom.xy) * view.origin_zoom.z;
    return vec4<f32>(
        s.x / view.screen.x * 2.0 - 1.0,
        1.0 - s.y / view.screen.y * 2.0,
        0.0,
        1.0,
    );
}

// The stroke's width on the screen, px: its true width, but never thinner
// than the minimum, nor than the instance's own floor.
fn width_px(weight_mm: f32, floor_px: f32) -> f32 {
    return max(max(weight_mm * view.origin_zoom.z, view.screen.z), floor_px);
}

// The instance's floor: only strokes have one.
fn floor_of(kind: f32, b: vec4<f32>) -> f32 {
    return select(0.0, b.w, kind < 1.5);
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, i: Inst) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
        vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    let k = corners[vi];
    let zoom = view.origin_zoom.z;
    let kind = i.k_a.x;
    let a = i.k_a.yz;
    // Half the width plus a pixel and a half, in mm.
    let pad = (width_px(i.k_a.w, floor_of(kind, i.b)) * 0.5 + 1.5) / zoom;
    var p: vec2<f32>;
    if (kind < 0.5) {
        let b = i.b.xy;
        var u = b - a;
        let len = length(u);
        if (len < 1e-9) { u = vec2(1.0, 0.0); } else { u = u / len; }
        let n = vec2(-u.y, u.x);
        p = (a + b) * 0.5 + u * k.x * (len * 0.5 + pad) + n * k.y * pad;
    } else if (kind < 1.5) {
        p = a + k * (i.b.x + pad);
    } else if (kind < 2.5) {
        let lo = min(a, min(i.b.xy, i.b.zw)) - vec2(pad);
        let hi = max(a, max(i.b.xy, i.b.zw)) + vec2(pad);
        p = mix(lo, hi, (k + vec2(1.0)) * 0.5);
    } else {
        p = a + k * (i.b.x + pad);
    }
    var o: Out;
    o.pos = to_clip(p);
    o.p = p;
    o.k_a = i.k_a;
    o.b = i.b;
    o.rgba = i.rgba;
    return o;
}

fn wrap(x: f32, m: f32) -> f32 {
    return x - floor(x / m) * m;
}

// Signed distance to a triangle, negative inside (Inigo Quilez).
fn sd_triangle(p: vec2<f32>, p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>) -> f32 {
    let e0 = p1 - p0; let e1 = p2 - p1; let e2 = p0 - p2;
    let v0 = p - p0; let v1 = p - p1; let v2 = p - p2;
    let pq0 = v0 - e0 * clamp(dot(v0, e0) / dot(e0, e0), 0.0, 1.0);
    let pq1 = v1 - e1 * clamp(dot(v1, e1) / dot(e1, e1), 0.0, 1.0);
    let pq2 = v2 - e2 * clamp(dot(v2, e2) / dot(e2, e2), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(
        min(
            vec2(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)),
            vec2(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x)),
        ),
        vec2(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x)),
    );
    return -sqrt(d.x) * sign(d.y);
}

// Distance from p to an arc: to the ring within the sweep, else to the
// nearer end.
fn arc_distance(p: vec2<f32>, c: vec2<f32>, r: f32, start: f32, sweep: f32) -> f32 {
    let tau = 6.283185307179586;
    let q = p - c;
    let ang = atan2(q.y, q.x);
    var inside: bool;
    if (abs(sweep) >= tau - 1e-5) {
        inside = true;
    } else if (sweep >= 0.0) {
        inside = wrap(ang - start, tau) <= sweep;
    } else {
        inside = wrap(start - ang, tau) <= -sweep;
    }
    if (inside) {
        return abs(length(q) - r);
    }
    let e0 = c + r * vec2(cos(start), sin(start));
    let e1 = c + r * vec2(cos(start + sweep), sin(start + sweep));
    return min(length(p - e0), length(p - e1));
}

@fragment
fn fs(o: Out) -> @location(0) vec4<f32> {
    let zoom = view.origin_zoom.z;
    let kind = o.k_a.x;
    let a = o.k_a.yz;
    var coverage: f32;
    if (kind < 1.5) {
        var d: f32;
        if (kind < 0.5) {
            let b = o.b.xy;
            let ab = b - a;
            let t = clamp(dot(o.p - a, ab) / max(dot(ab, ab), 1e-12), 0.0, 1.0);
            d = length(o.p - (a + ab * t));
        } else {
            d = arc_distance(o.p, a, o.b.x, o.b.y, o.b.z);
        }
        let true_px = o.k_a.w * zoom;
        let floor_px = floor_of(kind, o.b);
        let drawn_px = width_px(o.k_a.w, floor_px);
        // A sub-pixel line is drawn a pixel wide at its true coverage; an
        // instance with its own floor is drawn at full opacity.
        var thin = min(true_px / max(drawn_px, 1e-6), 1.0);
        if (floor_px > 0.0) {
            thin = 1.0;
        }
        coverage = clamp(drawn_px * 0.5 + 0.5 - d * zoom, 0.0, 1.0) * thin;
    } else if (kind < 2.5) {
        let sd = sd_triangle(o.p, a, o.b.xy, o.b.zw) * zoom;
        coverage = clamp(0.5 - sd, 0.0, 1.0);
    } else {
        let sd = (length(o.p - a) - o.b.x) * zoom;
        coverage = clamp(0.5 - sd, 0.0, 1.0);
    }
    if (coverage <= 0.0) {
        discard;
    }
    return vec4<f32>(o.rgba.rgb, o.rgba.a * coverage);
}
