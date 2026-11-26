use std::path::{Path, PathBuf};

use serde_json::json;
use zen_db_driver_kit::Failure;

pub(crate) const SQLCL_MISSING: &str = "sqlclMissing";
pub(crate) const JAVA_MISSING: &str = "javaMissing";

fn coded(code: &str, message: String) -> Failure {
    let mut failure = Failure::database("invalid", message);
    if let Some(data) = failure.data.as_mut() {
        data["code"] = json!(code);
    }
    failure
}

pub(crate) fn sqlcl_missing(detail: &str) -> Failure {
    coded(
        SQLCL_MISSING,
        format!(
            "SQLcl is missing: {detail}. Install SQLcl from oracle.com/sqlcl (or with Homebrew: brew install --cask sqlcl) and put its bin folder on PATH, set SQLCL_HOME, or set the connection's program to its sql program"
        ),
    )
}

pub(crate) fn java_missing(detail: &str) -> Failure {
    coded(
        JAVA_MISSING,
        format!(
            "Java is missing: {detail}. SQLcl runs on Java 17 or newer: install a JDK and set JAVA_HOME, or set the connection's javaHome to the JDK's folder"
        ),
    )
}

fn program_names() -> &'static [&'static str] {
    if cfg!(windows) { &["sql.exe", "sql.bat"] } else { &["sql"] }
}

fn in_folder(folder: &Path) -> Option<PathBuf> {
    program_names().iter().map(|name| folder.join(name)).find(|path| path.is_file())
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

pub(crate) fn find_sqlcl(set: Option<&str>) -> Result<PathBuf, Failure> {
    if let Some(set) = set.map(str::trim).filter(|set| !set.is_empty()) {
        let path = PathBuf::from(set);
        if !path.is_absolute() {
            return Err(sqlcl_missing(&format!("{set} is not an absolute path")));
        }
        if path.is_file() {
            return Ok(path);
        }
        return [path.clone(), path.join("bin")]
            .iter()
            .find_map(|folder| in_folder(folder))
            .ok_or_else(|| sqlcl_missing(&format!("{set} is not SQLcl's sql program")));
    }
    let homes = [
        env_path("SQLCL_HOME").map(|home| home.join("bin")),
        env_path("ORACLE_HOME").map(|home| home.join("sqlcl").join("bin")),
    ];
    let on_path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).filter(|folder| folder.is_absolute()).collect())
        .unwrap_or_default();
    let usual = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/homebrew/opt/sqlcl/bin", "/usr/local/opt/sqlcl/bin"]
        .map(PathBuf::from);
    homes
        .into_iter()
        .flatten()
        .chain(on_path)
        .chain(usual.into_iter().filter(|_| cfg!(unix)))
        .find_map(|folder| in_folder(&folder))
        .ok_or_else(|| sqlcl_missing("the sql program was not found"))
}

fn java_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

pub(crate) fn java_home(set: Option<&str>) -> Result<Option<PathBuf>, Failure> {
    let Some(set) = set.map(str::trim).filter(|set| !set.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(set);
    if !path.is_absolute() {
        return Err(java_missing(&format!("{set} is not an absolute path")));
    }
    let home = if path.is_file() { path.parent().and_then(Path::parent).map(Path::to_path_buf) } else { Some(path) };
    match home {
        Some(home) if home.join("bin").join(java_name()).is_file() => Ok(Some(home)),
        _ => Err(java_missing(&format!("{set} is not a Java home"))),
    }
}

pub(crate) fn java_found() -> bool {
    if env_path("JAVA_HOME").is_some_and(|home| home.join("bin").join(java_name()).is_file()) {
        return true;
    }
    let stand_in = Path::new("/usr/bin/java");
    let on_path = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|folder| folder.join(java_name()))
        .any(|java| java.is_file() && !(cfg!(target_os = "macos") && java == stand_in));
    if on_path {
        return true;
    }
    cfg!(target_os = "macos")
        && std::process::Command::new("/usr/libexec/java_home")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

pub(crate) fn ended(output: &[String], java_set: bool) -> Failure {
    let text = output.join("\n");
    let lower = text.to_ascii_lowercase();
    if lower.contains("unsupportedclassversionerror") || lower.contains("has been compiled by a more recent version") {
        return java_missing("the Java SQLcl found is too old");
    }
    let says_java = lower.contains("unable to locate a java runtime")
        || lower.contains("java: command not found")
        || lower.contains("java: not found")
        || (lower.contains("java") && (lower.contains("not found") || lower.contains("no such file")));
    if says_java || (!java_set && !java_found()) {
        return java_missing("SQLcl found no Java");
    }
    let detail = output.iter().rev().find(|line| !line.trim().is_empty()).map_or("SQLcl ended", String::as_str);
    Failure::database("connection", format!("SQLcl ended: {}", detail.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_set_for_the_connection_is_used_or_reported_missing() {
        let folder = std::env::temp_dir().join(format!("zen-oracle-program-{}", std::process::id()));
        let bin = folder.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let program = bin.join(program_names()[0]);
        std::fs::write(&program, "").unwrap();
        assert_eq!(find_sqlcl(Some(program.to_str().unwrap())).unwrap(), program);
        assert_eq!(find_sqlcl(Some(folder.to_str().unwrap())).unwrap(), program);
        let relative = find_sqlcl(Some("bin/sql")).unwrap_err();
        assert!(relative.message.contains("not an absolute path"), "{}", relative.message);
        assert!(java_home(Some("jdk")).unwrap_err().message.contains("not an absolute path"));
        let missing = find_sqlcl(Some("/nowhere/sql")).unwrap_err();
        assert_eq!(missing.data.as_ref().unwrap()["code"], SQLCL_MISSING);
        assert!(missing.message.starts_with("SQLcl is missing"));
        assert_eq!(java_home(Some("/nowhere")).unwrap_err().data.unwrap()["code"], JAVA_MISSING);
        assert_eq!(java_home(None).unwrap(), None);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_early_end_says_whether_java_is_missing() {
        let lines = |text: &[&str]| text.iter().map(|line| (*line).to_owned()).collect::<Vec<_>>();
        let java = ended(&lines(&["The operation couldn't be completed. Unable to locate a Java Runtime."]), true);
        assert_eq!(java.data.unwrap()["code"], JAVA_MISSING);
        let old = ended(&lines(&["Exception: java.lang.UnsupportedClassVersionError: oracle/dbtools/raptor"]), true);
        assert!(old.message.contains("too old"));
        let other = ended(&lines(&["", "something else went wrong", ""]), true);
        assert_eq!(other.kind(), Some("connection"));
        assert!(other.message.ends_with("something else went wrong"));
    }
}
