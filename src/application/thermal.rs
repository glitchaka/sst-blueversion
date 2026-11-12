//! Lecturas termicas de fuentes disponibles en Windows.
//! Nunca se equipara una zona ACPI a la temperatura del procesador.
//! Hardware WMI es opcional: SST no instala controladores ni inventa sensores.
#[cfg(windows)]
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::core::CommandOutput;

#[cfg(windows)]
#[derive(Clone)]
struct Reading {
    group: String,
    name: String,
    celsius: f64,
    source: String,
}

pub fn report() -> CommandOutput {
    #[cfg(not(windows))]
    {
        return CommandOutput::error("La telemetria termica solo esta disponible en Windows.", 3);
    }

    #[cfg(windows)]
    {
        let mut readings = Vec::new();
        let mut notes = Vec::new();
        match native_storage_sensors() {
            Ok(rows) => readings.extend(rows),
            Err(error) => notes.push(format!("Windows NVMe (API nativa): {error}")),
        }
        match windows_sensors() {
            Ok(rows) => readings.extend(rows),
            Err(error) => notes.push(format!("WMI/Storage: {error}")),
        }
        match nvidia_sensors() {
            Ok(rows) => readings.extend(rows),
            Err(error) => notes.push(format!("NVIDIA: {error}")),
        }

        readings.retain(|reading| reading.celsius.is_finite() && (-30.0..=150.0).contains(&reading.celsius));
        readings.sort_by(|a, b| {
            a.group.cmp(&b.group)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.source.cmp(&b.source))
        });
        readings.dedup_by(|a, b| {
            a.group == b.group && a.name == b.name && a.source == b.source
        });

        let mut text = String::from("SST SENSORES — TEMPERATURAS REALES\n============================================================\n");
        if readings.is_empty() {
            text.push_str("No se encontraron sensores termicos legibles con los proveedores disponibles.\n");
        } else {
            let mut previous = String::new();
            for row in &readings {
                if previous != row.group {
                    previous = row.group.clone();
                    text.push_str(&format!("\n{}\n------------------------------------------------------------\n", row.group));
                }
                text.push_str(&format!("{:<36} {:>6.1} °C  [{}]\n", row.name, row.celsius, row.source));
            }
        }

        let has_cpu = readings.iter().any(|r| r.group == "CPU");
        if !has_cpu {
            text.push_str("\nCPU: temperatura no disponible con los proveedores activos.\n");
        }
        text.push_str("\nFuentes: Windows Storage IOCTL, NVIDIA NVML (API directa),\n");
        text.push_str("WMI/CIM de Windows y LHM/OHM opcionales, zonas ACPI.\n");
        text.push_str("Las zonas ACPI NO representan necesariamente la temperatura del CPU.\n");
        text.push_str("Los sensores ausentes se omiten: SST no estima ni fabrica temperaturas.\n");
        if !notes.is_empty() {
            text.push_str("\nProveedores no disponibles:\n");
            for note in notes {
                text.push_str(&format!("  - {note}\n"));
            }
        }
        CommandOutput::ok(text)
    }
}

#[cfg(windows)]
fn windows_sensors() -> anyhow::Result<Vec<Reading>> {
    // PowerShell es solo el adaptador CIM/WMI incorporado en Windows. Los proveedores
    // de hardware son opcionales; no se requiere PowerShell para las demas herramientas SST.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$rows = New-Object 'System.Collections.Generic.List[object]'
foreach ($ns in @('root\LibreHardwareMonitor','root\OpenHardwareMonitor')) {
  try {
    $sensors = @(Get-CimInstance -Namespace $ns -ClassName Sensor -ErrorAction Stop)
    foreach ($s in $sensors) {
      if ([string]$s.SensorType -ne 'Temperature' -or $null -eq $s.Value) { continue }
      $rows.Add([pscustomobject]@{
        source=$ns; identifier=[string]$s.Identifier;
        name=[string]$s.Name; value=[double]$s.Value
      })
    }
  } catch { }
}
try {
  foreach ($disk in @(Get-PhysicalDisk -ErrorAction Stop)) {
    try {
      $counter = $disk | Get-StorageReliabilityCounter -ErrorAction Stop
      if ($null -ne $counter.Temperature -and [double]$counter.Temperature -gt 0) {
        $rows.Add([pscustomobject]@{
          source='Windows Storage'; identifier='/disk';
          name=([string]$disk.FriendlyName + ' (' + [string]$disk.DeviceId + ')');
          value=[double]$counter.Temperature
        })
      }
    } catch { }
  }
} catch { }
try {
  foreach ($zone in @(Get-CimInstance -Namespace 'root\wmi' -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop)) {
    if ($null -ne $zone.CurrentTemperature -and [double]$zone.CurrentTemperature -gt 0) {
      $rows.Add([pscustomobject]@{
        source='ACPI'; identifier='/acpi';
        name=[string]$zone.InstanceName;
        value=([math]::Round(([double]$zone.CurrentTemperature / 10.0) - 273.15, 1))
      })
    }
  }
} catch { }
[Console]::Out.Write((ConvertTo-Json -InputObject @($rows.ToArray()) -Compress -Depth 3))
"#;
    let ps = crate::support::windows::system32_executable(
        r"WindowsPowerShell\v1.0\powershell.exe",
    )?;
    let wide: Vec<u8> = SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, wide);
    let mut command = Command::new(ps);
    command.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.creation_flags(0x0800_0000);
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("consulta CIM/WMI excedio 12 segundos");
        }
        thread::sleep(Duration::from_millis(80));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        anyhow::bail!("consulta CIM/WMI fallo (exit {:?})", output.status.code());
    }
    if output.stdout.len() > 1024 * 1024 {
        anyhow::bail!("respuesta WMI demasiado grande");
    }
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut result = Vec::new();
    for value in rows.as_array().into_iter().flatten() {
        let celsius = value.get("value").and_then(serde_json::Value::as_f64);
        let Some(celsius) = celsius else { continue; };
        let source = value.get("source").and_then(serde_json::Value::as_str).unwrap_or("WMI");
        let id = value.get("identifier").and_then(serde_json::Value::as_str).unwrap_or("");
        let name = value.get("name").and_then(serde_json::Value::as_str).unwrap_or("Sensor");
        let group = if source == "ACPI" { "Zona ACPI" }
            else if source == "Windows Storage" { "Disco / SSD" }
            else { classify(id, name) };
        result.push(Reading {
            group: group.into(),
            name: name.into(),
            celsius,
            source: source.into(),
        });
    }
    Ok(result)
}

#[cfg(windows)]
fn classify(identifier: &str, name: &str) -> &'static str {
    let id = identifier.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    if id.contains("cpu") || name.contains("cpu package") { "CPU" }
    else if id.contains("gpu") { "GPU" }
    else if id.contains("hdd") || id.contains("disk") || id.contains("nvme") || id.contains("storage") { "Disco / SSD" }
    else if id.contains("ram") || id.contains("memory") { "Memoria" }
    else if id.contains("lpc") || id.contains("motherboard") || id.contains("superio") { "Placa base / VRM" }
    else if id.contains("battery") || id.contains("batt") { "Bateria" }
    else { "Otros componentes" }
}

// Query temperatures directly through the Windows storage driver stack.
// Windows 10+ supports IOCTL_STORAGE_QUERY_PROPERTY for NVMe devices. No
// PowerShell, SMART CLI, installed sensor application or custom kernel driver.
#[cfg(windows)]
fn native_storage_sensors() -> anyhow::Result<Vec<Reading>> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    };
    // CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x500, METHOD_BUFFERED, FILE_ANY_ACCESS)
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002d_1400;
    // STORAGE_PROPERTY_ID::StorageDeviceTemperatureProperty
    const STORAGE_DEVICE_TEMPERATURE_PROPERTY: u32 = 52;

    let mut result = Vec::new();
    let mut opened = 0usize;
    for disk in 0..32 {
        let path = format!(r"\\.\PhysicalDrive{disk}");
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // Access 0 is sufficient for read-only property queries where allowed.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE { continue; }
        opened += 1;
        struct Handle(windows_sys::Win32::Foundation::HANDLE);
        impl Drop for Handle {
            fn drop(&mut self) { unsafe { CloseHandle(self.0); } }
        }
        let _handle = Handle(handle);
        // STORAGE_PROPERTY_QUERY { PropertyId, QueryType, AdditionalParameters[1] }
        let query = [STORAGE_DEVICE_TEMPERATURE_PROPERTY, 0, 0];
        let mut output = [0u8; 512];
        let mut returned = 0u32;
        let success = unsafe {
            DeviceIoControl(
                handle, IOCTL_STORAGE_QUERY_PROPERTY,
                query.as_ptr().cast(), (query.len() * std::mem::size_of::<u32>()) as u32,
                output.as_mut_ptr().cast(), output.len() as u32,
                &mut returned, std::ptr::null_mut(),
            )
        };
        if success == 0 || returned < 24 { continue; }
        // STORAGE_TEMPERATURE_DATA_DESCRIPTOR: 24-byte header followed by
        // STORAGE_TEMPERATURE_INFO entries (each 16 bytes on Windows).
        let valid = (returned as usize).min(output.len());
        let descriptor_size = u32::from_le_bytes(output[4..8].try_into().unwrap()) as usize;
        if descriptor_size < 24 { continue; }
        let info_count = u16::from_le_bytes([output[12], output[13]]) as usize;
        for sensor in 0..info_count.min((valid - 24) / 16).min(16) {
            let offset = 24 + sensor * 16;
            let index = u16::from_le_bytes([output[offset], output[offset + 1]]);
            let degrees = i16::from_le_bytes([output[offset + 2], output[offset + 3]]);
            if (-30..=150).contains(&degrees) {
                result.push(Reading {
                    group: "Disco / SSD".to_owned(),
                    name: format!("Disco fisico {disk} · sensor {index}"),
                    celsius: f64::from(degrees),
                    source: "Windows Storage IOCTL (nativo)".to_owned(),
                });
            }
        }
    }
    if opened == 0 {
        anyhow::bail!("no fue posible abrir dispositivos de almacenamiento");
    }
    Ok(result)
}

// NVML is part of the installed NVIDIA display driver. Calling it directly
// avoids spawning nvidia-smi.exe and works when that utility is absent.
#[cfg(windows)]
fn nvidia_sensors() -> anyhow::Result<Vec<Reading>> {
    use windows_sys::Win32::System::LibraryLoader::{
        FreeLibrary, GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };

    type NvmlInit = unsafe extern "C" fn() -> i32;
    type NvmlShutdown = unsafe extern "C" fn() -> i32;
    type NvmlCount = unsafe extern "C" fn(*mut u32) -> i32;
    type NvmlHandle = unsafe extern "C" fn(u32, *mut *mut std::ffi::c_void) -> i32;
    type NvmlTemperature = unsafe extern "C" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32;

    // Only search the protected Windows System32 directory. An arbitrary
    // working-directory nvml.dll must never be loaded into SST.
    let dll: Vec<u16> = "nvml.dll".encode_utf16().chain(std::iter::once(0)).collect();
    let library = unsafe { LoadLibraryExW(dll.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if library.is_null() {
        anyhow::bail!("NVML no esta instalado en Windows System32");
    }
    struct Library(windows_sys::Win32::Foundation::HMODULE);
    impl Drop for Library {
        fn drop(&mut self) { unsafe { FreeLibrary(self.0); } }
    }
    let library = Library(library);
    unsafe fn symbol<T: Copy>(module: windows_sys::Win32::Foundation::HMODULE, name: &'static [u8]) -> anyhow::Result<T> {
        let address = unsafe { GetProcAddress(module, name.as_ptr()) };
        let Some(address) = address else {
            anyhow::bail!("NVML no exporta {}", String::from_utf8_lossy(&name[..name.len() - 1]));
        };
        // The symbol type above has the same pointer width as these C functions.
        Ok(unsafe { std::mem::transmute_copy(&address) })
    }

    // The NVML driver reports temperatures in degrees Celsius.
    let init: NvmlInit = unsafe { symbol(library.0, b"nvmlInit_v2\0")? };
    let shutdown: NvmlShutdown = unsafe { symbol(library.0, b"nvmlShutdown\0")? };
    let count_fn: NvmlCount = unsafe { symbol(library.0, b"nvmlDeviceGetCount_v2\0")? };
    let handle_fn: NvmlHandle = unsafe { symbol(library.0, b"nvmlDeviceGetHandleByIndex_v2\0")? };
    let temperature_fn: NvmlTemperature = unsafe { symbol(library.0, b"nvmlDeviceGetTemperature\0")? };

    let init_status = unsafe { init() };
    if init_status != 0 {
        anyhow::bail!("NVML no pudo iniciar (codigo {init_status})");
    }
    struct NvmlSession(NvmlShutdown);
    impl Drop for NvmlSession {
        fn drop(&mut self) { unsafe { (self.0)(); } }
    }
    let _session = NvmlSession(shutdown);

    let mut count = 0;
    let status = unsafe { count_fn(&mut count) };
    if status != 0 {
        anyhow::bail!("NVML no pudo contar las GPU (codigo {status})");
    }
    let mut rows = Vec::new();
    for index in 0..count.min(32) {
        let mut device = std::ptr::null_mut();
        if unsafe { handle_fn(index, &mut device) } != 0 || device.is_null() {
            continue;
        }
        let mut degrees = 0;
        if unsafe { temperature_fn(device, 0, &mut degrees) } == 0 && degrees <= 150 {
            rows.push(Reading {
                group: "GPU".to_owned(),
                name: format!("NVIDIA GPU {index}"),
                celsius: f64::from(degrees),
                source: "NVIDIA NVML (API nativa)".to_owned(),
            });
        }
    }
    Ok(rows)
}
