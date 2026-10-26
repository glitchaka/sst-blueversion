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
const EXIT_SETTLE_TIME: Duration = Duration::from_millis(100);

/// Supervises the in-process terminal worker so an internal startup/runtime
/// failure cannot make the whole SST window disappear before the diagnostic is
/// visible. Normal shell exits still propagate to the GUI after a very short
/// settle period.
pub struct EmbeddedSession {
    inner: embedded_inner::EmbeddedSession,
    pub output: mpsc::Receiver<Vec<u8>>,
    worker_failed: Arc<AtomicBool>,
    exit_seen_at: Mutex<Option<Instant>>,
}

impl EmbeddedSession {
    pub fn start(cols: u16, rows: u16) -> Result<Self> {
        let mut inner = embedded_inner::EmbeddedSession::start(cols, rows)?;

        // Move the worker output receiver into a small relay. The relay observes
        // the explicit terminal-error record emitted by embedded.rs before that
        // worker sets its exited flag. The GUI continues consuming the same byte
        // stream through this wrapper.
        let (_dummy_tx, dummy_rx) = mpsc::channel();
        let source = std::mem::replace(&mut inner.output, dummy_rx);
        let (relay_tx, output) = mpsc::channel();
        let worker_failed = Arc::new(AtomicBool::new(false));
        let relay_failed = Arc::clone(&worker_failed);

        thread::spawn(move || {
            for data in source {
                if contains_bytes(&data, TERMINAL_ERROR_MARKER) {
                    relay_failed.store(true, Ordering::SeqCst);
                }
                if relay_tx.send(data).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            inner,
            output,
            worker_failed,
            exit_seen_at: Mutex::new(None),
        })
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
            Some(instant) => instant.elapsed() >= EXIT_SETTLE_TIME,
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
