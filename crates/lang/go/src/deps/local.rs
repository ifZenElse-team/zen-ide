use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gomod::GoMod;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalReport {
    pub errors: BTreeMap<String, String>,
    pub unused: BTreeSet<String>,
}

pub fn requirement_key(module: &str, version: &str) -> String {
    format!("{module} {version}")
}

pub fn missing_from_go_sum<'a>(go_mod: &'a GoMod, go_sum: &str) -> Vec<&'a crate::gomod::Requirement> {
    let mut files: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut mods: BTreeSet<(&str, &str)> = BTreeSet::new();
    for line in go_sum.lines() {
        let mut words = line.split_whitespace();
        let (Some(module), Some(version)) = (words.next(), words.next()) else { continue };
        match version.strip_suffix("/go.mod") {
            Some(version) => mods.insert((module, version)),
            None => files.insert((module, version)),
        };
    }
    go_mod
        .requires
        .iter()
        .filter(|requirement| {
            let replaced =
                go_mod.replaces.iter().any(|replace| replace.matches(&requirement.path, &requirement.version));
            let key = (requirement.path.as_str(), requirement.version.as_str());
            !replaced && (!mods.contains(&key) || (!requirement.indirect && !files.contains(&key)))
        })
        .collect()
}

pub fn list_errors(output: &[u8]) -> BTreeMap<String, String> {
    let mut errors = BTreeMap::new();
    for module in serde_json::Deserializer::from_slice(output).into_iter::<Value>().map_while(Result::ok) {
        let (Some(path), Some(error)) = (module["Path"].as_str(), module["Error"]["Err"].as_str()) else { continue };
        let version = module["Version"].as_str().unwrap_or_default();
        let prefix = format!("{path}@{version}: ");
        let reason = error.strip_prefix(&prefix).unwrap_or(error).trim();
        errors.insert(requirement_key(path, version), reason.to_owned());
    }
    errors
}

pub fn tidy_removes(diff: &str) -> BTreeSet<String> {
    let mut removed = BTreeSet::new();
    let mut added = BTreeSet::new();
    let mut in_go_mod = false;
    for line in diff.lines() {
        if line.starts_with("diff ") {
            in_go_mod = line.ends_with("/go.mod");
            continue;
        }
        if !in_go_mod || line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        let (set, rest) = match line.as_bytes().first() {
            Some(b'-') => (&mut removed, &line[1..]),
            Some(b'+') => (&mut added, &line[1..]),
            _ => continue,
        };
        let rest = rest.split("//").next().unwrap_or_default();
        let words: Vec<&str> = rest.split_whitespace().collect();
        let module = match words.as_slice() {
            ["require", module, version] if version.starts_with('v') => module,
            [module, version] if version.starts_with('v') => module,
            _ => continue,
        };
        set.insert((*module).to_owned());
    }
    removed.difference(&added).cloned().collect()
}

const GO_TIMEOUT: Duration = Duration::from_secs(300);

pub fn run(tool: &Path, env: &[(String, String)], dir: &Path, network: bool, tidy: bool) -> LocalReport {
    let command = |args: &[&str]| {
        let mut command = Command::new(tool);
        command.args(args).current_dir(dir).envs(env.iter().map(|(key, value)| (key, value)));
        command
            .env("GOTOOLCHAIN", "local")
            .env("GOFLAGS", crate::project_env::readonly_flags(env))
            .stdin(Stdio::null());
        if !network {
            command.env("GOPROXY", "off");
        }
        command
    };
    let mut report = LocalReport::default();
    if let Ok(output) = zen_run::capture(&mut command(&["list", "-m", "-json", "-e", "all"]), GO_TIMEOUT) {
        report.errors = list_errors(&output.stdout);
    }
    if tidy && let Ok(output) = zen_run::capture(command(&["mod", "tidy", "-diff"]).env("GOWORK", "off"), GO_TIMEOUT) {
        let diff = String::from_utf8_lossy(&output.stdout);
        if output.status.code() == Some(1) && diff.starts_with("diff ") {
            report.unused = tidy_removes(&diff);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gomod::parse_go_mod;

    #[test]
    fn a_requirement_needs_both_of_its_go_sum_lines_and_an_indirect_one_its_go_mod_hash() {
        let go_mod = parse_go_mod(
            Path::new("go.mod"),
            "module m\n\nrequire (\n\ta.example/files v1.0.0\n\tb.example/mod v1.0.0\n\tc.example/both v1.0.0\n\td.example/ind v1.0.0 // indirect\n\te.example/ind v1.0.0 // indirect\n)\n",
        );
        let go_sum = "a.example/files v1.0.0/go.mod h1:a=\nb.example/mod v1.0.0 h1:b=\nc.example/both v1.0.0 h1:c=\nc.example/both v1.0.0/go.mod h1:c=\nd.example/ind v1.0.0/go.mod h1:d=\ne.example/ind v1.0.0 h1:e=\n";
        let missing: Vec<&str> =
            missing_from_go_sum(&go_mod, go_sum).iter().map(|requirement| requirement.path.as_str()).collect();
        assert_eq!(missing, ["a.example/files", "b.example/mod", "e.example/ind"]);
    }

    #[test]
    fn requirements_without_a_go_sum_entry_are_found() {
        let go_mod = parse_go_mod(
            Path::new("go.mod"),
            "module m\n\nrequire (\n\ta.example/one v1.0.0\n\tb.example/two v1.1.0\n\tc.example/local v0.0.0\n)\n\nreplace c.example/local => ../local\n",
        );
        let go_sum =
            "a.example/one v1.0.0 h1:abc=\na.example/one v1.0.0/go.mod h1:def=\nb.example/two v1.0.0/go.mod h1:x=\n";
        let missing: Vec<&str> =
            missing_from_go_sum(&go_mod, go_sum).iter().map(|requirement| requirement.path.as_str()).collect();
        assert_eq!(missing, ["b.example/two"], "a replaced module needs no entry");
    }

    #[test]
    fn go_list_errors_are_read_by_requirement() {
        let output = br#"{
	"Path": "example.com/m",
	"Main": true
}
{
	"Path": "github.com/google/uuid",
	"Version": "v1.99.0",
	"Error": {
		"Err": "github.com/google/uuid@v1.99.0: invalid version: unknown revision v1.99.0"
	}
}
{
	"Path": "rsc.io/quote",
	"Version": "v1.5.2"
}
"#;
        let errors = list_errors(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors["github.com/google/uuid v1.99.0"], "invalid version: unknown revision v1.99.0");
    }

    #[test]
    fn tidy_diff_tells_the_unused_requirements_apart_from_moved_ones() {
        let diff = "diff current/go.mod tidy/go.mod\n--- current/go.mod\n+++ tidy/go.mod\n@@ -2,8 +2,9 @@\n \n go 1.21\n \n+require rsc.io/quote v1.5.2\n+\n require (\n-\tgithub.com/hhrutter/lzw v1.0.0 // indirect\n-\tgolang.org/x/crypto v0.1.0\n-\trsc.io/quote v1.5.2\n+\tgolang.org/x/text v0.4.0 // indirect\n )\n\ndiff current/go.sum tidy/go.sum\n--- current/go.sum\n+++ tidy/go.sum\n@@ -0,0 +1,1 @@\n-golang.org/x/old v1.0.0 h1:x=\n";
        let removed: Vec<String> = tidy_removes(diff).into_iter().collect();
        assert_eq!(removed, ["github.com/hhrutter/lzw", "golang.org/x/crypto"]);
    }

    #[cfg(unix)]
    #[test]
    fn the_checks_run_the_go_command() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("go");
        let script = "#!/bin/sh\necho \"$* $GOPROXY $GOTOOLCHAIN $GOWORK\" >> calls\n\
            if [ \"$1\" = list ]; then echo '{\"Path\":\"a.example/b\",\"Version\":\"v9.0.0\",\"Error\":{\"Err\":\"a.example/b@v9.0.0: unknown revision v9.0.0\"}}'; fi\n\
            if [ \"$1\" = mod ]; then printf 'diff current/go.mod tidy/go.mod\\n-\\ta.example/unused v1.0.0 // indirect\\n'; exit 1; fi\n";
        std::fs::write(&go, script).unwrap();
        std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
        let report = run(&go, &[], dir.path(), false, true);
        assert_eq!(report.errors["a.example/b v9.0.0"], "unknown revision v9.0.0");
        assert_eq!(report.unused.iter().collect::<Vec<_>>(), ["a.example/unused"]);
        let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
        assert_eq!(calls, "list -m -json -e all off local \nmod tidy -diff off local off\n");
        let report = run(&go, &[], dir.path(), true, false);
        assert!(report.unused.is_empty(), "no tidy: nothing unused told");
    }
}
