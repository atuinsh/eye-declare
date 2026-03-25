pub mod app;
pub mod children;
pub mod component;
pub mod components;
pub mod element;
pub mod hooks;
pub mod inline;
pub mod insets;

pub(crate) mod escape;
pub(crate) mod frame;
pub(crate) mod node;
pub(crate) mod renderer;
pub(crate) mod wrap;

// Re-export key types at the crate root for convenience
pub use app::{Application, ApplicationBuilder, CommittedElement, ControlFlow, Handle};
pub use children::{AddTo, ChildCollector, ComponentWithSlot, DataHandle, SpliceInto};
pub use component::{Component, EventResult, HStack, Tracked, VStack};
pub use components::markdown::{Markdown, MarkdownState};
pub use components::spinner::{Spinner, SpinnerState};
pub use components::text::{Line, Span, TextBlock};
pub use element::{ElementHandle, Elements};
pub use hooks::Hooks;
pub use inline::InlineRenderer;
pub use insets::Insets;
pub use node::{NodeId, WidthConstraint};

// Re-export the element! proc macro
#[cfg(feature = "macros")]
pub use eye_declare_macros::element;
