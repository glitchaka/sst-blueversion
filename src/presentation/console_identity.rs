//! Built-in SST visual component. Geometry is expressed in device pixels so
//! the same marks work on the window chrome and on scrolling terminal rows.
pub type Ink = (u8, u8, u8);
pub const OXIDE: Ink = (225, 94, 52);
pub const CONCRETE: Ink = (201, 199, 172);
pub const GRAPHITE: Ink = (32, 39, 36);
pub const PATINA: Ink = (91, 125, 117);
use std::sync::atomic::{AtomicU8, Ordering};

// Zero is the ordinary terminal. The command owns this transient preview.
static PAGE: AtomicU8 = AtomicU8::new(0);
pub const COUNT: u8 = 8;
pub struct Preview;
impl Preview {
    pub fn open() -> anyhow::Result<Self> {
        PAGE.compare_exchange(0,1,Ordering::AcqRel,Ordering::Acquire)
            .map_err(|_|anyhow::anyhow!("La demo ya está abierta."))?;
        Ok(Self)
    }
    pub fn show(&self, index:u8) { PAGE.store(index+1, Ordering::Release); }
}
impl Drop for Preview {
    fn drop(&mut self) { PAGE.store(0, Ordering::Release); }
}
#[cfg(windows)]
pub fn page() -> u8 { PAGE.load(Ordering::Acquire) }


/// Editorial identity specimens: fictional data only. Each page shows a
/// graphic element at its actual working scale, alongside its information.
#[cfg(windows)]
pub struct Specimen {
    pub title: &'static str,
    pub caption: &'static str,
    pub index: &'static str,
    pub headline: &'static str,
    pub metric: &'static str,
    pub unit: &'static str,
    pub context: &'static str,
    pub rows: [&'static str; 3],
}

#[cfg(windows)]
pub fn specimen(page: u8) -> Specimen {
    match page {
        1 => Specimen { title:"Portada / identidad", caption:"Inventario de los recursos del equipo.", index:"01 / SYS", headline:"ESTADO DEL EQUIPO", metric:"08", unit:"RECURSOS", context:"Inventario local · Windows", rows:["CPU     Intel Core i3","RAM     15.68 GiB","ALCANCE Equipo local"] },
        2 => Specimen { title:"Cabecera de sección", caption:"Dos interfaces, un enlace activo.", index:"02 / NET", headline:"INTERFACES DE RED", metric:"02", unit:"INTERFACES", context:"Enlaces disponibles", rows:["01  Ethernet      ACTIVO","02  Wi-Fi         INACTIVO","    En uso        Ethernet"] },
        3 => Specimen { title:"Lectura de instrumento", caption:"10.4 de 16 GiB están en uso.", index:"03 / MEM", headline:"MEMORIA FÍSICA", metric:"65", unit:"% EN USO", context:"Capacidad instalada: 16 GiB", rows:["USADA      10.4 GiB","DISPONIBLE  5.6 GiB","MUESTRA     Ejemplo"] },
        4 => Specimen { title:"Guía de lectura", caption:"Cada fila reúne servicio, estado y PID.", index:"04 / SVC", headline:"SERVICIOS", metric:"03", unit:"REGISTROS", context:"Nombre · estado · PID", rows:["W32Time      ACTIVO    804","Spooler      ACTIVO   1220","BITS         DETENIDO    —"] },
        5 => Specimen { title:"Selección y foco", caption:"La banda señala el servicio seleccionado.", index:"05 / NAV", headline:"DIAGNÓSTICO DIRIGIDO", metric:"02", unit:"SELECCIÓN", context:"Selección actual · Servicios", rows:["01  Procesos","02  Servicios","03  Conexiones"] },
        6 => Specimen { title:"Advertencia", caption:"El uso supera el umbral en 4 puntos.", index:"06 / TRI", headline:"PRESIÓN DE MEMORIA", metric:"89", unit:"% EN USO", context:"ATENCIÓN · umbral de ejemplo 85 %", rows:["CAUSA    Uso elevado de RAM","ACCIÓN   Revisar procesos","CONTROL  Repetir medición"] },
        7 => Specimen { title:"Fallo", caption:"La ventana dejó de responder.", index:"07 / TRI", headline:"APLICACIÓN SIN RESPUESTA", metric:"01", unit:"INCIDENCIA", context:"FALLO · ventana bloqueada", rows:["EVIDENCIA  Ventana sin respuesta","ACCIÓN     Solicitar cierre normal","CONTROL    Verificar instancia"] },
        _ => Specimen { title:"Escala graduada", caption:"Cada segmento representa 5 % de la capacidad.", index:"08 / CPU", headline:"CARGA DEL PROCESADOR", metric:"65", unit:"% CPU", context:"Muestra ilustrativa · escala 0–100 %", rows:["0 %       Reposo","65 %      Lectura actual","100 %     Capacidad máxima"] },
    }
}
