//! `nameplate`: the command-line half of hypr-nameplate's Rust programs.

use nameplate::{bar, battery, keys, watch, window};
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "usage:
  nameplate bar ITEM [--watch]      a bar item as Waybar JSON, once or kept current
  nameplate refresh ITEM...         update those bar items now (after toggling something)
  nameplate volume raise|lower|mute|+N|-N
  nameplate mic mute
  nameplate brightness raise|lower|max|min|+N|-N
  nameplate osd volume|mic|brightness
  nameplate battery-watch           low-battery warnings (runs until logout)
  nameplate window minimize|pop|close-all|width save|width restore
  nameplate layout [back | current [WORKSPACE] | set MODE]
  nameplate keyboard cycle
  nameplate browser [--private]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();

    let result = match words.as_slice() {
        ["bar", name, rest @ ..] => {
            if bar::item(name).is_none() {
                Err(format!("unknown bar item {name}; one of: {}", bar::NAMES))
            } else if rest == ["--watch"] {
                watch::run(name)
            } else if rest.is_empty() {
                let line = bar::item(name).map(|it| it.to_json()).unwrap_or_default();
                let _ = writeln!(std::io::stdout(), "{line}"); // a closed pipe isn't worth a panic
                Ok(())
            } else {
                Err(USAGE.to_string())
            }
        }
        ["refresh", names @ ..] if !names.is_empty() => {
            watch::refresh(names);
            Ok(())
        }
        ["volume", action] => keys::volume(action),
        ["mic", action] => keys::mic(action),
        ["brightness", action] => keys::brightness(action),
        ["osd", what] => {
            keys::osd(what);
            Ok(())
        }
        ["battery-watch"] => battery::run(),
        ["window", "minimize"] => {
            window::minimize();
            Ok(())
        }
        ["window", "pop"] => {
            window::pop();
            Ok(())
        }
        ["window", "close-all"] => {
            window::close_all();
            Ok(())
        }
        ["window", "width", action] => window::width(action),
        ["layout", rest @ ..] => window::layout(rest),
        ["keyboard", "cycle"] => {
            window::keyboard_cycle();
            Ok(())
        }
        ["browser"] => window::browser(false),
        ["browser", "--private"] => window::browser(true),
        _ => Err(USAGE.to_string()),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("nameplate: {message}");
            ExitCode::FAILURE
        }
    }
}
