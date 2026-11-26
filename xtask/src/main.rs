#![allow(clippy::print_stdout, clippy::print_stderr, reason = "a command-line tool reports to the terminal")]

use std::process::ExitCode;

mod adr;
mod bench;
mod calendar;
mod changelog;
mod ci;
mod ci_host;
mod comments;
mod cross;
mod deb;
mod dev;
mod gate_memo;
mod i18n;
mod icon;
mod layers;
mod legal;
mod linux;
mod mascot;
mod native;
mod notices;
mod release;
mod release_ci;
mod release_flow;
mod site;
mod spelling;
mod unreleased;
mod windows;

const TASKS: &[(&str, &str)] = &[
    (
        "ci",
        "Run the full quality gate: format, clippy, test, layers, comments, legal, i18n, changelog, deny, wasm; a clean tree that passed is not run again (--force runs it)",
    ),
    (
        "changelog",
        "Check the changelogs (changelog/README.md); --json <file> writes them for the website; gather <YY.M.N> [--toolbox <version>] moves changelog/unreleased/ into a release",
    ),
    (
        "adr",
        "The decision records (ADR-0188): adr [check] checks their numbers, adr next names the next free number, adr list prints the index",
    ),
    ("layers", "Check the layering rules (docs/adr/0014-refined-layer-model.md)"),
    ("comments", "Check that Rust sources have no comments (docs/adr/0194-comments.md)"),
    (
        "legal",
        "Check this change against the clean-room rules (docs/adr/0280-clean-room-process.md): other products' names, images, secrets, sign-offs; --all lists the whole tree's findings",
    ),
    (
        "i18n",
        "Check that every translation matches the English messages and that code asks only for messages that exist (docs/adr/0024-localisation.md)",
    ),
    ("dev-server", "Run a local release server with test releases (docs/release-manifest.md)"),
    ("dev-publish", "Publish a test release to the local server: dev-publish <product> <version>"),
    ("icon-import", "Import a product icon exported from Figma: icon-import <export.svg> <product-id>"),
    ("release-toolbox", "Build, publish and package Zen Toolbox for macOS: release-toolbox <version> [notes]"),
    (
        "release",
        "Build and publish for macOS: release ides <YY.M.N> [--allow-version] (every IDE, ADR-0146); release toolbox <version>; release plugin <id> <version>",
    ),
    ("release-version", "Print the IDEs' next calendar version, YY.M.N (ADR-0146)"),
    (
        "release-build",
        "Build a release without secrets into a folder with build.json (ADR-0142): release-build ides <YY.M.N> [--allow-version] | toolbox <version> | plugin <id> <version>, [--out <dir>]; <ide|toolbox> <version> --trial builds one product without the release checks, never published",
    ),
    (
        "release-publish",
        "Verify, sign and publish a build (run locally by the owner, or on the protected release runner, ADR-0171): release-publish (--from <dir> | --pipeline <id> | --job <id>) [--dry-run] [--stage <name>]",
    ),
    (
        "release-ci",
        "The release pipeline of a protected tag v<YY.M.N> (ADR-0171): release-ci plan | stage | live [<tag>]; stage builds and stages everything, live (the button) makes it public",
    ),
    (
        "release-notes-sync",
        "Make the published release notes match changelog/<product>.md (run locally by the owner): release-notes-sync <product>",
    ),
    (
        "release-linux",
        "Build a release for Linux in Docker and publish it, on request: release-linux <product> <version>",
    ),
    ("bench-editor", "Measure typing and scrolling frame times on 1k/10k/100k-line Go files (docs/performance.md)"),
    (
        "release-windows",
        "Build a release for Windows in Docker and publish it, on request: release-windows <product> <version>",
    ),
    (
        "bundle-plugins",
        "Lay out an IDE's bundled plug-ins for ZEN_BUNDLED_PLUGINS (development builds): bundle-plugins <product> <folder>",
    ),
    ("mascot", "Write the website's pictures of Kozō from the sprites (ADR-0125) into deploy/releases/site/img/kozo"),
    ("site-docs", "Build the website's documentation from docs/guide and the keymap: site-docs [<folder>]"),
    (
        "spelling-dictionary",
        "Encode the bundled English dictionary from Webster's Second (ADR-0113): spelling-dictionary [<web2 file>]",
    ),
    (
        "notices",
        "Third-party licence notices (docs/licences.md): notices checks every shipped crate has a licence text; notices <product> [<target>] <file> writes them",
    ),
    ("help", "Print this list of tasks"),
];

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        None | Some("help" | "--help" | "-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("ci") => report(gate_memo::command(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("release-notes-sync") => report(release::notes_sync(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("release-linux") => report(linux::release_command(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("release-windows") => report(windows::release(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("bundle-plugins") => report(release::bundle_command(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("mascot") => report(mascot::run(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("site-docs") => report(site::run(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("bench-editor") => report(bench::run()),
        Some("spelling-dictionary") => report(spelling::run(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("notices") => report(notices::command(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("layers") => report(layers::run()),
        Some("comments") => report(comments::run()),
        Some("legal") => report(legal::run(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("changelog") => report(changelog::run(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("adr") => report(adr::run(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("i18n") => report(i18n::run()),
        Some("dev-server") => report(dev::server()),
        Some("icon-import") => report(icon::import(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("release") => report(release::ide(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("release-version") => report(release::next_version()),
        Some("release-toolbox") => report(release::toolbox(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("release-build") => report(release_flow::build_command(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("release-publish") => report(release_flow::publish_command(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("release-ci") => report(release_ci::command(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("dev-publish") => report(dev::publish_command(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some(unknown) => {
            eprintln!("error: unknown task `{unknown}`\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!("Usage: cargo xtask <task>\n\nTasks:");
    for (name, description) in TASKS {
        println!("  {name:<16} {description}");
    }
}
