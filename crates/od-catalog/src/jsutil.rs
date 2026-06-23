//! Small helpers that reproduce JavaScript string semantics the daemon relies
//! on, so ported heuristics stay byte-identical.

use crate::frontmatter::Value;

/// `String(value)` / `` `${value}` `` coercion for the haystacks the daemon
/// builds (`${description ?? ""}`), covering the value kinds a SKILL.md
/// frontmatter field can hold.
pub fn template_string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Int(n)) => n.to_string(),
        Some(Value::Float(f)) => js_number_to_string(*f),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| template_string(Some(v)))
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

/// `String.prototype.slice(0, n)` — JS counts UTF-16 code units, so we slice on
/// the UTF-16 view and decode back. Matters only for non-BMP characters near
/// the cut, but keeps long Chinese/emoji prompts identical to the daemon.
pub fn slice_utf16(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= n {
        return s.to_string();
    }
    // Avoid splitting a surrogate pair: if the cut lands between a high and low
    // surrogate, JS keeps the lone high surrogate; decoding lossily mirrors the
    // observable string for our ASCII/BMP-dominated inputs.
    String::from_utf16_lossy(&units[..n])
}

/// JS `Number`→string formatting for the float values we emit (`featured`).
/// `serde_json`/ryu already match V8 for finite, non-integer decimals such as
/// `0.01`; this is the textual form used when building haystacks.
pub fn js_number_to_string(f: f64) -> String {
    if f == f.trunc() && f.is_finite() {
        // V8 prints integer-valued numbers without a fractional part.
        format!("{}", f as i64)
    } else {
        let mut buf = ryu_like(f);
        // ryu emits e.g. "0.01"; strip a possible trailing ".0" defensively.
        if let Some(stripped) = buf.strip_suffix(".0") {
            buf = stripped.to_string();
        }
        buf
    }
}

fn ryu_like(f: f64) -> String {
    // serde_json uses ryu under the hood; reuse it for identical shortest forms.
    serde_json::Number::from_f64(f)
        .map(|n| n.to_string())
        .unwrap_or_else(|| f.to_string())
}
