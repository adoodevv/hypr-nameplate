//! Finding every history file, reading only the ones that changed since last
//! time (a cache keyed by size and modification time), and summing up the week.

use crate::model::{Part, Session};
use crate::readers;
use crate::time::{self, civil_from_days, days_from_civil};
use nameplate::sys;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Bump when the cached parts change shape, so old caches are read again.
const CACHE_VERSION: u32 = 1;

pub struct Agent {
    pub key: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub home: PathBuf,
}

fn env_dir(var: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(fallback)
}

fn data_home() -> PathBuf {
    env_dir("XDG_DATA_HOME", sys::home().join(".local").join("share"))
}

/// The agents, in the order the panel shows them.
pub fn agents() -> Vec<Agent> {
    let home = sys::home();
    vec![
        Agent {
            key: "claude",
            name: "Claude Code",
            command: "claude",
            home: env_dir("CLAUDE_CONFIG_DIR", home.join(".claude")),
        },
        Agent {
            key: "grok",
            name: "Grok",
            command: "grok",
            home: env_dir("GROK_HOME", home.join(".grok")),
        },
        Agent {
            key: "muse",
            name: "Muse",
            command: "muse",
            home: data_home().join("muse"),
        },
        Agent {
            key: "opencode",
            name: "opencode",
            command: "opencode",
            home: data_home().join("opencode"),
        },
    ]
}

pub fn installed(a: &Agent) -> bool {
    sys::which(a.command).is_some() || a.home.exists()
}

#[derive(Clone, Copy)]
enum Reader {
    Claude,
    Grok,
    Muse,
    Opencode,
}

struct Source {
    agent: &'static str,
    path: PathBuf,
    stamp: Vec<i128>,
    reader: Reader,
}

/// [size, mtime in ns] for each file that exists.
fn stamp_of(paths: &[PathBuf], missing_as_zero: bool) -> Vec<i128> {
    let mut out = Vec::new();
    for p in paths {
        match std::fs::metadata(p) {
            Ok(m) => {
                let ns = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos() as i128);
                out.extend([m.len() as i128, ns]);
            }
            Err(_) if missing_as_zero => out.extend([0, 0]),
            Err(_) => {}
        }
    }
    out
}

/// Every file under `dir` (recursively) matching `keep`.
fn walk(dir: &Path, keep: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, keep, out),
            Ok(t) if t.is_file() && keep(&p) => out.push(p),
            _ => {}
        }
    }
}

fn sources(list: &[Agent]) -> Vec<Source> {
    let home_of = |k: &str| {
        list.iter()
            .find(|a| a.key == k)
            .map(|a| a.home.clone())
            .unwrap_or_default()
    };
    let mut out = Vec::new();

    let mut jsonl = Vec::new();
    walk(
        &home_of("claude").join("projects"),
        &|p| p.extension().is_some_and(|e| e == "jsonl"),
        &mut jsonl,
    );
    for path in jsonl {
        let stamp = stamp_of(std::slice::from_ref(&path), false);
        out.push(Source {
            agent: "claude",
            path,
            stamp,
            reader: Reader::Claude,
        });
    }

    // Grok: ~/.grok/sessions/<project>/<session>/
    if let Ok(projects) = std::fs::read_dir(home_of("grok").join("sessions")) {
        for project in projects.flatten().filter(|e| e.path().is_dir()) {
            for folder in std::fs::read_dir(project.path())
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
            {
                let stamp = stamp_of(
                    &[folder.join("summary.json"), folder.join("usage.json")],
                    true,
                );
                out.push(Source {
                    agent: "grok",
                    path: folder,
                    stamp,
                    reader: Reader::Grok,
                });
            }
        }
    }

    // Muse: every session.jsonl that isn't a helper's (read with its session) or a view copy.
    let sessions = home_of("muse").join("sessions");
    let mut logs = Vec::new();
    walk(
        &sessions,
        &|p| p.file_name().is_some_and(|n| n == "session.jsonl"),
        &mut logs,
    );
    for log in logs {
        let rel: Vec<String> = log
            .strip_prefix(&sessions)
            .map(|r| r.iter().map(|c| c.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        if rel
            .iter()
            .any(|p| p == "subagent" || p.starts_with(".msp-view"))
        {
            continue;
        }
        let folder = log.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut files = vec![log.clone()];
        walk(
            &folder.join("subagent"),
            &|p| p.file_name().is_some_and(|n| n == "session.jsonl"),
            &mut files,
        );
        let stamp = stamp_of(&files, false);
        out.push(Source {
            agent: "muse",
            path: folder,
            stamp,
            reader: Reader::Muse,
        });
    }

    for name in ["opencode.db", "opencode-v2.db"] {
        let db = home_of("opencode").join(name);
        if db.is_file() {
            let wal = PathBuf::from(format!("{}-wal", db.display()));
            let stamp = stamp_of(&[db.clone(), wal], false);
            out.push(Source {
                agent: "opencode",
                path: db,
                stamp,
                reader: Reader::Opencode,
            });
            break;
        }
    }
    out
}

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    agent: String,
    stamp: Vec<i128>,
    parts: Vec<Part>,
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    version: u32,
    files: HashMap<String, Entry>,
}

fn cache_path() -> PathBuf {
    sys::home()
        .join(".cache")
        .join("nameplate")
        .join("agents-rs.json")
}

/// Every source's parts, in source order: from the cache when unchanged,
/// read again otherwise.
fn collect(list: &[Agent]) -> Vec<Entry> {
    let old: Cache = std::fs::read(cache_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Cache>(&b).ok())
        .filter(|c| c.version == CACHE_VERSION)
        .unwrap_or_default();
    let muse_home = list
        .iter()
        .find(|a| a.key == "muse")
        .map(|a| a.home.clone())
        .unwrap_or_default();
    let mut files: HashMap<String, Entry> = HashMap::new();
    let mut ordered = Vec::new();
    for src in sources(list) {
        let key = src.path.to_string_lossy().into_owned();
        if let Some(known) = old.files.get(&key).filter(|e| e.stamp == src.stamp) {
            files.insert(key.clone(), known.clone());
            ordered.push(known.clone());
            continue;
        }
        let parts = match src.reader {
            Reader::Claude => readers::claude(&src.path).ok(),
            Reader::Grok => readers::grok(&src.path).ok(),
            Reader::Muse => readers::muse(&src.path, &muse_home).ok(),
            Reader::Opencode => readers::opencode(&src.path).ok(),
        };
        let Some(parts) = parts else { continue };
        let entry = Entry {
            agent: src.agent.to_string(),
            stamp: src.stamp,
            parts,
        };
        files.insert(key, entry.clone());
        ordered.push(entry);
    }
    if let Ok(text) = serde_json::to_string(&Cache {
        version: CACHE_VERSION,
        files,
    }) {
        let _ = sys::write_atomic(cache_path(), &text);
    }
    ordered
}

fn iso(z: i64) -> String {
    let (y, m, d) = civil_from_days(z);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The calendar week so far, Monday to today (it starts again from zero on Monday).
pub fn this_week() -> Vec<String> {
    let (y, m, d) = time::today();
    let today = days_from_civil(y, m, d);
    let weekday = (today + 3).rem_euclid(7); // 1970-01-01 was a Thursday; Monday = 0
    (today - weekday..=today).map(iso).collect()
}

fn session_json(s: &Session) -> Value {
    json!({"id": s.id, "cwd": s.cwd, "title": s.title, "last": s.last, "tokens": s.tokens})
}

/// `agents stats`: today's and this week's use, and recent sessions, per agent.
pub fn stats() -> Value {
    let list = agents();
    let files = collect(&list);
    let week = this_week();
    let today = week.last().cloned().unwrap_or_default();
    let mut out = Map::new();
    for a in list.iter().filter(|a| installed(a)) {
        let mut by_day: BTreeMap<&str, (u64, u64)> =
            week.iter().map(|d| (d.as_str(), (0, 0))).collect();
        let mut models: Vec<(String, u64)> = Vec::new();
        let mut sessions_today: HashSet<String> = HashSet::new();
        let mut sessions: Vec<&Session> = Vec::new();
        for part in files
            .iter()
            .filter(|e| e.agent == a.key)
            .flat_map(|e| e.parts.iter())
        {
            let s = &part.session;
            for (day, tokens, prompts) in &part.days {
                if let Some(slot) = by_day.get_mut(day.as_str()) {
                    slot.0 += tokens;
                    slot.1 += prompts;
                }
                if *day == today && (*tokens > 0 || *prompts > 0) && s.last != 0.0 {
                    sessions_today.insert(s.id.clone());
                }
            }
            for (day, model, tokens) in &part.models {
                if by_day.contains_key(day.as_str()) {
                    match models.iter_mut().find(|m| m.0 == *model) {
                        Some(m) => m.1 += tokens,
                        None => models.push((model.clone(), *tokens)),
                    }
                }
            }
            if s.last != 0.0 {
                sessions.push(s);
            }
        }
        // Stable, newest first, like Python's sort(key=-last).
        sessions.sort_by(|x, y| {
            y.last
                .partial_cmp(&x.last)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let top = models
            .iter()
            .fold(None::<&(String, u64)>, |best, m| match best {
                Some(b) if b.1 >= m.1 => Some(b),
                _ => Some(m),
            });
        let (t_tokens, t_prompts) = by_day.get(today.as_str()).copied().unwrap_or_default();
        out.insert(
            a.key.to_string(),
            json!({
                "name": a.name,
                "command": if sys::which(a.command).is_some() { a.command } else { "" },
                "today": {"tokens": t_tokens, "prompts": t_prompts, "sessions": sessions_today.len()},
                "week": week.iter().map(|d| {
                    let (t, p) = by_day[d.as_str()];
                    json!({"date": d, "tokens": t, "prompts": p})
                }).collect::<Vec<_>>(),
                "top_model": top.map(|m| m.0.as_str()).unwrap_or(""),
                "recent": sessions.iter().take(4).map(|s| session_json(s)).collect::<Vec<_>>(),
            }),
        );
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn week_starts_on_monday() {
        let week = this_week();
        assert!((1..=7).contains(&week.len()));
        let (y, m, d) = time::today();
        assert_eq!(week.last().unwrap(), &format!("{y:04}-{m:02}-{d:02}"));
        let first = &week[0];
        let z = days_from_civil(
            first[..4].parse().unwrap(),
            first[5..7].parse().unwrap(),
            first[8..].parse().unwrap(),
        );
        assert_eq!((z + 3).rem_euclid(7), 0, "{first} is a Monday");
    }
}
