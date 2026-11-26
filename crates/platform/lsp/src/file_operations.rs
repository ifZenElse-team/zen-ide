use std::path::Path;

use serde_json::{Value, json};

use crate::position;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOperation {
    Create,
    Rename,
    Delete,
}

impl FileOperation {
    pub fn will_method(self) -> &'static str {
        match self {
            Self::Create => "workspace/willCreateFiles",
            Self::Rename => "workspace/willRenameFiles",
            Self::Delete => "workspace/willDeleteFiles",
        }
    }

    pub fn did_method(self) -> &'static str {
        match self {
            Self::Create => "workspace/didCreateFiles",
            Self::Rename => "workspace/didRenameFiles",
            Self::Delete => "workspace/didDeleteFiles",
        }
    }

    pub fn key(self, will: bool) -> &'static str {
        match (self, will) {
            (Self::Create, true) => "willCreate",
            (Self::Create, false) => "didCreate",
            (Self::Rename, true) => "willRename",
            (Self::Rename, false) => "didRename",
            (Self::Delete, true) => "willDelete",
            (Self::Delete, false) => "didDelete",
        }
    }

    pub fn params(self, files: &[(&Path, Option<&Path>)]) -> Value {
        let files: Vec<Value> = files
            .iter()
            .map(|(path, new_path)| match (self, new_path) {
                (Self::Rename, Some(new_path)) => {
                    json!({ "oldUri": position::file_uri(path), "newUri": position::file_uri(new_path) })
                }
                _ => json!({ "uri": position::file_uri(path) }),
            })
            .collect();
        json!({ "files": files })
    }
}

pub fn wanted(offered: &Value, key: &str, path: &Path, folder: bool) -> bool {
    let Some(filters) = offered["workspace"]["fileOperations"][key]["filters"].as_array() else { return false };
    filters.iter().any(|filter| filter_matches(filter, path, folder))
}

fn filter_matches(filter: &Value, path: &Path, folder: bool) -> bool {
    if filter["scheme"].as_str().is_some_and(|scheme| scheme != "file") {
        return false;
    }
    let pattern = &filter["pattern"];
    match pattern["matches"].as_str() {
        Some("file") if folder => return false,
        Some("folder") if !folder => return false,
        _ => {}
    }
    let Some(glob) = pattern["glob"].as_str() else { return false };
    let fold = pattern["options"]["ignoreCase"].as_bool() == Some(true);
    let text = path.to_string_lossy().replace('\\', "/");
    glob_matches(glob, &text, fold)
}

pub fn glob_matches(glob: &str, path: &str, fold_case: bool) -> bool {
    let fold = |text: &str| if fold_case { text.to_lowercase() } else { text.to_owned() };
    let path: Vec<char> = fold(path).chars().collect();
    expand(&fold(glob)).iter().any(|pattern| {
        let pattern: Vec<char> = pattern.chars().collect();
        matches(&pattern, &path)
    })
}

fn expand(glob: &str) -> Vec<String> {
    let chars: Vec<char> = glob.chars().collect();
    let Some(open) = chars.iter().position(|c| *c == '{') else { return vec![glob.to_owned()] };
    let mut depth = 0;
    let mut parts = Vec::new();
    let mut start = open + 1;
    let mut close = None;
    for (index, c) in chars.iter().enumerate().skip(open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    parts.push(chars[start..index].iter().collect::<String>());
                    close = Some(index);
                    break;
                }
            }
            ',' if depth == 1 => {
                parts.push(chars[start..index].iter().collect::<String>());
                start = index + 1;
            }
            _ => {}
        }
    }
    let Some(close) = close else { return vec![glob.to_owned()] };
    let head: String = chars[..open].iter().collect();
    let tail: String = chars[close + 1..].iter().collect();
    parts.iter().flat_map(|part| expand(&format!("{head}{part}{tail}"))).collect()
}

fn matches(pattern: &[char], path: &[char]) -> bool {
    match pattern.first() {
        None => path.is_empty(),
        Some('*') if pattern.get(1) == Some(&'*') => {
            let rest = &pattern[2..];
            if rest.first() == Some(&'/') && matches(&rest[1..], path) {
                return true;
            }
            (0..=path.len()).any(|skip| matches(rest, &path[skip..]))
        }
        Some('*') => {
            let rest = &pattern[1..];
            let segment = path.iter().position(|c| *c == '/').unwrap_or(path.len());
            (0..=segment).any(|skip| matches(rest, &path[skip..]))
        }
        Some('?') => path.first().is_some_and(|c| *c != '/') && matches(&pattern[1..], &path[1..]),
        Some('[') => match (class_end(pattern), path.first()) {
            (Some(end), Some(c)) if *c != '/' => {
                in_class(&pattern[1..end], *c) && matches(&pattern[end + 1..], &path[1..])
            }
            (Some(_), _) => false,
            (None, _) => path.first() == Some(&'[') && matches(&pattern[1..], &path[1..]),
        },
        Some(c) => path.first() == Some(c) && matches(&pattern[1..], &path[1..]),
    }
}

fn class_end(pattern: &[char]) -> Option<usize> {
    let first = if matches!(pattern.get(1), Some('!' | '^')) { 3 } else { 2 };
    pattern.iter().skip(first).position(|c| *c == ']').map(|at| at + first)
}

fn in_class(class: &[char], c: char) -> bool {
    let (negated, class) = match class.first() {
        Some('!' | '^') => (true, &class[1..]),
        _ => (false, class),
    };
    let mut found = false;
    let mut index = 0;
    while index < class.len() {
        if index + 2 < class.len() && class[index + 1] == '-' {
            found |= class[index] <= c && c <= class[index + 2];
            index += 3;
        } else {
            found |= class[index] == c;
            index += 1;
        }
    }
    found != negated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_whole_paths_with_stars_braces_and_classes() {
        assert!(glob_matches("**/*.rs", "/p/src/main.rs", false));
        assert!(glob_matches("**/*.rs", "main.rs", false));
        assert!(!glob_matches("**/*.rs", "/p/src/main.rsx", false));
        assert!(glob_matches("**/*.{ts,tsx,js}", "/p/a/b.tsx", false));
        assert!(!glob_matches("**/*.{ts,tsx,js}", "/p/a/b.go", false));
        assert!(glob_matches("/p/*.go", "/p/main.go", false));
        assert!(!glob_matches("/p/*.go", "/p/cmd/main.go", false));
        assert!(glob_matches("**/file?.[a-c]", "/x/file1.b", false));
        assert!(!glob_matches("**/file?.[!a-c]", "/x/file1.b", false));
        assert!(glob_matches("**/*.JAVA", "/x/A.java", true));
        assert!(!glob_matches("**/*.JAVA", "/x/A.java", false));
        assert!(glob_matches("**", "/x/folder", false));
        assert!(glob_matches("/p/**/test/**", "/p/a/b/test/c.go", false));
    }

    #[test]
    fn filters_check_the_scheme_and_whether_a_file_or_a_folder_is_meant() {
        let offered = json!({ "workspace": { "fileOperations": {
            "willRename": { "filters": [
                { "scheme": "file", "pattern": { "glob": "**/*.java", "matches": "file" } },
                { "pattern": { "glob": "**/src/**", "matches": "folder" } },
                { "scheme": "untitled", "pattern": { "glob": "**" } }
            ] }
        } } });
        let key = FileOperation::Rename.key(true);
        assert!(wanted(&offered, key, Path::new("/p/A.java"), false));
        assert!(!wanted(&offered, key, Path::new("/p/A.java"), true));
        assert!(wanted(&offered, key, Path::new("/p/src/app"), true));
        assert!(!wanted(&offered, key, Path::new("/p/README.md"), false));
        assert!(!wanted(&offered, FileOperation::Rename.key(false), Path::new("/p/A.java"), false));
    }

    #[test]
    fn parameters_name_old_and_new_uris_for_a_rename_and_one_uri_otherwise() {
        let renamed = FileOperation::Rename.params(&[(Path::new("/p/a.go"), Some(Path::new("/p/b.go")))]);
        assert_eq!(renamed, json!({ "files": [{ "oldUri": "file:///p/a.go", "newUri": "file:///p/b.go" }] }));
        let deleted = FileOperation::Delete.params(&[(Path::new("/p/a.go"), None)]);
        assert_eq!(deleted, json!({ "files": [{ "uri": "file:///p/a.go" }] }));
    }
}
