#![allow(clippy::print_stdout, reason = "a build script talks to cargo through standard output")]

use std::path::PathBuf;

const UNRELEASED: &str = "This driver program was not built for a release, so it carries no third-party notices; \
the release build adds them (docs/licences.md).\n";

fn main() {
    println!("cargo::rerun-if-env-changed=ZEN_THIRD_PARTY_NOTICES");
    let text = match std::env::var_os("ZEN_THIRD_PARTY_NOTICES").map(PathBuf::from) {
        Some(path) => {
            println!("cargo::rerun-if-changed={}", path.display());
            std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        }
        None => UNRELEASED.as_bytes().to_vec(),
    };
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("THIRD-PARTY-NOTICES");
    std::fs::write(&out, text).unwrap_or_else(|error| panic!("{}: {error}", out.display()));
}
