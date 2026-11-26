//! Battery level and power profiles (the bar's battery icon).

use super::{
    background, capitalize, clear, every, hbox, icon, label, out, row, run, separator, vbox, Panel,
    Ran,
};
use gtk::prelude::*;
use nameplate::sys;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Known profiles in the order they're listed: (name, icon, title, hint).
const PROFILES: [(&str, &str, &str, &str); 3] = [
    (
        "power-saver",
        icon::LEAF,
        "Power saver",
        "Longest battery life",
    ),
    ("balanced", icon::BALANCE, "Balanced", "The usual mix"),
    (
        "performance",
        icon::ROCKET,
        "Performance",
        "Fastest, uses more power",
    ),
];

/// powerprofilesctl's answer: its exit was fine, and what it printed.
type Profiles = (bool, String);

pub struct Power {
    root: gtk::Box,
    bat: Option<PathBuf>,
    battery: gtk::Box,
    profiles: gtk::Box,
    shown_battery: RefCell<Option<Option<(String, String)>>>,
    shown_profiles: RefCell<Option<Profiles>>,
}

impl Power {
    pub fn new() -> Rc<Self> {
        let supply = std::env::var("NAMEPLATE_POWER_SUPPLY")
            .unwrap_or_else(|_| "/sys/class/power_supply".into());
        let mut bats: Vec<PathBuf> = std::fs::read_dir(&supply)
            .map(|d| {
                d.flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("BAT"))
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default();
        bats.sort();

        let root = vbox(6);
        let battery = vbox(4);
        root.pack_start(&battery, false, false, 0);
        root.pack_start(&separator(), false, false, 4);
        root.pack_start(&label("Power mode", &["np-title"]), false, false, 0);
        let profiles = vbox(0);
        root.pack_start(&profiles, false, false, 0);
        let me = Rc::new(Power {
            root,
            bat: bats.into_iter().next(),
            battery,
            profiles,
            shown_battery: RefCell::new(None),
            shown_profiles: RefCell::new(None),
        });
        me.fill_battery();
        me.fill(profiles_now());
        every(4000, &me, |me| me.update());
        me
    }

    fn update(self: &Rc<Self>) {
        self.fill_battery();
        let weak = Rc::downgrade(self);
        background(profiles_now, move |result| {
            if let Some(me) = weak.upgrade() {
                if me.shown_profiles.borrow().as_ref() != Some(&result) {
                    me.fill(result);
                }
            }
        });
    }

    /// The level and status from sysfs, and how long is left from upower
    /// (asked only when the level or status changes).
    fn fill_battery(self: &Rc<Self>) {
        let state = self.bat.as_ref().map(|b| {
            let get = |f: &str| sys::read(b.join(f)).map(|v| v.trim().to_string());
            (
                get("capacity").unwrap_or_else(|| "?".into()),
                get("status").unwrap_or_else(|| "Unknown".into()),
            )
        });
        if self.shown_battery.borrow().as_ref() == Some(&state)
            && !self.battery.children().is_empty()
        {
            return;
        }
        *self.shown_battery.borrow_mut() = Some(state.clone());
        clear(&self.battery);
        let Some((level, status)) = state else {
            self.battery
                .pack_start(&label("No battery found", &["np-title"]), false, false, 0);
            self.battery.show_all();
            return;
        };
        let n: i32 = level.parse().unwrap_or(50);
        let glyph = if status == "Charging" || status == "Full" {
            icon::CHARGING
        } else {
            icon::BATTERY[((n as f64 / 10.0).round() as i32).clamp(0, 10) as usize]
        };
        let top = hbox(12);
        top.pack_start(&label(glyph, &["np-huge"]), false, false, 0);
        top.pack_start(&label(&format!("{level}%"), &["np-huge"]), false, false, 0);
        self.battery.pack_start(&top, false, false, 0);
        let words = battery_text(&status, time_left().as_deref());
        self.battery
            .pack_start(&label(&words, &["np-dim"]), false, false, 0);
        self.battery.show_all();
    }

    fn fill(self: &Rc<Self>, result: Profiles) {
        *self.shown_profiles.borrow_mut() = Some(result.clone());
        clear(&self.profiles);
        let (ok, text) = result;
        if !ok {
            self.profiles.pack_start(
                &label(
                    "Power modes aren't available here\n(power-profiles-daemon isn't running).",
                    &["np-dim"],
                ),
                false,
                false,
                4,
            );
            self.profiles.show_all();
            return;
        }
        let mut found = parse_profiles(&text);
        found.sort_by_key(|(name, _)| PROFILES.iter().position(|p| p.0 == name).unwrap_or(9));
        for (name, active) in found {
            let known = PROFILES.iter().find(|p| p.0 == name);
            let (glyph, title, hint) = match known {
                Some(&(_, i, t, h)) => (i, t.to_string(), h),
                None => (icon::COG, capitalize(&name), ""),
            };
            let weak = Rc::downgrade(self);
            let pick = name.clone();
            let b = row(
                glyph,
                &title,
                if active { icon::CHECK } else { "" },
                move || {
                    if let Some(me) = weak.upgrade() {
                        me.set(&pick);
                    }
                },
            );
            if active {
                b.style_context().add_class("np-active");
            }
            if !hint.is_empty() {
                b.set_tooltip_text(Some(hint));
            }
            self.profiles.pack_start(&b, false, false, 0);
        }
        self.profiles.show_all();
    }

    fn set(self: &Rc<Self>, name: &str) {
        let why = match run(
            "powerprofilesctl",
            &["set", name],
            None,
            Duration::from_secs(10),
        ) {
            Ran::Ok(_) => None,
            Ran::Failed(o) => Some(String::from_utf8_lossy(&o).trim().to_string()),
            Ran::Missing => Some("powerprofilesctl is not installed".into()),
            Ran::TimedOut => Some("timed out".into()),
        };
        if let Some(why) = why {
            sys::spawn(
                "notify-send",
                &["-a", "Power", "Couldn't change the power mode", &why],
            );
        }
        self.fill(profiles_now());
    }
}

fn profiles_now() -> Profiles {
    match run("powerprofilesctl", &["list"], None, Duration::from_secs(5)) {
        Ran::Ok(o) => (true, String::from_utf8_lossy(&o).into_owned()),
        Ran::Failed(o) => (false, String::from_utf8_lossy(&o).into_owned()),
        _ => (false, String::new()),
    }
}

/// The profiles in `powerprofilesctl list` ("* balanced:" marks the current one).
fn parse_profiles(text: &str) -> Vec<(String, bool)> {
    text.lines()
        .filter_map(|line| {
            let t = line.trim();
            let (active, rest) = match t.strip_prefix('*') {
                Some(r) => (true, r.trim_start()),
                None => (false, t),
            };
            let name = rest.strip_suffix(':')?;
            (!name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'))
                .then(|| (name.to_string(), active))
        })
        .collect()
}

/// How long until empty or full, from upower: "1 h 20 min", "45 min".
fn time_left() -> Option<String> {
    let dev = out("upower", &["-e"])
        .lines()
        .find(|l| l.contains("BAT"))?
        .to_string();
    let info = out("upower", &["-i", &dev]);
    parse_time_left(&info)
}

fn parse_time_left(info: &str) -> Option<String> {
    let rest = info.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("time to empty:")
            .or_else(|| l.strip_prefix("time to full:"))
    })?;
    let mut words = rest.split_whitespace();
    let value: f64 = words.next()?.replace(',', ".").parse().ok()?;
    let unit = words.next()?;
    let minutes = if unit.starts_with("hour") {
        value * 60.0
    } else {
        value
    };
    Some(if minutes >= 60.0 {
        format!(
            "{} h {:02} min",
            (minutes / 60.0) as i64,
            (minutes % 60.0) as i64
        )
    } else {
        format!("{} min", minutes as i64)
    })
}

/// "Charging, full in 1 h 20 min", "3 h 05 min left", "Fully charged"...
fn battery_text(status: &str, left: Option<&str>) -> String {
    match (status, left) {
        ("Charging", Some(w)) => format!("Charging, full in {w}"),
        ("Charging", None) => "Charging".into(),
        ("Full", _) => "Fully charged".into(),
        ("Discharging", Some(w)) => format!("{w} left"),
        ("Discharging", None) => "On battery".into(),
        ("Not charging", _) => "Plugged in, not charging".into(),
        (other, _) => other.into(),
    }
}

impl Panel for Power {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        280
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles() {
        let text = "  performance:\n    CpuDriver:\tintel_pstate\n\n* balanced:\n    CpuDriver:\tintel_pstate\n\n  power-saver:\n";
        assert_eq!(
            parse_profiles(text),
            [
                ("performance".into(), false),
                ("balanced".into(), true),
                ("power-saver".into(), false)
            ]
        );
    }

    #[test]
    fn time_left_text() {
        assert_eq!(
            parse_time_left("  state: discharging\n  time to empty:  3.1 hours\n").as_deref(),
            Some("3 h 06 min")
        );
        assert_eq!(
            parse_time_left("  time to full:  45,5 minutes\n").as_deref(),
            Some("45 min")
        );
        assert_eq!(parse_time_left("  state: charging\n"), None);
        assert_eq!(
            battery_text("Discharging", Some("2 h 00 min")),
            "2 h 00 min left"
        );
        assert_eq!(
            battery_text("Not charging", None),
            "Plugged in, not charging"
        );
    }
}
