//! In-process interpreter session. The window talks to a Rust worker through channels.
use std::{collections::HashMap, fs, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc}, thread, time::{Duration, Instant}};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use super::io::{self, WindowIo};

enum WorkerRequest {
    Execute(String),
    Complete {
        line: String,
        cursor: usize,
        reply: mpsc::Sender<Vec<String>>,
    },
    PrepareHistory {
        line: String,
        reply: mpsc::Sender<std::result::Result<(String, bool), String>>,
    },
    RunBinding {
        command: String,
        line: String,
        cursor: usize,
        reply: mpsc::Sender<std::result::Result<(String, usize, String, String), String>>,
    },
}

pub struct EmbeddedSession {
    commands: mpsc::Sender<WorkerRequest>, keys: mpsc::Sender<Event>, display: mpsc::Sender<Vec<u8>>,
    pub output: mpsc::Receiver<Vec<u8>>,
    size: Arc<Mutex<(u16,u16)>>, raw: Arc<AtomicBool>, busy: Arc<AtomicBool>,
    interrupt: Arc<AtomicBool>, force_abort: Arc<AtomicBool>, exited: Arc<AtomicBool>,
    config_reload_requested: Arc<AtomicBool>,
    line: Vec<char>, cursor: usize, selection_anchor: Option<usize>, history: Vec<String>, history_index: usize,
    pending: String, bindings: Arc<Mutex<HashMap<String, String>>>,
    secondary_prompt: Arc<Mutex<String>>,
    timeout: Arc<Mutex<Option<Duration>>>,
    last_activity: Arc<Mutex<Instant>>,
}
fn readline_sequence(event: &KeyEvent) -> Option<String> {
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(ch) = event.code {
            return Some(format!("\\C-{}", ch.to_ascii_lowercase()));
        }
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        if let KeyCode::Char(ch) = event.code {
            return Some(format!("\\M-{ch}"));
        }
    }
    match event.code {
        KeyCode::Tab => Some("\\C-i".to_owned()),
        KeyCode::Backspace => Some("\\C-h".to_owned()),
        KeyCode::Up => Some("\\e[A".to_owned()),
        KeyCode::Down => Some("\\e[B".to_owned()),
        KeyCode::Right => Some("\\e[C".to_owned()),
        KeyCode::Left => Some("\\e[D".to_owned()),
        KeyCode::Home => Some("\\e[H".to_owned()),
        KeyCode::End => Some("\\e[F".to_owned()),
        KeyCode::Delete => Some("\\e[3~".to_owned()),
        _ => None,
    }
}

impl EmbeddedSession {
    pub fn start(cols: u16, rows: u16) -> Result<Self> {
        let (commands, requests) = mpsc::channel::<WorkerRequest>();
        let (keys, key_events) = mpsc::channel();
        let (display, output) = mpsc::channel();
        let size = Arc::new(Mutex::new((cols, rows)));
        let raw = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(true));
        let interrupt = Arc::new(AtomicBool::new(false));
        let force_abort = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let config_reload_requested = Arc::new(AtomicBool::new(false));
        let bindings = Arc::new(Mutex::new(HashMap::new()));
        let secondary_prompt = Arc::new(Mutex::new("> ".to_owned()));
        let timeout = Arc::new(Mutex::new(None));
        let last_activity = Arc::new(Mutex::new(Instant::now()));
        let terminal_io = WindowIo::new(
            display.clone(),
            key_events,
            size.clone(),
            raw.clone(),
            interrupt.clone(),
            force_abort.clone(),
        );
        let worker_busy = busy.clone(); let worker_exited = exited.clone();
        let worker_config_reload_requested = config_reload_requested.clone();
        let worker_bindings = bindings.clone();
        let worker_secondary_prompt = secondary_prompt.clone();
        let worker_timeout = timeout.clone();
        let worker_last_activity = last_activity.clone();
        thread::spawn(move || {
            io::install(terminal_io);
            let result = (|| -> Result<()> {
                let (mut engine, _, paths) = crate::composition::build_engine()?;
                engine.set_interactive(true);

                // The native GUI is an interactive non-login Bash session. Unlike
                // run_cli(), it does not pass through main's startup loader, so load
                // ~/.bashrc here to preserve Bash startup semantics.
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"));
                if let Some(home) = home {
                    let bashrc = PathBuf::from(home).join(".bashrc");
                    if bashrc.is_file() {
                        let raw = bashrc.to_string_lossy();
                        let quoted = format!("'{}'", raw.replace('\'', "'\\''"));
                        let startup = engine.execute(&format!("source {quoted}"))?;
                        io::write(startup.stdout.as_bytes())?;
                        io::write(startup.stderr.as_bytes())?;
                    }
                }

                *worker_bindings.lock().unwrap_or_else(|error| error.into_inner()) = engine.readline_bindings();

                io::write(crate::presentation::shell::prompt::banner().as_bytes())?;

                let security = crate::application::security::shared_security_service(paths.clone());
                let report = security.run_startup_preload(|line| {
                    let _ = io::write(line.as_bytes());
                });
                io::write(security.render_startup(&report).as_bytes())?;

                let (prompt_stdout, prompt_stderr, bash_prompt) = engine.prepare_prompt(false)?;
                io::write(prompt_stdout.as_bytes())?;
                io::write(prompt_stderr.as_bytes())?;
                let prompt = bash_prompt.unwrap_or_else(|| crate::presentation::shell::prompt::render(engine.working_dir()));
                io::write(prompt.as_bytes())?;
                let (_, _, ps2) = engine.prepare_prompt(true)?;
                *worker_secondary_prompt.lock().unwrap_or_else(|error| error.into_inner()) =
                    ps2.unwrap_or_else(|| "> ".to_owned());
                *worker_timeout.lock().unwrap_or_else(|error| error.into_inner()) = engine.input_timeout();
                *worker_last_activity.lock().unwrap_or_else(|error| error.into_inner()) = Instant::now();
                worker_busy.store(false, Ordering::SeqCst);
                for request in requests {
                    match request {
                        WorkerRequest::Execute(command) => {
                            if !command.starts_with("__SST_EOF_CHECK") {
                                let _ = engine.execute("__SST_IGNOREEOF=0");
                            }
                            let ps0 = engine.pre_execute_prompt()?;
                            if !ps0.is_empty() { io::write(ps0.as_bytes())?; }
                            match engine.execute(&command) {
                                Ok(result) => {
                                    io::write(result.stdout.as_bytes())?;
                                    io::write(result.stderr.as_bytes())?;
                                    if result.status == 0 {
                                        let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
                                        if normalized == "reload" || normalized == "config reload" {
                                            worker_config_reload_requested.store(true, Ordering::SeqCst);
                                        }
                                        if crate::support::windows::take_shell_handoff_request() {
                                            break;
                                        }
                                    }
                                    if result.exit_requested { break; }
                                }
                                Err(error) => io::write(format!("sst: {error}\n").as_bytes())?,
                            }
                            *worker_bindings.lock().unwrap_or_else(|error| error.into_inner()) = engine.readline_bindings();
                            let (prompt_stdout, prompt_stderr, bash_prompt) = engine.prepare_prompt(false)?;
                            io::write(prompt_stdout.as_bytes())?;
                            io::write(prompt_stderr.as_bytes())?;
                            let prompt = bash_prompt.unwrap_or_else(|| crate::presentation::shell::prompt::render(engine.working_dir()));
                            io::write(prompt.as_bytes())?;
                            let (_, _, ps2) = engine.prepare_prompt(true)?;
                            *worker_secondary_prompt.lock().unwrap_or_else(|error| error.into_inner()) =
                                ps2.unwrap_or_else(|| "> ".to_owned());
                            *worker_timeout.lock().unwrap_or_else(|error| error.into_inner()) = engine.input_timeout();
                            *worker_last_activity.lock().unwrap_or_else(|error| error.into_inner()) = Instant::now();
                            worker_busy.store(false, Ordering::SeqCst);
                        }
                        WorkerRequest::Complete { line, cursor, reply } => {
                            let matches = engine.complete(&line, cursor).unwrap_or_default();
                            let _ = reply.send(matches);
                        }
                        WorkerRequest::PrepareHistory { line, reply } => {
                            let prepared = engine.prepare_history(&line)
                                .and_then(|(expanded, print_only)| {
                                    engine.record_history(&expanded)?;
                                    Ok((expanded, print_only))
                                })
                                .map_err(|error| error.to_string());
                            let _ = reply.send(prepared);
                        }
                        WorkerRequest::RunBinding { command, line, cursor, reply } => {
                            let result = engine.run_readline_binding(&command, &line, cursor)
                                .map_err(|error| error.to_string());
                            *worker_bindings.lock().unwrap_or_else(|error| error.into_inner()) = engine.readline_bindings();
                            let _ = reply.send(result);
                        }
                    }
                }
                Ok(())
            })();
            if let Err(error) = result { let _ = io::write(format!("\nError de terminal: {error}\n").as_bytes()); }
            worker_exited.store(true, Ordering::SeqCst);
        });
        let history = fs::read_to_string(crate::adapters::persistence::AppPaths::detect().history_file())
            .unwrap_or_default().lines().filter(|line| !line.starts_with('#')).map(str::to_owned).collect::<Vec<_>>();
        let history_index = history.len();
        Ok(Self { commands, keys, display, output, size, raw, busy, interrupt, force_abort, exited,
            line: Vec::new(), cursor: 0, selection_anchor: None, history, history_index, pending: String::new(), bindings,
            secondary_prompt, timeout, last_activity, config_reload_requested })
    }
    pub fn resize(&self, cols: u16, rows: u16) {
        *self.size.lock().unwrap_or_else(|e| e.into_inner()) = (cols, rows);
        if self.raw.load(Ordering::SeqCst) { let _ = self.keys.send(Event::Resize(cols, rows)); }
    }
    pub fn exited(&self) -> bool { self.exited.load(Ordering::SeqCst) }
    pub fn take_config_reload_request(&self) -> bool {
        self.config_reload_requested.swap(false, Ordering::SeqCst)
    }
    pub fn raw_mode(&self) -> bool { self.raw.load(Ordering::SeqCst) }
    pub fn send_raw_key(&self, key: KeyEvent) -> Result<()> {
        self.keys.send(Event::Key(key))?;
        Ok(())
    }

    pub fn send_key_event(&mut self, key: KeyEvent) -> Result<()> {
        if self.raw_mode() {
            self.keys.send(Event::Key(key))?;
            return Ok(());
        }
        self.handle_key_event(key)
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        if self.raw_mode() {
            self.keys.send(Event::Paste(text.replace("\r\n", "\n")))?;
            Ok(())
        } else {
            if self.delete_selection() {
                self.redraw();
            }
            self.write(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes())
        }
    }

    pub fn force_abort(&self) {
        self.force_abort.store(true, Ordering::SeqCst);
        self.interrupt.store(true, Ordering::SeqCst);
    }
    pub fn check_timeout(&mut self) {
        if self.busy.load(Ordering::SeqCst) || self.raw.load(Ordering::SeqCst) {
            return;
        }
        let timeout = *self.timeout.lock().unwrap_or_else(|error| error.into_inner());
        let Some(timeout) = timeout else { return; };
        let elapsed = self.last_activity.lock()
            .unwrap_or_else(|error| error.into_inner())
            .elapsed();
        if elapsed < timeout {
            return;
        }

        self.emit("\r\nbash: TMOUT: sesión terminada por inactividad\r\n");
        self.busy.store(true, Ordering::SeqCst);
        self.line.clear();
        self.clear_selection();
        self.pending.clear();
        let _ = self.commands.send(WorkerRequest::Execute("exit".to_owned()));
        *self.last_activity.lock().unwrap_or_else(|error| error.into_inner()) = Instant::now();
    }

    fn emit(&self, text: &str) { let _ = self.display.send(text.as_bytes().to_vec()); }

    fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    fn begin_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            self.clear_selection();
            return false;
        };
        self.line.drain(start..end);
        self.cursor = start;
        self.clear_selection();
        true
    }

    fn delete_previous_word(&mut self) {
        if self.delete_selection() || self.cursor == 0 {
            return;
        }

        let mut start = self.cursor;
        while start > 0 && self.line[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !self.line[start - 1].is_whitespace() {
            start -= 1;
        }

        self.line.drain(start..self.cursor);
        self.cursor = start;
    }

    fn move_word_left(&self, from: usize) -> usize {
        let mut pos = from;
        while pos > 0 && self.line[pos - 1].is_whitespace() {
            pos -= 1;
        }
        while pos > 0 && !self.line[pos - 1].is_whitespace() {
            pos -= 1;
        }
        pos
    }

    fn move_word_right(&self, from: usize) -> usize {
        let mut pos = from;
        while pos < self.line.len() && !self.line[pos].is_whitespace() {
            pos += 1;
        }
        while pos < self.line.len() && self.line[pos].is_whitespace() {
            pos += 1;
        }
        pos
    }

    fn redraw(&self) {
        let prompt = if self.pending.is_empty() {
            "$ ".to_owned()
        } else {
            self.secondary_prompt.lock().unwrap_or_else(|error| error.into_inner()).clone()
        };

        let mut rendered = String::new();
        if let Some((start, end)) = self.selection_range() {
            for (index, ch) in self.line.iter().enumerate() {
                if index == start {
                    rendered.push_str("\x1b[7m");
                }
                if index == end {
                    rendered.push_str("\x1b[27m");
                }
                rendered.push(*ch);
            }
            if end == self.line.len() {
                rendered.push_str("\x1b[27m");
            }
        } else {
            rendered.extend(self.line.iter());
        }

        self.emit(&format!("\r\x1b[2K{prompt}{rendered}\x1b[0m"));
        let back = self.line.len() - self.cursor;
        if back > 0 { self.emit(&format!("\x1b[{back}D")); }
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let text = String::from_utf8_lossy(bytes);
        let special = match text.as_ref() {
            "\x1b[A" | "\x1bOA" => Some(KeyCode::Up), "\x1b[B" | "\x1bOB" => Some(KeyCode::Down),
            "\x1b[C" => Some(KeyCode::Right), "\x1b[D" => Some(KeyCode::Left),
            "\x1b[H" => Some(KeyCode::Home), "\x1b[F" => Some(KeyCode::End),
            "\x1b[3~" => Some(KeyCode::Delete), "\x1b[2~" => Some(KeyCode::Insert),
            "\x1b[5~" => Some(KeyCode::PageUp), "\x1b[6~" => Some(KeyCode::PageDown), _ => None,
        };
        let events = if let Some(code) = special { vec![KeyEvent::new(code, KeyModifiers::NONE)] }
            else { text.chars().map(|ch| match ch {
                '\r' | '\n' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                '\x7f' | '\x08' => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                '\x1b' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                '\t' => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                '\x01'..='\x1a' => KeyEvent::new(KeyCode::Char((ch as u8 + b'a' - 1) as char), KeyModifiers::CONTROL),
                ch => KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            }).collect() };
        for event in events {
            self.handle_key_event(event)?;
        }
        Ok(())
    }

    fn handle_key_event(&mut self, event: KeyEvent) -> Result<()> {
            *self.last_activity.lock().unwrap_or_else(|error| error.into_inner()) = Instant::now();
            if self.raw.load(Ordering::SeqCst) {
                self.keys.send(Event::Key(event))?;
                return Ok(());
            }
            if event.modifiers.contains(KeyModifiers::CONTROL) && event.code == KeyCode::Char('c') {
                self.interrupt.store(true, Ordering::SeqCst);
                self.line.clear(); self.cursor = 0; self.clear_selection(); self.pending.clear();
                self.emit("^C\r\n");
                if !self.busy.load(Ordering::SeqCst) { self.redraw(); }
                return Ok(());
            }
            if self.busy.load(Ordering::SeqCst) { return Ok(()); }

            // Editing gestures expected from a desktop terminal. These are handled
            // before readline bindings so modifier information from the GUI is not lost.
            if event.modifiers.contains(KeyModifiers::CONTROL)
                && event.code == KeyCode::Backspace
            {
                self.delete_previous_word();
                self.redraw();
                return Ok(());
            }

            if event.modifiers.contains(KeyModifiers::CONTROL)
                && !event.modifiers.contains(KeyModifiers::SHIFT)
                && matches!(event.code, KeyCode::Left | KeyCode::Right)
            {
                self.clear_selection();
                self.cursor = match event.code {
                    KeyCode::Left => self.move_word_left(self.cursor),
                    KeyCode::Right => self.move_word_right(self.cursor),
                    _ => self.cursor,
                };
                self.redraw();
                return Ok(());
            }

            if event.modifiers.contains(KeyModifiers::SHIFT)
                && matches!(event.code, KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End)
            {
                self.begin_selection();
                let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
                self.cursor = match event.code {
                    KeyCode::Left if ctrl => self.move_word_left(self.cursor),
                    KeyCode::Right if ctrl => self.move_word_right(self.cursor),
                    KeyCode::Left => self.cursor.saturating_sub(1),
                    KeyCode::Right => (self.cursor + 1).min(self.line.len()),
                    KeyCode::Home => 0,
                    KeyCode::End => self.line.len(),
                    _ => self.cursor,
                };
                self.redraw();
                return Ok(());
            }

            if let Some(sequence) = readline_sequence(&event) {
                let action = self.bindings
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .get(&sequence)
                    .cloned();

                if let Some(action) = action {
                    if let Some(command) = action.strip_prefix("shell:") {
                        let (reply_tx, reply_rx) = mpsc::channel();
                        self.commands.send(WorkerRequest::RunBinding {
                            command: command.to_owned(),
                            line: self.line.iter().collect(),
                            cursor: self.cursor,
                            reply: reply_tx,
                        })?;
                        if let Ok(Ok((line, cursor, stdout, stderr))) =
                            reply_rx.recv_timeout(std::time::Duration::from_secs(2))
                        {
                            self.line = line.chars().collect();
                            self.cursor = cursor.min(self.line.len());
                            self.clear_selection();
                            if !stdout.is_empty() { self.emit(&stdout); }
                            if !stderr.is_empty() { self.emit(&stderr); }
                        }
                        self.redraw();
                        return Ok(());
                    }

                    let handled = match action.as_str() {
                        "beginning-of-line" => { self.cursor = 0; true }
                        "end-of-line" => { self.cursor = self.line.len(); true }
                        "backward-char" => { self.cursor = self.cursor.saturating_sub(1); true }
                        "forward-char" => { self.cursor = (self.cursor + 1).min(self.line.len()); true }
                        "previous-history" => {
                            if self.history_index > 0 {
                                self.history_index -= 1;
                                self.line = self.history[self.history_index].chars().collect();
                                self.cursor = self.line.len();
                            }
                            true
                        }
                        "next-history" => {
                            self.history_index = (self.history_index + 1).min(self.history.len());
                            self.line = self.history.get(self.history_index)
                                .map(|line| line.chars().collect())
                                .unwrap_or_default();
                            self.cursor = self.line.len();
                            true
                        }
                        "clear-screen" => { self.emit("\x1b[2J\x1b[H"); true }
                        "unix-line-discard" => {
                            self.line.drain(..self.cursor);
                            self.cursor = 0;
                            true
                        }
                        "backward-delete-char" => {
                            if self.cursor > 0 {
                                self.cursor -= 1;
                                self.line.remove(self.cursor);
                            }
                            true
                        }
                        "delete-char" | "delete-char-or-list" => {
                            if self.cursor < self.line.len() {
                                self.line.remove(self.cursor);
                            }
                            true
                        }
                        "complete" => { self.complete(); true }
                        _ => {
                            // Readline macro: unknown function names are treated as
                            // literal macro text, matching bind's macro form.
                            for ch in action.chars() {
                                self.line.insert(self.cursor, ch);
                                self.cursor += 1;
                            }
                            true
                        }
                    };
                    if handled {
                        self.clear_selection();
                        self.redraw();
                        return Ok(());
                    }
                }
            }

            match event.code {
                KeyCode::Enter => {
                    self.emit("\r\n");
                    self.pending.push_str(&self.line.iter().collect::<String>());
                    self.line.clear(); self.cursor = 0; self.clear_selection();
                    if crate::presentation::shell::session::needs_continuation(&self.pending) {
                        self.pending.push('\n'); self.redraw(); return Ok(());
                    }
                    let command = std::mem::take(&mut self.pending);
                    if command.trim().is_empty() { self.redraw(); return Ok(()); }

                    let original_command = command.clone();
                    let (reply_tx, reply_rx) = mpsc::channel();
                    self.commands.send(WorkerRequest::PrepareHistory {
                        line: command.clone(),
                        reply: reply_tx,
                    })?;
                    let prepared = reply_rx.recv_timeout(std::time::Duration::from_secs(2));
                    let (command, print_only) = match prepared {
                        Ok(Ok(value)) => value,
                        Ok(Err(error)) => {
                            self.emit(&format!("bash: {error}\r\n"));
                            self.redraw();
                            return Ok(());
                        }
                        Err(_) => (command, false),
                    };

                    if command != original_command {
                        self.emit(&format!("{command}\r\n"));
                    }
                    self.history.push(command.clone());
                    self.history_index = self.history.len();

                    if print_only {
                        self.emit(&format!("{command}\r\n"));
                        self.redraw();
                        return Ok(());
                    }

                    self.busy.store(true, Ordering::SeqCst);
                    self.commands.send(WorkerRequest::Execute(command))?;
                    return Ok(());
                }
                KeyCode::Char('d') if event.modifiers.contains(KeyModifiers::CONTROL) && self.line.is_empty() => {
                    self.busy.store(true, Ordering::SeqCst);
                    self.commands.send(WorkerRequest::Execute(
                        r#"__SST_EOF_CHECK=:; if [[ -o ignoreeof ]]; then (( __SST_IGNOREEOF += 1 )); if [[ $__SST_IGNOREEOF -ge ${IGNOREEOF:-10} ]]; then exit; else echo 'Use "exit" to leave the shell.'; fi; else exit; fi"#.to_owned()
                    ))?;
                }
                KeyCode::Char('l') if event.modifiers.contains(KeyModifiers::CONTROL) => self.emit("\x1b[2J\x1b[H"),
                KeyCode::Char('u') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    if !self.delete_selection() {
                        self.line.drain(..self.cursor);
                        self.cursor = 0;
                    }
                }
                KeyCode::Char('a') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.clear_selection();
                    self.cursor = 0;
                }
                KeyCode::Char('e') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.clear_selection();
                    self.cursor = self.line.len();
                }
                KeyCode::Char(ch) if !event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.delete_selection();
                    self.line.insert(self.cursor, ch);
                    self.cursor += 1;
                }
                KeyCode::Backspace => {
                    if !self.delete_selection() && self.cursor > 0 {
                        self.cursor -= 1;
                        self.line.remove(self.cursor);
                    }
                }
                KeyCode::Delete => {
                    if !self.delete_selection() && self.cursor < self.line.len() {
                        self.line.remove(self.cursor);
                    }
                }
                KeyCode::Left => {
                    if let Some((start, _)) = self.selection_range() {
                        self.cursor = start;
                        self.clear_selection();
                    } else {
                        self.cursor = self.cursor.saturating_sub(1);
                    }
                }
                KeyCode::Right => {
                    if let Some((_, end)) = self.selection_range() {
                        self.cursor = end;
                        self.clear_selection();
                    } else {
                        self.cursor = (self.cursor + 1).min(self.line.len());
                    }
                }
                KeyCode::Home => { self.clear_selection(); self.cursor = 0; }
                KeyCode::End => { self.clear_selection(); self.cursor = self.line.len(); }
                KeyCode::Up if self.history_index > 0 => {
                    self.clear_selection();
                    self.history_index -= 1;
                    self.line = self.history[self.history_index].chars().collect();
                    self.cursor = self.line.len();
                }
                KeyCode::Down => {
                    self.clear_selection();
                    self.history_index = (self.history_index + 1).min(self.history.len());
                    self.line = self.history.get(self.history_index).map(|s| s.chars().collect()).unwrap_or_default();
                    self.cursor = self.line.len();
                }
                KeyCode::Tab => {
                    self.clear_selection();
                    self.complete();
                },
                _ => {},
            }
            self.redraw();
            Ok(())
    }

    fn complete(&mut self) {
        let line = self.line.iter().collect::<String>();
        let (reply_tx, reply_rx) = mpsc::channel();
        if self.commands.send(WorkerRequest::Complete {
            line: line.clone(),
            cursor: self.cursor,
            reply: reply_tx,
        }).is_err() {
            return;
        }

        let Ok(mut matches) = reply_rx.recv_timeout(std::time::Duration::from_secs(2)) else {
            return;
        };
        if matches.is_empty() {
            return;
        }

        matches.sort();
        matches.dedup();

        let prefix: String = self.line[..self.cursor].iter().collect();
        let start = prefix.rfind(|ch: char| {
            ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | ')')
        }).map_or(0, |index| index + 1);

        if matches.len() == 1 {
            let begin = prefix[..start].chars().count();
            let chars = matches[0].chars().collect::<Vec<_>>();
            self.line.splice(begin..self.cursor, chars.iter().copied());
            self.cursor = begin + chars.len();
            self.clear_selection();
        } else {
            self.emit(&format!("\r\n{}\r\n", matches.join("  ")));
        }
    }
}
