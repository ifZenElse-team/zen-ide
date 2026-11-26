use std::collections::HashMap;
use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_project::osv::{Range, ranges_of, severity_of};

pub use zen_project::osv::Severity;

use super::proxy::Fetch;
use crate::compare_versions;

pub const DEFAULT_DATABASE: &str = "https://vuln.go.dev";

pub const DEFAULT_OSV: &str = "https://api.osv.dev/v1/vulns";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexVuln {
    pub id: String,
    pub modified: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub modified: String,
    pub modules: HashMap<String, Vec<IndexVuln>>,
}

impl Index {
    pub fn parse(modified: String, modules_json: &[u8]) -> Result<Index, String> {
        #[derive(Deserialize)]
        struct Module {
            path: String,
            #[serde(default)]
            vulns: Vec<IndexVuln>,
        }
        let modules: Vec<Module> = serde_json::from_slice(modules_json).map_err(|error| error.to_string())?;
        Ok(Index { modified, modules: modules.into_iter().map(|module| (module.path, module.vulns)).collect() })
    }

    pub fn candidates(&self, module: &str, version: &str) -> Vec<&IndexVuln> {
        self.modules
            .get(module)
            .into_iter()
            .flatten()
            .filter(|vuln| {
                vuln.fixed.as_deref().is_none_or(|fixed| compare_versions(version, &canonical(fixed)).is_lt())
            })
            .collect()
    }
}

pub fn fetch_index(fetch: &dyn Fetch, base: &str, known: Option<&Index>) -> Result<Index, String> {
    let db = fetch.get(&format!("{base}/index/db.json"))?.ok_or("the vulnerability database has no index")?;
    let modified = serde_json::from_slice::<Value>(&db)
        .ok()
        .and_then(|db| db["modified"].as_str().map(str::to_owned))
        .ok_or("the vulnerability database's index cannot be read")?;
    if let Some(known) = known
        && known.modified == modified
    {
        return Ok(known.clone());
    }
    let packed =
        fetch.get(&format!("{base}/index/modules.json.gz"))?.ok_or("the vulnerability database has no module index")?;
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(packed.as_slice())
        .take(256 << 20)
        .read_to_end(&mut json)
        .map_err(|error| format!("the module index cannot be unpacked: {error}"))?;
    Index::parse(modified, &json)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub modified: String,
    pub aliases: Vec<String>,
    pub summary: String,
    pub details: String,
    pub ranges: HashMap<String, Vec<Range>>,
    pub url: String,
}

impl Entry {
    pub fn parse(json: &[u8]) -> Result<Entry, String> {
        let osv: Value = serde_json::from_slice(json).map_err(|error| error.to_string())?;
        let strings = |value: &Value| -> Vec<String> {
            value.as_array().into_iter().flatten().filter_map(|item| item.as_str().map(str::to_owned)).collect()
        };
        let mut ranges: HashMap<String, Vec<Range>> = HashMap::new();
        for affected in osv["affected"].as_array().into_iter().flatten() {
            let Some(module) = affected["package"]["name"].as_str() else { continue };
            let found = ranges_of(affected, "SEMVER", &canonical);
            if !found.is_empty() {
                ranges.entry(module.to_owned()).or_default().extend(found);
            }
        }
        let id = osv["id"].as_str().ok_or("an entry without an id")?.to_owned();
        let url = osv["database_specific"]["url"]
            .as_str()
            .map_or_else(|| format!("https://pkg.go.dev/vuln/{id}"), str::to_owned);
        Ok(Entry {
            modified: osv["modified"].as_str().unwrap_or_default().to_owned(),
            aliases: strings(&osv["aliases"]),
            summary: osv["summary"].as_str().unwrap_or_default().to_owned(),
            details: osv["details"].as_str().unwrap_or_default().to_owned(),
            ranges,
            url,
            id,
        })
    }

    pub fn affects(&self, module: &str, version: &str) -> bool {
        self.ranges.get(module).into_iter().flatten().any(|range| range.contains(version, compare_versions))
    }

    pub fn fixes(&self, module: &str) -> Vec<String> {
        self.ranges.get(module).into_iter().flatten().filter_map(|range| range.fixed.clone()).collect()
    }

    pub fn shown_id(&self) -> &str {
        self.aliases.iter().find(|alias| alias.starts_with("CVE-")).unwrap_or(&self.id)
    }

    pub fn advisory(&self) -> Option<&str> {
        self.aliases.iter().find(|alias| alias.starts_with("GHSA-")).map(String::as_str)
    }
}

pub fn fetch_severity(fetch: &dyn Fetch, base: &str, id: &str) -> Result<Option<Severity>, String> {
    let Some(json) = fetch.get(&format!("{base}/{id}"))? else { return Ok(None) };
    let osv: Value = serde_json::from_slice(&json).map_err(|error| error.to_string())?;
    Ok(severity_of(&osv))
}

pub fn canonical(version: &str) -> String {
    if version.starts_with('v') { version.to_owned() } else { format!("v{version}") }
}

pub fn unaffected(module: &str, current: &str, entries: &[&Entry], newest: Option<&str>) -> Option<String> {
    let entries: Vec<&Entry> = entries.iter().copied().filter(|entry| !entry.fixes(module).is_empty()).collect();
    if entries.is_empty() {
        return None;
    }
    let mut candidates: Vec<String> = entries.iter().flat_map(|entry| entry.fixes(module)).collect();
    candidates.extend(newest.map(str::to_owned));
    candidates.retain(|candidate| compare_versions(candidate, current).is_gt());
    candidates.sort_by(|a, b| compare_versions(a, b));
    candidates.dedup();
    candidates.into_iter().find(|candidate| entries.iter().all(|entry| !entry.affects(module, candidate)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entry(id: &str, ranges: Value) -> Entry {
        let osv = json!({
            "id": id, "modified": "2025-01-01T00:00:00Z", "aliases": ["CVE-2024-1", "GHSA-aaaa-bbbb-cccc"],
            "summary": "Bad thing", "details": "Long story",
            "affected": [{ "package": { "name": "golang.org/x/crypto", "ecosystem": "Go" }, "ranges": [{ "type": "SEMVER", "events": ranges }] }],
        });
        Entry::parse(osv.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn ranges_decide_what_is_affected() {
        let early = entry("GO-1", json!([{ "introduced": "0" }, { "fixed": "0.31.0" }]));
        assert!(early.affects("golang.org/x/crypto", "v0.1.0"));
        assert!(!early.affects("golang.org/x/crypto", "v0.31.0"));
        assert!(!early.affects("golang.org/x/net", "v0.1.0"));
        assert_eq!(early.shown_id(), "CVE-2024-1");
        assert_eq!(early.advisory(), Some("GHSA-aaaa-bbbb-cccc"));
        assert_eq!(early.url, "https://pkg.go.dev/vuln/GO-1");
        let twice = entry(
            "GO-2",
            json!([{ "introduced": "0" }, { "fixed": "0.20.0" }, { "introduced": "0.25.0" }, { "fixed": "0.35.0" }]),
        );
        assert!(twice.affects("golang.org/x/crypto", "v0.10.0"));
        assert!(!twice.affects("golang.org/x/crypto", "v0.22.0"));
        assert!(twice.affects("golang.org/x/crypto", "v0.30.0"));
        let last = entry("GO-3", json!([{ "introduced": "0.40.0" }, { "last_affected": "0.41.0" }]));
        assert!(last.affects("golang.org/x/crypto", "v0.41.0") && !last.affects("golang.org/x/crypto", "v0.42.0"));
        let open = entry("GO-4", json!([{ "introduced": "1.0.0" }]));
        assert!(open.affects("golang.org/x/crypto", "v9.0.0") && !open.affects("golang.org/x/crypto", "v0.9.0"));
    }

    #[test]
    fn the_unaffected_version_is_the_oldest_safe_fix() {
        let a = entry("GO-1", json!([{ "introduced": "0" }, { "fixed": "0.31.0" }]));
        let b = entry(
            "GO-2",
            json!([{ "introduced": "0" }, { "fixed": "0.20.0" }, { "introduced": "0.25.0" }, { "fixed": "0.35.0" }]),
        );
        let module = "golang.org/x/crypto";
        assert_eq!(unaffected(module, "v0.1.0", &[&a, &b], Some("v0.43.0")).as_deref(), Some("v0.35.0"));
        assert_eq!(unaffected(module, "v0.1.0", &[&a], None).as_deref(), Some("v0.31.0"));
        let open = entry("GO-4", json!([{ "introduced": "0" }]));
        assert_eq!(unaffected(module, "v0.1.0", &[&open], Some("v0.43.0")), None);
        assert_eq!(
            unaffected(module, "v0.1.0", &[&a, &open], Some("v0.43.0")).as_deref(),
            Some("v0.31.0"),
            "the unfixed one is left"
        );
    }

    #[test]
    fn the_index_lists_the_candidates() {
        let modules = json!([{ "path": "golang.org/x/crypto", "vulns": [
            { "id": "GO-1", "modified": "2024-01-01T00:00:00Z", "fixed": "0.31.0" },
            { "id": "GO-2", "modified": "2024-01-01T00:00:00Z", "fixed": "0.0.0-20200220183623-bac4c82f6975" },
            { "id": "GO-3", "modified": "2024-01-01T00:00:00Z" },
        ] }]);
        let index = Index::parse("2026-10-08T22:31:09Z".to_owned(), modules.to_string().as_bytes()).unwrap();
        let ids: Vec<&str> =
            index.candidates("golang.org/x/crypto", "v0.1.0").iter().map(|vuln| vuln.id.as_str()).collect();
        assert_eq!(ids, ["GO-1", "GO-3"]);
        assert!(index.candidates("example.com/none", "v1.0.0").is_empty());
    }
}
