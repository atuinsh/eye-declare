//! The inline terminal rendering engine behind `eye_declare`.
//!
//! Contract: frames in, terminal-synced scrollback out. This crate knows
//! nothing about components, element trees, or reconciliation; it speaks
//! ratatui `Buffer`s and emits ANSI escape bytes.
//!
//! For more information, see the [eye-declare documentation](https://docs.rs/eye_declare/).

pub mod engine;
pub mod escape;
pub mod frame;
#[cfg(feature = "test-util")]
pub mod test_terminal;
pub mod wrap;

pub use engine::Engine;
