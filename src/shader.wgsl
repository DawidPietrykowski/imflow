struct Transforms {
    transform: mat4x4<f32>,
    width: u32,
    height: u32
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

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let texture_size = vec2<f32>(f32(transforms.width), f32(transforms.height));
    let out_dim = vec2<f32>(textureDimensions(texture));
    let scale = texture_size / out_dim;
    let pixel = uv * scale;
    return textureSample(texture, texture_sampler, pixel);
}
