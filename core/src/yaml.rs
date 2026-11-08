//! A small YAML reader and writer for the data files (`factions.yml`, `kits.yml`,
//! `stats.yml`, `bots.yml`, `paths.yml`, `scripts.yml`, `skin-cache.yml`, `bot-settings.yml`,
//! `messages.yml`).
//!
//! Only the block style a Bukkit `YamlConfiguration` writes is understood: nested mappings,
//! sequences (also at the indentation of their key), plain, single- and double-quoted scalars,
//! the empty flow collections `[]` and `{}` and short flow sequences. Anchors, tags other than
//! Bukkit's `==` key (an ordinary key here), multi-document streams and block scalars (`|`, `>`)
//! are not. The writer produces the same style, so a file written here reads back in any YAML tool that handles block style.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Yaml {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Yaml>),
    /// Keys in file order.
    Map(Vec<(String, Yaml)>),
}

impl Yaml {
    pub fn map() -> Self {
        Yaml::Map(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Yaml> {
        match self {
            Yaml::Map(m) => m.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets `key`, keeping its place when it exists. A non-map becomes a map.
    pub fn set(&mut self, key: &str, value: Yaml) {
        if !matches!(self, Yaml::Map(_)) {
            *self = Yaml::map();
        }
        if let Yaml::Map(m) = self {
            if let Some(slot) = m.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
            } else {
                m.push((key.to_owned(), value));
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Yaml> {
        match self {
            Yaml::Map(m) => {
                let i = m.iter().position(|(k, _)| k == key)?;
                Some(m.remove(i).1)
            }
            _ => None,
        }
    }

    /// The value under a dotted path (`a.b.c`).
    pub fn path(&self, path: &str) -> Option<&Yaml> {
        path.split('.').try_fold(self, |v, k| v.get(k))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Yaml::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Any scalar as text (numbers and booleans as written).
    pub fn as_text(&self) -> Option<String> {
        match self {
            Yaml::Str(s) => Some(s.clone()),
            Yaml::Int(i) => Some(i.to_string()),
            Yaml::Float(f) => Some(fmt_float(*f)),
            Yaml::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Yaml::Int(i) => Some(*i),
            Yaml::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            Yaml::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Yaml::Int(i) => Some(*i as f64),
            Yaml::Float(f) => Some(*f),
            Yaml::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Yaml::Bool(b) => Some(*b),
            Yaml::Str(s) => match s.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<&[Yaml]> {
        match self {
            Yaml::Seq(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&[(String, Yaml)]> {
        match self {
            Yaml::Map(m) => Some(m),
            _ => None,
        }
    }

    /// A sequence of scalars as strings; anything else is skipped.
    pub fn strings(&self) -> Vec<String> {
        self.as_seq()
            .map(|s| s.iter().filter_map(Yaml::as_text).collect())
            .unwrap_or_default()
    }

    pub fn str_seq<I: IntoIterator<Item = S>, S: Into<String>>(items: I) -> Yaml {
        Yaml::Seq(items.into_iter().map(|s| Yaml::Str(s.into())).collect())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct YamlError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

struct Line {
    no: usize,
    indent: usize,
    text: String,
}

/// Reads a document. An empty document is an empty map.
pub fn parse(src: &str) -> Result<Yaml, YamlError> {
    let mut lines = Vec::new();
    for (no, raw) in src.lines().enumerate() {
        let raw = raw.trim_end_matches('\r');
        if raw.trim_start().starts_with("---") && raw.trim() == "---" {
            continue;
        }
        let stripped = strip_comment(raw);
        if stripped.trim().is_empty() {
            continue;
        }
        if stripped.contains('\t') && stripped.trim_start().len() != stripped.len() {
            let lead = &stripped[..stripped.len() - stripped.trim_start().len()];
            if lead.contains('\t') {
                return Err(YamlError {
                    line: no + 1,
                    message: "tab in indentation".into(),
                });
            }
        }
        let indent = stripped.len() - stripped.trim_start().len();
        lines.push(Line {
            no: no + 1,
            indent,
            text: stripped.trim().to_owned(),
        });
    }
    if lines.is_empty() {
        return Ok(Yaml::map());
    }
    let mut i = 0;
    let indent = lines[0].indent;
    let v = block(&mut lines, &mut i, indent)?;
    if i < lines.len() {
        return Err(YamlError {
            line: lines[i].no,
            message: "unexpected indentation".into(),
        });
    }
    Ok(v)
}

fn strip_comment(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let (mut single, mut double) = (false, false);
    let mut prev = ' ';
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' if !double => single = !single,
            '"' if !single && prev != '\\' => double = !double,
            '#' if !single && !double && (prev == ' ' || out.is_empty()) => break,
            _ => {}
        }
        out.push(c);
        prev = c;
    }
    out.trim_end().to_owned()
}

fn is_seq_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn block(lines: &mut Vec<Line>, i: &mut usize, indent: usize) -> Result<Yaml, YamlError> {
    if *i >= lines.len() {
        return Ok(Yaml::Null);
    }
    if is_seq_item(&lines[*i].text) {
        seq(lines, i, indent)
    } else if split_key(&lines[*i].text).is_some() {
        map(lines, i, indent)
    } else {
        // A lone scalar (a document that is just a value).
        let v = scalar(&lines[*i].text, lines[*i].no)?;
        *i += 1;
        Ok(v)
    }
}

fn seq(lines: &mut Vec<Line>, i: &mut usize, indent: usize) -> Result<Yaml, YamlError> {
    let mut items = Vec::new();
    while *i < lines.len() && lines[*i].indent == indent && is_seq_item(&lines[*i].text) {
        let no = lines[*i].no;
        let rest = lines[*i].text[1..].trim_start().to_owned();
        if rest.is_empty() {
            *i += 1;
            if *i < lines.len() && lines[*i].indent > indent {
                let inner = lines[*i].indent;
                items.push(block(lines, i, inner)?);
            } else {
                items.push(Yaml::Null);
            }
        } else if is_seq_item(&rest) || split_key(&rest).is_some() {
            // `- key: value` (or `- - x`): the item's own block starts on this line.
            let inner = indent + (lines[*i].text.len() - rest.len());
            lines[*i].indent = inner;
            lines[*i].text = rest;
            items.push(block(lines, i, inner)?);
        } else {
            items.push(scalar(&rest, no)?);
            *i += 1;
        }
    }
    Ok(Yaml::Seq(items))
}

fn map(lines: &mut Vec<Line>, i: &mut usize, indent: usize) -> Result<Yaml, YamlError> {
    let mut entries: Vec<(String, Yaml)> = Vec::new();
    while *i < lines.len() && lines[*i].indent == indent && !is_seq_item(&lines[*i].text) {
        let no = lines[*i].no;
        let Some((key, rest)) = split_key(&lines[*i].text) else {
            return Err(YamlError {
                line: no,
                message: format!("expected `key: value`, found `{}`", lines[*i].text),
            });
        };
        *i += 1;
        let value = if rest.is_empty() {
            if *i < lines.len() && lines[*i].indent > indent {
                let inner = lines[*i].indent;
                block(lines, i, inner)?
            } else if *i < lines.len() && lines[*i].indent == indent && is_seq_item(&lines[*i].text) {
                // A sequence at the key's own indentation (Bukkit's default).
                seq(lines, i, indent)?
            } else {
                Yaml::Null
            }
        } else {
            let mut text = rest;
            // A plain or quoted scalar folded over more-indented lines.
            while *i < lines.len()
                && lines[*i].indent > indent
                && !is_seq_item(&lines[*i].text)
                && split_key(&lines[*i].text).is_none()
            {
                text.push(' ');
                text.push_str(&lines[*i].text);
                *i += 1;
            }
            scalar(&text, no)?
        };
        if let Some(slot) = entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            entries.push((key, value));
        }
    }
    Ok(Yaml::Map(entries))
}

/// `key: rest` (rest may be empty), with a plain or quoted key.
fn split_key(text: &str) -> Option<(String, String)> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if bytes[0] == b'\'' || bytes[0] == b'"' {
        let q = bytes[0];
        let mut j = 1;
        while j < bytes.len() {
            if bytes[j] == q {
                if q == b'\'' && j + 1 < bytes.len() && bytes[j + 1] == b'\'' {
                    j += 2;
                    continue;
                }
                if q == b'"' && bytes[j - 1] == b'\\' {
                    j += 1;
                    continue;
                }
                break;
            }
            j += 1;
        }
        let after = text.get(j + 1..)?;
        let after = after.strip_prefix(':')?;
        if !(after.is_empty() || after.starts_with(' ')) {
            return None;
        }
        let key = match scalar(&text[..=j], 0).ok()? {
            Yaml::Str(s) => s,
            other => other.as_text()?,
        };
        return Some((key, after.trim().to_owned()));
    }
    if bytes[0] == b'[' || bytes[0] == b'{' {
        return None;
    }
    let mut start = 0;
    while let Some(pos) = text[start..].find(':') {
        let at = start + pos;
        let after = &text[at + 1..];
        if after.is_empty() || after.starts_with(' ') {
            return Some((text[..at].trim().to_owned(), after.trim().to_owned()));
        }
        start = at + 1;
    }
    None
}

fn scalar(text: &str, line: usize) -> Result<Yaml, YamlError> {
    let t = text.trim();
    let err = |m: &str| YamlError {
        line,
        message: m.to_owned(),
    };
    if t.is_empty() || t == "~" || t == "null" || t == "Null" || t == "NULL" {
        return Ok(Yaml::Null);
    }
    if let Some(body) = t.strip_prefix('\'') {
        let body = body.strip_suffix('\'').ok_or_else(|| err("unclosed single quote"))?;
        return Ok(Yaml::Str(body.replace("''", "'")));
    }
    if let Some(body) = t.strip_prefix('"') {
        let body = body.strip_suffix('"').ok_or_else(|| err("unclosed double quote"))?;
        return Ok(Yaml::Str(unescape_double(body)));
    }
    if t.starts_with('[') {
        let inner = t
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .ok_or_else(|| err("unclosed ["))?;
        if inner.trim().is_empty() {
            return Ok(Yaml::Seq(Vec::new()));
        }
        let mut out = Vec::new();
        for part in split_flow(inner) {
            out.push(scalar(&part, line)?);
        }
        return Ok(Yaml::Seq(out));
    }
    if t.starts_with('{') {
        let inner = t
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .ok_or_else(|| err("unclosed {"))?;
        let mut out = Vec::new();
        if !inner.trim().is_empty() {
            for part in split_flow(inner) {
                let (k, v) = split_key(&part)
                    .or_else(|| part.split_once(':').map(|(a, b)| (a.trim().to_owned(), b.trim().to_owned())))
                    .ok_or_else(|| err("bad flow mapping"))?;
                out.push((k, scalar(&v, line)?));
            }
        }
        return Ok(Yaml::Map(out));
    }
    match t {
        "true" | "True" | "TRUE" | "yes" | "on" => return Ok(Yaml::Bool(true)),
        "false" | "False" | "FALSE" | "no" | "off" => return Ok(Yaml::Bool(false)),
        _ => {}
    }
    if let Ok(i) = t.parse::<i64>() {
        if !(t.len() > 1 && t.starts_with('0') && !t.starts_with("0.")) {
            return Ok(Yaml::Int(i));
        }
    }
    if looks_float(t) {
        if let Ok(f) = t.parse::<f64>() {
            return Ok(Yaml::Float(f));
        }
    }
    match t {
        ".inf" | ".Inf" => return Ok(Yaml::Float(f64::INFINITY)),
        "-.inf" | "-.Inf" => return Ok(Yaml::Float(f64::NEG_INFINITY)),
        ".nan" | ".NaN" => return Ok(Yaml::Float(f64::NAN)),
        _ => {}
    }
    Ok(Yaml::Str(t.to_owned()))
}

fn looks_float(t: &str) -> bool {
    let t = t.strip_prefix(['-', '+']).unwrap_or(t);
    !t.is_empty()
        && t.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')
        && t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '-' | '+'))
}

fn split_flow(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut single, mut double) = (0i32, false, false);
    for c in inner.chars() {
        match c {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '[' | '{' if !single && !double => depth += 1,
            ']' | '}' if !single && !double => depth -= 1,
            ',' if depth == 0 && !single && !double => {
                parts.push(cur.trim().to_owned());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_owned());
    }
    parts
}

fn unescape_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some(' ') => out.push(' '),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub fn fmt_float(f: f64) -> String {
    if f.is_nan() {
        ".NaN".into()
    } else if f.is_infinite() {
        if f > 0.0 { ".inf".into() } else { "-.inf".into() }
    } else if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// Writes a document in Bukkit's block style.
pub fn dump(doc: &Yaml) -> String {
    let mut out = String::new();
    match doc {
        Yaml::Map(m) if m.is_empty() => out.push_str("{}\n"),
        Yaml::Map(_) | Yaml::Seq(_) => write_block(doc, 0, &mut out),
        other => {
            out.push_str(&inline(other));
            out.push('\n');
        }
    }
    out
}

fn write_block(v: &Yaml, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match v {
        Yaml::Map(m) => {
            for (k, val) in m {
                let key = quote_key(k);
                match val {
                    Yaml::Map(inner) if !inner.is_empty() => {
                        let _ = writeln!(out, "{pad}{key}:");
                        write_block(val, indent + 2, out);
                    }
                    Yaml::Seq(inner) if !inner.is_empty() => {
                        let _ = writeln!(out, "{pad}{key}:");
                        write_block(val, indent, out);
                    }
                    _ => {
                        let _ = writeln!(out, "{pad}{key}: {}", inline(val));
                    }
                }
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                match item {
                    Yaml::Map(inner) if !inner.is_empty() => {
                        let mut sub = String::new();
                        write_block(item, indent + 2, &mut sub);
                        // The first key goes on the dash's line.
                        let sub = sub.replacen(&format!("{pad}  "), &format!("{pad}- "), 1);
                        out.push_str(&sub);
                    }
                    Yaml::Seq(inner) if !inner.is_empty() => {
                        let _ = writeln!(out, "{pad}-");
                        write_block(item, indent + 2, out);
                    }
                    _ => {
                        let _ = writeln!(out, "{pad}- {}", inline(item));
                    }
                }
            }
        }
        other => {
            let _ = writeln!(out, "{pad}{}", inline(other));
        }
    }
}

fn inline(v: &Yaml) -> String {
    match v {
        Yaml::Null => "null".into(),
        Yaml::Bool(b) => b.to_string(),
        Yaml::Int(i) => i.to_string(),
        Yaml::Float(f) => fmt_float(*f),
        Yaml::Str(s) => quote_str(s),
        Yaml::Seq(s) if s.is_empty() => "[]".into(),
        Yaml::Map(m) if m.is_empty() => "{}".into(),
        Yaml::Seq(s) => format!("[{}]", s.iter().map(inline).collect::<Vec<_>>().join(", ")),
        Yaml::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}: {}", quote_key(k), inline(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn needs_quotes(s: &str) -> bool {
    if s.is_empty() || s.trim() != s {
        return true;
    }
    if matches!(
        s,
        "null" | "Null" | "NULL" | "~" | "true" | "True" | "TRUE" | "false" | "False" | "FALSE"
            | "yes" | "no" | "on" | "off" | "y" | "n"
    ) {
        return true;
    }
    if s.parse::<f64>().is_ok() || s.parse::<i64>().is_ok() {
        return true;
    }
    let first = s.chars().next().unwrap_or(' ');
    if "-?:,[]{}#&*!|>'\"%@`=".contains(first) {
        return true;
    }
    s.contains(": ") || s.contains(" #") || s.ends_with(':') || s.contains('\n')
}

fn quote_str(s: &str) -> String {
    if s.contains('\n') || s.chars().any(|c| c.is_control()) {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => {
                    let _ = write!(out, "\\u{:04x}", c as u32);
                }
                c => out.push(c),
            }
        }
        out.push('"');
        out
    } else if needs_quotes(s) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        s.to_owned()
    }
}

fn quote_key(k: &str) -> String {
    // Bukkit quotes keys that would read back as numbers ('1': ...).
    if k == "==" {
        return "==".into();
    }
    quote_str(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# a comment
factions:
  player:
    members:
    - Alice
    - Bob
    enemies:
    - bots
    allies: []
    peace: []
  team1:
    members: []
    formation:
      shape: rows
    settings:
      melee-standoff: 2.5
      revenge: false
      swap-cooldown: 30
";

    #[test]
    fn reads_bukkit_style_mappings_and_sequences() {
        let y = parse(SAMPLE).unwrap();
        let p = y.path("factions.player").unwrap();
        assert_eq!(p.get("members").unwrap().strings(), vec!["Alice", "Bob"]);
        assert_eq!(p.get("allies").unwrap(), &Yaml::Seq(vec![]));
        let s = y.path("factions.team1.settings").unwrap();
        assert_eq!(s.get("melee-standoff").unwrap().as_f64(), Some(2.5));
        assert_eq!(s.get("revenge").unwrap().as_bool(), Some(false));
        assert_eq!(s.get("swap-cooldown").unwrap().as_i64(), Some(30));
        assert_eq!(y.path("factions.team1.formation.shape").unwrap().as_str(), Some("rows"));
    }

    #[test]
    fn round_trips_through_the_writer() {
        let y = parse(SAMPLE).unwrap();
        let text = dump(&y);
        assert_eq!(parse(&text).unwrap(), y);
    }

    #[test]
    fn reads_sequences_of_maps_with_null_items() {
        let src = "\
kits:
  k:
    contents:
    - null
    - ==: org.bukkit.inventory.ItemStack
      DataVersion: 4790
      id: minecraft:diamond_sword
      count: 1
      components:
        minecraft:enchantments: '{\"minecraft:sharpness\":2}'
      schema_version: 1
    armour: []
";
        let y = parse(src).unwrap();
        let c = y.path("kits.k.contents").unwrap().as_seq().unwrap();
        assert_eq!(c[0], Yaml::Null);
        assert_eq!(c[1].get("==").unwrap().as_str(), Some("org.bukkit.inventory.ItemStack"));
        assert_eq!(c[1].get("id").unwrap().as_str(), Some("minecraft:diamond_sword"));
        assert_eq!(
            c[1].path("components").unwrap().get("minecraft:enchantments").unwrap().as_str(),
            Some("{\"minecraft:sharpness\":2}")
        );
        let again = parse(&dump(&y)).unwrap();
        assert_eq!(again, y);
    }

    #[test]
    fn quoted_keys_and_values() {
        let y = parse("paths:\n  '1':\n    name: '1'\n    note: 'it''s'\n    x: -200.36756011576932\n").unwrap();
        let p = y.path("paths").unwrap().get("1").unwrap();
        assert_eq!(p.get("name").unwrap().as_str(), Some("1"));
        assert_eq!(p.get("note").unwrap().as_str(), Some("it's"));
        assert_eq!(p.get("x").unwrap().as_f64(), Some(-200.36756011576932));
        let text = dump(&y);
        assert!(text.contains("'1':"), "{text}");
        assert!(text.contains("name: '1'"), "{text}");
        assert_eq!(parse(&text).unwrap(), y);
    }

    #[test]
    fn comments_inside_quotes_are_kept() {
        let y = parse("a: 'x # y'\nb: c # d\n").unwrap();
        assert_eq!(y.get("a").unwrap().as_str(), Some("x # y"));
        assert_eq!(y.get("b").unwrap().as_str(), Some("c"));
    }

    #[test]
    fn strings_that_look_like_other_types_are_quoted() {
        let mut y = Yaml::map();
        y.set("a", Yaml::Str("true".into()));
        y.set("b", Yaml::Str("12".into()));
        y.set("c", Yaml::Str("".into()));
        y.set("d", Yaml::Str("plain words".into()));
        let text = dump(&y);
        assert_eq!(parse(&text).unwrap(), y, "{text}");
        assert!(text.contains("d: plain words"));
    }

    #[test]
    fn flow_sequences_and_nested_sequences() {
        let y = parse("a: [1, two, 'th,ree']\nb:\n- - x\n  - y\n").unwrap();
        assert_eq!(
            y.get("a").unwrap(),
            &Yaml::Seq(vec![Yaml::Int(1), Yaml::Str("two".into()), Yaml::Str("th,ree".into())])
        );
        assert_eq!(
            y.get("b").unwrap(),
            &Yaml::Seq(vec![Yaml::Seq(vec![Yaml::Str("x".into()), Yaml::Str("y".into())])])
        );
    }

    #[test]
    fn indented_sequences_also_read() {
        let y = parse("scripts:\n  raiders:\n    - horizonbot spawn 24\n    - wait 40\n").unwrap();
        assert_eq!(
            y.path("scripts.raiders").unwrap().strings(),
            vec!["horizonbot spawn 24", "wait 40"]
        );
    }
}
