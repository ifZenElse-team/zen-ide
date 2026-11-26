use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_modules::assist::{AssistContext, LanguageAssist};
use zen_syntax::{Suggestion, Suggestions};

use super::GoModInspector;
use crate::compare_versions;
use crate::gomod::{GoMod, is_directory_path, parse_go_mod, parse_go_work};
use crate::modcache::module_dir;
use crate::provider::{is_release, resolve};

pub type ModuleCache = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;

#[derive(Clone)]
pub struct GoModAssist {
    inspector: GoModInspector,
    module_cache: ModuleCache,
}

impl GoModAssist {
    pub fn new(inspector: GoModInspector, module_cache: ModuleCache) -> Self {
        GoModAssist { inspector, module_cache }
    }
}

const BLOCKS: &[&str] = &["require", "exclude", "replace", "use"];

fn open_block(before_line: &str) -> Option<&str> {
    let mut open = None;
    for line in before_line.lines() {
        let code = line.split("//").next().unwrap_or_default().trim();
        if code == ")" {
            open = None;
        } else if let Some(word) = code.strip_suffix('(').map(str::trim)
            && BLOCKS.contains(&word)
        {
            open = Some(word);
        }
    }
    open
}

struct Line<'a> {
    start: usize,
    text: &'a str,
    directive: Option<&'a str>,
    body: usize,
}

fn line_at(text: &str, offset: usize) -> Option<Line<'_>> {
    let start = text.get(..offset)?.rfind('\n').map_or(0, |at| at + 1);
    let line = text[start..].split('\n').next().unwrap_or_default();
    let code = line.split("//").next().unwrap_or_default();
    if offset - start > code.len() {
        return None;
    }
    if let Some(directive) = open_block(&text[..start]) {
        return Some(Line { start, text: code, directive: Some(directive), body: 0 });
    }
    let first = code.split_whitespace().next()?;
    let word = BLOCKS.iter().find(|word| **word == first)?;
    Some(Line { start, text: code, directive: Some(word), body: code.find(first)? + first.len() })
}

fn versioned_module<'a>(line: &Line<'a>, caret: usize) -> Option<&'a str> {
    let before = line.text.get(line.body..caret.checked_sub(line.start)?)?;
    let mut words: Vec<&str> = before.split_whitespace().collect();
    if !before.ends_with(char::is_whitespace) {
        words.pop();
    }
    match line.directive? {
        "require" | "exclude" => (words.len() == 1).then(|| words[0]),
        "replace" => match words.as_slice() {
            [.., "=>", target] if !is_directory_path(target) => Some(target),
            [module] => Some(module),
            _ => None,
        },
        _ => None,
    }
}

fn typed(text: &str, caret: usize) -> Option<&str> {
    let before = text.get(..caret)?;
    let from = before.rfind(char::is_whitespace).map_or(0, |at| at + 1);
    let typed = &before[from..];
    (typed.is_empty() || typed.starts_with('v')).then_some(typed)
}

fn word_at(line: &str, column: usize) -> Option<(usize, &str)> {
    let is_end = |c: char| c.is_whitespace() || c == '"' || c == '`';
    let from = line.get(..column)?.rfind(is_end).map_or(0, |at| at + 1);
    let to = line[column..].find(is_end).map_or(line.len(), |at| column + at);
    (from < to).then(|| (from, &line[from..to]))
}

fn workspace_member(go_mod: &Path, module: &str) -> Option<PathBuf> {
    let start = go_mod.parent()?;
    start.ancestors().find_map(|dir| {
        let file = dir.join("go.work");
        let text = std::fs::read_to_string(&file).ok()?;
        parse_go_work(&file, &text).uses.iter().find_map(|used| {
            let folder = resolve(dir, &used.path);
            let manifest = folder.join("go.mod");
            let own = std::fs::read_to_string(&manifest).ok()?;
            let found = parse_go_mod(&manifest, &own).module?;
            (found.path == module).then_some(folder)
        })
    })
}

fn required_version<'a>(parsed: &'a GoMod, module: &str) -> Option<&'a str> {
    parsed.requires.iter().find(|require| require.path == module).map(|require| require.version.as_str())
}

impl GoModAssist {
    fn folder(&self, path: &Path, text: &str, module: &str, version: Option<&str>) -> Result<PathBuf, String> {
        let dir = path.parent().unwrap_or(Path::new(""));
        let missing = || t!("go-module-not-downloaded", module = module);
        if let Some(member) = workspace_member(path, module) {
            return Ok(member);
        }
        let parsed = parse_go_mod(path, text);
        let version = version.or_else(|| required_version(&parsed, module)).unwrap_or_default();
        let replaced = parsed
            .replaces
            .iter()
            .filter(|replace| replace.matches(module, version))
            .max_by_key(|replace| replace.version.is_some());
        let (target, version) = match replaced {
            Some(replace) if replace.is_local() => return Ok(resolve(dir, &replace.new_path)),
            Some(replace) => (replace.new_path.as_str(), replace.new_version.as_deref().unwrap_or_default()),
            None => (module, version),
        };
        if version.is_empty() {
            return Err(missing());
        }
        (self.module_cache)().map(|cache| module_dir(&cache, target, version)).ok_or_else(missing)
    }
}

impl LanguageAssist for GoModAssist {
    fn handles(&self, file_name: &str) -> bool {
        file_name == "go.mod" || file_name == "go.work"
    }

    fn complete(&self, context: &AssistContext<'_>) -> Option<Suggestions> {
        let line = line_at(context.text, context.offset)?;
        let module = versioned_module(&line, context.offset)?;
        let typed = typed(context.text, context.offset)?;
        let parsed = parse_go_mod(context.path, context.text);
        let excluded =
            |version: &str| parsed.excludes.iter().any(|exclude| exclude.path == module && exclude.version == version);
        let mut versions = self.inspector.known_versions(module)?;
        versions.retain(|version| !excluded(version));
        let newest = versions.iter().find(|version| is_release(version)).cloned();
        let items: Vec<Suggestion> = versions
            .into_iter()
            .map(|version| Suggestion {
                detail: (Some(&version) == newest.as_ref()).then(|| t!("go-module-latest")),
                label: version.clone(),
                kind: "value".to_owned(),
                insert: version,
                caret: None,
            })
            .collect();
        (!items.is_empty()).then(|| Suggestions { from: context.offset - typed.len(), items })
    }

    fn declaration(&self, context: &AssistContext<'_>) -> Option<Result<PathBuf, String>> {
        let line = line_at(context.text, context.offset)?;
        let column = context.offset - line.start;
        let (at, word) = word_at(line.text, column)?;
        if at < line.body
            || word == "=>"
            || word.starts_with('v') && word[1..].starts_with(|c: char| c.is_ascii_digit())
        {
            return None;
        }
        let dir = context.path.parent().unwrap_or(Path::new(""));
        let folder = if line.directive == Some("use") || is_directory_path(word) {
            Ok(resolve(dir, word))
        } else {
            let words: Vec<&str> = line.text[line.body..].split_whitespace().collect();
            let next = words.iter().position(|found| *found == word).and_then(|index| words.get(index + 1));
            let version = next.copied().filter(|next| *next != "=>");
            self.folder(context.path, context.text, word, version)
        };
        Some(folder.and_then(|folder| {
            let manifest = folder.join("go.mod");
            if manifest.is_file() { Ok(manifest) } else { Err(t!("go-module-not-downloaded", module = word)) }
        }))
    }
}

impl GoModInspector {
    pub fn known_versions(&self, module: &str) -> Option<Vec<String>> {
        let state = super::lock(&self.shared.state);
        let suffix = format!(" {module}");
        let (key, slot) = state
            .versions
            .iter()
            .filter(|(key, slot)| key.ends_with(&suffix) && slot.value.as_ref().is_some_and(Option::is_some))
            .max_by_key(|(_, slot)| slot.at)?;
        let answer = slot.value.clone().flatten()?;
        let info = answer.latest().and_then(|latest| {
            state.modfiles.get(&format!("{key} {latest}")).and_then(|slot| slot.value.clone().flatten())
        });
        let retracted = |version: &str| info.as_ref().is_some_and(|info| info.retraction(version).is_some());
        let mut versions: Vec<String> = answer.list.iter().filter(|version| !retracted(version)).cloned().collect();
        versions.sort_by(|a, b| compare_versions(b, a));
        if versions.is_empty() {
            versions.extend(answer.latest);
        }
        Some(versions)
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VersionsParams {
    /// The module's path, e.g. `github.com/go-chi/chi/v5`.
    pub module: String,
}

/// The versions of a module, as the Go module proxy last gave them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnownVersions {
    /// The module's path.
    pub module: String,
    /// Its versions, newest first, without those its authors retracted; empty when none are known.
    pub versions: Vec<String>,
    /// Whether the proxy has answered for it: a module becomes known once a go.mod that requires it
    /// is open and the check for newer versions is on.
    pub known: bool,
}

pub struct ModuleVersions;

impl Command for ModuleVersions {
    type Params = VersionsParams;
    type Output = KnownVersions;
    const ID: &'static str = "go.module.versions";
    const TITLE: &'static str = "Go Module Versions";
    const DESCRIPTION: &'static str = "The versions of a Go module the go.mod checks know, newest first, without \
         those its authors retracted. It contacts nothing: a go.mod that requires the module must be open with \
         go.dependencies.checkUpdates on. Update a requirement with problems.fix.apply, complete one with \
         editor.completion.start, and open a module's go.mod with navigate.declaration.goTo.";

    fn run(&self, session: &mut Session, params: VersionsParams) -> Result<KnownVersions, CommandError> {
        let inspector =
            session.state::<GoModInspector>().ok_or_else(|| CommandError::new(t!("toolchain-error-none")))?;
        let versions = inspector.known_versions(&params.module);
        Ok(KnownVersions { known: versions.is_some(), versions: versions.unwrap_or_default(), module: params.module })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::proxy::Versions;
    use super::super::{ModInfo, Services, Slot, now, versions_key};
    use super::*;

    struct Offline;

    impl super::super::proxy::Fetch for Offline {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, String> {
            Err("offline".to_owned())
        }
    }

    const MODULE: &str = "github.com/acme/lib";

    fn assist(cache: Option<&Path>) -> GoModAssist {
        let inspector = GoModInspector::new(Arc::new(Offline), Services::default());
        {
            let mut state = super::super::lock(&inspector.shared.state);
            let key = versions_key("https://proxy.golang.org", MODULE);
            let list = ["v1.0.0", "v1.2.0", "v1.10.0", "v1.11.0", "v2.0.0-rc.1"].map(str::to_owned).to_vec();
            let answer = Versions { list, latest: None };
            state.versions.insert(key.clone(), Slot { value: Some(Some(answer)), at: now(), failed: false });
            let info =
                ModInfo { deprecated: None, retracted: vec![("v1.11.0".to_owned(), "v1.11.0".to_owned(), None)] };
            let latest = format!("{key} v1.11.0");
            state.modfiles.insert(latest, Slot { value: Some(Some(info)), at: now(), failed: false });
        }
        let cache = cache.map(Path::to_owned);
        GoModAssist::new(inspector, Arc::new(move || cache.clone()))
    }

    fn labels(assist: &GoModAssist, text: &str) -> Option<(usize, Vec<String>)> {
        let offset = text.find('|').unwrap();
        let text = text.replace('|', "");
        let context = AssistContext { path: Path::new("/w/go.mod"), text: &text, offset };
        assist.complete(&context).map(|found| (found.from, found.items.into_iter().map(|item| item.label).collect()))
    }

    #[test]
    fn a_requirement_completes_the_versions_the_proxy_gave_newest_first() {
        zen_i18n::register(crate::LOCALES);
        let assist = assist(None);
        let text = "module a\n\nrequire github.com/acme/lib v|\n";
        let (from, found) = labels(&assist, text).unwrap();
        assert_eq!(from, text.find('v').unwrap());
        assert_eq!(found[0], "v2.0.0-rc.1");
        assert_eq!(found.iter().filter(|version| version.as_str() == "v1.2.0").count(), 1);
        assert!(found.iter().position(|v| v == "v1.10.0") < found.iter().position(|v| v == "v1.2.0"));
        assert!(!found.iter().any(|version| version == "v1.11.0"), "a retracted version is not offered: {found:?}");
    }

    #[test]
    fn a_block_of_requirements_and_a_replacement_complete_too() {
        let assist = assist(None);
        let block = "module a\n\nrequire (\n\tgithub.com/acme/lib |\n)\n";
        assert!(labels(&assist, block).is_some());
        let replace = "replace example.com/old => github.com/acme/lib v1|\n";
        assert!(labels(&assist, replace).is_some());
        let local = "replace example.com/old => ../lib |\n";
        assert_eq!(labels(&assist, local), None);
    }

    #[test]
    fn an_excluded_version_is_not_offered() {
        let assist = assist(None);
        let text = "exclude github.com/acme/lib v1.2.0\nrequire github.com/acme/lib |\n";
        let (_, found) = labels(&assist, text).unwrap();
        assert!(!found.iter().any(|version| version == "v1.2.0"), "{found:?}");
    }

    #[test]
    fn nothing_is_completed_outside_a_version_or_for_an_unknown_module() {
        let assist = assist(None);
        for text in [
            "module a|\n",
            "require github.com/acme/li|\n",
            "require github.com/acme/lib v1.0.0 // v|\n",
            "require github.com/other/lib v|\n",
            "go 1.2|\n",
        ] {
            assert_eq!(labels(&assist, text), None, "{text}");
        }
    }

    fn declaration(assist: &GoModAssist, path: &Path, text: &str) -> Option<Result<PathBuf, String>> {
        let offset = text.find('|').unwrap();
        let text = text.replace('|', "");
        assist.declaration(&AssistContext { path, text: &text, offset })
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_requirement_goes_to_the_go_mod_in_the_module_cache() {
        zen_i18n::register(crate::LOCALES);
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("cache");
        let manifest = cache.join("github.com/acme/lib@v1.0.0/go.mod");
        write(&manifest, "module github.com/acme/lib\n");
        let assist = assist(Some(&cache));
        let path = temp.path().join("go.mod");
        let found = declaration(&assist, &path, "require github.com/ac|me/lib v1.0.0\n");
        assert_eq!(found, Some(Ok(manifest)));
        let not_there = declaration(&assist, &path, "require github.com/ac|me/lib v1.5.0\n").unwrap();
        assert_eq!(not_there, Err(t!("go-module-not-downloaded", module = MODULE)));
        assert_eq!(declaration(&assist, &path, "require github.com/acme/lib v1.0|.0\n"), None);
        assert_eq!(declaration(&assist, &path, "modu|le a\n"), None);
    }

    #[test]
    fn a_replacement_and_a_workspace_use_go_to_their_folders() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("app/go.mod");
        let fork = temp.path().join("fork/go.mod");
        write(&fork, "module fork\n");
        write(&temp.path().join("member/go.mod"), "module example.com/member\n");
        write(&temp.path().join("go.work"), "use (\n\t./app\n\t./member\n)\n");
        let assist = assist(None);
        let text = "require github.com/ac|me/lib v1.0.0\nreplace github.com/acme/lib => ../fork\n";
        assert_eq!(declaration(&assist, &path, text), Some(Ok(fork.clone())));
        assert_eq!(declaration(&assist, &path, "replace github.com/acme/lib => ../fo|rk\n"), Some(Ok(fork)));
        let member = temp.path().join("member/go.mod");
        assert_eq!(declaration(&assist, &path, "require example.com/mem|ber v0.0.0\n"), Some(Ok(member.clone())));
        let work = temp.path().join("go.work");
        assert_eq!(declaration(&assist, &work, "use (\n\t./mem|ber\n)\n"), Some(Ok(member)));
    }

    #[test]
    fn versions_tell_an_agent_what_is_known() {
        let assist = assist(None);
        let mut session = Session::new();
        session.set_state(assist.inspector.clone());
        let known = ModuleVersions.run(&mut session, VersionsParams { module: MODULE.to_owned() }).unwrap();
        assert!(known.known);
        assert_eq!(known.versions.first().map(String::as_str), Some("v2.0.0-rc.1"));
        let unknown = ModuleVersions.run(&mut session, VersionsParams { module: "a.b/c".to_owned() }).unwrap();
        assert_eq!((unknown.known, unknown.versions.len()), (false, 0));
    }
}
