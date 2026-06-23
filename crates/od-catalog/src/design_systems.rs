//! Port of `apps/daemon/src/design-systems/index.ts` (`listDesignSystems`) plus
//! the `listAllDesignSystems` wiring from `server.ts`, producing the
//! `/api/design-systems` listing payload. Built-in systems carry a `manifest.json`
//! (title/category/summary) and a `DESIGN.md` (swatches/surface); user systems
//! add a `metadata.json` sidecar.
//!
//! Field order mirrors the daemon's object construction with `body` stripped, so
//! `serde_json` emits byte-identical JSON.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value as Json;

use crate::frontmatter::{parse_frontmatter, Value};
use crate::jsutil::slice_utf16;
use crate::swift_colors::extract_swift_colors;

/// One entry of the `/api/design-systems` listing.
#[derive(Serialize)]
pub struct DesignSystemSummary {
    pub id: String,
    pub title: String,
    pub category: String,
    pub summary: String,
    pub swatches: Vec<String>,
    pub surface: String,
    pub source: String,
    pub status: String,
    #[serde(rename = "isEditable")]
    pub is_editable: bool,
    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Json>,
    #[serde(rename = "projectId", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

#[derive(Default)]
struct ListOptions {
    id_prefix: &'static str,
    source: &'static str,
    is_editable: bool,
    default_status: &'static str,
}

#[derive(Default)]
struct UserMetadata {
    title: Option<String>,
    category: Option<String>,
    surface: Option<String>,
    status: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

/// `listAllDesignSystems()` — built-in systems (no prefix) followed by user
/// systems (`user:` prefix). The frontend sorts client-side; downstream golden
/// comparison normalizes by `id`, so ordering here is not load-bearing.
pub fn list_design_systems(
    builtin_root: impl AsRef<Path>,
    user_root: impl AsRef<Path>,
) -> Vec<DesignSystemSummary> {
    let mut built_in = list_one_root(
        builtin_root.as_ref(),
        &ListOptions {
            id_prefix: "",
            source: "built-in",
            is_editable: false,
            default_status: "published",
        },
    );
    // server.ts forces these three on built-ins regardless of any sidecar.
    for s in &mut built_in {
        s.source = "built-in".to_string();
        s.is_editable = false;
        s.status = "published".to_string();
    }

    let installed = list_one_root(
        user_root.as_ref(),
        &ListOptions {
            id_prefix: "user:",
            source: "user",
            is_editable: true,
            default_status: "draft",
        },
    );

    let seen: Vec<String> = built_in.iter().map(|s| s.id.clone()).collect();
    let mut user: Vec<DesignSystemSummary> = installed
        .into_iter()
        .filter(|s| s.source == "user")
        .collect();
    // `(b.updatedAt ?? '').localeCompare(a.updatedAt ?? '')` — newest first.
    user.sort_by(|a, b| {
        b.updated_at
            .clone()
            .unwrap_or_default()
            .cmp(&a.updated_at.clone().unwrap_or_default())
    });

    let mut out = user;
    out.extend(built_in);
    // The third spread (non-user installed not already seen) is empty for our
    // roots; user systems above already cover the user root.
    let _ = seen;
    out
}

fn list_one_root(root: &Path, options: &ListOptions) -> Vec<DesignSystemSummary> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !ft.is_dir() && !ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let brand_root = root.join(&name);
        let manifest = read_project_manifest(&brand_root, &name);
        let design_file = manifest
            .as_ref()
            .map(|m| m.design.clone())
            .unwrap_or_else(|| "DESIGN.md".to_string());
        let design_path = brand_root.join(&design_file);
        let meta = match fs::metadata(&design_path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let raw = match fs::read_to_string(&design_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let metadata = read_user_metadata(root, &name);
        let (frontmatter, body) = parse_frontmatter(&raw);

        let markdown_title = title_match(&body)
            .map(|t| clean_title(&t))
            .unwrap_or_default();
        let fallback_title = if !markdown_title.is_empty() {
            markdown_title
        } else {
            let fm = string_field(&frontmatter, "name");
            if !fm.is_empty() {
                fm
            } else {
                name.clone()
            }
        };
        let title = clean_title(
            metadata
                .title
                .clone()
                .or_else(|| manifest.as_ref().map(|m| m.name.clone()))
                .unwrap_or(fallback_title)
                .as_str(),
        );

        let frontmatter_category = string_field(&frontmatter, "category");
        let category_pre = metadata
            .category
            .clone()
            .or_else(|| manifest.as_ref().map(|m| m.category.clone()))
            .or_else(|| extract_category(&body))
            .unwrap_or(frontmatter_category);
        let category = if category_pre.is_empty() {
            "Uncategorized".to_string()
        } else {
            category_pre
        };

        let markdown_summary = summarize(&body);
        let markdown_swatches = extract_swatches(&body);
        let frontmatter_swatch_row = swatches_from_frontmatter(&frontmatter);
        let swatches = pick_final_swatch_row(frontmatter_swatch_row.as_ref(), &markdown_swatches);

        // `(manifest?.description?.trim() || markdownSummary) || frontmatter.description || ''`
        let manifest_desc = manifest
            .as_ref()
            .and_then(|m| m.description.as_ref())
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty());
        let summary = manifest_desc
            .or_else(|| non_empty(markdown_summary))
            .or_else(|| non_empty(string_field(&frontmatter, "description")))
            .unwrap_or_default();

        let surface = metadata
            .surface
            .clone()
            .or_else(|| extract_surface(&body))
            .or_else(|| frontmatter_surface(&frontmatter))
            .unwrap_or_else(|| "web".to_string());

        out.push(DesignSystemSummary {
            id: format!("{}{}", options.id_prefix, name),
            title,
            category,
            summary,
            swatches,
            surface,
            source: options.source.to_string(),
            status: metadata
                .status
                .clone()
                .unwrap_or_else(|| options.default_status.to_string()),
            is_editable: options.is_editable,
            created_at: metadata.created_at.clone(),
            updated_at: metadata.updated_at.clone(),
            provenance: None,
            project_id: None,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Manifest + metadata.
// ---------------------------------------------------------------------------

struct ProjectManifest {
    name: String,
    category: String,
    description: Option<String>,
    design: String,
}

fn read_project_manifest(brand_root: &Path, expected_id: &str) -> Option<ProjectManifest> {
    let raw = fs::read_to_string(brand_root.join("manifest.json")).ok()?;
    let parsed: Json = serde_json::from_str(&raw).ok()?;
    let obj = parsed.as_object()?;
    if obj.get("schemaVersion").and_then(|v| v.as_str()) != Some("od-design-system-project/v1") {
        return None;
    }
    if obj.get("id").and_then(|v| v.as_str()) != Some(expected_id) {
        return None;
    }
    let name = obj.get("name").and_then(|v| v.as_str())?;
    if name.trim().is_empty() {
        return None;
    }
    let category = obj.get("category").and_then(|v| v.as_str())?;
    if category.trim().is_empty() {
        return None;
    }
    let description = match obj.get("description") {
        None => None,
        Some(Json::String(s)) => Some(s.clone()),
        Some(_) => return None, // present but not a string → invalid manifest
    };
    let files = obj.get("files")?.as_object()?;
    if files.get("design").and_then(|v| v.as_str()) != Some("DESIGN.md") {
        return None;
    }
    if files.get("tokens").and_then(|v| v.as_str()) != Some("tokens.css") {
        return None;
    }
    let opt_ok = |key: &str, expected: &str| match files.get(key) {
        None => true,
        Some(Json::String(s)) => s == expected,
        Some(_) => false,
    };
    if !opt_ok("designTokens", "design-tokens.json")
        || !opt_ok("tailwind", "tailwind-v4.css")
        || !opt_ok("components", "components.html")
    {
        return None;
    }
    Some(ProjectManifest {
        name: name.to_string(),
        category: category.to_string(),
        description,
        design: "DESIGN.md".to_string(),
    })
}

fn read_user_metadata(root: &Path, id: &str) -> UserMetadata {
    let raw = match fs::read_to_string(root.join(id).join("metadata.json")) {
        Ok(s) => s,
        Err(_) => return UserMetadata::default(),
    };
    let parsed: Json = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return UserMetadata::default(),
    };
    let Some(obj) = parsed.as_object() else {
        return UserMetadata::default();
    };
    let str_field = |key: &str| obj.get(key).and_then(|v| v.as_str()).map(|s| s.to_string());
    UserMetadata {
        title: str_field("title"),
        category: str_field("category"),
        surface: str_field("surface").filter(|s| is_design_surface(s)),
        status: str_field("status").filter(|s| s == "draft" || s == "published"),
        created_at: str_field("createdAt"),
        updated_at: str_field("updatedAt"),
    }
}

// ---------------------------------------------------------------------------
// Markdown extraction.
// ---------------------------------------------------------------------------

/// `s || …` truthiness for strings: `""` is falsy.
fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn string_field(data: &Value, key: &str) -> String {
    match data.get(key) {
        Some(Value::Str(s)) => s.trim().to_string(),
        _ => String::new(),
    }
}

fn frontmatter_surface(data: &Value) -> Option<String> {
    let v = string_field(data, "surface").to_lowercase();
    if is_design_surface(&v) {
        Some(v)
    } else {
        None
    }
}

fn is_design_surface(v: &str) -> bool {
    matches!(v, "web" | "image" | "video" | "audio")
}

fn title_match(body: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^#\s+(.+?)\s*$").unwrap());
    RE.captures(body).map(|c| c[1].to_string())
}

fn extract_category(body: &str) -> Option<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?im)^>\s*Category:\s*(.+?)\s*$").unwrap());
    RE.captures(body).map(|c| c[1].to_string())
}

fn extract_surface(body: &str) -> Option<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?im)^>\s*Surface:\s*(.+?)\s*$").unwrap());
    let caps = RE.captures(body)?;
    let v = caps[1].trim().to_lowercase();
    if is_design_surface(&v) {
        Some(v)
    } else {
        None
    }
}

fn clean_title(raw: &str) -> String {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^Design System (Inspired by|for)\s+").unwrap());
    RE.replace(raw, "").trim().to_string()
}

fn summarize(body: &str) -> String {
    let lines: Vec<&str> = body.split('\n').map(strip_cr).collect();
    static H1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#\s+").unwrap());
    static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#{1,6}\s+").unwrap());
    let Some(first_h1) = lines.iter().position(|l| H1.is_match(l)) else {
        return String::new();
    };
    let after: &[&str] = &lines[first_h1 + 1..];
    let window_lines: &[&str] = match after.iter().position(|l| HEADING.is_match(l)) {
        Some(idx) => &after[..idx],
        None => after,
    };
    let joined = window_lines.join("\n");
    static CAT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^>\s*Category:.*$").unwrap());
    static SURF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^>\s*Surface:.*$").unwrap());
    static BQ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^>\s*").unwrap());
    let s = CAT.replace_all(&joined, "");
    let s = SURF.replace_all(&s, "");
    let s = BQ.replace_all(&s, "");
    let trimmed = s.trim();
    let first_para = trimmed.split("\n\n").next().unwrap_or("");
    slice_utf16(first_para, 240)
}

fn strip_cr(l: &str) -> &str {
    l.strip_suffix('\r').unwrap_or(l)
}

// ---------------------------------------------------------------------------
// Swatches.
// ---------------------------------------------------------------------------

struct ColorToken {
    name: String,
    value: String,
}

struct SwatchRow {
    values: Vec<String>,
    filled_all_slots: bool,
}

fn normalize_hex(raw: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#([0-9a-fA-F]{3,8})$").unwrap());
    let caps = RE.captures(raw.trim())?;
    let mut hex = caps[1].to_string();
    if hex.len() == 3 {
        hex = hex.chars().map(|c| format!("{c}{c}")).collect();
    } else if hex.len() == 4 {
        let doubled: String = hex.chars().map(|c| format!("{c}{c}")).collect();
        hex = doubled[..8].to_string();
    }
    Some(format!("#{}", hex.to_lowercase()))
}

fn extract_swatches(raw: &str) -> Vec<String> {
    let mut colors: Vec<ColorToken> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    static CLEAN1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[*_`]+").unwrap());
    static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
    let mut push = |name: &str, value: &str| {
        let clean_name = WS
            .replace_all(&CLEAN1.replace_all(name, ""), " ")
            .trim()
            .to_lowercase();
        let Some(v) = normalize_hex(value) else {
            return;
        };
        if clean_name.chars().count() > 60 {
            return;
        }
        let key = format!("{clean_name}|{v}");
        if seen.contains(&key) {
            return;
        }
        seen.push(key);
        colors.push(ColorToken {
            name: clean_name,
            value: v,
        });
    };

    static RE_A: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^[\s>*-]*\**\s*([A-Za-z][A-Za-z0-9 /&()+_-]{1,40}?)\s*[:：]?\s*\**\s*[:：]?\s*`?(#[0-9a-fA-F]{3,8})").unwrap()
    });
    for caps in RE_A.captures_iter(raw) {
        push(&caps[1], &caps[2]);
    }
    static RE_B: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\*\*([A-Za-z][A-Za-z0-9 /&()+_-]{1,40}?)\*\*\s*\(?\s*`?(#[0-9a-fA-F]{3,8})")
            .unwrap()
    });
    for caps in RE_B.captures_iter(raw) {
        push(&caps[1], &caps[2]);
    }
    static RE_C: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^[ \t]*\|(.+)\|[ \t]*$").unwrap());
    static HEX_CELL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"#[0-9a-fA-F]{3,8}\b").unwrap());
    static HEX_FIND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#[0-9a-fA-F]{3,8}").unwrap());
    static HEX_ANY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#[0-9a-fA-F]{3,8}").unwrap());
    static SEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-:\s]+$").unwrap());
    for caps in RE_C.captures_iter(raw) {
        let cells: Vec<String> = caps[1].split('|').map(|c| c.trim().to_string()).collect();
        let hex_cell = cells.iter().find(|c| HEX_CELL.is_match(c));
        let Some(hex_cell) = hex_cell else { continue };
        let hex = HEX_FIND.find(hex_cell).map(|m| m.as_str()).unwrap_or("");
        let name_cell = cells
            .iter()
            .find(|c| !c.is_empty() && !HEX_ANY.is_match(c) && !SEP.is_match(c));
        push(name_cell.map(|s| s.as_str()).unwrap_or(""), hex);
    }
    for token in extract_swift_colors(raw) {
        push(&token.name, &token.hex);
    }
    if colors.is_empty() {
        return Vec::new();
    }
    pick_swatch_row(&colors).values
}

fn swatches_from_frontmatter(data: &Value) -> Option<SwatchRow> {
    let Some(Value::Object(entries)) = data.get("colors") else {
        return None;
    };
    let mut colors: Vec<ColorToken> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
    for (name, value) in entries {
        let Value::Str(value) = value else { continue };
        let Some(hex) = normalize_hex(value) else {
            continue;
        };
        let clean_name = WS.replace_all(name, " ").trim().to_lowercase();
        let key = format!("{clean_name}|{hex}");
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        colors.push(ColorToken {
            name: clean_name,
            value: hex,
        });
    }
    if colors.is_empty() {
        return None;
    }
    Some(pick_swatch_row(&colors))
}

fn pick_final_swatch_row(frontmatter: Option<&SwatchRow>, markdown: &[String]) -> Vec<String> {
    if let Some(fm) = frontmatter {
        if fm.filled_all_slots {
            return fm.values.clone();
        }
    }
    if !markdown.is_empty() {
        return markdown.to_vec();
    }
    frontmatter.map(|f| f.values.clone()).unwrap_or_default()
}

fn pick_swatch_row(colors: &[ColorToken]) -> SwatchRow {
    let pick = |hints: &[&str]| -> Option<String> {
        for h in hints {
            if let Some(found) = colors.iter().find(|c| c.name.contains(h)) {
                return Some(found.value.clone());
            }
        }
        None
    };
    static NEUTRAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#[0-9a-f]{6}$").unwrap());
    let is_neutral = |hex: &str| -> bool {
        if !NEUTRAL.is_match(hex) {
            return false;
        }
        let r = i64::from_str_radix(&hex[1..3], 16).unwrap_or(0);
        let g = i64::from_str_radix(&hex[3..5], 16).unwrap_or(0);
        let b = i64::from_str_radix(&hex[5..7], 16).unwrap_or(0);
        r.max(g).max(b) - r.min(g).min(b) < 10
    };

    let bg_hit = pick(&[
        "page background",
        "background",
        "canvas",
        "paper",
        "surface",
    ]);
    let fg_hit = pick(&[
        "heading",
        "foreground",
        "ink",
        "fg",
        "text",
        "navy",
        "graphite",
    ]);
    let accent_hit = pick(&[
        "primary brand",
        "brand primary",
        "accent",
        "brand",
        "primary",
    ]);
    let support_hit = pick(&["border", "divider", "rule", "muted", "secondary", "subtle"]);

    let bg = bg_hit.clone().unwrap_or_else(|| "#ffffff".to_string());
    let fg = fg_hit.clone().unwrap_or_else(|| "#111111".to_string());
    let accent = accent_hit.clone().unwrap_or_else(|| {
        colors
            .iter()
            .find(|c| !is_neutral(&c.value))
            .map(|c| c.value.clone())
            .or_else(|| colors.first().map(|c| c.value.clone()))
            .unwrap_or_else(|| "#888888".to_string())
    });
    let support = support_hit.clone().unwrap_or_else(|| {
        colors
            .iter()
            .find(|c| is_neutral(&c.value) && c.value != bg && c.value != fg)
            .map(|c| c.value.clone())
            .unwrap_or_else(|| "#cccccc".to_string())
    });

    let filled_all_slots =
        bg_hit.is_some() && fg_hit.is_some() && accent_hit.is_some() && support_hit.is_some();
    SwatchRow {
        values: vec![bg, support, fg, accent],
        filled_all_slots,
    }
}
