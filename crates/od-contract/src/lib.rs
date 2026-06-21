//! od-contract — shared request/response types and golden-test fixtures.
//!
//! This crate is the spine of the "sacred" HTTP/SSE API contract between the
//! frontend and the backend. During V2, every route migrated from the Node
//! daemon to Rust is asserted *byte-identical* against fixtures captured from
//! the pinned upstream daemon (see `tests/golden/`, `scripts/capture-golden.sh`).
//!
//! V1 (CP4): houses the first golden-fixture format + the shared types used by
//! `od-catalog`. Grows as each route is migrated.
//
// TODO(V2): shared DTOs + golden-fixture helpers, added as routes are migrated.
