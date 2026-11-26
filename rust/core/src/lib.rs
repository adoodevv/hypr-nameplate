//! Shared code for hypr-nameplate's Rust programs.
//!
//! The `nameplate` binary (main.rs) is the command line: bar items, media keys
//! and background watchers. `nameplate-shell` (../shell) builds the GTK desktop
//! pieces on the same Hyprland IPC, JSON and theme code.
pub mod bar;
pub mod battery;
pub mod clock;
pub mod ffi;
pub mod json;
pub mod keys;
pub mod sys;
pub mod watch;
pub mod window;
