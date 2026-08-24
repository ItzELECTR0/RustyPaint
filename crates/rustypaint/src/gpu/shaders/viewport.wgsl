
struct Uniforms {
    viewport_size: vec2<f32>,
    canvas_pos: vec2<f32>,
    canvas_size: vec2<f32>,
    texture_size: vec2<f32>,
    workspace_top: vec4<f32>,
    workspace_bottom: vec4<f32>,
    checker_light: vec4<f32>,
    checker_dark: vec4<f32>,
    zoom: f32,
    checker_size: f32,
    srgb_target: f32,
    show_canvas: f32,
    preview: vec4<f32>,
    handles: f32,
    hot_handle: f32,
    backing: f32,
    shadow: f32,
    float_centre: vec2<f32>,
    float_half: vec2<f32>,
    float_rotation: f32,
    float_present: f32,
    ants: f32,
    float_handles: f32,
    float_hot: f32,
    float_reach: f32,
    curve_count: f32,
    float_opacity: f32,
    curve_points: array<vec4<f32>, 12>,
    accent: vec4<f32>,
    float_masked: f32,
    brush_ring: vec4<f32>,
    crop: vec4<f32>,
    marquee: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var canvas_tex: texture_2d<f32>;
@group(0) @binding(2) var canvas_sampler: sampler;
@group(0) @binding(3) var float_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    let uv = corners[index];

    var out: VsOut;
    out.clip = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn sd_box(p: vec2<f32>, half_size: vec2<f32>) -> f32 {
    let d = abs(p) - half_size;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

const SHADOW_BLUR: f32 = 6.0;
const SHADOW_OFFSET: f32 = 2.0;

const HANDLE_HALF: f32 = 5.0;
const HANDLE_FILL: vec3<f32> = vec3<f32>(1.0, 1.0, 1.0);
const DIAL_RADIUS: f32 = 8.0;
const POINT_RADIUS: f32 = 5.0;

fn handle_centre(i: i32) -> vec2<f32> {
    return grip_of(u.canvas_pos, u.canvas_size, i);
}

fn grip_of(pos: vec2<f32>, size: vec2<f32>, i: i32) -> vec2<f32> {
    let a = pos;
    let b = pos + size;
    let m = pos + size * 0.5;
    switch i {
        case 0: { return vec2<f32>(a.x, a.y); }
        case 1: { return vec2<f32>(b.x, a.y); }
        case 2: { return vec2<f32>(a.x, b.y); }
        case 3: { return vec2<f32>(b.x, b.y); }
        case 4: { return vec2<f32>(m.x, a.y); }
        case 5: { return vec2<f32>(m.x, b.y); }
        case 6: { return vec2<f32>(a.x, m.y); }
        default: { return vec2<f32>(b.x, m.y); }
    }
}

fn draw_handles(base: vec3<f32>, local: vec2<f32>) -> vec3<f32> {
    var colour = base;
    for (var i = 0; i < 8; i = i + 1) {
        let d = sd_box(local - handle_centre(i), vec2<f32>(HANDLE_HALF));
        if (d <= 0.0) {
            let hot = u.hot_handle == f32(i);
            let fill = select(HANDLE_FILL, u.accent.rgb, hot);
            colour = select(fill, u.accent.rgb, d > -1.5);
        }
    }
    return colour;
}

fn float_grip(i: i32) -> vec2<f32> {
    var d = vec2<f32>(0.0);
    switch i {
        case 0: { d = vec2<f32>(-1.0, -1.0); }
        case 1: { d = vec2<f32>( 1.0, -1.0); }
        case 2: { d = vec2<f32>(-1.0,  1.0); }
        case 3: { d = vec2<f32>( 1.0,  1.0); }
        case 4: { d = vec2<f32>( 0.0, -1.0); }
        case 5: { d = vec2<f32>( 0.0,  1.0); }
        case 6: { d = vec2<f32>(-1.0,  0.0); }
        case 7: { d = vec2<f32>( 1.0,  0.0); }
        default: { d = vec2<f32>(0.0, -1.0); }
    }
    var offset = d * u.float_half;
    if (i == 8) {
        offset.y = offset.y - u.float_reach;
    }
    let s = sin(u.float_rotation);
    let c = cos(u.float_rotation);
    let r = vec2<f32>(offset.x * c - offset.y * s, offset.x * s + offset.y * c);
    return u.float_centre + r;
}

fn draw_float_handles(base: vec3<f32>, local: vec2<f32>) -> vec3<f32> {
    var colour = base;
    for (var i = 0; i < 8; i = i + 1) {
        let d = sd_box(local - float_grip(i), vec2<f32>(HANDLE_HALF));
        if (d <= 0.0) {
            let fill = select(HANDLE_FILL, u.accent.rgb, u.float_hot == f32(i));
            colour = select(fill, u.accent.rgb, d > -1.5);
        }
    }
    let dial = length(local - float_grip(8)) - DIAL_RADIUS;
    if (dial <= 0.0) {
        let fill = select(HANDLE_FILL, u.accent.rgb, u.float_hot == 8.0);
        colour = select(fill, u.accent.rgb, dial > -1.5);
    }
    return colour;
}

fn draw_curve_points(base: vec3<f32>, local: vec2<f32>) -> vec3<f32> {
    var colour = base;
    let count = i32(u.curve_count);
    for (var i = 0; i < count; i = i + 1) {
        let pair = u.curve_points[i / 2];
        let p = select(pair.xy, pair.zw, (i % 2) == 1);
        let d = length(local - p) - POINT_RADIUS;
        if (d <= 0.0) {
            let fill = select(HANDLE_FILL, u.accent.rgb, u.float_hot == f32(9 + i));
            colour = select(fill, u.accent.rgb, d > -1.5);
        }
    }
    return colour;
}

fn draw_brush_ring(base: vec3<f32>, local: vec2<f32>) -> vec3<f32> {
    let d = abs(length(local - u.brush_ring.xy) - u.brush_ring.z);
    let core = 1.0 - smoothstep(0.0, 0.6, d);
    let halo = (1.0 - smoothstep(0.5, 1.3, d)) * 0.35;
    let shaded = mix(base, vec3<f32>(0.0), halo);
    return mix(shaded, vec3<f32>(1.0), core);
}

fn draw_preview(base: vec3<f32>, local: vec2<f32>) -> vec3<f32> {
    let centre = u.preview.xy + u.preview.zw * 0.5;
    let d = sd_box(local - centre, u.preview.zw * 0.5);
    if (abs(d) <= 1.0) {
        return u.accent.rgb;
    }
    return base;
}

fn canvas_probe(local: vec2<f32>) -> vec2<f32> {
    if (u.zoom < 1.0) {
        return local;
    }
    let image = (local - u.canvas_pos) / u.canvas_size * u.texture_size;
    return u.canvas_pos + (floor(image) + 0.5) / u.texture_size * u.canvas_size;
}

fn float_texel(uv: vec2<f32>) -> vec2<f32> {
    let dims = vec2<f32>(textureDimensions(float_tex));
    let spread = u.float_half * 2.0 / max(dims, vec2<f32>(1.0));
    if (min(spread.x, spread.y) < 1.0) {
        return uv;
    }
    return (floor(uv * dims) + 0.5) / dims;
}

fn float_local(local: vec2<f32>) -> vec2<f32> {
    let d = local - u.float_centre;
    let s = sin(-u.float_rotation);
    let c = cos(-u.float_rotation);
    let r = vec2<f32>(d.x * c - d.y * s, d.x * s + d.y * c);
    return r / max(u.float_half * 2.0, vec2<f32>(0.001)) + vec2<f32>(0.5);
}

fn draw_crop(base: vec3<f32>, local: vec2<f32>) -> vec3<f32> {
    var colour = base;
    let pos = u.crop.xy;
    let size = u.crop.zw;
    let outside = sd_box(local - pos - size * 0.5, size * 0.5) > 0.0;
    let on_canvas = sd_box(local - u.canvas_pos - u.canvas_size * 0.5, u.canvas_size * 0.5) <= 0.0;
    if (outside && on_canvas) {
        colour = colour * 0.45;
    }

    let edge = abs(sd_box(local - pos - size * 0.5, size * 0.5));
    if (edge <= 1.0) {
        colour = vec3<f32>(1.0);
    }

    for (var i = 0; i < 8; i = i + 1) {
        let d = length(local - grip_of(pos, size, i)) - GRIP_RADIUS;
        if (d <= 0.0) {
            let hot = u.hot_handle == f32(i);
            colour = select(select(vec3<f32>(1.0), u.accent.rgb, hot), vec3<f32>(0.13), d > -1.5);
        }
    }
    return colour;
}

const GRIP_RADIUS: f32 = 6.5;

const DASH: f32 = 6.0;

fn ants_box(
    base: vec3<f32>,
    local: vec2<f32>,
    centre: vec2<f32>,
    half: vec2<f32>,
    rotation: f32,
    phase: f32,
) -> vec3<f32> {
    let d = local - centre;
    let s = sin(-rotation);
    let c = cos(-rotation);
    let r = vec2<f32>(d.x * c - d.y * s, d.x * s + d.y * c);
    if (abs(sd_box(r, half)) > 1.0) {
        return base;
    }

    let h = half;
    let side = vec2<f32>(h.x - abs(r.x), h.y - abs(r.y));
    var along: f32;
    if (side.y <= side.x) {
        if (r.y < 0.0) {
            along = r.x + h.x;
        } else {
            along = 2.0 * h.x + 2.0 * h.y + (h.x - r.x);
        }
    } else {
        if (r.x > 0.0) {
            along = 2.0 * h.x + (r.y + h.y);
        } else {
            along = 4.0 * h.x + 2.0 * h.y + (h.y - r.y);
        }
    }

    let perimeter = 4.0 * (h.x + h.y);
    let count = max(round(perimeter / (DASH * 2.0)), 1.0);
    return dash_along(along, perimeter / count, phase);
}

fn marching_ants(base: vec3<f32>, local: vec2<f32>) -> vec3<f32> {
    return ants_box(
        base, local, u.float_centre, u.float_half, u.float_rotation, u.ants
    );
}

fn mask_ants(base: vec3<f32>, local: vec2<f32>) -> vec3<f32> {
    let uv = float_local(local);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return base;
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(float_tex));
    let here = textureSampleLevel(float_tex, canvas_sampler, uv, 0.0).a;
    if (here <= 0.5) {
        return base;
    }
    let left = textureSampleLevel(float_tex, canvas_sampler, uv - vec2<f32>(texel.x, 0.0), 0.0).a;
    let right = textureSampleLevel(float_tex, canvas_sampler, uv + vec2<f32>(texel.x, 0.0), 0.0).a;
    let up = textureSampleLevel(float_tex, canvas_sampler, uv - vec2<f32>(0.0, texel.y), 0.0).a;
    let down = textureSampleLevel(float_tex, canvas_sampler, uv + vec2<f32>(0.0, texel.y), 0.0).a;
    let open = uv.x < texel.x || uv.x > 1.0 - texel.x || uv.y < texel.y || uv.y > 1.0 - texel.y;
    if (!open && left > 0.5 && right > 0.5 && up > 0.5 && down > 0.5) {
        return base;
    }
    return dash(local.x + local.y);
}

fn dash(along: f32) -> vec3<f32> {
    return dash_along(along, DASH * 2.0, u.ants);
}

fn dash_along(along: f32, period: f32, phase: f32) -> vec3<f32> {
    let p = max(period, 0.001);
    let at = fract((along - phase) / p) * p;
    let half = p * 0.5;

    let lit = at < half;
    let edge = select(-min(at - half, p - at), min(at, half - at), lit);
    return vec3<f32>(smoothstep(-0.5, 0.5, edge));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c <= vec3<f32>(0.04045);
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, cutoff);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let local = in.uv * u.viewport_size;
    let centre = u.canvas_pos + u.canvas_size * 0.5;
    let half_size = u.canvas_size * 0.5;

    var colour = mix(u.workspace_top.rgb, u.workspace_bottom.rgb, in.uv.y);

    let shadow_d = sd_box(local - centre - vec2<f32>(0.0, SHADOW_OFFSET), half_size);
    let shadow =
        (1.0 - smoothstep(-SHADOW_BLUR, SHADOW_BLUR, shadow_d)) * u.shadow * u.show_canvas;
    colour = colour * (1.0 - shadow);

    let inside = sd_box(local - centre, half_size);
    if (inside <= 0.0 && u.show_canvas > 0.5) {
        let square = (local - u.canvas_pos) / u.checker_size;
        let parity = (floor(square.x) + floor(square.y)) % 2.0;
        let checker = select(u.checker_light.rgb, u.checker_dark.rgb, parity >= 1.0);
        let behind = select(checker, vec3<f32>(1.0), u.backing > 0.5);

        var uv = (local - u.canvas_pos) / u.canvas_size;
        if (u.zoom >= 1.0) {
            uv = (floor(uv * u.texture_size) + 0.5) / u.texture_size;
        }
        let texel = textureSample(canvas_tex, canvas_sampler, uv);
        colour = mix(behind, texel.rgb, texel.a);
    }

    if (u.float_present > 0.5) {
        let uv = float_local(canvas_probe(local));
        if (uv.x >= 0.0 && uv.x < 1.0 && uv.y >= 0.0 && uv.y < 1.0) {
            let texel = textureSampleLevel(float_tex, canvas_sampler, float_texel(uv), 0.0);
            colour = mix(colour, texel.rgb, texel.a * u.float_opacity);
        }
        if (u.float_masked > 0.5) {
            colour = mask_ants(colour, local);
        } else {
            colour = marching_ants(colour, local);
        }
        if (u.float_handles > 0.5) {
            colour = draw_float_handles(colour, local);
        }
        colour = draw_curve_points(colour, local);
    }

    if (u.handles > 0.5) {
        colour = draw_handles(colour, local);
    }
    if (u.crop.z >= 0.0) {
        colour = draw_crop(colour, local);
    }
    if (u.brush_ring.w > 0.5) {
        colour = draw_brush_ring(colour, local);
    }
    if (u.preview.z >= 0.0) {
        colour = draw_preview(colour, local);
    }
    if (u.marquee.z >= 0.0) {
        let half = u.marquee.zw * 0.5;
        colour = ants_box(colour, local, u.marquee.xy + half, half, 0.0, 0.0);
    }

    if (u.srgb_target > 0.5) {
        colour = srgb_to_linear(colour);
    }
    return vec4<f32>(colour, 1.0);
}
