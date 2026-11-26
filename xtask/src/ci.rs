use std::process::Command;
use std::time::{Duration, Instant};

use crate::layers;

const WASM_TARGET: &str = "wasm32-unknown-unknown";

enum Outcome {
    Passed,
    Skipped(String),
    Failed(String),
}

type Step = (&'static str, fn() -> Outcome);

type Ran = (&'static str, Outcome, Duration);

pub(crate) const STEPS: [&str; 10] =
    ["format", "clippy", "test", "layers", "comments", "legal", "i18n", "changelog", "deny", "wasm"];

pub(crate) fn run_selected(selected: &dyn Fn(&str) -> bool) -> Result<(), String> {
    let steps: [Step; 10] = [
        ("format", || cargo(&["fmt", "--all", "--check"], "run `cargo fmt --all` to fix")),
        ("clippy", || {
            cargo(
                &["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"],
                "fix the warnings above",
            )
        }),
        ("test", tests),
        ("layers", || match layers::run() {
            Ok(()) => Outcome::Passed,
            Err(error) => Outcome::Failed(error),
        }),
        ("comments", || match crate::comments::run() {
            Ok(()) => Outcome::Passed,
            Err(error) => Outcome::Failed(error),
        }),
        ("legal", || match crate::legal::gate(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")) {
            Ok(()) => Outcome::Passed,
            Err(error) => Outcome::Failed(error),
        }),
        ("i18n", || match crate::i18n::run() {
            Ok(()) => Outcome::Passed,
            Err(error) => Outcome::Failed(error),
        }),
        ("changelog", || match crate::changelog::check() {
            Ok(()) => Outcome::Passed,
            Err(error) => Outcome::Failed(error),
        }),
        ("deny", deny),
        ("wasm", wasm),
    ];

    report(&run_steps(&steps, selected))
}

fn run_steps(steps: &[Step], selected: &dyn Fn(&str) -> bool) -> Vec<Ran> {
    let mut results = Vec::new();
    for (name, step) in steps.iter().copied() {
        if !selected(name) {
            results.push((name, Outcome::Skipped("left out by --skip/--only".to_owned()), Default::default()));
            continue;
        }
        println!("\n==> {name}");
        let started = Instant::now();
        let outcome = step();
        results.push((name, outcome, started.elapsed()));
    }
    results
}

fn report(results: &[Ran]) -> Result<(), String> {
    println!("\nSummary:");
    let mut failed = 0;
    for (name, outcome, elapsed) in results {
        let seconds = elapsed.as_secs_f32();
        match outcome {
            Outcome::Passed => println!("  ok      {name:<8} {seconds:>6.1}s"),
            Outcome::Skipped(reason) => println!("  skipped {name:<8} {seconds:>6.1}s  {reason}"),
            Outcome::Failed(reason) => {
                failed += 1;
                println!("  FAILED  {name:<8} {seconds:>6.1}s  {reason}");
            }
        }
    }
    if failed == 0 { Ok(()) } else { Err(format!("ci: {failed} step(s) failed")) }
}

fn cargo(args: &[&str], hint: &str) -> Outcome {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    match Command::new(cargo).args(args).status() {
        Ok(status) if status.success() => Outcome::Passed,
        Ok(_) => Outcome::Failed(hint.to_owned()),
        Err(error) => Outcome::Failed(format!("cannot run cargo: {error}")),
    }
}

fn tests() -> Outcome {
    use std::io::{BufRead, BufReader};
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let child = Command::new(cargo)
        .args(["test", "--workspace", "--locked"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return Outcome::Failed(format!("cannot run cargo: {error}")),
    };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Outcome::Failed("cannot read cargo's output".to_owned());
    };
    let started = std::thread::spawn(move || {
        let mut started = 0usize;
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("{line}");
            started += usize::from(binary_started(&line));
        }
        started
    });
    let mut reported = 0usize;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        println!("{line}");
        reported += usize::from(line.starts_with("test result:"));
    }
    let started = started.join().unwrap_or(0);
    match child.wait() {
        Ok(status) if !status.success() => Outcome::Failed("fix the failing tests above".to_owned()),
        Err(error) => Outcome::Failed(format!("cannot run cargo: {error}")),
        Ok(_) => unreported(started, reported).map_or(Outcome::Passed, Outcome::Failed),
    }
}

fn unreported(started: usize, reported: usize) -> Option<String> {
    (reported < started).then(|| {
        format!(
            "{} of {started} test binaries ended without reporting their result; one of them exits early",
            started - reported
        )
    })
}

fn binary_started(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("Running ") || line.starts_with("Doc-tests ")
}

fn deny() -> Outcome {
    let installed = Command::new("cargo-deny").arg("--version").output().is_ok_and(|output| output.status.success());
    if !installed {
        return Outcome::Failed("cargo-deny is not installed; run `cargo install --locked cargo-deny`".to_owned());
    }
    cargo(&["deny", "--workspace", "check"], "see deny.toml and docs/adr/0002-open-source-clean-room-pure-rust.md")
}

fn wasm() -> Outcome {
    let crates = match layers::portable_crates() {
        Ok(crates) => crates,
        Err(error) => return Outcome::Failed(error),
    };
    if crates.is_empty() {
        return Outcome::Skipped("no crates below L6 yet".to_owned());
    }
    let mut args = vec![
        "build",
        "--target",
        WASM_TARGET,
        "--locked",
        "--config",
        "target.wasm32-unknown-unknown.rustflags=[\"-Dwarnings\"]",
    ];
    for name in &crates {
        args.extend(["--package", name.as_str()]);
    }
    cargo(
        &args,
        "a crate below L6 does not compile for WebAssembly without warnings (see above); install the target with \
         `rustup target add wasm32-unknown-unknown` if it is missing",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{Outcome, Step, report, run_steps, unreported};

    static LATER_STEP_RAN: AtomicBool = AtomicBool::new(false);

    #[test]
    fn a_failed_step_stops_no_later_step_and_fails_the_run() {
        let steps: [Step; 2] = [
            ("first", || Outcome::Failed("no".to_owned())),
            ("later", || {
                LATER_STEP_RAN.store(true, Ordering::SeqCst);
                Outcome::Passed
            }),
        ];
        let results = run_steps(&steps, &|_| true);
        assert!(LATER_STEP_RAN.load(Ordering::SeqCst));
        assert_eq!(report(&results).unwrap_err(), "ci: 1 step(s) failed");
    }

    #[test]
    fn a_test_binary_that_ends_without_its_result_fails_the_gate() {
        assert!(unreported(3, 2).unwrap().starts_with("1 of 3 test binaries"));
        assert_eq!(unreported(3, 3), None);
    }

    #[test]
    fn test_binaries_are_counted_from_cargos_lines() {
        assert!(super::binary_started("     Running unittests src/lib.rs (target/debug/deps/zen_ui-1)"));
        assert!(super::binary_started("   Doc-tests zen_vcs"));
        assert!(!super::binary_started("   Compiling zen-ui v0.0.0"));
        assert!(!super::binary_started("running 12 tests"));
    }
}
