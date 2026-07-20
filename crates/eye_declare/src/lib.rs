//! Inline terminal UIs: timeline-first, Elm-shaped, built on Ratatui.
//!
//! Built on `eye_declare_engine`; designed in `.planning/REDESIGN.md` and
//! validated by the `spike` crate's bake-off and a production port. Core
//! commitments:
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

pub mod app;
#[cfg(feature = "tokio")]
pub mod driver_tokio;
pub mod element;
pub mod focus;
pub mod input;
#[cfg(feature = "markdown")]
pub mod markdown;
pub mod panel;
pub mod runtime;
pub mod spinner;
pub mod stack;
pub mod subscription;
pub mod task;
pub mod text;
pub mod text_area;
pub mod timeline;
pub mod viewport;

pub use app::{App, Ctx};
pub use element::{AnyElement, Element, ElementExt, Empty, Fluent, Padded, empty};
pub use focus::{Focus, FocusHandle};
pub use input::{InputEvent, Key, Keymap, key, keymap};
#[cfg(feature = "markdown")]
pub use markdown::{Markdown, MarkdownStyles, markdown};
pub use panel::{Panel, panel};
pub use runtime::{KeyboardProtocol, RunOptions, Runtime, run, run_with};
pub use spinner::{Spinner, spinner};
pub use stack::{Col, Row, Width, col, row};
pub use subscription::Subscriptions;
pub use task::{Effect, MsgStream, Task};
pub use text::{Text, text};
pub use text_area::{TextArea, TextAreaState, text_area};
pub use timeline::Timeline;
pub use viewport::{Viewport, viewport};
