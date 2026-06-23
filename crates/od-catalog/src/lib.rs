//! od-catalog — skills + design systems + design templates catalog (V2 step 1).
//!
//! Owns the read-only catalog routes `GET /api/skills`,
//! `GET /api/design-systems`, and `GET /api/design-templates`. These are pure
//! file walks over `SKILL.md` / `DESIGN.md` folders plus front-matter parsing of
//! the **vendored upstream content** (read-only data — this crate never writes).
//!
//! CP4 migrates these three routes from the Node daemon to Rust as the first
//! `Native` entries in the route table. Output is **byte-identical** to the
//! daemon (modulo array ordering, which the golden harness normalizes by `id`),
//! so the modules here port the daemon's exact parsing/normalization logic
//! rather than using a generic YAML library:
//!
//! - [`frontmatter`] — the daemon's bespoke YAML-subset parser.
//! - [`skills`] — `listSkills` → `/api/skills` + `/api/design-templates`.
//! - [`design_systems`] — `listDesignSystems` → `/api/design-systems`.
//!
//! `/api/templates` (the contract's third "catalog" name) is actually the
//! SQLite-backed user template store in the daemon, so it stays proxied and is
//! migrated with `od-store` (V2 step 2), not here.

mod frontmatter;
mod jsutil;
mod swift_colors;

pub mod design_systems;
pub mod skills;

use std::path::{Path, PathBuf};

use serde::Serialize;

pub use design_systems::DesignSystemSummary;
pub use skills::SkillSummary;

/// The on-disk roots each catalog route scans, mirroring the daemon's
/// `SKILL_ROOTS` / `DESIGN_TEMPLATE_ROOTS` / design-system roots (user root
/// first so user content shadows built-in by id).
#[derive(Clone, Debug)]
pub struct CatalogRoots {
    /// `[USER_SKILLS_DIR, SKILLS_DIR]`.
    pub skill_roots: Vec<PathBuf>,
    /// `[USER_DESIGN_TEMPLATES_DIR, DESIGN_TEMPLATES_DIR]`.
    pub design_template_roots: Vec<PathBuf>,
    /// `DESIGN_SYSTEMS_DIR` (built-in).
    pub design_systems_builtin: PathBuf,
    /// `USER_DESIGN_SYSTEMS_DIR`.
    pub design_systems_user: PathBuf,
}

impl CatalogRoots {
    /// Derive the roots from the vendored content root (`OD_RESOURCE_ROOT`) and
    /// the runtime data dir (`OD_DATA_DIR`), exactly as the daemon does.
    pub fn new(content_root: impl AsRef<Path>, data_dir: impl AsRef<Path>) -> Self {
        let content = content_root.as_ref();
        let data = data_dir.as_ref();
        Self {
            skill_roots: vec![data.join("skills"), content.join("skills")],
            design_template_roots: vec![
                data.join("design-templates"),
                content.join("design-templates"),
            ],
            design_systems_builtin: content.join("design-systems"),
            design_systems_user: data.join("design-systems"),
        }
    }
}

#[derive(Serialize)]
struct SkillsPayload {
    skills: Vec<SkillSummary>,
}

#[derive(Serialize)]
struct DesignTemplatesPayload {
    #[serde(rename = "designTemplates")]
    design_templates: Vec<SkillSummary>,
}

#[derive(Serialize)]
struct DesignSystemsPayload {
    #[serde(rename = "designSystems")]
    design_systems: Vec<DesignSystemSummary>,
}

/// Serialized body of `GET /api/skills` — `{"skills":[...]}`.
pub fn skills_json(roots: &CatalogRoots) -> String {
    let skills = skills::list_skills(&roots.skill_roots);
    serde_json::to_string(&SkillsPayload { skills }).expect("serialize skills")
}

/// Serialized body of `GET /api/design-templates` — `{"designTemplates":[...]}`.
pub fn design_templates_json(roots: &CatalogRoots) -> String {
    let design_templates = skills::list_skills(&roots.design_template_roots);
    serde_json::to_string(&DesignTemplatesPayload { design_templates })
        .expect("serialize design templates")
}

/// Serialized body of `GET /api/design-systems` — `{"designSystems":[...]}`.
pub fn design_systems_json(roots: &CatalogRoots) -> String {
    let design_systems = design_systems::list_design_systems(
        &roots.design_systems_builtin,
        &roots.design_systems_user,
    );
    serde_json::to_string(&DesignSystemsPayload { design_systems })
        .expect("serialize design systems")
}
