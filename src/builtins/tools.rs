use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::Threading::{GetExitCodeProcess, WaitForSingleObject},
    UI::{
        Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};
use zip::ZipArchive;

use crate::{
    core::{CommandContext, CommandOutput},
    support::windows::system32_executable,
};

use super::BuiltinCommand;

const GET_HELP_DOWNLOAD: &str = "https://aka.ms/SaRA_EnterpriseVersionFiles";
const GET_HELP_ZIP: &str = "GetHelpCmd.zip";
const LEGACY_SARA_DOWNLOAD: &str = "https://github.com/ServiceDeskCSI/SaRACmd/releases/download/17_01_2877_000/SaRACmd_17_01_2877_000.zip";
const LEGACY_SARA_ZIP: &str = "SaRACmd_17_01_2877_000.zip";

pub struct ToolsBuiltin;

impl BuiltinCommand for ToolsBuiltin {
    fn name(&self) -> &'static str {
        "tools"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["tool", "utils"]
    }

    fn help(&self) -> &'static str {
        "tools — framework de utilidades de mantenimiento por producto o proveedor"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if args.is_empty() || args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
            return Ok(help_output());
        }

        match args[0].to_ascii_lowercase().as_str() {
            "list" => Ok(help_output()),
            "office" => office_tools(&args[1..]),
            other => Ok(CommandOutput::error(
                format!(
                    "tools: familia o producto desconocido: {other}\nuse: tools list\nsintaxis: tools <producto> <acción> [opciones]"
                ),
                2,
            )),
        }
    }
}

fn help_output() -> CommandOutput {
    CommandOutput::ok(
        "SST Tools\n\n\
         Framework de diagnóstico, reparación, limpieza y desinstalación profunda\n\
         organizado por producto o proveedor. Office es una familia de tools;\n\
         no define el subsistema completo.\n\n\
         Sintaxis\n\
           tools list\n\
           tools <producto> <acción> [opciones]\n\n\
         Familias disponibles\n\
           Office / Microsoft 365\n\
             tools office diagnose\n\
             tools office verify\n\
             tools office auth repair\n\
             tools office scrub [all|m365|2024|2021|2019|2016|2013|2010|2007]\n\
             tools office sara [VERSION]\n\
             tools office scrub-current [VERSION]\n\n\
         Modelo para nuevas familias\n\
           tools <producto> diagnose\n\
           tools <producto> repair\n\
           tools <producto> remove\n\n\
         SST puede combinar diagnóstico propio antes/después con la herramienta\n\
         oficial del fabricante. Los backends externos se descargan desde su fuente,\n\
         se validan antes de ejecutarse y permanecen aislados del código de SST.\n\n\
         Office\n\
           diagnose/verify revisa instalación, residuos, versión/build, Outlook,\n\
           WebView2, WAM/AAD Broker, TokenBroker, estado Entra/PRT y cachés.\n\
           auth repair vuelve a registrar AAD BrokerPlugin para el usuario actual.\n\
           scrub/sara usa SaRACmd Enterprise legacy y valida la firma Microsoft.\n\
           scrub-current conserva GetHelpCmd como backend alternativo actual.\n",
    )
}

fn office_tools(args: &[String]) -> Result<CommandOutput> {
    let Some(action) = args.first() else {
        return Ok(CommandOutput::error(
            "tools office: falta acción\nuse: tools office diagnose | auth repair | scrub [VERSION]",
            2,
        ));
    };

    match action.to_ascii_lowercase().as_str() {
        "diagnose" | "status" | "verify" => {
            if args.len() != 1 {
                return Ok(CommandOutput::error(
                    "tools office diagnose: demasiados argumentos",
                    2,
                ));
            }
            run_office_diagnose()
        }
        "auth" => office_auth_tools(&args[1..]),
        "scrub" | "sara" | "uninstall" | "remove" => {
            let version = args.get(1).map(String::as_str).unwrap_or("all");
            if args.len() > 2 {
                return Ok(CommandOutput::error(
                    "tools office scrub: demasiados argumentos",
                    2,
                ));
            }
            let version = normalize_office_version(version)?;
            run_legacy_sara_scrub(version)
        }
        "scrub-current" | "gethelp" => {
            let version = args.get(1).map(String::as_str).unwrap_or("all");
            if args.len() > 2 {
                return Ok(CommandOutput::error(
                    "tools office scrub-current: demasiados argumentos",
                    2,
                ));
            }
            let version = normalize_office_version(version)?;
            run_gethelp_scrub(version)
        }
        _ => Ok(CommandOutput::error(
            format!("tools office: acción desconocida: {action}"),
            2,
        )),
    }
}

fn office_auth_tools(args: &[String]) -> Result<CommandOutput> {
    match args.first().map(|value| value.to_ascii_lowercase()) {
        Some(action) if action == "repair" && args.len() == 1 => run_office_auth_repair(),
        Some(action) if action == "repair" => Ok(CommandOutput::error(
            "tools office auth repair: demasiados argumentos",
            2,
        )),
        Some(action) => Ok(CommandOutput::error(
            format!("tools office auth: acción desconocida: {action}\nuse: tools office auth repair"),
            2,
        )),
        None => Ok(CommandOutput::error(
            "tools office auth: falta acción\nuse: tools office auth repair",
            2,
        )),
    }
}

fn run_office_diagnose() -> Result<CommandOutput> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'

function Value-OrUnknown($value) {
    if ($null -eq $value -or [string]::IsNullOrWhiteSpace([string]$value)) { return 'UNKNOWN' }
    return [string]$value
}

$inventoryPath = 'HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Inventory\Office\16.0'
$inventory = Get-ItemProperty -LiteralPath $inventoryPath -ErrorAction SilentlyContinue
$c2r = Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration' -ErrorAction SilentlyContinue
$c2rSvc = Get-Service -Name ClickToRunSvc -ErrorAction SilentlyContinue
$tokenBroker = Get-Service -Name TokenBroker -ErrorAction SilentlyContinue

$inventoryProducts = if ($inventory) { $inventory.OfficeProductReleaseIds } else { $null }
$inventoryVersion = if ($inventory) { $inventory.OfficePackageVersion } else { $null }
$hasInventory = [bool]($inventory -and ($inventoryProducts -or $inventoryVersion))

$roots = @()
if ($env:ProgramFiles) { $roots += (Join-Path $env:ProgramFiles 'Microsoft Office\root\Office16') }
if (${env:ProgramFiles(x86)}) { $roots += (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Office\root\Office16') }
$officeRoot = $roots | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
$outlookPath = $roots | ForEach-Object { Join-Path $_ 'OUTLOOK.EXE' } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
$outlookVersion = if ($outlookPath) { (Get-Item -LiteralPath $outlookPath).VersionInfo.FileVersion } else { $null }

$uninstallRoots = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
$registeredOffice = @(
    Get-ItemProperty -Path $uninstallRoots -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Publisher -match '^Microsoft' -and
            $_.DisplayName -and
            ($_.DisplayName -match '^Microsoft 365' -or $_.DisplayName -match '^Microsoft Office')
        }
)

$hasCoreComponents = [bool]($c2r -or $c2rSvc -or $officeRoot -or $outlookPath)
if ($hasInventory) {
    $officeState = 'INSTALLED / C2R INVENTORY PRESENT'
} elseif ($registeredOffice.Count -gt 0) {
    $officeState = 'INSTALLED / REGISTERED PRODUCT PRESENT'
} elseif ($hasCoreComponents) {
    $officeState = 'INCOMPLETE / C2R INVENTORY MISSING'
} else {
    $officeState = 'ABSENT / CLEAN'
}

$officeVersion = $inventoryVersion
if (-not $officeVersion -and $c2r) {
    if ($c2r.VersionToReport) { $officeVersion = $c2r.VersionToReport }
    elseif ($c2r.ClientVersionToReport) { $officeVersion = $c2r.ClientVersionToReport }
}
$productReleaseIds = if ($inventoryProducts) { $inventoryProducts } elseif ($c2r) { $c2r.ProductReleaseIds } else { $null }

$webView = $null
$edgeRoots = @(
    'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients'
)
foreach ($edgeRoot in $edgeRoots) {
    if (-not (Test-Path -LiteralPath $edgeRoot)) { continue }
    foreach ($child in Get-ChildItem -LiteralPath $edgeRoot -ErrorAction SilentlyContinue) {
        $candidate = Get-ItemProperty -LiteralPath $child.PSPath -ErrorAction SilentlyContinue
        if ($candidate.name -match 'WebView2') {
            $webView = $candidate
            break
        }
    }
    if ($webView) { break }
}
$webViewVersion = if ($webView -and $webView.pv) { $webView.pv } else { $null }
if (-not $webViewVersion -and ${env:ProgramFiles(x86)}) {
    $wvRoot = Join-Path ${env:ProgramFiles(x86)} 'Microsoft\EdgeWebView\Application'
    if (Test-Path -LiteralPath $wvRoot) {
        $wvDir = Get-ChildItem -LiteralPath $wvRoot -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^\d+\.' } |
            Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending |
            Select-Object -First 1
        if ($wvDir) { $webViewVersion = $wvDir.Name }
    }
}

$aad = Get-AppxPackage -Name Microsoft.AAD.BrokerPlugin -ErrorAction SilentlyContinue
$cloud = Get-AppxPackage -Name Microsoft.Windows.CloudExperienceHost -ErrorAction SilentlyContinue

$dsreg = & "$env:SystemRoot\System32\dsregcmd.exe" /status 2>$null
$dsValues = @{}
foreach ($line in $dsreg) {
    if ($line -match '^\s*(AzureAdJoined|DomainJoined|WorkplaceJoined|AzureAdPrt)\s*:\s*(.+?)\s*$') {
        $dsValues[$matches[1]] = $matches[2]
    }
}
function Ds-Value($name) {
    if ($dsValues.ContainsKey($name)) { return $dsValues[$name] }
    return 'UNKNOWN'
}

$oneAuth = Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'Microsoft\OneAuth')
$tokenCache = Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'Microsoft\TokenBroker')
$identityCache = Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'Microsoft\IdentityCache')
$wvProcesses = @(Get-Process -Name msedgewebview2 -ErrorAction SilentlyContinue).Count

Write-Output 'OFFICE'
Write-Output ("  State ................ {0}" -f $officeState)
Write-Output ("  C2R Inventory ........ {0}" -f $(if ($hasInventory) { 'PRESENT' } else { 'ABSENT' }))
Write-Output ("  OfficeProductReleaseIds {0}" -f (Value-OrUnknown $productReleaseIds))
Write-Output ("  OfficePackageVersion . {0}" -f (Value-OrUnknown $officeVersion))
Write-Output ("  Platform ............. {0}" -f (Value-OrUnknown $c2r.Platform))
Write-Output ("  Update channel ....... {0}" -f (Value-OrUnknown $c2r.UpdateChannel))
Write-Output ("  Registered products .. {0} (auxiliary)" -f $registeredOffice.Count)
foreach ($product in $registeredOffice) {
    Write-Output ("    - {0} {1}" -f $product.DisplayName, $product.DisplayVersion)
}
Write-Output ("  ClickToRunSvc ........ {0}" -f $(if ($c2rSvc) { $c2rSvc.Status } else { 'ABSENT' }))
Write-Output ("  Office16 root ........ {0}" -f $(if ($officeRoot) { $officeRoot } else { 'ABSENT' }))
Write-Output ("  Outlook.exe .......... {0}" -f $(if ($outlookPath) { $outlookPath } else { 'ABSENT' }))
Write-Output ("  Outlook version ...... {0}" -f (Value-OrUnknown $outlookVersion))
Write-Output ''
Write-Output 'AUTHENTICATION'
Write-Output ("  WebView2 Runtime ..... {0}" -f (Value-OrUnknown $webViewVersion))
Write-Output ("  WebView2 processes ... {0}" -f $wvProcesses)
Write-Output ("  AAD BrokerPlugin ..... {0}" -f $(if ($aad) { $aad.Version } else { 'ABSENT' }))
Write-Output ("  CloudExperienceHost .. {0}" -f $(if ($cloud) { $cloud.Version } else { 'ABSENT' }))
Write-Output ("  TokenBroker .......... {0}" -f $(if ($tokenBroker) { $tokenBroker.Status } else { 'ABSENT' }))
Write-Output ''
Write-Output 'ENTRA / WINDOWS'
Write-Output ("  Domain Joined ........ {0}" -f (Ds-Value 'DomainJoined'))
Write-Output ("  Entra Joined ......... {0}" -f (Ds-Value 'AzureAdJoined'))
Write-Output ("  Workplace Joined ..... {0}" -f (Ds-Value 'WorkplaceJoined'))
Write-Output ("  PRT .................. {0}" -f (Ds-Value 'AzureAdPrt'))
Write-Output ''
Write-Output 'AUTH CACHE'
Write-Output ("  OneAuth .............. {0}" -f $(if ($oneAuth) { 'PRESENT' } else { 'ABSENT' }))
Write-Output ("  TokenBroker .......... {0}" -f $(if ($tokenCache) { 'PRESENT' } else { 'ABSENT' }))
Write-Output ("  IdentityCache ........ {0}" -f $(if ($identityCache) { 'PRESENT' } else { 'ABSENT' }))
"#;

    let output = run_powershell(script)?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();

    if !output.status.success() {
        let detail = if stderr.is_empty() {
            "PowerShell no pudo completar el diagnóstico de Office".to_string()
        } else {
            stderr
        };
        return Ok(CommandOutput::error(detail, output.status.code().unwrap_or(1)));
    }

    if stdout.is_empty() {
        return Ok(CommandOutput::error(
            "tools office diagnose: no se obtuvo información del sistema",
            1,
        ));
    }

    Ok(CommandOutput::ok(format!("Office / Outlook diagnostics\n\n{stdout}\n")))
}

fn run_office_auth_repair() -> Result<CommandOutput> {
    let script = r#"
$ErrorActionPreference = 'Stop'
$running = Get-Process -Name OUTLOOK,WINWORD,EXCEL,POWERPNT,ONENOTE -ErrorAction SilentlyContinue
if ($running) {
    $names = ($running.ProcessName | Sort-Object -Unique) -join ', '
    [Console]::Error.WriteLine("Cierre las aplicaciones de Office antes de reparar WAM/AAD Broker: $names")
    exit 6
}

$manifest = Join-Path $env:windir 'SystemApps\Microsoft.AAD.BrokerPlugin_cw5n1h2txyewy\Appxmanifest.xml'
if (-not (Test-Path -LiteralPath $manifest)) {
    [Console]::Error.WriteLine('No se encontró el manifiesto de Microsoft.AAD.BrokerPlugin en Windows.')
    exit 2
}

try {
    Add-AppxPackage -Register $manifest -DisableDevelopmentMode -ForceApplicationShutdown -ErrorAction Stop
    $pkg = Get-AppxPackage -Name Microsoft.AAD.BrokerPlugin -ErrorAction Stop
    Write-Output ("AAD BrokerPlugin registrado correctamente para el usuario actual. Versión: {0}" -f $pkg.Version)
    exit 0
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
"#;

    let output = run_powershell(script)?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let code = output.status.code().unwrap_or(1);

    if output.status.success() {
        let message = if stdout.is_empty() {
            "AAD BrokerPlugin registrado correctamente para el usuario actual.".to_string()
        } else {
            stdout
        };
        return Ok(CommandOutput::ok(format!(
            "{message}\nVuelva a abrir Outlook y pruebe Actualizar contraseña. Si sigue fallando, ejecute tools office diagnose.\n"
        )));
    }

    if code == 6 {
        return Ok(CommandOutput::error(
            if stderr.is_empty() {
                "Cierre Outlook y las demás aplicaciones de Office antes de ejecutar tools office auth repair."
                    .to_string()
            } else {
                stderr
            },
            6,
        ));
    }

    Ok(CommandOutput::error(
        if stderr.is_empty() {
            "No se pudo reparar Microsoft.AAD.BrokerPlugin.".to_string()
        } else {
            stderr
        },
        code,
    ))
}

fn run_powershell(script: &str) -> Result<std::process::Output> {
    let powershell = system32_executable(r"WindowsPowerShell\v1.0\powershell.exe")?;
    Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
        ])
        .arg(script)
        .output()
        .context("no se pudo ejecutar Windows PowerShell desde SST")
}

fn normalize_office_version(value: &str) -> Result<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "all" => Ok("All"),
        "m365" | "365" | "microsoft365" => Ok("M365"),
        "2024" => Ok("2024"),
        "2021" => Ok("2021"),
        "2019" => Ok("2019"),
        "2016" => Ok("2016"),
        "2013" => Ok("2013"),
        "2010" => Ok("2010"),
        "2007" => Ok("2007"),
        other => bail!(
            "versión de Office no válida: {other}; use all, m365, 2024, 2021, 2019, 2016, 2013, 2010 o 2007"
        ),
    }
}

fn run_legacy_sara_scrub(version: &str) -> Result<CommandOutput> {
    let root = env::temp_dir().join("sst").join("tools").join("office-sara-legacy");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)
        .with_context(|| format!("no se pudo crear {}", root.display()))?;

    let zip_path = root.join(LEGACY_SARA_ZIP);
    download_file(LEGACY_SARA_DOWNLOAD, &zip_path, "SaRACmd legacy")?;
    extract_zip(&zip_path, &root)?;
    let sara = find_named_executable(&root, "SaRAcmd.exe")
        .context("el paquete legacy no contiene SaRAcmd.exe")?;

    verify_microsoft_signature(&sara)?;
    let exit_code = run_elevated_office_scrubber(&sara, version)?;
    let _ = fs::remove_dir_all(&root);

    Ok(office_result(
        exit_code,
        version,
        "SaRACmd legacy / OfficeScrubScenario",
    ))
}

fn run_gethelp_scrub(version: &str) -> Result<CommandOutput> {
    let root = env::temp_dir().join("sst").join("tools").join("office-gethelp");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)
        .with_context(|| format!("no se pudo crear {}", root.display()))?;

    let zip_path = root.join(GET_HELP_ZIP);
    download_file(GET_HELP_DOWNLOAD, &zip_path, "GetHelpCmd")?;
    extract_zip(&zip_path, &root)?;
    let get_help = find_named_executable(&root, "GetHelpCmd.exe")
        .context("el paquete oficial de Microsoft no contiene GetHelpCmd.exe")?;

    verify_microsoft_signature(&get_help)?;
    let exit_code = run_elevated_office_scrubber(&get_help, version)?;
    let _ = fs::remove_dir_all(&root);

    Ok(office_result(
        exit_code,
        version,
        "GetHelpCmd / OfficeScrubScenario",
    ))
}

fn download_file(url: &str, destination: &Path, label: &str) -> Result<()> {
    let curl = system32_executable("curl.exe")?;
    let status = Command::new(curl)
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            url,
            "-o",
        ])
        .arg(destination)
        .status()
        .with_context(|| format!("no se pudo ejecutar curl para descargar {label}"))?;

    if !status.success() {
        bail!("{label} no pudo descargarse por HTTPS");
    }
    Ok(())
}

fn extract_zip(zip_path: &Path, destination: &Path) -> Result<()> {
    let file = File::open(zip_path)
        .with_context(|| format!("no se pudo abrir {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).context("el paquete descargado no es un ZIP válido")?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            bail!("el paquete contiene una ruta insegura");
        };
        let output = destination.join(relative);

        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = File::create(&output)?;
        io::copy(&mut entry, &mut target)?;
    }
    Ok(())
}

fn find_named_executable(root: &Path, expected_name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_named_executable(&path, expected_name) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(expected_name))
        {
            return Some(path);
        }
    }
    None
}

fn verify_microsoft_signature(exe: &Path) -> Result<()> {
    let path = exe.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$s=Get-AuthenticodeSignature -LiteralPath '{path}'; \
         if($s.Status -ne 'Valid' -or $null -eq $s.SignerCertificate -or \
         $s.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation'){{ \
         [Console]::Error.WriteLine(('Firma no válida: ' + $s.Status + ' / ' + $s.SignerCertificate.Subject)); exit 1 }}; \
         Write-Output $s.SignerCertificate.Subject"
    );
    let output = run_powershell(&script)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        bail!(
            "SST rechazó {} porque no pudo validar una firma Authenticode de Microsoft{}",
            exe.display(),
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    Ok(())
}

fn run_elevated_office_scrubber(exe: &Path, version: &str) -> Result<u32> {
    let verb = wide("runas");
    let exe_w = wide(exe.as_os_str());
    let params = format!("-S OfficeScrubScenario -AcceptEula -OfficeVersion {version}");
    let params_w = wide(params);
    let dir_w = wide(exe.parent().unwrap_or_else(|| Path::new(".")).as_os_str());

    let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe_w.as_ptr();
    info.lpParameters = params_w.as_ptr();
    info.lpDirectory = dir_w.as_ptr();
    info.nShow = SW_SHOWNORMAL;

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        bail!("no se pudo elevar la herramienta de desinstalación de Office mediante UAC");
    }
    if info.hProcess.is_null() {
        bail!("la herramienta de Office se inició sin un handle de proceso válido");
    }

    unsafe {
        WaitForSingleObject(info.hProcess, u32::MAX);
        let mut exit_code = 1u32;
        if GetExitCodeProcess(info.hProcess, &mut exit_code) == 0 {
            CloseHandle(info.hProcess);
            bail!("no se pudo obtener el resultado de la herramienta de Office");
        }
        CloseHandle(info.hProcess);
        Ok(exit_code)
    }
}

fn office_result(code: u32, version: &str, backend: &str) -> CommandOutput {
    match code {
        0 => CommandOutput::ok(format!(
            "{backend} completado para {version}. Reinicie Windows y ejecute tools office diagnose para auditar residuos.\n"
        )),
        6 => CommandOutput::error(
            format!("{backend}: hay aplicaciones de Office/Teams abiertas. Ciérrelas y vuelva a ejecutar la limpieza."),
            6,
        ),
        7 | 68 => CommandOutput::error(
            format!("{backend}: no se encontró una instalación de Office que coincida con la selección."),
            code as i32,
        ),
        8 => CommandOutput::error(
            format!("{backend}: se detectaron varias versiones de Office y no se pudo resolver la selección."),
            8,
        ),
        9 => CommandOutput::error(format!("{backend}: no se pudo quitar Office."), 9),
        10 => CommandOutput::error(format!("{backend}: la herramienta no quedó elevada."), 10),
        66 => CommandOutput::error(
            format!("{backend}: la versión solicitada no coincide con la instalada."),
            66,
        ),
        67 => CommandOutput::error(
            format!("{backend}: se rechazó la versión indicada."),
            67,
        ),
        other => CommandOutput::error(
            format!("{backend} terminó con código {other}."),
            other.min(i32::MAX as u32) as i32,
        ),
    }
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
