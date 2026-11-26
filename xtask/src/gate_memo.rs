use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Skip,
    Run { record: Option<String> },
}

#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) tree: Option<String>,
    pub(crate) dirty: bool,
    pub(crate) toolchain: String,
    pub(crate) recorded: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Selection {
    skip: Vec<String>,
    only: Vec<String>,
}

impl Selection {
    pub(crate) fn is_partial(&self) -> bool {
        !self.skip.is_empty() || !self.only.is_empty()
    }

    pub(crate) fn takes(&self, name: &str) -> bool {
        (self.only.is_empty() || self.only.iter().any(|step| step == name))
            && !self.skip.iter().any(|step| step == name)
    }
}

fn steps_of(value: &str) -> Result<Vec<String>, String> {
    let mut steps = Vec::new();
    for name in value.split(',').map(str::trim).filter(|name| !name.is_empty()) {
        if !crate::ci::STEPS.contains(&name) {
            return Err(format!("unknown step `{name}`; the steps are {}", crate::ci::STEPS.join(", ")));
        }
        steps.push(name.to_owned());
    }
    if steps.is_empty() {
        return Err(format!("name a step: {}", crate::ci::STEPS.join(", ")));
    }
    Ok(steps)
}

pub(crate) fn parse(args: &[String]) -> Result<(bool, Selection), String> {
    let usage = "usage: cargo xtask ci [--force] [--skip <step>[,<step>...]] [--only <step>[,<step>...]]";
    let mut force = false;
    let mut selection = Selection::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        let mut value = || {
            inline.clone().or_else(|| args.next().cloned()).ok_or_else(|| format!("`{name}` needs step names; {usage}"))
        };
        match name {
            "--force" if inline.is_none() => force = true,
            "--skip" => selection.skip.extend(steps_of(&value()?)?),
            "--only" => selection.only.extend(steps_of(&value()?)?),
            other => return Err(format!("unknown option `{other}`; {usage}")),
        }
    }
    if !selection.only.is_empty() && selection.only.iter().all(|step| selection.skip.contains(step)) {
        return Err("`--skip` leaves out every step `--only` names".to_owned());
    }
    Ok((force, selection))
}

pub(crate) fn decide(state: &State, force: bool, partial: bool) -> Decision {
    let Some(tree) = state.tree.as_deref().filter(|_| !partial && !state.dirty && !state.toolchain.is_empty()) else {
        return Decision::Run { record: None };
    };
    if !force && state.recorded.as_deref().map(str::trim) == Some(state.toolchain.trim()) {
        return Decision::Skip;
    }
    Decision::Run { record: Some(tree.to_owned()) }
}

fn folder() -> Result<PathBuf, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    Ok(crate::bench::target_dir(&cargo)?.join("gate-passed"))
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn state(folder: &Path) -> State {
    let tree = output("git", &["rev-parse", "HEAD^{tree}"]).filter(|tree| !tree.is_empty());
    let dirty = output("git", &["status", "--porcelain"]).is_none_or(|changes| !changes.is_empty());
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let toolchain = output(&rustc, &["-V"]).unwrap_or_default();
    let recorded = tree.as_ref().and_then(|tree| std::fs::read_to_string(folder.join(tree)).ok());
    State { tree, dirty, toolchain, recorded }
}

pub(crate) fn run(force: bool) -> Result<(), String> {
    run_steps(force, &Selection::default())
}

fn run_steps(force: bool, selection: &Selection) -> Result<(), String> {
    let folder = folder()?;
    let before = state(&folder);
    let partial = selection.is_partial();
    if partial {
        let left_out: Vec<&str> = crate::ci::STEPS.iter().copied().filter(|step| !selection.takes(step)).collect();
        println!("==> Some steps only (left out: {}): the run is not remembered.", left_out.join(", "));
    }
    let record = match decide(&before, force, partial) {
        Decision::Skip => {
            let tree = before.tree.unwrap_or_default();
            println!(
                "==> The quality gate already passed on this tree ({}) with {}; nothing to run \
                 (`cargo xtask ci --force` runs it anyway).",
                &tree[..tree.len().min(12)],
                before.toolchain
            );
            return Ok(());
        }
        Decision::Run { record } => record,
    };
    if before.dirty {
        println!("==> The worktree has changes: the gate runs and its pass is not remembered.");
    }
    crate::ci::run_selected(&|step| selection.takes(step))?;
    if let Some(tree) = record {
        let after = state(&folder);
        if after.tree.as_deref() == Some(tree.as_str()) && !after.dirty {
            std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
            let file = folder.join(&tree);
            std::fs::write(&file, format!("{}\n", before.toolchain))
                .map_err(|error| format!("{}: {error}", file.display()))?;
            println!("==> Remembered: tree {} passed the gate.", &tree[..tree.len().min(12)]);
        } else {
            println!("==> The worktree changed while the gate ran: its pass is not remembered.");
        }
    }
    Ok(())
}

pub(crate) fn command(args: &[String]) -> Result<(), String> {
    let (force, selection) = parse(args)?;
    run_steps(force, &selection)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUSTC: &str = "rustc 1.99.1 (58cf8f9c 2026-10-13)";

    fn clean(recorded: Option<&str>) -> State {
        State {
            tree: Some("abc123".to_owned()),
            dirty: false,
            toolchain: RUSTC.to_owned(),
            recorded: recorded.map(str::to_owned),
        }
    }

    #[test]
    fn a_clean_tree_that_passed_with_this_toolchain_is_skipped() {
        assert_eq!(decide(&clean(Some(&format!("{RUSTC}\n"))), false, false), Decision::Skip);
    }

    #[test]
    fn force_runs_and_records_again() {
        assert_eq!(decide(&clean(Some(RUSTC)), true, false), Decision::Run { record: Some("abc123".to_owned()) });
    }

    #[test]
    fn a_clean_tree_never_passed_or_passed_with_another_toolchain_runs_and_records() {
        let record = Decision::Run { record: Some("abc123".to_owned()) };
        assert_eq!(decide(&clean(None), false, false), record);
        assert_eq!(decide(&clean(Some("rustc 1.98.0 (aaaa 2026-09-01)")), false, false), record);
    }

    #[test]
    fn a_dirty_tree_always_runs_and_records_nothing() {
        let state = State { dirty: true, ..clean(Some(RUSTC)) };
        assert_eq!(decide(&state, false, false), Decision::Run { record: None });
        let no_git = State { tree: None, ..clean(Some(RUSTC)) };
        assert_eq!(decide(&no_git, false, false), Decision::Run { record: None });
        let no_rustc = State { toolchain: String::new(), ..clean(Some("")) };
        assert_eq!(decide(&no_rustc, false, false), Decision::Run { record: None });
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn unknown_options_and_steps_are_errors_naming_the_valid_ones() {
        assert!(command(&args(&["--fast"])).unwrap_err().contains("--force"));
        let error = parse(&args(&["--skip", "tests"])).unwrap_err();
        assert!(error.contains("`tests`") && error.contains("format, clippy, test, layers"), "{error}");
        assert!(parse(&args(&["--only"])).unwrap_err().contains("needs step names"));
        assert!(parse(&args(&["--only", "test", "--skip", "test"])).is_err(), "nothing left to run");
    }

    #[test]
    fn steps_are_selected_by_skip_and_only() {
        let (force, stage_one) = parse(&args(&["--skip", "test"])).unwrap();
        assert!(!force && stage_one.is_partial());
        assert!(!stage_one.takes("test") && stage_one.takes("clippy") && stage_one.takes("wasm"));
        let (_, stage_two) = parse(&args(&["--only=test"])).unwrap();
        assert!(stage_two.takes("test") && !stage_two.takes("format"));
        let (_, two) = parse(&args(&["--only", "format,clippy", "--skip", "clippy"])).unwrap();
        assert_eq!(crate::ci::STEPS.iter().filter(|step| two.takes(step)).collect::<Vec<_>>(), [&"format"]);
        let (force, all) = parse(&args(&["--force"])).unwrap();
        assert!(force && !all.is_partial() && crate::ci::STEPS.iter().all(|step| all.takes(step)));
    }

    #[test]
    fn a_partial_run_is_neither_skipped_nor_remembered() {
        let (_, partial) = parse(&args(&["--skip", "test"])).unwrap();
        assert!(partial.is_partial());
        let passed = clean(Some(RUSTC));
        assert_eq!(decide(&passed, false, true), Decision::Run { record: None }, "not skipped, not recorded");
        assert_eq!(decide(&clean(None), false, true), Decision::Run { record: None });
        assert!(!Selection::default().is_partial(), "a full run is the remembered one");
    }
}
