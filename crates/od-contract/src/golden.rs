//! Golden-fixture format + runner helpers — the machinery that gates every V2
//! `Proxy → Native` route flip (CONTRACT.md "Golden-test harness").
//!
//! A fixture is a pair of sidecar files captured from the **pinned** Node daemon
//! by `scripts/capture-golden.sh`:
//!
//! - `<name>.json` — the response **body**, verbatim.
//! - `<name>.meta.json` — the request (`method`, `path`), the expected `status`,
//!   a header **subset** to assert (`headers`), and, for the catalog list
//!   routes, the `arrayKey` whose array is order-normalized before comparison.
//!
//! ## Normalization (documented, deliberate)
//!
//! The daemon builds catalog listings in `readdir` order, which is **not**
//! deterministic across filesystems, so a raw byte comparison would be flaky for
//! reasons unrelated to correctness. [`normalize_by_id`] removes exactly that one
//! degree of freedom — it parses the body, sorts the array under `arrayKey` by
//! each element's `id`, and re-serializes — while leaving every field value, key,
//! and **key order** intact (object key order is preserved via
//! `serde_json/preserve_order`). Two bodies that differ in anything other than
//! array element order still compare unequal.

use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

/// A captured golden fixture: the expected response plus the request that
/// produced it and the normalization key.
#[derive(Debug, Clone)]
pub struct GoldenFixture {
    pub method: String,
    pub path: String,
    pub status: u16,
    /// Header subset to assert (lowercased names → expected values).
    pub headers: Vec<(String, String)>,
    /// The JSON array key to order-normalize on, if this is a list route.
    pub array_key: Option<String>,
    /// The expected response body, verbatim from the daemon.
    pub body: String,
}

#[derive(Deserialize)]
struct Meta {
    method: String,
    path: String,
    status: u16,
    #[serde(default)]
    headers: std::collections::BTreeMap<String, String>,
    #[serde(rename = "arrayKey")]
    array_key: Option<String>,
}

impl GoldenFixture {
    /// Load `<dir>/<name>.json` + `<dir>/<name>.meta.json`.
    pub fn load(dir: impl AsRef<Path>, name: &str) -> io::Result<Self> {
        let dir = dir.as_ref();
        let body = fs::read_to_string(dir.join(format!("{name}.json")))?;
        let meta_raw = fs::read_to_string(dir.join(format!("{name}.meta.json")))?;
        let meta: Meta = serde_json::from_str(&meta_raw)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(GoldenFixture {
            method: meta.method,
            path: meta.path,
            status: meta.status,
            headers: meta
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            array_key: meta.array_key,
            body,
        })
    }

    /// The expected body, normalized for comparison (order-normalized when this
    /// is a list route, verbatim otherwise).
    pub fn normalized_body(&self) -> String {
        match &self.array_key {
            Some(key) => normalize_by_id(&self.body, key),
            None => self.body.clone(),
        }
    }
}

/// Parse `body`, sort the array under `array_key` by each element's `id`, and
/// re-serialize. Object key order within each element is preserved.
pub fn normalize_by_id(body: &str, array_key: &str) -> String {
    let mut value: Value = serde_json::from_str(body).expect("golden body is valid JSON");
    if let Some(arr) = value.get_mut(array_key).and_then(Value::as_array_mut) {
        arr.sort_by_key(element_id);
    }
    serde_json::to_string(&value).expect("re-serialize normalized body")
}

fn element_id(v: &Value) -> String {
    v.get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
