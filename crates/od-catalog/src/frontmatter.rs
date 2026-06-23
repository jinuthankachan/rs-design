//! A faithful Rust port of the daemon's minimal YAML front-matter parser
//! (`apps/daemon/src/design-systems/frontmatter.ts`).
//!
//! The daemon ships its *own* dependency-free YAML subset parser, not a real
//! YAML library, so the CP4 catalog routes can only be byte-identical to the
//! daemon if we reproduce that exact parser — quirks included (block literals,
//! flow arrays, the `- key: val` object form, lazy frontmatter delimiters).
//! Using `serde_yaml`/`gray_matter` instead would diverge on the same inputs.
//!
//! Object insertion order is preserved (a `Vec<(String, Value)>`) because the
//! daemon's JSON key order mirrors the order keys were inserted while parsing.

/// An ordered front-matter value. Mirrors the JS `FrontmatterValue` union.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    /// Insertion-ordered object (the daemon relies on key order).
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Object field lookup by key (last write wins, like JS property access).
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// `parseFrontmatter(src)` → `(data, body)`.
pub fn parse_frontmatter(src: &str) -> (Value, String) {
    // `text = src.replace(/^﻿/, '')` — strip a single leading BOM.
    let text = src.strip_prefix('\u{feff}').unwrap_or(src);

    // `/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/`
    let Some((yaml, body)) = split_frontmatter(text) else {
        return (Value::Object(Vec::new()), text.to_string());
    };
    (parse_yaml_subset(yaml), body.to_string())
}

/// Reproduce the frontmatter delimiter regex with a lazy first-match scan.
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    // `^---\r?\n`
    let rest = text.strip_prefix("---")?;
    let start = if let Some(r) = rest.strip_prefix("\r\n") {
        text.len() - r.len()
    } else if let Some(r) = rest.strip_prefix('\n') {
        text.len() - r.len()
    } else {
        return None;
    };

    let bytes = text.as_bytes();
    let mut k = start;
    while let Some(rel) = text[k..].find('\n') {
        let nl = k + rel;
        // `\r?\n` — an optional CR is part of the delimiter, not the content.
        let content_end = if nl > start && bytes[nl - 1] == b'\r' {
            nl - 1
        } else {
            nl
        };
        let p = nl + 1;
        if text[p..].starts_with("---") {
            // `---\r?\n?` — consume the optional trailing CR/LF after the fence.
            let mut q = p + 3;
            if bytes.get(q) == Some(&b'\r') {
                q += 1;
            }
            if bytes.get(q) == Some(&b'\n') {
                q += 1;
            }
            return Some((&text[start..content_end], &text[q..]));
        }
        k = nl + 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Stack/arena port of `parseYamlSubset`.
// ---------------------------------------------------------------------------

enum Node {
    Scalar(Value),
    Array(Vec<usize>),
    Object(Vec<(String, usize)>),
}

struct Arena {
    nodes: Vec<Node>,
}

impl Arena {
    fn new() -> Self {
        Arena { nodes: Vec::new() }
    }
    fn alloc(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }
    fn new_object(&mut self) -> usize {
        self.alloc(Node::Object(Vec::new()))
    }
    fn new_array(&mut self) -> usize {
        self.alloc(Node::Array(Vec::new()))
    }
    fn scalar(&mut self, v: Value) -> usize {
        self.alloc(Node::Scalar(v))
    }
    fn is_array(&self, idx: usize) -> bool {
        matches!(self.nodes[idx], Node::Array(_))
    }
    /// `obj[key] = child` — replace in place if present, else append (JS order).
    fn object_set(&mut self, obj: usize, key: &str, child: usize) {
        if let Node::Object(entries) = &mut self.nodes[obj] {
            if let Some(slot) = entries.iter_mut().find(|(k, _)| k == key) {
                slot.1 = child;
            } else {
                entries.push((key.to_string(), child));
            }
        }
    }
    fn array_push(&mut self, arr: usize, child: usize) {
        if let Node::Array(items) = &mut self.nodes[arr] {
            items.push(child);
        }
    }
    /// Materialize an arena node into an owned, insertion-ordered `Value`.
    fn build(&self, idx: usize) -> Value {
        match &self.nodes[idx] {
            Node::Scalar(v) => v.clone(),
            Node::Array(items) => Value::Array(items.iter().map(|&c| self.build(c)).collect()),
            Node::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, c)| (k.clone(), self.build(*c)))
                    .collect(),
            ),
        }
    }
}

struct Frame {
    indent: i64,
    container: usize,
    key: Option<String>,
}

fn parse_yaml_subset(src: &str) -> Value {
    let mut arena = Arena::new();
    let root = arena.new_object();
    let mut stack = vec![Frame {
        indent: -1,
        container: root,
        key: None,
    }];

    let lines: Vec<&str> = split_lines(src);
    let mut i = 0usize;
    while i < lines.len() {
        let raw = lines[i];
        if is_blank_or_comment(raw) {
            i += 1;
            continue;
        }
        let indent = leading_ws_len(raw);

        while stack.len() > 1 && (indent as i64) <= stack[stack.len() - 1].indent {
            stack.pop();
        }
        let top = stack.len() - 1;
        let line = &raw[indent..];

        // --- Array item -----------------------------------------------------
        if let Some(after) = line.strip_prefix("- ") {
            let value = after.trim();
            if !arena.is_array(stack[top].container) {
                // Convert the pending key's value to an array on first `-`.
                if stack.len() >= 2 && stack[top].key.is_some() {
                    let parent = stack[top - 1].container;
                    let key = stack[top].key.clone().unwrap();
                    let new_arr = arena.new_array();
                    arena.object_set(parent, &key, new_arr);
                    stack[top].container = new_arr;
                } else {
                    i += 1;
                    continue;
                }
            }
            let container = stack[top].container;
            if let Some(colon_idx) = value.find(':') {
                let obj = arena.new_object();
                let key = value[..colon_idx].trim();
                let val_raw = value[colon_idx + 1..].trim();
                if !val_raw.is_empty() {
                    let s = arena.scalar(coerce(val_raw));
                    arena.object_set(obj, key, s);
                }
                arena.array_push(container, obj);
                stack.push(Frame {
                    indent: indent as i64,
                    container: obj,
                    key: None,
                });
            } else {
                let s = arena.scalar(coerce(value));
                arena.array_push(container, s);
            }
            i += 1;
            continue;
        }

        // --- key: value / key: | --------------------------------------------
        // `/^([^:]+):\s*(.*)$/` — at least one non-colon char before the colon.
        let Some(colon) = line.find(':') else {
            i += 1;
            continue;
        };
        if colon == 0 {
            i += 1;
            continue;
        }
        let key = line[..colon].trim().to_string();
        // `:\s*` — drop the whitespace immediately after the colon; the rest is
        // `(.*)` (NOT trailing-trimmed — coerce trims later).
        let val = trim_start_ws(&line[colon + 1..]);

        if val.is_empty() {
            let child = arena.new_object();
            arena.object_set(stack[top].container, &key, child);
            stack.push(Frame {
                indent: indent as i64,
                container: child,
                key: Some(key),
            });
            i += 1;
            continue;
        }

        if val == "|" || val == "|-" || val == ">" || val == ">-" {
            let mut collected: Vec<String> = Vec::new();
            let child_indent = indent + 2;
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                if next.chars().all(|c| c.is_whitespace()) {
                    // `/^\s*$/` — a blank line keeps an empty entry.
                    collected.push(String::new());
                    i += 1;
                    continue;
                }
                let n_indent = leading_ws_len(next);
                if n_indent < child_indent {
                    break;
                }
                collected.push(next[child_indent..].to_string());
                i += 1;
            }
            let joined = trim_end_ws(&collected.join("\n"));
            let child = arena.scalar(Value::Str(joined));
            arena.object_set(stack[top].container, &key, child);
            // No `i` increment — the inner loop already advanced it.
            continue;
        }

        if val == "[]" {
            let child = arena.new_array();
            arena.object_set(stack[top].container, &key, child);
            i += 1;
            continue;
        }

        if val.starts_with('[') && val.ends_with(']') && val.len() >= 2 {
            let inner = &val[1..val.len() - 1];
            let arr = arena.new_array();
            for piece in inner.split(',') {
                let coerced = coerce(piece.trim());
                // `.filter((v) => v !== '')` — drop empty-string items only.
                if matches!(&coerced, Value::Str(s) if s.is_empty()) {
                    continue;
                }
                let c = arena.scalar(coerced);
                arena.array_push(arr, c);
            }
            arena.object_set(stack[top].container, &key, arr);
            i += 1;
            continue;
        }

        let child = arena.scalar(coerce(val));
        arena.object_set(stack[top].container, &key, child);
        i += 1;
    }

    arena.build(root)
}

/// `src.split(/\r?\n/)`.
fn split_lines(src: &str) -> Vec<&str> {
    // Splitting on '\n' then stripping a trailing '\r' reproduces `/\r?\n/`.
    src.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// `/^\s*(#.*)?$/` — whitespace-only, or whitespace then a `#` comment.
fn is_blank_or_comment(line: &str) -> bool {
    let t = line.trim_start_matches(is_ws);
    t.is_empty() || t.starts_with('#')
}

fn is_ws(c: char) -> bool {
    c.is_whitespace()
}

/// `raw.match(/^\s*/)[0].length` — leading-whitespace length in bytes (the
/// daemon counts characters, but YAML indentation is ASCII spaces/tabs so the
/// two agree; we use byte length so it doubles as a slice offset).
fn leading_ws_len(raw: &str) -> usize {
    raw.len() - raw.trim_start_matches(is_ws).len()
}

fn trim_start_ws(s: &str) -> &str {
    s.trim_start_matches(is_ws)
}

fn trim_end_ws(s: &str) -> String {
    s.trim_end_matches(is_ws).to_string()
}

/// `coerce(raw)` — scalar coercion matching the daemon's YAML 1.2 core subset.
fn coerce(raw: &str) -> Value {
    let v = raw.trim();
    let chars: Vec<char> = v.chars().collect();
    if chars.len() >= 2 {
        let first = chars[0];
        let last = chars[chars.len() - 1];
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            // `v.slice(1, -1)` — strip the matching quotes (ASCII, 1 byte each).
            return Value::Str(v[1..v.len() - 1].to_string());
        }
    }
    if v == "true" {
        return Value::Bool(true);
    }
    if v == "false" {
        return Value::Bool(false);
    }
    if v == "null" || v == "~" {
        return Value::Null;
    }
    if is_int(v) {
        if let Ok(n) = v.parse::<i64>() {
            return Value::Int(n);
        }
        if let Ok(f) = v.parse::<f64>() {
            return Value::Float(f);
        }
    }
    if is_float(v) {
        if let Ok(f) = v.parse::<f64>() {
            return Value::Float(f);
        }
    }
    Value::Str(v.to_string())
}

/// `/^-?\d+$/`
fn is_int(v: &str) -> bool {
    let body = v.strip_prefix('-').unwrap_or(v);
    !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit())
}

/// `/^-?\d*\.\d+$/`
fn is_float(v: &str) -> bool {
    let body = v.strip_prefix('-').unwrap_or(v);
    let Some(dot) = body.find('.') else {
        return false;
    };
    let (int_part, frac_part) = (&body[..dot], &body[dot + 1..]);
    int_part.bytes().all(|b| b.is_ascii_digit())
        && !frac_part.is_empty()
        && frac_part.bytes().all(|b| b.is_ascii_digit())
}
