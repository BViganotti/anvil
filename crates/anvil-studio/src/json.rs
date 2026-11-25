//! Small, bounded JSON parser and serializer.
//!
//! Limits: input bytes, nesting depth, string length and array/object sizes.
//! Duplicate object keys and non-finite numbers are rejected. Serialization is
//! deterministic (object keys sorted) and never emits NaN/infinity.

use anvil_core::util::limits;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn object() -> Self {
        Value::Object(BTreeMap::new())
    }

    pub fn arr(values: Vec<Value>) -> Self {
        Value::Array(values)
    }

    pub fn str(s: impl Into<String>) -> Self {
        Value::String(s.into())
    }

    pub fn num(n: impl Into<f64>) -> Self {
        Value::Number(n.into())
    }

    pub fn int(n: i64) -> Self {
        Value::Number(n as f64)
    }

    pub fn bool(b: bool) -> Self {
        Value::Bool(b)
    }

    pub fn set(&mut self, key: &str, value: Value) -> &mut Self {
        if let Value::Object(map) = self {
            map.insert(key.to_string(), value);
        }
        self
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(n) if n.is_finite() && n.abs() < 9e15 => Some(n.round() as i64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        write_value(self, &mut out);
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub at: usize,
    pub message: String,
}

impl JsonError {
    fn new(at: usize, message: impl Into<String>) -> Self {
        JsonError {
            at,
            message: message.into(),
        }
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON error at byte {}: {}", self.at, self.message)
    }
}

impl std::error::Error for JsonError {}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser {
            bytes: text.as_bytes(),
            pos: 0,
            depth: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), JsonError> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(JsonError::new(
                self.pos,
                format!("expected {:?}", b as char),
            ))
        }
    }

    fn value(&mut self) -> Result<Value, JsonError> {
        self.skip_ws();
        let b = self.peek().ok_or_else(|| JsonError::new(self.pos, "unexpected end"))?;
        match b {
            b'n' => {
                self.literal("null")?;
                Ok(Value::Null)
            }
            b't' => {
                self.literal("true")?;
                Ok(Value::Bool(true))
            }
            b'f' => {
                self.literal("false")?;
                Ok(Value::Bool(false))
            }
            b'"' => Ok(Value::String(self.string()?)),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(JsonError::new(self.pos, "unexpected character")),
        }
    }

    fn literal(&mut self, lit: &str) -> Result<(), JsonError> {
        if self.bytes.len() - self.pos < lit.len()
            || &self.bytes[self.pos..self.pos + lit.len()] != lit.as_bytes()
        {
            return Err(JsonError::new(self.pos, "invalid literal"));
        }
        self.pos += lit.len();
        Ok(())
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out = String::new();
        let mut escaped = false;
        loop {
            let b = self.bump().ok_or_else(|| JsonError::new(self.pos, "unterminated string"))?;
            match b {
                b'"' => break,
                b'\\' => {
                    escaped = true;
                    let e = self.bump().ok_or_else(|| JsonError::new(self.pos, "bad escape"))?;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let cp = self.hex4()?;
                            if (0xd800..=0xdbff).contains(&cp) {
                                if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                    return Err(JsonError::new(self.pos, "bad surrogate pair"));
                                }
                                let low = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return Err(JsonError::new(self.pos, "bad low surrogate"));
                                }
                                let c = 0x10000 + ((cp - 0xd800) << 10) + (low - 0xdc00);
                                let ch = char::from_u32(c)
                                    .ok_or_else(|| JsonError::new(self.pos, "bad code point"))?;
                                out.push(ch);
                            } else if (0xdc00..=0xdfff).contains(&cp) {
                                return Err(JsonError::new(self.pos, "lone low surrogate"));
                            } else {
                                let ch = char::from_u32(cp)
                                    .ok_or_else(|| JsonError::new(self.pos, "bad code point"))?;
                                out.push(ch);
                            }
                        }
                        _ => return Err(JsonError::new(self.pos, "unknown escape")),
                    }
                }
                0x00..=0x1f => {
                    return Err(JsonError::new(self.pos, "control character in string"));
                }
                _ => {
                    // The input is a &str; copy the UTF-8 bytes for this code
                    // point. ASCII is the common case.
                    if b < 0x80 {
                        out.push(b as char);
                    } else {
                        let start = self.pos - 1;
                        let len = utf8_len(b);
                        if start + len > self.bytes.len() {
                            return Err(JsonError::new(start, "truncated UTF-8"));
                        }
                        let s = std::str::from_utf8(&self.bytes[start..start + len])
                            .map_err(|_| JsonError::new(start, "invalid UTF-8"))?;
                        out.push_str(s);
                        self.pos = start + len;
                    }
                }
            }
            if out.len() > 1_000_000 {
                return Err(JsonError::new(self.pos, "string too large"));
            }
        }
        let _ = escaped;
        Ok(out)
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.bump().ok_or_else(|| JsonError::new(self.pos, "short unicode escape"))?;
            let d = (b as char)
                .to_digit(16)
                .ok_or_else(|| JsonError::new(self.pos, "bad unicode escape"))?;
            v = (v << 4) | d;
        }
        Ok(v)
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let int_start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == int_start {
            return Err(JsonError::new(start, "bad number"));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            let frac_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos == frac_start {
                return Err(JsonError::new(start, "bad fraction"));
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos == exp_start {
                return Err(JsonError::new(start, "bad exponent"));
            }
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| JsonError::new(start, "bad number"))?;
        let n: f64 = s
            .parse()
            .map_err(|_| JsonError::new(start, "bad number"))?;
        if !n.is_finite() {
            return Err(JsonError::new(start, "non-finite number"));
        }
        if (self.pos - start) > 64 {
            return Err(JsonError::new(start, "number too long"));
        }
        Ok(Value::Number(n))
    }

    fn array(&mut self) -> Result<Value, JsonError> {
        self.depth += 1;
        if self.depth > limits::MAX_JSON_DEPTH {
            return Err(JsonError::new(self.pos, "nesting too deep"));
        }
        self.expect(b'[')?;
        self.skip_ws();
        let mut out = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Array(out));
        }
        loop {
            if out.len() >= 1_000_000 {
                return Err(JsonError::new(self.pos, "array too large"));
            }
            out.push(self.value()?);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => break,
                _ => return Err(JsonError::new(self.pos, "expected , or ]")),
            }
        }
        self.depth -= 1;
        Ok(Value::Array(out))
    }

    fn object(&mut self) -> Result<Value, JsonError> {
        self.depth += 1;
        if self.depth > limits::MAX_JSON_DEPTH {
            return Err(JsonError::new(self.pos, "nesting too deep"));
        }
        self.expect(b'{')?;
        self.skip_ws();
        let mut map = BTreeMap::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(JsonError::new(self.pos, "expected object key"));
            }
            let key = self.string()?;
            if map.len() >= 100_000 {
                return Err(JsonError::new(self.pos, "object too large"));
            }
            if map.contains_key(&key) {
                return Err(JsonError::new(self.pos, format!("duplicate key {:?}", key)));
            }
            self.skip_ws();
            self.expect(b':')?;
            let val = self.value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => break,
                _ => return Err(JsonError::new(self.pos, "expected , or }")),
            }
        }
        self.depth -= 1;
        Ok(Value::Object(map))
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

pub fn parse(text: &str) -> Result<Value, JsonError> {
    if text.len() > limits::MAX_JSON_BYTES {
        return Err(JsonError::new(0, "input too large"));
    }
    let mut p = Parser::new(text);
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(JsonError::new(p.pos, "trailing data"));
    }
    Ok(v)
}

fn escape_into(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn number_into(n: f64, out: &mut String) {
    if !n.is_finite() {
        out.push_str("null");
        return;
    }
    if n.fract() == 0.0 && n.abs() < 9e15 {
        out.push_str(&format!("{}", n as i64));
    } else {
        let s = format!("{:.15}", n);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s.is_empty() || s == "-" {
            out.push('0');
        } else {
            out.push_str(s);
        }
    }
}

fn write_value(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => number_into(*n, out),
        Value::String(s) => escape_into(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, val)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                escape_into(k, out);
                out.push(':');
                write_value(val, out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_values() {
        let v = parse(r#"{"a":1,"b":[true,null,"x\n"],"c":-2.5e2}"#).unwrap();
        assert_eq!(v.get("a").unwrap().as_i64(), Some(1));
        assert_eq!(v.get("b").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(v.get("c").unwrap().as_f64(), Some(-250.0));
    }

    #[test]
    fn round_trip() {
        let text = r#"{"z":[1,2,3],"a":{"b":"c","n":1.25}}"#;
        let v = parse(text).unwrap();
        let out = v.to_json();
        assert_eq!(parse(&out).unwrap(), v);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("{").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("{\"a\":1,\"a\":2}").is_err());
        assert!(parse("1e999").is_err());
        assert!(parse("\"\\ud800\"").is_err());
        assert!(parse("{} trailing").is_err());
    }

    #[test]
    fn depth_is_limited() {
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn unicode() {
        let v = parse(r#"{"s":"caf\u00e9 \ud83d\ude00"}"#).unwrap();
        assert_eq!(v.get("s").unwrap().as_str().unwrap(), "café 😀");
        let out = v.to_json();
        assert_eq!(parse(&out).unwrap(), v);
    }
}
