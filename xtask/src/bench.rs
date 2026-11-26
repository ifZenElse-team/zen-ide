use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SIZES: [usize; 3] = [1_000, 10_000, 100_000];
const MODES: [&str; 2] = ["type", "scroll"];
const RUN: Duration = Duration::from_secs(10);
const WARM_UP: Duration = Duration::from_secs(5);
const BUDGET_MS: f64 = 16.0;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Times {
    mean: f64,
    p50: f64,
    p95: f64,
    max: f64,
    lexed: Option<usize>,
}

pub(crate) fn run() -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    println!("==> Building GoGarden (release)");
    let status = Command::new(&cargo)
        .args(["build", "--release", "--package", "go-garden"])
        .status()
        .map_err(|error| format!("cannot run cargo: {error}"))?;
    if !status.success() {
        return Err("cargo build --release failed".to_owned());
    }
    let target = target_dir(&cargo)?;
    let binary = release_binary(&target, "gogarden");
    if !binary.is_file() {
        return Err(format!("the build left no {}", binary.display()));
    }
    let folder = target.join("bench-editor");
    let _ = std::fs::remove_dir_all(&folder);
    let mut rows = Vec::new();
    for size in SIZES {
        let project = folder.join(format!("lines-{size}"));
        std::fs::create_dir_all(&project).map_err(|error| error.to_string())?;
        let file = project.join("main.go");
        std::fs::write(&file, go_file(size)).map_err(|error| error.to_string())?;
        let mut row = vec![size.to_string()];
        let mut lexed = String::from("-");
        for mode in MODES {
            println!("==> {size} lines, {mode}");
            let times = measure(&binary, &project, &file, mode)?;
            row.push(format!("{:.1} / {:.1} / {:.1}", times.p50, times.p95, times.max));
            if mode == "type"
                && let Some(lines) = times.lexed
            {
                lexed = lines.to_string();
            }
            if mode == "type" && size == 10_000 && times.p95 > BUDGET_MS {
                println!("    over budget: p95 {:.1} ms > {BUDGET_MS} ms", times.p95);
            }
        }
        row.push(lexed);
        rows.push(row);
    }
    let _ = std::fs::remove_dir_all(&folder);
    println!("\nFrame times in ms, p50 / p95 / max ({}):\n", std::env::consts::ARCH);
    println!("| Lines | Typing | Scrolling | Lines highlighted again per keystroke |\n|---:|---|---|---:|");
    for row in rows {
        println!("| {} |", row.join(" | "));
    }
    Ok(())
}

fn measure(binary: &Path, project: &Path, file: &Path, mode: &str) -> Result<Times, String> {
    let data = project.with_extension("data");
    let product = data.join("space.ifzenelse.gogarden");
    std::fs::create_dir_all(&product).map_err(|error| error.to_string())?;
    let recent = serde_json::json!({ "projects": [{ "path": absolute(project)?, "opened": 1, "open": true }] });
    std::fs::write(product.join("recent-projects.json"), recent.to_string()).map_err(|error| error.to_string())?;
    let mut child = Command::new(binary)
        .arg(absolute(file)?)
        .env("ZEN_DATA_DIR", absolute(&data)?)
        .env("ZEN_UI_PROFILE", mode)
        .env("ZEN_PLUGINS_CATALOG", "none")
        .env("ZEN_RELEASES", "none")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", binary.display()))?;
    let started = Instant::now();
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = sender.send((Instant::now(), line));
        }
    });
    let mut reports = Vec::new();
    while started.elapsed() < RUN {
        if let Ok((at, line)) = receiver.recv_timeout(Duration::from_millis(200))
            && at.duration_since(started) >= WARM_UP
            && let Some(times) = parse(&line)
        {
            reports.push(times);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&data);
    if reports.is_empty() {
        return Err(format!("{mode}: the IDE reported no frame times"));
    }
    let worst = |pick: fn(&Times) -> f64| reports.iter().map(pick).fold(0.0, f64::max);
    Ok(Times {
        mean: worst(|t| t.mean),
        p50: worst(|t| t.p50),
        p95: worst(|t| t.p95),
        max: worst(|t| t.max),
        lexed: reports.iter().filter_map(|t| t.lexed).max(),
    })
}

pub(crate) fn target_dir(cargo: &str) -> Result<PathBuf, String> {
    let from_env = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    choose_target_dir(from_env, || {
        let output = Command::new(cargo)
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .stderr(Stdio::inherit())
            .output()
            .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
        if !output.status.success() {
            return Err("cargo metadata failed".to_owned());
        }
        metadata_target_dir(&output.stdout)
    })
}

fn choose_target_dir(
    from_env: Option<PathBuf>,
    metadata: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
    match from_env.filter(|dir| !dir.as_os_str().is_empty()) {
        Some(dir) => std::path::absolute(&dir).map_err(|error| error.to_string()),
        None => metadata(),
    }
}

fn metadata_target_dir(json: &[u8]) -> Result<PathBuf, String> {
    let metadata: serde_json::Value =
        serde_json::from_slice(json).map_err(|error| format!("cargo metadata: {error}"))?;
    metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata gave no target_directory".to_owned())
}

fn release_binary(target: &Path, name: &str) -> PathBuf {
    target.join("release").join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn absolute(path: &Path) -> Result<String, String> {
    std::path::absolute(path).map(|path| path.display().to_string()).map_err(|error| error.to_string())
}

fn parse(line: &str) -> Option<Times> {
    let rest = line.strip_prefix("profile ")?;
    let value = |name: &str| -> Option<f64> {
        let start = rest.find(&format!(" {name}="))? + name.len() + 2;
        rest[start..].split_whitespace().next()?.parse().ok()
    };
    let lexed = value("lexed").map(|lines| lines as usize);
    Some(Times { mean: value("mean")?, p50: value("p50")?, p95: value("p95")?, max: value("max")?, lexed })
}

fn go_file(lines: usize) -> String {
    let mut text = String::from("package main\n\nimport \"fmt\"\n\n");
    let mut n = 0;
    while text.lines().count() < lines.saturating_sub(1) {
        text.push_str(&format!(
            "// Sum{n} adds the positive numbers of a slice.\nfunc Sum{n}(values []int) int {{\n\ttotal := 0\n\tfor _, v := range values {{\n\t\tif v > 0 {{\n\t\t\ttotal += v // positive only\n\t\t}}\n\t}}\n\tfmt.Println(\"sum\", {n})\n\treturn total + {n}\n}}\n\n"
        ));
        n += 1;
    }
    text.push_str("func main() { fmt.Println(Sum0(nil)) }\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_report() {
        let times = parse("profile type: frames=240 mean=1.97 p50=2.02 p95=2.59 max=2.77 ms").unwrap();
        assert_eq!(times, Times { mean: 1.97, p50: 2.02, p95: 2.59, max: 2.77, lexed: None });
        let times = parse("profile type: frames=240 mean=1.97 p50=2.02 p95=2.59 max=2.77 ms lexed=2").unwrap();
        assert_eq!(times.lexed, Some(2));
        assert!(parse("frame: 3 ms").is_none());
    }

    #[test]
    fn the_binary_is_in_the_target_directory_cargo_used() {
        let unused = || -> Result<PathBuf, String> { panic!("CARGO_TARGET_DIR is set: no cargo metadata") };
        let elsewhere = choose_target_dir(Some(PathBuf::from("/builds/zen")), unused).unwrap();
        assert_eq!(elsewhere, Path::new("/builds/zen"));
        let relative = choose_target_dir(Some(PathBuf::from("out")), unused).unwrap();
        assert_eq!(relative, std::env::current_dir().unwrap().join("out"));
        let configured = || Ok(PathBuf::from("/work/zen/target"));
        assert_eq!(choose_target_dir(None, configured).unwrap(), Path::new("/work/zen/target"));
        assert_eq!(choose_target_dir(Some(PathBuf::new()), configured).unwrap(), Path::new("/work/zen/target"));
        let json = br#"{"packages":[],"target_directory":"/work/zen/target","workspace_root":"/work/zen"}"#;
        assert_eq!(metadata_target_dir(json).unwrap(), Path::new("/work/zen/target"));
        assert!(metadata_target_dir(b"{}").is_err());
        let binary = release_binary(Path::new("/builds/zen"), "gogarden");
        assert_eq!(binary, Path::new("/builds/zen/release").join(format!("gogarden{}", std::env::consts::EXE_SUFFIX)));
    }

    #[test]
    fn the_target_directory_is_the_one_cargo_built_this_test_in() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let target = target_dir(&cargo).unwrap().canonicalize().unwrap();
        let test = std::env::current_exe().unwrap().canonicalize().unwrap();
        assert!(test.starts_with(&target), "{} is not in {}", test.display(), target.display());
    }

    #[test]
    fn files_have_the_size_asked() {
        let lines = go_file(1_000).lines().count();
        assert!((1_000..1_020).contains(&lines), "{lines}");
    }
}
