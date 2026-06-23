//! Port of the daemon's `listSkills` (`apps/daemon/src/skills.ts`) producing the
//! `/api/skills` and `/api/design-templates` listing payloads. Both routes share
//! the same `SkillInfo` shape; the only difference is which roots are scanned.
//!
//! The listing strips `body`/`dir` and appends `hasBody`, exactly as the daemon
//! route handler does (`static-resource.ts`). Field order matches the daemon's
//! object construction so `serde_json` emits byte-identical JSON.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Number, Value as Json};

use crate::frontmatter::{parse_frontmatter, Value};
use crate::jsutil::{slice_utf16, template_string};

/// One entry of the `/api/skills` (or `/api/design-templates`) listing.
///
/// Field order is significant: it mirrors the daemon's `out.push({...})` order
/// (with `body`/`dir` stripped and `hasBody` appended) so the serialized JSON is
/// byte-identical.
#[derive(Serialize)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<Json>,
    pub description: String,
    #[serde(rename = "descriptionI18n", skip_serializing_if = "Option::is_none")]
    pub description_i18n: Option<Json>,
    pub triggers: Vec<Json>,
    pub mode: String,
    pub surface: String,
    pub source: String,
    #[serde(rename = "craftRequires")]
    pub craft_requires: Vec<String>,
    pub platform: Option<String>,
    pub scenario: String,
    pub category: Option<String>,
    #[serde(rename = "previewType")]
    pub preview_type: String,
    #[serde(rename = "designSystemRequired")]
    pub design_system_required: bool,
    #[serde(rename = "defaultFor")]
    pub default_for: Vec<String>,
    pub upstream: Option<String>,
    pub featured: Option<Number>,
    pub fidelity: Option<String>,
    #[serde(rename = "speakerNotes")]
    pub speaker_notes: Option<bool>,
    pub animations: Option<bool>,
    #[serde(rename = "examplePrompt")]
    pub example_prompt: String,
    #[serde(rename = "examplePromptI18n", skip_serializing_if = "Option::is_none")]
    pub example_prompt_i18n: Option<Json>,
    #[serde(rename = "aggregatesExamples")]
    pub aggregates_examples: bool,
    #[serde(rename = "critiquePolicy")]
    pub critique_policy: Option<String>,
    #[serde(rename = "hasBody")]
    pub has_body: bool,
}

/// `listSkills(roots)` — walk each root in priority order, the first root tagged
/// `user` and the rest `built-in`; an earlier root shadows a later one by id.
pub fn list_skills(roots: &[impl AsRef<Path>]) -> Vec<SkillSummary> {
    let mut out: Vec<SkillSummary> = Vec::new();
    let mut seen_ids: Vec<String> = Vec::new();

    for (root_idx, root) in roots.iter().enumerate() {
        let source = if root_idx == 0 { "user" } else { "built-in" };
        let entries = match fs::read_dir(root.as_ref()) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if !file_type.is_dir() && !file_type.is_symlink() {
                continue;
            }
            let dir = entry.path();
            let skill_path = dir.join("SKILL.md");
            let meta = match fs::metadata(&skill_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_file() {
                continue;
            }
            let raw = match fs::read_to_string(&skill_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let (data, body) = parse_frontmatter(&raw);

            let entry_name = entry.file_name().to_string_lossy().into_owned();
            let parent_id = match data.get("name") {
                Some(Value::Str(s)) if !s.is_empty() => s.clone(),
                _ => entry_name.clone(),
            };
            if seen_ids.iter().any(|id| id == &parent_id) {
                continue;
            }
            seen_ids.push(parent_id.clone());

            let has_attachments = dir_has_attachments(&dir);
            let od = data.get("od");
            let mode = normalize_mode(
                od.and_then(|o| o.get("mode")),
                &body,
                data.get("description"),
            );
            let surface = normalize_surface(od.and_then(|o| o.get("surface")), &mode);
            let platform = normalize_platform(
                od.and_then(|o| o.get("platform")),
                &mode,
                &body,
                data.get("description"),
            );
            let scenario = normalize_scenario(
                od.and_then(|o| o.get("scenario")),
                &body,
                data.get("description"),
            );
            let category = normalize_category(od.and_then(|o| o.get("category")));
            let design_system_required = match od
                .and_then(|o| o.get("design_system"))
                .and_then(|d| d.get("requires"))
            {
                Some(Value::Bool(b)) => *b,
                _ => true,
            };
            let upstream = match od.and_then(|o| o.get("upstream")) {
                Some(Value::Str(s)) => Some(s.clone()),
                _ => None,
            };
            let preview_type = match od
                .and_then(|o| o.get("preview"))
                .and_then(|p| p.get("type"))
            {
                Some(Value::Str(s)) => s.clone(),
                _ => "html".to_string(),
            };
            let description = match data.get("description") {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            };
            let display_name = localized_map_from_fields(data.get("en_name"), data.get("zh_name"));
            let description_i18n =
                localized_map_from_fields(data.get("en_description"), data.get("zh_description"));
            let example_prompt_i18n =
                localized_map_from_record(od.and_then(|o| o.get("example_prompt_i18n")));
            let triggers = match data.get("triggers") {
                Some(Value::Array(items)) => items.iter().map(fm_to_json).collect(),
                _ => Vec::new(),
            };
            let craft_requires = normalize_craft_requires(
                od.and_then(|o| o.get("craft"))
                    .and_then(|c| c.get("requires")),
            );
            let default_for = normalize_default_for(od.and_then(|o| o.get("default_for")));
            let featured = normalize_featured(od.and_then(|o| o.get("featured")));
            let fidelity = normalize_fidelity(od.and_then(|o| o.get("fidelity")));
            let speaker_notes = normalize_bool_hint(od.and_then(|o| o.get("speaker_notes")));
            let animations = normalize_bool_hint(od.and_then(|o| o.get("animations")));
            let example_prompt = derive_prompt(&data);
            let critique_policy = normalize_critique_policy(
                od.and_then(|o| o.get("critique"))
                    .and_then(|c| c.get("policy")),
            );

            // `hasBody` = parentBody.length > 0; parentBody gains a non-empty
            // preamble whenever the skill ships attachments, otherwise it is the
            // raw body. So the boolean is exactly `hasAttachments || body != ""`.
            let has_body = has_attachments || !body.is_empty();

            let derived = collect_derived_examples(&dir);
            let aggregates_examples = !derived.is_empty();

            out.push(SkillSummary {
                id: parent_id.clone(),
                name: parent_id.clone(),
                display_name: display_name.clone(),
                description: description.clone(),
                description_i18n: description_i18n.clone(),
                triggers: triggers.clone(),
                mode: mode.clone(),
                surface: surface.clone(),
                source: source.to_string(),
                craft_requires,
                platform: platform.clone(),
                scenario: scenario.clone(),
                category: category.clone(),
                preview_type: preview_type.clone(),
                design_system_required,
                default_for,
                upstream: upstream.clone(),
                featured: featured.clone(),
                fidelity: fidelity.clone(),
                speaker_notes,
                animations,
                example_prompt: example_prompt.clone(),
                example_prompt_i18n: example_prompt_i18n.clone(),
                aggregates_examples,
                critique_policy: critique_policy.clone(),
                has_body,
            });

            for key in derived {
                let derived_id = format!("{parent_id}:{key}");
                if seen_ids.iter().any(|id| id == &derived_id) {
                    continue;
                }
                seen_ids.push(derived_id.clone());
                out.push(SkillSummary {
                    id: derived_id,
                    name: humanize_example_name(&key),
                    display_name: display_name.clone(),
                    description: description.clone(),
                    description_i18n: description_i18n.clone(),
                    triggers: triggers.clone(),
                    mode: mode.clone(),
                    surface: surface.clone(),
                    source: source.to_string(),
                    craft_requires: Vec::new(),
                    platform: platform.clone(),
                    scenario: scenario.clone(),
                    category: category.clone(),
                    preview_type: preview_type.clone(),
                    design_system_required,
                    default_for: Vec::new(),
                    upstream: upstream.clone(),
                    featured: None,
                    fidelity: fidelity.clone(),
                    speaker_notes,
                    animations,
                    example_prompt: example_prompt.clone(),
                    example_prompt_i18n: example_prompt_i18n.clone(),
                    aggregates_examples: false,
                    critique_policy: critique_policy.clone(),
                    has_body,
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Derived examples (`<parent>:<child>`) + attachments.
// ---------------------------------------------------------------------------

fn collect_derived_examples(dir: &Path) -> Vec<String> {
    let examples_dir = dir.join("examples");
    let entries = match fs::read_dir(&examples_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !ft.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.to_lowercase().ends_with(".html") {
            continue;
        }
        let key = name[..name.len() - 5].to_string(); // strip ".html" (case-insensitive length is 5)
        if !is_safe_example_key(&key) {
            continue;
        }
        out.push(key);
    }
    // Sort order is irrelevant downstream (the catalog is normalized by `id`
    // before any golden comparison), so we keep a deterministic byte sort.
    out.sort();
    out
}

fn is_safe_example_key(key: &str) -> bool {
    if key.is_empty() || key.starts_with('.') {
        return false;
    }
    if key.contains(':') {
        return false;
    }
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._-]+$").unwrap());
    RE.is_match(key)
}

fn humanize_example_name(key: &str) -> String {
    static DASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_]+").unwrap());
    static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
    let spaced = DASH.replace_all(key, " ");
    let collapsed = WS.replace_all(&spaced, " ");
    collapsed
        .trim()
        .split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn dir_has_attachments(dir: &Path) -> bool {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\.(md|html|css|js|json|txt)$").unwrap());
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "SKILL.md" {
            continue;
        }
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        if is_dir || RE.is_match(&name) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Normalizers (1:1 with skills.ts).
// ---------------------------------------------------------------------------

fn infer_mode(body: &str, description: Option<&Value>) -> String {
    let hay = format!("{}\n{}", template_string(description), body).to_lowercase();
    static IMAGE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?-u)\bimage|poster|illustration|photography|图片|海报|插画").unwrap()
    });
    static VIDEO: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?-u)\bvideo|motion|shortform|animation|视频|动效|短片").unwrap()
    });
    static AUDIO: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?-u)\baudio|music|jingle|tts|sound|音频|音乐|配音|音效").unwrap()
    });
    static DECK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?-u)\bppt|deck|slide|presentation|幻灯|投影").unwrap());
    static DS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?-u)\bdesign[- ]system|\bdesign\.md|\bdesign tokens").unwrap()
    });
    static TEMPLATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u)\btemplate\b").unwrap());
    if IMAGE.is_match(&hay) {
        return "image".into();
    }
    if VIDEO.is_match(&hay) {
        return "video".into();
    }
    if AUDIO.is_match(&hay) {
        return "audio".into();
    }
    if DECK.is_match(&hay) {
        return "deck".into();
    }
    if DS.is_match(&hay) {
        return "design-system".into();
    }
    if TEMPLATE.is_match(&hay) {
        return "template".into();
    }
    "prototype".into()
}

fn normalize_mode(value: Option<&Value>, body: &str, description: Option<&Value>) -> String {
    if let Some(Value::Str(v)) = value {
        if matches!(
            v.as_str(),
            "image" | "video" | "audio" | "deck" | "design-system" | "template" | "prototype"
        ) {
            return v.clone();
        }
    }
    infer_mode(body, description)
}

fn normalize_surface(value: Option<&Value>, mode: &str) -> String {
    if let Some(Value::Str(s)) = value {
        let v = s.trim().to_lowercase();
        if matches!(v.as_str(), "web" | "image" | "video" | "audio") {
            return v;
        }
    }
    if matches!(mode, "image" | "video" | "audio") {
        return mode.to_string();
    }
    "web".to_string()
}

fn normalize_platform(
    value: Option<&Value>,
    mode: &str,
    body: &str,
    description: Option<&Value>,
) -> Option<String> {
    if let Some(Value::Str(v)) = value {
        if v == "desktop" || v == "mobile" {
            return Some(v.clone());
        }
    }
    if mode != "prototype" {
        return None;
    }
    let hay = format!("{}\n{}", template_string(description), body).to_lowercase();
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"mobile|phone|ios|android|手机|移动端").unwrap());
    if RE.is_match(&hay) {
        Some("mobile".to_string())
    } else {
        Some("desktop".to_string())
    }
}

fn normalize_category(value: Option<&Value>) -> Option<String> {
    let Some(Value::Str(s)) = value else {
        return None;
    };
    let slug = s.trim().to_lowercase();
    if slug.is_empty() {
        return None;
    }
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap());
    if !RE.is_match(&slug) {
        return None;
    }
    Some(slice_utf16(&slug, 64))
}

fn normalize_scenario(value: Option<&Value>, body: &str, description: Option<&Value>) -> String {
    if let Some(Value::Str(s)) = value {
        let v = s.trim().to_lowercase();
        if !v.is_empty() {
            return v;
        }
    }
    let hay = format!("{}\n{}", template_string(description), body).to_lowercase();
    static FIN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"finance|invoice|expense|budget|p&l|revenue").unwrap());
    static HR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?-u)\bhr\b|onboarding|payroll|employee|人事").unwrap());
    static MKT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"marketing|campaign|brand|landing").unwrap());
    static ENG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"runbook|incident|deploy|engineering|sre|api").unwrap());
    static PROD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"spec|prd|roadmap|product manager|product team").unwrap());
    static DSGN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"design system|moodboard|mockup|ui kit").unwrap());
    static SALES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"sales|quote|proposal|lead").unwrap());
    static OPS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"operations|ops|logistics|inventory").unwrap());
    if FIN.is_match(&hay) {
        return "finance".into();
    }
    if HR.is_match(&hay) {
        return "hr".into();
    }
    if MKT.is_match(&hay) {
        return "marketing".into();
    }
    if ENG.is_match(&hay) {
        return "engineering".into();
    }
    if PROD.is_match(&hay) {
        return "product".into();
    }
    if DSGN.is_match(&hay) {
        return "design".into();
    }
    if SALES.is_match(&hay) {
        return "sales".into();
    }
    if OPS.is_match(&hay) {
        return "operations".into();
    }
    "general".into()
}

fn normalize_craft_requires(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap());
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for v in items {
        let Value::Str(s) = v else { continue };
        let slug = s.trim().to_lowercase();
        if slug.is_empty() || !RE.is_match(&slug) {
            continue;
        }
        if seen.contains(&slug) {
            continue;
        }
        seen.push(slug.clone());
        out.push(slug);
    }
    out
}

fn normalize_default_for(value: Option<&Value>) -> Vec<String> {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(js_string).collect(),
        // The JS `!value` guard also treats 0 and "" as empty.
        Some(Value::Int(0)) => Vec::new(),
        Some(Value::Str(s)) if s.is_empty() => Vec::new(),
        Some(other) => vec![js_string(other)],
    }
}

fn normalize_fidelity(value: Option<&Value>) -> Option<String> {
    if let Some(Value::Str(v)) = value {
        if v == "wireframe" || v == "high-fidelity" {
            return Some(v.clone());
        }
    }
    None
}

fn normalize_bool_hint(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::Str(s)) => {
            let v = s.trim().to_lowercase();
            match v.as_str() {
                "true" | "yes" | "1" => Some(true),
                "false" | "no" | "0" => Some(false),
                _ => None,
            }
        }
        _ => None,
    }
}

fn normalize_featured(value: Option<&Value>) -> Option<Number> {
    match value {
        Some(Value::Bool(true)) => Some(Number::from(1)),
        Some(Value::Int(n)) => Some(Number::from(*n)),
        Some(Value::Float(f)) if f.is_finite() => Number::from_f64(*f),
        Some(Value::Str(s)) if !s.trim().is_empty() => {
            let t = s.trim();
            // JS `Number(t)`: integer-valued results print without a fraction.
            if let Ok(n) = t.parse::<i64>() {
                return Some(Number::from(n));
            }
            if let Ok(f) = t.parse::<f64>() {
                if f.is_finite() {
                    if f == f.trunc() {
                        return Some(Number::from(f as i64));
                    }
                    return Number::from_f64(f);
                }
            }
            None
        }
        _ => None,
    }
}

fn normalize_critique_policy(value: Option<&Value>) -> Option<String> {
    let Some(Value::Str(s)) = value else {
        return None;
    };
    let v = s.trim().to_lowercase();
    if v == "required" || v == "opt-in" || v == "opt-out" {
        Some(v)
    } else {
        None
    }
}

fn derive_prompt(data: &Value) -> String {
    if let Some(Value::Str(explicit)) = data.get("od").and_then(|o| o.get("example_prompt")) {
        if !explicit.trim().is_empty() {
            return explicit.trim().to_string();
        }
    }
    let desc = match data.get("description") {
        Some(Value::Str(s)) => s.trim().to_string(),
        _ => String::new(),
    };
    if desc.is_empty() {
        return String::new();
    }
    static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
    let collapsed = WS.replace_all(&desc, " ").trim().to_string();
    static SENTENCE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^.+?[.!?。！？](?:\s|$)").unwrap());
    let first = SENTENCE
        .find(&collapsed)
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty());
    let chosen = first.unwrap_or(collapsed);
    slice_utf16(&chosen, 320)
}

// ---------------------------------------------------------------------------
// i18n helpers + value conversion.
// ---------------------------------------------------------------------------

fn localized_map_from_fields(en: Option<&Value>, zh: Option<&Value>) -> Option<Json> {
    let mut out = Map::new();
    if let Some(Value::Str(s)) = en {
        if !s.trim().is_empty() {
            out.insert("en".to_string(), Json::String(s.trim().to_string()));
        }
    }
    if let Some(Value::Str(s)) = zh {
        if !s.trim().is_empty() {
            out.insert("zh-CN".to_string(), Json::String(s.trim().to_string()));
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Json::Object(out))
    }
}

fn localized_map_from_record(value: Option<&Value>) -> Option<Json> {
    let Some(Value::Object(entries)) = value else {
        return None;
    };
    let mut out = Map::new();
    for (key, raw) in entries {
        if let Value::Str(s) = raw {
            if !s.trim().is_empty() {
                out.insert(key.clone(), Json::String(s.trim().to_string()));
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Json::Object(out))
    }
}

/// `String(value)` for the `defaultFor` coercion.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => crate::jsutil::js_number_to_string(*f),
        Value::Str(s) => s.clone(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// Convert a frontmatter value into the JSON the daemon would emit (used for
/// `triggers`, which pass through verbatim).
fn fm_to_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(n) => Json::Number(Number::from(*n)),
        Value::Float(f) => Number::from_f64(*f).map(Json::Number).unwrap_or(Json::Null),
        Value::Str(s) => Json::String(s.clone()),
        Value::Array(items) => Json::Array(items.iter().map(fm_to_json).collect()),
        Value::Object(entries) => {
            let mut map = Map::new();
            for (k, v) in entries {
                map.insert(k.clone(), fm_to_json(v));
            }
            Json::Object(map)
        }
    }
}
