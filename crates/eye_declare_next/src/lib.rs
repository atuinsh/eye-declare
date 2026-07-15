//! The v2 timeline/Elm layer (working name; spec question O4).
//!
//! Built on `eye_declare_engine` per `.planning/REDESIGN.md` and validated
//! by the `spike` crate's bake-off. Core commitments:
//!
//! - **Committed output is an effect; the live tail is a view.** Blocks are
//!   pushed once from `update` and flow into scrollback; only the small
//!   tail re-renders, every frame, with no dirty tracking.
//! - **Strict-Elm state** (bake-off O1): widget state lives in the app
//!   model as plain values; views borrow it.
//! - **`Msg`-free elements** (bake-off O7): elements describe structure and
//!   pixels only. All message emission happens in the keymap layer, so the
//!   element tree carries no message type parameter.
//! - **Honest measurement:** `Element::height(width)` is required, exact,
//!   and cheap. No probe rendering.
//!
//! Build order (thin slices): element layer + layout + text ✅ → runtime
//! core (tail present loop) → blocks/commit → keymap/focus/events → async
//! driver (spawn/Task/subscriptions) → widgets (text area, markdown,
//! viewport).

pub mod element;
pub mod spinner;
pub mod stack;
pub mod text;

pub use element::{AnyElement, Element, ElementExt, Empty, Fluent, Padded, empty};
pub use spinner::{Spinner, spinner};
pub use stack::{Col, Row, Width, col, row};
pub use text::{Text, text};
