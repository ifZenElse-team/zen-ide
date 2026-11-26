use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use schemars::JsonSchema;
use serde::Serialize;
use zen_i18n::t;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum VirtualKind {
    /// A library's source file, read from its sources archive.
    Sources,
    /// A view of compiled code: declarations without bodies.
    Decompiled,
    /// The bytecode of a class compiled from the project, as the JDK's javap prints it.
    Bytecode,
    /// A file a build tool generated to be read, such as an effective POM.
    Generated,
    /// The standard output of a plug-in's run target, opened as a document (`"output": "document"`).
    Output,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualDocument {
    pub kind: VirtualKind,
    pub origin: String,
    pub library: Option<PathBuf>,
    pub bytecode: Option<u16>,
    pub class_path: Option<PathBuf>,
}

static DOCUMENTS: RwLock<BTreeMap<PathBuf, VirtualDocument>> = RwLock::new(BTreeMap::new());
static FOLDER: RwLock<Option<PathBuf>> = RwLock::new(None);

pub fn set_folder(folder: Option<PathBuf>) {
    *FOLDER.write().unwrap_or_else(PoisonError::into_inner) = folder;
}

pub fn folder() -> PathBuf {
    FOLDER
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("zen-libraries"))
}

pub fn get(path: &Path) -> Option<VirtualDocument> {
    if let Some(found) = DOCUMENTS.read().unwrap_or_else(PoisonError::into_inner).get(path).cloned() {
        return Some(found);
    }
    let inside = path.strip_prefix(folder()).ok()?;
    let first = inside.components().next()?.as_os_str();
    let kind = match first.to_str() {
        Some("decompiled") => VirtualKind::Decompiled,
        Some("bytecode") => VirtualKind::Bytecode,
        Some(crate::maven::workspace::EFFECTIVE_FOLDER) => VirtualKind::Generated,
        _ => VirtualKind::Sources,
    };
    Some(VirtualDocument {
        kind,
        origin: path.to_string_lossy().into_owned(),
        library: None,
        bytecode: None,
        class_path: None,
    })
}

pub fn reason(path: &Path) -> Option<String> {
    let document = get(path)?;
    let file = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    Some(match document.kind {
        VirtualKind::Sources => t!("workbench-library-read-only", file = file),
        VirtualKind::Decompiled => t!("workbench-library-decompiled-read-only", file = file),
        VirtualKind::Bytecode => t!("workbench-bytecode-read-only", file = file),
        VirtualKind::Generated => t!("workbench-generated-read-only", file = file),
        VirtualKind::Output => t!("workbench-output-read-only", file = file, command = document.origin.as_str()),
    })
}

pub fn served(path: &Path) -> bool {
    get(path).is_none() || zen_lsp::position::mapped_uri(path).is_some()
}

pub fn hashed(text: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn store(
    folder: &Path,
    relative: &Path,
    text: &str,
    document: VirtualDocument,
    uri: Option<&str>,
) -> std::io::Result<PathBuf> {
    let path = folder.join(relative);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, text)?;
    }
    DOCUMENTS.write().unwrap_or_else(PoisonError::into_inner).insert(path.clone(), document);
    if let Some(uri) = uri {
        zen_lsp::position::map_uri(uri, &path);
    }
    Ok(path)
}

pub fn from_server(folder: &Path, uri: &str, text: &str, extension: &str) -> std::io::Result<PathBuf> {
    let scheme = zen_lsp::position::scheme(uri).unwrap_or("virtual");
    let before_query = uri.split(['?', '#']).next().unwrap_or(uri);
    let last = before_query.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or("document");
    let last = percent_decoded(last);
    let stem = last.rsplit_once('.').map_or(last.as_str(), |(stem, _)| stem);
    let name = if extension.is_empty() { last.clone() } else { format!("{stem}.{extension}") };
    let kind = if is_disassembled(text) { VirtualKind::Decompiled } else { VirtualKind::Sources };
    let relative = Path::new(scheme).join(hashed(uri)).join(name);
    store(
        folder,
        &relative,
        text,
        VirtualDocument { kind, origin: uri.to_owned(), library: None, bytecode: None, class_path: None },
        Some(uri),
    )
}

fn is_disassembled(text: &str) -> bool {
    text.lines().take(5).any(|line| line.contains("Failed to get sources") || line.contains("Compiled from"))
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(byte) = text.get(at + 1..at + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_document_is_kept_as_a_read_only_file_that_speaks_its_uri() {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let uri = "jdt://contents/java.base/java.util/ArrayList.class?=demo/%5C/jdk%3Cjava.util(ArrayList.class";
        let path = from_server(dir.path(), uri, "package java.util;\npublic class ArrayList {}\n", "java").unwrap();
        assert!(path.starts_with(dir.path().join("jdt")));
        assert_eq!(path.file_name().unwrap(), "ArrayList.java");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "package java.util;\npublic class ArrayList {}\n");
        assert_eq!(zen_lsp::position::file_uri(&path), uri);
        assert_eq!(zen_lsp::position::uri_path(uri).as_deref(), Some(path.as_path()));
        assert_eq!(get(&path).unwrap().kind, VirtualKind::Sources);
        assert!(served(&path), "the server knows it by its URI");
        assert!(reason(&path).is_some_and(|reason| reason.contains("ArrayList.java")));

        let stub = "// Failed to get sources. Instead, stub sources have been generated.\nclass Gone {}\n";
        let other = from_server(dir.path(), "jdt://contents/x.jar/a/Gone.class", stub, "java").unwrap();
        assert_eq!(get(&other).unwrap().kind, VirtualKind::Decompiled);

        let local = store(
            dir.path(),
            Path::new("decompiled/x/Local.java"),
            "class Local {}\n",
            VirtualDocument {
                kind: VirtualKind::Decompiled,
                origin: "x".to_owned(),
                library: None,
                bytecode: None,
                class_path: None,
            },
            None,
        )
        .unwrap();
        assert!(!served(&local), "no server is told of a document it cannot read");
        assert!(served(&dir.path().join("plain.java")));
        assert!(reason(&dir.path().join("plain.java")).is_none());
        let kept = folder().join("jdk/jdk-21/java.base/java/lang/String.java");
        assert_eq!(
            get(&kept).map(|found| found.kind),
            Some(VirtualKind::Sources),
            "the cache is read-only after a restart"
        );
        let decompiled = folder().join("decompiled/x/A.java");
        assert_eq!(get(&decompiled).map(|found| found.kind), Some(VirtualKind::Decompiled));
        assert!(!served(&kept), "and never sent to a server without its URI");
    }
}
