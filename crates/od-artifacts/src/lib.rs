//! od-artifacts — artifact parser, save/lint, and export.
//!
//! Owns `/api/artifacts/*`. Parses the `<artifact>` format and handles export:
//! ZIP (`zip` crate) and Markdown are trivial; PDF becomes the webview's
//! print-to-PDF; PPTX is agent-written (mostly file handling).
//
// TODO(V2 steps 4 & 7): artifact parser + save/lint + export pipeline.
