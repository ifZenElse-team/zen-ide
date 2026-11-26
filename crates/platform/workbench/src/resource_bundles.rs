pub mod format;
mod references;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;
use crate::language::edits::{Applied, FileEdit, TextChange};
use crate::language::places::Place;
use crate::language::refactor::Refactoring;
use crate::language::with_languages;

pub(crate) use references::{complete, completes_in, go_to};

pub const EXTENSION: &str = "properties";

/// One file of a resource bundle: the texts of one locale.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleFile {
    /// The locale its name ends with, e.g. `ru` or `de_DE`; empty for the default file (`messages.properties`).
    pub locale: String,
    /// The file.
    pub path: PathBuf,
}

/// A resource bundle: the files `name.properties`, `name_ru.properties`, `name_de_DE.properties`... of one folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    /// Its base name, e.g. `messages`.
    pub name: String,
    /// The folder its files are in.
    pub folder: PathBuf,
    /// Its files, the default one first, then by locale.
    pub files: Vec<BundleFile>,
}

impl Bundle {
    pub fn path(&self) -> PathBuf {
        self.folder.join(format!("{}.{EXTENSION}", self.name))
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.files.iter().any(|file| file.path == path)
    }
}

fn is_locale(locale: &str) -> bool {
    let parts: Vec<&str> = locale.split('_').collect();
    let language = parts[0].len() == 2 && parts[0].bytes().all(|b| b.is_ascii_lowercase());
    let country = parts.get(1).is_none_or(|country| {
        (country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase()))
            || (country.len() == 3 && country.bytes().all(|b| b.is_ascii_digit()))
            || (country.is_empty() && parts.len() > 2)
    });
    let variant = parts.iter().skip(2).all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()));
    language && country && variant
}

pub fn split_name(file_name: &str) -> Option<(&str, &str)> {
    let stem = file_name.strip_suffix(".properties")?;
    if stem.is_empty() {
        return None;
    }
    for (at, _) in stem.match_indices('_') {
        let (base, locale) = (&stem[..at], &stem[at + 1..]);
        if !base.is_empty() && is_locale(locale) {
            return Some((base, locale));
        }
    }
    Some((stem, ""))
}

pub fn group(paths: &[PathBuf]) -> Vec<Bundle> {
    let mut found: BTreeMap<(PathBuf, String), Vec<BundleFile>> = BTreeMap::new();
    for path in paths {
        let (Some(folder), Some(name)) = (path.parent(), path.file_name().and_then(|name| name.to_str())) else {
            continue;
        };
        if let Some((base, locale)) = split_name(name) {
            found
                .entry((folder.to_owned(), base.to_owned()))
                .or_default()
                .push(BundleFile { locale: locale.to_owned(), path: path.clone() });
        }
    }
    found
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|((folder, name), mut files)| {
            files.sort_by(|a, b| a.locale.cmp(&b.locale));
            Bundle { name, folder, files }
        })
        .collect()
}

pub fn in_folder(folder: &Path) -> Vec<Bundle> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == EXTENSION))
        .collect();
    group(&paths)
}

pub fn bundle_of(path: &Path) -> Option<Bundle> {
    let name = path.file_name()?.to_str()?;
    let (base, _) = split_name(name)?;
    in_folder(path.parent()?).into_iter().find(|bundle| bundle.name == base)
}

pub fn grouped_in_tree(session: &Session) -> bool {
    session
        .state::<crate::plugins::PluginFeatures>()
        .is_some_and(|features| features.all().iter().any(|plugin| plugin.handles("messages.properties")))
}

fn text_of(session: &Session, path: &Path) -> Result<String, CommandError> {
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    crate::language::file_text(workbench, path).map_err(CommandError::new)
}

/// One key of a resource bundle with its value in each file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleKey {
    /// The key, unescaped.
    pub key: String,
    /// Its value in each file, in the order of `locales`, unescaped; null where the file lacks the key.
    pub values: Vec<Option<String>>,
    /// The locales whose file lacks the key or gives it an empty value: translations still missing.
    pub missing: Vec<String>,
}

/// What `properties.bundle.get` answers: the table of the Resource Bundle editor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleTable {
    /// The bundle's base name, e.g. `messages`.
    pub name: String,
    /// The path that stands for the bundle: its default file, `<folder>/<name>.properties`.
    pub path: PathBuf,
    /// Its files, one column each, the default one first.
    pub locales: Vec<BundleFile>,
    /// Its keys: those of the default file in their order, then the others' in theirs.
    pub keys: Vec<BundleKey>,
    /// How many keys miss a translation somewhere.
    pub incomplete: usize,
}

fn guard_hidden(session: &Session, bundle: &Bundle) -> Result<(), CommandError> {
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
    let paths: Vec<&Path> = bundle.files.iter().map(|file| file.path.as_path()).collect();
    crate::hidden_files::guard_change(session, &root, &paths)
}

pub fn table(session: &Session, bundle: &Bundle) -> Result<BundleTable, CommandError> {
    guard_hidden(session, bundle)?;
    let mut columns = Vec::new();
    for file in &bundle.files {
        let text = text_of(session, &file.path)?;
        columns.push(format::read(&text));
    }
    let mut order: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for entries in &columns {
        for entry in entries {
            if seen.insert(entry.key.clone()) {
                order.push(entry.key.clone());
            }
        }
    }
    let keys: Vec<BundleKey> = order
        .into_iter()
        .map(|key| {
            let values: Vec<Option<String>> = columns
                .iter()
                .map(|entries| entries.iter().rfind(|entry| entry.key == key).map(|entry| entry.value.clone()))
                .collect();
            let missing = bundle
                .files
                .iter()
                .zip(&values)
                .filter(|(_, value)| value.as_deref().is_none_or(str::is_empty))
                .map(|(file, _)| file.locale.clone())
                .collect();
            BundleKey { key, values, missing }
        })
        .collect();
    let incomplete = keys.iter().filter(|key| !key.missing.is_empty()).count();
    Ok(BundleTable { name: bundle.name.clone(), path: bundle.path(), locales: bundle.files.clone(), keys, incomplete })
}

fn find(path: &Path) -> Result<Bundle, CommandError> {
    bundle_of(path).ok_or_else(|| CommandError::new(t!("workbench-bundle-none", path = path.display().to_string())))
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

pub(crate) fn change(path: &Path, before: &str, after: &str) -> Option<FileEdit> {
    if before == after {
        return None;
    }
    let prefix = before.bytes().zip(after.bytes()).take_while(|(a, b)| a == b).count();
    let prefix = (0..=prefix).rev().find(|at| before.is_char_boundary(*at) && after.is_char_boundary(*at))?;
    let room = before.len().min(after.len()) - prefix;
    let suffix = before.bytes().rev().zip(after.bytes().rev()).take(room).take_while(|(a, b)| a == b).count();
    let suffix = (0..=suffix)
        .rev()
        .find(|n| before.is_char_boundary(before.len() - n) && after.is_char_boundary(after.len() - n))?;
    let start = char_count(&before[..prefix]);
    let end = start + char_count(&before[prefix..before.len() - suffix]);
    let place = Place { path: path.to_owned(), start, end, line: 0, column: 0, preview: None, highlight: None };
    Some(FileEdit {
        path: path.to_owned(),
        uri: String::new(),
        before: before.to_owned(),
        server_version: None,
        changes: vec![TextChange { place, new_text: after[prefix..after.len() - suffix].to_owned() }],
    })
}

fn edit(
    session: &mut Session,
    bundle: &Bundle,
    rewrite: impl Fn(&BundleFile, &str) -> String,
) -> Result<usize, CommandError> {
    guard_hidden(session, bundle)?;
    let mut files = Vec::new();
    for file in &bundle.files {
        let before = text_of(session, &file.path)?;
        let after = rewrite(file, &before);
        files.extend(change(&file.path, &before, &after).map(crate::language::edits::Step::Text));
    }
    if files.is_empty() {
        return Ok(0);
    }
    session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
    let applied =
        with_languages(session, |languages, workbench| crate::language::edits::apply(&files, languages, workbench))
            .map_err(|error| CommandError::new(error.message()))?;
    let count = applied.len();
    session.state_mut::<Refactoring>().record(applied);
    Ok(count)
}

fn check_key(key: &str) -> Result<(), CommandError> {
    if key.is_empty() {
        return Err(CommandError::new(t!("workbench-bundle-empty-key")));
    }
    Ok(())
}

fn table_of(session: &Session, path: &Path) -> Result<BundleTable, CommandError> {
    table(session, &find(path)?)
}

/// The parameters of a command on a resource bundle.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleParams {
    /// Any file of the bundle, or `<folder>/<name>.properties` for the bundle `name`.
    pub path: PathBuf,
}

pub struct GetBundle;

impl Command for GetBundle {
    type Params = BundleParams;
    type Output = BundleTable;
    const ID: &'static str = "properties.bundle.get";
    const TITLE: &'static str = "Resource Bundle";
    const DESCRIPTION: &'static str = "The table of a resource bundle (the files name.properties, name_ru.properties, \
         name_de_DE.properties... of one folder): its keys with the value in each file, unescaped, and the \
         locales that miss a translation (the key absent or empty).";

    fn run(&self, session: &mut Session, params: BundleParams) -> Result<BundleTable, CommandError> {
        table_of(session, &params.path)
    }
}

/// The parameters of `properties.bundle.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// A folder whose bundles are listed; the whole project when absent.
    pub folder: Option<PathBuf>,
}

pub struct ListBundles;

impl Command for ListBundles {
    type Params = ListParams;
    type Output = Vec<Bundle>;
    const ID: &'static str = "properties.bundle.list";
    const TITLE: &'static str = "Resource Bundles";
    const DESCRIPTION: &'static str = "The resource bundles of a folder or of the whole project: each base name with \
         two files or more (name.properties, name_<locale>.properties) in one folder, with its files by locale.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<Vec<Bundle>, CommandError> {
        if let Some(folder) = params.folder {
            return Ok(in_folder(&folder));
        }
        let paths =
            references::project_files(session, |path| path.extension().is_some_and(|extension| extension == EXTENSION));
        Ok(group(&paths))
    }
}

/// The parameters of `properties.bundle.setValue`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetValueParams {
    /// Any file of the bundle.
    pub path: PathBuf,
    /// The key, unescaped.
    pub key: String,
    /// The locale of the file to change, as `properties.bundle.get` names it; empty for the default file.
    #[serde(default)]
    pub locale: String,
    /// The new value, unescaped; it is written escaped.
    pub value: String,
}

pub struct SetValue;

impl Command for SetValue {
    type Params = SetValueParams;
    type Output = BundleTable;
    const ID: &'static str = "properties.bundle.setValue";
    const TITLE: &'static str = "Set Translation";
    const DESCRIPTION: &'static str = "Sets a key's value in one file of a resource bundle (adding the key to that \
         file when it lacks it), escaped as java.util.Properties reads it; one undo step.";

    fn run(&self, session: &mut Session, params: SetValueParams) -> Result<BundleTable, CommandError> {
        check_key(&params.key)?;
        let bundle = find(&params.path)?;
        if !bundle.files.iter().any(|file| file.locale == params.locale) {
            return Err(CommandError::new(t!("workbench-bundle-no-locale", locale = params.locale.as_str())));
        }
        edit(session, &bundle, |file, text| {
            if file.locale == params.locale {
                format::with_value(text, &params.key, &params.value)
            } else {
                text.to_owned()
            }
        })?;
        table(session, &bundle)
    }
}

/// The parameters of `properties.bundle.addKey`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyParams {
    /// Any file of the bundle.
    pub path: PathBuf,
    /// The new key, unescaped.
    pub key: String,
    /// Its value in the default file; empty by default. The other files get it empty, marked as missing.
    #[serde(default)]
    pub value: String,
}

pub struct AddKey;

impl Command for AddKey {
    type Params = AddKeyParams;
    type Output = BundleTable;
    const ID: &'static str = "properties.bundle.addKey";
    const TITLE: &'static str = "Add Property";
    const DESCRIPTION: &'static str = "Adds a key to every file of a resource bundle that lacks it, at its end: the \
         value in the default file (or the first one), empty in the others; one undo step.";

    fn run(&self, session: &mut Session, params: AddKeyParams) -> Result<BundleTable, CommandError> {
        check_key(&params.key)?;
        let bundle = find(&params.path)?;
        let first = bundle.files.first().map(|file| file.locale.clone()).unwrap_or_default();
        edit(session, &bundle, |file, text| {
            if format::read(text).iter().any(|entry| entry.key == params.key) {
                return text.to_owned();
            }
            let value = if file.locale == first { params.value.as_str() } else { "" };
            format::appended(text, &params.key, value)
        })?;
        table(session, &bundle)
    }
}

/// The parameters of `properties.bundle.renameKey`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameKeyParams {
    /// Any file of the bundle.
    pub path: PathBuf,
    /// The key, unescaped.
    pub key: String,
    /// Its new name, unescaped.
    pub new_key: String,
}

pub struct RenameKey;

impl Command for RenameKey {
    type Params = RenameKeyParams;
    type Output = BundleTable;
    const ID: &'static str = "properties.bundle.renameKey";
    const TITLE: &'static str = "Rename Property";
    const DESCRIPTION: &'static str = "Renames a key in every file of a resource bundle; one undo step. Refuses a new \
         name that a file already has.";

    fn run(&self, session: &mut Session, params: RenameKeyParams) -> Result<BundleTable, CommandError> {
        check_key(&params.new_key)?;
        let bundle = find(&params.path)?;
        if params.key != params.new_key {
            for file in &bundle.files {
                if format::read(&text_of(session, &file.path)?).iter().any(|entry| entry.key == params.new_key) {
                    return Err(CommandError::new(t!("workbench-bundle-key-taken", key = params.new_key.as_str())));
                }
            }
        }
        edit(session, &bundle, |_, text| format::renamed(text, &params.key, &params.new_key))?;
        table(session, &bundle)
    }
}

/// The parameters of `properties.bundle.removeKey`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveKeyParams {
    /// Any file of the bundle.
    pub path: PathBuf,
    /// The key, unescaped.
    pub key: String,
}

pub struct RemoveKey;

impl Command for RemoveKey {
    type Params = RemoveKeyParams;
    type Output = BundleTable;
    const ID: &'static str = "properties.bundle.removeKey";
    const TITLE: &'static str = "Remove Property";
    const DESCRIPTION: &'static str = "Removes a key, with its value lines, from every file of a resource bundle; one \
         undo step.";

    fn run(&self, session: &mut Session, params: RemoveKeyParams) -> Result<BundleTable, CommandError> {
        let bundle = find(&params.path)?;
        edit(session, &bundle, |_, text| format::without(text, &params.key))?;
        table(session, &bundle)
    }
}

pub struct UndoBundle;

impl Command for UndoBundle {
    type Params = BundleParams;
    type Output = BundleTable;
    const ID: &'static str = "properties.bundle.undo";
    const TITLE: &'static str = "Undo";
    const DESCRIPTION: &'static str = "Undoes the last change made to a resource bundle's files by \
         properties.bundle.* (a value, an added, renamed or removed key) in all its files at once.";

    fn run(&self, session: &mut Session, params: BundleParams) -> Result<BundleTable, CommandError> {
        let bundle = find(&params.path)?;
        let entry = session
            .state_mut::<Refactoring>()
            .take_latest(|entry| entry.files.iter().all(|applied: &Applied| bundle.contains(applied.path())))
            .ok_or_else(|| CommandError::new(t!("workbench-bundle-nothing-to-undo")))?;
        with_languages(session, |languages, workbench| {
            crate::language::refactor::undo_entry(&entry, languages, workbench)
        });
        table(session, &bundle)
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetBundle)?;
    registry.register(ListBundles)?;
    registry.register(SetValue)?;
    registry.register(AddKey)?;
    registry.register(RenameKey)?;
    registry.register(RemoveKey)?;
    registry.register(UndoBundle)?;
    Ok(())
}

#[cfg(test)]
mod tests;
