#Requires -RunAsAdministrator
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][ValidatePattern('^S-1-5-21-(\d+-){3}\d+$')][string]$OperatorSid,
    [Parameter(Mandatory = $true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256
)
$ErrorActionPreference = 'Stop'
$serviceName = 'SSTPrivilegedBroker'
$source = (Resolve-Path -LiteralPath $Executable).ProviderPath
if ([IO.Path]::GetExtension($source) -ne '.exe') { throw 'Executable must be an .exe file.' }
if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $ExpectedSha256) {
    throw 'Source hash differs from the explicitly approved artifact.'
}
$sid = [Security.Principal.SecurityIdentifier]::new($OperatorSid)
if ($sid.Value -ne $OperatorSid) { throw 'Operator SID must be canonical.' }
# Resolve once to reject nonexistent/well-known/group-like configuration mistakes.
$null = $sid.Translate([Security.Principal.NTAccount])
$programFiles = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)
$install = [IO.Path]::GetFullPath((Join-Path $programFiles $serviceName))
if ([IO.Path]::GetDirectoryName($install) -ne $programFiles.TrimEnd('\')) { throw 'Invalid install root.' }
for ($ancestor = Get-Item -LiteralPath $programFiles; $null -ne $ancestor; $ancestor = $ancestor.Parent) {
    if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse points in install ancestry are refused.' }
}
if (Test-Path -LiteralPath $install) { throw 'Install directory already exists. No automatic overwrite or update is allowed.' }
if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) { throw 'Service already exists.' }
if (-not $PSCmdlet.ShouldProcess($install, "Install LocalSystem broker for $OperatorSid (stopped)")) { return }

# Explicit owner is essential: a non-elevated user owning a file could rewrite its DACL.
$directoryAcl = [Security.AccessControl.DirectorySecurity]::new()
$directoryAcl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)')
$null = New-Item -ItemType Directory -Path $install
Set-Acl -LiteralPath $install -AclObject $directoryAcl
$destination = Join-Path $install 'sst.exe'
Copy-Item -LiteralPath $source -Destination $destination
$fileAcl = [Security.AccessControl.FileSecurity]::new()
$fileAcl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)')
Set-Acl -LiteralPath $destination -AclObject $fileAcl
if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $ExpectedSha256) {
    throw 'Copied artifact hash mismatch; service was not registered.'
}
$audit = Join-Path $install 'broker-audit.jsonl'
$null = New-Item -ItemType File -Path $audit
$auditAcl = [Security.AccessControl.FileSecurity]::new()
$auditAcl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)')
Set-Acl -LiteralPath $audit -AclObject $auditAcl
$binaryPath = '"{0}" --broker-service --operator-sid {1}' -f $destination, $OperatorSid
# New-Service without credentials uses LocalSystem. Manual startup avoids implicit activation.
$null = New-Service -Name $serviceName -DisplayName 'SST privileged process broker' -BinaryPathName $binaryPath -StartupType Manual
# Interactive users may query SCM identity, but cannot start/stop/reconfigure the service.
& "$env:SystemRoot\System32\sc.exe" sdset $serviceName 'D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;CCLCLORC;;;IU)'
if ($LASTEXITCODE -ne 0) { throw 'Service ACL failed. Service remains stopped; fix permissions before starting it.' }
& "$env:SystemRoot\System32\sc.exe" privs $serviceName 'SeChangeNotifyPrivilege/SeImpersonatePrivilege/SeDebugPrivilege'
if ($LASTEXITCODE -ne 0) { throw 'Required privileges configuration failed; service remains stopped.' }
Write-Output "Installed, stopped: $serviceName"
Write-Output 'Start-Service SSTPrivilegedBroker must be run explicitly by an administrator.'
