# ADR-0002: Open source, clean-room, pure Rust

- **Status:** Accepted; decision 2 (the sentence on software under an open licence) amended by [ADR-0280](0280-clean-room-process.md)
- **Date:** 2026-10-16

## Context

Zen IDE is an independent, free implementation of a family of IDEs: open source, written from
scratch, native, and drivable by tools and agents. The project is non-commercial.

## Decision

1. **License.** The code is dual-licensed under MIT or Apache-2.0, at the user's option.
   Contributions are accepted under the same terms (inbound = outbound), certified with a
   Developer Certificate of Origin sign-off.
2. **Clean-room.** No code, shaders, icons, images, keymap files, colour schemes or other assets
   are copied from any other IDE or product. This includes software published under
   an open licence: we may use its documentation to understand
   behaviour, but never its code or assets. Compatibility is achieved through public
   specifications, public documentation and observed behaviour.
3. **AI-generated code is held to the same rule.** Agents must not reproduce code from other IDEs
   from memory. A contribution that looks like it was derived from another project's source is
   rejected.
4. **Pure Rust.** The default build contains no C or C++ code. Native code is allowed only behind
   an opt-in Cargo feature, in an edge crate, with an ADR.
5. **`unsafe` is forbidden** (`#![forbid(unsafe_code)]`) except in crates listed as exceptions in
   the layering rules, each with a documented reason.
6. **Third-party dependencies** must have licences compatible with MIT and Apache-2.0
   (for example MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0 for unmodified use). Copyleft licences
   such as GPL and AGPL are not allowed. This is checked in CI with `cargo deny`.
7. **No telemetry, no accounts, no licence checks.** Network access is limited to explicit actions
   (updates, toolchain downloads) and can be turned off.

## Consequences

- The project can be used, forked and redistributed freely, including inside companies.
- Every feature costs more than copying would, and some behaviour has to be rediscovered.
- Using external language servers (for example rust-analyzer, MIT/Apache-2.0) as separate
  processes or libraries is allowed: it is use of a dependency, not copying.

## Alternatives considered

- **Fork an existing open-source IDE:** gives an IDE immediately, but not one written in Rust,
  which contradicts the goals of a native, fast, pure-Rust platform.
- **GPL licence:** protects against closed forks but blocks use in many organisations and does
  not match the ecosystem of Rust libraries the project builds on.
