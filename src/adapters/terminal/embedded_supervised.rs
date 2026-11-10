use std::{
    ops::{Deref, DerefMut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;

use super::embedded_inner;

const TERMINAL_ERROR_MARKER: &[u8] = b"Error de terminal:";
const LEGACY_BANNER_MARKER: &[u8] = b"Escribe help para ver los comandos.";
const SECURITY_PRELOAD_MARKER: &[u8] = b"procesos              revisando";
const STARTUP_GREETING_MARKER: &str = "«Hola. ¿Te gustaría destruir algún mal hoy?»";
const STARTUP_FOOTER: &str =
    "\r\n\x1b[38;5;250mConsejo: usa \x1b[1;38;5;117mguide\x1b[0;38;5;250m para conocer todo lo que puedes hacer con SST.\x1b[0m\r\n\r\n\x1b[1;38;5;117m«Hola. ¿Te gustaría destruir algún mal hoy?»\x1b[0m\r\n";
const EXIT_SETTLE_TIME: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, PartialEq, Eq)]
enum StartupRelayState {
    WaitingForFetch,
    FetchVisible,
    SecurityHidden,
    Complete,
}

/// Supervises the in-process terminal worker so an internal startup/runtime
/// failure cannot make the whole SST window disappear before the diagnostic is
/// visible. Normal shell exits still propagate to the GUI after a very short
/// settle period.
pub struct EmbeddedSession {
    inner: embedded_inner::EmbeddedSession,
    pub output: mpsc::Receiver<Vec<u8>>,
    worker_failed: Arc<AtomicBool>,
    exit_seen_at: Mutex<Option<Instant>>,
    startup_ready: Arc<AtomicBool>,
    available_update: mpsc::Receiver<String>,
    pending_update: Option<String>,
}

impl EmbeddedSession {
    pub fn start(cols: u16, rows: u16) -> Result<Self> {
        let mut inner = embedded_inner::EmbeddedSession::start(cols, rows)?;
        // Move the worker output receiver into a relay. Besides supervising fatal
        // worker errors, this keeps the visual startup compact: neofetch remains
        // visible while the security preload runs silently in the background.
        let (_dummy_tx, dummy_rx) = mpsc::channel();
        let source = std::mem::replace(&mut inner.output, dummy_rx);
        let (relay_tx, output) = mpsc::channel();
        let worker_failed = Arc::new(AtomicBool::new(false));
        let relay_failed = Arc::clone(&worker_failed);
        let startup_ready = Arc::new(AtomicBool::new(false));
        let relay_ready = Arc::clone(&startup_ready);
        let (notice_tx, available_update) = mpsc::channel();
        // Never block opening the GUI or the Nwash prompt on the network.
        // A manifest check is allowed, but downloading/applying a release isn't.
        thread::spawn(move || {
            let paths = crate::adapters::persistence::AppPaths::detect();
            let version = crate::application::update::current_version();
            let notice = match crate::application::update::available_update_notice(&paths) {
                Ok(Some(message)) => message,
                Ok(None) if crate::application::update::configured_manifest_url(&paths).is_some() => {
                    format!("SST {version}: no hay una versión más reciente publicada en el canal configurado.")
                }
                Ok(None) => format!(
                    "SST {version}: no hay canal de actualización configurado. Usa 'instp sst' para revisarlo."
                ),
                Err(error) => format!(
                    "SST {version}: no se pudo consultar el canal de actualizaciones ({error}). \
                     Puedes reintentar con 'instp check sst'."
                ),
            };
            let _ = notice_tx.send(notice);
        });

        thread::spawn(move || {
            let mut startup = StartupRelayState::WaitingForFetch;

            for data in source {
                if contains_bytes(&data, TERMINAL_ERROR_MARKER) {
                    relay_failed.store(true, Ordering::SeqCst);
                    if relay_tx.send(data).is_err() {
                        break;
                    }
                    continue;
                }

                match startup {
                    StartupRelayState::WaitingForFetch => {
                        // Ignore any startup-file chatter and the legacy banner.
                        // The first visible content in the GUI must be neofetch.
                        if contains_bytes(&data, LEGACY_BANNER_MARKER) {
                            startup = StartupRelayState::FetchVisible;
                        }
                    }
                    StartupRelayState::FetchVisible => {
                        // The first preload progress record marks the end of the
                        // neofetch block. From here until render_startup(), preload
                        // diagnostics stay internal instead of scrolling the GUI.
                        if contains_bytes(&data, SECURITY_PRELOAD_MARKER) {
                            startup = StartupRelayState::SecurityHidden;
                            continue;
                        }

                        if relay_tx.send(data).is_err() {
                            break;
                        }
                    }
                    StartupRelayState::SecurityHidden => {
                        if contains_bytes(&data, STARTUP_GREETING_MARKER.as_bytes()) {
                            if relay_tx.send(STARTUP_FOOTER.as_bytes().to_vec()).is_err() {
                                break;
                            }

                            // Only the read-only availability notice runs at startup;
                            // downloading and applying still require "instp upgrade sst".
                            startup = StartupRelayState::Complete;
                            relay_ready.store(true, Ordering::SeqCst);
                        }
                        // All preload progress and the verbose startup report are
                        // intentionally swallowed. `triage` remains fully verbose
                        // after startup because this filter disables itself here.
                    }
                    StartupRelayState::Complete => {
                        if relay_tx.send(data).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            inner,
            output,
            worker_failed,
            exit_seen_at: Mutex::new(None),
            startup_ready,
            available_update,
            pending_update: None,
        })
    }

    /// Called by the GUI event loop. Display only when the startup banner is
    /// finished and the shell can redraw the editable input line safely.
    pub fn poll_update_notification(&mut self) {
        if !self.startup_ready.load(Ordering::SeqCst) {
            return;
        }
        if self.pending_update.is_none() {
            self.pending_update = self.available_update.try_recv().ok();
        }
        if let Some(message) = self.pending_update.as_deref() {
            if self.inner.show_notification(message) {
                self.pending_update = None;
            }
        }
    }

    pub fn exited(&self) -> bool {
        if !self.inner.exited() {
            *self
                .exit_seen_at
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = None;
            return false;
        }

        // A worker error must remain on screen instead of being immediately
        // converted into a window close by gui_slint.rs.
        if self.worker_failed.load(Ordering::SeqCst) {
            return false;
        }

        // The worker emits its final diagnostic immediately before marking itself
        // exited. Give the relay one UI tick to observe that record before a
        // normal exit is allowed to close the GUI.
        let mut seen = self
            .exit_seen_at
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match *seen {
            Some(instant) if instant.elapsed() >= EXIT_SETTLE_TIME => true,
            Some(_) => false,
            None => {
                *seen = Some(Instant::now());
                false
            }
        }
    }

}

impl Deref for EmbeddedSession {
    type Target = embedded_inner::EmbeddedSession;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for EmbeddedSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
