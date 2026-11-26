# Rust module: its name and description. English is the source language (ADR-0024).
module-rust-name = Rust
module-rust-description = Everything for Rust: highlighting, Cargo workspaces and crates, rust-analyzer (completion, navigation, problems, refactorings), the Rust toolchain and its settings.

# The Cargo project model.
rust-libraries-crates = Crates
rust-model-no-cargo = No Rust toolchain is installed: cargo was not found. Install Rust from https://rustup.rs/.
rust-model-offline = { $message } (the IDE reads Cargo projects offline: download the dependencies with `cargo fetch` or a build, then run Reload Project Model)
rust-toolchain-error-none = This IDE has no Rust toolchain.

file-type-rust = Rust

rust-postfix-let = Puts the expression into a new variable
rust-postfix-if = Runs a block when the expression is true
rust-postfix-else = Runs a block when the expression is false
rust-postfix-not = Negates the expression
rust-postfix-while = Loops while the expression is true
rust-postfix-for = Loops over the expression's items
rust-postfix-match = Matches the expression
rust-postfix-ifl = Runs a block when the expression is Some
rust-postfix-ref = Borrows the expression
rust-postfix-refm = Borrows the expression mutably
rust-postfix-some = Wraps the expression in Some
rust-postfix-ok = Wraps the expression in Ok
rust-postfix-err = Wraps the expression in Err
rust-postfix-box = Puts the expression in a Box
rust-postfix-dbg = Prints the expression with dbg!
rust-postfix-println = Prints the expression
rust-postfix-return = Returns the expression
rust-postfix-par = Puts the expression in parentheses
rust-tests-framework-builtin = Built-in #[test]
