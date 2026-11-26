//! Keeping bar items current: `nameplate bar ITEM --watch` prints the item, then
//! prints it again whenever it changes, until the bar goes away.
//!
//! Each item names the events that can change it. Every source runs on its own
//! thread and wakes the printing loop through a channel:
//!
//! | item                         | woken by                                         |
//! |------------------------------|--------------------------------------------------|
//! | clock                        | the start of each minute                         |
//! | network                      | `ip monitor` (links and addresses), 5 s for signal strength |
//! | sound, mic                   | `pactl subscribe` (PipeWire's change events)      |
//! | media                        | `playerctl --follow`                             |
//! | keyboard, monitor            | Hyprland's event socket                          |
//! | awake, nightlight, recording, dnd | `nameplate refresh ITEM` from the script that toggles it |
//! | battery, bluetooth           | a short tick (sysfs reads cost microseconds)     |
//!
//! Every item also follows the panels' open-panel file (the accent line under an
//! icon) and refreshes on SIGUSR1, which `nameplate refresh` sends.

use crate::{bar, ffi, sys};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

/// Seconds between safety re-reads, for events that are missed or have no source.
fn tick_seconds(name: &str) -> u64 {
    match name {
        "network" | "bluetooth" => 5,
        "battery" => 15,
        "awake" | "nightlight" | "recording" | "dnd" => 30,
        "sound" | "mic" | "media" => 60,
        _ => 300,
    }
}

pub fn run(name: &str) -> ! {
    // Before any thread starts, so they all inherit the mask and the signal
    // reaches the one thread waiting for it.
    ffi::block_signals(&[ffi::SIGUSR1]);
    let (tx, rx) = mpsc::channel::<()>();

    spawn_exit_watchdog();
    spawn_signal_listener(tx.clone());
    spawn_open_panel_follower(tx.clone());
    spawn_ticker(tick_seconds(name), tx.clone());
    match name {
        "clock" => spawn_minute_ticker(tx.clone()),
        "network" => {
            spawn_line_follower("ip", &["monitor", "link", "address"], tx.clone(), |_| true)
        }
        "sound" => spawn_line_follower("pactl", &["subscribe"], tx.clone(), |l| {
            l.contains(" on sink #") || l.contains(" on server") || l.contains(" on card")
        }),
        "mic" => spawn_line_follower("pactl", &["subscribe"], tx.clone(), |l| {
            l.contains(" on source #") || l.contains(" on server") || l.contains(" on card")
        }),
        "media" => spawn_line_follower(
            "playerctl",
            &[
                "--follow",
                "metadata",
                "--format",
                "{{status}}{{title}}{{playerName}}",
            ],
            tx.clone(),
            |_| true,
        ),
        "keyboard" => spawn_hypr_follower(&["activelayout"], tx.clone()),
        "monitor" => spawn_hypr_follower(&["monitoradded", "monitorremoved"], tx.clone()),
        _ => {}
    }
    drop(tx);

    let mut last: Option<String> = None;
    let stdout = std::io::stdout();
    loop {
        if let Some(item) = bar::item(name) {
            let line = item.to_json();
            if last.as_deref() != Some(line.as_str()) {
                let mut out = stdout.lock();
                if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
                    std::process::exit(0); // the bar is gone
                }
                last = Some(line);
            }
        }
        if rx.recv().is_err() {
            std::process::exit(0);
        }
        // Let a burst of events settle into one update.
        thread::sleep(Duration::from_millis(40));
        while rx.try_recv().is_ok() {}
    }
}

/// Leave with the bar. Restarting Waybar (a theme switch does) must not strand
/// watchers, including ones that would never print again to notice.
fn spawn_exit_watchdog() {
    thread::spawn(|| loop {
        thread::sleep(Duration::from_secs(2));
        if ffi::stdout_closed() || ffi::orphaned() {
            std::process::exit(0);
        }
    });
}

fn spawn_signal_listener(tx: Sender<()>) {
    thread::spawn(move || loop {
        ffi::wait_signal(&[ffi::SIGUSR1]);
        if tx.send(()).is_err() {
            return;
        }
    });
}

fn spawn_ticker(seconds: u64, tx: Sender<()>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(seconds));
        if tx.send(()).is_err() {
            return;
        }
    });
}

fn spawn_minute_ticker(tx: Sender<()>) {
    thread::spawn(move || loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let to_next_minute = 60_000 - (now.as_millis() % 60_000) as u64;
        thread::sleep(Duration::from_millis(to_next_minute + 50));
        if tx.send(()).is_err() {
            return;
        }
    });
}

/// The panels rewrite this small file when one opens or closes.
fn spawn_open_panel_follower(tx: Sender<()>) {
    thread::spawn(move || {
        let mut last = sys::open_panel();
        loop {
            thread::sleep(Duration::from_millis(150));
            let now = sys::open_panel();
            if now != last {
                last = now;
                if tx.send(()).is_err() {
                    return;
                }
            }
        }
    });
}

/// Follow a program that prints a line per event, restarting it if it stops.
fn spawn_line_follower(
    program: &'static str,
    args: &'static [&'static str],
    tx: Sender<()>,
    wanted: fn(&str) -> bool,
) {
    thread::spawn(move || loop {
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = child {
            if let Some(out) = child.stdout.take() {
                for line in BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if wanted(&line) && tx.send(()).is_err() {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        thread::sleep(Duration::from_secs(5));
    });
}

fn spawn_hypr_follower(wanted: &'static [&'static str], tx: Sender<()>) {
    thread::spawn(move || loop {
        if let Some(events) = sys::hypr_events() {
            for line in events.lines() {
                let Ok(line) = line else { break };
                let event = line.split(">>").next().unwrap_or("");
                if wanted.iter().any(|w| event.starts_with(w)) && tx.send(()).is_err() {
                    return;
                }
            }
        }
        thread::sleep(Duration::from_secs(5));
    });
}

/// `nameplate refresh ITEM...`: wake the watchers for these items, and send the
/// bar the item's old signal too (for a config that still runs items once).
pub fn refresh(names: &[&str]) {
    let rtmin = ffi::sigrtmin();
    let procs = sys::processes();
    for name in names {
        let watcher = format!("nameplate bar {name} --watch");
        for p in procs.iter().filter(|p| p.cmdline.ends_with(&watcher)) {
            ffi::send_signal(p.pid, ffi::SIGUSR1);
        }
        if let Some(n) = bar::waybar_signal(name) {
            for p in procs.iter().filter(|p| p.name == "waybar") {
                ffi::send_signal(p.pid, rtmin + n);
            }
        }
    }
}
