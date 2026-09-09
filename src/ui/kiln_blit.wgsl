@group(0) @binding(0) var source:texture_2d<f32>;
@group(0) @binding(1) var tex_sampler:sampler;
struct Out {@builtin(position) p:vec4<f32>,@location(0) uv:vec2<f32>}
@vertex fn vs(@builtin(vertex_index) i:u32)->Out {
    var o:Out;let uv=vec2(f32((i<<1u)&2u),f32(i&2u));o.p=vec4(uv*2.0-1.0,0.0,1.0);o.uv=vec2(uv.x,1.0-uv.y);return o;
}
@fragment fn fs(v:Out)->@location(0) vec4<f32>{return textureSample(source,tex_sampler,v.uv);}
