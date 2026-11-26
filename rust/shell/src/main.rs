//! `nameplate-shell`: hypr-nameplate's GTK desktop pieces.
//!
//!   nameplate-shell wallpaper              the desktop background (started at login)
//!   nameplate-shell wallpaper set IMAGE    switch to an image
//!   nameplate-shell wallpaper next|list    next image in the collection / print it
//!   nameplate-shell wallpaper lock         refresh the lock screen's copy
//!   nameplate-shell dock                   the app dock (started at login)
//!   nameplate-shell dock pinned|pin APP|unpin APP|autohide [on|off]

mod dock;
mod util;
mod wallpaper;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // With more arguments it's a command, done without a display.
    if args.len() > 1 {
        let result = match args[0].as_str() {
            "wallpaper" => wallpaper::cli(&args[1..]),
            "dock" => dock::cli(&args[1..]),
            _ => Err("usage: nameplate-shell wallpaper|dock [...]".into()),
        };
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let run: fn() -> Result<(), String> = match args.first().map(String::as_str) {
        Some("wallpaper") => wallpaper::run,
        Some("dock") => dock::run,
        _ => {
            eprintln!("usage: nameplate-shell wallpaper|dock [...]");
            return ExitCode::from(2);
        }
    };
    if gtk::init().is_err() {
        eprintln!("nameplate-shell: no display");
        return ExitCode::FAILURE;
    }
    if !gtk_layer_shell::is_supported() {
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
