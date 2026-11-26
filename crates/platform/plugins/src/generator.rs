use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_project::generators::{
    GeneratedFile, GeneratorDeclaration, GeneratorEnvironment, GeneratorForm, GeneratorInfo, GeneratorRequest,
    GeneratorSection, ModuleBuild, ModuleEntry, ProjectGenerator, StepAnswer, ToolRun, ToolRunner, Values, hoist,
    localized_form, unzipped, write_files,
};
use zen_project::support::DeclaredBuild;

use crate::WasmHighlighter;

const MAX_ROUNDS: usize = 8;

const MAX_FETCHES: usize = 16;

#[cfg(not(target_arch = "wasm32"))]
const MAX_FETCHED: u64 = 16 << 20;

#[cfg(not(target_arch = "wasm32"))]
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

const USER_AGENT: &str = "User-Agent";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetched {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub bytes: Vec<u8>,
}

impl Fetched {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    fn max_age(&self) -> Option<u64> {
        let control = self.header("Cache-Control")?;
        if control.to_ascii_lowercase().contains("no-store") {
            return None;
        }
        let directive = |part: &str| part.trim().strip_prefix("max-age=")?.trim().parse().ok();
        Some(control.split(',').find_map(directive).unwrap_or(0))
    }
}

pub trait Fetcher: Send + Sync {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Fetched, String>;
}

#[cfg(not(target_arch = "wasm32"))]
pub struct WebFetcher;

#[cfg(not(target_arch = "wasm32"))]
impl Fetcher for WebFetcher {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Fetched, String> {
        let answer = zen_update::fetch::with_headers(url, headers, MAX_FETCHED, FETCH_TIMEOUT)
            .map_err(|error| error.to_string())?;
        Ok(Fetched { status: answer.status, headers: answer.headers, bytes: answer.bytes })
    }
}

pub type ArchiveMemo = Arc<Mutex<Option<(String, Vec<GeneratedFile>)>>>;

#[derive(Clone)]
pub struct GeneratorHost {
    pub fetcher: Arc<dyn Fetcher>,
    pub client: String,
    pub cache: Option<PathBuf>,
    pub environment: GeneratorEnvironment,
    pub archive: ArchiveMemo,
}

impl GeneratorHost {
    pub fn new(fetcher: Arc<dyn Fetcher>, client: &str) -> Self {
        Self {
            fetcher,
            client: client.to_owned(),
            cache: None,
            environment: GeneratorEnvironment::default(),
            archive: ArchiveMemo::default(),
        }
    }
}

#[derive(Deserialize)]
struct Wanted {
    id: String,
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    cache: bool,
}

#[derive(Deserialize)]
struct WantedArchive {
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct WrittenFile {
    path: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    executable: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
    saved_at: u64,
    max_age: u64,
    text: String,
}

impl Saved {
    fn fresh(&self) -> bool {
        now() < self.saved_at.saturating_add(self.max_age)
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn web_address(url: &str) -> bool {
    url.starts_with("https://")
}

fn file_name(fetch: &Wanted) -> String {
    let key = format!("{}\n{:?}", fetch.url, fetch.headers);
    let hash = key
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{hash:016x}.json")
}

fn refusal(answer: &Fetched) -> String {
    let parsed: Option<Value> = serde_json::from_slice(&answer.bytes).ok();
    let said = parsed.as_ref().and_then(|value| value.get("message")).and_then(Value::as_str);
    t!("workbench-new-project-service-refused", status = answer.status, message = said.unwrap_or_default())
}

#[derive(Default)]
struct Round {
    fetched: serde_json::Map<String, Value>,
    from_saved: bool,
}

pub struct PluginGenerator {
    plugin: String,
    declaration: GeneratorDeclaration,
    module: Arc<WasmHighlighter>,
    host: GeneratorHost,
    tools: Option<Arc<dyn ToolRunner>>,
}

impl PluginGenerator {
    pub fn new(
        plugin: &str,
        declaration: GeneratorDeclaration,
        module: Arc<WasmHighlighter>,
        host: GeneratorHost,
    ) -> Self {
        Self { plugin: plugin.to_owned(), declaration, module, host, tools: None }
    }

    #[must_use]
    pub fn with_tools(mut self, tools: Arc<dyn ToolRunner>) -> Self {
        self.tools = Some(tools);
        self
    }

    fn id(&self) -> String {
        format!("{}.{}", self.plugin, self.declaration.id)
    }

    fn settings(&self) -> BTreeMap<String, String> {
        let prefix = format!("{}.", self.id());
        let given = &self.host.environment.settings;
        self.declaration
            .resolved_settings(&zen_i18n::language(), &self.id())
            .into_iter()
            .map(|setting| {
                let value = given.get(&setting.key).map(|value| value.trim()).filter(|value| !value.is_empty());
                let value = value.map_or(setting.default.clone(), str::to_owned);
                (setting.key[prefix.len()..].to_owned(), value)
            })
            .collect()
    }

    fn web(&self, url: &str, headers: &BTreeMap<String, String>) -> Result<Fetched, String> {
        if !self.host.environment.network {
            return Err(t!("workbench-new-project-network-off"));
        }
        let mut sent: Vec<(String, String)> = headers
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case(USER_AGENT))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        sent.push((USER_AGENT.to_owned(), self.host.client.clone()));
        self.host.fetcher.get(url, &sent)
    }

    fn saved_path(&self, fetch: &Wanted) -> Option<PathBuf> {
        Some(self.host.cache.as_ref()?.join(&self.plugin).join(file_name(fetch)))
    }

    fn save(&self, path: Option<&Path>, saved: &Saved) {
        let Some(path) = path else { return };
        if let (Some(folder), Ok(text)) = (path.parent(), serde_json::to_vec(saved)) {
            let _ = std::fs::create_dir_all(folder);
            let _ = zen_vfs::write_atomic(path, &text);
        }
    }

    fn revalidated(&self, fetch: &Wanted, round: &mut Round) -> Value {
        let path = self.saved_path(fetch);
        let saved: Option<Saved> = path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .filter(|saved: &Saved| saved.url == fetch.url);
        if let Some(saved) = saved.as_ref().filter(|saved| saved.fresh()) {
            return json!({ "status": 200, "text": saved.text });
        }
        let mut headers = fetch.headers.clone();
        if let Some(etag) = saved.as_ref().and_then(|saved| saved.etag.clone()) {
            headers.insert("If-None-Match".to_owned(), etag);
        }
        match (self.web(&fetch.url, &headers), saved) {
            (Ok(answer), Some(saved)) if answer.status == 304 => {
                let max_age = answer.max_age().unwrap_or(saved.max_age);
                let saved = Saved { saved_at: now(), max_age, ..saved };
                self.save(path.as_deref(), &saved);
                json!({ "status": 200, "text": saved.text })
            }
            (Ok(answer), _) if answer.status == 200 => {
                if let Some(max_age) = answer.max_age() {
                    let saved = Saved {
                        url: fetch.url.clone(),
                        etag: answer.header("ETag").map(str::to_owned),
                        saved_at: now(),
                        max_age,
                        text: answer.text(),
                    };
                    self.save(path.as_deref(), &saved);
                }
                json!({ "status": 200, "text": answer.text() })
            }
            (Ok(answer), None) => json!({ "status": answer.status, "text": answer.text() }),
            (Ok(answer), Some(_)) if answer.status < 500 => json!({ "status": answer.status, "text": answer.text() }),
            (Err(error), None) => json!({ "error": error }),
            (_, Some(saved)) => {
                round.from_saved = true;
                json!({ "status": 200, "text": saved.text })
            }
        }
    }

    fn fetched(&self, wanted: Vec<Wanted>, round: &mut Round) -> Result<(), String> {
        if !self.declaration.network {
            return Err(format!("{}: the generator does not declare `network`", self.plugin));
        }
        if wanted.len() > MAX_FETCHES {
            return Err(format!("{}: the generator asks for too many addresses", self.plugin));
        }
        for fetch in wanted {
            if !web_address(&fetch.url) {
                return Err(format!("{}: {} is not an https address", self.plugin, fetch.url));
            }
            let answer = if fetch.cache {
                self.revalidated(&fetch, round)
            } else {
                match self.web(&fetch.url, &fetch.headers) {
                    Ok(answer) => json!({ "status": answer.status, "text": answer.text() }),
                    Err(error) => json!({ "error": error }),
                }
            };
            round.fetched.insert(fetch.id, answer);
        }
        Ok(())
    }

    fn ask(&self, step: &str, request: &GeneratorRequest) -> Result<(Value, bool), String> {
        let mut round = Round::default();
        for _ in 0..MAX_ROUNDS {
            let input = json!({
                "step": step,
                "generator": self.declaration.id,
                "language": zen_i18n::language(),
                "name": request.name,
                "values": request.values,
                "settings": self.settings(),
                "fetched": round.fetched,
            });
            let stopped = || format!("The plug-in {} stopped working.", self.plugin);
            let answer = self.module.project(&input.to_string()).ok_or_else(stopped)?;
            let mut answer: Value = serde_json::from_slice(&answer)
                .map_err(|error| format!("{}: the generator's answer is invalid: {error}", self.plugin))?;
            if let Some(error) = answer.get("error").and_then(Value::as_str) {
                return Err(error.to_owned());
            }
            match answer.get_mut("fetch").map(Value::take) {
                Some(wanted) => {
                    let wanted: Vec<Wanted> = serde_json::from_value(wanted)
                        .map_err(|error| format!("{}: the generator's answer is invalid: {error}", self.plugin))?;
                    self.fetched(wanted, &mut round)?;
                }
                None => return Ok((answer, round.from_saved)),
            }
        }
        Err(format!("{}: the generator keeps asking for more addresses", self.plugin))
    }

    fn memo(&self) -> std::sync::MutexGuard<'_, Option<(String, Vec<GeneratedFile>)>> {
        self.host.archive.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn archive(&self, wanted: WantedArchive, keep: bool) -> Result<Vec<GeneratedFile>, String> {
        if !self.declaration.network {
            return Err(format!("{}: the generator does not declare `network`", self.plugin));
        }
        if !web_address(&wanted.url) {
            return Err(format!("{}: {} is not an https address", self.plugin, wanted.url));
        }
        let kept = {
            let mut memo = self.memo();
            let same = memo.as_ref().is_some_and(|(url, _)| *url == wanted.url);
            match (same, keep) {
                (true, true) => memo.as_ref().map(|(_, files)| files.clone()),
                (true, false) => memo.take().map(|(_, files)| files),
                _ => None,
            }
        };
        if let Some(files) = kept {
            return Ok(files);
        }
        let answer = self.web(&wanted.url, &wanted.headers).map_err(|error| {
            if self.host.environment.network {
                t!("workbench-new-project-needs-network", error = error.as_str())
            } else {
                error
            }
        })?;
        if answer.status != 200 {
            return Err(refusal(&answer));
        }
        let files =
            unzipped(answer.bytes).map_err(|error| t!("workbench-new-project-bad-archive", error = error.as_str()))?;
        if keep {
            *self.memo() = Some((wanted.url, files.clone()));
        }
        Ok(files)
    }

    fn produced(&self, request: &GeneratorRequest, keep: bool) -> Result<Vec<GeneratedFile>, String> {
        let (mut answer, _) = self.ask("files", request)?;
        let invalid = |error: serde_json::Error| format!("{}: the generator's answer is invalid: {error}", self.plugin);
        if let Some(archive) = answer.get_mut("archive").map(Value::take) {
            return self.archive(serde_json::from_value(archive).map_err(invalid)?, keep);
        }
        let files = answer.get_mut("files").map(Value::take);
        let files = files.ok_or_else(|| format!("{}: the generator gave no files", self.plugin))?;
        written(files).map_err(|error| format!("{}: the generator's files are invalid: {error}", self.plugin))
    }
}

impl ProjectGenerator for PluginGenerator {
    fn info(&self) -> GeneratorInfo {
        let language = zen_i18n::language();
        GeneratorInfo {
            id: self.id(),
            name: self.declaration.display_name(&language),
            icon: self.declaration.icon.clone(),
            section: if self.declaration.language { GeneratorSection::Types } else { GeneratorSection::Generators },
            plugin: self.plugin.clone(),
            description: None,
            network: self.declaration.network,
            settings: self.declaration.resolved_settings(&language, &self.id()),
            form_depends_on: self.declaration.form_depends_on.clone(),
            tool: self.declaration.tool.clone(),
            service: self
                .declaration
                .service
                .as_ref()
                .filter(|_| self.declaration.network)
                .and_then(|setting| self.settings().remove(setting)),
        }
    }

    fn form(&self) -> Result<GeneratorForm, String> {
        self.form_with(&Values::new())
    }

    fn form_with(&self, values: &Values) -> Result<GeneratorForm, String> {
        let language = zen_i18n::language();
        if !self.declaration.dynamic_form {
            return self.declaration.form(&language);
        }
        let request = GeneratorRequest { name: String::new(), values: values.clone() };
        let (answer, from_saved) = self.ask("form", &request)?;
        let form = answer.get("form").ok_or_else(|| format!("{}: the generator gave no form", self.plugin))?;
        let mut form = localized_form(form, &language)?;
        if from_saved {
            form.notice = Some(t!("workbench-new-project-saved-form"));
        }
        Ok(form)
    }

    fn files(&self, request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
        self.produced(request, true)
    }

    fn generate(&self, request: &GeneratorRequest, folder: &Path, stop: &AtomicBool) -> Result<(), String> {
        self.generate_showing(request, folder, stop, &|_| {})
    }

    fn step(&self, step: &str, values: Values) -> Result<StepAnswer, String> {
        let request = GeneratorRequest { name: String::new(), values };
        let (answer, from_saved) = self.ask(step, &request)?;
        Ok(StepAnswer { answer, from_saved })
    }

    fn generate_showing(
        &self,
        request: &GeneratorRequest,
        folder: &Path,
        stop: &AtomicBool,
        output: &dyn Fn(&str),
    ) -> Result<(), String> {
        let Some(tool) = &self.declaration.tool else { return write_files(folder, &self.produced(request, false)?) };
        if self.declaration.network && !self.host.environment.network {
            return Err(t!("workbench-new-project-network-off"));
        }
        let (answer, _) = self.ask("generate", request)?;
        let run = answer.get("run").ok_or_else(|| format!("{}: the generator gave nothing to run", self.plugin))?;
        let run: ToolRun = serde_json::from_value(run.clone())
            .map_err(|error| format!("{}: the generator's answer is invalid: {error}", self.plugin))?;
        if run.tool != *tool {
            return Err(format!("{}: the generator does not declare the tool {}", self.plugin, run.tool));
        }
        let tools = self.tools.as_ref().ok_or_else(|| format!("{}: this IDE cannot run {tool}", self.plugin))?;
        tools.run(&run, folder, stop, output)?;
        match &run.subfolder {
            Some(subfolder) => hoist(folder, subfolder),
            None => Ok(()),
        }
    }
}

pub struct PluginModuleBuild {
    plugin: String,
    build: DeclaredBuild,
    module: Arc<WasmHighlighter>,
}

impl PluginModuleBuild {
    pub fn new(plugin: &str, build: DeclaredBuild, module: Arc<WasmHighlighter>) -> Self {
        Self { plugin: plugin.to_owned(), build, module }
    }
}

impl ModuleBuild for PluginModuleBuild {
    fn plugin(&self) -> String {
        self.plugin.clone()
    }

    fn module_files(&self) -> Vec<String> {
        self.build.module_files.clone()
    }

    fn reload(&self) -> String {
        self.build.reload.clone()
    }

    fn add_module(&self, entry: &ModuleEntry) -> Result<Vec<GeneratedFile>, String> {
        let input = json!({
            "step": "addModule",
            "language": zen_i18n::language(),
            "file": entry.file,
            "text": entry.text,
            "path": entry.path,
            "project": entry.project,
            "hasBuild": entry.has_build,
            "mustBuild": entry.must_build,
        });
        let stopped = || format!("The plug-in {} stopped working.", self.plugin);
        let answer = self.module.project(&input.to_string()).ok_or_else(stopped)?;
        let answer: Value = serde_json::from_slice(&answer)
            .map_err(|error| format!("{}: the build's answer is invalid: {error}", self.plugin))?;
        if let Some(error) = answer.get("error").and_then(Value::as_str) {
            return Err(error.to_owned());
        }
        let files = answer.get("files").cloned().unwrap_or_else(|| json!([]));
        written(files).map_err(|error| format!("{}: the build's answer is invalid: {error}", self.plugin))
    }
}

fn written(files: Value) -> Result<Vec<GeneratedFile>, serde_json::Error> {
    let files: Vec<WrittenFile> = serde_json::from_value(files)?;
    Ok(files
        .into_iter()
        .map(|file| GeneratedFile { path: file.path, bytes: file.text.into_bytes(), executable: file.executable })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn escaped(text: &str) -> String {
        text.replace('\\', "\\\\").replace('"', "\\\"")
    }

    fn module(body: &str, answers: &[&str]) -> Arc<WasmHighlighter> {
        let data: String = answers
            .iter()
            .enumerate()
            .map(|(index, answer)| format!("(data (i32.const {}) \"{}\")\n", 4096 * (index + 1), escaped(answer)))
            .collect();
        let wat = format!(
            r#"(module
              (memory (export "memory") 8)
              (global $calls (mut i32) (i32.const 0))
              {data}
              (func (export "zen_abi") (result i32) i32.const 1)
              (func (export "zen_alloc") (param i32) (result i32) i32.const 262144)
              (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0)
              (func (export "zen_project") (param i32 i32) (result i64) {body}))"#
        );
        Arc::new(WasmHighlighter::load("sample", &wat::parse_str(wat).unwrap(), &[]).unwrap())
    }

    fn answering(answer: &str) -> Arc<WasmHighlighter> {
        let body = format!("(i64.or (i64.const {}) (i64.const {}))", 4096_i64 << 32, answer.len());
        module(&body, &[answer])
    }

    const CLIENT: &str = "zen-ide/26.11.2";

    type Asked = (String, Vec<(String, String)>);

    #[derive(Default)]
    struct FakeWeb {
        asked: Mutex<Vec<Asked>>,
    }

    impl Fetcher for FakeWeb {
        fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Fetched, String> {
            self.asked.lock().unwrap().push((url.to_owned(), headers.to_vec()));
            Ok(Fetched { status: 200, headers: Vec::new(), bytes: b"{}".to_vec() })
        }
    }

    fn declaration(extra: Value) -> GeneratorDeclaration {
        let mut base = json!({ "id": "vite", "name": { "en": "Vite", "ru": "Вайт" }, "icon": "package.json" });
        base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    fn generator(extra: Value, module: Arc<WasmHighlighter>) -> PluginGenerator {
        PluginGenerator::new(
            "sample",
            declaration(extra),
            module,
            GeneratorHost::new(Arc::new(FakeWeb::default()), CLIENT),
        )
    }

    #[test]
    fn the_java_and_kotlin_plug_ins_declare_language_types_whose_jdk_the_module_can_read() {
        for manifest in [
            include_str!("../../../../plugins/java/plugin.json"),
            include_str!("../../../../plugins/kotlin/plugin.json"),
        ] {
            let manifest: Value = serde_json::from_str(manifest).unwrap();
            let list = GeneratorDeclaration::parse_list(&manifest["projectGenerators"].to_string()).unwrap();
            assert!(list[0].language);
            let form = list[0].form("ru").unwrap();
            let jdk =
                form.fields.iter().find(|field| matches!(field.kind, zen_project::generators::FieldKind::Jdk { .. }));
            assert_eq!(zen_project::generators::jdk_release_key(&jdk.unwrap().id), "jdkRelease");
            assert_eq!(form.fields.iter().filter(|field| field.advanced).count(), 2);
            assert_eq!(form.fields[0].label, "Система сборки:");
        }
    }

    #[derive(Default)]
    struct FakeTools {
        runs: Mutex<Vec<ToolRun>>,
    }

    impl ToolRunner for FakeTools {
        fn run(&self, run: &ToolRun, folder: &Path, _stop: &AtomicBool, output: &dyn Fn(&str)) -> Result<(), String> {
            self.runs.lock().unwrap().push(run.clone());
            std::fs::create_dir(folder.join("shop")).unwrap();
            std::fs::write(folder.join("shop/pom.xml"), "x").unwrap();
            output("BUILD SUCCESS");
            Ok(())
        }
    }

    fn tool_generator(answer: &str, tools: Option<Arc<FakeTools>>) -> PluginGenerator {
        let declared = GeneratorDeclaration {
            id: "archetype".to_owned(),
            tool: Some("maven".to_owned()),
            ..GeneratorDeclaration::default()
        };
        let generator = PluginGenerator::new(
            "maven",
            declared,
            answering(answer),
            GeneratorHost::new(Arc::new(FakeWeb::default()), CLIENT),
        );
        match tools {
            Some(tools) => generator.with_tools(tools),
            None => generator,
        }
    }

    #[test]
    fn a_generator_runs_the_tool_it_declares_and_the_tool_s_folder_becomes_the_project() {
        let answer = r#"{"run":{"tool":"maven","args":["archetype:generate"],"javaHome":"/jdk","subfolder":"shop"}}"#;
        let tools = Arc::new(FakeTools::default());
        let generator = tool_generator(answer, Some(Arc::clone(&tools)));
        assert_eq!(generator.info().tool.as_deref(), Some("maven"));
        let folder = tempfile::tempdir().unwrap();
        let lines = Mutex::new(Vec::new());
        generator
            .generate_showing(&GeneratorRequest::default(), folder.path(), &AtomicBool::new(false), &|line| {
                lines.lock().unwrap().push(line.to_owned());
            })
            .unwrap();
        assert!(folder.path().join("pom.xml").is_file());
        assert_eq!(lines.into_inner().unwrap(), ["BUILD SUCCESS"]);
        assert_eq!(tools.runs.lock().unwrap()[0].java_home.as_deref(), Some("/jdk"));
    }

    #[test]
    fn a_generator_cannot_run_a_tool_it_does_not_declare() {
        let tools = Arc::new(FakeTools::default());
        let generator = tool_generator(r#"{"run":{"tool":"sh","args":["-c","id"]}}"#, Some(Arc::clone(&tools)));
        let folder = tempfile::tempdir().unwrap();
        assert!(generator.generate(&GeneratorRequest::default(), folder.path(), &AtomicBool::new(false)).is_err());
        assert!(tools.runs.lock().unwrap().is_empty());
    }

    #[test]
    fn a_generator_gets_the_values_its_form_depends_on() {
        let declared = GeneratorDeclaration {
            id: "archetype".to_owned(),
            dynamic_form: true,
            form_depends_on: vec!["catalog".to_owned()],
            ..GeneratorDeclaration::default()
        };
        let answer = r#"{"form":{"fields":[]}}"#;
        let generator = PluginGenerator::new(
            "maven",
            declared,
            answering(answer),
            GeneratorHost::new(Arc::new(FakeWeb::default()), CLIENT),
        );
        assert_eq!(generator.info().form_depends_on, ["catalog"]);
        let values = Values::from([("catalog".to_owned(), json!("internal"))]);
        assert_eq!(generator.form_with(&values).unwrap(), GeneratorForm::default());
    }

    #[test]
    fn a_build_edits_its_files_through_the_module() {
        let build = DeclaredBuild {
            files: vec!["pom.xml".to_owned()],
            reload: "maven.project.reload".to_owned(),
            module_files: vec!["pom.xml".to_owned()],
        };
        let answer = r#"{"files":[{"path":"pom.xml","text":"edited"}]}"#;
        let module = PluginModuleBuild::new("maven", build, answering(answer));
        assert_eq!((module.plugin().as_str(), module.reload().as_str()), ("maven", "maven.project.reload"));
        assert_eq!(module.add_module(&ModuleEntry::default()).unwrap(), [GeneratedFile::text("pom.xml", "edited")]);
        let refusing = PluginModuleBuild::new("maven", DeclaredBuild::default(), answering(r#"{"error":"no pom"}"#));
        assert_eq!(refusing.add_module(&ModuleEntry::default()), Err("no pom".to_owned()));
    }

    #[test]
    fn the_maven_and_gradle_plug_ins_declare_what_they_list_modules_in() {
        let maven: Value = serde_json::from_str(include_str!("../../../../plugins/maven/plugin.json")).unwrap();
        let build: DeclaredBuild = serde_json::from_value(maven["project"]["build"].clone()).unwrap();
        assert_eq!(build.module_files, ["pom.xml"]);
        let list = GeneratorDeclaration::parse_list(&maven["projectGenerators"].to_string()).unwrap();
        assert_eq!(list[0].tool.as_deref(), Some("maven"));
        assert!(list[0].network && list[0].dynamic_form && !list[0].language);
        let gradle: Value = serde_json::from_str(include_str!("../../../../plugins/gradle/plugin.json")).unwrap();
        let build: DeclaredBuild = serde_json::from_value(gradle["project"]["build"].clone()).unwrap();
        assert_eq!(build.module_files[0], "settings.gradle.kts");
    }

    #[test]
    fn a_language_declaration_stands_with_the_types_and_others_under_generators() {
        let generator = |language: bool| {
            let declared = GeneratorDeclaration { id: "x".to_owned(), language, ..GeneratorDeclaration::default() };
            PluginGenerator::new(
                "p",
                declared,
                answering("{}"),
                GeneratorHost::new(Arc::new(FakeWeb::default()), CLIENT),
            )
        };
        assert_eq!(generator(true).info().section, GeneratorSection::Types);
        assert_eq!(generator(false).info().section, GeneratorSection::Generators);
    }

    #[test]
    fn a_generator_stands_under_generators_with_the_plugin_s_id_in_its_own() {
        let generator = generator(json!({}), answering("{}"));
        let info = generator.info();
        assert_eq!(info.id, "sample.vite");
        assert_eq!(info.name, "Vite");
        assert_eq!(info.icon, "package.json");
        assert_eq!(info.section, GeneratorSection::Generators);
    }

    #[test]
    fn a_declared_form_comes_from_the_manifest() {
        let fields = json!({ "fields": [{ "id": "ts", "label": "TypeScript", "type": "check", "default": true }] });
        let form = generator(fields, answering("{}")).form().unwrap();
        assert_eq!(form.fields[0].id, "ts");
    }

    #[test]
    fn the_module_writes_the_files() {
        let answer =
            r#"{"files":[{"path":"package.json","text":"{}"},{"path":"run.sh","text":"run","executable":true}]}"#;
        let files = generator(json!({}), answering(answer)).files(&GeneratorRequest::default()).unwrap();
        assert_eq!(files[0], GeneratedFile::text("package.json", "{}"));
        assert!(files[1].executable);
    }

    #[test]
    fn a_failing_generator_reports_its_error() {
        let generator = generator(json!({}), answering(r#"{"error":"npm is missing"}"#));
        assert_eq!(generator.files(&GeneratorRequest::default()).unwrap_err(), "npm is missing");
        assert!(generator.module.has_project(), "an error is an answer, not a crash");
    }

    #[test]
    fn a_module_that_traps_is_reported_and_writes_nothing() {
        let generator = generator(json!({}), module("unreachable", &[]));
        let folder = tempfile::tempdir().unwrap();
        let error = generator.generate(&GeneratorRequest::default(), folder.path(), &AtomicBool::new(false));
        assert!(error.unwrap_err().contains("stopped working"));
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
    }

    fn fetching_then(answer: &str) -> Arc<WasmHighlighter> {
        let wanted =
            r#"{"fetch":[{"id":"metadata","url":"https://example.org/meta","headers":{"Accept":"text/plain"}}]}"#;
        let body = format!(
            r#"(global.set $calls (i32.add (global.get $calls) (i32.const 1)))
               (if (result i64) (i32.eq (global.get $calls) (i32.const 1))
                 (then (i64.or (i64.const {}) (i64.const {})))
                 (else (i64.or (i64.const {}) (i64.const {}))))"#,
            4096_i64 << 32,
            wanted.len(),
            8192_i64 << 32,
            answer.len()
        );
        module(&body, &[wanted, answer])
    }

    #[test]
    fn a_network_generator_gets_its_addresses_from_the_ide_and_loads_its_form() {
        let answer = r#"{"form":{"fields":[{"id":"boot","label":"Boot","type":"text"}]}}"#;
        let web = Arc::new(FakeWeb::default());
        let declaration = declaration(json!({ "dynamicForm": true, "network": true }));
        let host = GeneratorHost::new(Arc::clone(&web) as _, CLIENT);
        let generator = PluginGenerator::new("sample", declaration, fetching_then(answer), host);
        let form = generator.form().unwrap();
        assert_eq!(form.fields[0].id, "boot");
        let asked = web.asked.lock().unwrap();
        assert_eq!(asked[0].0, "https://example.org/meta");
        assert_eq!(
            asked[0].1,
            [("Accept".to_owned(), "text/plain".to_owned()), ("User-Agent".to_owned(), CLIENT.to_owned())]
        );
    }

    #[test]
    fn a_generator_that_did_not_declare_the_network_gets_none() {
        let web = Arc::new(FakeWeb::default());
        let declaration = declaration(json!({ "dynamicForm": true }));
        let host = GeneratorHost::new(Arc::clone(&web) as _, CLIENT);
        let generator = PluginGenerator::new("sample", declaration, fetching_then("{}"), host);
        assert!(generator.form().unwrap_err().contains("network"));
        assert!(web.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn the_installed_manifest_keeps_the_declarations() {
        let plugin: crate::InstalledPlugin = serde_json::from_value(json!({
            "id": "sample", "name": "Sample", "version": "0.1.0", "extensions": [],
            "projectGenerators": [{ "id": "vite", "name": "Vite", "dynamicForm": true }],
        }))
        .unwrap();
        assert_eq!(plugin.project_generators[0].id, "vite");
        assert!(plugin.project_generators[0].dynamic_form);
    }

    #[test]
    fn a_module_without_the_export_has_no_generators() {
        let wat = r#"(module (memory (export "memory") 1)
            (func (export "zen_abi") (result i32) i32.const 1)
            (func (export "zen_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "zen_highlight") (param i32 i32) (result i64) i64.const 0))"#;
        let module = WasmHighlighter::load("sample", &wat::parse_str(wat).unwrap(), &[]).unwrap();
        assert!(!module.has_project());
    }

    const STUB: &str = include_str!("../../../../plugins/spring-boot/testdata/metadataWithV23AcceptHeader.json");

    fn stub_answer() -> Fetched {
        let mapping: Value = serde_json::from_str(STUB).unwrap();
        let headers = mapping["response"]["headers"].as_object().unwrap();
        Fetched {
            status: u16::try_from(mapping["response"]["status"].as_u64().unwrap()).unwrap(),
            headers: headers.iter().map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned())).collect(),
            bytes: mapping["response"]["body"].as_str().unwrap().as_bytes().to_vec(),
        }
    }

    #[derive(Default)]
    struct Replay {
        answers: Mutex<Vec<Result<Fetched, String>>>,
        asked: Mutex<Vec<Asked>>,
    }

    impl Replay {
        fn with(answers: Vec<Result<Fetched, String>>) -> Arc<Self> {
            Arc::new(Self { answers: Mutex::new(answers), asked: Mutex::default() })
        }

        fn asked(&self) -> usize {
            self.asked.lock().unwrap().len()
        }
    }

    impl Fetcher for Replay {
        fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Fetched, String> {
            self.asked.lock().unwrap().push((url.to_owned(), headers.to_vec()));
            self.answers.lock().unwrap().remove(0)
        }
    }

    const SERVICE: &str = "https://start.example.com";

    fn wanted() -> Wanted {
        Wanted {
            id: "metadata".to_owned(),
            url: SERVICE.to_owned(),
            headers: BTreeMap::from([("Accept".to_owned(), "application/vnd.initializr.v2.3+json".to_owned())]),
            cache: true,
        }
    }

    fn host(web: &Arc<Replay>, cache: &Path) -> GeneratorHost {
        GeneratorHost { cache: Some(cache.to_owned()), ..GeneratorHost::new(Arc::clone(web) as _, CLIENT) }
    }

    fn cached_generator(host: GeneratorHost) -> PluginGenerator {
        let declaration = declaration(json!({ "network": true }));
        PluginGenerator::new("sample", declaration, answering("{}"), host)
    }

    fn expire(cache: &Path) {
        let folder = cache.join("sample");
        let file = std::fs::read_dir(folder).unwrap().next().unwrap().unwrap().path();
        let mut saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        saved["savedAt"] = json!(1);
        std::fs::write(&file, saved.to_string()).unwrap();
    }

    #[test]
    fn saved_metadata_answers_without_the_network_until_it_ages() {
        let cache = tempfile::tempdir().unwrap();
        let web = Replay::with(vec![Ok(stub_answer())]);
        let first = cached_generator(host(&web, cache.path())).revalidated(&wanted(), &mut Round::default());
        assert_eq!(first["status"], 200);
        let second = cached_generator(host(&web, cache.path())).revalidated(&wanted(), &mut Round::default());
        assert_eq!(second["text"], first["text"]);
        assert_eq!(web.asked(), 1);
    }

    #[test]
    fn aged_metadata_is_revalidated_with_its_etag_and_kept_on_304() {
        let cache = tempfile::tempdir().unwrap();
        let web = Replay::with(vec![Ok(stub_answer())]);
        let generator = cached_generator(host(&web, cache.path()));
        let first = generator.revalidated(&wanted(), &mut Round::default());
        expire(cache.path());
        let same = Fetched { status: 304, ..Fetched::default() };
        let web = Replay::with(vec![Ok(same)]);
        let mut round = Round::default();
        let again = cached_generator(host(&web, cache.path())).revalidated(&wanted(), &mut round);
        assert_eq!(again, first);
        assert!(!round.from_saved, "a 304 is a live answer");
        let asked = web.asked.lock().unwrap();
        let etag = asked[0].1.iter().find(|(name, _)| name == "If-None-Match").unwrap();
        assert_eq!(etag.1, "\"4b2b7ff2ae9659ba873f5345dada8a7d\"");
    }

    #[test]
    fn offline_the_last_saved_metadata_answers_and_says_so() {
        let cache = tempfile::tempdir().unwrap();
        let web = Replay::with(vec![Ok(stub_answer())]);
        let first = cached_generator(host(&web, cache.path())).revalidated(&wanted(), &mut Round::default());
        expire(cache.path());
        let web = Replay::with(vec![Err("no route".to_owned())]);
        let mut round = Round::default();
        let again = cached_generator(host(&web, cache.path())).revalidated(&wanted(), &mut round);
        assert_eq!(again, first);
        assert!(round.from_saved);
    }

    #[test]
    fn a_server_error_falls_back_but_a_refusal_does_not() {
        let cache = tempfile::tempdir().unwrap();
        let web = Replay::with(vec![Ok(stub_answer())]);
        cached_generator(host(&web, cache.path())).revalidated(&wanted(), &mut Round::default());
        expire(cache.path());
        let answers =
            vec![Ok(Fetched { status: 503, ..Fetched::default() }), Ok(Fetched { status: 404, ..Fetched::default() })];
        let web = Replay::with(answers);
        let generator = cached_generator(host(&web, cache.path()));
        assert_eq!(generator.revalidated(&wanted(), &mut Round::default())["status"], 200);
        assert_eq!(generator.revalidated(&wanted(), &mut Round::default())["status"], 404);
    }

    #[test]
    fn with_nothing_saved_a_failure_reaches_the_plugin() {
        let cache = tempfile::tempdir().unwrap();
        let web = Replay::with(vec![Err("no route".to_owned())]);
        let answer = cached_generator(host(&web, cache.path())).revalidated(&wanted(), &mut Round::default());
        assert_eq!(answer["error"], "no route");
    }

    #[test]
    fn with_the_network_off_nothing_is_asked_and_saved_metadata_still_answers() {
        let cache = tempfile::tempdir().unwrap();
        let web = Replay::with(vec![Ok(stub_answer())]);
        let first = cached_generator(host(&web, cache.path())).revalidated(&wanted(), &mut Round::default());
        expire(cache.path());
        let web = Replay::with(Vec::new());
        let off = GeneratorEnvironment { network: false, ..GeneratorEnvironment::default() };
        let host = GeneratorHost { environment: off, ..host(&web, cache.path()) };
        let mut round = Round::default();
        let again = cached_generator(host).revalidated(&wanted(), &mut round);
        assert_eq!(again, first);
        assert!(round.from_saved);
        assert_eq!(web.asked(), 0);
    }

    #[test]
    fn a_form_from_saved_data_carries_the_notice() {
        let cache = tempfile::tempdir().unwrap();
        let web = Replay::with(vec![Err("no route".to_owned())]);
        let declaration = || declaration(json!({ "dynamicForm": true, "network": true }));
        let form = r#"{"form":{"fields":[]}}"#;
        let saved = Saved {
            url: "https://example.org/meta".to_owned(),
            etag: None,
            saved_at: 1,
            max_age: 1,
            text: "{}".to_owned(),
        };
        let fetch = Wanted {
            id: "metadata".to_owned(),
            url: saved.url.clone(),
            headers: BTreeMap::from([("Accept".to_owned(), "text/plain".to_owned())]),
            cache: true,
        };
        let generator =
            PluginGenerator::new("sample", declaration(), fetching_saved_then(form), host(&web, cache.path()));
        let path = generator.saved_path(&fetch).unwrap();
        generator.save(Some(&path), &saved);
        assert!(generator.form().unwrap().notice.is_some());
    }

    fn fetching_saved_then(answer: &str) -> Arc<WasmHighlighter> {
        let wanted = r#"{"fetch":[{"id":"metadata","url":"https://example.org/meta","cache":true,"headers":{"Accept":"text/plain"}}]}"#;
        let body = format!(
            r#"(global.set $calls (i32.add (global.get $calls) (i32.const 1)))
               (if (result i64) (i32.eq (global.get $calls) (i32.const 1))
                 (then (i64.or (i64.const {}) (i64.const {})))
                 (else (i64.or (i64.const {}) (i64.const {}))))"#,
            4096_i64 << 32,
            wanted.len(),
            8192_i64 << 32,
            answer.len()
        );
        module(&body, &[wanted, answer])
    }

    const ARCHIVE_URL: &str = "https://start.example.com/starter.zip?type=maven-project";

    fn zip_answer() -> Fetched {
        let mut writer = zen_vfs::zip::ZipWriter::new(1_792_000_000);
        writer.add("pom.xml", b"<project/>").unwrap();
        writer.add("mvnw", b"#!/bin/sh").unwrap();
        Fetched { status: 200, headers: Vec::new(), bytes: writer.finish() }
    }

    fn archive_generator(web: &Arc<Replay>, network: bool) -> PluginGenerator {
        let answer = format!(r#"{{"archive":{{"url":"{ARCHIVE_URL}"}}}}"#);
        let declaration = declaration(json!({ "network": network, "preview": true }));
        PluginGenerator::new(
            "sample",
            declaration,
            answering(&answer),
            GeneratorHost::new(Arc::clone(web) as _, CLIENT),
        )
    }

    #[test]
    fn a_generator_names_the_address_it_will_ask_before_it_asks() {
        let web = Replay::with(Vec::new());
        let declared = declaration(json!({
            "network": true, "service": "serviceUrl",
            "settings": [{ "id": "serviceUrl", "label": "URL", "default": "https://own.example" }],
        }));
        let host = GeneratorHost::new(Arc::clone(&web) as _, CLIENT);
        let generator = PluginGenerator::new("sample", declared, answering("{}"), host);
        assert_eq!(generator.info().service.as_deref(), Some("https://own.example"));
        assert!(web.asked.lock().unwrap().is_empty(), "naming it asks nothing");
    }

    #[test]
    fn with_the_network_off_a_generator_s_tool_does_not_start() {
        let declared = declaration(json!({ "network": true, "tool": "maven" }));
        let mut host = GeneratorHost::new(Replay::with(Vec::new()) as _, CLIENT);
        host.environment.network = false;
        let generator = PluginGenerator::new("sample", declared, answering("{}"), host);
        let dir = tempfile::tempdir().unwrap();
        let stop = AtomicBool::new(false);
        let refused = generator.generate(&GeneratorRequest::default(), dir.path(), &stop).unwrap_err();
        assert_eq!(refused, t!("workbench-new-project-network-off"));
    }

    #[test]
    fn a_generator_fetches_https_only() {
        let web = Replay::with(Vec::new());
        let plain = format!(r#"{{"archive":{{"url":"{}"}}}}"#, ARCHIVE_URL.replacen("https://", "http://", 1));
        let declaration = declaration(json!({ "network": true, "preview": true }));
        let host = GeneratorHost::new(Arc::clone(&web) as _, CLIENT);
        let generator = PluginGenerator::new("sample", declaration, answering(&plain), host);
        let refused = generator.files(&GeneratorRequest::default()).unwrap_err();
        assert!(refused.contains("is not an https address"), "{refused}");
        assert!(web.asked.lock().unwrap().is_empty(), "nothing was asked");
    }

    #[test]
    fn an_archive_is_read_in_memory_and_sent_with_the_client_s_name() {
        let web = Replay::with(vec![Ok(zip_answer())]);
        let files = archive_generator(&web, true).files(&GeneratorRequest::default()).unwrap();
        assert_eq!(files[0], GeneratedFile::text("pom.xml", "<project/>"));
        let asked = web.asked.lock().unwrap();
        assert_eq!(asked[0].0, ARCHIVE_URL);
        assert_eq!(asked[0].1, [("User-Agent".to_owned(), CLIENT.to_owned())]);
    }

    #[test]
    fn creating_what_was_previewed_does_not_download_it_again() {
        let web = Replay::with(vec![Ok(zip_answer())]);
        let generator = archive_generator(&web, true);
        generator.files(&GeneratorRequest::default()).unwrap();
        generator.files(&GeneratorRequest::default()).unwrap();
        let folder = tempfile::tempdir().unwrap();
        generator.generate(&GeneratorRequest::default(), folder.path(), &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(folder.path().join("pom.xml")).unwrap(), b"<project/>");
        assert_eq!(web.asked(), 1);
        assert!(generator.host.archive.lock().unwrap().is_none(), "the memory is freed");
    }

    #[test]
    fn a_failed_download_says_generating_needs_the_network() {
        let web = Replay::with(vec![Err("no route".to_owned())]);
        let error = archive_generator(&web, true).files(&GeneratorRequest::default()).unwrap_err();
        assert!(error.contains("needs-network"), "{error}");
    }

    #[test]
    fn with_the_network_off_an_archive_is_not_asked_for() {
        let web = Replay::with(Vec::new());
        let mut generator = archive_generator(&web, true);
        generator.host.environment.network = false;
        let error = generator.files(&GeneratorRequest::default()).unwrap_err();
        assert!(error.contains("network-off"), "{error}");
        assert_eq!(web.asked(), 0);
    }

    #[test]
    fn a_refusal_of_the_service_keeps_its_message() {
        let refused = Fetched {
            status: 400,
            headers: Vec::new(),
            bytes: br#"{"message":"Dependency 'old' is not compatible"}"#.to_vec(),
        };
        let web = Replay::with(vec![Ok(refused)]);
        assert!(archive_generator(&web, true).files(&GeneratorRequest::default()).is_err());
    }

    #[test]
    fn a_generator_that_did_not_declare_the_network_gets_no_archive() {
        let web = Replay::with(Vec::new());
        assert!(archive_generator(&web, false).files(&GeneratorRequest::default()).unwrap_err().contains("network"));
        assert_eq!(web.asked(), 0);
    }

    #[test]
    fn a_step_answers_through_the_same_network_rules_as_a_form() {
        let cache = tempfile::tempdir().unwrap();
        let declared = || declaration(json!({ "network": true }));
        let fetching = || fetching_saved_then(r#"{"found":true}"#);
        let web = Replay::with(vec![Ok(stub_answer())]);
        let first = PluginGenerator::new("sample", declared(), fetching(), host(&web, cache.path()));
        let answer = first.step("starters", Values::new()).unwrap();
        assert_eq!(answer.answer["found"], true);
        assert!(!answer.from_saved);
        assert_eq!(web.asked(), 1);

        expire(cache.path());
        let web = Replay::with(Vec::new());
        let off = GeneratorEnvironment { network: false, ..GeneratorEnvironment::default() };
        let host = GeneratorHost { environment: off, ..host(&web, cache.path()) };
        let answer = PluginGenerator::new("sample", declared(), fetching(), host).step("starters", Values::new());
        assert!(answer.unwrap().from_saved, "with the network off the saved answer serves and says so");
        assert_eq!(web.asked(), 0, "the switch asks nothing");
    }

    #[test]
    fn a_plugin_cannot_choose_the_client_s_name() {
        let web = Replay::with(vec![Ok(Fetched::default())]);
        let generator = archive_generator(&web, true);
        let headers = BTreeMap::from([("user-agent".to_owned(), "other".to_owned())]);
        generator.web(SERVICE, &headers).unwrap();
        assert_eq!(web.asked.lock().unwrap()[0].1, [("User-Agent".to_owned(), CLIENT.to_owned())]);
    }

    #[test]
    fn a_setting_has_its_default_until_the_person_gives_a_value() {
        let extra =
            json!({ "settings": [{ "id": "serviceUrl", "label": "URL", "default": "https://start.spring.io" }] });
        let mut generator = generator(extra, answering("{}"));
        assert_eq!(generator.settings()["serviceUrl"], "https://start.spring.io");
        assert_eq!(generator.info().settings[0].key, "sample.vite.serviceUrl");
        let settings = BTreeMap::from([("sample.vite.serviceUrl".to_owned(), " https://own.example ".to_owned())]);
        generator.host.environment.settings = settings;
        assert_eq!(generator.settings()["serviceUrl"], "https://own.example");
    }
}
