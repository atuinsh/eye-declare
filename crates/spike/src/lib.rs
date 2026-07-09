//! Compile-only bake-off spike for the eye-declare v2 DSL.
//!
//! Phase 1 of `.planning/REDESIGN.md`. This crate exists to test the *call-site
//! shape* of the candidate API against real views ported from Atuin AI
//! (`~/src/atuin/crates/atuin-ai/src/tui/`). Nothing here renders; `cargo check`
//! is the bar. Ergonomics findings accumulate in `FINDINGS.md`.

#![allow(dead_code)]

pub mod fixtures;
pub mod ports;
pub mod ui;
