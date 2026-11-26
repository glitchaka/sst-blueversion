//! Built-in SST visual component. Geometry is expressed in device pixels so
//! the same marks work on the window chrome and on scrolling terminal rows.
pub type Ink = (u8, u8, u8);
pub const OXIDE: Ink = (225, 94, 52);
pub const CONCRETE: Ink = (201, 199, 172);
pub const GRAPHITE: Ink = (32, 39, 36);
pub const PATINA: Ink = (91, 125, 117);
pub const TITLE_SCALE: f32 = 1.78;
pub const METER_DIVISION: usize = 5;

#[cfg(windows)]
pub enum Mark { Portal, Masthead, Readout, Rail, Selected, Warning, Failure }

#[cfg(windows)]
pub fn draw(mark: Mark, width: i32, height: i32, mut rect: impl FnMut(i32,i32,i32,i32,Ink)) {
    if width < 1 || height < 1 { return; }
    let unit = (height/12).clamp(1,4);
    match mark {
        Mark::Portal => {
            // Two offset concrete spans and a circular opening cut into the pier.
            rect(0,0,width,height,GRAPHITE);
            rect(0,height/6,width*5/6,height/3,CONCRETE);
            rect(width/6,height/2,width*2/3,height/3,CONCRETE);
            rect(width*2/3,0,width/3,height/4,OXIDE);
            let radius = (height/4).min(width/4).max(1);
            for dy in -radius..radius {
                let half = ((radius*radius-dy*dy) as f32).sqrt() as i32;
                rect(width/2-half,height/2+dy,half*2,1,GRAPHITE);
            }
            rect(0,height-unit,width/3,unit,PATINA);
        }
        Mark::Masthead => {
            // A heavy lintel spans the display title. The outer shoulder
            // steps down like the cantilevers in the reference.
            rect(0,0,width,height,GRAPHITE);
            rect(0,0,width/3,unit,CONCRETE);
            rect(0,unit,unit*2,height-unit,OXIDE);
            rect(width-width/8,0,width/8,unit,OXIDE);
            rect(width-width/14,unit,width/14,unit,OXIDE);
            rect(0,height-unit,width,unit,PATINA);
            rect(0,height-unit*2,width.min(height*2),unit*2,OXIDE);
            let end = width-unit*2;
            for n in 0..5 {
                let x = end-n*unit*3;
                if x < width/2 { break; }
                rect(x,height-unit*3,unit,unit*3,CONCRETE);
            }
        }
        Mark::Readout => {
            rect(0,0,unit,height,PATINA);
            rect(0,height-unit,width,unit,PATINA);
            rect(0,height-2*unit,(width/8).min(height),unit,OXIDE);
            rect(width-unit,0,unit,unit*3,CONCRETE);
        }
        Mark::Rail => rect(width-1,0,1,height,PATINA),
        Mark::Selected => {
            rect(width/2,0,(width/2).max(1),height,CONCRETE);
            rect(0,0,width,unit*2,OXIDE);
        }
        Mark::Warning | Mark::Failure => {
            let ink = if matches!(mark,Mark::Failure) {(251,73,52)} else {(255,163,61)};
            rect(0,height/5,width,(height/2).max(1),ink);
            rect(0,height*4/5,width,unit,ink);
        }
    }
}
