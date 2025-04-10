struct Transforms {
    transform: mat4x4<f32>,
    width: u32,
    height: u32,
    orientation: u32
};
@group(0) @binding(2) var<uniform> transforms: Transforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = transforms.transform * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    return out;
}

@group(0) @binding(0) var texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

fn reverse(in: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(in.y, in.x);
}

@fragment
fn fs_main(@location(0) in: vec2<f32>) -> @location(0) vec4<f32> {
    var texture_size = vec2<f32>(f32(transforms.width), f32(transforms.height));
    let out_dim = vec2<f32>(textureDimensions(texture));
    var uv = in;
    if transforms.orientation == 2 {
        uv.x = 1.0-uv.x;
    } else if transforms.orientation == 3 {
        uv.y = 1.0-uv.y;
    }
    let scale = texture_size / out_dim;
    var pixel = uv * scale;

    // add offset to remove bleed from uncleared buffer
    let half_texel = vec2<f32>(0.5) / out_dim;
    let min_uv = half_texel;
    let max_uv = scale - half_texel;
    pixel = clamp(pixel, min_uv, max_uv);

    if transforms.orientation == 3 {
        pixel = reverse(pixel);
    }
    return textureSample(texture, texture_sampler, pixel);
}
