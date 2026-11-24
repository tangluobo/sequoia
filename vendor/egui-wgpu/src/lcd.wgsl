// The second output supplies independent RGB destination coverage. This keeps
// black, white, disabled and colored text correct on every actual background.
struct LcdOutput {
    @location(0) @blend_src(0) color: vec4<f32>,
    @location(0) @blend_src(1) coverage: vec4<f32>,
};

@fragment
fn fs_lcd_gamma_framebuffer(in: VertexOutput) -> LcdOutput {
    let mask = sample_texture(in);
    var out: LcdOutput;
    out.color = in.color * mask;
    out.coverage = vec4<f32>(mask.rgb * in.color.a, mask.a * in.color.a);
    if r_locals.font_gamma != 1.0 && in.color.a > 0.0 &&
        any(mask.rgb > vec3<f32>(0.0)) && any(mask.rgb < vec3<f32>(1.0)) {
        let text = in.color.rgb / in.color.a;
        let gamma = vec3<f32>(r_locals.font_gamma);
        // Interpolate the contrast-correct results on black and white using
        // dual-source blending. Black text is exact on every background.
        let on_black = text * pow(mask.rgb, gamma);
        let on_white = pow(mix(vec3<f32>(1.0), pow(text, vec3<f32>(1.0) / gamma), mask.rgb), gamma);
        out.color = vec4<f32>(on_black * in.color.a, out.color.a);
        out.coverage = vec4<f32>((1.0 - on_white + on_black) * in.color.a, out.coverage.a);
    }
    return out;
}
