use serde::Serialize;

pub fn row(path: &str, line: u32, column: u32, rest: &str) -> String {
    if rest.is_empty() { format!("{path}:{line}:{column}") } else { format!("{path}:{line}:{column} {rest}") }
}

pub fn name_of<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default()
}

pub fn path_matches(pattern: &str, relative: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    let name = relative.rsplit('/').next().unwrap_or(relative);
    let folder = pattern.trim_end_matches('/');
    zen_vfs::file_types::glob_matches(pattern, relative, false)
        || (!pattern.contains('/') && zen_vfs::file_types::glob_matches(pattern, name, false))
        || relative.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_the_place_and_what_follows() {
        assert_eq!(row("src/lib.rs", 12, 5, "struct Workbench"), "src/lib.rs:12:5 struct Workbench");
        assert_eq!(row("src/lib.rs", 1, 1, ""), "src/lib.rs:1:1");
        assert_eq!(name_of(&super::super::Severity::WeakWarning), "weakWarning");
    }

    #[test]
    fn a_pattern_matches_the_path_the_name_or_a_folder() {
        assert!(path_matches("crates/*/src/*.rs", "crates/platform/core/src/lib.rs"));
        assert!(path_matches("*.pem", "deploy/keys/server.pem"));
        assert!(path_matches(".env*", ".env.local"));
        assert!(path_matches("docs", "docs/guide/a.md") && path_matches("docs/", "docs/a.md"));
        assert!(!path_matches("docs", "docsite/a.md"));
        assert!(!path_matches("*.pem", "pem.txt") && !path_matches("", "a"));
    }
}
