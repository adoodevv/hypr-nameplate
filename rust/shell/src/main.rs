//! `nameplate-shell`: hypr-nameplate's GTK desktop pieces.
//!
//!   nameplate-shell wallpaper              the desktop background (started at login)
//!   nameplate-shell wallpaper set IMAGE    switch to an image
//!   nameplate-shell wallpaper next|list    next image in the collection / print it
//!   nameplate-shell wallpaper lock         refresh the lock screen's copy
//!   nameplate-shell dock                   the app dock (started at login)
//!   nameplate-shell dock pinned|pin APP|unpin APP|autohide [on|off]
//!   nameplate-shell panel KIND [ROUTE] [--bar]   open a panel (see panel/mod.rs)
//!   nameplate-shell panel daemon|reload|close

mod dock;
mod panel;
mod util;
mod wallpaper;

use std::process::ExitCode;

/// Keep the reason when something goes wrong: the dock, the wallpaper and the
/// panels run in the background, where nobody sees their error output.
fn log_panics() {
    std::panic::set_hook(Box::new(|info| {
        let what = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
        let line = format!("nameplate-shell {what}: {info}\n");
        eprint!("{line}");
        let path = nameplate::sys::home().join(".cache/nameplate/nameplate-shell.log");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }));
}

fn main() -> ExitCode {
    log_panics();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // With more arguments it's a command, done without a display.
    if args.len() > 1 {
        let result = match args[0].as_str() {
            "wallpaper" => wallpaper::cli(&args[1..]),
            "dock" => dock::cli(&args[1..]),
            "panel" => panel::cli(&args[1..]),
            _ => Err("usage: nameplate-shell wallpaper|dock|panel [...]".into()),
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
            eprintln!("usage: nameplate-shell wallpaper|dock|panel [...]");
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
