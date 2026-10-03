param(
    [string]$Exe = ".\target\debug\sst.exe"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Exe)) {
    throw "No existe $Exe. Ejecuta cargo build primero."
}

$cases = @(
    @{ Name="quotes"; Script='x="a b"; printf "%s\n" "$x"' },
    @{ Name="single-vs-double-quotes"; Script='x=world; printf "%s|%s\n" ''$x'' "$x"' },
    @{ Name="parameter-default"; Script='unset x; printf "%s\n" "${x:-fallback}"' },
    @{ Name="parameter-length"; Script='x=abcdef; printf "%s\n" "${#x}"' },
    @{ Name="parameter-replace"; Script='x=abcabc; printf "%s\n" "${x//a/X}"' },
    @{ Name="arithmetic"; Script='x=20; printf "%s\n" "$((x+22))"' },
    @{ Name="arithmetic-assignment"; Script='x=2; (( x += 3 )); printf "%s\n" "$x"' },
    @{ Name="arithmetic-for"; Script='for ((i=0; i<3; i++)); do printf "%s" "$i"; done; echo' },
    @{ Name="function"; Script='f() { printf "f=%s\n" "$1"; }; f hello' },
    @{ Name="function-local"; Script='x=outer; f() { local x=inner; printf "%s\n" "$x"; }; f; echo "$x"' },
    @{ Name="if"; Script='if true; then echo yes; else echo no; fi' },
    @{ Name="elif"; Script='n=7; if (( n >= 10 )); then echo high; elif (( n >= 6 )); then echo mid; else echo low; fi' },
    @{ Name="for"; Script='for x in a b c; do echo "$x"; done' },
    @{ Name="while"; Script='i=0; while (( i < 3 )); do echo "$i"; ((i++)); done' },
    @{ Name="case"; Script='x=beta; case "$x" in alpha) echo no ;; beta|gamma) echo yes ;; *) echo fallback ;; esac' },
    @{ Name="conditional"; Script='x=informe.txt; if [[ "$x" == *.txt && -n "$x" ]]; then echo yes; else echo no; fi' },
    @{ Name="indexed-array"; Script='a=(uno dos tres); printf "%s|%s|%s\n" "${a[0]}" "${a[2]}" "${#a[@]}"' },
    @{ Name="assoc-array"; Script='declare -A a; a[language]=Bash; a[platform]=Windows; printf "%s|%s\n" "${a[language]}" "${a[platform]}"' },
    @{ Name="pipeline"; Script='printf "c\na\nb\n" | sort' },
    @{ Name="pipeline-status"; Script='set -o pipefail; false | true; printf "%s\n" "$?"' },
    @{ Name="redirection"; Script='f=$(mktemp); printf "hola\n" > "$f"; cat "$f"; rm -f "$f"' },
    @{ Name="subshell"; Script='x=before; (x=inside; echo "$x"); echo "$x"' },
    @{ Name="command-substitution"; Script='x=$(printf hello); echo "$x"' },
    @{ Name="nested-command-substitution"; Script='printf "%s\n" "$(printf "%s" "$(printf nested)")"' },
    @{ Name="logical"; Script='false || echo recovered; true && echo ok' },
    @{ Name="brace-expansion"; Script='printf "%s\n" item-{1..3}' },
    @{ Name="here-document"; Script='cat <<EOF
uno
dos
EOF' }
)

$failed = 0

foreach ($case in $cases) {
    $ours = & $Exe -c $case.Script 2>&1 | Out-String
    $oursExit = $LASTEXITCODE

    $bash = wsl.exe bash -c $case.Script 2>&1 | Out-String
    $bashExit = $LASTEXITCODE

    if ($ours -ceq $bash -and $oursExit -eq $bashExit) {
        Write-Host "PASS $($case.Name)"
    }
    else {
        $failed++
        Write-Host "FAIL $($case.Name)" -ForegroundColor Red
        Write-Host "  SST exit=$oursExit output=[$ours]"
        Write-Host "  WSL exit=$bashExit output=[$bash]"
    }
}

if ($failed -gt 0) {
    throw "$failed prueba(s) de conformidad fallaron"
}

Write-Host "Conformidad básica: OK"
