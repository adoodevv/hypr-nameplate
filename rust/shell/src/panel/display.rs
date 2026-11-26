//! Screens (the monitor icon in the bar), like Omarchy 4's display panel: which
//! screen, brightness (when it has a backlight), text size, scale, resolution,
//! refresh rate, and the layout when a second screen is plugged in. Changes
//! apply at once and are remembered by bin/display, which also reports the
//! screens (as JSON) and restores the settings at login.

use super::{background, clear, hbox, label, run_text, vbox, Panel};
use gtk::glib;
use gtk::prelude::*;
use nameplate::json::{self, Value};
use nameplate::sys;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// Omarchy's text sizes, one notch each.
const TEXT_STOPS: [i64; 7] = [9, 10, 11, 12, 14, 16, 20];
const SCALES: [&str; 4] = ["1", "1.25", "1.6", "2"];

#[derive(Clone)]
struct Screen {
    name: String,
    title: String,
    internal: bool,
    disabled: bool,
    size: String,
    hz: String,
    scale: String,
    /// (size, refresh rate)
    modes: Vec<(String, String)>,
}

#[derive(Clone)]
struct Data {
    screens: Vec<Screen>,
    brightness: Option<i64>,
    text: i64,
}

pub struct Display {
    root: gtk::Box,
    body: gtk::Box,
    chosen: RefCell<Option<String>>,
    data: RefCell<Option<Data>>,
    timers: RefCell<HashMap<&'static str, glib::SourceId>>,
}

fn display_bin() -> String {
    sys::bin("display").to_string_lossy().into_owned()
}

impl Display {
    pub fn new(route: Option<&str>) -> Rc<Self> {
        let root = vbox(10);
        let body = vbox(10);
        root.pack_start(&body, false, false, 0);
        let me = Rc::new(Display {
            root,
            body,
            chosen: RefCell::new(route.map(String::from)),
            data: RefCell::new(None),
            timers: RefCell::default(),
        });
        me.load();
        me
    }

    fn load(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        background(
            || {
                let (ok, text) = run_text(&display_bin(), &[], Duration::from_secs(15));
                if ok {
                    json::parse(&text).and_then(|v| parse_data(&v))
                } else {
                    None
                }
            },
            move |data| {
                if let Some(me) = weak.upgrade() {
                    me.show(data);
                }
            },
        );
    }

    fn show(self: &Rc<Self>, data: Option<Data>) {
        clear(&self.body);
        let Some(data) = data.filter(|d| !d.screens.is_empty()) else {
            self.body.pack_start(
                &label("Couldn't read the screens.", &["np-dim"]),
                false,
                false,
                6,
            );
            self.body.show_all();
            return;
        };
        *self.data.borrow_mut() = Some(data.clone());
        let live: Vec<&Screen> = {
            let on: Vec<&Screen> = data.screens.iter().filter(|m| !m.disabled).collect();
            if on.is_empty() {
                data.screens.iter().collect()
            } else {
                on
            }
        };
        let chosen = self.chosen.borrow().clone();
        let cur = live
            .iter()
            .find(|m| Some(&m.name) == chosen.as_ref())
            .or_else(|| live.iter().find(|m| m.internal))
            .unwrap_or(&live[0]);
        let cur: Screen = (*cur).clone();

        // The screen and what it's running at; a picker when there are several.
        let head = vbox(2);
        head.pack_start(&label(&cur.title, &["np-title"]), false, false, 0);
        head.pack_start(
            &label(
                &format!(
                    "{}  ·  {} Hz  ·  {}×",
                    cur.size.replace('x', "×"),
                    cur.hz,
                    cur.scale
                ),
                &["np-dim"],
            ),
            false,
            false,
            0,
        );
        self.body.pack_start(&head, false, false, 0);
        if live.len() > 1 {
            let items: Vec<(String, String)> = live
                .iter()
                .map(|m| (m.title.clone(), m.name.clone()))
                .collect();
            let weak = Rc::downgrade(self);
            self.body.pack_start(
                &chips(&items, Some(&cur.name), 4, move |name| {
                    if let Some(me) = weak.upgrade() {
                        *me.chosen.borrow_mut() = Some(name);
                        let data = me.data.borrow().clone();
                        me.show(data);
                    }
                }),
                false,
                false,
                0,
            );
        }

        if let (Some(bright), true) = (data.brightness, cur.internal) {
            let (line, value) = heading("Brightness", &format!("{bright}%"));
            self.body.pack_start(&line, false, false, 0);
            let sc = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 100.0, 1.0);
            sc.set_draw_value(false);
            sc.set_value(bright as f64);
            let weak = Rc::downgrade(self);
            sc.connect_value_changed(move |s| {
                let v = s.value() as i64;
                value.set_text(&format!("{v}%"));
                if let Some(me) = weak.upgrade() {
                    me.debounce("bright", move || {
                        let level = format!("{v}%");
                        sys::spawn("brightnessctl", &["-q", "-c", "backlight", "set", &level]);
                    });
                }
            });
            self.body.pack_start(&sc, false, false, 0);
        }

        // Text size, at Omarchy's stops.
        let now = nearest(&TEXT_STOPS, data.text);
        let (line, value) = heading("Text size", &format!("{} px", TEXT_STOPS[now]));
        self.body.pack_start(&line, false, false, 0);
        let ts = gtk::Scale::with_range(
            gtk::Orientation::Horizontal,
            0.0,
            (TEXT_STOPS.len() - 1) as f64,
            1.0,
        );
        ts.set_draw_value(false);
        ts.set_round_digits(0);
        for i in 0..TEXT_STOPS.len() {
            ts.add_mark(i as f64, gtk::PositionType::Bottom, None);
        }
        ts.set_value(now as f64);
        let weak = Rc::downgrade(self);
        ts.connect_value_changed(move |s| {
            let px = TEXT_STOPS[(s.value().round() as usize).min(TEXT_STOPS.len() - 1)];
            value.set_text(&format!("{px} px"));
            if let Some(me) = weak.upgrade() {
                me.debounce("text", move || {
                    let px = px.to_string();
                    sys::spawn(display_bin(), &["text", &px]);
                });
            }
        });
        self.body.pack_start(&ts, false, false, 0);

        self.body
            .pack_start(&heading("Scale", "").0, false, false, 0);
        let mut scales: Vec<String> = SCALES
            .iter()
            .filter(|v| scale_valid(&cur.size, v))
            .map(|v| v.to_string())
            .collect();
        if scales.is_empty() {
            scales.push("1".into());
        }
        if !scales.contains(&cur.scale) {
            scales.push(cur.scale.clone());
            if scales.iter().all(|s| s.parse::<f64>().is_ok()) {
                scales.sort_by(|a, b| {
                    a.parse::<f64>()
                        .unwrap_or(0.0)
                        .total_cmp(&b.parse::<f64>().unwrap_or(0.0))
                });
            }
        }
        let items: Vec<(String, String)> = scales
            .iter()
            .map(|v| (format!("{v}×"), v.clone()))
            .collect();
        let weak = Rc::downgrade(self);
        self.body.pack_start(
            &chips(&items, Some(&cur.scale), 4, move |v| {
                if let Some(me) = weak.upgrade() {
                    me.set("scale", &v);
                }
            }),
            false,
            false,
            0,
        );

        let sizes = sizes(&cur.modes);
        if sizes.len() > 1 {
            let shown: Vec<String> = if sizes.iter().take(6).any(|s| *s == cur.size) {
                sizes.iter().take(6).cloned().collect()
            } else {
                sizes
                    .iter()
                    .take(5)
                    .cloned()
                    .chain([cur.size.clone()])
                    .collect()
            };
            self.body
                .pack_start(&heading("Resolution", "").0, false, false, 0);
            let items: Vec<(String, String)> = shown
                .iter()
                .map(|t| (t.replace('x', "×"), t.clone()))
                .collect();
            let weak = Rc::downgrade(self);
            self.body.pack_start(
                &chips(&items, Some(&cur.size), 3, move |t| {
                    if let Some(me) = weak.upgrade() {
                        me.set_mode(&t, None);
                    }
                }),
                false,
                false,
                0,
            );
        }
        let rates = rates(&cur.modes, &cur.size);
        if rates.len() > 1 {
            let hz_now: f64 = cur.hz.parse().unwrap_or(0.0);
            let near = rates
                .iter()
                .fold(None::<&String>, |best, r| {
                    let d = |x: &String| (x.parse::<f64>().unwrap_or(0.0) - hz_now).abs();
                    match best {
                        Some(b) if d(b) <= d(r) => Some(b),
                        _ => Some(r),
                    }
                })
                .cloned();
            self.body
                .pack_start(&heading("Refresh rate", "").0, false, false, 0);
            let items: Vec<(String, String)> = rates
                .iter()
                .take(5)
                .map(|h| {
                    (
                        format!("{:.0} Hz", h.parse::<f64>().unwrap_or(0.0)),
                        h.clone(),
                    )
                })
                .collect();
            let (weak, size) = (Rc::downgrade(self), cur.size.clone());
            self.body.pack_start(
                &chips(&items, near.as_deref(), 4, move |h| {
                    if let Some(me) = weak.upgrade() {
                        me.set_mode(&size, Some(h));
                    }
                }),
                false,
                false,
                0,
            );
        }

        if data.screens.len() > 1 {
            self.body
                .pack_start(&heading("Layout", "").0, false, false, 0);
            self.layout_rows(&data.screens);
        }
        self.body.show_all();
    }

    /// Apply a slider at most every 250 ms.
    fn debounce(self: &Rc<Self>, key: &'static str, f: impl FnOnce() + 'static) {
        if let Some(id) = self.timers.borrow_mut().remove(key) {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            if let Some(me) = weak.upgrade() {
                me.timers.borrow_mut().remove(key); // it has fired: nothing to cancel
            }
            f();
        });
        self.timers.borrow_mut().insert(key, id);
    }

    fn current(&self) -> Option<Screen> {
        let data = self.data.borrow();
        let data = data.as_ref()?;
        let live: Vec<&Screen> = {
            let on: Vec<&Screen> = data.screens.iter().filter(|m| !m.disabled).collect();
            if on.is_empty() {
                data.screens.iter().collect()
            } else {
                on
            }
        };
        let chosen = self.chosen.borrow();
        live.iter()
            .find(|m| Some(&m.name) == chosen.as_ref())
            .or_else(|| live.iter().find(|m| m.internal))
            .or(live.first())
            .map(|m| (*m).clone())
    }

    fn set(self: &Rc<Self>, key: &str, value: &str) {
        let Some(cur) = self.current() else {
            return;
        };
        let (key, value) = (key.to_string(), value.to_string());
        let weak = Rc::downgrade(self);
        background(
            move || {
                run_text(
                    &display_bin(),
                    &["set", &cur.name, &key, &value],
                    Duration::from_secs(15),
                )
            },
            move |(ok, text)| {
                if !ok {
                    sys::spawn(
                        "notify-send",
                        &["-a", "Display", "Couldn't change the screen", &text],
                    );
                }
                if let Some(me) = weak.upgrade() {
                    me.load();
                }
            },
        );
    }

    /// A new resolution gets its fastest refresh rate.
    fn set_mode(self: &Rc<Self>, size: &str, hz: Option<String>) {
        let Some(cur) = self.current() else {
            return;
        };
        let hz = hz.unwrap_or_else(|| {
            cur.modes
                .iter()
                .filter(|(s, _)| s == size)
                .map(|(_, h)| h.clone())
                .fold(None::<String>, |best, h| match best {
                    Some(b)
                        if b.parse::<f64>().unwrap_or(0.0) >= h.parse::<f64>().unwrap_or(0.0) =>
                    {
                        Some(b)
                    }
                    _ => Some(h),
                })
                .unwrap_or(cur.hz.clone())
        });
        self.set("mode", &format!("{size}@{hz}"));
    }

    fn layout_rows(self: &Rc<Self>, screens: &[Screen]) {
        let (Some(lap), Some(ext)) = (
            screens.iter().find(|m| m.internal),
            screens.iter().find(|m| !m.internal),
        ) else {
            return;
        };
        let (lap, name) = (lap.name.clone(), ext.name.clone());
        let ext_word = ext
            .title
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        let options: Vec<(String, Option<Vec<String>>)> = vec![
            (
                "Extend right".into(),
                Some(vec![on(&lap, "auto", ""), on(&name, "auto-right", "")]),
            ),
            (
                "Extend left".into(),
                Some(vec![on(&lap, "auto", ""), on(&name, "auto-left", "")]),
            ),
            (
                "Mirror".into(),
                Some(vec![
                    on(&lap, "auto", ""),
                    on(&name, "auto", &format!(", mirror = \"{lap}\"")),
                ]),
            ),
            (
                format!("{ext_word} only"),
                Some(vec![on(&name, "auto", ""), off(&lap)]),
            ),
            (
                "Laptop only".into(),
                Some(vec![on(&lap, "auto", ""), off(&name)]),
            ),
            ("Reset".into(), None),
        ];
        let items: Vec<(String, String)> = options
            .iter()
            .map(|(t, _)| (t.clone(), t.clone()))
            .collect();
        self.body.pack_start(
            &chips(&items, None, 3, move |t| {
                match options
                    .iter()
                    .find(|(o, _)| *o == t)
                    .and_then(|(_, r)| r.clone())
                {
                    None => {
                        run_text("hyprctl", &["reload"], Duration::from_secs(10));
                        run_text(&display_bin(), &["restore"], Duration::from_secs(15));
                    }
                    Some(rules) => {
                        let (ok, text) = run_text(
                            "hyprctl",
                            &["eval", &rules.join("; ")],
                            Duration::from_secs(10),
                        );
                        if !ok || (!text.is_empty() && !text.starts_with("ok")) {
                            sys::spawn(
                                "notify-send",
                                &["-a", "Display", "Couldn't change the screens", &text],
                            );
                        }
                    }
                }
                super::close();
            }),
            false,
            false,
            0,
        );
    }
}

fn on(name: &str, pos: &str, extra: &str) -> String {
    format!("hl.monitor({{ output = \"{name}\", mode = \"preferred\", position = \"{pos}\", scale = 1{extra} }})")
}

fn off(name: &str) -> String {
    format!("hl.monitor({{ output = \"{name}\", disabled = true }})")
}

/// A heading line, with a value on the right; the value's label too.
fn heading(text: &str, value: &str) -> (gtk::Box, gtk::Label) {
    let line = hbox(8);
    line.pack_start(&label(text, &["np-dim"]), true, true, 0);
    let v = label(value, &["np-dim"]);
    v.set_xalign(1.0);
    line.pack_end(&v, false, false, 0);
    (line, v)
}

/// A row of choices (shown as text, picked by value), the current one marked.
fn chips(
    items: &[(String, String)],
    current: Option<&str>,
    per_line: u32,
    pick: impl Fn(String) + 'static,
) -> gtk::FlowBox {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    let across = per_line.min(items.len() as u32).max(1);
    flow.set_min_children_per_line(across);
    flow.set_max_children_per_line(across);
    flow.set_column_spacing(6);
    flow.set_row_spacing(6);
    let pick = Rc::new(pick);
    for (text, value) in items {
        let b = gtk::Button::with_label(text);
        b.set_hexpand(true);
        b.style_context().add_class("np-chip");
        if Some(value.as_str()) == current {
            b.style_context().add_class("np-current");
        }
        let (pick, value) = (pick.clone(), value.clone());
        b.connect_clicked(move |_| pick(value.clone()));
        flow.add(&b);
    }
    flow
}

/// The index of the stop nearest `value` (the first of equals).
fn nearest(stops: &[i64], value: i64) -> usize {
    (0..stops.len())
        .min_by_key(|&i| (stops[i] - value).abs())
        .unwrap_or(0)
}

/// Hyprland only accepts scales that divide the mode into whole logical
/// pixels: 1.25 works on 1920x1080 but not on 1366x768.
fn scale_valid(size: &str, value: &str) -> bool {
    let parts: Vec<Option<f64>> = size
        .to_lowercase()
        .split('x')
        .map(|p| p.parse::<f64>().ok())
        .collect();
    let (Some(Some(w)), Some(Some(h)), Ok(s)) = (parts.first(), parts.get(1), value.parse::<f64>())
    else {
        return true;
    };
    if parts.len() != 2 {
        return true;
    }
    if s <= 0.0 {
        return false;
    }
    let whole = |x: f64| (x - x.round()).abs() < 1e-9;
    whole(w / s) && whole(h / s)
}

/// The sizes offered, each once, biggest first.
fn sizes(modes: &[(String, String)]) -> Vec<String> {
    let mut sizes: Vec<String> = Vec::new();
    for (s, _) in modes {
        if !sizes.contains(s) {
            sizes.push(s.clone());
        }
    }
    let area = |t: &String| {
        let mut p = t.split('x').map(|x| x.parse::<i64>().unwrap_or(0));
        p.next().unwrap_or(0) * p.next().unwrap_or(0)
    };
    sizes.sort_by_key(|t| -area(t));
    sizes
}

/// The refresh rates for a size, each once, fastest first.
fn rates(modes: &[(String, String)], size: &str) -> Vec<String> {
    let mut rates: Vec<String> = Vec::new();
    for (s, h) in modes {
        if s == size && !rates.contains(h) {
            rates.push(h.clone());
        }
    }
    rates.sort_by(|a, b| {
        b.parse::<f64>()
            .unwrap_or(0.0)
            .total_cmp(&a.parse::<f64>().unwrap_or(0.0))
    });
    rates
}

fn parse_data(v: &Value) -> Option<Data> {
    let text_of = |v: &Value| match v {
        Value::Str(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let screens = v
        .get("screens")?
        .as_array()
        .iter()
        .map(|m| Screen {
            name: m.str_of("name").to_string(),
            title: m.str_of("title").to_string(),
            internal: m.get("internal").and_then(Value::as_bool).unwrap_or(false),
            disabled: m.get("disabled").and_then(Value::as_bool).unwrap_or(false),
            size: m.str_of("size").to_string(),
            hz: m.get("hz").map(text_of).unwrap_or_default(),
            scale: m.get("scale").map(text_of).unwrap_or_default(),
            modes: m
                .get("modes")
                .map_or(&[][..], Value::as_array)
                .iter()
                .filter_map(|p| {
                    let p = p.as_array();
                    Some((text_of(p.first()?), text_of(p.get(1)?)))
                })
                .collect(),
        })
        .collect();
    Some(Data {
        screens,
        brightness: v
            .get("brightness")
            .and_then(Value::as_f64)
            .map(|b| b as i64),
        text: v
            .get("text")
            .and_then(Value::as_f64)
            .map_or(12, |t| t as i64),
    })
}

impl Panel for Display {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        360
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices() {
        assert!(scale_valid("1920x1080", "1.25"));
        assert!(!scale_valid("1366x768", "1.25"));
        assert!(scale_valid("2880x1800", "1.6"));
        assert!(scale_valid("weird", "1.25"));
        assert_eq!(nearest(&TEXT_STOPS, 13), 3); // 12 and 14 tie: the first
        let modes: Vec<(String, String)> = [
            ("1920x1080", "60.00"),
            ("1280x720", "60.00"),
            ("1920x1080", "144.00"),
            ("2560x1440", "59.95"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(sizes(&modes), ["2560x1440", "1920x1080", "1280x720"]);
        assert_eq!(rates(&modes, "1920x1080"), ["144.00", "60.00"]);
    }
}
