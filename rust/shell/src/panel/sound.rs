//! Volume and the output device (the bar's sound icon). Follows changes made
//! elsewhere (volume keys, headphones plugged in) except while you're moving
//! the slider.

use super::{
    background, clear, every, hbox, icon, icon_button, label, out, row, separator, vbox, Panel,
};
use gtk::glib;
use gtk::prelude::*;
use nameplate::sys;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

const SINK: &str = "@DEFAULT_AUDIO_SINK@";
/// After you move the slider, how long to leave it alone.
const HANDS_OFF: Duration = Duration::from_millis(1500);

/// The default output and every output as (name, description).
type Outputs = (String, Vec<(String, String)>);

pub struct Sound {
    root: gtk::Box,
    mute: gtk::Button,
    scale: gtk::Scale,
    percent: gtk::Label,
    outputs: gtk::Box,
    shown_outputs: RefCell<Option<Outputs>>,
    /// The panel is moving the slider itself (don't send that back to wpctl).
    syncing: Cell<bool>,
    touched: Cell<Option<Instant>>,
    /// A volume change waiting to be sent (sent at most ~15 times a second).
    pending: RefCell<Option<glib::SourceId>>,
}

impl Sound {
    pub fn new() -> Rc<Self> {
        let root = vbox(6);
        root.pack_start(&label("Sound", &["np-title"]), false, false, 0);
        let (vol, muted) = volume();
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_draw_value(false);
        scale.set_value(f64::from(vol));
        let percent = label(&format!("{vol}%"), &[]);
        percent.set_xalign(1.0);
        percent.set_width_chars(4);

        let me = Rc::new_cyclic(|weak: &std::rc::Weak<Sound>| {
            let weak = weak.clone();
            let mute = icon_button(volume_icon(vol, muted), &[], "Mute", move || {
                if let Some(me) = weak.upgrade() {
                    me.toggle_mute();
                }
            });
            let line = hbox(6);
            line.pack_start(&mute, false, false, 0);
            line.pack_start(&scale, true, true, 0);
            line.pack_start(&percent, false, false, 0);
            root.pack_start(&line, false, false, 0);
            root.pack_start(&separator(), false, false, 0);
            root.pack_start(&label("Output", &["np-title"]), false, false, 0);
            let outputs = vbox(0);
            root.pack_start(&outputs, false, false, 0);
            Sound {
                root,
                mute,
                scale,
                percent,
                outputs,
                shown_outputs: RefCell::new(None),
                syncing: Cell::new(false),
                touched: Cell::new(None),
                pending: RefCell::new(None),
            }
        });
        me.fill_outputs(sinks());
        me.root.pack_start(&separator(), false, false, 0);
        let weak = Rc::downgrade(&me);
        me.root.pack_start(
            &row(icon::MIXER, "Open mixer", "", move || {
                sys::spawn("pavucontrol", &[]);
                if weak.upgrade().is_some() {
                    super::close();
                }
            }),
            false,
            false,
            0,
        );
        let weak = Rc::downgrade(&me);
        me.scale.connect_value_changed(move |s| {
            if let Some(me) = weak.upgrade() {
                let v = s.value().round() as i32;
                me.percent.set_text(&format!("{v}%"));
                me.set_volume(v);
            }
        });
        every(1000, &me, |me| me.update());
        me
    }

    fn hands_off(&self) -> bool {
        self.pending.borrow().is_some()
            || self.touched.get().is_some_and(|t| t.elapsed() < HANDS_OFF)
    }

    /// Follow changes made elsewhere.
    fn update(self: &Rc<Self>) {
        if self.hands_off() {
            return;
        }
        let weak = Rc::downgrade(self);
        background(
            || (volume(), sinks()),
            move |((vol, muted), outputs)| {
                let Some(me) = weak.upgrade() else {
                    return;
                };
                if me.hands_off() {
                    return;
                }
                me.syncing.set(true);
                if me.scale.value() as i32 != vol {
                    me.scale.set_value(f64::from(vol));
                }
                me.syncing.set(false);
                me.mute.set_label(volume_icon(vol, muted));
                if me.shown_outputs.borrow().as_ref() != Some(&outputs) {
                    me.fill_outputs(outputs);
                }
            },
        );
    }

    fn set_volume(self: &Rc<Self>, v: i32) {
        if self.syncing.get() {
            return;
        }
        self.touched.set(Some(Instant::now()));
        if let Some(id) = self.pending.borrow_mut().take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(60), move || {
            let Some(me) = weak.upgrade() else {
                return;
            };
            me.pending.borrow_mut().take();
            me.mute.set_label(volume_icon(v, false));
            background(
                move || {
                    let level = format!("{v}%");
                    out("wpctl", &["set-volume", SINK, &level]);
                    out("wpctl", &["set-mute", SINK, "0"]);
                },
                |_| (),
            );
        });
        *self.pending.borrow_mut() = Some(id);
    }

    fn toggle_mute(&self) {
        out("wpctl", &["set-mute", SINK, "toggle"]);
        let (vol, muted) = volume();
        self.mute.set_label(volume_icon(vol, muted));
    }

    fn fill_outputs(self: &Rc<Self>, outputs: Outputs) {
        *self.shown_outputs.borrow_mut() = Some(outputs.clone());
        let (default, sinks) = outputs;
        clear(&self.outputs);
        if sinks.is_empty() {
            self.outputs
                .pack_start(&label("No outputs found", &["np-dim"]), false, false, 0);
        }
        for (name, desc) in sinks {
            let low = format!("{desc}{name}").to_lowercase();
            let glyph = if ["headphone", "headset", "bluez"]
                .iter()
                .any(|w| low.contains(w))
            {
                icon::HEADPHONES
            } else {
                icon::SPEAKER
            };
            let current = name == default;
            let weak = Rc::downgrade(self);
            let b = row(
                glyph,
                &desc,
                if current { icon::CHECK } else { "" },
                move || {
                    if let Some(me) = weak.upgrade() {
                        me.set_output(&name);
                    }
                },
            );
            if current {
                b.style_context().add_class("np-active");
            }
            self.outputs.pack_start(&b, false, false, 0);
        }
        self.outputs.show_all();
    }

    /// Make it the default output and move what's playing over to it.
    fn set_output(self: &Rc<Self>, sink: &str) {
        out("pactl", &["set-default-sink", sink]);
        for line in out("pactl", &["list", "short", "sink-inputs"]).lines() {
            if let Some(id) = line.split('\t').next() {
                out("pactl", &["move-sink-input", id, sink]);
            }
        }
        self.fill_outputs(sinks());
    }
}

/// (percent, muted) of the default output.
fn volume() -> (i32, bool) {
    parse_volume(&out("wpctl", &["get-volume", SINK]))
}

/// "Volume: 0.42 [MUTED]" -> (42, true).
fn parse_volume(text: &str) -> (i32, bool) {
    let number = text
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|w| !w.is_empty() && w.chars().any(|c| c.is_ascii_digit()));
    let vol = number
        .and_then(|n| n.parse::<f64>().ok())
        .map_or(0, |v| (v * 100.0).round() as i32);
    (vol, text.contains("MUTED"))
}

fn volume_icon(vol: i32, muted: bool) -> &'static str {
    if muted {
        icon::MUTE
    } else if vol >= 66 {
        icon::VOLUME[2]
    } else if vol >= 33 {
        icon::VOLUME[1]
    } else {
        icon::VOLUME[0]
    }
}

fn sinks() -> Outputs {
    let default = out("pactl", &["get-default-sink"]);
    (default, parse_sinks(&out("pactl", &["list", "sinks"])))
}

fn parse_sinks(text: &str) -> Vec<(String, String)> {
    let mut sinks = Vec::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("\tName: ") {
            name = Some(n.to_string());
        } else if let (Some(d), Some(n)) = (line.strip_prefix("\tDescription: "), &name) {
            sinks.push((n.clone(), d.to_string()));
        }
    }
    sinks
}

impl Panel for Sound {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        320
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_text() {
        assert_eq!(parse_volume("Volume: 0.42"), (42, false));
        assert_eq!(parse_volume("Volume: 1.00 [MUTED]"), (100, true));
        assert_eq!(parse_volume(""), (0, false));
        assert_eq!(volume_icon(70, false), icon::VOLUME[2]);
        assert_eq!(volume_icon(70, true), icon::MUTE);
    }

    #[test]
    fn sink_list() {
        let text = "Sink #47\n\tState: RUNNING\n\tName: alsa_output.pci.analog-stereo\n\tDescription: Built-in Audio Analog Stereo\n\nSink #52\n\tName: bluez_output.AA_BB.1\n\tDescription: WH-1000XM4\n";
        assert_eq!(
            parse_sinks(text),
            [
                (
                    "alsa_output.pci.analog-stereo".into(),
                    "Built-in Audio Analog Stereo".into()
                ),
                ("bluez_output.AA_BB.1".into(), "WH-1000XM4".into())
            ]
        );
    }
}
