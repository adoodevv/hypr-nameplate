//! Volume and brightness keys, stepped the way Omarchy steps them, plus the small
//! on-screen level indicator (OSD) each key press shows.
//!
//!   nameplate volume raise|lower|mute|+N|-N
//!   nameplate mic mute
//!   nameplate brightness raise|lower|max|min|+N|-N
//!   nameplate osd volume|mic|brightness
//!
//! Omarchy's rules, from its omarchy-audio-output-volume and
//! omarchy-brightness-display scripts:
//! - Volume moves 5% a press (ALT: 1%), between 0 and 100%, and a volume key
//!   unmutes the speakers.
//! - Brightness moves 5% a press, but 1% at a time at 5% and below, so the dim
//!   end is usable. It never goes under 1% (a black screen) and lands on whole
//!   percentages, so the indicator steps evenly.

use crate::bar;
use crate::sys;

const SINK: &str = "@DEFAULT_AUDIO_SINK@";
const SOURCE: &str = "@DEFAULT_AUDIO_SOURCE@";

/// Where the volume goes for a key: "raise", "lower", "+1", "-1"...
pub fn volume_target(current: u32, action: &str) -> Option<u32> {
    let step: i64 = match action {
        "raise" => 5,
        "lower" => -5,
        other => other.parse().ok()?, // "+1", "-1", "+10"
    };
    Some((current as i64 + step).clamp(0, 100) as u32)
}

/// Where the brightness goes for a key, Omarchy's way (see the top of this file).
pub fn brightness_target(current: u32, action: &str) -> Option<u32> {
    let target: i64 = match action {
        "raise" if current < 5 => current as i64 + 1,
        "raise" => current as i64 + 5,
        "lower" if current <= 5 => current as i64 - 1,
        "lower" => current as i64 - 5,
        "max" => 100,
        "min" => 1,
        other => current as i64 + other.parse::<i64>().ok()?,
    };
    Some(target.clamp(1, 100) as u32)
}

pub fn volume(action: &str) -> Result<(), String> {
    if action == "mute" {
        sys::output("wpctl", &["set-mute", SINK, "toggle"]);
    } else {
        let (current, _) = bar::volume(SINK).ok_or("no speakers found")?;
        let next =
            volume_target(current, action).ok_or(format!("unknown volume action: {action}"))?;
        sys::output("wpctl", &["set-mute", SINK, "0"]);
        sys::output("wpctl", &["set-volume", SINK, &format!("{next}%")]);
    }
    osd("volume");
    Ok(())
}

pub fn mic(action: &str) -> Result<(), String> {
    if action != "mute" {
        return Err(format!("unknown mic action: {action}"));
    }
    sys::output("wpctl", &["set-mute", SOURCE, "toggle"]);
    osd("mic");
    Ok(())
}

/// The built-in screen's brightness in percent, from `brightnessctl -m`
/// ("intel_backlight,backlight,12000,63%,19200").
fn brightness_now() -> Option<u32> {
    let out = sys::output("brightnessctl", &["-m", "-c", "backlight"])?;
    out.lines()
        .next()?
        .split(',')
        .nth(3)?
        .trim_end_matches('%')
        .parse()
        .ok()
}

pub fn brightness(action: &str) -> Result<(), String> {
    let current = brightness_now().ok_or("no screen brightness control here")?;
    let next =
        brightness_target(current, action).ok_or(format!("unknown brightness action: {action}"))?;
    sys::output(
        "brightnessctl",
        &["-q", "-c", "backlight", "set", &format!("{next}%")],
    );
    osd("brightness");
    Ok(())
}

/// Show (or update in place) the little level notification, then refresh the
/// bar's volume icon. The notification id is kept so presses replace each other.
pub fn osd(what: &str) {
    let (icon, text, level) = match what {
        "volume" | "mic" => {
            let (target, label, icons, mute_icon) = if what == "mic" {
                (SOURCE, "Microphone", ["\u{f036c}"; 3], "\u{f036d}")
            } else {
                (
                    SINK,
                    "Volume",
                    ["\u{f057f}", "\u{f0580}", "\u{f057e}"],
                    "\u{f075f}",
                )
            };
            let Some((pct, muted)) = bar::volume(target) else {
                return;
            };
            if muted {
                (mute_icon, format!("{label} muted"), pct)
            } else {
                let i = if pct >= 66 {
                    2
                } else if pct >= 33 {
                    1
                } else {
                    0
                };
                (icons[i], format!("{label} {pct}%"), pct)
            }
        }
        "brightness" => {
            let Some(pct) = brightness_now() else { return };
            ("\u{f00e0}", format!("Brightness {pct}%"), pct)
        }
        _ => return,
    };
    let idfile = sys::runtime_dir().join("nameplate-osd-id");
    let old = sys::read(&idfile).unwrap_or_default();
    let hint = format!("int:value:{}", level.min(100));
    let summary = format!("{icon}  {text}");
    let mut args = vec![
        "-a",
        "nameplate-osd",
        "-u",
        "low",
        "-t",
        "1500",
        "-h",
        &hint,
        "-p",
    ];
    if !old.is_empty() {
        args.push("-r");
        args.push(&old);
    }
    args.push(&summary);
    if let Some(id) = sys::output("notify-send", &args) {
        let _ = std::fs::write(&idfile, format!("{}\n", id.trim()));
    }
    // A watching volume item has already seen the change; this covers a bar
    // config that runs it once on a signal.
    crate::watch::refresh(&[if what == "mic" { "mic" } else { "sound" }]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_like_omarchy() {
        assert_eq!(volume_target(47, "raise"), Some(52));
        assert_eq!(volume_target(98, "raise"), Some(100));
        assert_eq!(volume_target(3, "lower"), Some(0));
        assert_eq!(volume_target(50, "+1"), Some(51));
        assert_eq!(volume_target(50, "-1"), Some(49));
        assert_eq!(volume_target(50, "loud"), None);
    }

    #[test]
    fn brightness_like_omarchy() {
        assert_eq!(brightness_target(50, "raise"), Some(55));
        assert_eq!(brightness_target(98, "raise"), Some(100));
        assert_eq!(brightness_target(3, "raise"), Some(4)); // 1% steps at the dim end
        assert_eq!(brightness_target(5, "raise"), Some(10));
        assert_eq!(brightness_target(5, "lower"), Some(4));
        assert_eq!(brightness_target(6, "lower"), Some(1));
        assert_eq!(brightness_target(1, "lower"), Some(1)); // never black
        assert_eq!(brightness_target(40, "+1"), Some(41));
        assert_eq!(brightness_target(40, "max"), Some(100));
        assert_eq!(brightness_target(40, "min"), Some(1));
    }
}
