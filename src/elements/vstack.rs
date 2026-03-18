use crate::component::VStack;
use crate::element::Element;
use crate::node::NodeId;
use crate::renderer::Renderer;

/// Element builder for a [`VStack`] container.
///
/// Typically used with `Elements::add_with_children` or
/// `Elements::group` to create nested containers.
///
/// ```ignore
/// let mut children = Elements::new();
/// children.add(TextBlockEl::new().unstyled("child 1"));
/// children.add(TextBlockEl::new().unstyled("child 2"));
/// els.add_with_children(VStackEl, children);
/// ```
pub struct VStackEl;

impl Element for VStackEl {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
        renderer.append_child(parent, VStack)
    }
}
