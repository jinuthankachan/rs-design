//! Port of `apps/daemon/src/design-systems/swift-colors.ts` — parse SwiftUI
//! `Color(...)` declarations into named hex swatches (Form D of swatch
//! extraction). Most DESIGN.md files have none, but the daemon runs this last so
//! we must reproduce it for byte-identical swatches on Swift-derived systems.

use std::sync::LazyLock;

use regex::Regex;

pub struct SwiftColorToken {
    pub name: String,
    pub hex: String,
}

/// `evalSwiftNumber(expr)` — a hex byte, decimal, integer, or single division.
fn eval_swift_number(expr: &str) -> Option<f64> {
    let parts: Vec<&str> = expr.split('/').collect();
    if parts.len() > 2 {
        return None;
    }
    static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^0x[0-9a-f]+$").unwrap());
    static DEC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[+-]?(?:\d+\.?\d*|\.\d+)$").unwrap());
    let mut values: Vec<f64> = Vec::new();
    for part in &parts {
        let token = part.trim();
        if HEX.is_match(token) {
            match i64::from_str_radix(&token[2..], 16) {
                Ok(n) => values.push(n as f64),
                Err(_) => return None,
            }
        } else if DEC.is_match(token) {
            match token.parse::<f64>() {
                Ok(f) => values.push(f),
                Err(_) => return None,
            }
        } else {
            return None;
        }
    }
    if values.len() == 1 {
        return Some(values[0]);
    }
    if values[1] == 0.0 {
        return None;
    }
    Some(values[0] / values[1])
}

fn clamp_unit(value: f64) -> f64 {
    // Swift color components arrive finite (via `eval_swift_number`), so `clamp`
    // matches the daemon's `Math.max(0, Math.min(1, value))`.
    value.clamp(0.0, 1.0)
}

fn byte_hex(unit: f64) -> String {
    // JS `Math.round` rounds half up; for the non-negative `clampUnit`*255 range
    // that matches `f64::round` (half away from zero).
    let n = (clamp_unit(unit) * 255.0).round() as i64;
    format!("{n:02x}")
}

fn rgb_unit_to_hex(red: f64, green: f64, blue: f64) -> String {
    format!("#{}{}{}", byte_hex(red), byte_hex(green), byte_hex(blue))
}

fn hsb_to_hex(hue: f64, saturation: f64, brightness: f64) -> String {
    let h = ((hue % 1.0) + 1.0) % 1.0;
    let s = clamp_unit(saturation);
    let v = clamp_unit(brightness);
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (red, green, blue) = match (i as i64).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    rgb_unit_to_hex(red, green, blue)
}

fn named_arg(args: &str, key: &str) -> Option<f64> {
    // `new RegExp("\\b" + key + "\\s*:\\s*([^,)]+)", "u")` — key is ASCII.
    let re = Regex::new(&format!(r"\b{key}\s*:\s*([^,)]+)")).ok()?;
    let caps = re.captures(args)?;
    eval_swift_number(caps.get(1)?.as_str().trim())
}

fn swift_color_args_to_hex(args: &str) -> Option<String> {
    let red = named_arg(args, "red");
    let green = named_arg(args, "green");
    let blue = named_arg(args, "blue");
    if let (Some(r), Some(g), Some(b)) = (red, green, blue) {
        return Some(rgb_unit_to_hex(r, g, b));
    }
    let hue = named_arg(args, "hue");
    let saturation = named_arg(args, "saturation");
    let brightness = named_arg(args, "brightness");
    if let (Some(h), Some(s), Some(v)) = (hue, saturation, brightness) {
        return Some(hsb_to_hex(h, s, v));
    }
    let white = named_arg(args, "white");
    if let Some(w) = white {
        return Some(rgb_unit_to_hex(w, w, w));
    }
    None
}

/// `extractSwiftColors(raw)`.
pub fn extract_swift_colors(raw: &str) -> Vec<SwiftColorToken> {
    // `[A-Za-z_]\w*` in JS is ASCII (`\w` stays ASCII even under the `u` flag);
    // spell it out so it stays ASCII in Rust's Unicode-by-default regex.
    static DECL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?:(?:static\s+|public\s+|private\s+|internal\s+)*(?:let|var)\s+([A-Za-z_][0-9A-Za-z_]*)\s*(?::[^=\n]+)?=\s*)?\bColor\s*\(([^)]*)\)",
        )
        .unwrap()
    });
    let mut tokens = Vec::new();
    for caps in DECL.captures_iter(raw) {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
        let args = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        if let Some(hex) = swift_color_args_to_hex(args) {
            tokens.push(SwiftColorToken { name, hex });
        }
    }
    tokens
}
