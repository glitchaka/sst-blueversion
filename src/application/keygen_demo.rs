//! SST procedural demoscene stage: original geometry, no external media.
use super::VisualBeat;
use sdl2::{pixels::Color,rect::{FPoint,Rect},render::{Canvas,RenderTarget,Vertex,VertexIndices}};
use std::f32::consts::TAU;
type V=[f32;3];
type Rgb=[f32;3];
struct Face{p:[V;3],rgb:Rgb,alpha:f32}
fn sub(a:V,b:V)->V{[a[0]-b[0],a[1]-b[1],a[2]-b[2]]}
fn cross(a:V,b:V)->V{[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]}
fn dot(a:V,b:V)->f32{a[0]*b[0]+a[1]*b[1]+a[2]*b[2]}
fn unit(a:V)->V{let n=dot(a,a).sqrt().max(0.0001);a.map(|v|v/n)}
fn mix(a:Rgb,b:Rgb,t:f32)->Rgb{std::array::from_fn(|i|a[i]+(b[i]-a[i])*t)}
fn rotate([x,y,z]:V,a:f32,b:f32)->V{let(sa,ca)=a.sin_cos();let(sb,cb)=b.sin_cos();let(x,z)=(x*ca+z*sa,-x*sa+z*ca);[x,y*cb-z*sb,y*sb+z*cb]}
fn smooth(x:f32)->f32{let x=x.clamp(0.0,1.0);x*x*(3.0-2.0*x)}
fn palette(c:u8)->Rgb{match c{0=>[96.,145.,180.],1=>[65.,180.,160.],2=>[200.,148.,85.],3=>[136.,126.,207.],_=>[198.,183.,142.]}}
fn noise(n:u32)->f32{let x=n.wrapping_mul(747796405).wrapping_add(2891336453);let x=((x>>((x>>28)+4))^x).wrapping_mul(277803737);((x>>22)^x) as f32/u32::MAX as f32}
#[derive(Default)]
pub(super) struct Stage{faces:Vec<Face>,projected:Vec<(f32,[V;3],Color)>,vertices:Vec<Vertex>}
impl Stage{
    fn tri(&mut self,p:[V;3],rgb:Rgb,alpha:f32){self.faces.push(Face{p,rgb,alpha});}
    fn quad(&mut self,p:[V;4],rgb:Rgb,a:f32){self.tri([p[0],p[1],p[2]],rgb,a);self.tri([p[0],p[2],p[3]],rgb,a);}
    fn ribbons(&mut self,t:f32,pulse:f32,rgb:Rgb,w:f32){
        if w<0.01{return;}
        for band in 0..3{
            let point=|i:usize,edge:f32|{
                let a=i as f32/56.*TAU;let phase=t*0.38+band as f32*TAU/3.;
                let r=190.+band as f32*44.+18.*(a*3.+phase).sin()+pulse*15.;
                let twist=a*2.+phase;
                rotate([(r+edge*20.*twist.cos())*a.cos(),(r+edge*20.*twist.cos())*a.sin()*0.54,
                    48.*(a*2.+phase).sin()+edge*20.*twist.sin()+band as f32*25.],phase*0.45,0.30*(phase+band as f32).sin())
            };
            for i in 0..56{let material=mix(rgb,[195.,201.,208.],0.30+0.23*(i as f32*0.3+t).sin());
                self.quad([point(i,-1.),point(i+1,-1.),point(i+1,1.),point(i,1.)],material,w*0.8);}
        }
    }
    fn tunnel(&mut self,t:f32,pulse:f32,rgb:Rgb,w:f32){
        if w<0.01{return;}
        let ring=|j:usize,k:usize|{
            let z=85.+j as f32*78.-(t*90.).rem_euclid(78.);
            let a=k as f32/18.*TAU+t*0.22+z*0.003;
            let r=220.+32.*(z*0.012+t*0.9).sin()+pulse*18.;
            [r*a.cos()+45.*(z*0.004+t*0.5).sin(),r*a.sin()*0.65,z]
        };
        for j in 0..10{for k in 0..18{if(j+k)%3==0{continue;}let color=if(j+k)%2==0{rgb}else{mix(rgb,[20.,34.,48.],0.55)};
            self.quad([ring(j,k),ring(j,k+1),ring(j+1,k+1),ring(j+1,k)],color,w*0.63);}}
    }
    fn lattice(&mut self,t:f32,pulse:f32,rgb:Rgb,w:f32){
        if w<0.01{return;}
        let point=|i:usize,j:usize|{let x=(i as f32-9.)*51.;let z=80.+j as f32*65.;
            [x,115.+(x*0.012+t*1.6).sin()*(z*0.009-t*0.8).cos()*(28.+45.*pulse),z]};
        for i in 0..18{for j in 0..9{
            let a=point(i,j);let b=point(i+1,j);let c=point(i+1,j+1);let d=point(i,j+1);
            let material=mix(rgb,[146.,171.,182.],0.30+0.15*((i+j) as f32+t).sin());
            self.quad([a,b,c,d],material,w*0.72);
            if j<5{let reflect=|p:V|[p[0],-p[1],p[2]];self.quad([reflect(a),reflect(d),reflect(c),reflect(b)],material,w*0.18);}
        }}
    }
    fn crystals(&mut self,t:f32,pulse:f32,rgb:Rgb,w:f32){
        if w<0.01{return;}
        for i in 0..48u32{
            let side=if i%2==0{-1.}else{1.};
            let a=t*(0.6+noise(i)*0.7)+i as f32*0.31;
            let center=[side*(170.+(i/2%6) as f32*54.),-108.+(i/12) as f32*67.+(a*1.2).sin()*20.,35.+noise(i+97)*160.];
            let radius=9.+10.*noise(i+19)+pulse*4.;
            let point=|p:V|{let p=rotate(p.map(|x|x*radius),a,a*0.7);std::array::from_fn(|j|center[j]+p[j])};
            let p=[[1.,0.,0.],[0.,1.,0.],[-1.,0.,0.],[0.,-1.,0.],[0.,0.,1.],[0.,0.,-1.]].map(point);
            let hue=mix(rgb,[196.,176.,118.],0.5+0.4*a.sin());
            for j in 0..4{self.tri([p[j],p[(j+1)%4],p[4]],hue,w*0.9);self.tri([p[(j+1)%4],p[j],p[5]],hue,w*0.9);}
        }
    }
    fn project(&mut self,t:f32,pulse:f32){
        self.projected.clear();self.vertices.clear();
        let light=unit([(t*0.65).sin(),-0.65,-0.8]);
        for f in &self.faces{
            let normal=unit(cross(sub(f.p[1],f.p[0]),sub(f.p[2],f.p[0])));
            let diffuse=dot(normal,light).abs();let spec=diffuse.powi(12);
            let rgb=f.rgb.map(|v|(v*(0.23+0.77*diffuse)+spec*(85.+35.*pulse)).clamp(0.,255.) as u8);
            let p=f.p.map(|[x,y,z]|{let q=390./(390.+z).max(130.);[576.+x*q,205.+y*q,z]});
            if p.iter().any(|v|!v.iter().all(|x|x.is_finite())||v[0]< -100.||v[0]>1252.||v[1]<0.||v[1]>420.){continue;}
            self.projected.push(((p[0][2]+p[1][2]+p[2][2])/3.,p,Color::RGBA(rgb[0],rgb[1],rgb[2],(f.alpha*255.).clamp(0.,255.) as u8)));
        }
        self.projected.sort_unstable_by(|a,b|b.0.total_cmp(&a.0));
        for (_,p,color) in &self.projected{for v in p{self.vertices.push(Vertex{position:FPoint::new(v[0],v[1]),color:*color,tex_coord:FPoint::new(0.,0.)});}}
    }
    pub(super) fn draw<T:RenderTarget>(&mut self,canvas:&mut Canvas<T>,beat:VisualBeat,cue:u8,progress:f32)->anyhow::Result<()>{
        let t=beat.seconds;let pulse=beat.pulse;let rgb=palette(cue);
        for i in 0..22{let glow=(1.-((i as f32-10.)/12.).abs()).max(0.);
            canvas.set_draw_color(Color::RGBA((7.+rgb[0]*0.05*glow) as u8,(11.+rgb[1]*0.05*glow) as u8,(16.+rgb[2]*0.06*glow) as u8,(228.*smooth((22-i) as f32/5.)) as u8));
            canvas.fill_rect(Rect::new(0,i*20,1152,20)).map_err(anyhow::Error::msg)?;}
        self.faces.clear();
        let fade=if beat.beat_count<24{1.}else{smooth(progress*6.)};
        for (scene,w) in [((cue+4)%5,1.-fade),(cue,fade)]{let color=palette(scene);
            match scene{0=>self.ribbons(t,pulse,color,w),1=>self.tunnel(t,pulse,color,w),2=>self.lattice(t,pulse,color,w),3=>self.crystals(t,pulse,color,w),
                _=>{self.ribbons(t*0.7,pulse,color,w*0.6);self.crystals(t,pulse,color,w*0.55);}}}
        self.project(t,pulse);
        if !self.vertices.is_empty(){canvas.render_geometry(&self.vertices,None,VertexIndices::Sequential).map_err(anyhow::Error::msg)?;}
        for i in 0..96u32{
            let depth=0.25+(noise(i*4+3)+t*(0.065+noise(i*4+2)*0.035)).fract()*1.75;
            let a=noise(i*4)*TAU+t*0.10;let r=35.+noise(i*4+1)*220.;
            let x=576.+r*a.cos()/depth;let y=205.+r*a.sin()*0.6/depth;
            if !(12.0..1140.0).contains(&x)||!(12.0..414.0).contains(&y){continue;}
            let alpha=(38.+110.*(1.-depth/2.)+pulse*40.).clamp(0.,255.) as u8;
            let size=if depth<0.60{3}else{1};
            canvas.set_draw_color(Color::RGBA(rgb[0] as u8,rgb[1] as u8,rgb[2] as u8,alpha));
            canvas.fill_rect(Rect::new(x as i32,y as i32,size,size)).map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn stage_renders_distinct_procedural_frames(){
        use sdl2::{pixels::PixelFormatEnum,surface::Surface,render::BlendMode};
        let mut canvas=Surface::new(1152,672,PixelFormatEnum::RGBA32).unwrap().into_canvas().unwrap();
        canvas.set_blend_mode(BlendMode::Blend);
        let mut stage=Stage::default();let mut hashes=Vec::new();
        for cue in 0..5u8{
            canvas.set_draw_color(Color::RGBA(0,0,0,0));canvas.clear();
            stage.draw(&mut canvas,VisualBeat{seconds:cue as f32*9.+2.,pulse:0.7,beat_count:cue as usize*24+12,beat_phase:0.4},cue,0.5).unwrap();
            assert!(stage.vertices.len()>100);
            assert!(stage.vertices.iter().all(|v|v.position.x.is_finite()&&v.position.y.is_finite()));
            canvas.present();
            let p=canvas.read_pixels(None,PixelFormatEnum::RGBA32).unwrap();
            let hash=p.iter().fold(0u64,|h,&v|h.wrapping_mul(31).wrapping_add(v as u64));
            assert!(!hashes.contains(&hash));hashes.push(hash);
            if let Ok(folder)=std::env::var("SST_KEYGEN_PREVIEW_DIR"){
                std::fs::create_dir_all(&folder).unwrap();
                image::save_buffer(std::path::Path::new(&folder).join(format!("stage-{cue}.png")),&p,1152,672,image::ColorType::Rgba8).unwrap();
            }
        }
    }
}
