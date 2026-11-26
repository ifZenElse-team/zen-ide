use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zen_i18n::t;
use zen_project::inspect::{
    DeclaredSetting, Finding, FindingSeverity, FixJob, InspectContext, InspectionFix, Inspector, Wake,
};

pub const CHECK: &str = "springBoot.supportEnd.check";

pub const URL: &str = "https://api.spring.io/projects/spring-boot/generations";

pub const HOST: &str = "api.spring.io";

const FRESH: Duration = Duration::from_secs(24 * 60 * 60);

const CACHE: &str = "generations.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    NotModified,
    Fresh { body: Vec<u8>, etag: Option<String>, last_modified: Option<String> },
}

pub trait Source: Send + Sync {
    fn get(&self, etag: Option<&str>, last_modified: Option<&str>) -> Result<Answer, String>;
}

pub struct Http;

impl Source for Http {
    #[cfg(not(target_arch = "wasm32"))]
    fn get(&self, etag: Option<&str>, last_modified: Option<&str>) -> Result<Answer, String> {
        match zen_update::fetch::conditional_json(URL, etag, last_modified, 4 << 20, Duration::from_secs(20)) {
            Ok(zen_update::fetch::JsonAnswer::NotModified) => Ok(Answer::NotModified),
            Ok(zen_update::fetch::JsonAnswer::Fresh { bytes, etag, last_modified }) => {
                Ok(Answer::Fresh { body: bytes, etag, last_modified })
            }
            Err(error) => Err(error.to_string()),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn get(&self, _etag: Option<&str>, _last_modified: Option<&str>) -> Result<Answer, String> {
        Err("no network in the browser".to_owned())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Generation {
    pub name: String,
    pub oss_support_end_date: Option<String>,
    pub commercial_support_end_date: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Embedded {
    generations: Vec<Generation>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Page {
    #[serde(rename = "_embedded")]
    embedded: Embedded,
}

pub fn parse(body: &[u8]) -> Result<Vec<Generation>, String> {
    let page: Page = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    Ok(page.embedded.generations)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Cached {
    generations: Vec<Generation>,
    etag: Option<String>,
    last_modified: Option<String>,
    fetched: u64,
}

#[derive(Default)]
struct State {
    cached: Option<Cached>,
    read_disk: bool,
    fetching: bool,
    failed_at: Option<u64>,
}

pub type Today = Arc<dyn Fn() -> String + Send + Sync>;

pub struct BootSupport {
    source: Arc<dyn Source>,
    state: Arc<Mutex<State>>,
    generation: Arc<AtomicU64>,
    wake: Arc<Mutex<Option<Wake>>>,
    cache: Mutex<Option<PathBuf>>,
    today: Today,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

pub fn date_of(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl BootSupport {
    pub fn new(source: Arc<dyn Source>) -> Self {
        Self {
            source,
            state: Arc::default(),
            generation: Arc::default(),
            wake: Arc::default(),
            cache: Mutex::new(None),
            today: Arc::new(|| date_of(now())),
        }
    }

    pub fn with_today(mut self, today: Today) -> Self {
        self.today = today;
        self
    }

    fn cache_file(&self) -> Option<PathBuf> {
        lock(&self.cache).as_ref().map(|dir| dir.join(CACHE))
    }

    fn read_disk(&self) {
        let file = self.cache_file();
        let mut state = lock(&self.state);
        if state.read_disk {
            return;
        }
        state.read_disk = true;
        if state.cached.is_none()
            && let Some(file) = file
            && let Ok(bytes) = std::fs::read(file)
            && let Ok(cached) = serde_json::from_slice::<Cached>(&bytes)
        {
            state.cached = Some(cached);
        }
    }

    pub fn fetch(&self) {
        let (etag, last_modified) = {
            let state = lock(&self.state);
            let cached = state.cached.as_ref();
            (cached.and_then(|cached| cached.etag.clone()), cached.and_then(|cached| cached.last_modified.clone()))
        };
        let answer = self.source.get(etag.as_deref(), last_modified.as_deref());
        let mut state = lock(&self.state);
        state.fetching = false;
        match answer {
            Ok(Answer::NotModified) => {
                if let Some(cached) = &mut state.cached {
                    cached.fetched = now();
                }
            }
            Ok(Answer::Fresh { body, etag, last_modified }) => match parse(&body) {
                Ok(generations) => {
                    state.cached = Some(Cached { generations, etag, last_modified, fetched: now() });
                    self.generation.fetch_add(1, Ordering::SeqCst);
                }
                Err(_) => state.failed_at = Some(now()),
            },
            Err(_) => state.failed_at = Some(now()),
        }
        let saved = state.cached.clone();
        drop(state);
        if let (Some(file), Some(cached)) = (self.cache_file(), saved) {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(file, serde_json::to_vec(&cached).unwrap_or_default());
        }
        if let Some(wake) = lock(&self.wake).clone() {
            wake();
        }
    }

    fn due(&self) -> bool {
        let state = lock(&self.state);
        let stale = state.cached.as_ref().is_none_or(|cached| now().saturating_sub(cached.fetched) >= FRESH.as_secs());
        let resting = state.failed_at.is_some_and(|at| now().saturating_sub(at) < 15 * 60);
        stale && !resting && !state.fetching
    }

    fn start_fetch(self: &BootSupport) {
        lock(&self.state).fetching = true;
        let worker = BootSupport {
            source: Arc::clone(&self.source),
            state: Arc::clone(&self.state),
            generation: Arc::clone(&self.generation),
            wake: Arc::clone(&self.wake),
            cache: Mutex::new(lock(&self.cache).clone()),
            today: Arc::clone(&self.today),
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || worker.fetch());
        #[cfg(target_arch = "wasm32")]
        worker.fetch();
    }

    pub fn generations(&self) -> Vec<Generation> {
        lock(&self.state).cached.as_ref().map(|cached| cached.generations.clone()).unwrap_or_default()
    }
}

fn find_tag(text: &str, from: usize, tag: &str) -> Option<Range<usize>> {
    let open = format!("<{tag}>");
    let start = text[from..].find(&open)? + from + open.len();
    let end = text[start..].find(&format!("</{tag}>"))? + start;
    Some(start..end)
}

fn trimmed(text: &str, range: Range<usize>) -> Range<usize> {
    let value = &text[range.clone()];
    let start = range.start + (value.len() - value.trim_start().len());
    start..start + value.trim().len()
}

fn pom_version(text: &str) -> Option<Range<usize>> {
    for artifact in ["spring-boot-starter-parent", "spring-boot-dependencies"] {
        let Some(at) = text.find(&format!("<artifactId>{artifact}</artifactId>")) else { continue };
        let block_start = text[..at].rfind("<parent>").or_else(|| text[..at].rfind("<dependency>")).unwrap_or(0);
        let block_end = ["</parent>", "</dependency>"]
            .iter()
            .filter_map(|end| text[at..].find(end).map(|found| found + at))
            .min()
            .unwrap_or(text.len());
        let version = find_tag(text, block_start, "version").filter(|range| range.end <= block_end)?;
        let version = trimmed(text, version);
        let value = &text[version.clone()];
        if let Some(property) = value.strip_prefix("${").and_then(|rest| rest.strip_suffix('}')) {
            return find_tag(text, 0, property).map(|range| trimmed(text, range));
        }
        return Some(version);
    }
    find_tag(text, 0, "spring-boot.version").map(|range| trimmed(text, range))
}

fn quoted_after(text: &str, from: usize, line_end: usize) -> Option<Range<usize>> {
    let rest = &text[from..line_end];
    let open = rest.find(['"', '\''])?;
    let quote = rest[open..].chars().next()?;
    let start = from + open + 1;
    let end = text[start..line_end].find(quote)? + start;
    Some(start..end)
}

fn gradle_version(text: &str) -> Option<Range<usize>> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_end = offset + line.trim_end_matches(['\n', '\r']).len();
        if let Some(at) = line.find("org.springframework.boot") {
            let after = offset + at + "org.springframework.boot".len();
            if let Some(dependencies) = text[after..line_end].strip_prefix(":spring-boot-dependencies:") {
                let start = line_end - dependencies.len();
                let end = text[start..line_end].find(['"', '\'']).map_or(line_end, |found| found + start);
                return Some(start..end);
            }
            if let Some(version) = text[after..line_end].find("version") {
                return quoted_after(text, after + version, line_end);
            }
        }
        offset += line.len();
    }
    None
}

fn catalog_version(text: &str) -> Option<Range<usize>> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let key = line.split('=').next().unwrap_or_default().trim();
        if matches!(key, "spring-boot" | "springBoot" | "spring_boot" | "springboot") && line.contains('=') {
            let line_end = offset + line.trim_end_matches(['\n', '\r']).len();
            return quoted_after(text, offset + line.find('=')?, line_end);
        }
        offset += line.len();
    }
    None
}

pub fn version_in(path: &Path, text: &str) -> Option<Range<usize>> {
    let name = path.file_name()?.to_str()?;
    let range = match name {
        "pom.xml" => pom_version(text),
        "build.gradle" | "build.gradle.kts" => gradle_version(text),
        "libs.versions.toml" => catalog_version(text),
        _ => None,
    }?;
    let version = &text[range.clone()];
    (version.split('.').take(2).all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && version.contains('.'))
    .then_some(range)
}

pub fn generation_of(version: &str) -> String {
    let mut parts = version.split('.');
    format!("{}.{}.x", parts.next().unwrap_or_default(), parts.next().unwrap_or_default())
}

pub fn check(path: &Path, text: &str, generations: &[Generation], today: &str) -> Vec<Finding> {
    let Some(range) = version_in(path, text) else { return Vec::new() };
    let version = &text[range.clone()];
    let name = generation_of(version);
    let Some(generation) = generations.iter().find(|generation| generation.name == name) else { return Vec::new() };
    let Some(end) = generation.oss_support_end_date.as_deref().filter(|end| *end < today) else { return Vec::new() };
    let message = match generation.commercial_support_end_date.as_deref() {
        Some(commercial) if commercial >= today => t!(
            "workbench-spring-boot-support-ended-commercial",
            version = version.to_owned(),
            generation = name.clone(),
            date = end.to_owned(),
            commercial = commercial.to_owned()
        ),
        _ => t!(
            "workbench-spring-boot-support-ended",
            version = version.to_owned(),
            generation = name.clone(),
            date = end.to_owned()
        ),
    };
    vec![Finding {
        start: range.start,
        end: range.end,
        severity: FindingSeverity::Warning,
        message,
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        source: super::PLUGIN.to_owned(),
        code: "supportEnded".to_owned(),
        deprecated: false,
        unused: false,
    }]
}

impl Inspector for BootSupport {
    fn id(&self) -> &str {
        "spring-boot-support"
    }

    fn handles(&self, file_name: &str) -> bool {
        matches!(file_name, "pom.xml" | "build.gradle" | "build.gradle.kts" | "libs.versions.toml")
    }

    fn plugin(&self) -> Option<&str> {
        Some(super::PLUGIN)
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    fn set_wake(&self, wake: Wake) {
        *lock(&self.wake) = Some(wake);
    }

    fn set_cache_dir(&self, dir: PathBuf) {
        *lock(&self.cache) = Some(dir);
    }

    fn settings(&self) -> Vec<DeclaredSetting> {
        vec![DeclaredSetting {
            key: CHECK.to_owned(),
            page: crate::maven::settings::BUILD_TOOLS_PAGE.to_owned(),
            group: t!("workbench-spring-boot-group"),
            title: t!("workbench-spring-boot-check"),
            description: t!("workbench-spring-boot-check-description"),
            default: false,
            network: vec![HOST.to_owned()],
        }]
    }

    fn refresh(&self) {
        let mut state = lock(&self.state);
        state.failed_at = None;
        if let Some(cached) = &mut state.cached {
            cached.fetched = 0;
        }
    }

    fn inspect(&self, context: &InspectContext<'_>) -> Vec<Finding> {
        let on = (context.setting)(CHECK).and_then(|value| value.as_bool()).unwrap_or(false);
        if !on || version_in(context.path, context.text).is_none() {
            return Vec::new();
        }
        self.read_disk();
        if self.due() {
            self.start_fetch();
        }
        check(context.path, context.text, &self.generations(), &(self.today)())
    }

    fn apply(&self, _context: &InspectContext<'_>, _fixes: &[InspectionFix]) -> Result<Box<dyn FixJob>, String> {
        Err(t!("workbench-spring-boot-no-fix"))
    }
}
