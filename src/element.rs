use crate::node::NodeId;
use crate::renderer::Renderer;

/// Describes a component to create in the tree.
///
/// Each built-in component has a corresponding element builder (e.g.,
/// `TextBlockEl`, `SpinnerEl`). Users can implement this trait for
/// custom components.
///
/// The `build` method creates the component, adds it as a child of
/// `parent`, initializes its state, and returns the new NodeId.
pub trait Element: Send {
    /// Create the component, add it as a child of `parent`,
    /// and initialize its state. Returns the new NodeId.
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId;
}

/// An entry in an Elements list: an element description with optional children.
struct ElementEntry {
    element: Box<dyn Element>,
    children: Option<Elements>,
}

/// A list of element descriptions for declarative tree building.
///
/// Used with `Renderer::rebuild` to describe what the tree should
/// look like. View functions return `Elements`.
///
/// ```ignore
/// fn my_view(state: &MyState) -> Elements {
///     let mut els = Elements::new();
///     els.add(TextBlockEl::new().unstyled("Hello"));
///     if state.loading {
///         els.add(SpinnerEl::new("Loading..."));
///     }
///     els
/// }
/// ```
pub struct Elements {
    items: Vec<ElementEntry>,
}

impl Elements {
    /// Create an empty element list.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add an element to the list.
    pub fn add(&mut self, element: impl Element + 'static) -> &mut Self {
        self.items.push(ElementEntry {
            element: Box::new(element),
            children: None,
        });
        self
    }

    /// Add an element with nested children.
    ///
    /// The element is created first, then children are built as its
    /// descendants.
    pub fn add_with_children(
        &mut self,
        element: impl Element + 'static,
        children: Elements,
    ) -> &mut Self {
        self.items.push(ElementEntry {
            element: Box::new(element),
            children: Some(children),
        });
        self
    }

    /// Add a VStack wrapper around the given children.
    ///
    /// Shorthand for `add_with_children(VStackEl, children)`.
    pub fn group(&mut self, children: Elements) -> &mut Self {
        self.add_with_children(crate::elements::VStackEl, children)
    }

    /// Consume the Elements and build all entries into the tree
    /// as children of `parent`.
    pub(crate) fn build_into(self, renderer: &mut Renderer, parent: NodeId) {
        for entry in self.items {
            let node_id = entry.element.build(renderer, parent);
            if let Some(children) = entry.children {
                children.build_into(renderer, node_id);
            }
        }
    }
}

impl Default for Elements {
    fn default() -> Self {
        Self::new()
    }
}
