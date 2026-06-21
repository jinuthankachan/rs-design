//! od-catalog — skills + design systems + templates catalog.
//!
//! Owns `GET /api/skills`, `GET /api/design-systems`, `GET /api/templates`.
//! Pure file walks over `SKILL.md` / `DESIGN.md` folders (`walkdir`) + frontmatter
//! parsing (`gray_matter`). The reused content is read-only upstream data; this
//! crate only reads the folders. This is V2 *step 1*, migrated early in V1 (CP4)
//! to exercise the whole route-table + golden-test seam end to end.
//
// TODO(V2 step 1 / CP4): walkdir + gray_matter catalog reads matching upstream JSON.
