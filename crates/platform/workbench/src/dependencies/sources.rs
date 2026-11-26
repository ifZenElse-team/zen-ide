use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_project::osv::{Range, ranges_of, severity_of};

use super::versions;

pub const SEARCH: &str = "https://search.maven.org/solrsearch/select";
pub const CENTRAL: &str = "https://repo.maven.apache.org/maven2";
pub const OSV: &str = "https://api.osv.dev/v1/query";

const ROWS: usize = 50;
const SHOWN: usize = 20;
#[cfg(not(target_arch = "wasm32"))]
const MAX_DOCUMENT: u64 = 16 << 20;
#[cfg(not(target_arch = "wasm32"))]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const MAX_PAGES: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub search: String,
    pub central: String,
    pub osv: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self { search: SEARCH.to_owned(), central: CENTRAL.to_owned(), osv: OSV.to_owned() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Unchanged,
    Missing,
    Changed { body: Vec<u8>, validators: Validators },
}

pub trait Transport: Send + Sync {
    fn get(&self, url: &str, validators: &Validators, authorization: Option<&str>) -> Result<Answer, String>;

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoNetwork;

impl Transport for NoNetwork {
    fn get(&self, url: &str, _: &Validators, _: Option<&str>) -> Result<Answer, String> {
        Err(format!("no network for {url}"))
    }

    fn post(&self, url: &str, _: &[u8]) -> Result<Vec<u8>, String> {
        Err(format!("no network for {url}"))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Http;

impl Transport for Http {
    #[cfg(not(target_arch = "wasm32"))]
    fn get(&self, url: &str, validators: &Validators, authorization: Option<&str>) -> Result<Answer, String> {
        use zen_update::fetch;
        if !fetch::allowed(url) {
            return Err(format!("{url} is not an HTTPS address"));
        }
        let validators =
            fetch::Validators { etag: validators.etag.clone(), last_modified: validators.last_modified.clone() };
        match fetch::conditional(url, MAX_DOCUMENT, TIMEOUT, &validators, authorization) {
            Ok(fetch::Conditional::Unchanged) => Ok(Answer::Unchanged),
            Ok(fetch::Conditional::Missing) => Ok(Answer::Missing),
            Ok(fetch::Conditional::Changed { body, validators }) => Ok(Answer::Changed {
                body,
                validators: Validators { etag: validators.etag, last_modified: validators.last_modified },
            }),
            Err(error) => Err(error.to_string()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        zen_update::fetch::post_json(url, body, MAX_DOCUMENT, TIMEOUT).map_err(|error| error.to_string())
    }

    #[cfg(target_arch = "wasm32")]
    fn get(&self, url: &str, _: &Validators, _: Option<&str>) -> Result<Answer, String> {
        Err(format!("no network for {url}"))
    }

    #[cfg(target_arch = "wasm32")]
    fn post(&self, url: &str, _: &[u8]) -> Result<Vec<u8>, String> {
        Err(format!("no network for {url}"))
    }
}

/// One repository dependencies are looked up in.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    /// Its id, as the build names it.
    pub id: String,
    /// Its address, without a slash at the end.
    pub url: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub authorization: Option<String>,
}

impl std::fmt::Debug for Repository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repository").field("id", &self.id).field("url", &self.url).finish_non_exhaustive()
    }
}

impl Repository {
    pub fn new(id: &str, url: &str) -> Self {
        Self { id: id.to_owned(), url: url.trim_end_matches('/').to_owned(), authorization: None }
    }

    pub fn basic(mut self, user: &str, password: &str) -> Self {
        use base64::Engine as _;
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
        self.authorization = Some(format!("Basic {token}"));
        self
    }

    pub fn is_secure(&self) -> bool {
        self.url.starts_with("https://")
            || ["http://localhost", "http://127.0.0.1"].iter().any(|local| self.url.starts_with(local))
    }

    pub fn is_public(&self) -> bool {
        self.authorization.is_none()
    }

    fn folder(&self, group: &str, artifact: &str) -> String {
        format!("{}/{}/{artifact}", self.url, group.replace('.', "/"))
    }

    pub fn metadata_url(&self, group: &str, artifact: &str) -> String {
        format!("{}/maven-metadata.xml", self.folder(group, artifact))
    }

    pub fn pom_url(&self, group: &str, artifact: &str, version: &str) -> String {
        format!("{}/{version}/{artifact}-{version}.pom", self.folder(group, artifact))
    }
}

fn is_plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~')
}

fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if is_plain(byte as char) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn safe_coordinates(parts: &[&str]) -> bool {
    parts.iter().all(|part| {
        !part.starts_with('.')
            && !part.contains("..")
            && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
    })
}

fn is_coordinate(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IndexQuery {
    Text(String),
    Coordinates { group: String, artifact: String },
    Group(String),
    Artifact { group: String, prefix: String },
}

impl IndexQuery {
    pub fn parse(typed: &str) -> Option<Self> {
        let typed = typed.trim();
        if typed.is_empty() {
            return None;
        }
        let parts: Vec<&str> = typed.split(':').collect();
        match parts.as_slice() {
            [group, artifact, ..] if is_coordinate(group) && is_coordinate(artifact) => {
                Some(Self::Coordinates { group: (*group).to_owned(), artifact: (*artifact).to_owned() })
            }
            _ => Some(Self::Text(typed.to_owned())),
        }
    }

    fn solr(&self) -> Option<String> {
        match self {
            Self::Text(text) if is_coordinate(text) => Some(format!("a:\"{text}\" OR {text}")),
            Self::Text(text) => Some(text.replace(['"', '\\'], " ")),
            Self::Coordinates { group, artifact } => Some(format!("g:\"{group}\" AND a:\"{artifact}\"")),
            Self::Group(prefix) => is_coordinate(prefix).then(|| format!("g:{prefix}*")),
            Self::Artifact { group, prefix } if prefix.is_empty() => {
                is_coordinate(group).then(|| format!("g:\"{group}\""))
            }
            Self::Artifact { group, prefix } => {
                (is_coordinate(group) && is_coordinate(prefix)).then(|| format!("g:\"{group}\" AND a:{prefix}*"))
            }
        }
    }

    pub fn url(&self, base: &str) -> Option<String> {
        Some(format!("{base}?q={}&rows={ROWS}&wt=json", encoded(&self.solr()?)))
    }
}

pub fn parse_search(body: &[u8]) -> Result<Vec<(String, String)>, String> {
    let json: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let docs = json["response"]["docs"].as_array().ok_or("the search index answered no documents")?;
    let mut ranked: Vec<&Value> = docs.iter().collect();
    ranked.sort_by_key(|doc| std::cmp::Reverse(doc["versionCount"].as_u64().unwrap_or(0)));
    Ok(ranked
        .into_iter()
        .filter_map(|doc| Some((doc["g"].as_str()?.to_owned(), doc["a"].as_str()?.to_owned())))
        .take(SHOWN)
        .collect())
}

pub fn parse_metadata(body: &[u8]) -> Result<Vec<String>, String> {
    let text = String::from_utf8_lossy(body);
    let document = roxmltree::Document::parse(&text).map_err(|error| error.to_string())?;
    if document.root_element().tag_name().name() != "metadata" {
        return Err("not a maven-metadata.xml".to_owned());
    }
    Ok(document
        .descendants()
        .filter(|node| {
            node.has_tag_name("version") && node.parent().is_some_and(|parent| parent.has_tag_name("versions"))
        })
        .filter_map(|node| node.text())
        .map(|version| version.trim().to_owned())
        .filter(|version| !version.is_empty())
        .collect())
}

/// A known vulnerability of a dependency.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShownAdvisory {
    /// The advisory's id: its CVE when it has one.
    pub id: String,
    /// Its one-line summary.
    pub summary: String,
    /// The page that describes it.
    pub url: String,
    /// The CVSS base score, when the advisory has a vector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// The rating the advisory gives, e.g. `HIGH`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Advisory {
    pub id: String,
    pub summary: String,
    pub url: String,
    pub score: Option<f64>,
    pub rating: Option<String>,
    pub ranges: Vec<Range>,
    pub versions: Vec<String>,
}

impl Advisory {
    pub fn shown(&self) -> ShownAdvisory {
        ShownAdvisory {
            id: self.id.clone(),
            summary: self.summary.clone(),
            url: self.url.clone(),
            score: self.score,
            rating: self.rating.clone(),
        }
    }

    pub fn affects(&self, version: &str) -> bool {
        self.versions.iter().any(|listed| listed == version)
            || self.ranges.iter().any(|range| range.contains(version, versions::compare))
    }

    fn parse(vulnerability: &Value, package: &str) -> Option<Self> {
        let id = vulnerability["id"].as_str()?;
        let aliases = vulnerability["aliases"].as_array().into_iter().flatten().filter_map(Value::as_str);
        let shown = aliases.into_iter().find(|alias| alias.starts_with("CVE-")).unwrap_or(id);
        let severity = severity_of(vulnerability).unwrap_or_default();
        let mut ranges = Vec::new();
        let mut listed = Vec::new();
        for affected in vulnerability["affected"].as_array().into_iter().flatten() {
            if affected["package"]["name"] != package {
                continue;
            }
            ranges.extend(ranges_of(affected, "ECOSYSTEM", &str::to_owned));
            listed.extend(
                affected["versions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned),
            );
        }
        let summary = vulnerability["summary"].as_str().or(vulnerability["details"].as_str()).unwrap_or_default();
        Some(Self {
            id: shown.to_owned(),
            summary: summary.lines().next().unwrap_or_default().to_owned(),
            url: format!("https://osv.dev/vulnerability/{id}"),
            score: severity.score,
            rating: severity.rating,
            ranges,
            versions: listed,
        })
    }
}

fn osv_query(package: &str, page: Option<&str>) -> Vec<u8> {
    let mut query = json!({ "package": { "name": package, "ecosystem": "Maven" } });
    if let Some(page) = page {
        query["page_token"] = json!(page);
    }
    query.to_string().into_bytes()
}

pub fn fetch_advisories(
    transport: &dyn Transport,
    url: &str,
    group: &str,
    artifact: &str,
) -> Result<Vec<Advisory>, String> {
    let package = format!("{group}:{artifact}");
    let mut found = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let body = transport.post(url, &osv_query(&package, page.as_deref()))?;
        let answer: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
        found.extend(
            answer["vulns"].as_array().into_iter().flatten().filter_map(|entry| Advisory::parse(entry, &package)),
        );
        page = answer["next_page_token"].as_str().filter(|token| !token.is_empty()).map(str::to_owned);
        if page.is_none() {
            break;
        }
    }
    Ok(found)
}

pub fn nearest_safe(current: &str, listed: &[String], advisories: &[&Advisory]) -> Option<String> {
    let mut candidates: Vec<&String> = listed
        .iter()
        .filter(|version| versions::offered_for(current, version))
        .filter(|version| versions::compare(version, current).is_gt())
        .filter(|version| advisories.iter().all(|advisory| !advisory.affects(version)))
        .collect();
    candidates.sort_by(|a, b| versions::compare(a, b));
    candidates.first().map(|version| (*version).clone())
}
