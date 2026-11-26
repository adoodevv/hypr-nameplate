//! Wi-Fi (the bar's network icon), through NetworkManager's nmcli: the radio
//! switch, the ten strongest networks around, and connecting, with a password
//! line when a network needs one.
//!
//! Connecting tries a saved profile first. A new network gets a profile from
//! `nmcli dev wifi connect`; when that fails (a wrong password) the half-made
//! profile is removed again, but only one that is both new and for that network.

use super::{
    background, clear, every, hbox, icon, label, later, out, row, run_text, separator, vbox, Panel,
};
use gtk::prelude::*;
use nameplate::sys;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

const QUICK: Duration = Duration::from_secs(25);
const CONNECT: Duration = Duration::from_secs(45);

/// (signal bars 0-4, security, SSID)
type Network = (usize, String, String);
/// The connected SSID and the networks around, strongest first.
type Scan = (String, Vec<Network>);

pub struct Wifi {
    root: gtk::Box,
    dev: String,
    switch: gtk::Switch,
    /// The panel is moving the switch itself.
    syncing: Cell<bool>,
    status: gtk::Label,
    prompt: gtk::Box,
    list: gtk::Box,
    shown: RefCell<Option<Scan>>,
    polling: Cell<bool>,
}

/// How a saved profile went.
enum Saved {
    Ok,
    /// No saved profile for this network.
    New,
    /// It needs a (new) password.
    Secret(String),
    Fail(String),
}

impl Wifi {
    pub fn new() -> Rc<Self> {
        let dev = out("nmcli", &["-t", "-f", "DEVICE,TYPE", "dev"])
            .lines()
            .find(|l| l.ends_with(":wifi"))
            .and_then(|l| l.split(':').next())
            .unwrap_or("")
            .to_string();
        let root = vbox(6);
        let head = hbox(8);
        head.pack_start(&label("Wi-Fi", &["np-title"]), true, true, 0);
        let switch = gtk::Switch::new();
        switch.set_active(radio_on());
        head.pack_end(&switch, false, false, 0);
        root.pack_start(&head, false, false, 0);
        let status = label("", &["np-dim"]);
        status.set_line_wrap(true);
        status.set_max_width_chars(36);
        root.pack_start(&status, false, false, 0);
        let prompt = vbox(4); // the password line appears here when needed
        root.pack_start(&prompt, false, false, 0);
        let list = vbox(0);
        root.pack_start(&list, false, false, 0);
        root.pack_start(&separator(), false, false, 0);

        let me = Rc::new(Wifi {
            root,
            dev,
            switch,
            syncing: Cell::new(false),
            status,
            prompt,
            list,
            shown: RefCell::new(None),
            polling: Cell::new(false),
        });
        let weak = Rc::downgrade(&me);
        me.root.pack_start(
            &row(icon::REFRESH, "Scan again", "", move || {
                if let Some(me) = weak.upgrade() {
                    me.rescan();
                }
            }),
            false,
            false,
            0,
        );
        me.root.pack_start(
            &row(icon::COG, "Network settings", "", || {
                let wifi = sys::bin("wifi");
                let wifi = wifi.to_string_lossy();
                sys::spawn(
                    "alacritty",
                    &[
                        "--class",
                        "nameplate-float",
                        "--title",
                        "Network",
                        "-e",
                        &wifi,
                        "edit",
                    ],
                );
                super::close();
            }),
            false,
            false,
            0,
        );
        let weak = Rc::downgrade(&me);
        me.switch.connect_active_notify(move |sw| {
            if let Some(me) = weak.upgrade() {
                if !me.syncing.get() {
                    me.toggle_radio(sw.is_active());
                }
            }
        });
        me.load("auto");
        every(3000, &me, |me| me.poll());
        me
    }

    /// Mid-connection, or asking for a password: leave the list alone.
    fn busy(&self) -> bool {
        !self.prompt.children().is_empty() || self.status.text().ends_with('…')
    }

    /// Keep the panel current: the switch, what's connected, the networks around.
    fn poll(self: &Rc<Self>) {
        if self.polling.get() || self.busy() {
            return;
        }
        self.polling.set(true);
        let dev = self.dev.clone();
        let weak = Rc::downgrade(self);
        background(
            move || {
                let radio = radio_on();
                (radio, radio.then(|| scan(&dev, "no")))
            },
            move |(radio, nets)| {
                let Some(me) = weak.upgrade() else {
                    return;
                };
                me.polling.set(false);
                if radio != me.switch.is_active() {
                    me.syncing.set(true);
                    me.switch.set_active(radio);
                    me.syncing.set(false);
                    if !radio {
                        *me.shown.borrow_mut() = None;
                        me.load("auto");
                        return;
                    }
                }
                if let Some(nets) = nets {
                    if top(Some(&nets)) != top(me.shown.borrow().as_ref()) && !me.busy() {
                        me.show_networks(nets);
                    }
                }
            },
        );
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

    fn toggle_radio(self: &Rc<Self>, on: bool) {
        let state = if on { "on" } else { "off" };
        self.say(&format!("Turning Wi-Fi {state}…"), false);
        let weak = Rc::downgrade(self);
        background(
            move || run_text("nmcli", &["radio", "wifi", state], QUICK),
            move |(ok, text)| {
                let Some(me) = weak.upgrade() else {
                    return;
                };
                me.say(if ok { "" } else { &text }, !ok);
                if on {
                    // Networks take a moment to show up once the radio is on.
                    later(2500, &me, |me| me.load("auto"));
                } else {
                    me.load("auto");
                }
            },
        );
    }

    fn load(self: &Rc<Self>, rescan: &'static str) {
        if !radio_on() {
            clear(&self.list);
            self.list.pack_start(
                &label(
                    "Wi-Fi is off. Use the switch above to turn it on.",
                    &["np-dim"],
                ),
                false,
                false,
                6,
            );
            self.list.show_all();
            return;
        }
        if self.list.children().is_empty() {
            self.list.pack_start(
                &label("Looking for networks…", &["np-dim"]),
                false,
                false,
                6,
            );
            self.list.show_all();
        }
        let dev = self.dev.clone();
        let weak = Rc::downgrade(self);
        background(
            move || scan(&dev, rescan),
            move |result| {
                if let Some(me) = weak.upgrade() {
                    me.show_networks(result);
                }
            },
        );
    }

    fn show_networks(self: &Rc<Self>, result: Scan) {
        *self.shown.borrow_mut() = Some(result.clone());
        let (current, nets) = result;
        clear(&self.list);
        if !current.is_empty() {
            let weak = Rc::downgrade(self);
            self.list.pack_start(
                &row(
                    icon::UNLINK,
                    &format!("Disconnect from {current}"),
                    "",
                    move || {
                        if let Some(me) = weak.upgrade() {
                            me.disconnect();
                        }
                    },
                ),
                false,
                false,
                0,
            );
        }
        if nets.is_empty() {
            self.list
                .pack_start(&label("No networks found.", &["np-dim"]), false, false, 6);
        }
        for (level, security, ssid) in nets.into_iter().take(10) {
            // the 10 strongest; "Scan again" refreshes
            let secure = !security.is_empty() && security != "--";
            let here = ssid == current;
            let mut right = if here {
                format!("{} ", icon::CHECK)
            } else {
                String::new()
            };
            right.push_str(if secure { icon::LOCK } else { " " });
            let weak = Rc::downgrade(self);
            let (name, now) = (ssid.clone(), current.clone());
            let b = row(icon::WIFI[level], &ssid, &right, move || {
                if let Some(me) = weak.upgrade() {
                    me.choose(&name, secure, &now);
                }
            });
            if here {
                b.style_context().add_class("np-active");
            }
            self.list.pack_start(&b, false, false, 0);
        }
        self.list.show_all();
    }

    fn rescan(self: &Rc<Self>) {
        self.say("Scanning…", false);
        let dev = self.dev.clone();
        let weak = Rc::downgrade(self);
        background(
            move || rescan_now(&dev),
            move |_| {
                if let Some(me) = weak.upgrade() {
                    later(2500, &me, |me| {
                        me.say("", false);
                        me.load("no");
                    });
                }
            },
        );
    }

    fn disconnect(self: &Rc<Self>) {
        let active = out(
            "nmcli",
            &[
                "-t",
                "-e",
                "no",
                "-f",
                "NAME,TYPE",
                "con",
                "show",
                "--active",
            ],
        )
        .lines()
        .find_map(|l| {
            let (name, kind) = l.rsplit_once(':')?;
            (kind == "802-11-wireless").then(|| name.to_string())
        });
        let Some(active) = active else {
            return;
        };
        self.say("Disconnecting…", false);
        let weak = Rc::downgrade(self);
        background(
            move || run_text("nmcli", &["con", "down", "id", &active], QUICK),
            move |(ok, text)| {
                if let Some(me) = weak.upgrade() {
                    me.say(if ok { "" } else { &text }, !ok);
                    me.load("no");
                }
            },
        );
    }

    // -- connecting -------------------------------------------------------

    fn choose(self: &Rc<Self>, ssid: &str, secure: bool, current: &str) {
        if ssid == current {
            self.say(&format!("Already connected to {ssid}."), false);
            return;
        }
        clear(&self.prompt);
        self.say(&format!("Connecting to {ssid}…"), false);
        let (dev, name) = (self.dev.clone(), ssid.to_string());
        let weak = Rc::downgrade(self);
        let ssid = ssid.to_string();
        background(
            move || connect_saved(&dev, &name),
            move |saved| {
                if let Some(me) = weak.upgrade() {
                    me.after_saved(&ssid, secure, saved);
                }
            },
        );
    }

    fn after_saved(self: &Rc<Self>, ssid: &str, secure: bool, saved: (Saved, Option<String>)) {
        let (state, uuid) = saved;
        match state {
            Saved::Ok => self.connected(ssid),
            Saved::New if secure => {
                let name = ssid.to_string();
                self.ask_password(ssid, move |me, pw| me.connect_new(&name, Some(pw)));
            }
            Saved::New => self.connect_new(ssid, None),
            Saved::Secret(_) if secure && uuid.is_some() => {
                let (name, uuid) = (ssid.to_string(), uuid.unwrap_or_default());
                self.ask_password(ssid, move |me, pw| me.retry_saved(&name, &uuid, pw));
            }
            Saved::Secret(text) | Saved::Fail(text) => self.failed(ssid, &text),
        }
    }

    fn retry_saved(self: &Rc<Self>, ssid: &str, uuid: &str, password: String) {
        self.say(&format!("Connecting to {ssid}…"), false);
        let (dev, uuid, name) = (self.dev.clone(), uuid.to_string(), ssid.to_string());
        let weak = Rc::downgrade(self);
        background(
            move || up(&dev, &uuid, Some(&password)),
            move |(ok, text)| {
                if let Some(me) = weak.upgrade() {
                    if ok {
                        me.connected(&name);
                    } else {
                        me.failed(&name, &text);
                    }
                }
            },
        );
    }

    fn connect_new(self: &Rc<Self>, ssid: &str, password: Option<String>) {
        self.say(&format!("Connecting to {ssid}…"), false);
        let (dev, name) = (self.dev.clone(), ssid.to_string());
        let weak = Rc::downgrade(self);
        background(move || connect_new_now(&dev, &name, password.as_deref()), {
            let ssid = ssid.to_string();
            move |(ok, text): (bool, String)| {
                if let Some(me) = weak.upgrade() {
                    if ok {
                        me.connected(&ssid);
                    } else {
                        me.failed(&ssid, &text);
                    }
                }
            }
        });
    }

    fn ask_password(self: &Rc<Self>, ssid: &str, then: impl Fn(&Rc<Self>, String) + 'static) {
        clear(&self.prompt);
        self.say(&format!("Password for {ssid}:"), false);
        let entry = gtk::Entry::new();
        entry.set_visibility(false);
        entry.set_input_purpose(gtk::InputPurpose::Password);
        entry.set_placeholder_text(Some("Password"));
        let line = hbox(6);
        line.pack_start(&entry, true, true, 0);
        let go = gtk::Button::with_label("Connect");
        line.pack_start(&go, false, false, 0);
        self.prompt.pack_start(&line, false, false, 0);

        let then = Rc::new(then);
        let submit = {
            let (weak, entry) = (Rc::downgrade(self), entry.downgrade());
            move || {
                let (Some(me), Some(entry)) = (weak.upgrade(), entry.upgrade()) else {
                    return;
                };
                let pw = entry.text().to_string();
                if !pw.is_empty() {
                    clear(&me.prompt);
                    then(&me, pw);
                }
            }
        };
        let submit = Rc::new(submit);
        let s = submit.clone();
        entry.connect_activate(move |_| s());
        go.connect_clicked(move |_| submit());
        self.prompt.show_all();
        entry.grab_focus();
    }

    fn connected(self: &Rc<Self>, ssid: &str) {
        clear(&self.prompt);
        self.say(&format!("{}  Connected to {ssid}", icon::CHECK), false);
        sys::spawn(
            "notify-send",
            &["-a", "Wi-Fi", &format!("Connected to {ssid}")],
        );
        self.load("no");
    }

    fn failed(self: &Rc<Self>, ssid: &str, text: &str) {
        self.say(
            &format!("Couldn't connect to {ssid}. {}", explain(ssid, text)),
            true,
        );
        self.load("no");
    }
}

fn radio_on() -> bool {
    out("nmcli", &["radio", "wifi"]) == "enabled"
}

/// The first ten networks, for telling whether the list changed enough to redraw.
fn top(scan: Option<&Scan>) -> Option<(String, Vec<Network>)> {
    scan.map(|(current, nets)| (current.clone(), nets.iter().take(10).cloned().collect()))
}

fn ifname(dev: &str) -> Vec<&str> {
    if dev.is_empty() {
        Vec::new()
    } else {
        vec!["ifname", dev]
    }
}

fn rescan_now(dev: &str) {
    let mut args = vec!["dev", "wifi", "rescan"];
    args.extend(ifname(dev));
    run_text("nmcli", &args, Duration::from_secs(10));
}

fn scan(_dev: &str, rescan: &str) -> Scan {
    let current = out(
        "nmcli",
        &[
            "-t",
            "-e",
            "no",
            "-f",
            "ACTIVE,SSID",
            "dev",
            "wifi",
            "list",
            "--rescan",
            "no",
        ],
    )
    .lines()
    .filter_map(|l| l.strip_prefix("yes:"))
    .next_back()
    .unwrap_or("")
    .to_string();
    let list = match super::run(
        "nmcli",
        &[
            "-t",
            "-e",
            "no",
            "-f",
            "SIGNAL,SECURITY,SSID",
            "dev",
            "wifi",
            "list",
            "--rescan",
            rescan,
        ],
        None,
        Duration::from_secs(40),
    ) {
        super::Ran::Ok(o) => String::from_utf8_lossy(&o).into_owned(),
        _ => String::new(),
    };
    (current, parse_networks(&list))
}

/// nmcli's "SIGNAL:SECURITY:SSID" lines: each network once, at its strongest,
/// by bars and then name, so small signal changes don't reshuffle the list.
fn parse_networks(text: &str) -> Vec<Network> {
    let mut parsed: Vec<(i32, &str, &str)> = text
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, ':');
            let (sig, sec, ssid) = (parts.next()?, parts.next()?, parts.next()?);
            let sig = if sig.is_empty() { 0 } else { sig.parse().ok()? };
            (!ssid.is_empty()).then_some((sig, sec, ssid))
        })
        .collect();
    parsed.sort_by_key(|p| -p.0);
    let mut seen = HashSet::new();
    let mut nets: Vec<Network> = parsed
        .into_iter()
        .filter(|(_, _, ssid)| seen.insert(*ssid))
        .map(|(sig, sec, ssid)| (bars(sig), sec.to_string(), ssid.to_string()))
        .collect();
    nets.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.2.to_lowercase().cmp(&b.2.to_lowercase()))
    });
    nets
}

fn bars(signal: i32) -> usize {
    match signal {
        s if s >= 80 => 4,
        s if s >= 60 => 3,
        s if s >= 40 => 2,
        s if s >= 20 => 1,
        _ => 0,
    }
}

/// A usable saved profile for this network, skipping copies tied to another
/// network card and hotspot profiles this laptop broadcasts itself.
fn saved_for(dev: &str, ssid: &str) -> Option<String> {
    for l in out("nmcli", &["-t", "-f", "UUID,TYPE", "con", "show"]).lines() {
        let Some((uuid, kind)) = l.split_once(':') else {
            continue;
        };
        if kind != "802-11-wireless" {
            continue;
        }
        let fields = out(
            "nmcli",
            &[
                "-g",
                "802-11-wireless.ssid,connection.interface-name,802-11-wireless.mode",
                "con",
                "show",
                "uuid",
                uuid,
            ],
        );
        let mut f = fields.lines();
        let (ssid_, iface, mode) = (
            f.next().unwrap_or(""),
            f.next().unwrap_or(""),
            f.next().unwrap_or(""),
        );
        if ssid_ != ssid || mode == "ap" || (!iface.is_empty() && !dev.is_empty() && iface != dev) {
            continue;
        }
        return Some(uuid.to_string());
    }
    None
}

/// `nmcli con up`, with a password given through a private file rather than
/// on the command line (where other users could see it in the process list).
fn up(dev: &str, uuid: &str, password: Option<&str>) -> (bool, String) {
    let mut args = vec!["con", "up", "uuid", uuid];
    args.extend(ifname(dev));
    let Some(password) = password else {
        return run_text("nmcli", &args, CONNECT);
    };
    let file = sys::runtime_dir().join(format!("nameplate-wifi-{}.psk", std::process::id()));
    let written = {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let _ = std::fs::remove_file(&file);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .and_then(|mut f| writeln!(f, "802-11-wireless-security.psk:{password}"))
    };
    if let Err(e) = written {
        return (false, e.to_string());
    }
    let path = file.to_string_lossy().into_owned();
    args.extend(["passwd-file", path.as_str()]);
    let result = run_text("nmcli", &args, CONNECT);
    let _ = std::fs::remove_file(&file);
    result
}

fn connect_saved(dev: &str, ssid: &str) -> (Saved, Option<String>) {
    if !dev.is_empty() {
        run_text("nmcli", &["dev", "set", dev, "autoconnect", "yes"], QUICK);
    }
    let Some(uuid) = saved_for(dev, ssid) else {
        return (Saved::New, None);
    };
    let (ok, mut text) = up(dev, &uuid, None);
    if ok {
        return (Saved::Ok, Some(uuid));
    }
    if !text.contains("ecret") {
        // The network may have dropped out of the last scan (phone hotspots
        // sleep while nobody's connected): scan again and retry once.
        rescan_now(dev);
        std::thread::sleep(Duration::from_secs(4));
        let (ok, again) = up(dev, &uuid, None);
        if ok {
            return (Saved::Ok, Some(uuid));
        }
        text = again;
    }
    let state = if text.contains("ecret") {
        Saved::Secret(text)
    } else {
        Saved::Fail(text)
    };
    (state, Some(uuid))
}

fn connection_uuids() -> Option<HashSet<String>> {
    let (ok, text) = run_text("nmcli", &["-t", "-f", "UUID", "con", "show"], QUICK);
    ok.then(|| text.lines().map(String::from).collect())
}

fn connect_new_now(dev: &str, ssid: &str, password: Option<&str>) -> (bool, String) {
    let before = connection_uuids();
    let mut args = vec!["dev", "wifi", "connect", ssid];
    if let Some(pw) = password {
        args.extend(["password", pw]);
    }
    args.extend(ifname(dev));
    let (ok, text) = run_text("nmcli", &args, CONNECT);
    if !ok {
        if let (Some(before), Some(after)) = (before, connection_uuids()) {
            // Remove the half-made profile from a wrong password: only ones new
            // since a moment ago AND for this network, never anything else.
            for uuid in after.difference(&before) {
                if out(
                    "nmcli",
                    &["-g", "802-11-wireless.ssid", "con", "show", "uuid", uuid],
                ) == ssid
                {
                    run_text("nmcli", &["con", "delete", "uuid", uuid], QUICK);
                }
            }
        }
    }
    (ok, text)
}

/// nmcli's error in words for people.
fn explain(ssid: &str, text: &str) -> String {
    let text = text.rsplit("Error: ").next().unwrap_or(text).trim();
    if text.contains("not found") || text.contains("No suitable device") {
        format!("{ssid} isn't in range right now. If it's a phone hotspot, open the hotspot screen on the phone and try again.")
    } else if text.contains("ecret") || text.contains("802-1X") {
        "Wrong password, or the network needs a sign-in this panel can't do (use Network settings)."
            .into()
    } else {
        text.to_string()
    }
}

impl Panel for Wifi {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        320
    }
    fn shown(self: Rc<Self>) {
        self.status.set_visible(!self.status.text().is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_list() {
        let text = "54:WPA2:Home\n82:WPA2:Home\n70::Cafe Free\n30:WPA1 WPA2:alpha\n30:WPA2:Beta\n:--:\nbad:x:y\n";
        assert_eq!(
            parse_networks(text),
            [
                (4, "WPA2".into(), "Home".into()),
                (3, "".into(), "Cafe Free".into()),
                (1, "WPA1 WPA2".into(), "alpha".into()),
                (1, "WPA2".into(), "Beta".into()),
            ]
        );
    }

    #[test]
    fn errors_in_words() {
        assert!(explain(
            "Phone",
            "Error: No suitable device found (access point not found)"
        )
        .contains("isn't in range"));
        assert!(explain(
            "Home",
            "Error: Connection activation failed: Secrets were required"
        )
        .starts_with("Wrong password"));
        assert_eq!(explain("Home", "Error: Something else."), "Something else.");
    }
}
