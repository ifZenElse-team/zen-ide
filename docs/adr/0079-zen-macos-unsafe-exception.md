# ADR-0079: zen-macos, the one crate allowed `unsafe`, so that a system quit asks first

- **Status:** Accepted
- **Date:** 2026-10-20
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (decision 5: `unsafe` only in
  listed exceptions), [ADR-0004](0004-monorepo-and-layering.md) and
  [ADR-0014](0014-refined-layer-model.md) (the layering rules), [ADR-0031](0031-plugins.md) (the
  plug-ins' `#[unsafe(no_mangle)]` exports)

## Context

⌘Q and **GoGarden → Quit GoGarden** ask **Are you sure you want to exit GoGarden?** (Confirm Exit,
on by default) and exiting stops the runs and debug sessions first. Quit from the Dock, logging
out, restarting the Mac and `osascript -e 'quit app "GoGarden"'` did not ask: macOS sends the
application a quit Apple event, `-[NSApplication terminate:]` asks the application delegate's
`applicationShouldTerminate:`, and the delegate winit 0.30 installs does not implement it, so the
application ended at once. A program started from a run configuration was left running, adopted by
`launchd`. The requirement to confirm before quitting (⌘Q) was not met for these quits.

There is no safe way to answer `applicationShouldTerminate:`: winit exposes no hook for it, and
`objc2` can only add a method to an existing class through the Objective-C runtime's
`class_addMethod`, which is `unsafe` (its function pointer and type encoding cannot be checked by
the compiler). The owner chose (2026-10-20) to allow a documented `unsafe` exception for this,
rather than leaving the Dock out of the feature.

## Decision

1. **A platform crate `zen-macos` (`crates/platform/macos`, layer L0)** holds the macOS application
   hooks that have no safe binding. It depends on no Zen crate and on no UI toolkit; its
   Objective-C dependencies (`objc2`, `objc2-app-kit`, versions already in the tree) are declared
   for `cfg(target_os = "macos")` only, and elsewhere the crate is plain Rust that installs nothing
   (it builds for WebAssembly like every crate below L6).
2. **It is the first and only workspace crate allowed `unsafe`.** Its `Cargo.toml` copies the
   workspace lints with `unsafe_code = "deny"` instead of `"forbid"` (as a plug-in crate does,
   ADR-0031) and adds `clippy::undocumented_unsafe_blocks = "deny"`. The boundary:
   - the smallest possible surface: one private module (`src/appkit.rs`, macOS only) with
     `#![allow(unsafe_code)]`; every other module keeps the deny;
   - every `unsafe` block carries a `// SAFETY:` comment saying why it is sound;
   - only what cannot be done in safe Rust is inside `unsafe` (today: turning the hook's function
     into an `Imp` and calling `class_addMethod`); every decision is made in safe, unit-tested code;
   - no other `unsafe` goes into this crate without a new ADR, and no other crate gets an exception
     without one.
3. **The exception is listed in the layering rules**: AGENTS.md rule 3 and section 4,
   `crates/platform/README.md`, and the crate's entry in `xtask/src/layers.rs`. `cargo xtask
   layers` does not check lints; `unsafe_code = "forbid"` in the workspace lints is what keeps
   every other crate free of `unsafe`, so a crate that copies the lints instead of
   `[lints] workspace = true` is a review stop unless it is listed there.
4. **The hook.** On the first frame the IDE calls `zen_macos::intercept_system_quit`, which adds
   `applicationShouldTerminate:` to the class of the delegate winit installed
   (`NSApp.delegate`), only if that class does not answer it already, so none of winit's own
   behaviour is replaced. The method answers with `zen_macos::QuitGate`:
   - while the IDE is not exiting, it answers `NSTerminateCancel`, records the request and wakes
     the IDE, which turns it into its ordinary exit request, the same path as ⌘Q: the Confirm Exit
     dialog naming the running work when the setting is on, an exit at once when it is off, the
     runs and debug sessions stopped in both cases;
   - once the IDE exits or restarts (`QuitGate::allow`), it answers `NSTerminateNow`, so the IDE's
     own exit is never held back.
5. **Logging out.** The system's quit is cancelled when it arrives, so macOS stops the logout and
   says GoGarden interrupted it; once the user confirms **Exit**, GoGarden quits (its runs stopped)
   and the logout can be started again. `NSTerminateLater` with
   `replyToApplicationShouldTerminate:` was not chosen: it runs the run loop in a modal mode while
   the IDE waits for the user, which the winit event loop is not built for.

## Consequences

- Quit from the Dock and every other system quit ask as ⌘Q does, and no longer leave programs
  running after GoGarden has quit.
- The project now contains `unsafe` code, in one reviewed file of one crate. A winit upgrade that
  implements `applicationShouldTerminate:` itself makes the hook stand aside (it is not added);
  the change must then be revisited.
- The exit's moment is measured on a monotonic clock instead of egui's input time, which stands
  still while no window is drawn: an exit asked for by the Dock while every window is covered
  happens at once instead of waiting for the next drawn frame.
