//! AI coding agents (the robot in the bar), laid out like Omarchy 4's Agents
//! panel: a header whose line cycles through what the week adds up to, then a
//! row per agent with its logo, today's use, a meter, and buttons to resume
//! or start one. The numbers come from `nameplate-agents stats`.

use super::{
    background, clear, every, hbox, icon, icon_button, label, run_text, separator, vbox, Panel,
};
use gtk::glib;
use gtk::prelude::*;
use nameplate::json::{self, Value};
use nameplate::sys;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

const ROBOT: &str = "\u{f16a3}";
const NEW_SESSION: &str = "\u{f07b7}";
const DOWNLOAD: &str = "\u{f01da}";
const NO_LOGO: &str = "\u{f06a9}";

/// Agents the + button offers to install, with their official install commands.
const INSTALL: [(&str, &str, &str); 2] = [
    (
        "claude",
        "Claude Code",
        "curl -fsSL https://claude.ai/install.sh | bash",
    ),
    (
        "opencode",
        "opencode",
        "curl -fsSL https://opencode.ai/install | bash",
    ),
];

pub struct Agents {
    root: gtk::Box,
    phrase: gtk::Label,
    add: gtk::Button,
    body: gtk::Box,
    phrases: RefCell<Vec<String>>,
    turn: Cell<usize>,
    /// The agents, in nameplate-agents' order; `None` when it couldn't read them.
    agents: RefCell<Option<Vec<(String, Value)>>>,
    adding: Cell<bool>,
}

impl Agents {
    pub fn new() -> Rc<Self> {
        let root = vbox(0);
        let head = hbox(14);
        head.style_context().add_class("np-agents-head");
        head.pack_start(&label(ROBOT, &["np-agents-mark"]), false, false, 0);
        let titles = vbox(2);
        titles.set_valign(gtk::Align::Center);
        titles.pack_start(&label("Agents", &["np-agents-title"]), false, false, 0);
        let phrase = label("READING YOUR HISTORY", &["np-agents-phrase"]);
        titles.pack_start(&phrase, false, false, 0);
        head.pack_start(&titles, true, true, 0);
        root.pack_start(&head, false, false, 0);
        root.pack_start(&separator(), false, false, 0);
        let body = vbox(0);
        root.pack_start(&body, false, false, 0);
        body.pack_start(
            &label("Reading your history…", &["np-dim"]),
            false,
            false,
            14,
        );

        let me = Rc::new_cyclic(|weak: &std::rc::Weak<Agents>| {
            let weak = weak.clone();
            let add = icon_button(icon::PLUS, &["np-agents-add"], "Add an agent", move || {
                if let Some(me) = weak.upgrade() {
                    me.adding.set(!me.adding.get());
                    let agents = me.agents.borrow().clone();
                    me.show(agents);
                }
            });
            add.set_valign(gtk::Align::Center);
            add.set_no_show_all(true);
            head.pack_end(&add, false, false, 0);
            Agents {
                root,
                phrase,
                add,
                body,
                phrases: RefCell::default(),
                turn: Cell::new(0),
                agents: RefCell::new(None),
                adding: Cell::new(false),
            }
        });
        every(4000, &me, |me| {
            let phrases = me.phrases.borrow();
            if !phrases.is_empty() {
                me.turn.set((me.turn.get() + 1) % phrases.len());
                me.phrase.set_text(&phrases[me.turn.get()]);
            }
        });
        let weak = Rc::downgrade(&me);
        background(
            || {
                let (ok, text) = run_text(
                    &sys::bin("nameplate-agents").to_string_lossy(),
                    &["stats"],
                    Duration::from_secs(120),
                );
                match json::parse(&text) {
                    Some(Value::Object(map)) if ok => Some(
                        map.iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                }
            },
            move |agents| {
                if let Some(me) = weak.upgrade() {
                    me.show(agents);
                }
            },
        );
        me
    }

    fn show(self: &Rc<Self>, agents: Option<Vec<(String, Value)>>) {
        *self.agents.borrow_mut() = agents.clone();
        clear(&self.body);
        match &agents {
            None => self.body.pack_start(
                &label("Couldn't read the agents' history.", &["np-dim"]),
                false,
                false,
                14,
            ),
            Some(list) if list.is_empty() => self.body.pack_start(
                &label("No AI coding agents yet. Add one with +.", &["np-dim"]),
                false,
                false,
                14,
            ),
            Some(list) => {
                let phrases = summary(list);
                self.turn.set(0);
                self.phrase.set_text(&phrases[0]);
                *self.phrases.borrow_mut() = phrases;
                for (i, (key, a)) in list.iter().enumerate() {
                    if i > 0 {
                        self.body.pack_start(&separator(), false, false, 0);
                    }
                    self.body
                        .pack_start(&self.agent_row(key, a), false, false, 0);
                }
            }
        }
        let missing = self.missing();
        if self.adding.get() && !missing.is_empty() {
            self.body.pack_start(&separator(), false, false, 0);
            self.body.pack_start(&add_rows(&missing), false, false, 0);
        }
        self.add.set_visible(!missing.is_empty());
        self.body.show_all();
    }

    fn agent_row(&self, key: &str, a: &Value) -> gtk::Box {
        let row = vbox(12);
        row.style_context().add_class("np-agent");
        let top = hbox(12);
        top.pack_start(&logo(key), false, false, 0);
        top.pack_start(
            &label(a.str_of("name"), &["np-agent-name"]),
            false,
            false,
            0,
        );
        let model = a.str_of("top_model");
        if !model.is_empty() {
            let m = label(&friendly_model(model), &["np-dim"]);
            m.set_ellipsize(gtk::pango::EllipsizeMode::End);
            m.set_max_width_chars(16);
            m.set_width_chars(10);
            top.pack_start(&m, false, false, 0);
        }
        let command = a.str_of("command").to_string();
        if !command.is_empty() {
            let c = command.clone();
            top.pack_end(
                &icon_button(NEW_SESSION, &["np-agent-btn"], "New session", move || {
                    terminal("", &c)
                }),
                false,
                false,
                0,
            );
            if let Some(last) = a
                .get("recent")
                .map(Value::as_array)
                .and_then(<[Value]>::first)
            {
                let place = Path::new(last.str_of("cwd").trim_end_matches('/'))
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let what = [last.str_of("title"), place.as_str(), "the last session"]
                    .into_iter()
                    .find(|s| !s.is_empty())
                    .unwrap_or_default()
                    .to_string();
                let (key, cwd, id) = (
                    key.to_string(),
                    last.str_of("cwd").to_string(),
                    last.str_of("id").to_string(),
                );
                top.pack_end(
                    &icon_button(
                        icon::PLAY,
                        &["np-agent-btn"],
                        &format!("Resume {what}"),
                        move || {
                            if key == "claude" {
                                terminal(
                                    &cwd,
                                    &format!("claude --resume {} || exec bash", shell_quote(&id)),
                                );
                            } else {
                                // The others: a new session in that project's folder.
                                terminal(&cwd, &format!("{command} || exec bash"));
                            }
                        },
                    ),
                    false,
                    false,
                    0,
                );
            }
        }
        row.pack_start(&top, false, false, 0);

        let today = a.get("today");
        let count = |k: &str| {
            today
                .and_then(|t| t.get(k))
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as i64
        };
        let (tokens, prompts, sessions) = (count("tokens"), count("prompts"), count("sessions"));
        let week: Vec<i64> = a
            .get("week")
            .map_or(&[][..], Value::as_array)
            .iter()
            .map(|d| d.get("tokens").and_then(Value::as_f64).unwrap_or(0.0) as i64)
            .collect();
        let facts = if tokens > 0 || prompts > 0 {
            vec![
                format!("{} tokens", short_number(tokens)),
                plural(prompts, "prompt"),
                plural(sessions, "session"),
            ]
        } else {
            let total: i64 = week.iter().sum();
            let mut f = vec!["Quiet today".to_string()];
            if total > 0 {
                f.push(format!("{} tokens this week", short_number(total)));
            }
            f
        };
        row.pack_start(&label(&facts.join("  ·  "), &["np-dim"]), false, false, 0);

        // Today against this agent's busiest day of the week.
        let peak = week.iter().copied().chain([1]).max().unwrap_or(1);
        let meter = hbox(14);
        meter.pack_start(&label("Today", &["np-dim"]), false, false, 0);
        let bar = gtk::ProgressBar::new();
        bar.style_context().add_class("np-meter");
        bar.set_fraction((tokens as f64 / peak as f64).min(1.0));
        bar.set_valign(gtk::Align::Center);
        meter.pack_start(&bar, true, true, 0);
        let pct = label(
            &format!("{}%", round_half_even(100.0 * tokens as f64 / peak as f64)),
            &["np-dim"],
        );
        pct.set_xalign(1.0);
        pct.set_width_chars(4);
        meter.pack_start(&pct, false, false, 0);
        meter.set_tooltip_text(Some("Today's tokens against the busiest day this week"));
        row.pack_start(&meter, false, false, 0);
        row
    }

    /// Agents to offer: not shown already and not on PATH.
    fn missing(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        let agents = self.agents.borrow();
        INSTALL
            .into_iter()
            .filter(|(k, _, _)| {
                !agents
                    .as_ref()
                    .is_some_and(|a| a.iter().any(|(key, _)| key == k))
                    && sys::which(k).is_none()
            })
            .collect()
    }
}

fn add_rows(missing: &[(&'static str, &'static str, &'static str)]) -> gtk::Box {
    let rows = vbox(2);
    rows.style_context().add_class("np-agent");
    rows.pack_start(
        &label("ADD AN AGENT", &["np-agents-phrase"]),
        false,
        false,
        4,
    );
    for &(key, name, command) in missing {
        let r = hbox(12);
        r.pack_start(&logo(key), false, false, 0);
        r.pack_start(&label(name, &["np-agent-name"]), true, true, 0);
        r.pack_end(
            &icon_button(
                DOWNLOAD,
                &["np-agent-btn"],
                "Install in a terminal",
                move || {
                    let script = format!(
                        "{command}; echo; read -n 1 -s -r -p \"Done. Press any key to close.\""
                    );
                    let title = format!("Install {name}");
                    sys::spawn(
                        "alacritty",
                        &[
                            "--class",
                            "nameplate-float",
                            "--title",
                            &title,
                            "-e",
                            "bash",
                            "-c",
                            &script,
                        ],
                    );
                    super::close();
                },
            ),
            false,
            false,
            0,
        );
        rows.pack_start(&r, false, false, 4);
    }
    rows
}

/// The agent's logo (from Omarchy), the light version on a light theme.
fn logo(key: &str) -> gtk::Widget {
    let light = gtk::Settings::default().is_some_and(|s| !s.is_gtk_application_prefer_dark_theme());
    let dir = sys::app_dir().join("agents");
    let names = if light {
        vec![format!("{key}-light"), key.to_string()]
    } else {
        vec![key.to_string()]
    };
    for name in names {
        let path = dir.join(format!("{name}.svg"));
        if path.exists() {
            if let Ok(pix) = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&path, 20, 20, true) {
                let img = gtk::Image::from_pixbuf(Some(&pix));
                img.set_size_request(22, 22);
                return img.upcast();
            }
        }
    }
    label(NO_LOGO, &["np-icon"]).upcast()
}

/// Run the agent in a terminal, in the session's folder. bash -i so ~/.bashrc
/// sets up PATH (npm, ~/.local/bin) the way your own terminal has it.
fn terminal(cwd: &str, command: &str) {
    let home = sys::home();
    let dir = if !cwd.is_empty() && Path::new(cwd).is_dir() {
        cwd.to_string()
    } else {
        home.to_string_lossy().into_owned()
    };
    sys::spawn(
        "alacritty",
        &["--working-directory", &dir, "-e", "bash", "-ic", command],
    );
    super::close();
}

/// What the week adds up to, across every agent (Omarchy's rotating line).
fn summary(agents: &[(String, Value)]) -> Vec<String> {
    let (mut week, mut today, mut prompts, mut sessions) = (0i64, 0i64, 0i64, 0i64);
    let mut by_day: Vec<(String, i64)> = Vec::new();
    let mut models: Vec<(String, i64)> = Vec::new();
    let num = |v: Option<&Value>| v.and_then(Value::as_f64).unwrap_or(0.0) as i64;
    let add = |list: &mut Vec<(String, i64)>, key: &str, n: i64| match list
        .iter_mut()
        .find(|(k, _)| k == key)
    {
        Some(e) => e.1 += n,
        None => list.push((key.to_string(), n)),
    };
    for (_, a) in agents {
        let days = a.get("week").map_or(&[][..], Value::as_array);
        let mut agent_week = 0;
        for d in days {
            let t = num(d.get("tokens"));
            week += t;
            agent_week += t;
            add(&mut by_day, d.str_of("date"), t);
        }
        let t = a.get("today");
        today += num(t.and_then(|t| t.get("tokens")));
        prompts += num(t.and_then(|t| t.get("prompts")));
        sessions += num(t.and_then(|t| t.get("sessions")));
        let model = a.str_of("top_model");
        if !model.is_empty() {
            add(&mut models, model, agent_week);
        }
    }
    // The first of equals wins, as Python's max() does.
    let most = |list: &[(String, i64)]| {
        list.iter()
            .fold(None::<&(String, i64)>, |best, e| match best {
                Some(b) if b.1 >= e.1 => Some(b),
                _ => Some(e),
            })
            .cloned()
    };
    let mut out = Vec::new();
    if week != 0 {
        out.push(format!("{} tokens this week", short_number(week)));
    }
    if today != 0 {
        out.push(format!("{} tokens today", short_number(today)));
    }
    if let Some((model, _)) = most(&models) {
        out.push(format!("Mostly {}", friendly_model(&model)));
    }
    if let Some((day, n)) = most(&by_day) {
        if n > 0 {
            out.push(format!("Busiest day: {}", weekday(&day)));
        }
    }
    if prompts != 0 {
        out.push(plural(prompts, "prompt") + " today");
    }
    if sessions != 0 {
        out.push(plural(sessions, "session") + " today");
    }
    if out.is_empty() {
        return vec!["NOTHING YET THIS WEEK".into()];
    }
    out.into_iter().map(|p| p.to_uppercase()).collect()
}

fn plural(n: i64, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// 1234 -> "1.2K", 3400000 -> "3.4M" (as Omarchy's panel shows token counts).
fn short_number(n: i64) -> String {
    let n_f = n as f64;
    if n >= 1_000_000_000 {
        format!("{:.1}B", n_f / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n_f / 1e6)
    } else if n >= 1000 {
        format!("{:.1}K", n_f / 1e3)
    } else {
        n.to_string()
    }
}

/// A model id made readable: `claude-opus-4-6` -> `Opus 4.6`,
/// `muse-spark-1.3-contributor` -> `Muse Spark 1.3 Contributor`.
fn friendly_model(id: &str) -> String {
    let mut name = id.strip_prefix("claude-").unwrap_or(id);
    // A trailing date: -20250514
    if let Some((head, tail)) = name.rsplit_once('-') {
        if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) {
            name = head;
        }
    }
    let (mut words, mut version): (Vec<String>, Vec<&str>) = (Vec::new(), Vec::new());
    for part in name.split('-').filter(|p| !p.is_empty()) {
        if part.starts_with(|c: char| c.is_ascii_digit()) {
            version.push(part);
            continue;
        }
        if !version.is_empty() {
            words.push(version.join("."));
            version.clear();
        }
        words.push(match part.to_lowercase().as_str() {
            "gpt" => "GPT".into(),
            "deepseek" => "DeepSeek".into(),
            _ => {
                let mut c = part.chars();
                c.next()
                    .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
            }
        });
    }
    if !version.is_empty() {
        words.push(version.join("."));
    }
    if words.is_empty() {
        id.to_string()
    } else {
        words.join(" ")
    }
}

fn weekday(date: &str) -> String {
    let mut p = date.split('-').map(|x| x.parse::<i32>().ok());
    if let (Some(Some(y)), Some(Some(m)), Some(Some(d))) = (p.next(), p.next(), p.next()) {
        if let Ok(t) = glib::DateTime::from_local(y, m, d, 12, 0, 0.0) {
            if let Ok(s) = t.format("%A") {
                return s.to_string();
            }
        }
    }
    date.to_string()
}

/// Python's round(): halves go to the even neighbour.
fn round_half_even(x: f64) -> i64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        (r - x.signum()) as i64
    } else {
        r as i64
    }
}

/// Python's shlex.quote: safe as one word in a shell command.
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if s.chars()
        .all(|c| c.is_alphanumeric() || "_@%+=:,./-".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

impl Panel for Agents {
    fn root(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn width(&self) -> i32 {
        380
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_numbers() {
        assert_eq!(friendly_model("claude-opus-4-6"), "Opus 4.6");
        assert_eq!(friendly_model("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(
            friendly_model("muse-spark-1.3-contributor"),
            "Muse Spark 1.3 Contributor"
        );
        assert_eq!(friendly_model("gpt-5"), "GPT 5");
        assert_eq!(friendly_model("--"), "--");
        assert_eq!(short_number(999), "999");
        assert_eq!(short_number(1250), "1.2K");
        assert_eq!(short_number(3_400_000), "3.4M");
        assert_eq!(round_half_even(2.5), 2);
        assert_eq!(round_half_even(3.5), 4);
        assert_eq!(round_half_even(66.6), 67);
        assert_eq!(shell_quote("abc-123"), "abc-123");
        assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
        assert_eq!(plural(1, "prompt"), "1 prompt");
    }

    #[test]
    fn week_summary() {
        let agents = json::parse(
            r#"{"claude": {"top_model": "claude-opus-4-6", "today": {"tokens": 1500, "prompts": 1, "sessions": 2},
                "week": [{"date": "2026-11-25", "tokens": 9000}, {"date": "2026-11-26", "tokens": 1500}]},
                "grok": {"top_model": "", "today": {"tokens": 0, "prompts": 0, "sessions": 0},
                "week": [{"date": "2026-11-25", "tokens": 100}]}}"#,
        )
        .unwrap();
        let Value::Object(map) = agents else { panic!() };
        let list: Vec<(String, Value)> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(
            summary(&list),
            [
                "10.6K TOKENS THIS WEEK",
                "1.5K TOKENS TODAY",
                "MOSTLY OPUS 4.6",
                "BUSIEST DAY: WEDNESDAY",
                "1 PROMPT TODAY",
                "2 SESSIONS TODAY"
            ]
        );
        assert_eq!(summary(&[]), ["NOTHING YET THIS WEEK"]);
    }
}
