use std::path::Path;

use zen_lang_go::gomod::{Exclude, Godebug, PathDirective, Replace, Requirement, Retract, parse_go_mod, parse_go_work};

const FULL: &str = include_str!("fixtures/full/go.mod");
const ERRORS: &str = include_str!("fixtures/errors/go.mod");
const WORK: &str = include_str!("fixtures/work/go.work");

fn require(path: &str, version: &str, indirect: bool, line: u32) -> Requirement {
    Requirement { path: path.to_owned(), version: version.to_owned(), indirect, line }
}

fn path(path: &str, line: u32) -> PathDirective {
    PathDirective { path: path.to_owned(), line }
}

#[test]
fn every_go_mod_directive_is_read() {
    let parsed = parse_go_mod(Path::new("go.mod"), FULL);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let module = parsed.module.unwrap();
    assert_eq!(module.path, "example.com/garden");
    assert_eq!(module.line, 3);
    assert_eq!(module.deprecated.as_deref(), Some("use example.com/garden/v2 instead.\nIt will not get fixes."));
    assert_eq!(parsed.go.as_deref(), Some("1.23.0"));
    assert_eq!(parsed.toolchain.as_deref(), Some("go1.23.4"));
    assert_eq!(
        parsed.requires,
        [
            require("example.com/single", "v1.0.0", false, 9),
            require("example.com/alpha", "v1.2.3", false, 12),
            require("example.com/beta", "v0.4.0", true, 13),
            require("example.com/quoted", "v2.0.0+incompatible", false, 14),
            require("example.com/raw", "v0.0.0-20240101000000-abcdefabcdef", true, 15),
        ]
    );
    assert_eq!(
        parsed.replaces,
        [
            Replace {
                path: "example.com/alpha".to_owned(),
                version: None,
                new_path: "../alpha".to_owned(),
                new_version: None,
                line: 18,
            },
            Replace {
                path: "example.com/beta".to_owned(),
                version: Some("v0.4.0".to_owned()),
                new_path: "example.com/beta-fork".to_owned(),
                new_version: Some("v0.4.1".to_owned()),
                line: 21,
            },
            Replace {
                path: "example.com/gamma".to_owned(),
                version: None,
                new_path: "./third party/gamma".to_owned(),
                new_version: None,
                line: 22,
            },
        ]
    );
    assert!(parsed.replaces[0].is_local() && !parsed.replaces[1].is_local());
    assert!(parsed.replaces[1].matches("example.com/beta", "v0.4.0"));
    assert!(!parsed.replaces[1].matches("example.com/beta", "v0.5.0"));
    assert!(parsed.replaces[0].matches("example.com/alpha", "v9.9.9"));
    assert_eq!(
        parsed.excludes,
        [
            Exclude { path: "example.com/alpha".to_owned(), version: "v1.2.0".to_owned(), line: 25 },
            Exclude { path: "example.com/beta".to_owned(), version: "v0.3.9".to_owned(), line: 28 },
        ]
    );
    assert_eq!(
        parsed.retracts,
        [
            Retract {
                low: "v0.1.0".to_owned(),
                high: "v0.1.0".to_owned(),
                rationale: Some("The first release had a broken API.".to_owned()),
                line: 32,
            },
            Retract {
                low: "v0.2.0".to_owned(),
                high: "v0.2.5".to_owned(),
                rationale: Some("Published too early.".to_owned()),
                line: 35,
            },
            Retract { low: "v0.3.0".to_owned(), high: "v0.3.0".to_owned(), rationale: None, line: 36 },
        ]
    );
    let godebug = |key: &str, value: &str, line| Godebug { key: key.to_owned(), value: value.to_owned(), line };
    assert_eq!(
        parsed.godebugs,
        [godebug("default", "go1.21", 39), godebug("panicnil", "1", 42), godebug("asynctimerchan", "0", 43)]
    );
    assert_eq!(parsed.tools, [path("example.com/tools/cmd/gen", 46), path("example.com/tools/cmd/lint", 49)]);
    assert_eq!(parsed.ignores, [path("./node_modules", 52), path("./build", 55), path("./my docs", 56)]);
}

#[test]
fn problems_have_their_lines() {
    let parsed = parse_go_mod(Path::new("/p/go.mod"), ERRORS);
    let problems: Vec<(u32, &str)> =
        parsed.problems.iter().map(|problem| (problem.line, problem.message.as_str())).collect();
    let expected: &[(u32, &str)] = &[
        (3, "invalid go version"),
        (4, "unknown directive: frobnicate"),
        (5, "usage: require"),
        (8, "invalid version 1.0.0"),
        (10, "replacement module without version must be directory path"),
        (11, "usage: replace"),
        (12, "usage: retract"),
        (13, "usage: godebug"),
        (14, "repeated module statement"),
        (15, "unterminated string"),
        (15, "usage: require"),
        (16, "unterminated require block"),
    ];
    assert_eq!(problems.len(), expected.len(), "{problems:?}");
    for ((line, message), (expected_line, expected_message)) in problems.iter().zip(expected) {
        assert_eq!(line, expected_line, "{message}");
        assert!(message.starts_with(expected_message), "{message:?} on line {line}");
    }
    assert!(parsed.problems.iter().all(|problem| problem.file == Path::new("/p/go.mod")));
    assert_eq!(parsed.module.unwrap().path, "example.com/broken");
    assert_eq!(parsed.go, None);
    let paths: Vec<&str> = parsed.requires.iter().map(|require| require.path.as_str()).collect();
    assert_eq!(paths, ["example.com/ok", "example.com/bad", "example.com/last"]);
}

#[test]
fn go_work_is_read() {
    let parsed = parse_go_work(Path::new("go.work"), WORK);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    assert_eq!(parsed.go.as_deref(), Some("1.23"));
    assert_eq!(parsed.toolchain.as_deref(), Some("go1.23.4"));
    assert_eq!(parsed.uses, [path("./app", 5), path("./lib", 8), path("./missing", 9)]);
    assert_eq!(parsed.replaces.len(), 1);
    assert_eq!(parsed.replaces[0].new_path, "./forks/shared");
    assert_eq!(parsed.replaces[0].version.as_deref(), Some("v1.0.0"));
    assert_eq!(parsed.godebugs[0].key, "default");
}

#[test]
fn go_work_rejects_go_mod_directives() {
    let parsed = parse_go_work(Path::new("go.work"), "go 1.23\nrequire example.com/x v1.0.0\nmodule m\n");
    let lines: Vec<u32> = parsed.problems.iter().map(|problem| problem.line).collect();
    assert_eq!(lines, [2, 3]);
}

#[test]
fn small_files_and_edge_cases() {
    let parsed = parse_go_mod(Path::new("go.mod"), "");
    assert_eq!(parsed.problems.len(), 1, "no module directive");
    let parsed = parse_go_mod(Path::new("go.mod"), "module m\nrequire ()\ntoolchain default\ngo 1.22rc1\n");
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    assert_eq!(parsed.toolchain.as_deref(), Some("default"));
    assert_eq!(parsed.go.as_deref(), Some("1.22rc1"));
    let parsed = parse_go_mod(Path::new("go.mod"), "module m\n\n// Deprecated: far away.\n\nrequire x v1.0.0\n");
    assert_eq!(parsed.module.unwrap().deprecated, None);
    let parsed = parse_go_mod(Path::new("go.mod"), "module m // Deprecated: inline.\n");
    assert_eq!(parsed.module.unwrap().deprecated.as_deref(), Some("inline."));
}

#[test]
fn never_panics() {
    let samples = [
        "(",
        ")",
        "=>",
        "\"",
        "`",
        "module",
        "module (",
        "require (\n)",
        "replace a => ",
        "retract [",
        "retract [v1,",
        "godebug =",
        "use",
        "é ü\r\n",
        "module 例え.com // コメント",
        "require x v1 // indirect\n)",
        "\u{0}",
    ];
    for text in samples {
        let _ = parse_go_mod(Path::new("go.mod"), text);
        let _ = parse_go_work(Path::new("go.work"), text);
    }
}
