//! `nameplate-agents`: AI coding agents in the bar, like Omarchy's: Claude
//! Code, Grok, Muse and opencode.
//!
//!   nameplate-agents          the bar item (Waybar JSON): a robot while one is installed
//!   nameplate-agents stats    today's and this week's use and recent sessions, as JSON
//!
//! Everything comes from the history the apps keep on this computer
//! (~/.claude/projects, ~/.grok/sessions, ~/.local/share/muse,
//! ~/.local/share/opencode). Nothing is sent anywhere and no sign-in details
//! are read. Results are cached in ~/.cache/nameplate/agents-rs.json and only
//! changed files are read again. This replaces bin/agents (Python), whose
//! output it matches; bin/agents hands over to it when it's installed.

mod collect;
mod model;
mod readers;
mod time;

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let line = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => {
            if collect::agents().iter().any(collect::installed) {
                let open = nameplate::sys::open_panel().0 == "agents";
                let class = if open { r#"["open"]"# } else { "[]" };
                format!("{{\"text\": \"\u{f16a3}\", \"tooltip\": \"\", \"class\": {class}}}")
            } else {
                String::new() // none installed: the item hides
            }
        }
        ["stats"] => collect::stats().to_string(),
        _ => {
            eprintln!("usage: nameplate-agents [stats]");
            return ExitCode::from(2);
        }
    };
    let _ = writeln!(std::io::stdout(), "{line}");
    ExitCode::SUCCESS
}
