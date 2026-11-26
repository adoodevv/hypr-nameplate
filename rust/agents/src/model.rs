//! What one session (or one history file) adds up to.

use crate::time::Day;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub cwd: String,
    pub title: String,
    /// When it was last used, in epoch seconds; 0 for helper sessions, which
    /// aren't listed as yours.
    pub last: f64,
    pub tokens: u64,
}

/// Use by day and by model for one session. Kept in insertion order, so ties
/// (the "most used model") resolve the same way every time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// (day, tokens, prompts)
    pub days: Vec<(Day, u64, u64)>,
    /// (day, model, tokens)
    pub models: Vec<(Day, String, u64)>,
    pub session: Session,
}

impl Part {
    pub fn new(id: &str, title: &str) -> Part {
        Part {
            session: Session {
                id: id.into(),
                title: title.into(),
                ..Session::default()
            },
            ..Part::default()
        }
    }

    pub fn day_mut(&mut self, day: &str) -> &mut (Day, u64, u64) {
        match self.days.iter().position(|d| d.0 == day) {
            Some(i) => &mut self.days[i],
            None => {
                self.days.push((day.to_string(), 0, 0));
                self.days.last_mut().expect("just pushed")
            }
        }
    }

    pub fn add_model(&mut self, day: &str, model: &str, tokens: u64) {
        match self.models.iter_mut().find(|m| m.0 == day && m.1 == model) {
            Some(m) => m.2 += tokens,
            None => self
                .models
                .push((day.to_string(), model.to_string(), tokens)),
        }
    }

    /// Book tokens and prompts on a day (nothing without a day, or when both are 0).
    pub fn add(&mut self, day: Option<&str>, model: &str, tokens: u64, prompts: u64) {
        let Some(day) = day else { return };
        if tokens == 0 && prompts == 0 {
            return;
        }
        let d = self.day_mut(day);
        d.1 += tokens;
        d.2 += prompts;
        if tokens > 0 && !model.is_empty() {
            self.add_model(day, model, tokens);
        }
        self.session.tokens += tokens;
    }

    pub fn see(&mut self, moment: f64) {
        if moment > self.session.last {
            self.session.last = moment;
        }
    }
}

/// A non-negative whole number from whatever JSON holds (Python's `int()`).
pub fn number(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f > 0.0).map(|f| f as u64))
            .unwrap_or(0),
        Some(Value::Bool(true)) => 1,
        Some(Value::String(s)) => s.trim().parse::<i64>().map_or(0, |n| n.max(0) as u64),
        _ => 0,
    }
}

/// Python's `str(x or "")`: the text of a value, "" for anything falsy.
pub fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | Some(Value::Bool(false)) | None => String::new(),
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => String::new(),
        Some(Value::Bool(true)) => "True".into(),
        Some(other) => other.to_string(),
    }
}

/// Python truthiness, for `a or b` chains.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        _ => true,
    }
}

/// The first truthy of several values (`a or b or c`).
pub fn first<'a>(values: &[Option<&'a Value>]) -> Option<&'a Value> {
    values.iter().copied().find(|v| truthy(*v)).flatten()
}

/// At most `n` characters (not bytes).
pub fn chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_like_python_int() {
        assert_eq!(number(Some(&json!(12))), 12);
        assert_eq!(number(Some(&json!(-3))), 0);
        assert_eq!(number(Some(&json!(2.9))), 2);
        assert_eq!(number(Some(&json!(" 7 "))), 7);
        assert_eq!(number(Some(&json!("2.5"))), 0);
        assert_eq!(number(Some(&json!(true))), 1);
        assert_eq!(number(None), 0);
    }

    #[test]
    fn booking() {
        let mut p = Part::new("s", "");
        p.add(Some("2026-11-26"), "opus", 10, 1);
        p.add(Some("2026-11-26"), "opus", 5, 0);
        p.add(Some("2026-11-26"), "", 0, 0); // nothing
        p.add(None, "opus", 99, 9); // no day
        assert_eq!(p.days, vec![("2026-11-26".into(), 15, 1)]);
        assert_eq!(p.models, vec![("2026-11-26".into(), "opus".into(), 15)]);
        assert_eq!(p.session.tokens, 15);
    }
}
