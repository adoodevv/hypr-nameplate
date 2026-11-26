//! `nameplate-shell`: hypr-nameplate's GTK desktop pieces.
//!
//!   nameplate-shell wallpaper    the desktop background (started at login)
//!   nameplate-shell dock         the app dock (started at login)
//!
//! Their command-line halves stay in bin/wallpaper and bin/dock, which talk to
//! these through the same pidfiles and signals as before.

mod dock;
mod util;
mod wallpaper;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run: fn() -> Result<(), String> = match args.first().map(String::as_str) {
        Some("wallpaper") => wallpaper::run,
        Some("dock") => dock::run,
        _ => {
            eprintln!("usage: nameplate-shell wallpaper|dock");
            return ExitCode::from(2);
        }
    };
    if gtk::init().is_err() {
        eprintln!("nameplate-shell: no display");
        return ExitCode::FAILURE;
    }
    if !gtk_layer_shell::is_supported() {
        // Not a layer-shell desktop: let the Python version and its fallbacks
        // (swaybg for the wallpaper) handle it.
        eprintln!("nameplate-shell: this desktop doesn't support layer-shell");
        return ExitCode::from(3);
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nameplate-shell: {e}");
            ExitCode::FAILURE
        }
    }
}
