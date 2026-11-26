//! `nameplate battery-watch`: low-battery warnings (started at login).
//!
//! 15%: a normal notification. 5%: an urgent one that stays until dismissed.
//! Plugging in resets both. Thresholds and the check interval can be set with
//! NAMEPLATE_BATTERY_LOW, NAMEPLATE_BATTERY_CRITICAL and NAMEPLATE_BATTERY_INTERVAL.

use crate::{bar, ffi, sys};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Warned {
    None,
    Low,
    Critical,
}

/// What to say for a reading, given what was already said.
pub fn next_warning(
    level: u32,
    status: &str,
    warned: Warned,
    low: u32,
    critical: u32,
) -> (Warned, Option<Warned>) {
    if status != "Discharging" {
        return (Warned::None, None);
    }
    if level <= critical && warned != Warned::Critical {
        return (Warned::Critical, Some(Warned::Critical));
    }
    if level <= low && warned == Warned::None {
        return (Warned::Low, Some(Warned::Low));
    }
    (warned, None)
}

fn env_number(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub fn run() -> Result<(), String> {
    let lock_path = sys::runtime_dir().join("nameplate-battery-watch.lock");
    let lock = std::fs::File::create(&lock_path).map_err(|e| e.to_string())?;
    if !ffi::try_lock(&lock) {
        return Ok(()); // already running
    }
    let Some(bat) = bar::battery_path() else {
        return Ok(());
    }; // a desktop
    let low = env_number("NAMEPLATE_BATTERY_LOW", 15);
    let critical = env_number("NAMEPLATE_BATTERY_CRITICAL", 5);
    let interval = env_number("NAMEPLATE_BATTERY_INTERVAL", 60).max(5);

    let mut warned = Warned::None;
    loop {
        let level = sys::read(bat.join("capacity"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);
        let status = sys::read(bat.join("status")).unwrap_or_default();
        let (now, say) = next_warning(level, &status, warned, low, critical);
        warned = now;
        match say {
            Some(Warned::Critical) => {
                sys::output(
                    "notify-send",
                    &[
                        "-a",
                        "Battery",
                        "-u",
                        "critical",
                        &format!("Battery critically low: {level}%"),
                        "Plug in now. The laptop will shut down soon.",
                    ],
                );
            }
            Some(Warned::Low) => {
                sys::output(
                    "notify-send",
                    &[
                        "-a",
                        "Battery",
                        &format!("Battery low: {level}%"),
                        "Plug in your charger soon.",
                    ],
                );
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_secs(interval as u64));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_once_per_level() {
        let (w, say) = next_warning(14, "Discharging", Warned::None, 15, 5);
        assert_eq!((w, say), (Warned::Low, Some(Warned::Low)));
        assert_eq!(
            next_warning(12, "Discharging", w, 15, 5),
            (Warned::Low, None)
        );
        let (w, say) = next_warning(5, "Discharging", w, 15, 5);
        assert_eq!((w, say), (Warned::Critical, Some(Warned::Critical)));
        assert_eq!(
            next_warning(4, "Discharging", w, 15, 5),
            (Warned::Critical, None)
        );
        assert_eq!(next_warning(4, "Charging", w, 15, 5), (Warned::None, None));
    }

    #[test]
    fn straight_to_critical() {
        assert_eq!(
            next_warning(3, "Discharging", Warned::None, 15, 5),
            (Warned::Critical, Some(Warned::Critical))
        );
    }
}
