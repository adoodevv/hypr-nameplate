//! One reader per agent, each turning that agent's own local history into
//! [`Part`]s. Formats and counting rules match the earlier Python version
//! exactly.

use crate::model::{chars, first, number, text, truthy, Part};
use crate::time::{self, local_day};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

fn obj(v: Option<&Value>) -> Option<&Map<String, Value>> {
    v.and_then(Value::as_object)
}

/// Lines of a file as text, invalid UTF-8 replaced rather than fatal.
fn lines(path: &Path) -> std::io::Result<impl Iterator<Item = String>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut buf = Vec::new();
    Ok(std::iter::from_fn(move || {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(String::from_utf8_lossy(&buf).into_owned()),
        }
    }))
}

fn read_json(path: &Path) -> Value {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}

// ------------------------------------------------------------- Claude Code --

/// What you typed, for a user line that's a real prompt (not a tool result or
/// one of Claude Code's own notes).
fn prompt_text(message: Option<&Map<String, Value>>) -> Option<String> {
    let content = message?.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            if blocks
                .iter()
                .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            {
                return None;
            }
            blocks
                .iter()
                .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .and_then(|b| b.get("text").and_then(Value::as_str))
                .unwrap_or("")
                .to_string()
        }
        _ => return None,
    };
    let text = text.trim();
    let notes = [
        "<command-",
        "<local-command",
        "Caveat:",
        "[Request interrupted",
    ];
    if text.is_empty() || notes.iter().any(|n| text.starts_with(n)) {
        return None;
    }
    Some(text.to_string())
}

pub fn claude(path: &Path) -> std::io::Result<Vec<Part>> {
    let id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut part = Part::new(&id, "");
    let mut seen: HashSet<Option<String>> = HashSet::new();
    for line in lines(path)? {
        if !line.contains("\"timestamp\"") {
            continue;
        }
        let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some((day, moment)) = entry.get("timestamp").and_then(local_day) else {
            continue;
        };
        if let Some(Value::String(cwd)) = entry.get("cwd") {
            if !cwd.is_empty() {
                part.session.cwd = cwd.clone();
            }
        }
        part.see(moment);
        let message = obj(entry.get("message"));
        match entry.get("type").and_then(Value::as_str) {
            Some("user") if !truthy(entry.get("isMeta")) && !truthy(entry.get("isSidechain")) => {
                if let Some(text) = prompt_text(message) {
                    part.day_mut(&day).2 += 1;
                    if part.session.title.is_empty() {
                        part.session.title =
                            chars(&text.split_whitespace().collect::<Vec<_>>().join(" "), 80);
                    }
                }
            }
            Some("assistant") => {
                let empty = Map::new();
                let message = message.unwrap_or(&empty);
                // One reply is written as several lines (one per content block): count it once.
                let key = first(&[message.get("id"), entry.get("requestId"), entry.get("uuid")])
                    .map(|v| v.to_string());
                let Some(usage) = obj(message.get("usage")) else {
                    continue;
                };
                if !seen.insert(key) {
                    continue;
                }
                let tokens: u64 = [
                    "input_tokens",
                    "output_tokens",
                    "cache_read_input_tokens",
                    "cache_creation_input_tokens",
                ]
                .iter()
                .map(|k| number(usage.get(*k)))
                .sum();
                let model = text(message.get("model"));
                if tokens == 0 || model.is_empty() || model == "<synthetic>" {
                    continue;
                }
                part.day_mut(&day).1 += tokens;
                part.add_model(&day, &model, tokens);
                part.session.tokens += tokens;
            }
            _ => {}
        }
    }
    Ok(vec![part])
}

// -------------------------------------------------------------------- Grok --

pub fn grok(folder: &Path) -> std::io::Result<Vec<Part>> {
    let id = folder
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut part = Part::new(&id, "");
    let summary = read_json(&folder.join("summary.json"));
    let summary = summary.as_object().cloned().unwrap_or_default();
    let subagent = text(summary.get("session_kind")).starts_with("subagent");
    part.session.title = chars(
        &text(first(&[summary.get("title"), summary.get("name")])),
        80,
    );
    part.session.cwd = text(first(&[
        summary.get("cwd"),
        summary.get("working_directory"),
    ]));
    part.session.last = summary
        .get("last_active_at")
        .and_then(local_day)
        .map_or(0.0, |(_, t)| t);
    let usage = read_json(&folder.join("usage.json"));
    for turn in usage
        .get("turns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(turn) = turn.as_object() else {
            continue;
        };
        let Some((day, moment)) = turn.get("endedAt").and_then(local_day) else {
            continue;
        };
        part.see(moment);
        if !subagent {
            part.day_mut(&day).2 += 1;
        }
        for (model, b) in obj(turn.get("modelUsage")).into_iter().flatten() {
            let Some(b) = b.as_object() else { continue };
            // Grok's inputTokens already include the cached part
            let tokens = number(b.get("inputTokens"))
                + number(b.get("outputTokens"))
                + number(b.get("cacheCreationTokens"));
            part.day_mut(&day).1 += tokens;
            part.add_model(&day, model, tokens);
            part.session.tokens += tokens;
        }
    }
    if subagent {
        part.session.last = 0.0; // a helper isn't one of your sessions
    }
    Ok(vec![part])
}

// -------------------------------------------------------------------- Muse --

/// An event's payload, its `recorded_at`, and its id.
type MuseEvent = (Map<String, Value>, Value, Option<Value>);

/// Every event in a Muse log, including those wrapped in retained frames.
fn muse_events(path: &Path) -> std::io::Result<Vec<MuseEvent>> {
    let mut out = Vec::new();
    for raw in lines(path)? {
        if !raw.contains("usage") && !raw.contains("model_id") {
            continue;
        }
        let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let mut found = vec![Value::Object(entry.clone())];
        for child in entry
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(Value::String(rec)) = child.get("record_json") {
                if let Ok(v) = serde_json::from_str(rec) {
                    found.push(v);
                }
            }
        }
        for event in found {
            if let Some(Value::Object(payload)) = event.get("payload") {
                out.push((
                    payload.clone(),
                    event.get("recorded_at").cloned().unwrap_or(Value::Null),
                    event.get("id").cloned(),
                ));
            }
        }
    }
    Ok(out)
}

fn files_named(dir: &Path, name: &str, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            files_named(&p, name, out);
        } else if p.file_name().is_some_and(|n| n == name) {
            out.push(p);
        }
    }
}

pub fn muse(folder: &Path, muse_home: &Path) -> std::io::Result<Vec<Part>> {
    let id = folder
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut part = Part::new(&id, "Muse session");
    let mut seen: HashSet<Option<String>> = HashSet::new();
    let mut paths = vec![folder.join("session.jsonl")];
    let mut helpers = Vec::new();
    files_named(&folder.join("subagent"), "session.jsonl", &mut helpers);
    helpers.sort();
    paths.extend(helpers);
    for path in paths {
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_file() => {}
            _ => continue,
        }
        let mut model = String::new();
        for (payload, recorded, event_id) in muse_events(&path)? {
            let record = obj(payload.get("record"));
            let kind = payload.get("kind").and_then(Value::as_str);
            if matches!(kind, Some("metadata" | "run_model")) {
                let m = text(record.and_then(|r| r.get("model_id")));
                if !m.is_empty() {
                    model = m;
                }
                continue;
            }
            let empty = Map::new();
            let event = obj(payload.get("event")).unwrap_or(&empty);
            let kind_ok =
                matches!(payload.get("kind"), None | Some(Value::Null)) || kind == Some("run");
            if !kind_ok || event.get("kind").and_then(Value::as_str) != Some("model_completed") {
                continue;
            }
            let key = event_id
                .as_ref()
                .filter(|v| truthy(Some(v)))
                .map(|v| v.to_string());
            if key.is_some() && seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            let usage = obj(event.get("usage"));
            // cached tokens are part of input_tokens
            let tokens = number(usage.and_then(|u| u.get("input_tokens")))
                + number(usage.and_then(|u| u.get("output_tokens")));
            let when = local_day(&recorded);
            if let Some((_, moment)) = &when {
                part.see(*moment);
            }
            let event_model = text(event.get("model"));
            let m = if event_model.is_empty() {
                model.clone()
            } else {
                event_model
            };
            part.add(when.as_ref().map(|(d, _)| d.as_str()), &m, tokens, 0);
        }
    }
    // Muse's own index has the real prompt counts.
    let index = muse_home.join("session-index.db");
    if let Ok(db) = Connection::open_with_flags(&index, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        let _ = db.busy_timeout(std::time::Duration::from_secs(2));
        if let Ok(mut stmt) =
            db.prepare("SELECT created_at_us, prompt_count FROM sessions WHERE session_id = ?1")
        {
            if let Ok(rows) = stmt.query_map([&id], |r| {
                Ok((sql_json(r.get_ref(0)?), sql_json(r.get_ref(1)?)))
            }) {
                for (created, prompts) in rows.flatten() {
                    let day = local_day(&created).map(|(d, _)| d);
                    part.add(day.as_deref(), "", 0, number(Some(&prompts)));
                }
            }
        }
    }
    Ok(vec![part])
}

/// An SQLite cell as JSON, so the shared number/time helpers apply.
fn sql_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(_) => Value::Null,
    }
}

// ---------------------------------------------------------------- opencode --

/// The model id without provider prefix or ':variant' suffix, as Omarchy shows it.
pub fn opencode_model_name(raw: &str) -> String {
    let last = raw.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let value = last.split(':').next().unwrap_or("");
    match value.get(..3) {
        Some(p) if p.eq_ignore_ascii_case("glm") => format!("GLM{}", &value[3..]),
        _ => value.to_string(),
    }
}

/// input + output (+ reasoning, which opencode keeps out of output) + cache read + write.
fn opencode_tokens(entry: &Map<String, Value>) -> u64 {
    let Some(t) = obj(entry.get("tokens")) else {
        return 0;
    };
    let cache = obj(t.get("cache"));
    number(t.get("input"))
        + number(t.get("output"))
        + number(t.get("reasoning"))
        + number(cache.and_then(|c| c.get("read")))
        + number(cache.and_then(|c| c.get("write")))
}

struct Event {
    order: u64,
    user: bool,
    day: String,
    moment: f64,
    model: String,
    tokens: u64,
}

fn columns(db: &Connection, table: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Ok(mut stmt) = db.prepare(&format!("PRAGMA table_info({table})")) {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(1)) {
            out.extend(rows.flatten());
        }
    }
    out
}

/// Every row of a query as JSON cells.
fn rows(db: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let Ok(mut stmt) = db.prepare(sql) else {
        return Vec::new();
    };
    let n = stmt.column_count();
    let Ok(rows) = stmt.query_map([], |r| {
        (0..n)
            .map(|i| r.get_ref(i).map(sql_json))
            .collect::<Result<Vec<_>, _>>()
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

fn cell_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(), // Python's str(None), as the original keys it
        other => other.to_string(),
    }
}

pub fn opencode(db_path: &Path) -> rusqlite::Result<Vec<Part>> {
    let db = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(2))?;
    let _ = db.execute_batch("PRAGMA query_only = ON");
    let tables: HashSet<String> = rows(&db, "SELECT name FROM sqlite_master WHERE type = 'table'")
        .iter()
        .map(|r| cell_str(&r[0]))
        .collect();
    let has = |t: &str, cols: &[&str]| {
        tables.contains(t) && {
            let c = columns(&db, t);
            cols.iter().all(|x| c.contains(*x))
        }
    };

    // Subagent sessions: tokens yes, prompts no, and not listed as yours.
    let mut helpers: HashSet<String> = HashSet::new();
    let mut born: HashMap<String, u64> = HashMap::new();
    let mut meta: HashMap<String, (String, String)> = HashMap::new(); // id -> (title, directory)
    for table in ["session", "session_v2"] {
        if has(table, &["id", "parent_id"]) {
            for r in rows(
                &db,
                &format!("SELECT id FROM {table} WHERE parent_id IS NOT NULL"),
            ) {
                helpers.insert(cell_str(&r[0]));
            }
        }
        if has(table, &["id", "time_created"]) {
            for r in rows(&db, &format!("SELECT id, time_created FROM {table}")) {
                born.insert(cell_str(&r[0]), number(Some(&r[1])));
            }
        }
        if tables.contains(table) {
            let c = columns(&db, table);
            if c.contains("id") {
                let picks: Vec<&str> = ["id", "title", "directory"]
                    .into_iter()
                    .filter(|x| c.contains(*x))
                    .collect();
                for r in rows(&db, &format!("SELECT {} FROM {table}", picks.join(", "))) {
                    let get = |name: &str| {
                        picks
                            .iter()
                            .position(|p| *p == name)
                            .map(|i| text(Some(&r[i])))
                            .unwrap_or_default()
                    };
                    let id = cell_str(&r[0]);
                    if table == "session" || !meta.contains_key(&id) {
                        meta.insert(id, (get("title"), get("directory")));
                    }
                }
            }
        }
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut events: Vec<(String, Vec<Event>)> = Vec::new();
    let mut order = 0u64;
    let mut take = |sid: String,
                    mid: String,
                    kind: Option<String>,
                    raw: &Value,
                    fallback: Option<(String, f64)>| {
        let data = match raw {
            Value::String(s) => match serde_json::from_str::<Value>(s) {
                Ok(Value::Object(m)) => m,
                Ok(_) | Err(_) => return,
            },
            _ => Map::new(),
        };
        if seen.contains(&mid) {
            return; // migrated sessions sit in both stores
        }
        let role = kind
            .filter(|k| !k.is_empty())
            .or_else(|| data.get("role").and_then(Value::as_str).map(String::from));
        let role = role.as_deref().unwrap_or("");
        if role == "user" && truthy(data.get("synthetic")) {
            return; // opencode's own notes, not typed
        }
        if !matches!(role, "user" | "assistant" | "compaction") {
            return;
        }
        let t = obj(data.get("time"));
        let created_v = t.and_then(|t| t.get("created"));
        let (day, moment) = created_v
            .and_then(local_day)
            .or(fallback)
            .or_else(|| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64();
                time::day_of(now).map(|d| (d, now)) // no date to trust: still today's session
            })
            .unwrap_or_default();
        let created = number(created_v);
        if created > 0 && created < born.get(&sid).copied().unwrap_or(0) {
            return; // a fork copy, spent in its source session
        }
        seen.insert(mid);
        order += 1;
        let ev = if role == "user" {
            Event {
                order,
                user: true,
                day,
                moment,
                model: String::new(),
                tokens: 0,
            }
        } else {
            let nested = obj(data.get("model"));
            let raw_model = text(first(&[
                data.get("modelID"),
                nested.and_then(|n| n.get("id")),
                nested.and_then(|n| n.get("modelID")),
            ]));
            Event {
                order,
                user: false,
                day,
                moment,
                model: opencode_model_name(&raw_model),
                tokens: opencode_tokens(&data),
            }
        };
        match events.iter_mut().find(|(s, _)| *s == sid) {
            Some((_, list)) => list.push(ev),
            None => events.push((sid, vec![ev])),
        }
    };

    if has("session_message", &["id", "session_id", "type", "data"]) {
        let c = columns(&db, "session_message");
        let created = if c.contains("time_created") {
            "time_created"
        } else {
            "NULL"
        };
        let ord = if c.contains("seq") { "seq" } else { created };
        for r in rows(&db, &format!("SELECT id, session_id, type, {created}, data FROM session_message ORDER BY {ord}, id")) {
            let fallback = match &r[3] {
                Value::Number(n) if n.as_f64().unwrap_or(0.0) != 0.0 => local_day(&r[3]),
                _ => None,
            };
            let kind = match &r[2] {
                Value::String(s) => Some(s.clone()),
                _ => None,
            };
            take(cell_str(&r[1]), cell_str(&r[0]), kind, &r[4], fallback);
        }
    }
    if has("message", &["session_id", "data"]) {
        if columns(&db, "message").contains("id") {
            // Same ids as the V2 store: deduplicated together, as Omarchy does.
            for r in rows(&db, "SELECT id, session_id, data FROM message") {
                take(cell_str(&r[1]), cell_str(&r[0]), None, &r[2], None);
            }
        } else {
            for r in rows(&db, "SELECT rowid, session_id, data FROM message") {
                take(
                    cell_str(&r[1]),
                    format!("legacy:{}", cell_str(&r[0])),
                    None,
                    &r[2],
                    None,
                );
            }
        }
    }

    let mut parts: Vec<Part> = Vec::new();
    for (sid, mut evs) in events {
        let (title, cwd) = meta.get(&sid).cloned().unwrap_or_default();
        let mut part = Part::new(&sid, &chars(&title, 80));
        part.session.cwd = cwd;
        let helper = helpers.contains(&sid);
        evs.sort_by_key(|e| e.order);
        if !evs.iter().any(|e| e.user) && !helper {
            // No typed prompts: each reply that spent tokens books its own, as Omarchy's collector does.
            for e in &evs {
                if e.tokens > 0 {
                    part.add(Some(&e.day), &e.model, e.tokens, 1);
                }
                if e.moment != 0.0 {
                    part.see(e.moment);
                }
            }
        } else {
            let mut pending = 0u64;
            for e in &evs {
                if e.moment != 0.0 {
                    part.see(e.moment);
                }
                if e.user {
                    pending += 1;
                } else if pending > 0 && !helper {
                    // The first reply answering a user message books it, even when it failed and spent nothing.
                    part.add(Some(&e.day), &e.model, e.tokens, 1);
                    pending -= 1;
                } else if e.tokens > 0 {
                    // A later step: its tokens, but no new prompt.
                    part.add(Some(&e.day), &e.model, e.tokens, 0);
                }
            }
            for e in &evs {
                // A typed prompt no reply answered (failed or cut off) books itself.
                if e.user && pending > 0 && !helper {
                    part.add(Some(&e.day), "", 0, 1);
                    pending -= 1;
                }
            }
        }
        if helper {
            part.session.last = 0.0;
        }
        parts.push(part);
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names() {
        assert_eq!(opencode_model_name("zai/glm-4.6:high"), "GLM-4.6");
        assert_eq!(
            opencode_model_name("anthropic/claude-opus-4-5/"),
            "claude-opus-4-5"
        );
        assert_eq!(opencode_model_name(""), "");
    }
}
