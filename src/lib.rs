pub mod component;
pub mod components;
pub mod escape;
pub mod frame;
pub mod inline;
pub mod node;
pub mod renderer;
pub mod wrap;

// Re-export key types at the crate root for convenience
pub use component::{Component, EventResult, Tracked, VStack};
pub use components::text::{TextBlock, TextState};
pub use escape::CursorState;
pub use frame::{Diff, Frame};
pub use inline::InlineRenderer;
pub use node::NodeId;
pub use renderer::Renderer;
