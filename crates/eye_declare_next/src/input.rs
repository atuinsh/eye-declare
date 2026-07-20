//! Input events and the keymap: keys resolve to messages as *data*,
//! rebuilt from the model each update so bindings can be conditional on
//! app state.
//!
//! Dispatch order (bake-off O2, first match in declaration order wins):
//! 1. [`on_override`](Keymap::on_override) bindings — Ctrl+C-tier chords
//!    that fire regardless of focus
//! 2. [`in_scope`](Keymap::in_scope) bindings for the focused handle
//! 3. [`on`](Keymap::on) global bindings
//! 4. [`fallthrough`](Keymap::fallthrough) mappers for the focused handle
//!    (raw events → messages; how a text input receives ordinary typing)

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::focus::FocusHandle;

/// A terminal input event as delivered to the app: key press or paste.
#[derive(Clone, Debug)]
pub enum InputEvent {
    Key(KeyEvent),
    Paste(String),
}

/// A key pattern for bindings.
///
/// Matching is exact on code + modifiers. Note the usual terminal wart:
/// shifted characters arrive as the shifted char (`Char('J')` + SHIFT),
/// so bind the character you expect to receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

pub fn key(code: KeyCode) -> Key {
    Key {
        code,
        mods: KeyModifiers::NONE,
    }
}

impl Key {
    pub fn ctrl(mut self) -> Self {
        self.mods |= KeyModifiers::CONTROL;
        self
    }

    pub fn shift(mut self) -> Self {
        self.mods |= KeyModifiers::SHIFT;
        self
    }

    pub fn alt(mut self) -> Self {
        self.mods |= KeyModifiers::ALT;
        self
    }

    fn matches(&self, event: &KeyEvent) -> bool {
        self.code == event.code && self.mods == event.modifiers
    }
}

enum Scope {
    /// Fires before focus-scoped and global bindings. For chords that must
    /// work regardless of what's focused (Ctrl+C-tier).
    Override,
    /// Fires while the handle has focus.
    Focus(FocusHandle),
    /// Fires if no override or focus-scoped binding matched.
    Global,
}

type FallthroughFn<Msg> = Box<dyn Fn(InputEvent) -> Msg + Send>;

/// Key → message bindings. Values, not callbacks: rebuild from the model
/// each update (see the conditional Tab bindings in the spike's Port 3A —
/// this is what makes key-policy conflicts impossible by construction).
pub struct Keymap<Msg> {
    bindings: Vec<(Scope, Key, Msg)>,
    fallthrough: Vec<(FocusHandle, FallthroughFn<Msg>)>,
}

impl<Msg> Default for Keymap<Msg> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn keymap<Msg>() -> Keymap<Msg> {
    Keymap::new()
}

impl<Msg> Keymap<Msg> {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
            fallthrough: Vec::new(),
        }
    }

    /// Global binding, checked after focus-scoped bindings.
    pub fn on(mut self, key: Key, msg: Msg) -> Self {
        self.bindings.push((Scope::Global, key, msg));
        self
    }

    /// Binding that fires before everything else, regardless of focus.
    pub fn on_override(mut self, key: Key, msg: Msg) -> Self {
        self.bindings.push((Scope::Override, key, msg));
        self
    }

    /// Binding active while `focus` is focused.
    pub fn in_scope(mut self, focus: &FocusHandle, key: Key, msg: Msg) -> Self {
        self.bindings.push((Scope::Focus(focus.clone()), key, msg));
        self
    }

    /// Route events unclaimed by any binding to a message while `focus` is
    /// focused. This is how a text input receives ordinary typing (and
    /// pastes) without the framework owning any editing logic.
    pub fn fallthrough(
        mut self,
        focus: &FocusHandle,
        map: impl Fn(InputEvent) -> Msg + Send + 'static,
    ) -> Self {
        self.fallthrough.push((focus.clone(), Box::new(map)));
        self
    }

    /// Append another keymap's bindings. Within each scope tier, earlier
    /// declarations still win, so a parent that merges a child's keymap
    /// after its own bindings keeps priority on contested keys — the usual
    /// composition is `parent_bindings.merge(child.keymap().map(Msg::Child))`.
    pub fn merge(mut self, other: Keymap<Msg>) -> Self {
        self.bindings.extend(other.bindings);
        self.fallthrough.extend(other.fallthrough);
        self
    }

    /// Re-target to a parent message type (Elm's `Html.map` for keymaps) —
    /// how a sub-model's keymap embeds into the app's.
    pub fn map<M2>(self, f: impl Fn(Msg) -> M2 + Clone + Send + 'static) -> Keymap<M2>
    where
        Msg: 'static,
    {
        Keymap {
            bindings: self
                .bindings
                .into_iter()
                .map(|(scope, key, msg)| (scope, key, f(msg)))
                .collect(),
            fallthrough: self
                .fallthrough
                .into_iter()
                .map(|(focus, g)| {
                    let f = f.clone();
                    let mapped: FallthroughFn<M2> = Box::new(move |ev| f(g(ev)));
                    (focus, mapped)
                })
                .collect(),
        }
    }
}

impl<Msg: Clone> Keymap<Msg> {
    /// Resolve an event to a message, or `None` if nothing claims it.
    pub fn dispatch(&self, event: &InputEvent) -> Option<Msg> {
        if let InputEvent::Key(k) = event {
            for (scope, key, msg) in &self.bindings {
                if matches!(scope, Scope::Override) && key.matches(k) {
                    return Some(msg.clone());
                }
            }
            for (scope, key, msg) in &self.bindings {
                if let Scope::Focus(handle) = scope
                    && handle.is_focused()
                    && key.matches(k)
                {
                    return Some(msg.clone());
                }
            }
            for (scope, key, msg) in &self.bindings {
                if matches!(scope, Scope::Global) && key.matches(k) {
                    return Some(msg.clone());
                }
            }
        }

        // Fallthrough handles both unclaimed keys and pastes.
        for (handle, map) in &self.fallthrough {
            if handle.is_focused() {
                return Some(map(event.clone()));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::Focus;

    fn press(code: KeyCode) -> InputEvent {
        InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn press_mod(code: KeyCode, mods: KeyModifiers) -> InputEvent {
        InputEvent::Key(KeyEvent::new(code, mods))
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Interrupt,
        Submit,
        GlobalHelp,
        Edit(char),
    }

    fn edit_msg(ev: InputEvent) -> Msg {
        match ev {
            InputEvent::Key(k) => {
                if let KeyCode::Char(c) = k.code {
                    Msg::Edit(c)
                } else {
                    Msg::Edit('?')
                }
            }
            InputEvent::Paste(_) => Msg::Edit('P'),
        }
    }

    #[test]
    fn merge_appends_and_earlier_declarations_win() {
        // The composition shape: a child sub-model's keymap, mapped and
        // merged after the parent's own bindings.
        let child = keymap()
            .on(key(KeyCode::Up), Msg::Edit('u'))
            .on(key(KeyCode::Enter), Msg::Edit('!'));
        let km = keymap().on(key(KeyCode::Enter), Msg::Submit).merge(child);

        // The child's uncontested binding works…
        assert_eq!(km.dispatch(&press(KeyCode::Up)), Some(Msg::Edit('u')));
        // …but on a contested key the parent, declared first, wins.
        assert_eq!(km.dispatch(&press(KeyCode::Enter)), Some(Msg::Submit));
    }

    #[test]
    fn dispatch_order_override_scoped_global_fallthrough() {
        let focus = Focus::new();
        let input = focus.handle();
        input.focus();

        let km = keymap()
            .on_override(key(KeyCode::Char('c')).ctrl(), Msg::Interrupt)
            .in_scope(&input, key(KeyCode::Enter), Msg::Submit)
            .on(key(KeyCode::Char('h')), Msg::GlobalHelp)
            .fallthrough(&input, edit_msg);

        assert_eq!(
            km.dispatch(&press_mod(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Msg::Interrupt)
        );
        assert_eq!(km.dispatch(&press(KeyCode::Enter)), Some(Msg::Submit));
        // 'h' is claimed by fallthrough? No — scoped/global bindings win
        // before fallthrough... but 'h' has no scoped binding, so global
        // fires before the focused fallthrough.
        assert_eq!(
            km.dispatch(&press(KeyCode::Char('h'))),
            Some(Msg::GlobalHelp)
        );
        assert_eq!(
            km.dispatch(&press(KeyCode::Char('x'))),
            Some(Msg::Edit('x'))
        );
        assert_eq!(
            km.dispatch(&InputEvent::Paste("hi".into())),
            Some(Msg::Edit('P'))
        );
    }

    #[test]
    fn scoped_bindings_inactive_without_focus() {
        let focus = Focus::new();
        let input = focus.handle();
        // not focused

        let km = keymap()
            .in_scope(&input, key(KeyCode::Enter), Msg::Submit)
            .fallthrough(&input, edit_msg);

        assert_eq!(km.dispatch(&press(KeyCode::Enter)), None);
        assert_eq!(km.dispatch(&press(KeyCode::Char('x'))), None);
    }

    #[test]
    fn first_match_in_declaration_order_wins() {
        let focus = Focus::new();
        let input = focus.handle();
        input.focus();

        // Two conditional Tab meanings, declared in priority order.
        let km = keymap()
            .in_scope(&input, key(KeyCode::Tab), Msg::Submit)
            .in_scope(&input, key(KeyCode::Tab), Msg::GlobalHelp);

        assert_eq!(km.dispatch(&press(KeyCode::Tab)), Some(Msg::Submit));
    }

    #[test]
    fn map_retargets_bindings_and_fallthrough() {
        #[derive(Clone, Debug, PartialEq)]
        enum Outer {
            Inner(Msg),
        }

        let focus = Focus::new();
        let input = focus.handle();
        input.focus();

        let km = keymap()
            .in_scope(&input, key(KeyCode::Enter), Msg::Submit)
            .fallthrough(&input, edit_msg)
            .map(Outer::Inner);

        assert_eq!(
            km.dispatch(&press(KeyCode::Enter)),
            Some(Outer::Inner(Msg::Submit))
        );
        assert_eq!(
            km.dispatch(&press(KeyCode::Char('z'))),
            Some(Outer::Inner(Msg::Edit('z')))
        );
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let km: Keymap<Msg> = keymap().on(key(KeyCode::Enter), Msg::Submit);
        assert_eq!(
            km.dispatch(&press_mod(KeyCode::Enter, KeyModifiers::SHIFT)),
            None
        );
    }
}
