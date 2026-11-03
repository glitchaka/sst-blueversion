from __future__ import annotations

import argparse
import ctypes
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass
class Run:
    exit_code: int | None
    stdout: str
    stderr: str
    timed_out: bool
    duration_ms: float
    peak_mb: float


@dataclass
class Result:
    name: str
    group: str
    passed: bool
    exit_code: int | None
    timed_out: bool
    duration_ms: float
    peak_mb: float
    details: str


def normalize(value: str) -> str:
    return value.replace("\r\n", "\n").replace("\r", "\n")


def peak_working_set_mb(proc: subprocess.Popen[str]) -> float:
    if os.name != "nt":
        return 0.0
    try:
        class PMC(ctypes.Structure):
            _fields_ = [
                ("cb", ctypes.c_ulong),
                ("PageFaultCount", ctypes.c_ulong),
                ("PeakWorkingSetSize", ctypes.c_size_t),
                ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t),
                ("PeakPagefileUsage", ctypes.c_size_t),
            ]
        counters = PMC()
        counters.cb = ctypes.sizeof(PMC)
        ok = ctypes.windll.psapi.GetProcessMemoryInfo(
            ctypes.c_void_p(int(proc._handle)),
            ctypes.byref(counters),
            counters.cb,
        )
        return round(counters.PeakWorkingSetSize / (1024 * 1024), 2) if ok else 0.0
    except Exception:
        return 0.0


def native_crash(code: int | None) -> bool:
    if code is None:
        return False
    unsigned = code & 0xFFFFFFFF
    return unsigned >= 0xC0000000


class Harness:
    def __init__(self, exe: Path, scale: int) -> None:
        self.exe = exe.resolve()
        self.scale = max(1, scale)
        self.root = Path(tempfile.mkdtemp(prefix="sst-blackbox-"))
        self.home = self.root / "home"
        self.work = self.root / "work"
        self.home.mkdir()
        self.work.mkdir()
        self.outdir = Path.cwd() / "qa-results"
        self.outdir.mkdir(exist_ok=True)
        self.results: list[Result] = []
        self.warnings: list[str] = []

    def env(self) -> dict[str, str]:
        env = os.environ.copy()
        env["HOME"] = str(self.home)
        env["USERPROFILE"] = str(self.home)
        env["SST_FONT_COLOR"] = "#DFE8EF"
        env.pop("BASH_ENV", None)
        env.pop("SST_UPDATE_MANIFEST_URL", None)
        return env

    def run_program(
        self,
        file: str | Path,
        args: list[str] | None = None,
        stdin: str | None = None,
        timeout: float = 10.0,
        cwd: Path | None = None,
    ) -> Run:
        started = time.perf_counter()
        proc = subprocess.Popen(
            [str(file), *(args or [])],
            cwd=str(cwd or self.work),
            env=self.env(),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            creationflags=(subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0),
        )
        timed_out = False
        try:
            stdout, stderr = proc.communicate(input=stdin, timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            proc.kill()
            stdout, stderr = proc.communicate(timeout=3)
        peak = peak_working_set_mb(proc)
        elapsed = (time.perf_counter() - started) * 1000
        return Run(
            None if timed_out else proc.returncode,
            normalize(stdout),
            normalize(stderr),
            timed_out,
            round(elapsed, 2),
            peak,
        )

    def sst(self, args: list[str] | None = None, stdin: str | None = None, timeout: float = 10.0) -> Run:
        return self.run_program(self.exe, args, stdin, timeout)

    def shell(self, script: str, timeout: float = 10.0) -> Run:
        return self.sst(["--norc", "--noprofile", "-c", script], timeout=timeout)

    def add(self, name: str, group: str, passed: bool, run: Run, details: str = "") -> None:
        self.results.append(Result(name, group, passed, run.exit_code, run.timed_out, run.duration_ms, run.peak_mb, details))
        status = "PASS" if passed else "FAIL"
        print(f"{status} [{group}] {name} ({run.duration_ms:.2f} ms, {run.peak_mb:.2f} MB)")
        if not passed:
            print(f"  {details}")
            if run.stdout:
                print(f"  stdout=[{run.stdout}]")
            if run.stderr:
                print(f"  stderr=[{run.stderr}]")

    def exact(self, name: str, script: str, out: str, code: int = 0, err: str = "", group: str = "blackbox", timeout: float = 10.0) -> None:
        run = self.shell(script, timeout)
        ok = not run.timed_out and run.exit_code == code and run.stdout == normalize(out) and run.stderr == normalize(err)
        self.add(name, group, ok, run, f"expected exit={code} stdout=[{out}] stderr=[{err}]")

    def contains(self, name: str, args: list[str], needle: str, code: int = 0, group: str = "invocation", timeout: float = 10.0) -> None:
        run = self.sst(args, timeout=timeout)
        ok = not run.timed_out and run.exit_code == code and needle in (run.stdout + run.stderr)
        self.add(name, group, ok, run, f"expected exit={code} and text [{needle}]")

    def no_crash(self, name: str, script: str, group: str = "parser-fuzz", timeout: float = 5.0) -> None:
        run = self.shell(script, timeout)
        ok = not run.timed_out and run.exit_code is not None and not native_crash(run.exit_code)
        self.add(name, group, ok, run, "must terminate without timeout or native Windows crash")

    def write_report(self) -> int:
        failed = [r for r in self.results if not r.passed]
        report = {
            "executable": str(self.exe),
            "stress_scale": self.scale,
            "total": len(self.results),
            "passed": len(self.results) - len(failed),
            "failed": len(failed),
            "warnings": self.warnings,
            "results": [asdict(r) for r in self.results],
        }
        (self.outdir / "terminal-blackbox.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        lines = [
            "# SST terminal black-box / stress",
            "",
            f"- Total: {report['total']}",
            f"- Pass: {report['passed']}",
            f"- Fail: {report['failed']}",
            f"- Stress scale: {self.scale}",
            "",
            "| Group | Test | Result | Exit | ms | Peak MB |",
            "|---|---|---:|---:|---:|---:|",
        ]
        for r in self.results:
            lines.append(f"| {r.group} | {r.name} | {'PASS' if r.passed else 'FAIL'} | {r.exit_code} | {r.duration_ms:.2f} | {r.peak_mb:.2f} |")
        if failed:
            lines.extend(["", "## Failures"])
            lines.extend(f"- [{r.group}] {r.name}: {r.details}" for r in failed)
        if self.warnings:
            lines.extend(["", "## Warnings"])
            lines.extend(f"- {w}" for w in self.warnings)
        (self.outdir / "terminal-blackbox.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"=== SUMMARY: {report['passed']} passed / {report['failed']} failed / {report['total']} total ===")
        return 1 if failed else 0

    def close(self) -> None:
        shutil.rmtree(self.root, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    parser.add_argument("--stress-scale", type=int, default=1)
    ns = parser.parse_args()
    h = Harness(Path(ns.exe), ns.stress_scale)
    try:
        print("=== Invocation / transport ===")
        h.contains("version", ["--version"], "Shell Shock Tool", 0)
        h.contains("help", ["--help"], "-c COMANDO", 0)
        h.contains("embedded-pty-transport", ["--verify-terminal"], "Shell Shock Tool: prompt, entrada, ejecución y salida correctos.", 0, "transport", 20)

        print("=== Bash/Nwash language ===")
        cases = [
            ("echo", "echo hello", "hello\n", 0),
            ("printf", 'printf "%s:%d\\n" alpha 42', "alpha:42\n", 0),
            ("quoted-space", 'x="a b"; printf "<%s>\\n" "$x"', "<a b>\n", 0),
            ("unicode", 'printf "%s\\n" "áéí 日本 😀"', "áéí 日本 😀\n", 0),
            ("parameter-default", 'unset x; printf "%s\\n" "${x:-fallback}"', "fallback\n", 0),
            ("positional-default", 'f(){ printf "%s\\n" "${2:-0}"; }; f 8 3', "3\n", 0),
            ("parameter-length", 'x=abcdef; printf "%s\\n" "${#x}"', "6\n", 0),
            ("parameter-replace", 'x=abcabc; printf "%s\\n" "${x//ab/X}"', "XcXc\n", 0),
            ("parameter-substring", 'x=abcdef; printf "%s\\n" "${x:2:3}"', "cde\n", 0),
            ("parameter-trim", 'x=prefix-value-suffix; echo "${x#prefix-}"; echo "${x%-suffix}"', "value-suffix\nprefix-value\n", 0),
            ("arithmetic", 'x=20; printf "%s\\n" "$((x+22))"', "42\n", 0),
            ("arithmetic-assign", 'x=2; (( x += 3 )); printf "%s\\n" "$x"', "5\n", 0),
            ("function", 'f() { printf "f=%s\\n" "$1"; }; f hello', "f=hello\n", 0),
            ("local-scope", 'x=outer; f(){ local x=inner; echo "$x"; }; f; echo "$x"', "inner\nouter\n", 0),
            ("local-composition", 'sum(){ local base=$(printf 8); local bonus="${2:-0}"; local total=$((base+bonus)); echo "$total"; }; sum x 3', "11\n", 0),
            ("if-elif", 'x=7; if ((x>=10)); then echo high; elif ((x>=6)); then echo medium; else echo low; fi', "medium\n", 0),
            ("case", 'x=beta; case "$x" in alpha) echo a ;; beta|gamma) echo b ;; *) echo z ;; esac', "b\n", 0),
            ("for", 'for x in a b c; do echo "$x"; done', "a\nb\nc\n", 0),
            ("arithmetic-for", 'for ((i=0;i<4;i++)); do echo "$i"; done', "0\n1\n2\n3\n", 0),
            ("while", 'i=0; while ((i<3)); do echo "$i"; ((i++)); done', "0\n1\n2\n", 0),
            ("conditional", 'x=informe.txt; if [[ "$x" == *.txt && -n "$x" ]]; then echo yes; fi', "yes\n", 0),
            ("indexed-array", 'a=(zero one two); printf "%s|%s|%s\\n" "${a[0]}" "${a[2]}" "${#a[@]}"', "zero|two|3\n", 0),
            ("assoc-array", 'declare -A a; a[k]=value; printf "%s\\n" "${a[k]}"', "value\n", 0),
            ("cmd-sub", 'x=$(printf hello); echo "$x"', "hello\n", 0),
            ("nested-cmd-sub", 'x="$(printf "%s" "$(printf nested)")"; echo "$x"', "nested\n", 0),
            ("subshell", 'x=before; (x=inside; echo "$x"); echo "$x"', "inside\nbefore\n", 0),
            ("brace", 'printf "<%s>\\n" {1..3}', "<1>\n<2>\n<3>\n", 0),
            ("logical", 'false || echo recovered; true && echo ok', "recovered\nok\n", 0),
            ("break-continue", 'for x in 1 2 3 4; do [[ "$x" == 2 ]] && continue; echo "$x"; [[ "$x" == 3 ]] && break; done', "1\n3\n", 0),
            ("pipeline", 'printf "c\\na\\nb\\n" | sort', "a\nb\nc\n", 0),
            ("here-string", 'cat <<< "hello"', "hello\n", 0),
            ("pipefail", 'set -o pipefail; false | true; exit $?', "", 1),
            ("function-status", 'f(){ return 7; }; f; exit $?', "", 7),
            ("shift", 'set -- a b c; shift; printf "%s|%s|%s\\n" "$1" "$2" "$#"', "b|c|2\n", 0),
        ]
        for name, script, out, code in cases:
            h.exact(name, script, out, code)

        heredoc = "user=tester\nvalue=42\ncat <<EOF\nuser=$user\nvalue=$value\nEOF\n"
        run = h.sst(["--norc", "--noprofile", "-s"], stdin=heredoc)
        h.add("heredoc-expanded", "blackbox", not run.timed_out and run.exit_code == 0 and run.stdout == "user=tester\nvalue=42\n" and run.stderr == "", run, "expanded heredoc must feed cat and return status 0")

        quoted_heredoc = "x=secret\ncat <<'EOF'\n$x\nEOF\n"
        run = h.sst(["--norc", "--noprofile", "-s"], stdin=quoted_heredoc)
        h.add("heredoc-quoted", "blackbox", not run.timed_out and run.exit_code == 0 and run.stdout == "$x\n", run, "quoted heredoc delimiter must suppress expansion")

        run = h.shell('printf out; printf err >&2')
        h.add("stdout-stderr", "blackbox", not run.timed_out and run.exit_code == 0 and run.stdout == "out" and run.stderr == "err", run, "stdout/stderr separation")
        h.exact("external-cmd", 'cmd.exe /D /C echo external-ok', "external-ok\n")
        h.exact("external-exit", 'cmd.exe /D /C exit /b 13; exit $?', "", 13)

        print("=== Invocation modes ===")
        run = h.sst(["--norc", "--noprofile", "-s"], stdin="printf 'stdin-ok\\n'\n")
        h.add("stdin-script", "invocation", not run.timed_out and run.exit_code == 0 and run.stdout == "stdin-ok\n", run, "stdin -s must execute exactly once")
        script_file = h.work / "args-test.sh"
        script_file.write_text("printf '%s|%s|%s|%s\\n' \"$0\" \"$1\" \"$2\" \"$#\"\n", encoding="utf-8")
        run = h.sst(["--norc", "--noprofile", str(script_file), "uno", "dos"])
        h.add("script-positional", "invocation", not run.timed_out and run.exit_code == 0 and "|uno|dos|2\n" in run.stdout, run, "script positional args")
        h.exact("errexit", 'set -e; false; echo SHOULD_NOT_PRINT', "", 1, group="invocation")
        h.exact("noexec", 'set -n; echo SHOULD_NOT_PRINT', "", 0, group="invocation")
        run = h.sst(["--norc", "--noprofile", "-u", "-c", 'echo "$THIS_VARIABLE_DOES_NOT_EXIST"'])
        h.add("nounset", "invocation", not run.timed_out and run.exit_code not in (None, 0) and not native_crash(run.exit_code), run, "nounset must fail cleanly")
        for name, args in [("unknown-option", ["--definitely-invalid"]), ("missing-c", ["-c"])]:
            run = h.sst(args)
            h.add(name, "negative", not run.timed_out and run.exit_code not in (None, 0) and not native_crash(run.exit_code), run, "invalid invocation must fail cleanly")

        print("=== Filesystem / Unix ===")
        wd = h.work.as_posix()
        h.exact("redirect-append", f"cd '{wd}'; printf 'one\\n' > out.txt; printf 'two\\n' >> out.txt; cat out.txt", "one\ntwo\n", group="filesystem")
        h.exact("path-spaces", f"cd '{wd}'; mkdir -p 'space dir'; printf x > 'space dir/a.txt'; cat 'space dir/a.txt'", "x", group="filesystem")
        h.exact("copy-move-remove", f"cd '{wd}'; printf data > a.tmp; cp a.tmp b.tmp; mv b.tmp c.tmp; cat c.tmp; rm a.tmp c.tmp", "data", group="filesystem")
        unix_cases = [
            ("grep", 'printf "alpha\\nbeta\\ngamma\\n" | grep beta', "beta\n"),
            ("sed", 'printf "abc\\n" | sed "s/b/X/"', "aXc\n"),
            ("awk", "printf 'a b\\n' | awk '{print $2}'", "b\n"),
            ("cut", 'printf "a:b:c\\n" | cut -d: -f2', "b\n"),
            ("uniq", 'printf "a\\na\\nb\\n" | uniq', "a\nb\n"),
            ("wc", 'printf "a\\nb\\nc\\n" | wc -l', "3\n"),
            ("base64", 'printf abc | base64', "YWJj\n"),
        ]
        for name, script, out in unix_cases:
            h.exact(name, script, out, group="unix")
        hash_file = h.work / "hash.bin"
        hash_file.write_bytes(b"SST-blackbox-hash")
        import hashlib
        expected_hash = hashlib.sha256(hash_file.read_bytes()).hexdigest()
        run = h.shell(f"sha256sum '{hash_file.as_posix()}'")
        h.add("sha256sum", "unix", not run.timed_out and run.exit_code == 0 and expected_hash in run.stdout.lower(), run, f"expected {expected_hash}")

        print("=== Read-only SST smoke ===")
        for name, script in [
            ("whoami", "whoami"), ("uname", "uname"), ("uptime", "uptime"),
            ("free", "free"), ("df", "df"), ("ps", "ps"),
            ("sys-whoami", "sys whoami"), ("net-interfaces", "net interfaces"),
            ("sys-sensors", "sys sensors"), ("triage-status", "triage --status"),
            ("triage", "triage"),
        ]:
            timeout = 30 if name == "sys-sensors" else 15
            run = h.shell(script, timeout)
            h.add(name, "sst-smoke", not run.timed_out and run.exit_code == 0 and bool(run.stdout + run.stderr), run, "read-only command must finish and produce output")

        print("=== Differential against Git Bash ===")
        bash = next((Path(p) for p in [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files\Git\usr\bin\bash.exe"] if Path(p).exists()), None)
        diff_cases = [
            'x="a b"; printf "%s\\n" "$x"',
            'unset x; printf "%s\\n" "${x:-fallback}"',
            'f(){ printf "%s\\n" "${2:-0}"; }; f 8 3',
            'x=abcdef; printf "%s\\n" "${#x}"',
            'x=abcabc; printf "%s\\n" "${x//ab/X}"',
            'x=20; printf "%s\\n" "$((x+22))"',
            'f(){ printf "f=%s\\n" "$1"; }; f hello',
            'x=outer; f(){ local x=inner; printf "%s\\n" "$x"; }; f; printf "%s\\n" "$x"',
            'for ((i=0;i<3;i++)); do echo "$i"; done',
            'x=beta; case "$x" in alpha) echo a ;; beta|gamma) echo b ;; *) echo z ;; esac',
            'a=(zero one two); printf "%s|%s|%s\\n" "${a[0]}" "${a[2]}" "${#a[@]}"',
            'declare -A a; a[k]=value; printf "%s\\n" "${a[k]}"',
            'set -o pipefail; false | true; printf "%s\\n" "$?"',
            'x=before; (x=inside; echo "$x"); echo "$x"',
            'x="$(printf "%s" "$(printf nested)")"; echo "$x"',
            'printf "<%s>\\n" {1..3}',
            'cat <<< "hello"',
            'x=abcdef; echo "${x:2:3}"',
            'set -- a b c; shift; printf "%s|%s|%s\\n" "$1" "$2" "$#"',
        ]
        if bash:
            for idx, script in enumerate(diff_cases, 1):
                ours = h.shell(script)
                ref = h.run_program(bash, ["--noprofile", "--norc", "-c", script])
                ok = not ours.timed_out and not ref.timed_out and ours.exit_code == ref.exit_code and ours.stdout == ref.stdout
                h.add(f"bash-diff-{idx:02d}", "bash-differential", ok, ours, f"bash exit={ref.exit_code} stdout=[{ref.stdout}] stderr=[{ref.stderr}]")
        else:
            h.warnings.append("Git Bash unavailable; differential suite skipped")

        print("=== Malformed / deterministic fuzz ===")
        malformed = [
            "'unterminated", '"unterminated', '$(echo', '$((1+', 'if true; then',
            'for x in a b; do', 'case x in a) echo x', '[[ x ==', '{ echo x;', '( echo x',
            'echo >', 'echo 2>&', '${', '${x//', 'function', 'f() {', 'while false; do',
            'until true; do', 'select x in a b; do', 'coproc {'
        ]
        for idx, script in enumerate(malformed, 1):
            h.no_crash(f"malformed-{idx:02d}", script, "parser-negative")
        atoms = ["echo", "x=1", ";", "&&", "||", "|", "(", ")", "{", "}", "[[", "]]", "<", ">", ">>", "2>", '"x"', "'y'", "${x}", "$(printf z)", "*", "?", "[a-z]"]
        rng = random.Random(4640)
        for idx in range(80 * h.scale):
            script = " ".join(rng.choice(atoms) for _ in range(rng.randint(2, 11)))
            h.no_crash(f"fuzz-{idx+1:03d}", script)

        print("=== Stress ===")
        loops = 25_000 * h.scale
        run = h.shell(f'i=0; while ((i<{loops})); do ((i++)); done; echo "$i"', 60)
        h.add("arithmetic-loop", "stress", not run.timed_out and run.exit_code == 0 and run.stdout == f"{loops}\n" and (run.peak_mb == 0 or run.peak_mb < 768), run, f"{loops} iterations; peak < 768 MB")
        pipes = 10_000 * h.scale
        run = h.shell(f'for ((i=0;i<{pipes};i++)); do echo "$i"; done | wc -l', 60)
        h.add("large-pipeline", "stress", not run.timed_out and run.exit_code == 0 and run.stdout.strip() == str(pipes) and (run.peak_mb == 0 or run.peak_mb < 768), run, f"{pipes} lines")
        big = h.work / "big.bin"
        size = 16 * 1024 * 1024 * h.scale
        rng_bytes = random.Random(12345)
        with big.open("wb") as fh:
            remaining = size
            while remaining:
                chunk = bytes(rng_bytes.randrange(256) for _ in range(min(65536, remaining)))
                fh.write(chunk)
                remaining -= len(chunk)
        expected = hashlib.sha256(big.read_bytes()).hexdigest()
        run = h.shell(f"sha256sum '{big.as_posix()}'", 60)
        h.add("large-file-sha256", "stress", not run.timed_out and run.exit_code == 0 and expected in run.stdout.lower() and (run.peak_mb == 0 or run.peak_mb < 768), run, f"{size} bytes")
        run = h.shell(f"cat '{big.as_posix()}' | wc -c", 60)
        h.add("large-binary-pipeline", "stress", not run.timed_out and run.exit_code == 0 and run.stdout.strip() == str(size) and (run.peak_mb == 0 or run.peak_mb < 768), run, f"expected {size} bytes")
        huge = ("#" + "x" * 200 + "\n") * (5000 * h.scale) + "echo LARGE_STDIN_OK\n"
        run = h.sst(["--norc", "--noprofile", "-s"], stdin=huge, timeout=60)
        h.add("megabyte-stdin", "stress", not run.timed_out and run.exit_code == 0 and run.stdout.strip() == "LARGE_STDIN_OK" and (run.peak_mb == 0 or run.peak_mb < 768), run, "large stdin script")

        seq_count = 100 * h.scale
        started = time.perf_counter()
        seq_bad = 0
        peak = 0.0
        for _ in range(seq_count):
            r = h.shell("true")
            seq_bad += int(r.timed_out or r.exit_code != 0 or native_crash(r.exit_code))
            peak = max(peak, r.peak_mb)
        seq = Run(0, "", "", False, round((time.perf_counter() - started) * 1000, 2), peak)
        h.add("sequential-process-burst", "stress", seq_bad == 0, seq, f"{seq_count} launches; failures={seq_bad}")

        parallel_count = 32 * h.scale
        procs: list[subprocess.Popen[str]] = []
        for _ in range(parallel_count):
            procs.append(subprocess.Popen(
                [str(h.exe), "--norc", "--noprofile", "-c", "true"],
                cwd=str(h.work), env=h.env(), stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                encoding="utf-8", errors="replace",
                creationflags=(subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0),
            ))
        bad = 0
        peak = 0.0
        deadline = time.monotonic() + 30
        for p in procs:
            remaining = max(0.1, deadline - time.monotonic())
            try:
                p.communicate(timeout=remaining)
            except subprocess.TimeoutExpired:
                p.kill(); p.communicate(); bad += 1
            peak = max(peak, peak_working_set_mb(p))
            if p.returncode != 0 or native_crash(p.returncode):
                bad += 1
        par = Run(0, "", "", False, 0.0, peak)
        h.add("parallel-process-burst", "stress", bad == 0, par, f"{parallel_count} concurrent launches; failures={bad}")

        latencies = []
        for _ in range(20):
            r = h.shell("true")
            if not r.timed_out and r.exit_code == 0:
                latencies.append(r.duration_ms)
        if latencies:
            latencies.sort()
            median = latencies[len(latencies)//2]
            p95 = latencies[min(len(latencies)-1, int(len(latencies)*0.95))]
            print(f"startup latency median={median:.2f} ms p95={p95:.2f} ms")
            if p95 > 3000:
                h.warnings.append(f"CLI startup p95 high: {p95:.2f} ms")

        return h.write_report()
    finally:
        h.close()


if __name__ == "__main__":
    raise SystemExit(main())
