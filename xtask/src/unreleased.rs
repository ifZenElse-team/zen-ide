use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const FOLDER: &str = "changelog/unreleased";

pub(crate) const SECTIONS: [&str; 6] = ["Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Repository,
    Ides,
    Product(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Note {
    pub(crate) target: Target,
    pub(crate) section: &'static str,
    pub(crate) text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fragment {
    pub(crate) name: String,
    pub(crate) notes: Vec<Note>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Gathered {
    pub(crate) changelogs: Vec<PathBuf>,
    pub(crate) removed: Vec<String>,
    pub(crate) kept: Vec<String>,
}

fn target(heading: &str) -> Result<Target, String> {
    let heading = heading.trim();
    if heading == "repository" {
        return Ok(Target::Repository);
    }
    if heading == "ides" {
        return Ok(Target::Ides);
    }
    if heading == "toolbox" || crate::release::IDES.iter().any(|ide| ide.0 == heading) {
        return Ok(Target::Product(heading.to_owned()));
    }
    let ides: Vec<&str> = crate::release::IDES.iter().map(|ide| ide.0).collect();
    Err(format!(
        "`## {heading}`: notes go to `ides`, `repository`, `toolbox` or one IDE ({}); a plug-in's notes go in its own \
         changelog with its version",
        ides.join(", ")
    ))
}

pub(crate) fn parse(text: &str) -> Result<Vec<Note>, String> {
    let mut notes: Vec<Note> = Vec::new();
    let mut current: Option<(Target, Option<&'static str>)> = None;
    let mut body: Vec<&str> = Vec::new();
    let mut close = |current: &Option<(Target, Option<&'static str>)>, body: &mut Vec<&str>| -> Result<(), String> {
        let text = body.join("\n").trim().to_owned();
        body.clear();
        match current {
            Some((target, Some(section))) => {
                if text.is_empty() {
                    return Err(format!("`### {section}` has no notes"));
                }
                notes.push(Note { target: target.clone(), section, text });
            }
            Some((_, None)) | None if !text.is_empty() => {
                return Err(format!(
                    "`{}`: notes go under `## <target>` and `### <section>` ({})",
                    text.lines().next().unwrap_or_default(),
                    SECTIONS.join(", ")
                ));
            }
            _ => {}
        }
        Ok(())
    };
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            close(&current, &mut body)?;
            let Some((target, _)) = &current else {
                return Err(format!("`### {heading}` comes before a `## <target>`"));
            };
            let section = SECTIONS
                .iter()
                .find(|section| **section == heading.trim())
                .ok_or_else(|| format!("`### {heading}`: a section is one of {}", SECTIONS.join(", ")))?;
            current = Some((target.clone(), Some(section)));
        } else if let Some(heading) = line.strip_prefix("## ") {
            close(&current, &mut body)?;
            current = Some((target(heading)?, None));
        } else if line.starts_with('#') && line.trim_start_matches('#').starts_with(' ') {
            return Err(format!("`{line}`: only `## <target>` and `### <section>` headings are used"));
        } else {
            body.push(line);
        }
    }
    close(&current, &mut body)?;
    if notes.is_empty() {
        return Err("the file has no notes".to_owned());
    }
    Ok(notes)
}

fn render(notes: &[Note]) -> String {
    let mut text = String::new();
    let mut last: Option<(&Target, &str)> = None;
    for note in notes {
        if last.map(|(target, _)| target) != Some(&note.target) {
            let name = match &note.target {
                Target::Repository => "repository",
                Target::Ides => "ides",
                Target::Product(id) => id.as_str(),
            };
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&format!("## {name}\n\n"));
            last = None;
        }
        if last.map(|(_, section)| section) != Some(note.section) {
            text.push_str(&format!("### {}\n\n", note.section));
        }
        text.push_str(&note.text);
        text.push_str("\n\n");
        last = Some((&note.target, note.section));
    }
    text.trim_end().to_owned() + "\n"
}

fn valid_name(name: &str) -> bool {
    name.strip_suffix(".md").is_some_and(|stem| {
        !stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    })
}

pub(crate) fn read_all(root: &Path) -> Result<Vec<Fragment>, String> {
    let folder = root.join(FOLDER);
    if !folder.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .map_err(|error| format!("cannot read {}: {error}", folder.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .filter(|name| name != "README.md" && !name.starts_with('.'))
        .collect();
    names.sort();
    let mut fragments = Vec::new();
    let mut problems = Vec::new();
    for name in names {
        if !valid_name(&name) {
            problems.push(format!(
                "{FOLDER}/{name}: name a notes file after its card or branch, letters, digits, `-` and `_`, ending in .md"
            ));
            continue;
        }
        let path = folder.join(&name);
        let text =
            std::fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        match parse(&text) {
            Ok(notes) => fragments.push(Fragment { name, notes }),
            Err(error) => problems.push(format!("{FOLDER}/{name}: {error}")),
        }
    }
    if problems.is_empty() { Ok(fragments) } else { Err(problems.join("\n")) }
}

fn unreleased_section(text: &str) -> Option<Vec<&str>> {
    let mut lines = text.lines().skip_while(|line| line.trim() != "## [Unreleased]");
    lines.next()?;
    Some(lines.take_while(|line| !line.starts_with("## ")).collect())
}

pub(crate) fn check_in(root: &Path) -> Result<(), String> {
    read_all(root)?;
    let path = root.join("CHANGELOG.md");
    let text = std::fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let section = unreleased_section(&text).ok_or("CHANGELOG.md has no `## [Unreleased]` section")?;
    if section.iter().any(|line| line.starts_with("- ") || line.starts_with("### ")) {
        return Err(format!(
            "CHANGELOG.md: `## [Unreleased]` holds notes; put them in a file of {FOLDER}/ under `## repository` \
             ({FOLDER}/README.md)"
        ));
    }
    Ok(())
}

pub(crate) fn release_problem(root: &Path, version: &str, toolbox: Option<&str>) -> Option<String> {
    let fragments = match read_all(root) {
        Ok(fragments) => fragments,
        Err(error) => return Some(error),
    };
    let waiting: Vec<String> = fragments
        .iter()
        .filter(|fragment| {
            fragment.notes.iter().any(|note| toolbox.is_some() || note.target != Target::Product("toolbox".to_owned()))
        })
        .map(|fragment| format!("{FOLDER}/{}", fragment.name))
        .collect();
    let missing: Vec<String> = crate::release::IDES
        .iter()
        .map(|ide| root.join(crate::changelog::product_path(ide.0)))
        .filter(|path| crate::changelog::notes(path, version).is_err())
        .map(|path| path.display().to_string())
        .collect();
    let gather = format!(
        "run `cargo xtask changelog gather {version}{}` on a branch from main, merge it, and tag that commit (docs/releasing.md, step 2)",
        toolbox.map(|toolbox| format!(" --toolbox {toolbox}")).unwrap_or_default()
    );
    if !waiting.is_empty() {
        return Some(format!(
            "{version}: release notes still wait in {} and would not be in the release; {gather}",
            waiting.join(", ")
        ));
    }
    (!missing.is_empty())
        .then(|| format!("{version}: {} have no section `## {version} - <date>`; {gather}", missing.join(", ")))
}

fn body(notes: &[&Note]) -> String {
    let mut parts = Vec::new();
    for section in SECTIONS {
        let texts: Vec<&str> =
            notes.iter().filter(|note| note.section == section).map(|note| note.text.as_str()).collect();
        if !texts.is_empty() {
            parts.push(format!("### {section}\n\n{}", texts.join("\n")));
        }
    }
    parts.join("\n\n")
}

fn insert(text: &str, section: &str, after: impl Fn(&str) -> bool, starts: impl Fn(&str) -> bool) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.iter().position(|line| after(line)).map_or(0, |index| index + 1);
    let at = lines.iter().skip(from).position(|line| starts(line)).map_or(lines.len(), |index| index + from);
    let mut result: Vec<String> = lines[..at].iter().map(|line| (*line).to_owned()).collect();
    while result.last().is_some_and(|line| line.trim().is_empty()) {
        result.pop();
    }
    result.push(String::new());
    result.push(section.trim_end().to_owned());
    result.push(String::new());
    result.extend(lines[at..].iter().map(|line| (*line).to_owned()));
    while result.last().is_some_and(|line| line.trim().is_empty()) {
        result.pop();
    }
    result.join("\n") + "\n"
}

pub(crate) fn today() -> Result<String, String> {
    let output = Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .map_err(|error| format!("cannot run date ({error}); pass the release's day with --date <YYYY-MM-DD>"))?;
    let date = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || !crate::changelog::is_date(&date) {
        return Err(format!("`date` did not give today's date (`{date}`); pass it with --date <YYYY-MM-DD>"));
    }
    Ok(date)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Released {
    pub(crate) tags: BTreeSet<String>,
    pub(crate) sections: BTreeMap<String, BTreeSet<String>>,
}

impl Released {
    fn has(&self, file: &str, version: &str) -> bool {
        self.tags.contains(version) || self.sections.get(file).is_some_and(|versions| versions.contains(version))
    }
}

fn repository_versions(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("## ["))
        .filter_map(|rest| rest.split_once(']').map(|(version, _)| version.to_owned()))
        .filter(|version| version != "Unreleased")
        .collect()
}

pub(crate) fn released_in(root: &Path) -> Released {
    let git = |args: &[&str]| -> Option<String> {
        let output = Command::new("git").current_dir(root).args(args).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let mut released = Released::default();
    let tags = git(&["tag", "--list", "v*"]).unwrap_or_default();
    let mut versions: Vec<crate::calendar::Calendar> =
        tags.lines().filter_map(|tag| crate::calendar::parse(tag.trim().strip_prefix('v')?)).collect();
    versions.sort();
    released.tags = versions.iter().map(ToString::to_string).collect();
    let Some(newest) = versions.last() else { return released };
    let mut files: Vec<String> = crate::release::IDES.iter().map(|ide| format!("changelog/{}.md", ide.0)).collect();
    files.push("changelog/toolbox.md".to_owned());
    for file in files {
        if let Some(text) = git(&["show", &format!("v{newest}:{file}")]) {
            let versions = crate::changelog::parse(&text).unwrap_or_default().into_iter().map(|entry| entry.version);
            released.sections.insert(file, versions.collect());
        }
    }
    if let Some(text) = git(&["show", &format!("v{newest}:CHANGELOG.md")]) {
        released.sections.insert("CHANGELOG.md".to_owned(), repository_versions(&text));
    }
    released
}

fn extend(body: &str, notes: &[&Note]) -> String {
    let mut preamble: Vec<&str> = Vec::new();
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            blocks.push((heading.trim().to_owned(), Vec::new()));
        } else if let Some((_, lines)) = blocks.last_mut() {
            lines.push(line.to_owned());
        } else {
            preamble.push(line);
        }
    }
    let order = |heading: &str| SECTIONS.iter().position(|section| *section == heading);
    for section in SECTIONS {
        let texts: Vec<&str> =
            notes.iter().filter(|note| note.section == section).map(|note| note.text.as_str()).collect();
        if texts.is_empty() {
            continue;
        }
        let added: Vec<String> = texts.join("\n").lines().map(str::to_owned).collect();
        if let Some((_, lines)) = blocks.iter_mut().find(|(heading, _)| heading == section) {
            while lines.last().is_some_and(|line| line.trim().is_empty()) {
                lines.pop();
            }
            lines.extend(added);
        } else {
            let at = blocks
                .iter()
                .position(|(heading, _)| order(heading).is_some_and(|index| Some(index) > order(section)))
                .unwrap_or(blocks.len());
            let mut lines = vec![String::new()];
            lines.extend(added);
            blocks.insert(at, (section.to_owned(), lines));
        }
    }
    let mut parts: Vec<String> = Vec::new();
    let preamble = preamble.join("\n");
    if !preamble.trim().is_empty() {
        parts.push(preamble.trim().to_owned());
    }
    for (heading, lines) in blocks {
        parts.push(format!("### {heading}\n\n{}", lines.join("\n").trim()));
    }
    parts.join("\n\n")
}

fn replace_body(text: &str, heading: usize, body: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let end = lines
        .iter()
        .skip(heading + 1)
        .position(|line| line.starts_with("## "))
        .map_or(lines.len(), |at| at + heading + 1);
    let mut result: Vec<String> = lines[..=heading].iter().map(|line| (*line).to_owned()).collect();
    result.push(String::new());
    result.push(body.trim_end().to_owned());
    if end < lines.len() {
        result.push(String::new());
        result.extend(lines[end..].iter().map(|line| (*line).to_owned()));
    }
    result.join("\n") + "\n"
}

fn put(
    file: &str,
    text: &str,
    version: &str,
    heading: &str,
    notes: &[&Note],
    released: &Released,
    repository: bool,
) -> Result<Option<String>, String> {
    let is_section = |line: &str| {
        if repository {
            line.starts_with(&format!("## [{version}]"))
        } else {
            line.strip_prefix("## ").and_then(|rest| rest.split(" - ").next()).is_some_and(|it| it.trim() == version)
        }
    };
    let existing = text.lines().position(is_section);
    if existing.is_some() && released.has(file, version) {
        return Err(format!(
            "{file}: {version} is released already (its tag or the newest release tag has its section), and a \
             release's notes are never changed; gather into the next version"
        ));
    }
    if notes.is_empty() {
        return Ok(None);
    }
    let Some(at) = existing else {
        let section = format!("{heading}\n\n{}", body(notes));
        return Ok(Some(if repository {
            insert(text, &section, |line| line.trim() == "## [Unreleased]", |line| line.starts_with("## "))
        } else {
            insert(text, &section, |_| false, |line| line.starts_with("## "))
        }));
    };
    let first = text.lines().position(|line| line.starts_with("## ") && line.trim() != "## [Unreleased]");
    if first != Some(at) {
        return Err(format!("{file}: the section of {version} is not the newest one; gather into the newest version"));
    }
    let lines: Vec<&str> = text.lines().collect();
    let current: Vec<&str> = lines[at + 1..].iter().take_while(|line| !line.starts_with("## ")).copied().collect();
    Ok(Some(replace_body(text, at, &extend(&current.join("\n"), notes))))
}

pub(crate) fn gather_in(
    root: &Path,
    version: &str,
    date: &str,
    toolbox: Option<&str>,
    released: &Released,
) -> Result<Gathered, String> {
    if crate::calendar::parse(version).is_none() {
        return Err(format!("`{version}` is not a calendar version YY.M.N (ADR-0146)"));
    }
    if released.tags.contains(version) {
        return Err(format!(
            "{version} is released already (the tag v{version}): its notes are never changed; gather into the next \
             version"
        ));
    }
    if let Some(toolbox) = toolbox
        && crate::changelog::version_key(toolbox).is_none()
    {
        return Err(format!("`{toolbox}` is not a version for Zen Toolbox (major.minor.patch)"));
    }
    if !crate::changelog::is_date(date) {
        return Err(format!("`{date}` is not a date (YYYY-MM-DD)"));
    }
    let fragments = read_all(root)?;
    let notes: Vec<&Note> = fragments.iter().flat_map(|fragment| &fragment.notes).collect();
    let gathered = |note: &Note| match &note.target {
        Target::Product(id) if id == "toolbox" => toolbox.is_some(),
        _ => true,
    };
    let read = |file: &str| -> Result<String, String> {
        let path = root.join(file);
        std::fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))
    };
    let mut texts: Vec<(String, String)> = Vec::new();
    let ide_notes = notes.iter().any(|note| {
        matches!(&note.target, Target::Ides) || matches!(&note.target, Target::Product(id) if id != "toolbox")
    });
    let mut empty = Vec::new();
    for (id, _, _) in crate::release::IDES {
        let file = format!("changelog/{id}.md");
        let text = read(&file)?;
        let own: Vec<&Note> = notes
            .iter()
            .copied()
            .filter(|note| note.target == Target::Ides || note.target == Target::Product((*id).to_owned()))
            .collect();
        let has_section = crate::changelog::parse(&text)
            .map_err(|error| format!("{file}: {error}"))?
            .iter()
            .any(|entry| entry.version == version);
        if ide_notes && own.is_empty() && !has_section {
            empty.push(*id);
            continue;
        }
        if let Some(updated) = put(&file, &text, version, &format!("## {version} - {date}"), &own, released, false)? {
            crate::changelog::parse(&updated).map_err(|error| format!("{file}: {error}"))?;
            texts.push((file, updated));
        }
    }
    if !empty.is_empty() {
        return Err(format!(
            "{} would have no notes for {version}: every IDE is released together (ADR-0146); add notes under \
             `## ides` or `## <id>`",
            empty.join(", ")
        ));
    }
    if let Some(toolbox) = toolbox {
        let own: Vec<&Note> =
            notes.iter().copied().filter(|note| note.target == Target::Product("toolbox".to_owned())).collect();
        let file = "changelog/toolbox.md".to_owned();
        let text = read(&file)?;
        let has_section = crate::changelog::parse(&text)
            .map_err(|error| format!("{file}: {error}"))?
            .iter()
            .any(|entry| entry.version == toolbox);
        if own.is_empty() && !has_section {
            return Err(format!("there are no notes for Zen Toolbox ({FOLDER}/, `## toolbox`) to release {toolbox}"));
        }
        if let Some(updated) = put(&file, &text, toolbox, &format!("## {toolbox} - {date}"), &own, released, false)? {
            crate::changelog::parse(&updated).map_err(|error| format!("{file}: {error}"))?;
            texts.push((file, updated));
        }
    }
    let repository: Vec<&Note> = notes.iter().copied().filter(|note| note.target == Target::Repository).collect();
    let file = "CHANGELOG.md".to_owned();
    let text = read(&file)?;
    if unreleased_section(&text).is_none() {
        return Err("CHANGELOG.md has no `## [Unreleased]` section".to_owned());
    }
    if let Some(updated) = put(&file, &text, version, &format!("## [{version}] - {date}"), &repository, released, true)?
    {
        texts.push((file, updated));
    }
    if texts.is_empty() {
        return Err(format!("there are no notes in {FOLDER}/ to gather into {version}"));
    }
    let mut result = Gathered::default();
    for (file, text) in texts {
        let path = root.join(&file);
        std::fs::write(&path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        result.changelogs.push(PathBuf::from(file));
    }
    for fragment in &fragments {
        let path = root.join(FOLDER).join(&fragment.name);
        let left: Vec<Note> = fragment.notes.iter().filter(|note| !gathered(note)).cloned().collect();
        if left.is_empty() {
            std::fs::remove_file(&path).map_err(|error| format!("cannot remove {}: {error}", path.display()))?;
            result.removed.push(fragment.name.clone());
        } else if left.len() != fragment.notes.len() {
            std::fs::write(&path, render(&left))
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            result.kept.push(fragment.name.clone());
        } else {
            result.kept.push(fragment.name.clone());
        }
    }
    Ok(result)
}

pub(crate) fn gather(args: &[String]) -> Result<(), String> {
    let usage = "usage: cargo xtask changelog gather <YY.M.N> [--date <YYYY-MM-DD>] [--toolbox <version>]";
    let mut version = None;
    let mut date = None;
    let mut toolbox = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--date" => date = Some(args.next().ok_or(usage)?.clone()),
            "--toolbox" => toolbox = Some(args.next().ok_or(usage)?.clone()),
            _ if version.is_none() && !arg.starts_with("--") => version = Some(arg.clone()),
            _ => return Err(usage.to_owned()),
        }
    }
    let version = version.ok_or(usage)?;
    let date = match date {
        Some(date) => date,
        None => today()?,
    };
    let root = Path::new(".");
    let gathered = gather_in(root, &version, &date, toolbox.as_deref(), &released_in(root))?;
    for path in &gathered.changelogs {
        println!("Wrote the notes of {version} into {}.", path.display());
    }
    for name in &gathered.removed {
        println!("Removed {FOLDER}/{name}.");
    }
    for name in &gathered.kept {
        println!("Kept {FOLDER}/{name}: it holds notes for a later release.");
    }
    crate::changelog::check()
}

pub(crate) fn preview_in(root: &Path) -> Result<String, String> {
    let fragments = read_all(root)?;
    let notes: Vec<&Note> = fragments.iter().flat_map(|fragment| &fragment.notes).collect();
    if notes.is_empty() {
        return Ok("Nothing is waiting for the next release yet.\n".to_owned());
    }
    let mut groups: Vec<(String, Target)> = vec![("Every IDE".to_owned(), Target::Ides)];
    for (id, name, _) in crate::release::IDES {
        groups.push(((*name).to_owned(), Target::Product((*id).to_owned())));
    }
    groups.push(("Zen Toolbox".to_owned(), Target::Product("toolbox".to_owned())));
    groups.push(("Repository".to_owned(), Target::Repository));
    let mut text = format!(
        "{} from {} waiting for the next release.\n",
        counted(notes.len(), "note", "notes"),
        counted(fragments.len(), "pull request", "pull requests")
    );
    for (title, target) in groups {
        let own: Vec<&Note> = notes.iter().copied().filter(|note| note.target == target).collect();
        if own.is_empty() {
            continue;
        }
        let sections = body(&own).replace("### ", "#### ");
        text.push_str(&format!("\n### {title}\n\n{sections}\n"));
    }
    Ok(text)
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

pub(crate) fn preview(args: &[String]) -> Result<(), String> {
    let text = preview_in(Path::new("."))?;
    match args {
        [] => {
            print!("{text}");
            Ok(())
        }
        [flag, file] if flag == "--out" => {
            std::fs::write(file, &text).map_err(|error| format!("cannot write {file}: {error}"))?;
            println!("Wrote {file}.");
            Ok(())
        }
        _ => Err("usage: cargo xtask changelog preview [--out <file>]".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &str = "## ides\n\n### Fixed\n\n- **F4** opens the file.\n\n### Added\n\n- A thing.\n\n\
        ## repository\n\n### Changed\n\n- `cargo xtask` gathers notes.\n";

    #[test]
    fn notes_files_are_read_by_target_and_section() {
        let notes = parse(NOTES).unwrap();
        assert_eq!(notes.len(), 3);
        assert_eq!((&notes[0].target, notes[0].section), (&Target::Ides, "Fixed"));
        assert_eq!(notes[0].text, "- **F4** opens the file.");
        assert_eq!((&notes[2].target, notes[2].section), (&Target::Repository, "Changed"));
        assert_eq!(parse(&render(&notes)).unwrap(), notes, "written back as read");
        let one = parse("## gogarden\n\n### Fixed\n\n- Go only.\n").unwrap();
        assert_eq!(one[0].target, Target::Product("gogarden".to_owned()));
    }

    #[test]
    fn badly_formed_notes_files_are_refused() {
        for (text, expected) in [
            ("- A note with no heading.\n", "notes go under"),
            ("## ides\n\n- No section.\n", "notes go under"),
            ("## ides\n\n### Fixed\n\n", "has no notes"),
            ("## ides\n\n### Improved\n\n- A.\n", "a section is one of"),
            ("## plugin markdown\n\n### Fixed\n\n- A.\n", "a plug-in's notes"),
            ("### Fixed\n\n- A.\n", "before a `## <target>`"),
            ("## ides\n\n### Fixed\n\n#### More\n\n- A.\n", "only `## <target>`"),
            ("", "no notes"),
        ] {
            let error = parse(text).unwrap_err();
            assert!(error.contains(expected), "{text:?}: {error}");
        }
        assert!(valid_name("OOnyJnPX.md") && valid_name("fix-xtask_2.md"));
        assert!(!valid_name("notes.txt") && !valid_name("a b.md") && !valid_name(".md"));
    }

    fn repository(dir: &Path) {
        std::fs::create_dir_all(dir.join(FOLDER)).unwrap();
        for (id, name, _) in crate::release::IDES {
            let text = format!("# {name} changelog\n\nIntro.\n\n## 26.10.4 - 2026-10-27\n\n### Added\n\n- Old.\n");
            std::fs::write(dir.join(crate::changelog::product_path(id)), text).unwrap();
        }
        std::fs::write(
            dir.join(crate::changelog::product_path("toolbox")),
            "# Zen Toolbox changelog\n\n## 0.3.0 - 2026-10-20\n\nOld.\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("CHANGELOG.md"),
            "# Changelog\n\n## [Unreleased]\n\nIn changelog/unreleased/.\n\n## [26.10.4] - 2026-10-27\n\n### Fixed\n\n- Old.\n",
        )
        .unwrap();
    }

    #[test]
    fn gathering_moves_the_notes_into_the_release_and_keeps_what_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repository(root);
        let folder = root.join(FOLDER);
        std::fs::write(folder.join("OOnyJnPX.md"), NOTES).unwrap();
        std::fs::write(folder.join("b.md"), "## gogarden\n\n### Fixed\n\n- Go only.\n").unwrap();
        std::fs::write(
            folder.join("c.md"),
            "## toolbox\n\n### Fixed\n\n- Toolbox.\n\n## ides\n\n### Fixed\n\n- Both.\n",
        )
        .unwrap();
        check_in(root).unwrap();

        let released = out(&["26.10.4"]);
        let gathered = gather_in(root, "26.10.5", "2026-11-02", None, &released).unwrap();
        assert_eq!(gathered.removed, ["OOnyJnPX.md", "b.md"]);
        assert_eq!(gathered.kept, ["c.md"]);
        let gogarden = std::fs::read_to_string(root.join(crate::changelog::product_path("gogarden"))).unwrap();
        let entries = crate::changelog::parse(&gogarden).unwrap();
        assert_eq!(entries[0].version, "26.10.5");
        assert_eq!(entries[0].date, "2026-11-02");
        assert_eq!(
            entries[0].notes,
            "### Added\n\n- A thing.\n\n### Fixed\n\n- **F4** opens the file.\n- Go only.\n- Both."
        );
        assert_eq!(entries[1].notes, "### Added\n\n- Old.", "the released section stays as it was");
        assert!(gogarden.starts_with("# GoGarden changelog\n\nIntro.\n\n## 26.10.5 - 2026-11-02\n"));
        let rustronin = std::fs::read_to_string(root.join(crate::changelog::product_path("rustronin"))).unwrap();
        assert!(!rustronin.contains("Go only."), "a product's own notes stay in its file");
        let repository = std::fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
        assert!(
            repository.contains(
                "## [Unreleased]\n\nIn changelog/unreleased/.\n\n## [26.10.5] - 2026-11-02\n\n### Changed\n\n- `cargo \
                 xtask` gathers notes.\n\n## [26.10.4]"
            ),
            "{repository}"
        );
        assert_eq!(std::fs::read_to_string(folder.join("c.md")).unwrap(), "## toolbox\n\n### Fixed\n\n- Toolbox.\n");
        check_in(root).unwrap();

        std::fs::write(folder.join("d.md"), "## ides\n\n### Fixed\n\n- Later.\n\n### Removed\n\n- Gone.\n").unwrap();
        let error = gather_in(root, "26.10.5", "2026-11-02", Some("0.3.1"), &out(&["26.10.4", "26.10.5"])).unwrap_err();
        assert!(error.contains("26.10.5 is released already"), "{error}");
        assert!(
            folder.join("c.md").exists() && folder.join("d.md").exists(),
            "nothing is written when anything is wrong"
        );
        gather_in(root, "26.10.5", "2026-11-04", None, &released).unwrap();
        let gogarden = std::fs::read_to_string(root.join(crate::changelog::product_path("gogarden"))).unwrap();
        let entries = crate::changelog::parse(&gogarden).unwrap();
        assert_eq!((entries.len(), entries[0].date.as_str()), (2, "2026-11-02"));
        assert_eq!(
            entries[0].notes,
            "### Added\n\n- A thing.\n\n### Removed\n\n- Gone.\n\n### Fixed\n\n- **F4** opens the file.\n- Go only.\n- \
             Both.\n- Later."
        );
        assert!(!folder.join("d.md").exists() && folder.join("c.md").exists());
        let mut toolbox_out = out(&["26.10.4"]);
        toolbox_out.sections.insert("changelog/toolbox.md".to_owned(), ["0.3.0".to_owned()].into());
        let error = gather_in(root, "26.10.6", "2026-11-03", Some("0.3.0"), &toolbox_out).unwrap_err();
        assert!(error.contains("changelog/toolbox.md: 0.3.0 is released already"), "{error}");
        gather_in(root, "26.10.6", "2026-11-03", Some("0.3.1"), &released).unwrap();
        let toolbox = std::fs::read_to_string(root.join(crate::changelog::product_path("toolbox"))).unwrap();
        assert_eq!(crate::changelog::parse(&toolbox).unwrap()[0].notes, "### Fixed\n\n- Toolbox.");
        assert!(!folder.join("c.md").exists());
    }

    #[test]
    fn gathering_refuses_an_ide_without_notes_and_notes_in_unreleased() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repository(root);
        let folder = root.join(FOLDER);
        let released = out(&["26.10.4"]);
        assert!(gather_in(root, "26.10.5", "2026-11-02", None, &released).unwrap_err().contains("no notes"));
        std::fs::write(folder.join("a.md"), "## gogarden\n\n### Fixed\n\n- Go only.\n").unwrap();
        let error = gather_in(root, "26.10.5", "2026-11-02", None, &released).unwrap_err();
        assert!(error.contains("rustronin") && error.contains("every IDE is released together"), "{error}");
        assert!(gather_in(root, "0.39.0", "2026-11-02", None, &released).unwrap_err().contains("calendar version"));
        assert!(gather_in(root, "26.10.4", "2026-11-02", None, &released).unwrap_err().contains("released already"));
        assert!(gather_in(root, "26.10.5", "2.11.2026", None, &released).unwrap_err().contains("not a date"));

        let path = root.join("CHANGELOG.md");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("In changelog/unreleased/.", "### Fixed\n\n- Written here.")).unwrap();
        assert!(check_in(root).unwrap_err().contains("`## [Unreleased]` holds notes"));
    }

    fn out(versions: &[&str]) -> Released {
        Released { tags: versions.iter().map(|version| (*version).to_owned()).collect(), ..Released::default() }
    }

    #[test]
    fn gathering_adds_to_a_section_written_by_hand_before_the_tag() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repository(root);
        for (id, _, _) in crate::release::IDES {
            let path = root.join(crate::changelog::product_path(id));
            let text = std::fs::read_to_string(&path).unwrap();
            let text = text.replace(
                "## 26.10.4",
                "## 26.10.5 - 2026-11-01\n\nThe database wave.\n\n### Fixed\n\n- By hand.\n\n## 26.10.4",
            );
            std::fs::write(path, text).unwrap();
        }
        let folder = root.join(FOLDER);
        std::fs::write(folder.join("a.md"), "## gogarden\n\n### Added\n\n- Go only.\n\n### Fixed\n\n- Go fix.\n")
            .unwrap();
        let released = out(&["26.10.4"]);
        let error = gather_in(root, "26.10.4", "2026-11-02", None, &Released::default()).unwrap_err();
        assert!(error.contains("not the newest one"), "{error}");
        gather_in(root, "26.10.5", "2026-11-02", None, &released).unwrap();
        let gogarden = std::fs::read_to_string(root.join(crate::changelog::product_path("gogarden"))).unwrap();
        let entries = crate::changelog::parse(&gogarden).unwrap();
        assert_eq!((entries[0].version.as_str(), entries[0].date.as_str()), ("26.10.5", "2026-11-01"));
        assert_eq!(
            entries[0].notes,
            "The database wave.\n\n### Added\n\n- Go only.\n\n### Fixed\n\n- By hand.\n- Go fix."
        );
        assert_eq!(entries[1].notes, "### Added\n\n- Old.");
        let rustronin = std::fs::read_to_string(root.join(crate::changelog::product_path("rustronin"))).unwrap();
        assert!(rustronin.contains("### Fixed\n\n- By hand.\n\n## 26.10.4"), "{rustronin}");
        assert!(!folder.join("a.md").exists());
    }

    #[test]
    fn the_release_tags_say_what_is_out() {
        let released = released_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
        if !released.tags.contains("26.10.4") {
            return;
        }
        assert!(released.has("changelog/gogarden.md", "26.10.3"));
        assert!(released.has("CHANGELOG.md", "26.10.4"));
        assert!(released.sections["changelog/toolbox.md"].contains("0.3.15"));
        assert!(!released.has("changelog/toolbox.md", "99.0.0"));
    }

    #[test]
    fn a_tag_with_notes_not_gathered_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repository(root);
        let folder = root.join(FOLDER);
        let problem = release_problem(root, "26.10.5", None).unwrap();
        assert!(problem.contains("have no section `## 26.10.5") && problem.contains("gogarden.md"), "{problem}");
        assert_eq!(release_problem(root, "26.10.4", None), None, "a gathered release with nothing waiting");
        std::fs::write(folder.join("t.md"), "## toolbox\n\n### Fixed\n\n- Toolbox.\n").unwrap();
        assert_eq!(release_problem(root, "26.10.4", None), None, "Zen Toolbox's notes wait for its release");
        let problem = release_problem(root, "26.10.4", Some("0.3.1")).unwrap();
        assert!(
            problem.contains("changelog/unreleased/t.md") && problem.contains("gather 26.10.4 --toolbox 0.3.1`"),
            "{problem}"
        );
        std::fs::write(folder.join("r.md"), "## repository\n\n### Fixed\n\n- Late.\n").unwrap();
        let problem = release_problem(root, "26.10.4", None).unwrap();
        assert!(
            problem.contains("still wait in changelog/unreleased/r.md")
                && !problem.contains("t.md")
                && problem.contains("cargo xtask changelog gather 26.10.4"),
            "{problem}"
        );
    }

    #[test]
    fn the_repository_notes_are_in_order() {
        check_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..")).unwrap();
    }

    #[test]
    fn the_repository_notes_up_to_26_10_4_are_as_tagged() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let Ok(output) = Command::new("git").current_dir(&root).args(["show", "v26.10.4:CHANGELOG.md"]).output() else {
            return;
        };
        if !output.status.success() {
            return;
        }
        let tagged = String::from_utf8(output.stdout).unwrap();
        let tagged = unreleased_section(&tagged).unwrap().join("\n");
        let text = std::fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
        let section: Vec<&str> = text
            .lines()
            .skip_while(|line| *line != "## [26.10.4] and earlier - 2026-10-27")
            .skip(1)
            .take_while(|line| !line.starts_with("## "))
            .skip_while(|line| !line.starts_with("### "))
            .collect();
        assert!(!section.is_empty(), "CHANGELOG.md has no section `## [26.10.4] and earlier - 2026-10-27`");
        assert!(section.join("\n").trim() == tagged.trim(), "the section of 26.10.4 differs from the tag v26.10.4");
    }

    #[test]
    fn the_preview_lists_what_waits_by_product_and_section() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        assert_eq!(preview_in(root).unwrap(), "Nothing is waiting for the next release yet.\n");
        std::fs::create_dir_all(root.join(FOLDER)).unwrap();
        std::fs::write(root.join(FOLDER).join("README.md"), "# Notes\n").unwrap();
        std::fs::write(root.join(FOLDER).join("one.md"), NOTES).unwrap();
        std::fs::write(
            root.join(FOLDER).join("two.md"),
            "## gogarden\n\n### Fixed\n\n- Go only.\n\n## toolbox\n\n### Added\n\n- A setting.\n",
        )
        .unwrap();
        let text = preview_in(root).unwrap();
        assert!(text.starts_with("5 notes from 2 pull requests waiting for the next release.\n"), "{text}");
        let every = text.find("### Every IDE").unwrap();
        let go = text.find("### GoGarden").unwrap();
        let toolbox = text.find("### Zen Toolbox").unwrap();
        let repository = text.find("### Repository").unwrap();
        assert!(every < go && go < toolbox && toolbox < repository, "{text}");
        assert!(
            text[every..go].contains("#### Added\n\n- A thing.\n\n#### Fixed\n\n- **F4** opens the file."),
            "{text}"
        );
        assert!(text[go..toolbox].contains("#### Fixed\n\n- Go only."), "{text}");
        assert!(!text.contains("### RustRonin"), "a product with no notes has no heading: {text}");
        std::fs::write(root.join(FOLDER).join("bad.md"), "- No heading.\n").unwrap();
        assert!(preview_in(root).unwrap_err().contains("bad.md"));
    }
}
