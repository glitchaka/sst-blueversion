//! Isolated native SST editorial component laboratory. No host operations.
use std::sync::Mutex;

pub const COUNT: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoState {
    pub page: u8,
    pub selected: u8,
    pub tick: u32,
    pub paused: bool,
    pub action: bool,
}
impl DemoState {
    const CLOSED: Self = Self { page:0, selected:0, tick:0, paused:false, action:false };
    fn new(page:u8) -> Self { Self {page, ..Self::CLOSED} }
    fn select_page(&mut self,page:u8) { *self=Self::new(page.clamp(1,COUNT)); }
    fn step(&mut self,delta:i16) {
        self.select_page((i16::from(self.page)-1+delta).rem_euclid(i16::from(COUNT)) as u8+1);
    }
    fn focus(&mut self,delta:i16) {
        let count=match self.page { 5|9|15|16|20=>3, 11|14=>5, 13=>4, _=>3 };
        self.selected=(i16::from(self.selected)+delta).rem_euclid(count) as u8;
        self.action=false;
    }
    fn advance(&mut self) { if !self.paused {self.tick=self.tick.wrapping_add(1);} }
    #[cfg(any(windows,test))]
    pub fn progress(self)->i32 { self.tick.saturating_mul(2).min(100) as i32 }
    #[cfg(any(windows,test))]
    pub fn cpu(self)->i32 {
        // 0..95: a deliberately repeating synthetic load rise and fall.
        let phase=(self.tick%100) as i32;
        15+if phase<50 {phase*8/5} else {(100-phase)*8/5}
    }
    #[cfg(windows)]
    pub fn blink(self)->bool { self.paused || (self.tick/5)%2==0 }
    #[cfg(windows)]
    pub fn phase(self)->i32 { (self.tick%12) as i32 }
}
static STATE: Mutex<DemoState> = Mutex::new(DemoState::CLOSED);
fn locked()->std::sync::MutexGuard<'static,DemoState> {
    STATE.lock().unwrap_or_else(|e|e.into_inner())
}
pub fn is_active()->bool {locked().page!=0}
#[cfg(windows)]
pub fn snapshot()->DemoState {*locked()}

pub struct Preview;
impl Preview {
    pub fn open()->anyhow::Result<Self>{
        let mut state=locked();
        anyhow::ensure!(state.page==0,"La demo ya está abierta.");
        state.select_page(1);
        Ok(Self)
    }
    pub fn show(&self,page:u8){locked().select_page(page);}
    pub fn step(&self,delta:i16){locked().step(delta);}
    pub fn focus(&self,delta:i16){locked().focus(delta);}
    pub fn pause(&self){let mut s=locked();s.paused=!s.paused;}
    pub fn action(&self){let mut s=locked();s.action=!s.action;}
    pub fn reset(&self){let mut s=locked();let page=s.page;s.select_page(page);}
    pub fn advance(&self){locked().advance();}
}
impl Drop for Preview {fn drop(&mut self){*locked()=DemoState::CLOSED;}}

#[cfg(windows)]
pub const TITLES: [&str;COUNT as usize]=[
    "Composición / inventario", "Cabecera / red", "Magnitud / memoria", "Registro / servicios",
    "Foco / teclado", "Advertencia / umbral", "Incidencia / acción", "Instrumento / escala",
    "Entrada / cursor", "Transferencia / progreso", "Procesos / consumo", "Red / continuidad",
    "Semántica / estados", "Densidad / inventario", "Detalle / expansión", "Decisión / confirmación",
    "Ruta / topología", "Evidencia / datos parciales", "Espera / indeterminada", "Comando / sugerencias",
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editorial_lab_has_native_geometry_and_one_plate_per_index(){
        let source=include_str!("editorial_demo.slint");
        assert!(!source.contains("@image-url("));
        assert!(!source.contains("Image {"));
        for page in 1..=COUNT {
            let marker=format!("if root.page == {page} : Rectangle");
            assert_eq!(source.matches(&marker).count(),1,"plate {page}");
        }
        assert!(source.matches("Path {").count()>=40);
    }
    #[test]
    fn navigation_wraps_and_resets_local_interaction(){
        let mut s=DemoState::new(1);
        s.step(-1);assert_eq!(s.page,COUNT);
        s.focus(1);s.advance();s.action=true;s.paused=true;
        s.step(1);assert_eq!(s,DemoState::new(1));
        s.select_page(200);assert_eq!(s.page,COUNT);
    }
    #[test]
    fn simulated_motion_is_bounded_and_pause_freezes(){
        let mut s=DemoState::new(10);
        assert_eq!(s.progress(),0);
        for _ in 0..50{s.advance();}
        assert_eq!(s.progress(),100);
        s.paused=true;s.advance();assert_eq!(s.tick,50);
        s.paused=false;
        let mut min=100;let mut max=0;
        for _ in 0..200{s.advance();min=min.min(s.cpu());max=max.max(s.cpu());assert_eq!(s.progress(),100);}
        assert!(min<25&&max>=85);
    }
    #[test]
    fn keyboard_focus_wraps_in_each_control(){
        let mut s=DemoState::new(5);s.focus(-1);assert_eq!(s.selected,2);
        s.focus(1);assert_eq!(s.selected,0);
        s.select_page(11);s.focus(-1);assert_eq!(s.selected,4);
    }
}
