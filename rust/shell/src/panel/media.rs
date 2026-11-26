//! What's playing (playerctl): title, artist, a seek bar and the controls,
//! plus a list to switch between players when more than one is open. Also
//! shown under the calendar, where it hides entirely while nothing plays.

use super::{
    background, capitalize, clear, every, hbox, icon, icon_button, label, later, row, run,
    separator, vbox, Panel, Ran,
};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const FORMAT: &str =
    "{{status}}\x1f{{title}}\x1f{{artist}}\x1f{{playerName}}\x1f{{position}}\x1f{{mpris:length}}";
const TIMEOUT: Duration = Duration::from_secs(5);

/// One look at the players, taken off the GTK thread.
struct Snapshot {
    players: Vec<String>,
    /// The chosen player, if it's still open.
    player: Option<String>,
    /// playerctl's metadata line, when something is playing.
    metadata: Option<String>,
}

pub struct Media {
    root: gtk::Box,
    compact: bool,
    empty: gtk::Box,
    info: gtk::Box,
    title: gtk::Label,
    artist: gtk::Label,
    source: gtk::Label,
    seek: gtk::Scale,
    times: gtk::Box,
    position: gtk::Label,
    length: gtk::Label,
    play: gtk::Button,
    players: gtk::Box,
    player: RefCell<Option<String>>,
    dragging: Cell<bool>,
    busy: Cell<bool>,
    /// The player list as last drawn, to redraw it only when it changes.
    shape: RefCell<Option<(Vec<String>, String)>>,
}

impl Media {
    pub fn new(compact: bool) -> Rc<Self> {
        let root = vbox(6);
        let empty = vbox(4);
        empty.pack_start(
            &label(
                &format!("{}  Nothing is playing", icon::MUSIC),
                &["np-title"],
            ),
            false,
            false,
            0,
        );
        empty.pack_start(
            &label(
                "Spotify, YouTube in Chrome and most\nmedia apps show up here while playing.",
                &["np-dim"],
            ),
            false,
            false,
            0,
        );
        root.pack_start(&empty, false, false, 0);

        let info = vbox(6);
        if compact {
            info.pack_start(&separator(), false, false, 4);
        }
        let title = label("", &["np-title"]);
        let artist = label("", &[]);
        for l in [&title, &artist] {
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_max_width_chars(34);
            l.set_width_chars(10);
        }
        let source = label("", &["np-dim"]);
        let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
        seek.set_draw_value(false);
        let (position, length) = (label("", &["np-dim"]), label("", &["np-dim"]));
        let times = hbox(8);
        times.pack_start(&position, false, false, 0);
        times.pack_end(&length, false, false, 0);

        let me = Rc::new_cyclic(|weak: &std::rc::Weak<Media>| {
            let act = |what: &'static str| {
                let weak = weak.clone();
                move || {
                    if let Some(me) = weak.upgrade() {
                        me.act(what);
                    }
                }
            };
            let buttons = hbox(18);
            buttons.set_halign(gtk::Align::Center);
            buttons.pack_start(
                &icon_button(icon::PREV, &[], "Previous", act("previous")),
                false,
                false,
                0,
            );
            let play = icon_button(icon::PLAY, &["np-main"], "Play / pause", act("play-pause"));
            buttons.pack_start(&play, false, false, 0);
            buttons.pack_start(
                &icon_button(icon::NEXT, &[], "Next", act("next")),
                false,
                false,
                0,
            );
            let players = vbox(0);
            for w in [
                title.upcast_ref::<gtk::Widget>(),
                artist.upcast_ref(),
                source.upcast_ref(),
                seek.upcast_ref(),
                times.upcast_ref(),
                buttons.upcast_ref(),
                players.upcast_ref(),
            ] {
                info.pack_start(w, false, false, 0);
            }
            root.pack_start(&info, false, false, 0);
            Media {
                root,
                compact,
                empty,
                info,
                title,
                artist,
                source,
                seek,
                times,
                position,
                length,
                play,
                players,
                player: RefCell::new(None),
                dragging: Cell::new(false),
                busy: Cell::new(false),
                shape: RefCell::new(None),
            }
        });

        let weak = Rc::downgrade(&me);
        me.seek.connect_button_press_event(move |_, _| {
            if let Some(me) = weak.upgrade() {
                me.dragging.set(true);
            }
            gtk::glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(&me);
        me.seek.connect_button_release_event(move |scale, _| {
            if let Some(me) = weak.upgrade() {
                let to = (scale.value().max(0.0) as i64).to_string();
                me.ctl(&["position", &to]);
                me.dragging.set(false);
                later(300, &me, |me| me.refresh());
            }
            gtk::glib::Propagation::Proceed
        });
        every(1000, &me, |me| me.refresh());
        me
    }

    /// playerctl for the chosen player (or whichever it picks).
    fn ctl(&self, args: &[&str]) {
        let player = self.player.borrow().clone();
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        background(
            move || {
                let mut all: Vec<&str> = Vec::new();
                if let Some(p) = &player {
                    all.extend(["-p", p.as_str()]);
                }
                all.extend(args.iter().map(String::as_str));
                run("playerctl", &all, None, TIMEOUT);
            },
            |_| (),
        );
    }

    fn act(self: &Rc<Self>, what: &str) {
        self.ctl(&[what]);
        later(250, self, |me| me.refresh());
    }

    pub fn refresh(self: &Rc<Self>) {
        if self.busy.replace(true) {
            return; // the last look hasn't come back yet
        }
        let chosen = self.player.borrow().clone();
        let weak = Rc::downgrade(self);
        background(
            move || snapshot(chosen),
            move |snap| {
                if let Some(me) = weak.upgrade() {
                    me.busy.set(false);
                    me.show(snap);
                }
            },
        );
    }

    /// Look now, on the GTK thread: when the panel opens, so it appears right.
    pub fn refresh_now(self: &Rc<Self>) {
        let snap = snapshot(self.player.borrow().clone());
        self.show(snap);
    }
}

fn snapshot(chosen: Option<String>) -> Snapshot {
    let players: Vec<String> = match run("playerctl", &["-l"], None, TIMEOUT) {
        Ran::Ok(o) => String::from_utf8_lossy(&o)
            .lines()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    };
    let player = chosen.filter(|p| players.contains(p));
    let mut args = Vec::new();
    if let Some(p) = &player {
        args.extend(["-p", p.as_str()]);
    }
    args.extend(["metadata", "--format", FORMAT]);
    let metadata = match run("playerctl", &args, None, TIMEOUT) {
        Ran::Ok(o) => {
            Some(String::from_utf8_lossy(&o).trim().to_string()).filter(|t| !t.is_empty())
        }
        _ => None,
    };
    Snapshot {
        players,
        player,
        metadata,
    }
}

impl Media {
    fn show(self: &Rc<Self>, snap: Snapshot) {
        *self.player.borrow_mut() = snap.player;
        let playing = snap.metadata.is_some();
        self.empty.set_visible(!playing && !self.compact);
        self.info.set_visible(playing);
        let Some(text) = snap.metadata else {
            return;
        };
        let mut f: Vec<&str> = text.split('\x1f').collect();
        f.resize(6, "");
        let (status, title, artist, name, pos, len) = (f[0], f[1], f[2], f[3], f[4], f[5]);
        self.title.set_text(if title.is_empty() {
            "Unknown title"
        } else {
            title
        });
        self.artist.set_text(artist);
        self.artist.set_visible(!artist.is_empty());
        self.source
            .set_text(&capitalize(name.split('.').next().unwrap_or("")));
        self.play.set_label(if status == "Playing" {
            icon::PAUSE
        } else {
            icon::PLAY
        });
        // Microseconds; an empty field is 0, and anything unreadable drops both.
        let micros = |v: &str| {
            if v.is_empty() {
                Ok(0)
            } else {
                v.parse::<i64>()
            }
        };
        let (pos, len) = match (micros(pos), micros(len)) {
            (Ok(p), Ok(l)) => (p as f64 / 1e6, l as f64 / 1e6),
            _ => (0.0, 0.0),
        };
        let has_len = len > 0.0;
        self.seek.set_visible(has_len);
        self.times.set_visible(has_len);
        if has_len && !self.dragging.get() {
            self.seek.set_range(0.0, len);
            self.seek.set_value(pos.min(len));
            self.position.set_text(&clock(pos));
            self.length.set_text(&clock(len));
        }

        let shape = (snap.players.clone(), name.to_string());
        if snap.players.len() > 1 && self.shape.borrow().as_ref() != Some(&shape) {
            clear(&self.players);
            self.players.pack_start(&separator(), false, false, 0);
            for p in &snap.players {
                let weak = Rc::downgrade(self);
                let pick = p.clone();
                let b = row(
                    icon::MUSIC,
                    &capitalize(p.split('.').next().unwrap_or("")),
                    "",
                    move || {
                        if let Some(me) = weak.upgrade() {
                            *me.player.borrow_mut() = Some(pick.clone());
                            *me.shape.borrow_mut() = None;
                            me.refresh();
                        }
                    },
                );
                if p == name || p.split('.').next() == Some(name) {
                    b.style_context().add_class("np-active");
                }
                self.players.pack_start(&b, false, false, 0);
            }
            self.players.show_all();
        } else if snap.players.len() <= 1 {
            clear(&self.players);
        }
        *self.shape.borrow_mut() = Some(shape);
    }
}

/// 75 -> "1:15", 3725 -> "1:02:05".
fn clock(secs: f64) -> String {
    let s = secs as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

impl Panel for Media {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        320
    }
    fn shown(self: Rc<Self>) {
        self.refresh_now();
    }
}

impl Media {
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clock() {
        assert_eq!(super::clock(75.9), "1:15");
        assert_eq!(super::clock(3725.0), "1:02:05");
        assert_eq!(super::clock(0.0), "0:00");
    }
}
