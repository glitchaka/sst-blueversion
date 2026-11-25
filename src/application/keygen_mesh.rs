//! Solid 3D glyphs from unmodified TrueType outlines.
//! Caps are tessellated with the even-odd fill rule; every contour edge,
//! including the holes in O, R, etc., has a real connecting side wall.
use sdl2::{pixels::Color,rect::FPoint,render::Vertex};

type P = [f32;2];
type Triangle = [[f32;3];3];
const THICKNESS: f32 = 0.14; // constant fraction of the font em

#[derive(Default)]
struct Outline {
    contours: Vec<Vec<P>>,
    current: Vec<P>,
}
impl Outline {
    fn finish(&mut self) {
        if self.current.first() == self.current.last() { self.current.pop(); }
        if self.current.len() >= 3 {
            self.contours.push(std::mem::take(&mut self.current));
        } else { self.current.clear(); }
    }
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self,x:f32,y:f32) {
        self.finish();
        self.current.push([x,y]);
    }
    fn line_to(&mut self,x:f32,y:f32) { self.current.push([x,y]); }
    fn quad_to(&mut self,x1:f32,y1:f32,x:f32,y:f32) {
        let Some(&p)=self.current.last() else {return};
        for i in 1..=8 {
            let t=i as f32/8.0;
            let u=1.0-t;
            self.current.push([u*u*p[0]+2.0*u*t*x1+t*t*x,
                u*u*p[1]+2.0*u*t*y1+t*t*y]);
        }
    }
    fn curve_to(&mut self,x1:f32,y1:f32,x2:f32,y2:f32,x:f32,y:f32) {
        let Some(&p)=self.current.last() else {return};
        for i in 1..=12 {
            let t=i as f32/12.0;
            let u=1.0-t;
            self.current.push([u*u*u*p[0]+3.0*u*u*t*x1+3.0*u*t*t*x2+t*t*t*x,
                u*u*u*p[1]+3.0*u*u*t*y1+3.0*u*t*t*y2+t*t*t*y]);
        }
    }
    fn close(&mut self) { self.finish(); }
}

struct Face {
    vertices: Triangle,
    shade: f32,
}
pub(super) struct GlyphMesh {
    faces: Vec<Face>,
}
fn edge_x(a:P,b:P,y:f32)->f32 {
    a[0]+(b[0]-a[0])*(y-a[1])/(b[1]-a[1])
}
fn edges(contours:&[Vec<P>])->Vec<(P,P)> {
    contours.iter().flat_map(|c| c.iter().copied().zip(c.iter().copied().cycle().skip(1)).take(c.len()))
        .filter(|(a,b)| a!=b).collect()
}
fn caps(contours:&[Vec<P>])->Vec<[P;3]> {
    let edges=edges(contours);
    let mut levels:Vec<f32>=contours.iter().flatten().map(|p|p[1]).collect();
    levels.sort_by(f32::total_cmp);
    levels.dedup_by(|a,b| (*a-*b).abs()<1e-6);
    let mut triangles=Vec::new();
    for band in levels.windows(2) {
        let (lo,hi)=(band[0],band[1]);
        if hi-lo<1e-6 {continue}
        let mid=(lo+hi)*0.5;
        let mut active:Vec<_>=edges.iter().copied().filter(|(a,b)|
            mid>a[1].min(b[1]) && mid<a[1].max(b[1])).collect();
        active.sort_by(|&(a,b),&(c,d)|edge_x(a,b,mid).total_cmp(&edge_x(c,d,mid)));
        for pair in active.chunks_exact(2) {
            let (a,b)=pair[0];let (c,d)=pair[1];
            let l0=[edge_x(a,b,lo),lo];let l1=[edge_x(a,b,hi),hi];
            let r0=[edge_x(c,d,lo),lo];let r1=[edge_x(c,d,hi),hi];
            triangles.push([l0,r0,r1]);
            triangles.push([l0,r1,l1]);
        }
    }
    triangles
}
impl GlyphMesh {
    pub(super) fn from_font(face:&ttf_parser::Face<'_>, ch:char)->anyhow::Result<Self> {
        let id=face.glyph_index(ch).ok_or_else(||anyhow::anyhow!("keygen: falta el glifo {ch}"))?;
        let mut outline=Outline::default();
        let bounds=face.outline_glyph(id,&mut outline).ok_or_else(||anyhow::anyhow!("keygen: glifo sin contorno {ch}"))?;
        outline.finish();
        let em=f32::from(face.units_per_em());
        let cx=(f32::from(bounds.x_min)+f32::from(bounds.x_max))*0.5;
        // Shared cap-height baseline: punctuation and short letters keep
        // their natural vertical alignment, rather than being stretched.
        let cap=face.glyph_index('H').and_then(|id|face.glyph_bounding_box(id))
            .map(|r| f32::from(r.y_max)).unwrap_or(em*0.7);
        for p in outline.contours.iter_mut().flatten() {
            *p=[(p[0]-cx)/em,(cap*0.5-p[1])/em];
        }
        let mesh=Self::from_contours(&outline.contours);
        anyhow::ensure!(!mesh.faces.is_empty(),"keygen: malla vacía para {ch}");
        Ok(mesh)
    }
    fn from_contours(contours:&[Vec<P>])->Self {
        let mut faces=Vec::new();
        let front=THICKNESS*0.5;
        let back=-front;
        for tri in caps(contours) {
            faces.push(Face{vertices:tri.map(|[x,y]|[x,y,front]),shade:1.0});
            faces.push(Face{vertices:tri.map(|[x,y]|[x,y,back]),shade:0.66});
        }
        for (a,b) in edges(contours) {
            let (dx,dy)=(b[0]-a[0],b[1]-a[1]);
            let len=dx.hypot(dy);
            if len<1e-6 {continue}
            // Flat side lighting, no bevel or gradient on the glyph face.
            let shade=0.36+0.36*((dy-dx*0.6)/len).abs().min(1.0);
            let af=[a[0],a[1],front];let bf=[b[0],b[1],front];
            let ab=[a[0],a[1],back];let bb=[b[0],b[1],back];
            faces.push(Face{vertices:[af,bf,bb],shade});
            faces.push(Face{vertices:[af,bb,ab],shade});
        }
        Self{faces}
    }
    pub(super) fn append_vertices(&self,vertices:&mut Vec<Vertex>,size:f32,rgb:(u8,u8,u8),
        project:impl Fn([f32;3])->(f32,f32,f32)){
        let mut projected:Vec<_>=self.faces.iter().map(|face|{
            let pts=face.vertices.map(|v|project(v.map(|c|c*size)));
            let depth=pts.iter().map(|p|p.2).sum::<f32>()/3.0;
            (depth,pts,face.shade)
        }).collect();
        projected.sort_unstable_by(|a,b|a.0.total_cmp(&b.0));
        for (_,pts,shade) in projected {
            let tone=|v:u8|(f32::from(v)*shade).round() as u8;
            let color=Color::RGB(tone(rgb.0),tone(rgb.1),tone(rgb.2));
            for (x,y,_) in pts{
                vertices.push(Vertex{position:FPoint::new(x,y),color,tex_coord:FPoint::new(0.0,0.0)});
            }
        }
    }
    #[cfg(test)]
    fn draw<T:sdl2::render::RenderTarget>(&self,canvas:&mut sdl2::render::Canvas<T>,size:f32,rgb:(u8,u8,u8),
        project:impl Fn([f32;3])->(f32,f32,f32))->anyhow::Result<()>{
        let mut vertices=Vec::new();
        self.append_vertices(&mut vertices,size,rgb,project);
        canvas.render_geometry(&vertices,None,sdl2::render::VertexIndices::Sequential).map_err(anyhow::Error::msg)
    }

}
#[cfg(test)]
mod tests {
    use super::*;
    fn square()->Vec<Vec<P>> {
        vec![vec![[0.0,0.0],[4.0,0.0],[4.0,4.0],[0.0,4.0]],
             vec![[1.0,1.0],[1.0,3.0],[3.0,3.0],[3.0,1.0]]]
    }
    #[test]
    fn caps_preserve_holes_and_walls_have_constant_depth() {
        let contours=square();
        let area:f32=caps(&contours).iter().map(|[a,b,c]|
            ((b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])).abs()*0.5).sum();
        assert!((area-12.0).abs()<0.0001);
        let mesh=GlyphMesh::from_contours(&contours);
        assert_eq!(mesh.faces.iter().filter(|f|f.vertices.iter().any(|p|p[2]>0.0)
            && f.vertices.iter().any(|p|p[2]<0.0)).count(),16);
        assert!(mesh.faces.iter().flat_map(|f|f.vertices).all(|p|
            (p[2].abs()-THICKNESS*0.5).abs()<0.0001));
    }
    #[test]
    fn sdl_renders_solid_cap_and_open_counter() {
        use sdl2::{pixels::PixelFormatEnum,surface::Surface};
        let mut canvas=Surface::new(128,128,PixelFormatEnum::RGBA32).unwrap().into_canvas().unwrap();
        canvas.set_draw_color(Color::RGB(0,0,0));canvas.clear();
        GlyphMesh::from_contours(&square()).draw(&mut canvas,20.0,(220,180,120),
            |[x,y,z]|(20.0+x,20.0+y,z)).unwrap();
        canvas.present();
        let p=canvas.read_pixels(None,PixelFormatEnum::RGBA32).unwrap();
        let at=|x:usize,y:usize|p[(y*128+x)*4];
        assert!(at(30,30)>100);
        assert_eq!(at(60,60),0);
    }
    #[test]
    fn actual_sst_terminal_font_has_closed_glyphs() {
        let face=ttf_parser::Face::parse(super::super::SST_TERMINAL_FONT_BYTES,0).unwrap();
        for ch in "SHEL OCKT/KEYGN20".chars().filter(|c|*c!=' ') {
            let mesh=GlyphMesh::from_font(&face,ch).unwrap();
            assert!(mesh.faces.len()>4);
            assert!(mesh.faces.iter().flat_map(|f|f.vertices).flatten().all(f32::is_finite));
        }
    }
}
