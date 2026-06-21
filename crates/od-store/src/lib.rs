//! od-store — persistence (projects, conversations, messages, tabs, templates).
//!
//! Owns `/api/projects` and related persistence routes. Points `rusqlite` at the
//! *same* `.od/app.sqlite` schema the upstream daemon uses, so data is shared
//! during migration. `rusqlite` bundles SQLite, so V2 needs no native addon here
//! (one of the reasons the V1 `better-sqlite3` packaging pain disappears in V2).
//
// TODO(V2 step 2): rusqlite access over the upstream schema.
