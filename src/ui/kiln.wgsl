struct Camera { vp:mat4x4<f32>, eye:vec4<f32>, background:vec4<f32> }
@group(0) @binding(0) var<uniform> camera:Camera;
struct In { @location(0) position:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) uv:vec2<f32>, @location(3) material:f32, @location(4) displacement:f32 }
struct Out { @builtin(position) clip:vec4<f32>, @location(0) world:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) uv:vec2<f32>, @location(3) @interpolate(flat) material:f32, @location(4) displacement:f32 }
@vertex fn vs(v:In)->Out {var o:Out;o.clip=camera.vp*vec4(v.position,1.0);o.world=v.position;o.normal=v.normal;o.uv=v.uv;o.material=v.material;o.displacement=v.displacement;return o;}
@fragment fn fs(v:Out,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
    if v.material>=4.5 && v.material<5.5 && !front { discard; }
    let n=normalize(v.normal)*select(-1.0,1.0,front);
    let eye=normalize(camera.eye.xyz-v.world);
    let key=normalize(vec3(-3.0,5.0,4.0));let rim=normalize(vec3(3.0,2.0,-3.0));
    var base=vec3(0.11,0.15,0.14);var metal=0.85;var rough=0.25;
    if v.material>8.5 {
        let shade=0.12+0.14*abs(dot(n,key));
        return vec4(mix(camera.background.rgb,vec3(0.08,0.38,0.34),shade),1.0);
    } else if v.material>5.5 {
        base=select(select(vec3(0.09,0.66,0.56),vec3(0.24,0.43,0.95),v.material>7.5),vec3(0.98,0.44,0.11),v.material<6.5);
        metal=0.3;rough=0.3;
    } else if v.material<0.5 {
        let up=vec3(0.95,0.39,0.075);let down=vec3(0.03,0.42,0.42);
        base=mix(vec3(0.45,0.49,0.43),select(down,up,v.displacement>0.0),clamp(abs(v.displacement)*7.0,0.0,0.9));
        let ring=abs(fract(length(v.uv)*24.0)-0.5);
        let radial=abs(fract(atan2(v.uv.y,v.uv.x)*15.2788745)-0.5);
        let line=1.0-smoothstep(0.02,0.065,min(ring,radial));
        base*=1.0-line*0.18;metal=0.06;rough=0.7;
    } else if v.material<1.5 {
        base=vec3(0.10,0.19,0.17)*(0.88+0.12*sin(v.world.y*190.0));
    } else if v.material<2.5 {base=vec3(0.48,0.51,0.47);rough=0.18;
    } else if v.material<3.5 {base=vec3(0.74,0.35,0.09);metal=0.25;rough=0.5;
    } else if v.material<4.5 {base=vec3(0.50,0.37,0.19);rough=0.27;
    } else {
        let grid=abs(fract(v.world.xz*3.0)-0.5);
        let lines=1.0-smoothstep(0.01,0.04,min(grid.x,grid.y));
        let shadow=1.0-0.72*exp(-dot(v.world.xz,v.world.xz)*1.4);
        let bg=camera.background.rgb;
        return vec4(bg*(1.05+lines*0.28)*shadow,1.0);
    }
    let diffuse=max(dot(n,key),0.0);let back=max(dot(n,rim),0.0);
    let halfv=normalize(key+eye);let spec=pow(max(dot(n,halfv),0.0),mix(120.0,12.0,rough));
    let fresnel=pow(1.0-max(dot(n,eye),0.0),4.0);
    let ambient=0.27+0.16*max(n.y,0.0);
    var colour=base*(ambient+diffuse*0.9+back*vec3(0.13,0.34,0.29));
    colour+=mix(vec3(0.12),base,metal)*spec*2.3+fresnel*vec3(0.09,0.14,0.13);
    colour=colour/(vec3(1.0)+colour*0.4);
    return vec4(colour,1.0);
}
