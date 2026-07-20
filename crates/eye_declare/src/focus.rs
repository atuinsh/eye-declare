//! Focus as data (GPUI's `FocusHandle` pattern, per the bake-off).
//!
//! The app model owns a [`Focus`] system and creates [`FocusHandle`]s from
//! it. All handles from one system share a single "currently focused" cell,
//! so exactly one handle is focused at a time *by construction* — there is
//! no framework focus registry to fall out of sync with, no autofocus
//! lifecycle, and no Tab cycling unless the app binds Tab to a message.
//!
//! - Moving focus is a plain call in `update`: `self.search_focus.focus()`.
//! - Views read `handle.is_focused()` for focus-dependent visuals.
//! - [`Keymap`](crate::input::Keymap) scopes bindings to handles.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The focus system for an app. Lives in the app model; hand out handles
/// via [`handle`](Focus::handle).
#[derive(Default)]
pub struct Focus {
    current: Arc<AtomicU64>,
    next_id: AtomicU64,
}

impl Focus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new focusable identity. The first handle created is NOT
    /// focused automatically — call [`FocusHandle::focus`] to set initial
    /// focus.
    pub fn handle(&self) -> FocusHandle {
        // ids start at 1; 0 means "nothing focused".
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        FocusHandle {
            id,
            current: Arc::clone(&self.current),
        }
    }

    /// Clear focus entirely (no handle focused).
    pub fn blur_all(&self) {
        self.current.store(0, Ordering::Relaxed);
    }
}

/// Identity of one focusable thing. Cheap to clone; clones share identity.
#[derive(Clone)]
pub struct FocusHandle {
    id: u64,
    current: Arc<AtomicU64>,
}

impl FocusHandle {
    /// Make this handle the focused one (unfocusing whichever was).
    pub fn focus(&self) {
        self.current.store(self.id, Ordering::Relaxed);
    }

    /// Unfocus, but only if this handle currently has focus.
    pub fn blur(&self) {
        let _ = self
            .current
            .compare_exchange(self.id, 0, Ordering::Relaxed, Ordering::Relaxed);
    }

    pub fn is_focused(&self) -> bool {
        self.current.load(Ordering::Relaxed) == self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_handle_focused() {
        let focus = Focus::new();
        let a = focus.handle();
        let b = focus.handle();

        assert!(!a.is_focused() && !b.is_focused());

        a.focus();
        assert!(a.is_focused() && !b.is_focused());

        b.focus();
        assert!(!a.is_focused() && b.is_focused());
    }

    #[test]
    fn blur_only_affects_the_holder() {
        let focus = Focus::new();
        let a = focus.handle();
        let b = focus.handle();

        a.focus();
        b.blur(); // b doesn't hold focus; no-op
        assert!(a.is_focused());

        a.blur();
        assert!(!a.is_focused() && !b.is_focused());
    }

    #[test]
    fn clones_share_identity() {
        let focus = Focus::new();
        let a = focus.handle();
        let a2 = a.clone();
        a.focus();
        assert!(a2.is_focused());
    }
}
