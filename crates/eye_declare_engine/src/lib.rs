//! The inline terminal rendering engine extracted from `eye_declare`
//! (redesign Phase 2 — see `.planning/REDESIGN.md` at the repo root).
//!
//! Contract: frames in, terminal-synced scrollback out. This crate knows
//! nothing about components, element trees, or reconciliation — it speaks
//! `ratatui` `Buffer`s and emits ANSI escape bytes.
//!
//! Extraction in progress: `frame` (buffer diffing), `escape` (ANSI
//! generation, relative-cursor discipline, synchronized output), and `wrap`
//! (word-wrap measurement) have moved; the terminal-sync state machine from
//! `inline.rs` (row accounting, scrollback streaming, resize/finalize) is
//! next, along with the VTE test terminal.
//!
//! `publish = false` until the extraction completes and the crate is named
//! for real (spec question O4).

pub mod engine;
pub mod escape;
pub mod frame;
#[cfg(feature = "test-util")]
pub mod test_terminal;
pub mod wrap;

pub use engine::Engine;
