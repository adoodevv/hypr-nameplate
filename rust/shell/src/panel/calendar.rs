//! The calendar under the clock, with a world clock and, while something
//! plays, the media player underneath.
//!
//! The world clock (SUPER + CTRL + ALT + E opens the calendar with it, like
//! Omarchy's) lists the places in ~/.config/nameplate/clocks, one per line:
//! "Europe/London", or a name and then the zone, "Mum  America/Toronto".

use super::{hbox, icon, label, media::Media, row, separator, vbox, Anchor, Panel};
use gtk::glib;
use gtk::prelude::*;
use nameplate::sys;
use std::path::Path;
use std::rc::Rc;

pub struct Calendar {
    root: gtk::Box,
    cal: gtk::Calendar,
    media: Rc<Media>,
}

impl Calendar {
    /// `clock_hint`: opened for the world clock, so explain how to add places
    /// when there are none.
    pub fn new(clock_hint: bool) -> Rc<Self> {
        let root = vbox(6);
        let now = glib::DateTime::now_local().ok();
        let fmt = |f: &str| {
            now.as_ref()
                .and_then(|n| n.format(f).ok())
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        let day = label(&fmt("%A"), &["np-dim"]);
        let date = label(&fmt("%-d %B %Y"), &["np-big"]);
        for l in [&day, &date] {
            l.set_xalign(0.5);
            root.pack_start(l, false, false, 0);
        }
        let cal = gtk::Calendar::new();
        cal.set_display_options(
            gtk::CalendarDisplayOptions::SHOW_HEADING | gtk::CalendarDisplayOptions::SHOW_DAY_NAMES,
        );
        root.pack_start(&cal, false, false, 4);

        let media = Media::new(true);
        let me = Rc::new(Calendar { root, cal, media });
        let weak = Rc::downgrade(&me);
        me.root.pack_start(
            &row(icon::REFRESH, "Back to today", "", move || {
                if let Some(me) = weak.upgrade() {
                    me.today();
                }
            }),
            false,
            false,
            0,
        );
        me.world_clock(clock_hint);
        me.root.pack_start(me.media.widget(), false, false, 0);
        me
    }

    fn world_clock(&self, hint: bool) {
        let rows = clocks(
            &sys::read(sys::app_dir().join("clocks")).unwrap_or_default(),
            glib::DateTime::now_local().ok().as_ref(),
        );
        if rows.is_empty() && !hint {
            return;
        }
        self.root.pack_start(&separator(), false, false, 4);
        if rows.is_empty() {
            self.root.pack_start(
                &label(
                    "Add places to ~/.config/nameplate/clocks,\none per line: Europe/London",
                    &["np-dim"],
                ),
                false,
                false,
                0,
            );
            return;
        }
        for (name, when) in rows {
            let line = hbox(10);
            line.pack_start(&label(&name, &[]), false, false, 0);
            line.pack_end(&label(&when, &["np-dim"]), false, false, 0);
            self.root.pack_start(&line, false, false, 0);
        }
    }

    fn today(&self) {
        if let Ok(now) = glib::DateTime::now_local() {
            self.cal
                .select_month(now.month() as u32 - 1, now.year() as u32);
            self.cal.select_day(now.day_of_month() as u32);
        }
    }
}

/// (place, "14:05  tomorrow") for each line of the clocks file. Zones that
/// don't exist are skipped.
fn clocks(text: &str, local: Option<&glib::DateTime>) -> Vec<(String, String)> {
    let zonedir = std::env::var("TZDIR").unwrap_or_else(|_| "/usr/share/zoneinfo".into());
    let mut rows = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let zone = words[words.len() - 1];
        // A real zone file, not a path out of the folder.
        if zone.contains("..") || zone.starts_with('/') || !Path::new(&zonedir).join(zone).is_file()
        {
            continue;
        }
        let name = if words.len() > 1 {
            words[..words.len() - 1].join(" ")
        } else {
            zone.rsplit('/').next().unwrap_or(zone).replace('_', " ")
        };
        let Ok(there) = glib::DateTime::now(&glib::TimeZone::new(Some(zone))) else {
            continue;
        };
        let mut when = there
            .format("%H:%M")
            .map(|s| s.to_string())
            .unwrap_or_default();
        if let Some(here) = local {
            let ymd = |d: &glib::DateTime| (d.year(), d.month(), d.day_of_month());
            match ymd(&there).cmp(&ymd(here)) {
                std::cmp::Ordering::Greater => when.push_str("  tomorrow"),
                std::cmp::Ordering::Less => when.push_str("  yesterday"),
                std::cmp::Ordering::Equal => {}
            }
        }
        rows.push((name, when));
    }
    rows
}

impl Panel for Calendar {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn anchor(&self) -> Anchor {
        Anchor::Top
    }
    fn shown(self: Rc<Self>) {
        self.media.clone().refresh_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_clock_lines() {
        let rows = clocks(
            "# a comment\n\nEurope/London\nMum  America/New_York\nNowhere/Atlantis\n",
            None,
        );
        let names: Vec<&str> = rows.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["London", "Mum"]);
        assert!(rows
            .iter()
            .all(|(_, when)| when.len() == 5 && when.as_bytes()[2] == b':'));
        assert!(clocks("../../etc/passwd\n", None).is_empty());
    }
}
