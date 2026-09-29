mod admin;
mod command;
mod config;
mod editor;
mod registry;
mod security;
mod system;
mod sudo;
mod unix;

pub use admin::{
    DeviceBuiltin, DiagnosticsBuiltin, DomainBuiltin, NetworkBuiltin, SwitchBuiltin,
    WakeOnLanBuiltin,
};
pub use command::BuiltinCommand;
pub use config::{ConfigBuiltin, PathBuiltin};
pub use editor::EditorBuiltin;
pub use registry::CommandRegistry;
pub use security::{IntelBuiltin, TriageBuiltin};
pub use system::SystemBuiltin;
pub use sudo::SudoBuiltin;
pub use unix::{UNIX_COMMANDS, UnixBuiltin};
