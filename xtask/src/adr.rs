use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const FOLDER: &str = "docs/adr";

pub(crate) fn check_in(root: &Path) -> Result<(), String> {
    let names = records(root)?;
    duplicates(&names)?;
    let mut problems = Vec::new();
    for name in &names {
        let Some(number) = number_of(name) else { continue };
        let text = std::fs::read_to_string(root.join(FOLDER).join(name)).map_err(|error| format!("{name}: {error}"))?;
        if !text.lines().next().is_some_and(|line| line.starts_with(&format!("# ADR-{number}: "))) {
            problems.push(format!("{FOLDER}/{name}: the first line is `# ADR-{number}: <title>`"));
        }
        if !text.lines().any(|line| line.starts_with("- **Status:** ")) {
            problems.push(format!("{FOLDER}/{name}: the record has no `- **Status:** ` line"));
        }
    }
    if let Some(main) = git_lines(root, &["ls-tree", "--name-only", "origin/main", &format!("{FOLDER}/")]) {
        let main: Vec<String> = main.iter().filter_map(|path| path.rsplit('/').next()).map(str::to_owned).collect();
        problems.extend(taken_on_main(&names, &main));
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) }
}

fn records(root: &Path) -> Result<Vec<String>, String> {
    let folder = root.join(FOLDER);
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .map_err(|error| format!("cannot read {}: {error}", folder.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect();
    names.sort();
    Ok(names)
}

fn number_of(name: &str) -> Option<&str> {
    let (number, _) = name.split_once('-')?;
    (number.len() == 4 && number.bytes().all(|byte| byte.is_ascii_digit()) && number != "0000").then_some(number)
}

fn duplicates(names: &[String]) -> Result<(), String> {
    let mut by_number: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for name in names {
        let Some((number, _)) = name.split_once('-') else { continue };
        if number.len() == 4 && number.bytes().all(|byte| byte.is_ascii_digit()) {
            by_number.entry(number).or_default().push(name);
        }
    }
    let clashes: Vec<String> = by_number
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(number, files)| {
            format!("ADR-{number} is used by {}: give the later one the next free number", files.join(" and "))
        })
        .collect();
    if clashes.is_empty() { Ok(()) } else { Err(clashes.join("\n")) }
}

fn taken_on_main(names: &[String], main: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| !main.contains(name))
        .filter_map(|name| {
            let number = number_of(name)?;
            let other = main.iter().find(|other| number_of(other) == Some(number))?;
            Some(format!(
                "ADR-{number}: {name} takes the number of {other} on main; `cargo xtask adr next` names a free one"
            ))
        })
        .collect()
}

fn git_lines(root: &Path, args: &[&str]) -> Option<Vec<String>> {
    let output = Command::new("git").current_dir(root).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect())
}

fn next_free(names: &[String], above: u32) -> (u32, Vec<String>) {
    let mut by_number: BTreeMap<u32, BTreeSet<&str>> = BTreeMap::new();
    for name in names {
        if let Some(number) = number_of(name).and_then(|number| number.parse::<u32>().ok()) {
            by_number.entry(number).or_default().insert(name);
        }
    }
    let next = by_number.keys().next_back().map_or(1, |last| last + 1);
    let clashes = by_number
        .iter()
        .filter(|(number, files)| **number > above && files.len() > 1)
        .map(|(number, files)| format!("ADR-{number:04}: {}", files.iter().copied().collect::<Vec<_>>().join(", ")))
        .collect();
    (next, clashes)
}

fn index(root: &Path) -> Result<String, String> {
    let mut table = String::from("| ADR | Title | Status |\n|---|---|---|\n");
    for name in records(root)? {
        let Some(number) = number_of(&name) else { continue };
        let text =
            std::fs::read_to_string(root.join(FOLDER).join(&name)).map_err(|error| format!("{name}: {error}"))?;
        let title = text.lines().next().and_then(|line| line.split_once(": ")).map_or("", |(_, title)| title.trim());
        let status =
            text.lines().find_map(|line| line.strip_prefix("- **Status:** ")).unwrap_or_default().replace('|', "\\|");
        table.push_str(&format!("| [{number}](adr/{name}) | {title} | {status} |\n"));
    }
    Ok(table)
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let root = Path::new(".");
    match args.first().map(String::as_str) {
        None | Some("check") => {
            check_in(root)?;
            println!("The decision records have numbers of their own.");
            Ok(())
        }
        Some("next") => {
            let _ = Command::new("git").args(["fetch", "--quiet", "origin"]).status();
            let mut names = records(root)?;
            let branches = git_lines(root, &["for-each-ref", "--format=%(refname)", "refs/remotes/origin"])
                .ok_or("cannot list the branches with git")?;
            for branch in branches {
                let files =
                    git_lines(root, &["ls-tree", "--name-only", &branch, &format!("{FOLDER}/")]).unwrap_or_default();
                names.extend(files.iter().filter_map(|path| path.rsplit('/').next()).map(str::to_owned));
            }
            names.sort();
            names.dedup();
            let main =
                git_lines(root, &["ls-tree", "--name-only", "origin/main", &format!("{FOLDER}/")]).unwrap_or_default();
            let above = main
                .iter()
                .filter_map(|path| number_of(path.rsplit('/').next()?)?.parse::<u32>().ok())
                .max()
                .unwrap_or(0);
            let (next, clashes) = next_free(&names, above);
            for clash in clashes {
                eprintln!("taken by two records in the branches: {clash}");
            }
            println!("{next:04}");
            Ok(())
        }
        Some("list") => {
            print!("{}", index(root)?);
            Ok(())
        }
        Some(_) => Err("usage: cargo xtask adr [check | next | list]".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn two_records_under_one_number_are_refused() {
        let error = duplicates(&names(&["0120-zoom.md", "0121-a.md", "0121-b.md", "README.md"])).unwrap_err();
        assert!(error.contains("ADR-0121 is used by 0121-a.md and 0121-b.md"), "{error}");
        assert!(duplicates(&names(&["0120-zoom.md", "0121-a.md", "README.md", "template.md"])).is_ok());
    }

    #[test]
    fn a_number_main_gives_another_record_is_refused() {
        let main = names(&["0187-git.md", "0188-other.md"]);
        assert_eq!(taken_on_main(&names(&["0187-git.md", "0189-mine.md"]), &main), Vec::<String>::new());
        let taken = taken_on_main(&names(&["0187-git.md", "0188-mine.md"]), &main);
        assert_eq!(taken.len(), 1);
        assert!(taken[0].contains("0188-mine.md takes the number of 0188-other.md on main"), "{taken:?}");
    }

    #[test]
    fn the_next_number_follows_every_branch_and_names_clashes() {
        let list =
            names(&["0000-template.md", "0079-a.md", "0079-b.md", "0184-a.md", "0184-b.md", "0187-c.md", "README.md"]);
        let (next, clashes) = next_free(&list, 179);
        assert_eq!(next, 188);
        assert_eq!(clashes, ["ADR-0184: 0184-a.md, 0184-b.md"], "only the numbers main has not taken yet");
        assert_eq!(next_free(&names(&["0000-template.md"]), 0).0, 1);
    }

    #[test]
    fn the_index_comes_from_the_records() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(FOLDER);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("0000-template.md"), "# ADR-NNNN: Title\n\n- **Status:** Proposed\n").unwrap();
        std::fs::write(
            folder.join("0007-commands.md"),
            "# ADR-0007: Everything is a command\n\n- **Status:** Accepted\n",
        )
        .unwrap();
        std::fs::write(folder.join("0008-wrong.md"), "# ADR-0009: Wrong number\n\nNo status.\n").unwrap();
        assert_eq!(
            index(dir.path()).unwrap(),
            "| ADR | Title | Status |\n|---|---|---|\n| [0007](adr/0007-commands.md) | Everything is a command | Accepted |\n\
             | [0008](adr/0008-wrong.md) | Wrong number |  |\n"
        );
        let error = check_in(dir.path()).unwrap_err();
        assert!(error.contains("0008-wrong.md: the first line is `# ADR-0008: <title>`"), "{error}");
        assert!(error.contains("0008-wrong.md: the record has no"), "{error}");
        assert!(!error.contains("0007"), "{error}");
    }

    #[test]
    fn the_repository_records_have_numbers_of_their_own() {
        check_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..")).unwrap();
    }
}
