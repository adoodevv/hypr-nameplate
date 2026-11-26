//! Just enough JSON for this program: reading what Hyprland answers, and writing
//! the one-line objects Waybar reads.
//!
//! A JSON value is one of a handful of shapes, which is exactly what a Rust `enum`
//! is for. The parser below is a "recursive descent" parser: one small function per
//! kind of value, each calling the others for whatever is nested inside.

/// A JSON object: its keys in the order they were written (as Python's dict
/// keeps them), so lists that come as objects keep their order. Objects here are
/// small, so a list of pairs is all it takes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Map(Vec<(String, Value)>);

impl Map {
    pub fn get(&self, key: &str) -> Option<&Value> {
        // The last one wins, as in most JSON readers, if a key comes twice.
        self.0.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    /// Set a key, keeping its place if it's already there.
    pub fn insert(&mut self, key: String, value: Value) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key, value)),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.0.iter().map(|(_, v)| v)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<Value>),
    Object(Map),
}

impl Value {
    /// `value.get("key")` on an object; `None` for anything else or a missing key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Value] {
        match self {
            Value::Array(items) => items,
            _ => &[],
        }
    }

    /// A string field, or "" when it's missing (most callers only compare it).
    pub fn str_of(&self, key: &str) -> &str {
        self.get(key).and_then(Value::as_str).unwrap_or("")
    }
}

impl std::fmt::Display for Value {
    /// Compact JSON, object keys in their order.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => write!(f, "{}", *n as i64),
            Value::Number(n) => write!(f, "{n}"),
            Value::Str(s) => f.write_str(&quote(s)),
            Value::Array(items) => {
                f.write_str("[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Value::Object(map) => {
                f.write_str("{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}: {v}", quote(k))?;
                }
                f.write_str("}")
            }
        }
    }
}

/// Parse a whole document. `None` if it isn't valid JSON.
pub fn parse(text: &str) -> Option<Value> {
    let mut p = Parser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let value = p.value()?;
    p.skip_space();
    if p.pos == p.bytes.len() {
        Some(value)
    } else {
        None
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, word: &str) -> Option<()> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Some(())
        } else {
            None
        }
    }

    fn value(&mut self) -> Option<Value> {
        self.skip_space();
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Value::Str),
            b't' => self.eat("true").map(|_| Value::Bool(true)),
            b'f' => self.eat("false").map(|_| Value::Bool(false)),
            b'n' => self.eat("null").map(|_| Value::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Option<Value> {
        self.pos += 1; // the '{'
        let mut map = Map::default();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(Value::Object(map));
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.skip_space();
            self.eat(":")?;
            let value = self.value()?;
            map.insert(key, value);
            self.skip_space();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Some(Value::Object(map));
                }
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<Value> {
        self.pos += 1; // the '['
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_space();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Some(Value::Array(items));
                }
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.peek()? != b'"' {
            return None;
        }
        self.pos += 1;
        let mut out = String::new();
        loop {
            // Copy a run of ordinary bytes in one go (they're valid UTF-8 already).
            let start = self.pos;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(std::str::from_utf8(&self.bytes[start..self.pos]).ok()?);
            match self.peek()? {
                b'"' => {
                    self.pos += 1;
                    return Some(out);
                }
                _ => {
                    // A backslash escape
                    self.pos += 1;
                    let c = self.peek()?;
                    self.pos += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            // Characters past U+FFFF come as a pair of escapes
                            if (0xD800..0xDC00).contains(&code) && self.eat("\\u").is_some() {
                                let low = self.hex4()?;
                                code = 0x10000
                                    + ((code - 0xD800) << 10)
                                    + (low.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return None,
                    }
                }
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = std::str::from_utf8(self.bytes.get(self.pos..self.pos + 4)?).ok()?;
        self.pos += 4;
        u32::from_str_radix(digits, 16).ok()
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        text.parse().ok().map(Value::Number)
    }
}

/// Put a string inside JSON quotes, escaping what has to be escaped.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_hyprctl_sends() {
        let v = parse(
            r#"{"keyboards": [{"name": "at-translated", "layout": "us,de",
            "active_layout_index": 1, "main": true, "x": null, "t": "a\"bé"}]}"#,
        )
        .unwrap();
        let kb = &v.get("keyboards").unwrap().as_array()[0];
        assert_eq!(kb.str_of("layout"), "us,de");
        assert_eq!(
            kb.get("active_layout_index").and_then(Value::as_f64),
            Some(1.0)
        );
        assert_eq!(kb.get("main").and_then(Value::as_bool), Some(true));
        assert_eq!(kb.str_of("t"), "a\"bé");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("{").is_none());
        assert!(parse("[1,]").is_none());
        assert!(parse("ok").is_none());
    }

    #[test]
    fn round_trips() {
        let text = r#"{"autohide": true, "pinned": ["a.desktop", "b \"c\""], "n": 3, "x": 1.5}"#;
        let v = parse(text).unwrap();
        assert_eq!(parse(&v.to_string()).unwrap(), v);
        // Keys come back in the order they were written.
        assert_eq!(v.to_string(), text);
    }

    #[test]
    fn quotes() {
        assert_eq!(quote("a\"b\\c\n"), r#""a\"b\\c\n""#);
        assert_eq!(quote("󰤨"), "\"󰤨\"");
    }
}

#[cfg(test)]
mod order_tests {
    use super::*;

    #[test]
    fn objects_keep_their_order() {
        let v = parse(r#"{"zeta": 1, "alpha": 2, "mid": {"b": 1, "a": 2}, "alpha": 3}"#).unwrap();
        let Value::Object(map) = &v else { panic!() };
        let keys: Vec<&str> = map.iter().map(|(k, _)| k.as_str()).collect();
        // A repeated key keeps its first place and its last value, like a Python dict.
        assert_eq!(keys, ["zeta", "alpha", "mid"]);
        assert_eq!(v.get("alpha").and_then(Value::as_f64), Some(3.0));
        assert_eq!(
            v.to_string(),
            r#"{"zeta": 1, "alpha": 3, "mid": {"b": 1, "a": 2}}"#
        );
        let mut m = Map::default();
        m.insert("b".into(), Value::Null);
        m.insert("a".into(), Value::Bool(true));
        m.insert("b".into(), Value::Bool(false));
        assert_eq!(Value::Object(m).to_string(), r#"{"b": false, "a": true}"#);
    }
}
