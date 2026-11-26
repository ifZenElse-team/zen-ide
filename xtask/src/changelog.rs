use std::path::{Path, PathBuf};

const FOLDER: &str = "changelog";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Entry {
    pub(crate) version: String,
    pub(crate) date: String,
    pub(crate) notes: String,
}

pub(crate) fn product_path(id: &str) -> PathBuf {
    Path::new(FOLDER).join(format!("{id}.md"))
}

pub(crate) fn plugin_path(id: &str) -> PathBuf {
    Path::new(FOLDER).join("plugins").join(format!("{id}.md"))
}

pub(crate) fn version_key(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    let key = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(key)
}

pub(crate) fn is_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| if index == 4 || index == 7 { *byte == b'-' } else { byte.is_ascii_digit() })
}

pub(crate) fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut body = Vec::new();
    let close = |entries: &mut Vec<Entry>, body: &mut Vec<&str>| {
        if let Some(entry) = entries.last_mut() {
            entry.notes = body.join("\n").trim().to_owned();
        }
        body.clear();
    };
    for line in text.lines() {
        let Some(heading) = line.strip_prefix("## ") else {
            if !entries.is_empty() {
                body.push(line);
            }
            continue;
        };
        close(&mut entries, &mut body);
        let (version, date) = heading
            .trim()
            .split_once(" - ")
            .ok_or_else(|| format!("`## {heading}`: a section is headed `## <version> - <YYYY-MM-DD>`"))?;
        let key = version_key(version).ok_or_else(|| format!("`## {heading}`: `{version}` is not a version"))?;
        if crate::calendar::is_calendar(version) && crate::calendar::parse(version).is_none() {
            return Err(format!("`## {heading}`: `{version}` is not a calendar version YY.M.N (ADR-0146)"));
        }
        if !is_date(date) {
            return Err(format!("`## {heading}`: `{date}` is not a date (YYYY-MM-DD)"));
        }
        if let Some(previous) = entries.last()
            && version_key(&previous.version).is_some_and(|previous| previous <= key)
        {
            return Err(format!(
                "`## {heading}`: versions go newest first, and each once (after {})",
                previous.version
            ));
        }
        entries.push(Entry { version: version.to_owned(), date: date.to_owned(), notes: String::new() });
    }
    close(&mut entries, &mut body);
    if let Some(empty) = entries.iter().find(|entry| entry.notes.is_empty()) {
        return Err(format!("`## {}`: the section has no notes", empty.version));
    }
    Ok(entries)
}

pub(crate) fn all_notes(path: &Path) -> Result<Vec<(String, String)>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let entries = parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(entries.into_iter().map(|entry| (entry.version, entry.notes)).collect())
}

pub(crate) fn notes(path: &Path, version: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let entries = parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    entries.into_iter().find(|entry| entry.version == version).map(|entry| entry.notes).ok_or_else(|| {
        format!(
            "{} has no section `## {version} - <date>` with the release notes; `cargo xtask changelog gather` moves \
             the notes of changelog/unreleased/ into it (changelog/README.md)",
            path.display()
        )
    })
}

struct Changelog {
    kind: &'static str,
    id: String,
    name: String,
    entries: Vec<Entry>,
}

fn all(root: &Path) -> Result<Vec<Changelog>, String> {
    let mut result = Vec::new();
    let folder = root.join(FOLDER);
    for (kind, folder) in [("product", folder.clone()), ("plugin", folder.join("plugins"))] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
            .map_err(|error| format!("cannot read {}: {error}", folder.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
            .filter(|path| path.file_name().is_some_and(|name| name != "README.md"))
            .collect();
        files.sort();
        for path in files {
            let id = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let entries = parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
            let name = text
                .lines()
                .next()
                .and_then(|title| title.strip_prefix("# "))
                .map(|title| title.trim_end_matches(" changelog").to_owned())
                .unwrap_or_else(|| id.clone());
            result.push(Changelog { kind, id, name, entries });
        }
    }
    Ok(result)
}

pub(crate) fn check() -> Result<(), String> {
    check_in(Path::new("."))?;
    crate::unreleased::check_in(Path::new("."))?;
    crate::adr::check_in(Path::new("."))
}

fn calendar_problem(ides: &[(&str, Vec<String>)]) -> Option<String> {
    let released: Vec<&(&str, Vec<String>)> = ides.iter().filter(|(_, versions)| !versions.is_empty()).collect();
    let since =
        released.iter().filter_map(|(_, versions)| versions.last().and_then(|v| crate::calendar::parse(v))).max();
    let common = |versions: &[String]| -> Vec<String> {
        versions
            .iter()
            .filter(|version| since.is_none_or(|since| crate::calendar::parse(version).is_some_and(|it| it >= since)))
            .cloned()
            .collect()
    };
    let (first, rest) = released.split_first()?;
    rest.iter().find(|(_, versions)| common(versions) != common(&first.1)).map(|(other, _)| {
        format!(
            "changelog/{}.md and changelog/{other}.md list other calendar releases: every IDE is released together \
             under one version (ADR-0146), so each has the same `## <version> - <date>` sections",
            first.0
        )
    })
}

fn check_in(root: &Path) -> Result<(), String> {
    let logs = all(root)?;
    let calendar = |id: &str| -> Vec<String> {
        logs.iter()
            .filter(|log| log.id == id)
            .flat_map(|log| log.entries.iter())
            .filter(|entry| crate::calendar::is_calendar(&entry.version))
            .map(|entry| entry.version.clone())
            .collect()
    };
    let ides: Vec<(&str, Vec<String>)> = crate::release::IDES.iter().map(|ide| (ide.0, calendar(ide.0))).collect();
    if let Some(problem) = calendar_problem(&ides) {
        return Err(problem);
    }
    let mut missing = Vec::new();
    let mut plugins: Vec<PathBuf> = std::fs::read_dir(root.join("plugins"))
        .map_err(|error| format!("cannot read plugins: {error}"))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    plugins.sort();
    for folder in plugins {
        let id = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let manifest = std::fs::read_to_string(folder.join("Cargo.toml")).map_err(|error| error.to_string())?;
        let Some(version) = manifest.lines().find_map(|line| {
            line.trim().strip_prefix("version = \"").and_then(|rest| rest.strip_suffix('"')).map(str::to_owned)
        }) else {
            continue;
        };
        if let Err(error) = notes(&root.join(plugin_path(&id)), &version) {
            missing.push(error);
        }
    }
    if missing.is_empty() { Ok(()) } else { Err(missing.join("\n")) }
}

pub(crate) fn json() -> Result<Vec<u8>, String> {
    let changelogs: Vec<serde_json::Value> = all(Path::new("."))?
        .into_iter()
        .map(|log| serde_json::json!({ "kind": log.kind, "id": log.id, "name": log.name, "releases": log.entries }))
        .collect();
    serde_json::to_vec_pretty(&serde_json::json!({ "schema": 1, "changelogs": changelogs }))
        .map_err(|error| error.to_string())
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    match args {
        [command, rest @ ..] if command == "gather" => crate::unreleased::gather(rest),
        [command, rest @ ..] if command == "preview" => crate::unreleased::preview(rest),
        [] => {
            check()?;
            println!("The changelogs are in order.");
            Ok(())
        }
        [flag, file] if flag == "--json" => {
            check()?;
            std::fs::write(file, json()?).map_err(|error| format!("cannot write {file}: {error}"))?;
            println!("Wrote {file}.");
            Ok(())
        }
        _ => Err("usage: cargo xtask changelog [--json <file> | preview [--out <file>] | gather <YY.M.N> [--date <YYYY-MM-DD>] [--toolbox <version>]]"
            .to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# GoGarden changelog\n\nIntro.\n\n\
        ## 0.10.0 - 2026-11-02\n\n### Added\n\n- **Find** in files.\n\n\
        ## 0.9.1 - 2026-10-20\n\nOne line.\n";

    #[test]
    fn reads_sections_newest_first() {
        let entries = parse(LOG).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "0.10.0");
        assert_eq!(entries[0].date, "2026-11-02");
        assert_eq!(entries[0].notes, "### Added\n\n- **Find** in files.");
        assert_eq!(entries[1].notes, "One line.");
    }

    #[test]
    fn refuses_bad_changelogs() {
        let wrong_order = "## 0.1.0 - 2026-10-16\n\nA.\n\n## 0.2.0 - 2026-10-17\n\nB.\n";
        assert!(parse(wrong_order).unwrap_err().contains("newest first"));
        let twice = "## 0.1.0 - 2026-10-16\n\nA.\n\n## 0.1.0 - 2026-10-16\n\nB.\n";
        assert!(parse(twice).is_err());
        assert!(parse("## 0.1.0 - 2026-10-16\n\n## 0.0.9 - 2026-10-16\n\nB.\n").unwrap_err().contains("no notes"));
        assert!(parse("## GoGarden 0.1.0 - 2026-10-16\n\nA.\n").is_err());
        assert!(parse("## 0.1.0 - 16.10.2026\n\nA.\n").is_err());
        assert!(parse("## 0.1.0\n\nA.\n").is_err());
    }

    #[test]
    fn calendar_versions_are_checked() {
        let log = "## 26.10.1 - 2026-10-25\n\nNew.\n\n## 0.38.49 - 2026-10-25\n\nOld.\n";
        assert_eq!(parse(log).unwrap()[0].version, "26.10.1");
        assert!(parse("## 26.13.1 - 2026-10-25\n\nA.\n").unwrap_err().contains("calendar version"));
        assert!(parse("## 26.10.1 Preview - 2026-10-25\n\nA.\n").unwrap_err().contains("not a version"));
    }

    #[test]
    fn an_ide_joins_the_comparison_with_its_first_release() {
        let versions = |list: &[&str]| list.iter().map(|version| (*version).to_owned()).collect::<Vec<_>>();
        let before = [("gogarden", versions(&["26.11.7", "26.11.6"])), ("sqlscroll", Vec::new())];
        assert_eq!(calendar_problem(&before), None, "not released yet: nothing to compare");
        let after = [("gogarden", versions(&["26.11.8", "26.11.7", "26.11.6"])), ("sqlscroll", versions(&["26.11.8"]))];
        assert_eq!(calendar_problem(&after), None, "compared from its first release on");
        let skipped =
            [("gogarden", versions(&["26.11.8", "26.11.7"])), ("sqlscroll", versions(&["26.11.8", "26.11.6"]))];
        assert!(calendar_problem(&skipped).is_some(), "a release one IDE missed");
        let behind = [("gogarden", versions(&["26.11.8", "26.11.7"])), ("webwave", versions(&["26.11.7"]))];
        assert!(calendar_problem(&behind).unwrap().contains("webwave"), "a release one IDE lacks");
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(version_key("0.10.0") > version_key("0.9.1"));
        assert_eq!(version_key("0.1"), None);
        assert_eq!(version_key("0.1.0-beta"), None);
    }

    #[test]
    fn the_repository_changelogs_are_in_order() {
        check_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..")).unwrap();
    }
}
