struct Params {
    size: vec2<u32>,
    mode: u32,
    horizontal: u32,
    strength: f32,
    angle: f32,
    detail: f32,
    passes: u32,
    blades: u32,
    pass_index: u32,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: Params;

const PI: f32 = 3.141592653589793;
const TAU: f32 = 6.283185307179586;

fn pixel(x: i32, y: i32) -> vec4<f32> {
    let edge = vec2<i32>(params.size) - vec2<i32>(1);
    return textureLoad(source, clamp(vec2<i32>(x, y), vec2<i32>(0), edge), 0);
}

fn quantise(value: vec4<f32>) -> vec4<f32> {
    return round(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0) / 255.0;
}

fn turned(x: f32, y: f32) -> vec2<f32> {
    let radians = params.angle * PI / 180.0;
    let sine = sin(radians);
    let cosine = cos(radians);
    return vec2<f32>(x * cosine - y * sine, x * sine + y * cosine);
}

fn round_away(value: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        select(ceil(value.x - 0.5), floor(value.x + 0.5), value.x >= 0.0),
        select(ceil(value.y - 0.5), floor(value.y + 0.5), value.y >= 0.0),
    );
}

@compute @workgroup_size(64)
fn box_blur(@builtin(global_invocation_id) id: vec3<u32>) {
    let horizontal = params.horizontal != 0u;
    let lines = select(params.size.x, params.size.y, horizontal);
    if (id.x >= lines) {
        return;
    }

    let length = i32(select(params.size.y, params.size.x, horizontal));
    let line = i32(id.x);
    let radius = i32(round(params.strength));
    let weight = 1.0 / f32(radius * 2 + 1);
    var sum = vec4<f32>(0.0);

    for (var offset = -radius; offset <= radius; offset += 1) {
        if (horizontal) {
            sum += pixel(offset, line);
        } else {
            sum += pixel(line, offset);
        }
    }

    for (var at = 0; at < length; at += 1) {
        if (horizontal) {
            textureStore(output_tex, vec2<i32>(at, line), quantise(sum * weight));
            sum += pixel(at + radius + 1, line) - pixel(at - radius, line);
        } else {
            textureStore(output_tex, vec2<i32>(line, at), quantise(sum * weight));
            sum += pixel(line, at + radius + 1) - pixel(line, at - radius);
        }
    }
}

fn median9(at: vec2<i32>) -> vec4<f32> {
    var values: array<vec4<f32>, 9>;
    var index = 0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            values[index] = pixel(at.x + x, at.y + y);
            index += 1;
        }
    }
    for (var end = 8; end > 0; end -= 1) {
        for (var i = 0; i < end; i += 1) {
            let low = min(values[i], values[i + 1]);
            let high = max(values[i], values[i + 1]);
            values[i] = low;
            values[i + 1] = high;
        }
    }
    return values[4];
}

fn motion(at: vec2<i32>) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    for (var step = 0; step < 17; step += 1) {
        let along = (f32(step) / 16.0 - 0.5) * params.strength;
        let offset = round_away(turned(along, 0.0));
        sum += pixel(at.x + i32(offset.x), at.y + i32(offset.y)) / 17.0;
    }
    return sum;
}

fn bilateral(at: vec2<i32>) -> vec4<f32> {
    let centre = pixel(at.x, at.y);
    let spacing = params.strength / 2.0;
    let looseness = 1.0 - params.detail;
    let range = (12.0 + looseness * looseness * 500.0) / 255.0;
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var y = -2; y <= 2; y += 1) {
        for (var x = -2; x <= 2; x += 1) {
            let offset = round_away(vec2<f32>(f32(x), f32(y)) * spacing);
            let value = pixel(at.x + i32(offset.x), at.y + i32(offset.y));
            let spatial = exp(-f32(x * x + y * y) / 4.0);
            let difference = length(value.rgb - centre.rgb);
            let weight = spatial * exp(-(difference * difference) / (2.0 * range * range));
            sum += value * weight;
            total += weight;
        }
    }
    return sum / total;
}

fn directional(at: vec2<i32>) -> vec4<f32> {
    let major = params.strength / 2.0;
    let minor = major * max(1.0 - params.detail, 0.05);
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var y = -2; y <= 2; y += 1) {
        for (var x = -2; x <= 2; x += 1) {
            let weight = exp(-f32(x * x + y * y) / 4.0);
            let offset = round_away(turned(f32(x) * major / 2.0, f32(y) * minor / 2.0));
            sum += pixel(at.x + i32(offset.x), at.y + i32(offset.y)) * weight;
            total += weight;
        }
    }
    return sum / total;
}

fn defocus(at: vec2<i32>) -> vec4<f32> {
    var sum = pixel(at.x, at.y);
    let rotation = params.angle * PI / 180.0;
    let blades = f32(params.blades);
    for (var sample_index = 0; sample_index < 24; sample_index += 1) {
        let inner = sample_index < 8;
        let ring = select(1.0, 0.5, inner);
        let around = select(16.0, 8.0, inner);
        let index = select(sample_index - 8, sample_index, inner);
        let angle = f32(index) * TAU / around + rotation;
        let sector = fract(angle * blades / TAU + 0.5) - 0.5;
        let aperture = cos(PI / blades) / cos(sector * TAU / blades);
        let radius = params.strength * ring * aperture;
        let offset = round_away(vec2<f32>(cos(angle), sin(angle)) * radius);
        sum += pixel(at.x + i32(offset.x), at.y + i32(offset.y));
    }
    return sum / 25.0;
}

fn kawase(at: vec2<i32>) -> vec4<f32> {
    let offset = i32(round(params.strength * f32(params.pass_index + 1u) / f32(params.passes)));
    return (
        pixel(at.x - offset, at.y - offset)
        + pixel(at.x + offset, at.y - offset)
        + pixel(at.x - offset, at.y + offset)
        + pixel(at.x + offset, at.y + offset)
    ) * 0.25;
}

@compute @workgroup_size(8, 8)
fn effect_blur(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.size.x || id.y >= params.size.y) {
        return;
    }
    let at = vec2<i32>(id.xy);
    var value = pixel(at.x, at.y);
    switch params.mode {
        case 1u: { value = median9(at); }
        case 2u: { value = motion(at); }
        case 3u: { value = bilateral(at); }
        case 4u: { value = directional(at); }
        case 5u: { value = defocus(at); }
        case 6u: { value = kawase(at); }
        default: {}
    }
    textureStore(output_tex, at, quantise(value));
}
