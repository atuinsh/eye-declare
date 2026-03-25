use crate::component::Component;
use crate::element::{ElementHandle, Elements};
use crate::node::WidthConstraint;

// ---------------------------------------------------------------------------
// AddTo — how a value adds itself to a collector
// ---------------------------------------------------------------------------

/// Trait for adding a value to a child collector.
///
/// The element! macro dispatches all child additions through this trait,
/// enabling compile-time type checking of parent-child relationships.
///
/// Blanket impl: any [`Component`] can be added to [`Elements`].
/// Data types (like [`Line`](crate::Line), [`Span`](crate::Span)) implement
/// this for their specific collector types.
pub trait AddTo<Collector: ?Sized> {
    /// Handle returned after adding. Supports `.key()` / `.width()` chaining.
    type Handle<'a>
    where
        Collector: 'a;

    fn add_to(self, collector: &mut Collector) -> Self::Handle<'_>;
}

/// Blanket: any Component can be added to Elements.
impl<C: Component> AddTo<Elements> for C {
    type Handle<'a> = ElementHandle<'a>;

    fn add_to(self, els: &mut Elements) -> ElementHandle<'_> {
        els.add(self)
    }
}

// ---------------------------------------------------------------------------
// SpliceInto — how Elements splice into a collector
// ---------------------------------------------------------------------------

/// Trait for splicing an [`Elements`] list into a collector.
///
/// Only implemented for `Elements` → `Elements`. Custom collectors
/// that don't support splicing will produce a compile error if
/// `#(expr)` is used inside their children block.
pub trait SpliceInto<Collector: ?Sized> {
    fn splice_into(self, collector: &mut Collector);
}

impl SpliceInto<Elements> for Elements {
    fn splice_into(self, collector: &mut Elements) {
        collector.splice(self);
    }
}

// ---------------------------------------------------------------------------
// ChildCollector — how a component collects and finalizes children
// ---------------------------------------------------------------------------

/// Declares how a component collects children in the `element!` macro.
///
/// Components that accept children in `element!` must implement this trait.
/// The `Collector` type determines what child types are accepted (via [`AddTo`]).
///
/// - **Layout containers** (VStack, HStack): use `Elements` as collector,
///   `finish` wraps in [`ComponentWithSlot`] to pass children as slot.
/// - **Data-absorbing components** (TextBlock): use a custom collector,
///   `finish` absorbs the data and returns the component directly.
///
/// Components that don't implement `ChildCollector` will produce a
/// compile error if used with children in `element!`.
pub trait ChildCollector: Sized {
    /// The type used to accumulate children.
    type Collector: Default;

    /// The output type after finalizing children.
    ///
    /// For layout containers: [`ComponentWithSlot<Self>`].
    /// For data components: `Self` (with data absorbed).
    type Output;

    /// Finalize children collection, producing the output value.
    fn finish(self, collector: Self::Collector) -> Self::Output;
}

// ---------------------------------------------------------------------------
// ComponentWithSlot — wrapper for component + slot children
// ---------------------------------------------------------------------------

/// Wrapper that carries a component together with its slot children.
///
/// Produced by layout containers' [`ChildCollector::finish`].
/// Implements [`AddTo<Elements>`] by calling `add_with_children`.
pub struct ComponentWithSlot<C> {
    component: C,
    children: Elements,
}

impl<C> ComponentWithSlot<C> {
    /// Create a new component-with-slot wrapper.
    pub fn new(component: C, children: Elements) -> Self {
        Self {
            component,
            children,
        }
    }
}

impl<C: Component> AddTo<Elements> for ComponentWithSlot<C> {
    type Handle<'a> = ElementHandle<'a>;

    fn add_to(self, els: &mut Elements) -> ElementHandle<'_> {
        els.add_with_children(self.component, self.children)
    }
}

// ---------------------------------------------------------------------------
// DataHandle — no-op handle for data child additions
// ---------------------------------------------------------------------------

/// No-op handle returned when adding data children to a custom collector.
///
/// Provides `.key()` and `.width()` methods that silently do nothing,
/// so the macro's key/width chaining compiles regardless of context.
pub struct DataHandle;

impl DataHandle {
    pub fn key(self, _key: impl Into<String>) -> Self {
        self
    }

    pub fn width(self, _constraint: WidthConstraint) -> Self {
        self
    }
}
