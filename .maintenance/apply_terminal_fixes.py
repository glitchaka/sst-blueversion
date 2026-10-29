from pathlib import Path
import re


def read(path):
    return Path(path).read_text(encoding="utf-8")


def write(path, text):
    Path(path).write_text(text, encoding="utf-8", newline="\n")


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, got {count}")
    return text.replace(old, new, 1)


# 1) Positional parameters participate in ${N:-...}, nounset, etc.
p = "src/core/bash/environment.rs"
s = read(p)
s = replace_once(
    s,
    '''        if let Some(array) = self.assoc_arrays.get(name) {
            return array.contains_key("0");
        }
        self.vars.contains_key(name)
''',
    '''        if let Some(array) = self.assoc_arrays.get(name) {
            return array.contains_key("0");
        }
        if let Ok(index) = name.parse::<usize>() {
            return index == 0 || (index > 0 && index <= self.positional.len());
        }
        self.vars.contains_key(name)
''',
    "positional is_set",
)
write(p, s)

# 2) ShellEngine supports explicit stdin for pipeline children.
p = "src/core/ports.rs"
s = read(p)
s = replace_once(
    s,
    '''    fn working_dir(&self) -> &Path;
    fn execute(&mut self, line: &str) -> Result<ShellExecution>;
    fn set_arguments(&mut self, name: &str, args: &[String]);
''',
    '''    fn working_dir(&self) -> &Path;
    fn execute(&mut self, line: &str) -> Result<ShellExecution>;
    fn execute_with_stdin(&mut self, line: &str, stdin: Option<&[u8]>) -> Result<ShellExecution> {
        let _ = stdin;
        self.execute(line)
    }
    fn set_arguments(&mut self, name: &str, args: &[String]);
''',
    "ShellEngine stdin API",
)
write(p, s)

# 3) Nwash execution fixes: noexec, deferred heredoc expansion and pipeline stdin.
p = "src/core/bash/interpreter.rs"
s = read(p)
s = replace_once(
    s,
    "    fn execute_text_with_stdin(\n",
    "    pub(crate) fn execute_text_with_stdin(\n",
    "expose execute_text_with_stdin",
)
s = replace_once(
    s,
    '''        environment.unset("__SST_NWASH_STREAM_CHILD");
        environment.unset("__SST_NWASH_PARENT_STATUS");
''',
    '''        environment.unset("__SST_NWASH_STREAM_CHILD");
        environment.unset("__SST_NWASH_PIPE_STDIN");
        environment.unset("__SST_NWASH_PARENT_STATUS");
''',
    "hide pipeline stdin marker",
)
s = replace_once(
    s,
    '''                for node in nodes {
                    last.append(self.execute(node, stdin)?);
                    if last.exit_requested || last.flow != FlowSignal::None { break; }
''',
    '''                for node in nodes {
                    last.append(self.execute(node, stdin)?);
                    if self.env.option_enabled("noexec") { break; }
                    if last.exit_requested || last.flow != FlowSignal::None { break; }
''',
    "set -n sequence semantics",
)
s = replace_once(
    s,
    '''            if !spec.quoted {
                body = self.expand_scalar(&body)?;
            }
''',
    '''            if !spec.quoted {
                body = format!("\\u{1f}SST_HEREDOC_EXPAND\\u{1f}{body}");
            }
''',
    "defer heredoc expansion",
)
marker = "    fn expand_words(&mut self, words: &[String]) -> Result<Vec<String>> {"
helper = '''    fn expand_here_input(&mut self, target: &str) -> Result<String> {
        const DEFERRED_HEREDOC: &str = "\\u{1f}SST_HEREDOC_EXPAND\\u{1f}";
        let mut value = self.expand_scalar(target)?;
        if let Some(raw) = value.strip_prefix(DEFERRED_HEREDOC) {
            value = self.expand_scalar(raw)?;
        }
        value.push('\\n');
        Ok(value)
    }

'''
if helper.strip() not in s:
    if marker not in s:
        raise SystemExit("expand_here_input insertion marker not found")
    s = s.replace(marker, helper + marker, 1)
pattern = re.compile(r"let mut value = self\.expand_scalar\(&redirect\.target\)\?;\s*value\.push\('\\n'\);")
s, count = pattern.subn("let value = self.expand_here_input(&redirect.target)?;", s)
if count < 1:
    raise SystemExit("HereString consumers: no expansion sites replaced")
write(p, s)

# 4) Brace expansion reaches the expander instead of becoming a command group.
p = "src/core/bash/lexer.rs"
s = read(p)
s = replace_once(
    s,
    '''            '{' if word.is_empty() => {
                let mut end = i + 1;
''',
    '''            '{' if word.is_empty() => {
                if let Some(relative_end) = chars[i + 1..].iter().position(|ch| *ch == '}') {
                    let close = i + 1 + relative_end;
                    let inner: String = chars[i + 1..close].iter().collect();
                    let brace_expansion = !inner.is_empty()
                        && !inner.chars().any(char::is_whitespace)
                        && (inner.contains("..") || inner.contains(','));
                    if brace_expansion {
                        word.extend(&chars[i..=close]);
                        i = close + 1;
                        continue;
                    }
                }
                let mut end = i + 1;
''',
    "brace expansion lexer",
)
write(p, s)

# 5) Native host preserves empty stderr and passes pipe stdin into Nwash children.
p = "src/adapters/shell/native_engine.rs"
s = read(p)
s = replace_once(
    s,
    '''                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                if status != 0 && stdout.trim().is_empty() && stderr.trim().is_empty() {
                    stderr = format!(
                        "{program}: el proceso terminó sin salida con código {status}\\n"
                    );
                }
                return Ok(ExecutionResult::from_parts(stdout, stderr, status));
''',
    '''                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                return Ok(ExecutionResult::from_parts(stdout, stderr, status));
''',
    "external empty stderr",
)
s = replace_once(
    s,
    '''            if index == 0 {
                if stdin.is_some() {
                    command.stdin(Stdio::piped());
                } else {
                    command.stdin(Stdio::inherit());
                }
            } else {
                let upstream = previous_stdout.take()
                    .ok_or_else(|| anyhow::anyhow!("pipeline: stdout anterior no disponible"))?;
                command.stdin(Stdio::from(upstream));
            }

            let mut child = command.spawn()
''',
    '''            if index == 0 {
                if stdin.is_some() {
                    command.stdin(Stdio::piped());
                } else {
                    command.stdin(Stdio::inherit());
                }
            } else {
                let upstream = previous_stdout.take()
                    .ok_or_else(|| anyhow::anyhow!("pipeline: stdout anterior no disponible"))?;
                command.stdin(Stdio::from(upstream));
            }
            if index > 0 || stdin.is_some() {
                command.env("__SST_NWASH_PIPE_STDIN", "1");
            }

            let mut child = command.spawn()
''',
    "pipeline child stdin marker",
)
s = replace_once(
    s,
    '''    fn execute(&mut self, line: &str) -> Result<ShellExecution> {
        self.interrupt.store(false, std::sync::atomic::Ordering::SeqCst);
        self.force_abort.store(false, std::sync::atomic::Ordering::SeqCst);
        let result = self.interpreter.execute_text(line)?;

        let mut execution = if result.exit_requested {
            ShellExecution::exit(result.status)
        } else {
            ShellExecution::continue_running(result.status)
        };
        execution.stdout = result.stdout;
        execution.stderr = result.stderr;
        Ok(execution)
    }
''',
    '''    fn execute(&mut self, line: &str) -> Result<ShellExecution> {
        self.execute_with_stdin(line, None)
    }

    fn execute_with_stdin(&mut self, line: &str, stdin: Option<&[u8]>) -> Result<ShellExecution> {
        self.interrupt.store(false, std::sync::atomic::Ordering::SeqCst);
        self.force_abort.store(false, std::sync::atomic::Ordering::SeqCst);
        let result = self.interpreter.execute_text_with_stdin(line, stdin)?;

        let mut execution = if result.exit_requested {
            ShellExecution::exit(result.status)
        } else {
            ShellExecution::continue_running(result.status)
        };
        execution.stdout = result.stdout;
        execution.stderr = result.stderr;
        Ok(execution)
    }
''',
    "NativeShellEngine stdin implementation",
)
write(p, s)

# 6) CLI pipeline children read their actual OS pipe as bytes.
p = "src/main.rs"
s = read(p)
s = replace_once(
    s,
    '''    engine.set_arguments(&script_name, &positional);
    let result = engine.execute(&source)?;
    print!("{}", result.stdout);
''',
    '''    engine.set_arguments(&script_name, &positional);
    let piped_stdin = if std::env::var("__SST_NWASH_PIPE_STDIN").as_deref() == Ok("1") {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        Some(bytes)
    } else {
        None
    };
    let result = engine.execute_with_stdin(&source, piped_stdin.as_deref())?;
    print!("{}", result.stdout);
''',
    "CLI pipeline stdin",
)
write(p, s)

# 7) Automatic update runs at GUI startup. No network means unverified, never current.
p = "src/adapters/terminal/embedded.rs"
s = read(p)
s = replace_once(
    s,
    '''            let result = (|| -> Result<()> {
                let (mut engine, _, paths) = crate::composition::build_engine()?;
                engine.set_interactive(true);
''',
    '''            let result = (|| -> Result<()> {
                let paths = crate::adapters::persistence::AppPaths::detect();
                paths.ensure_layout()?;
                if let Err(error) = crate::application::update::startup_update(&paths, |line| {
                    let _ = io::write(line.as_bytes());
                }) {
                    io::write(format!(
                        "\\r\\nActualizaciones: no se pudo comprobar el canal ({error}).\\r\\n\\r\\n"
                    ).as_bytes())?;
                }
                if crate::application::update::schedule_pending_update(&paths)? {
                    return Ok(());
                }

                let (mut engine, _, _) = crate::composition::build_engine()?;
                engine.set_interactive(true);
''',
    "GUI startup updater",
)
write(p, s)

# 8) GUI idle resource use: 30 Hz terminal tick; CPU+RAM only every 2 seconds.
p = "src/presentation/gui_slint.rs"
s = read(p)
s = replace_once(s, "let metrics = Rc::new(RefCell::new(System::new_all()));", "let metrics = Rc::new(RefCell::new(System::new()));", "sysinfo allocation")
s = replace_once(s, "timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {", "timer.start(TimerMode::Repeated, Duration::from_millis(33), move || {", "GUI frame cadence")
s = replace_once(s, "if last_status.borrow().elapsed() >= Duration::from_secs(1) {", "if last_status.borrow().elapsed() >= Duration::from_secs(2) {", "status cadence")
s = replace_once(s, "                system.refresh_all();\n", "                system.refresh_cpu_usage();\n                system.refresh_memory();\n", "selective sysinfo refresh")
write(p, s)

# 9) Updater swap: verified same-directory .new, real delay, rollback.
p = "src/application/update.rs"
s = read(p)
s = s.replace("    thread,\n", "")
s, removed_bg = re.subn(r"pub fn spawn_background_check\(paths: AppPaths\) \{.*?\n\}\n\n", "", s, count=1, flags=re.S)
s, removed_stage = re.subn(r"pub fn check_and_stage\(paths: &AppPaths\) -> Result<bool> \{.*?\n\}\n\n", "", s, count=1, flags=re.S)
if removed_bg != 1 or removed_stage != 1:
    raise SystemExit(f"legacy updater removal failed: background={removed_bg}, stage={removed_stage}")
s = s.replace("Actualización {} preparada y verificada. Se aplicará al cerrar SST.", "Actualización {} preparada y verificada. Aplicando actualización...")
start = s.find("pub fn schedule_pending_update(paths: &AppPaths) -> Result<bool> {")
if start < 0:
    raise SystemExit("schedule_pending_update not found")
brace = s.find("{", start)
depth = 0
end = None
for i in range(brace, len(s)):
    if s[i] == "{":
        depth += 1
    elif s[i] == "}":
        depth -= 1
        if depth == 0:
            end = i + 1
            break
if end is None:
    raise SystemExit("schedule_pending_update closing brace not found")
replacement = r'''pub fn schedule_pending_update(paths: &AppPaths) -> Result<bool> {
    let Some(pending) = load_pending(paths)? else {
        return Ok(false);
    };
    if !is_newer(&pending.version, current_version())? {
        cleanup_obsolete_pending(paths)?;
        return Ok(false);
    }

    let staged = PathBuf::from(&pending.staged_path);
    verify_file(&staged, &pending.sha256, None)?;
    let target = std::env::current_exe().context("no se pudo resolver sst.exe")?;
    let swap_new = target.with_extension("exe.new");
    let backup = target.with_extension("exe.old");
    if swap_new != staged {
        let _ = fs::remove_file(&swap_new);
        fs::copy(&staged, &swap_new)
            .context("no se pudo preparar sst.exe.new junto al ejecutable activo")?;
    }
    verify_file(&swap_new, &pending.sha256, None)?;

    let update_dir = update_dir(paths);
    fs::create_dir_all(&update_dir)?;
    let script = update_dir.join(APPLY_SCRIPT);
    let pending_file = pending_path(paths);
    let applied_staged = update_dir.join(APPLIED_STAGED_FILE);
    let applied_file = update_dir.join(APPLIED_FILE);
    let applied = AppliedUpdate {
        from_version: if pending.from_version.trim().is_empty() {
            current_version().to_owned()
        } else {
            pending.from_version.clone()
        },
        version: pending.version.clone(),
        applied_at: Utc::now().to_rfc3339(),
        changes: pending.changes.clone(),
    };
    fs::write(&applied_staged, serde_json::to_vec_pretty(&applied)?)?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let cmd = system32_executable("cmd.exe")?;
        let ping = system32_executable("ping.exe")?;
        let body = format!(
            "@echo off\r\n\
             setlocal DisableDelayedExpansion\r\n\
             set \"TARGET={}\"\r\n\
             set \"NEW={}\"\r\n\
             set \"BACKUP={}\"\r\n\
             set \"PENDING={}\"\r\n\
             set \"APPLIED_PENDING={}\"\r\n\
             set \"APPLIED={}\"\r\n\
             set \"PING={}\"\r\n\
             if exist \"%BACKUP%\" del /Q \"%BACKUP%\" >nul 2>&1\r\n\
             for /L %%I in (1,1,120) do (\r\n\
               move /Y \"%TARGET%\" \"%BACKUP%\" >nul 2>&1 && goto replace\r\n\
               \"%PING%\" -n 2 127.0.0.1 >nul\r\n\
             )\r\n\
             exit /b 10\r\n\
             :replace\r\n\
             move /Y \"%NEW%\" \"%TARGET%\" >nul 2>&1\r\n\
             if errorlevel 1 goto rollback\r\n\
             if not exist \"%TARGET%\" goto rollback\r\n\
             move /Y \"%APPLIED_PENDING%\" \"%APPLIED%\" >nul 2>&1\r\n\
             del /Q \"%PENDING%\" >nul 2>&1\r\n\
             del /Q \"%BACKUP%\" >nul 2>&1\r\n\
             start \"\" \"%TARGET%\"\r\n\
             del /Q \"%~f0\" >nul 2>&1\r\n\
             exit /b 0\r\n\
             :rollback\r\n\
             if exist \"%BACKUP%\" move /Y \"%BACKUP%\" \"%TARGET%\" >nul 2>&1\r\n\
             exit /b 11\r\n",
            batch_value(&target),
            batch_value(&swap_new),
            batch_value(&backup),
            batch_value(&pending_file),
            batch_value(&applied_staged),
            batch_value(&applied_file),
            batch_value(&ping),
        );
        fs::write(&script, body)?;
        Command::new(cmd)
            .args(["/D", "/S", "/C"])
            .arg(&script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("no se pudo iniciar el aplicador de actualización")?;
    }

    #[cfg(not(windows))]
    {
        let _ = (&script, &swap_new, &backup);
        bail!("la aplicación automática de updates sólo está disponible en Windows");
    }

    Ok(true)
}'''
s = s[:start] + replacement + s[end:]
write(p, s)

# 10) Remove dead references to deleted migration APIs.
p = "src/app.rs"
s = read(p)
s, count = re.subn(
    r'''        // update\.rs still exposes these compatibility entry points.*?crate::application::update::check_and_stage;\n\n''',
    "",
    s,
    count=1,
    flags=re.S,
)
if count != 1:
    raise SystemExit(f"legacy app references: expected 1 block, got {count}")
write(p, s)

print("Terminal fixes applied")
