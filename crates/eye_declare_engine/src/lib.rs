//! The inline terminal rendering engine behind `eye_declare`.
//!
//! Contract: frames in, terminal-synced scrollback out. This crate knows
//! nothing about components, element trees, or reconciliation — it speaks
//! `ratatui` `Buffer`s and emits ANSI escape bytes.
//!
//! Modules: `engine` (the terminal-sync state machine: row accounting,
//! scrollback streaming, resize/finalize), `frame` (buffer diffing),
//! `escape` (ANSI generation, relative-cursor discipline, synchronized
//! output), `wrap` (word-wrap measurement), and — behind the `test-util`
//! feature — `test_terminal`, a VTE-based terminal emulator for headless
//! tests.

pub mod engine;
pub mod escape;
pub mod frame;
#[cfg(feature = "test-util")]
pub mod test_terminal;
pub mod wrap;

pub use engine::Engine;
