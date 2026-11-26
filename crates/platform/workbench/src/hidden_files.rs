use std::path::{Component, Path, PathBuf};

use zen_core::{CommandError, Session};
use zen_i18n::t;

#[derive(Clone, Debug, Default)]
pub struct HiddenFromAgents {
    patterns: Vec<String>,
}

impl HiddenFromAgents {
    pub fn set(&mut self, patterns: &[String]) {
        self.patterns = patterns.iter().map(|pattern| pattern.trim().to_owned()).filter(|p| !p.is_empty()).collect();
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

#[derive(Clone, Default)]
pub struct AgentHiddenPaths(pub Option<HiddenPathsFn>);

pub type HiddenPathsFn = std::sync::Arc<dyn Fn(&Session) -> Vec<PathBuf> + Send + Sync>;

const FOLD_CASE: bool = cfg!(any(target_os = "macos", windows));

fn folded(text: &str) -> String {
    if FOLD_CASE { text.to_lowercase() } else { text.to_owned() }
}

pub fn hidden_by(patterns: &[String], relative: &str) -> bool {
    let relative = folded(relative);
    let mut hidden = false;
    for pattern in patterns {
        let pattern = folded(pattern);
        match pattern.strip_prefix('!') {
            Some(kept) if crate::language::compact::path_matches(kept, &relative) => return false,
            Some(_) => {}
            None => hidden = hidden || crate::language::compact::path_matches(&pattern, &relative),
        }
    }
    hidden
}

pub fn real_path(path: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(path) {
        return real;
    }
    let mut rest = Vec::new();
    let mut base = path.to_path_buf();
    loop {
        if let Ok(real) = std::fs::canonicalize(&base) {
            let mut joined = real;
            for part in rest.iter().rev() {
                joined.push(part);
            }
            return lexical(&joined);
        }
        match (base.file_name().map(std::ffi::OsStr::to_owned), base.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                base = parent;
            }
            _ => return lexical(path),
        }
    }
}

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

pub fn is_hidden(patterns: &[String], root: Option<&Path>, path: &Path) -> bool {
    if patterns.is_empty() || path.as_os_str().is_empty() {
        return false;
    }
    let given = path.to_string_lossy().replace('\\', "/");
    let absolute = match (path.is_absolute(), root) {
        (true, _) => path.to_path_buf(),
        (false, Some(root)) => root.join(path),
        (false, None) => return hidden_by(patterns, &lexical(path).to_string_lossy().replace('\\', "/")),
    };
    let real = real_path(&absolute);
    let relative = match root.map(real_path) {
        Some(real_root) => match real.strip_prefix(&real_root) {
            Ok(inside) => inside.to_string_lossy().replace('\\', "/"),
            Err(_) => real.to_string_lossy().replace('\\', "/"),
        },
        None => real.to_string_lossy().replace('\\', "/"),
    };
    hidden_by(patterns, &relative) || hidden_by(patterns, given.trim_start_matches("./"))
}

pub(crate) fn for_caller(session: &Session) -> Option<Vec<String>> {
    if !zen_core::macros::by_agent(session) {
        return None;
    }
    let mut patterns = session.state::<HiddenFromAgents>().map(|hidden| hidden.patterns().to_vec()).unwrap_or_default();
    if let Some(Some(paths)) = session.state::<AgentHiddenPaths>().map(|hidden| hidden.0.clone()) {
        patterns.extend(paths(session).iter().map(|path| real_path(path).to_string_lossy().replace('\\', "/")));
    }
    (!patterns.is_empty()).then_some(patterns)
}

pub(crate) fn count(session: &Session) -> Option<usize> {
    let patterns = for_caller(session)?;
    let workbench = session.state::<crate::Workbench>()?;
    let root = workbench.project().map(Path::to_path_buf);
    let index = workbench.index.as_ref()?;
    Some(index.file_list().iter().filter(|(path, _)| is_hidden(&patterns, root.as_deref(), path)).count())
}

pub const REDACTED_CODE: &str = "redacted";

const WALK_LIMIT: usize = 50_000;

pub fn refused(file: &str) -> CommandError {
    CommandError::with_code(REDACTED_CODE, t!("workbench-hidden-change", file = file))
        .with_data(serde_json::json!({ "file": file }))
}

pub fn hidden_inside(patterns: &[String], root: Option<&Path>, folder: &Path) -> Option<String> {
    hidden_within(patterns, root, folder, WALK_LIMIT)
}

fn hidden_within(patterns: &[String], root: Option<&Path>, folder: &Path, limit: usize) -> Option<String> {
    let folder = match (folder.is_absolute(), root) {
        (true, _) => folder.to_path_buf(),
        (false, Some(root)) => root.join(folder),
        (false, None) => folder.to_path_buf(),
    };
    if patterns.is_empty() || !folder.is_dir() {
        return None;
    }
    let shown = |path: &Path| {
        root.and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path).to_string_lossy().replace('\\', "/")
    };
    let mut stack = vec![folder.clone()];
    let mut seen = 0;
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > limit {
                return Some(shown(&folder));
            }
            let path = entry.path();
            if entry.file_name() == ".git" {
                continue;
            }
            if is_hidden(patterns, root, &path) {
                return Some(shown(&path));
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(path);
            }
        }
    }
    None
}

pub(crate) fn guard_read<P: AsRef<Path>>(session: &Session, paths: &[P]) -> Result<(), CommandError> {
    let Some(patterns) = for_caller(session) else { return Ok(()) };
    let root = session.state::<crate::Workbench>().and_then(crate::Workbench::project);
    match paths.iter().map(AsRef::as_ref).find(|path| is_hidden(&patterns, root, path)) {
        Some(path) => Err(refused(&path.to_string_lossy())),
        None => Ok(()),
    }
}

pub fn guard_change<P: AsRef<Path>>(session: &Session, root: &Path, paths: &[P]) -> Result<(), CommandError> {
    crate::trust::guard_agent_data_folder(session, paths)?;
    let Some(patterns) = for_caller(session) else { return Ok(()) };
    for path in paths {
        let path = path.as_ref();
        if is_hidden(&patterns, Some(root), path) {
            return Err(refused(&path.to_string_lossy()));
        }
        if let Some(inside) = hidden_inside(&patterns, Some(root), path) {
            return Err(refused(&inside));
        }
    }
    Ok(())
}

pub fn guard_copy<P: AsRef<Path>>(
    session: &Session,
    root: &Path,
    sources: &[P],
    directory: &Path,
) -> Result<(), CommandError> {
    guard_change(session, root, sources)?;
    crate::trust::guard_agent_data_folder(session, &[directory])?;
    let Some(patterns) = for_caller(session) else { return Ok(()) };
    for source in sources {
        let Some(name) = source.as_ref().file_name() else { continue };
        let target = directory.join(name);
        if is_hidden(&patterns, Some(root), &target) {
            return Err(refused(&target.to_string_lossy()));
        }
    }
    Ok(())
}

pub fn without_hidden(session: &Session, root: &Path, paths: Vec<String>) -> (Vec<String>, usize) {
    let Some(patterns) = for_caller(session) else { return (paths, 0) };
    let before = paths.len();
    let kept: Vec<String> =
        paths.into_iter().filter(|path| !is_hidden(&patterns, Some(root), Path::new(path))).collect();
    let left = before - kept.len();
    (kept, left)
}

pub fn guard_revision(session: &Session, repository: &zen_vcs::Repository, target: &str) -> Result<(), CommandError> {
    let Some(patterns) = for_caller(session) else { return Ok(()) };
    let Ok(files) = repository.diff_revisions("HEAD", Some(target)) else { return Ok(()) };
    guard_changed(&patterns, repository.root(), files)
}

pub fn guard_commit(session: &Session, repository: &zen_vcs::Repository, id: &str) -> Result<(), CommandError> {
    let Some(patterns) = for_caller(session) else { return Ok(()) };
    let files = match repository.diff_revisions(&format!("{id}^"), Some(id)) {
        Ok(files) => files,
        Err(_) => match repository.commit_files(id) {
            Ok(paths) => paths
                .into_iter()
                .map(|path| zen_vcs::ChangedFile { path, status: zen_vcs::FileStatus::Added, old_path: None })
                .collect(),
            Err(_) => return Ok(()),
        },
    };
    guard_changed(&patterns, repository.root(), files)
}

fn guard_changed(patterns: &[String], root: &Path, files: Vec<zen_vcs::ChangedFile>) -> Result<(), CommandError> {
    for file in files {
        for path in std::iter::once(file.path).chain(file.old_path) {
            if is_hidden(patterns, Some(root), Path::new(&path)) {
                return Err(refused(&path));
            }
        }
    }
    Ok(())
}

pub fn hidden_changes(session: &Session, repository: &zen_vcs::Repository) -> Vec<String> {
    let Some(patterns) = for_caller(session) else { return Vec::new() };
    let Ok(status) = repository.status() else { return Vec::new() };
    status
        .changes
        .into_iter()
        .flat_map(|change| std::iter::once(change.path).chain(change.old_path))
        .filter(|path| is_hidden(&patterns, Some(repository.root()), Path::new(path)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_too_large_to_walk_counts_as_holding_a_hidden_file() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("huge")).unwrap();
        for name in ["a", "b", "c"] {
            std::fs::write(root.join("huge").join(name), "").unwrap();
        }
        let patterns = vec![".env".to_owned()];
        assert_eq!(hidden_within(&patterns, Some(&root), Path::new("huge"), 2).as_deref(), Some("huge"));
        assert_eq!(hidden_within(&patterns, Some(&root), Path::new("huge"), 3), None);
    }

    #[test]
    fn an_agent_never_copies_a_file_to_where_a_pattern_hides_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("token.txt"), "x").unwrap();
        std::fs::create_dir_all(root.join("secrets")).unwrap();
        let mut registry = zen_core::Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.state_mut::<HiddenFromAgents>().set(&["secrets/".to_owned()]);
        let sources = [root.join("token.txt")];
        let directory = root.join("secrets");
        let refused = zen_core::macros::as_agent(&mut session, |session| {
            guard_copy(session, &root, &sources, &directory).unwrap_err()
        });
        assert_eq!(refused.code.as_deref(), Some(REDACTED_CODE));
        assert_eq!(refused.data.unwrap()["file"], directory.join("token.txt").to_string_lossy().as_ref());
        guard_copy(&session, &root, &sources, &directory).unwrap();
    }

    #[test]
    fn a_folder_holding_a_hidden_file_is_found_deep_inside() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("deploy/prod")).unwrap();
        std::fs::write(root.join("deploy/prod/.env"), "A=1").unwrap();
        std::fs::write(root.join("deploy/readme.md"), "").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let patterns = vec![".env".to_owned()];
        assert_eq!(hidden_inside(&patterns, Some(&root), Path::new("deploy")).as_deref(), Some("deploy/prod/.env"));
        assert_eq!(hidden_inside(&patterns, Some(&root), Path::new("src")), None);
        assert_eq!(hidden_inside(&[], Some(&root), Path::new("deploy")), None);
    }

    #[test]
    fn a_pattern_hides_and_a_negated_one_keeps() {
        let patterns: Vec<String> = [".env*", "!.env.example", "*.pem", "deploy/secrets"].map(str::to_owned).to_vec();
        assert!(hidden_by(&patterns, ".env") && hidden_by(&patterns, "app/.env.local"));
        assert!(!hidden_by(&patterns, ".env.example"));
        assert!(hidden_by(&patterns, "certs/server.pem") && hidden_by(&patterns, "deploy/secrets/token.txt"));
        assert!(!hidden_by(&patterns, "src/main.rs") && !hidden_by(&[], ".env"));
    }

    #[cfg(unix)]
    #[test]
    fn the_real_path_decides_whatever_the_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join(".env"), "x").unwrap();
        std::fs::write(root.join("config/.env.local"), "x").unwrap();
        std::os::unix::fs::symlink(root.join(".env"), root.join("innocent.txt")).unwrap();
        std::os::unix::fs::symlink(root.join("config"), root.join("linkdir")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(root.join(".env"), outside.path().join("plain.txt")).unwrap();
        let patterns: Vec<String> = [".env*", "!.env.example"].map(str::to_owned).to_vec();
        let hidden = |path: &str| is_hidden(&patterns, Some(&root), Path::new(path));
        for path in ["./sub/../.env", "sub/../.env", "innocent.txt", "linkdir/.env.local", "new/.env.prod"] {
            assert!(hidden(path), "{path}");
        }
        assert!(is_hidden(&patterns, Some(&root), &outside.path().join("plain.txt")), "a link from outside");
        if FOLD_CASE {
            for path in [".ENV", ".Env", "config/.ENV.LOCAL"] {
                assert!(hidden(path), "{path}");
            }
            assert!(!hidden(".ENV.EXAMPLE"), "a sample stays visible in any case");
        }
        assert!(!hidden("main.go") && !hidden(".env.example") && !hidden("sub"));
    }
}
