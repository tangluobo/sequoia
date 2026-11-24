override SRGB_FRAMEBUFFER: bool = false;
@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var u_plane: texture_2d<f32>;
@group(0) @binding(2) var v_plane: texture_2d<f32>;
@group(0) @binding(3) var plane_sampler: sampler;
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let p = array<vec2<f32>,3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0))[index];
    var out: VertexOutput;
    out.position = vec4(p,0.0,1.0);
    out.uv = vec2((p.x+1.0)*0.5,(1.0-p.y)*0.5);
    return out;
}
fn linear_from_srgb(v: vec3<f32>) -> vec3<f32> {
    return select(pow((v+vec3(0.055))/1.055,vec3(2.4)), v/12.92, v <= vec3(0.04045));
}
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Texel-centred sampling preserves 1:1 luma and reconstructs half-resolution
    // chroma smoothly. Filtering runs on the GPU without another frame buffer.
    let y = textureSampleLevel(y_plane,plane_sampler,in.uv,0.0).r*255.0-16.0;
    let u = textureSampleLevel(u_plane,plane_sampler,in.uv,0.0).r*255.0-128.0;
    let v = textureSampleLevel(v_plane,plane_sampler,in.uv,0.0).r*255.0-128.0;
    let base = y*(255.0/219.0);
    var rgb = clamp(vec3(base+v*(255.0/224.0*1.402),
        base+v*(-255.0/224.0*1.402*0.299/0.587)+u*(-255.0/224.0*1.772*0.114/0.587),
        base+u*(255.0/224.0*1.772))/255.0,vec3(0.0),vec3(1.0));
    if SRGB_FRAMEBUFFER { rgb = linear_from_srgb(rgb); }
    return vec4(rgb,1.0);
}
