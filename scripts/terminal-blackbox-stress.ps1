param(
    [Parameter(Mandatory = $true)]
    [string]$Exe,
    [int]$StressScale = 1
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Exe = (Resolve-Path -LiteralPath $Exe).Path
$root = Join-Path $env:RUNNER_TEMP ("sst-blackbox-" + [guid]::NewGuid().ToString('N'))
$home = Join-Path $root 'home'
$work = Join-Path $root 'work'
$resultDir = Join-Path $PWD 'qa-results'
New-Item -ItemType Directory -Force -Path $home,$work,$resultDir | Out-Null

$results = [System.Collections.Generic.List[object]]::new()
$failures = [System.Collections.Generic.List[string]]::new()
$warnings = [System.Collections.Generic.List[string]]::new()

function Normalize-Text([string]$value) {
    if ($null -eq $value) { return '' }
    return ($value -replace "`r`n", "`n") -replace "`r", "`n"
}

function Invoke-Program {
    param(
        [Parameter(Mandatory = $true)][string]$File,
        [string[]]$Arguments = @(),
        [string]$Stdin = $null,
        [int]$TimeoutMs = 10000,
        [string]$WorkingDirectory = $work
    )

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $File
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.WorkingDirectory = $WorkingDirectory
    foreach ($arg in $Arguments) { [void]$psi.ArgumentList.Add($arg) }
    $psi.Environment['HOME'] = $home
    $psi.Environment['USERPROFILE'] = $home
    $psi.Environment['SST_FONT_COLOR'] = '#DFE8EF'
    if ($psi.Environment.ContainsKey('BASH_ENV')) { [void]$psi.Environment.Remove('BASH_ENV') }
    if ($psi.Environment.ContainsKey('SST_UPDATE_MANIFEST_URL')) { [void]$psi.Environment.Remove('SST_UPDATE_MANIFEST_URL') }

    $proc = [System.Diagnostics.Process]::new()
    $proc.StartInfo = $psi
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    [void]$proc.Start()
    $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
    $stderrTask = $proc.StandardError.ReadToEndAsync()

    if ($null -ne $Stdin) { $proc.StandardInput.Write($Stdin) }
    $proc.StandardInput.Close()

    $timedOut = -not $proc.WaitForExit($TimeoutMs)
    if ($timedOut) {
        try { $proc.Kill($true) } catch {}
        try { $proc.WaitForExit(3000) | Out-Null } catch {}
    }
    $sw.Stop()

    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $exitCode = if ($timedOut) { $null } else { $proc.ExitCode }
    $peak = 0L
    try { $peak = $proc.PeakWorkingSet64 } catch {}
    $proc.Dispose()

    [pscustomobject]@{
        ExitCode = $exitCode
        Stdout = (Normalize-Text $stdout)
        Stderr = (Normalize-Text $stderr)
        TimedOut = $timedOut
        DurationMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
        PeakWorkingSetMB = [math]::Round($peak / 1MB, 2)
    }
}

function Invoke-Sst {
    param(
        [string[]]$Arguments = @(),
        [string]$Stdin = $null,
        [int]$TimeoutMs = 10000,
        [string]$WorkingDirectory = $work
    )
    Invoke-Program -File $Exe -Arguments $Arguments -Stdin $Stdin -TimeoutMs $TimeoutMs -WorkingDirectory $WorkingDirectory
}

function Add-Result {
    param([string]$Name,[string]$Group,[bool]$Passed,[object]$Run,[string]$Details='')
    $results.Add([pscustomobject]@{
        Name = $Name
        Group = $Group
        Passed = $Passed
        ExitCode = $Run.ExitCode
        TimedOut = $Run.TimedOut
        DurationMs = $Run.DurationMs
        PeakWorkingSetMB = $Run.PeakWorkingSetMB
        Details = $Details
    })
    if ($Passed) {
        Write-Host ("PASS [{0}] {1} ({2} ms, {3} MB)" -f $Group,$Name,$Run.DurationMs,$Run.PeakWorkingSetMB)
    } else {
        $failures.Add("[$Group] $Name :: $Details")
        Write-Host ("FAIL [{0}] {1}: {2}" -f $Group,$Name,$Details) -ForegroundColor Red
        if ($Run.Stdout) { Write-Host "  stdout: [$($Run.Stdout)]" }
        if ($Run.Stderr) { Write-Host "  stderr: [$($Run.Stderr)]" }
    }
}

function Assert-Exact {
    param([string]$Name,[string]$Script,[string]$ExpectedOut,[int]$ExpectedExit=0,[string]$ExpectedErr='',[int]$TimeoutMs=10000,[string]$Group='blackbox')
    $run = Invoke-Sst -Arguments @('--norc','--noprofile','-c',$Script) -TimeoutMs $TimeoutMs
    $passed = (-not $run.TimedOut) -and ($run.ExitCode -eq $ExpectedExit) -and ($run.Stdout -ceq (Normalize-Text $ExpectedOut)) -and ($run.Stderr -ceq (Normalize-Text $ExpectedErr))
    Add-Result -Name $Name -Group $Group -Passed $passed -Run $run -Details ("expected exit=$ExpectedExit stdout=[$ExpectedOut] stderr=[$ExpectedErr]")
}

function Assert-Contains {
    param([string]$Name,[string[]]$Arguments,[string]$Needle,[int]$ExpectedExit=0,[int]$TimeoutMs=10000,[string]$Group='blackbox')
    $run = Invoke-Sst -Arguments $Arguments -TimeoutMs $TimeoutMs
    $passed = (-not $run.TimedOut) -and ($run.ExitCode -eq $ExpectedExit) -and (($run.Stdout + $run.Stderr).Contains($Needle))
    Add-Result -Name $Name -Group $Group -Passed $passed -Run $run -Details ("expected exit=$ExpectedExit and text [$Needle]")
}

function Assert-NonZero {
    param([string]$Name,[string[]]$Arguments,[int]$TimeoutMs=10000,[string]$Group='negative')
    $run = Invoke-Sst -Arguments $Arguments -TimeoutMs $TimeoutMs
    $passed = (-not $run.TimedOut) -and ($null -ne $run.ExitCode) -and ($run.ExitCode -ne 0) -and ($run.ExitCode -ge 0)
    Add-Result -Name $Name -Group $Group -Passed $passed -Run $run -Details 'expected bounded non-zero exit without process crash'
}

function Assert-NoCrash {
    param([string]$Name,[string]$Script,[int]$TimeoutMs=5000,[string]$Group='fuzz')
    $run = Invoke-Sst -Arguments @('--norc','--noprofile','-c',$Script) -TimeoutMs $TimeoutMs
    $passed = (-not $run.TimedOut) -and ($null -ne $run.ExitCode) -and ($run.ExitCode -ge 0)
    Add-Result -Name $Name -Group $Group -Passed $passed -Run $run -Details 'must return without timeout or native crash'
}

Write-Host '=== Invocation / transport ==='
Assert-Contains 'version' @('--version') 'Shell Shock Tool' 0 5000 'invocation'
Assert-Contains 'help' @('--help') '-c COMANDO' 0 5000 'invocation'
Assert-Contains 'embedded-pty-transport' @('--verify-terminal') 'SST_NATIVE_OK' 0 20000 'transport'

Write-Host '=== Language black-box ==='
Assert-Exact 'echo' 'echo hello' "hello`n"
Assert-Exact 'printf' 'printf "%s:%d\n" alpha 42' "alpha:42`n"
Assert-Exact 'single-double-quote' 'x="a b"; printf "<%s>\n" "$x"' "<a b>`n"
Assert-Exact 'unicode' 'printf "%s\n" "áéí 日本 😀"' "áéí 日本 😀`n"
Assert-Exact 'parameter-default' 'unset x; printf "%s\n" "${x:-fallback}"' "fallback`n"
Assert-Exact 'parameter-length' 'x=abcdef; printf "%s\n" "${#x}"' "6`n"
Assert-Exact 'parameter-replace' 'x=abcabc; printf "%s\n" "${x//ab/X}"' "XcXc`n"
Assert-Exact 'arithmetic' 'x=20; printf "%s\n" "$((x+22))"' "42`n"
Assert-Exact 'arithmetic-assignment' 'x=2; (( x += 3 )); printf "%s\n" "$x"' "5`n"
Assert-Exact 'function' 'f() { printf "f=%s\n" "$1"; }; f hello' "f=hello`n"
Assert-Exact 'local-scope' 'x=outer; f() { local x=inner; echo "$x"; }; f; echo "$x"' "inner`nouter`n"
Assert-Exact 'if-elif' 'x=7; if ((x>=10)); then echo high; elif ((x>=6)); then echo medium; else echo low; fi' "medium`n"
Assert-Exact 'case' 'x=beta; case "$x" in alpha) echo a ;; beta|gamma) echo b ;; *) echo z ;; esac' "b`n"
Assert-Exact 'for-loop' 'for x in a b c; do echo "$x"; done' "a`nb`nc`n"
Assert-Exact 'arithmetic-for' 'for ((i=0; i<4; i++)); do echo "$i"; done' "0`n1`n2`n3`n"
Assert-Exact 'while-loop' 'i=0; while ((i<3)); do echo "$i"; ((i++)); done' "0`n1`n2`n"
Assert-Exact 'conditional' 'x=informe.txt; if [[ "$x" == *.txt && -n "$x" ]]; then echo yes; fi' "yes`n"
Assert-Exact 'indexed-array' 'a=(zero one two); printf "%s|%s|%s\n" "${a[0]}" "${a[2]}" "${#a[@]}"' "zero|two|3`n"
Assert-Exact 'assoc-array' 'declare -A a; a[k]=value; printf "%s\n" "${a[k]}"' "value`n"
Assert-Exact 'command-substitution' 'x=$(printf hello); echo "$x"' "hello`n"
Assert-Exact 'nested-command-substitution' 'x="$(printf "%s" "$(printf nested)")"; echo "$x"' "nested`n"
Assert-Exact 'subshell-isolation' 'x=before; (x=inside; echo "$x"); echo "$x"' "inside`nbefore`n"
Assert-Exact 'brace-expansion' 'printf "<%s>\n" {1..3}' "<1>`n<2>`n<3>`n"
Assert-Exact 'logical-operators' 'false || echo recovered; true && echo ok' "recovered`nok`n"
Assert-Exact 'break-continue' 'for x in 1 2 3 4; do [[ "$x" == 2 ]] && continue; echo "$x"; [[ "$x" == 3 ]] && break; done' "1`n3`n"
Assert-Exact 'pipeline' 'printf "c\na\nb\n" | sort' "a`nb`nc`n"
Assert-Exact 'here-string' 'cat <<< "hello"' "hello`n"
Assert-Exact 'stdout-stderr' 'printf out; printf err >&2' 'out' 0 'err'
Assert-Exact 'pipefail' 'set -o pipefail; false | true; exit $?' '' 1
Assert-Exact 'function-return-status' 'f() { return 7; }; f; exit $?' '' 7
Assert-Exact 'external-cmd' 'cmd.exe /D /C echo external-ok' "external-ok`n"
Assert-Exact 'external-exit-code' 'cmd.exe /D /C exit /b 13; exit $?' '' 13

Write-Host '=== Invocation modes / scripts / stdin ==='
$stdinRun = Invoke-Sst -Arguments @('--norc','--noprofile','-s') -Stdin "printf 'stdin-ok\n'`n"
Add-Result 'stdin-script' 'invocation' ((-not $stdinRun.TimedOut) -and $stdinRun.ExitCode -eq 0 -and $stdinRun.Stdout -ceq "stdin-ok`n") $stdinRun 'stdin -s must execute exactly once'

$scriptFile = Join-Path $work 'args-test.sh'
[IO.File]::WriteAllText($scriptFile, "printf '%s|%s|%s|%s\n' \"`$0\" \"`$1\" \"`$2\" \"`$#\"`n", [Text.UTF8Encoding]::new($false))
$scriptRun = Invoke-Sst -Arguments @('--norc','--noprofile',$scriptFile,'uno','dos')
$scriptOk = (-not $scriptRun.TimedOut) -and $scriptRun.ExitCode -eq 0 -and $scriptRun.Stdout.Contains('|uno|dos|2')
Add-Result 'script-positional-args' 'invocation' $scriptOk $scriptRun 'script must receive $0/$1/$2/$#'

Assert-Exact 'errexit' 'set -e; false; echo SHOULD_NOT_PRINT' '' 1
Assert-Exact 'noexec' 'set -n; echo SHOULD_NOT_PRINT' '' 0
$nounset = Invoke-Sst -Arguments @('--norc','--noprofile','-u','-c','echo "$THIS_VARIABLE_DOES_NOT_EXIST"')
Add-Result 'nounset' 'invocation' ((-not $nounset.TimedOut) -and $nounset.ExitCode -ne 0 -and $nounset.ExitCode -ge 0) $nounset 'nounset must fail cleanly'
Assert-NonZero 'unknown-option' @('--definitely-invalid')
Assert-NonZero 'missing-c-argument' @('-c')

Write-Host '=== Filesystem / Unix layer ==='
Assert-Exact 'redirection-append' "cd '$($work -replace '\\','/')'; printf 'one\n' > out.txt; printf 'two\n' >> out.txt; cat out.txt" "one`ntwo`n" 0 '' 10000 'filesystem'
Assert-Exact 'path-with-spaces' "cd '$($work -replace '\\','/')'; mkdir -p 'space dir'; printf x > 'space dir/a.txt'; cat 'space dir/a.txt'" 'x' 0 '' 10000 'filesystem'
Assert-Exact 'copy-move-remove' "cd '$($work -replace '\\','/')'; printf data > a.tmp; cp a.tmp b.tmp; mv b.tmp c.tmp; cat c.tmp; rm a.tmp c.tmp" 'data' 0 '' 10000 'filesystem'
Assert-Exact 'grep' 'printf "alpha\nbeta\ngamma\n" | grep beta' "beta`n" 0 '' 10000 'unix'
Assert-Exact 'sed' 'printf "abc\n" | sed "s/b/X/"' "aXc`n" 0 '' 10000 'unix'
Assert-Exact 'awk' 'printf "a b\n" | awk "{print $2}"' "b`n" 0 '' 10000 'unix'
Assert-Exact 'cut' 'printf "a:b:c\n" | cut -d: -f2' "b`n" 0 '' 10000 'unix'
Assert-Exact 'uniq' 'printf "a\na\nb\n" | uniq' "a`nb`n" 0 '' 10000 'unix'
Assert-Exact 'wc-lines' 'printf "a\nb\nc\n" | wc -l' "3`n" 0 '' 10000 'unix'
Assert-Exact 'base64' 'printf abc | base64' "YWJj`n" 0 '' 10000 'unix'

$hashFile = Join-Path $work 'hash.bin'
[IO.File]::WriteAllBytes($hashFile, [Text.Encoding]::UTF8.GetBytes('SST-blackbox-hash'))
$expectedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $hashFile).Hash.ToLowerInvariant()
$hashRun = Invoke-Sst -Arguments @('--norc','--noprofile','-c',"sha256sum '$($hashFile -replace '\\','/')'")
$hashOk = (-not $hashRun.TimedOut) -and $hashRun.ExitCode -eq 0 -and $hashRun.Stdout.ToLowerInvariant().Contains($expectedHash)
Add-Result 'sha256sum' 'unix' $hashOk $hashRun "expected hash $expectedHash"

Write-Host '=== Read-only SST administrative smoke tests ==='
foreach ($smoke in @(
    @{Name='whoami'; Script='whoami'},
    @{Name='uname'; Script='uname'},
    @{Name='uptime'; Script='uptime'},
    @{Name='free'; Script='free'},
    @{Name='df'; Script='df'},
    @{Name='ps'; Script='ps'},
    @{Name='sys-whoami'; Script='sys whoami'},
    @{Name='net-interfaces'; Script='net interfaces'}
)) {
    $run = Invoke-Sst -Arguments @('--norc','--noprofile','-c',$smoke.Script) -TimeoutMs 15000
    $ok = (-not $run.TimedOut) -and $run.ExitCode -eq 0 -and (($run.Stdout + $run.Stderr).Length -gt 0)
    Add-Result $smoke.Name 'sst-smoke' $ok $run 'read-only command must complete successfully and produce output'
}

Write-Host '=== Differential Bash compatibility (Git Bash reference) ==='
$bashCandidates = @('C:\Program Files\Git\bin\bash.exe','C:\Program Files\Git\usr\bin\bash.exe')
$bash = $bashCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if ($bash) {
    $diffCases = @(
        'x="a b"; printf "%s\n" "$x"',
        'unset x; printf "%s\n" "${x:-fallback}"',
        'x=abcdef; printf "%s\n" "${#x}"',
        'x=abcabc; printf "%s\n" "${x//ab/X}"',
        'x=20; printf "%s\n" "$((x+22))"',
        'x=2; (( x += 3 )); printf "%s\n" "$x"',
        'f() { printf "f=%s\n" "$1"; }; f hello',
        'x=outer; f() { local x=inner; printf "%s\n" "$x"; }; f; printf "%s\n" "$x"',
        'if true; then echo yes; else echo no; fi',
        'x=7; if ((x>=10)); then echo high; elif ((x>=6)); then echo medium; else echo low; fi',
        'for x in a b c; do echo "$x"; done',
        'for ((i=0; i<3; i++)); do echo "$i"; done',
        'i=0; while ((i<3)); do echo "$i"; ((i++)); done',
        'x=beta; case "$x" in alpha) echo a ;; beta|gamma) echo b ;; *) echo z ;; esac',
        'x=informe.txt; if [[ "$x" == *.txt && -n "$x" ]]; then echo yes; fi',
        'a=(zero one two); printf "%s|%s|%s\n" "${a[0]}" "${a[2]}" "${#a[@]}"',
        'declare -A a; a[k]=value; printf "%s\n" "${a[k]}"',
        'printf "c\na\nb\n" | sort',
        'set -o pipefail; false | true; printf "%s\n" "$?"',
        'false || echo recovered; true && echo ok',
        'x=before; (x=inside; echo "$x"); echo "$x"',
        'x=$(printf hello); echo "$x"',
        'x="$(printf "%s" "$(printf nested)")"; echo "$x"',
        'printf "<%s>\n" {1..3}',
        'cat <<< "hello"',
        'f() { return 7; }; f; printf "%s\n" "$?"',
        'for x in 1 2 3 4; do [[ "$x" == 2 ]] && continue; echo "$x"; [[ "$x" == 3 ]] && break; done',
        'x=abcdef; echo "${x:2:3}"',
        'x=prefix-value-suffix; echo "${x#prefix-}"; echo "${x%-suffix}"',
        'set -- a b c; shift; printf "%s|%s|%s\n" "$1" "$2" "$#"'
    )
    $i = 0
    foreach ($script in $diffCases) {
        $i++
        $ours = Invoke-Sst -Arguments @('--norc','--noprofile','-c',$script) -TimeoutMs 10000
        $ref = Invoke-Program -File $bash -Arguments @('--noprofile','--norc','-c',$script) -TimeoutMs 10000
        $ok = (-not $ours.TimedOut) -and (-not $ref.TimedOut) -and $ours.ExitCode -eq $ref.ExitCode -and $ours.Stdout -ceq $ref.Stdout -and $ours.Stderr -ceq $ref.Stderr
        Add-Result ("bash-diff-{0:D2}" -f $i) 'bash-differential' $ok $ours ("bash exit=$($ref.ExitCode) stdout=[$($ref.Stdout)] stderr=[$($ref.Stderr)]")
    }
} else {
    $warnings.Add('Git Bash not available; differential suite skipped.')
}

Write-Host '=== Parser robustness / malformed input ==='
$malformed = @(
    "'unterminated",
    '"unterminated',
    '$(echo',
    '$((1+',
    'if true; then',
    'for x in a b; do',
    'case x in a) echo x',
    '[[ x ==',
    '{ echo x;',
    '( echo x',
    'echo >',
    'echo 2>&',
    '${',
    '${x//',
    'function',
    'f() {',
    'while false; do',
    'until true; do',
    'select x in a b; do',
    'coproc {'
)
$i = 0
foreach ($script in $malformed) {
    $i++
    Assert-NoCrash ("malformed-{0:D2}" -f $i) $script 5000 'parser-negative'
}

# Deterministic fuzz corpus: weird token adjacency, quoting and redirection. The
# contract is only that SST must not hang or terminate through a native crash.
$atoms = @('echo','x=1',';','&&','||','|','(',')','{','}','[[',']]','<','>','>>','2>','"x"',"'y'",'${x}','$(printf z)','*','?','[a-z]')
$rng = [Random]::new(4640)
for ($case = 0; $case -lt (80 * [math]::Max(1,$StressScale)); $case++) {
    $parts = [System.Collections.Generic.List[string]]::new()
    $count = $rng.Next(2,12)
    for ($j=0; $j -lt $count; $j++) { $parts.Add($atoms[$rng.Next(0,$atoms.Count)]) }
    Assert-NoCrash ("fuzz-{0:D3}" -f ($case+1)) ($parts -join ' ') 4000 'parser-fuzz'
}

Write-Host '=== Stress ==='
$loopCount = 25000 * [math]::Max(1,$StressScale)
$run = Invoke-Sst -Arguments @('--norc','--noprofile','-c',"i=0; while ((i<$loopCount)); do ((i++)); done; echo \"`$i\"") -TimeoutMs 60000
$ok = (-not $run.TimedOut) -and $run.ExitCode -eq 0 -and $run.Stdout -ceq "$loopCount`n" -and $run.PeakWorkingSetMB -lt 768
Add-Result 'arithmetic-loop' 'stress' $ok $run "expected $loopCount iterations, peak memory < 768 MB"

$pipeCount = 10000 * [math]::Max(1,$StressScale)
$run = Invoke-Sst -Arguments @('--norc','--noprofile','-c',"for ((i=0;i<$pipeCount;i++)); do echo \"`$i\"; done | wc -l") -TimeoutMs 60000
$ok = (-not $run.TimedOut) -and $run.ExitCode -eq 0 -and $run.Stdout.Trim() -eq [string]$pipeCount -and $run.PeakWorkingSetMB -lt 768
Add-Result 'large-pipeline' 'stress' $ok $run "expected $pipeCount lines, peak memory < 768 MB"

$bigFile = Join-Path $work 'big.bin'
$bytes = New-Object byte[] (16MB * [math]::Max(1,$StressScale))
[Random]::new(12345).NextBytes($bytes)
[IO.File]::WriteAllBytes($bigFile,$bytes)
$bigHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $bigFile).Hash.ToLowerInvariant()
$run = Invoke-Sst -Arguments @('--norc','--noprofile','-c',"sha256sum '$($bigFile -replace '\\','/')'") -TimeoutMs 60000
$ok = (-not $run.TimedOut) -and $run.ExitCode -eq 0 -and $run.Stdout.ToLowerInvariant().Contains($bigHash) -and $run.PeakWorkingSetMB -lt 768
Add-Result 'large-file-sha256' 'stress' $ok $run '16 MiB+ file hash must match and stay bounded'

$run = Invoke-Sst -Arguments @('--norc','--noprofile','-c',"cat '$($bigFile -replace '\\','/')' | wc -c") -TimeoutMs 60000
$expectedBytes = (Get-Item -LiteralPath $bigFile).Length
$ok = (-not $run.TimedOut) -and $run.ExitCode -eq 0 -and $run.Stdout.Trim() -eq [string]$expectedBytes -and $run.PeakWorkingSetMB -lt 768
Add-Result 'large-binary-pipeline' 'stress' $ok $run "expected $expectedBytes bytes through pipeline"

$hugeComment = ('#' + ('x' * 200) + "`n") * (5000 * [math]::Max(1,$StressScale))
$hugeScript = $hugeComment + "echo LARGE_STDIN_OK`n"
$run = Invoke-Sst -Arguments @('--norc','--noprofile','-s') -Stdin $hugeScript -TimeoutMs 60000
$ok = (-not $run.TimedOut) -and $run.ExitCode -eq 0 -and $run.Stdout.Trim() -eq 'LARGE_STDIN_OK' -and $run.PeakWorkingSetMB -lt 768
Add-Result 'megabyte-stdin-script' 'stress' $ok $run 'large stdin script must parse and execute without runaway memory'

$sequential = 100 * [math]::Max(1,$StressScale)
$sw = [Diagnostics.Stopwatch]::StartNew()
$seqFailures = 0
$seqPeak = 0.0
for ($i=0; $i -lt $sequential; $i++) {
    $r = Invoke-Sst -Arguments @('--norc','--noprofile','-c','true') -TimeoutMs 10000
    if ($r.TimedOut -or $r.ExitCode -ne 0) { $seqFailures++ }
    $seqPeak = [math]::Max($seqPeak,$r.PeakWorkingSetMB)
}
$sw.Stop()
$seqRun = [pscustomobject]@{ExitCode=0;Stdout='';Stderr='';TimedOut=$false;DurationMs=[math]::Round($sw.Elapsed.TotalMilliseconds,2);PeakWorkingSetMB=$seqPeak}
Add-Result 'sequential-process-burst' 'stress' ($seqFailures -eq 0) $seqRun "$sequential launches; failures=$seqFailures"

$parallel = 32 * [math]::Max(1,$StressScale)
$procs = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
for ($i=0; $i -lt $parallel; $i++) {
    $psi = [Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $Exe
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.ArgumentList.Add('--norc'); $psi.ArgumentList.Add('--noprofile'); $psi.ArgumentList.Add('-c'); $psi.ArgumentList.Add('true')
    $psi.Environment['HOME'] = $home; $psi.Environment['USERPROFILE'] = $home
    $p = [Diagnostics.Process]::new(); $p.StartInfo = $psi; [void]$p.Start(); $procs.Add($p)
}
$parallelFailures = 0
$parallelPeak = 0.0
foreach ($p in $procs) {
    if (-not $p.WaitForExit(20000)) { try {$p.Kill($true)} catch {}; $parallelFailures++; continue }
    if ($p.ExitCode -ne 0) { $parallelFailures++ }
    try { $parallelPeak = [math]::Max($parallelPeak,[math]::Round($p.PeakWorkingSet64 / 1MB,2)) } catch {}
    $p.Dispose()
}
$parallelRun = [pscustomobject]@{ExitCode=0;Stdout='';Stderr='';TimedOut=$false;DurationMs=0;PeakWorkingSetMB=$parallelPeak}
Add-Result 'parallel-process-burst' 'stress' ($parallelFailures -eq 0) $parallelRun "$parallel concurrent launches; failures=$parallelFailures"

Write-Host '=== Startup latency sample ==='
$latencies = [System.Collections.Generic.List[double]]::new()
for ($i=0; $i -lt 20; $i++) {
    $r = Invoke-Sst -Arguments @('--norc','--noprofile','-c','true') -TimeoutMs 10000
    if (-not $r.TimedOut -and $r.ExitCode -eq 0) { $latencies.Add([double]$r.DurationMs) }
}
if ($latencies.Count -gt 0) {
    $sorted = $latencies | Sort-Object
    $median = $sorted[[int][math]::Floor($sorted.Count/2)]
    $p95 = $sorted[[math]::Min($sorted.Count-1,[int][math]::Floor($sorted.Count*0.95))]
    Write-Host "Startup latency median=$median ms p95=$p95 ms"
    if ($p95 -gt 3000) { $warnings.Add("CLI startup p95 is high: $p95 ms") }
}

$passed = @($results | Where-Object Passed).Count
$failed = @($results | Where-Object { -not $_.Passed }).Count
$summary = [ordered]@{
    executable = $Exe
    stressScale = $StressScale
    total = $results.Count
    passed = $passed
    failed = $failed
    warnings = $warnings
    failures = $failures
    generatedAt = [DateTime]::UtcNow.ToString('o')
    results = $results
}
$summary | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $resultDir 'terminal-blackbox.json') -Encoding utf8

$md = [System.Collections.Generic.List[string]]::new()
$md.Add('# SST terminal black-box / stress')
$md.Add('')
$md.Add("- Total: $($results.Count)")
$md.Add("- Pass: $passed")
$md.Add("- Fail: $failed")
$md.Add("- Stress scale: $StressScale")
$md.Add('')
$md.Add('| Group | Test | Result | Exit | ms | Peak MB |')
$md.Add('|---|---|---:|---:|---:|---:|')
foreach ($r in $results) { $md.Add("| $($r.Group) | $($r.Name) | $(if($r.Passed){'PASS'}else{'FAIL'}) | $($r.ExitCode) | $($r.DurationMs) | $($r.PeakWorkingSetMB) |") }
if ($failures.Count) { $md.Add(''); $md.Add('## Failures'); foreach ($f in $failures) { $md.Add("- $f") } }
if ($warnings.Count) { $md.Add(''); $md.Add('## Warnings'); foreach ($w in $warnings) { $md.Add("- $w") } }
[IO.File]::WriteAllLines((Join-Path $resultDir 'terminal-blackbox.md'),$md,[Text.UTF8Encoding]::new($false))

Write-Host "=== SUMMARY: $passed passed / $failed failed / $($results.Count) total ==="
if ($warnings.Count) { Write-Host ("Warnings: " + ($warnings -join '; ')) -ForegroundColor Yellow }
if ($failed -gt 0) { throw "$failed terminal black-box/stress test(s) failed" }
