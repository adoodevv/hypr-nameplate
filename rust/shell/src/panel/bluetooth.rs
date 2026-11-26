//! Bluetooth (the bar's Bluetooth icon): the power switch and the paired
//! devices, click one to connect or disconnect it. Pairing a new device is
//! left to GNOME's settings, which do it well.

use super::{
    background, clear, every, hbox, icon, label, out, row, run_text, separator, vbox, Panel,
};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// (address, name, connected)
type Device = (String, String, bool);
/// Powered, and the paired devices.
type State = (bool, Vec<Device>);

pub struct Bluetooth {
    root: gtk::Box,
    switch: gtk::Switch,
    syncing: Cell<bool>,
    status: gtk::Label,
    list: gtk::Box,
    shown: RefCell<Option<State>>,
    polling: Cell<bool>,
}

impl Bluetooth {
    pub fn new() -> Rc<Self> {
        let root = vbox(6);
        let head = hbox(8);
        head.pack_start(&label("Bluetooth", &["np-title"]), true, true, 0);
        let switch = gtk::Switch::new();
        switch.set_active(powered());
        head.pack_end(&switch, false, false, 0);
        root.pack_start(&head, false, false, 0);
        let status = label("", &["np-dim"]);
        status.set_line_wrap(true);
        status.set_max_width_chars(34);
        root.pack_start(&status, false, false, 0);
        let list = vbox(0);
        root.pack_start(&list, false, false, 0);
        root.pack_start(&separator(), false, false, 0);
        root.pack_start(
            &row(icon::PLUS, "Pair a new device", "", settings),
            false,
            false,
            0,
        );
        root.pack_start(
            &row(icon::COG, "Bluetooth settings", "", settings),
            false,
            false,
            0,
        );
        let me = Rc::new(Bluetooth {
            root,
            switch,
            syncing: Cell::new(false),
            status,
            list,
            shown: RefCell::new(None),
            polling: Cell::new(false),
        });
        let weak = Rc::downgrade(&me);
        me.switch.connect_active_notify(move |sw| {
            if let Some(me) = weak.upgrade() {
                if !me.syncing.get() {
                    me.toggle_power(sw.is_active());
                }
            }
        });
        me.load();
        every(3000, &me, |me| me.load());
        me
    }

    fn say(&self, text: &str, error: bool) {
        self.status.set_text(text);
        self.status.set_visible(!text.is_empty());
        let ctx = self.status.style_context();
        if error {
            ctx.add_class("np-error");
        } else {
            ctx.remove_class("np-error");
        }
    }

    fn toggle_power(self: &Rc<Self>, on: bool) {
        let weak = Rc::downgrade(self);
        background(
            move || {
                if on {
                    run_text("rfkill", &["unblock", "bluetooth"], Duration::from_secs(5));
                    std::thread::sleep(Duration::from_millis(500));
                }
                run_text(
                    "bluetoothctl",
                    &["power", if on { "on" } else { "off" }],
                    Duration::from_secs(10),
                )
                .0
            },
            move |ok| {
                if let Some(me) = weak.upgrade() {
                    me.say(if ok { "" } else { "Couldn't change Bluetooth." }, !ok);
                    me.load();
                }
            },
        );
    }

    /// Read the state in the background (bluetoothctl is slow), then redraw if it changed.
    fn load(self: &Rc<Self>) {
        if self.polling.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        background(state, move |state| {
            let Some(me) = weak.upgrade() else {
                return;
            };
            me.polling.set(false);
            if state.0 != me.switch.is_active() {
                me.syncing.set(true);
                me.switch.set_active(state.0);
                me.syncing.set(false);
            }
            if me.shown.borrow().as_ref() != Some(&state) && !me.status.text().ends_with('…') {
                me.show(state);
            }
        });
    }

    fn show(self: &Rc<Self>, state: State) {
        *self.shown.borrow_mut() = Some(state.clone());
        let (on, devices) = state;
        clear(&self.list);
        if !on {
            self.list
                .pack_start(&label("Bluetooth is off.", &["np-dim"]), false, false, 6);
        } else if devices.is_empty() {
            self.list.pack_start(
                &label("No paired devices yet.", &["np-dim"]),
                false,
                false,
                6,
            );
        }
        for (mac, name, connected) in devices {
            let glyph = if connected {
                icon::BLUETOOTH_CONNECTED
            } else {
                icon::BLUETOOTH
            };
            let weak = Rc::downgrade(self);
            let who = name.clone();
            let b = row(
                glyph,
                &name,
                if connected { "connected" } else { "" },
                move || {
                    if let Some(me) = weak.upgrade() {
                        me.toggle(&mac, &who, connected);
                    }
                },
            );
            if connected {
                b.style_context().add_class("np-active");
            }
            self.list.pack_start(&b, false, false, 0);
        }
        self.list.show_all();
    }

    fn toggle(self: &Rc<Self>, mac: &str, name: &str, connected: bool) {
        let verb = if connected {
            "Disconnecting "
        } else {
            "Connecting to "
        };
        self.say(&format!("{verb}{name}…"), false);
        let (mac, who) = (mac.to_string(), name.to_string());
        let weak = Rc::downgrade(self);
        background(
            move || {
                if connected {
                    run_text(
                        "bluetoothctl",
                        &["disconnect", &mac],
                        Duration::from_secs(15),
                    )
                    .0
                } else {
                    // bluetoothctl can exit 0 without connecting; trust its words.
                    let (_, text) = run_text(
                        "bluetoothctl",
                        &["--timeout", "15", "connect", &mac],
                        Duration::from_secs(20),
                    );
                    text.contains("Connection successful")
                }
            },
            move |ok| {
                if let Some(me) = weak.upgrade() {
                    let why = format!("Couldn't connect to {who}. Is it switched on and nearby?");
                    me.say(if ok { "" } else { &why }, !ok);
                    me.load();
                }
            },
        );
    }
}

fn powered() -> bool {
    out("bluetoothctl", &["show"]).contains("Powered: yes")
}

fn state() -> State {
    if !powered() {
        return (false, Vec::new());
    }
    let devices = out("bluetoothctl", &["devices", "Paired"])
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, ' ');
            let (kind, mac, name) = (parts.next()?, parts.next()?, parts.next()?);
            (kind == "Device").then(|| {
                let connected = out("bluetoothctl", &["info", mac]).contains("Connected: yes");
                (mac.to_string(), name.to_string(), connected)
            })
        })
        .collect();
    (true, devices)
}

/// GNOME's Bluetooth settings, which only open when they think GNOME is running.
fn settings() {
    use std::os::unix::process::CommandExt;
    let _ = std::process::Command::new("gnome-control-center")
        .arg("bluetooth")
        .env("XDG_CURRENT_DESKTOP", "GNOME")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .map(|mut c| std::thread::spawn(move || c.wait()));
    super::close();
}

impl Panel for Bluetooth {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn shown(self: Rc<Self>) {
        self.status.set_visible(!self.status.text().is_empty());
    }
}
