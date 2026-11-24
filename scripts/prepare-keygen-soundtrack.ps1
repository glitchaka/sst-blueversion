# Reconstruct the two author-supplied MP3s from versioned Base64 fragments.
# CI runs this before packaging. No operator action or external download.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$encoded = Join-Path $root 'assets\keygen\encoded'
$out = Join-Path $root 'assets\keygen'
$expected = @(
  @{ Index=1; Parts=10; Size=4394990; Sha256='ffecd8d5d7c96dfba062aedc02742a5fad1987f2367a1ef14bb71a7f1419013d' },
  @{ Index=2; Parts=8; Size=3729695; Sha256='e49c316532be0e8c235ceb08a208f52821c0e07479715e143346a3177972b516' }
)
New-Item -ItemType Directory -Force -Path $out | Out-Null
foreach ($track in $expected) {
  $name = 'track{0:D2}' -f $track.Index
  $builder = [System.Text.StringBuilder]::new()
  for ($i=0; $i -lt $track.Parts; $i++) {
    $part = Join-Path $encoded ('{0}.part{1:D2}.txt' -f $name, $i)
    if (-not (Test-Path -LiteralPath $part -PathType Leaf)) {
      throw "Falta fragmento original: $part"
    }
    $data = [IO.File]::ReadAllText($part).Trim()
    if ($data -notmatch '^[A-Za-z0-9+/=]+$') { throw "Base64 inválido: $part" }
    [void]$builder.Append($data)
  }
  try { $bytes = [Convert]::FromBase64String($builder.ToString()) }
  catch { throw "No se pudo recuperar $($name): $($_.Exception.Message)" }
  if ($bytes.Length -ne $track.Size) {
    throw "Tamaño inválido de $($name): $($bytes.Length), esperado $($track.Size)"
  }
  $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
  if ($hash -ne $track.Sha256) { throw "SHA256 inválido de $($name): $hash" }
  $file = Join-Path $out "$name.mp3"
  [IO.File]::WriteAllBytes($file, $bytes)
  Write-Host "SST $name.mp3 recuperado y verificado ($($bytes.Length) bytes)"
}
