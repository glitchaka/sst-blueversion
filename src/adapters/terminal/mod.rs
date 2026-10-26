pub mod guard;
mod crossterm_terminal;

pub use crossterm_terminal::CrosstermTerminalFactory;
#[cfg(windows)]
#[path = "embedded.rs"]
mod embedded_inner;
#[cfg(windows)]
#[path = "embedded_supervised.rs"]
pub mod embedded;
pub mod io;
