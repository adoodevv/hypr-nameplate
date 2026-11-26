//! The weather after the clock: now, the next hours, the next two days, and
//! the place, which you can set. bin/weather fetches from wttr.in and keeps
//! the cache (~/.cache/nameplate/weather.json); this reads what it gives.

use super::{
    background, clear, hbox, icon, icon_button, label, run_text, separator, vbox, Anchor, Panel,
};
use gtk::glib;
use gtk::prelude::*;
use nameplate::json::{self, Value};
use nameplate::sys;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const HUMIDITY: &str = "\u{f058e}";
const WIND: &str = "\u{f059d}";

pub struct Weather {
    root: gtk::Box,
    place: gtk::Label,
    body: gtk::Box,
    city: gtk::Entry,
}

impl Weather {
    pub fn new() -> Rc<Self> {
        let root = vbox(6);
        let head = hbox(8);
        let place = label("Weather", &["np-title"]);
        place.set_ellipsize(gtk::pango::EllipsizeMode::End);
        place.set_max_width_chars(34);
        place.set_width_chars(10);
        head.pack_start(&place, true, true, 0);
        root.pack_start(&head, false, false, 0);
        let body = vbox(8);
        root.pack_start(&body, false, false, 0);
        root.pack_start(&separator(), false, false, 2);
        let city = gtk::Entry::new();
        city.set_placeholder_text(Some("Your city (blank: automatic). Press Enter"));
        city.set_text(
            sys::read(sys::app_dir().join("weather-location"))
                .unwrap_or_default()
                .trim(),
        );
        let r#where = hbox(6);
        r#where.pack_start(&city, true, true, 0);
        root.pack_start(&r#where, false, false, 0);

        let me = Rc::new(Weather {
            root,
            place,
            body,
            city,
        });
        let weak = Rc::downgrade(&me);
        head.pack_end(
            &icon_button(icon::REFRESH, &[], "Refresh", move || {
                if let Some(me) = weak.upgrade() {
                    me.load(true);
                }
            }),
            false,
            false,
            0,
        );
        let weak = Rc::downgrade(&me);
        me.city.connect_activate(move |_| {
            if let Some(me) = weak.upgrade() {
                me.set_city();
            }
        });
        match cached() {
            Some(entry) => me.show(Some(entry)),
            None => {
                me.body
                    .pack_start(&label("Getting the weather…", &["np-dim"]), false, false, 6)
            }
        }
        me.load(false);
        me
    }

    /// Ask bin/weather (which fetches only when its copy is old, or when forced).
    fn load(self: &Rc<Self>, force: bool) {
        let weak = Rc::downgrade(self);
        background(
            move || {
                let weather = sys::bin("weather");
                let args: &[&str] = if force {
                    &["data", "--refresh"]
                } else {
                    &["data"]
                };
                let (ok, text) =
                    run_text(&weather.to_string_lossy(), args, Duration::from_secs(30));
                if ok {
                    json::parse(&text).filter(|v| v.get("data").is_some())
                } else {
                    None
                }
            },
            move |entry| {
                if let Some(me) = weak.upgrade() {
                    me.show(entry);
                }
            },
        );
    }

    fn set_city(self: &Rc<Self>) {
        let city = self.city.text().trim().to_string();
        clear(&self.body);
        let place = if city.is_empty() {
            "where you are"
        } else {
            city.as_str()
        };
        self.body.pack_start(
            &label(&format!("Getting the weather for {place}…"), &["np-dim"]),
            false,
            false,
            6,
        );
        self.body.show_all();
        let weak = Rc::downgrade(self);
        background(
            move || {
                let weather = sys::bin("weather");
                let mut args = vec!["location"];
                if !city.is_empty() {
                    args.push(&city);
                }
                run_text(&weather.to_string_lossy(), &args, Duration::from_secs(30));
                cached()
            },
            move |entry| {
                if let Some(me) = weak.upgrade() {
                    me.show(entry);
                }
            },
        );
    }

    fn show(&self, entry: Option<Value>) {
        clear(&self.body);
        let Some(entry) = entry else {
            self.body.pack_start(
                &label(
                    "Couldn't get the weather. Check your internet connection.",
                    &["np-dim"],
                ),
                false,
                false,
                6,
            );
            self.body.show_all();
            return;
        };
        let Some(d) = entry.get("data") else {
            return;
        };
        let now = first(d, "current_condition");
        let area = first(d, "nearest_area");
        let place = area.map(|a| {
            (
                text(first(a, "areaName"), "value", ""),
                text(first(a, "country"), "value", ""),
            )
        });
        match place {
            Some((name, country)) if !name.is_empty() => {
                self.place.set_text(&format!("{name}, {country}"))
            }
            _ => self.place.set_text("Weather"),
        }

        let top = hbox(14);
        top.pack_start(
            &label(glyph(text(now, "weatherCode", ""), None), &["np-huge"]),
            false,
            false,
            0,
        );
        top.pack_start(
            &label(&format!("{}°", text(now, "temp_C", "?")), &["np-huge"]),
            false,
            false,
            0,
        );
        let desc = vbox(2);
        desc.pack_start(
            &label(
                text(now.and_then(|n| first(n, "weatherDesc")), "value", ""),
                &[],
            ),
            false,
            false,
            0,
        );
        desc.pack_start(
            &label(
                &format!("Feels like {}°", text(now, "FeelsLikeC", "?")),
                &["np-dim"],
            ),
            false,
            false,
            0,
        );
        top.pack_start(&desc, false, false, 0);
        self.body.pack_start(&top, false, false, 0);
        self.body.pack_start(
            &label(
                &format!(
                    "{HUMIDITY} {}%   {WIND} {} km/h {}",
                    text(now, "humidity", "?"),
                    text(now, "windspeedKmph", "?"),
                    text(now, "winddir16Point", "")
                ),
                &["np-dim"],
            ),
            false,
            false,
            0,
        );

        // The next eight 3-hour slots, today running into tomorrow.
        let days = d.get("weather").map_or(&[][..], Value::as_array);
        let hour_now = glib::DateTime::now_local().map_or(0, |t| t.hour());
        let mut slots = Vec::new();
        for (i, day) in days.iter().take(2).enumerate() {
            for h in day.get("hourly").map_or(&[][..], Value::as_array) {
                let hour = text(Some(h), "time", "0").parse::<i32>().unwrap_or(0) / 100;
                if i > 0 || hour + 3 > hour_now {
                    slots.push((hour, h));
                }
            }
        }
        if !slots.is_empty() {
            self.body
                .pack_start(&label("Next hours", &["np-title"]), false, false, 2);
            let grid = gtk::Grid::new();
            grid.set_column_homogeneous(true);
            grid.set_row_spacing(2);
            for (col, (hour, h)) in slots.iter().take(8).enumerate() {
                let rain: i32 = text(Some(h), "chanceofrain", "0").parse().unwrap_or(0);
                let cells = [
                    (format!("{hour:02}:00"), Some("np-dim")),
                    (
                        glyph(text(Some(h), "weatherCode", ""), Some(*hour)).to_string(),
                        Some("np-big"),
                    ),
                    (format!("{}°", text(Some(h), "tempC", "?")), None),
                    (
                        if rain >= 20 {
                            format!("{rain}%")
                        } else {
                            String::new()
                        },
                        Some("np-dim"),
                    ),
                ];
                for (row, (t, class)) in cells.iter().enumerate() {
                    let l = label(t, class.as_slice());
                    l.set_xalign(0.5);
                    grid.attach(&l, col as i32, row as i32, 1, 1);
                }
            }
            self.body.pack_start(&grid, false, false, 0);
        }

        if days.len() > 1 {
            self.body
                .pack_start(&label("Next days", &["np-title"]), false, false, 2);
            for day in days.iter().skip(1).take(2) {
                let date = text(Some(day), "date", "");
                let noon = day
                    .get("hourly")
                    .map_or(&[][..], Value::as_array)
                    .iter()
                    .find(|h| text(Some(h), "time", "") == "1200");
                let line = hbox(10);
                line.pack_start(
                    &label(glyph(text(noon, "weatherCode", ""), Some(12)), &["np-icon"]),
                    false,
                    false,
                    0,
                );
                line.pack_start(&label(&weekday(date), &[]), true, true, 0);
                line.pack_end(
                    &label(
                        &format!(
                            "{}° / {}°",
                            text(Some(day), "mintempC", "?"),
                            text(Some(day), "maxtempC", "?")
                        ),
                        &[],
                    ),
                    false,
                    false,
                    0,
                );
                self.body.pack_start(&line, false, false, 0);
            }
        }
        let now_s = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let fetched = entry
            .get("fetched")
            .and_then(Value::as_f64)
            .unwrap_or(now_s);
        let age = ((now_s - fetched) / 60.0).floor() as i64;
        let updated = if age < 1 {
            "Updated just now".to_string()
        } else {
            format!("Updated {age} min ago")
        };
        self.body
            .pack_start(&label(&updated, &["np-dim"]), false, false, 0);
        self.body.show_all();
        // The bar shows the same reading.
        let rtmin = nameplate::ffi::sigrtmin();
        for p in sys::processes().iter().filter(|p| p.name == "waybar") {
            nameplate::ffi::send_signal(p.pid, rtmin + 16);
        }
    }
}

fn cached() -> Option<Value> {
    let text = sys::read(sys::home().join(".cache/nameplate/weather.json"))?;
    json::parse(&text).filter(|v| v.get("data").is_some())
}

/// The first item of an array field.
fn first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)?.as_array().first()
}

/// A text field, or `default` when it's missing.
fn text<'a>(v: Option<&'a Value>, key: &str, default: &'a str) -> &'a str {
    v.and_then(|v| v.get(key))
        .and_then(Value::as_str)
        .unwrap_or(default)
}

/// "2026-11-27" -> "Friday".
fn weekday(date: &str) -> String {
    let mut parts = date.split('-').map(|p| p.parse::<i32>().ok());
    if let (Some(Some(y)), Some(Some(m)), Some(Some(d))) =
        (parts.next(), parts.next(), parts.next())
    {
        if let Ok(t) = glib::DateTime::from_local(y, m, d, 12, 0, 0.0) {
            if let Ok(name) = t.format("%A") {
                return name.to_string();
            }
        }
    }
    date.to_string()
}

/// The Nerd Font weather icon for a wttr.in condition code: Omarchy's set,
/// with night versions from 18:00 to 06:00. `hour` defaults to now.
fn glyph(code: &str, hour: Option<i32>) -> &'static str {
    const CLOUDY: &str = "\u{e33d}";
    let hour = hour.unwrap_or_else(|| glib::DateTime::now_local().map_or(12, |t| t.hour()));
    let night = !(6..18).contains(&hour);
    let pick = |day: &'static str, dark: &'static str| if night { dark } else { day };
    match code.trim().parse::<u32>().unwrap_or(0) {
        113 => pick("\u{e30d}", "\u{e32b}"),
        116 => pick("\u{e302}", "\u{e32e}"),
        143 | 248 | 260 => pick("\u{e313}", "\u{e346}"),
        176 | 263 | 353 => pick("\u{e308}", "\u{e333}"),
        179 | 227 | 230 | 323 | 326 | 368 => pick("\u{e30a}", "\u{e327}"),
        119 | 122 => CLOUDY,
        182 | 185 | 281 | 284 | 311 | 314 | 317 | 320 | 350 | 362 | 365 | 374 | 377 => "\u{e3ad}",
        200 | 386 | 389 | 392 | 395 => "\u{e31d}",
        266 | 293 | 296 | 299 | 302 | 305 | 308 | 356 | 359 => "\u{e318}",
        329 | 332 | 335 | 338 | 371 => "\u{e31a}",
        _ => CLOUDY,
    }
}

impl Panel for Weather {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        380
    }
    fn anchor(&self) -> Anchor {
        Anchor::Top
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_and_days() {
        assert_eq!(glyph("113", Some(12)), "\u{e30d}");
        assert_eq!(glyph("113", Some(20)), "\u{e32b}");
        assert_eq!(glyph("389", Some(3)), "\u{e31d}");
        assert_eq!(glyph("", Some(12)), "\u{e33d}");
        assert_eq!(weekday("2026-11-27"), "Friday");
        assert_eq!(weekday("soon"), "soon");
    }
}
