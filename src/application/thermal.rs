//! Lecturas termicas de fuentes disponibles en Windows.
//! Nunca se equipara una zona ACPI a la temperatura del procesador.
//! Hardware WMI es opcional: SST no instala controladores ni inventa sensores.
#[cfg(windows)]
use std::{
    path::PathBuf,
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
        text.push_str("\nFuentes: LibreHardwareMonitor/OpenHardwareMonitor (WMI, si estan ejecutandose),\n");
        text.push_str("almacenamiento SMART/CIM de Windows, NVIDIA NVML (nvidia-smi) y zonas ACPI.\n");
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

#[cfg(windows)]
fn nvidia_sensors() -> anyhow::Result<Vec<Reading>> {
    let mut candidates = Vec::<PathBuf>::new();
    if let Some(dir) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(dir).join(r"NVIDIA Corporation\NVSMI\nvidia-smi.exe"));
    }
    candidates.push(PathBuf::from("nvidia-smi.exe"));
    let mut last_error = None;
    for executable in candidates {
        let output = Command::new(&executable)
            .args(["--query-gpu=index,name,temperature.gpu", "--format=csv,noheader,nounits"])
            .creation_flags(0x0800_0000)
            .output();
        match output {
            Ok(output) if output.status.success() => {
                let mut result = Vec::new();
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let fields = line.splitn(3, ',').map(str::trim).collect::<Vec<_>>();
                    if fields.len() != 3 { continue; }
                    let Ok(celsius) = fields[2].parse::<f64>() else { continue; };
                    result.push(Reading {
                        group: "GPU".to_owned(),
                        name: format!("{} · GPU {}", fields[1], fields[0]),
                        celsius,
                        source: "NVIDIA NVML".to_owned(),
                    });
                }
                return Ok(result);
            }
            Ok(output) => last_error = Some(format!("exit {:?}", output.status.code())),
            Err(error) => last_error = Some(error.to_string()),
        }
    }
    anyhow::bail!("{}", last_error.unwrap_or_else(|| "nvidia-smi no disponible".into()))
}
