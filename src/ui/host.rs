//! The panel host: a registry that turns "add a panel" into one new file and
//! one `register()` line.
//!
//! # What a panel author writes
//!
//! ```ignore
//! pub struct Mixer;
//!
//! impl Panel for Mixer {
//!     fn id(&self) -> &'static str { "mixer" }
//!     fn title(&self) -> &'static str { "Mixer" }
//!     fn dock(&self) -> Dock { Dock::Right }
//!     fn show(&mut self, ui: &mut egui::Ui, cx: &mut PanelCx<'_>) {
//!         kit::title(ui, cx.theme, "Mixer");
//!         if kit::button(ui, cx.theme, "panic") { cx.act(UiAction::Return); }
//!     }
//! }
//! ```
//!
//! Nothing else. No dock arithmetic, no `egui::Panel` id, no frame choice, no
//! visibility flag, no place in the app loop's call order.
//!
//! # Why `show` takes `ui` AND `cx` and not one bundle
//!
//! Because panels are supposed to use egui's layout (`ui.horizontal(|ui| …)`),
//! and every such closure hands back a FRESH `&mut Ui`. If the `Ui` lived
//! inside `cx`, each nesting level would have to rebuild the context. Split,
//! it just works: `ui` is the surface, `cx` is everything else, and both stay
//! usable inside any closure depth.
//!
//! # The two things the host owns that panels must not
//!
//! 1. **Order.** Docks are shown bars-first, then sides, then center — egui
//!    panels nest in show order, so this is layout, not preference.
//! 2. **Identity.** Ids are `Panel::id()`, unique by debug assert, and they
//!    are what preferences persist. Renaming an id forgets a user's layout.

use crate::ui::action::UiAction;
use crate::ui::keymap::Keymap;
use crate::ui::kit;
use crate::ui::theme::Theme;
use crate::ui::tokens::pane;
use crate::ui::vm::ViewState;
use eframe::egui::{self, KeyboardShortcut};

/// Which edge a panel attaches to. `Center` is the working area; more than
/// one visible center panel becomes a tab strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dock {
    Top,
    Bottom,
    Left,
    Right,
    Center,
}

/// How much room a panel takes. Values come from tokens, so a panel never
/// writes a width.
#[derive(Debug, Clone, Copy)]
pub enum Sizing {
    /// Fixed-height bar. Not resizable — transport and status bars.
    Bar,
    /// Resizable pane; the user's dragged size is remembered by egui itself.
    Pane { default: f32, min: f32, max: f32 },
    /// Takes what is left. Only meaningful for `Dock::Center`.
    Fill,
}

impl Sizing {
    /// The standard side pane, sized from `tokens::pane`.
    pub const fn side() -> Self {
        Self::Pane {
            default: pane::SIDE_W,
            min: pane::SIDE_W_MIN,
            max: pane::SIDE_W_MAX,
        }
    }
}

/// Everything a panel is allowed to know, minus the `Ui` it draws into.
///
/// It is a borrow of the app's frame state: read `vs`, name colors through
/// `theme`, emit wishes with `act`. There is no `&mut` anything else in here
/// on purpose — a panel that could reach further would be a panel that could
/// touch the engine.
pub struct PanelCx<'a> {
    pub theme: &'a Theme,
    pub vs: &'a ViewState,
    pub keys: &'a Keymap,
    actions: &'a mut Vec<UiAction>,
}

impl<'a> PanelCx<'a> {
    /// Build a context by hand. The host does this each frame; the gallery
    /// does it to render one panel against a synthetic `ViewState`, with no
    /// engine anywhere — which is the whole reason panels take a context
    /// instead of reaching for globals.
    pub fn new(
        theme: &'a Theme,
        vs: &'a ViewState,
        keys: &'a Keymap,
        actions: &'a mut Vec<UiAction>,
    ) -> Self {
        Self {
            theme,
            vs,
            keys,
            actions,
        }
    }

    /// Ask the app for something. This is the ONLY way a panel affects the world.
    pub fn act(&mut self, action: UiAction) {
        self.actions.push(action);
    }

    /// Density-scaled spacing, so panels write `cx.sp(space::MD)`.
    pub fn sp(&self, token: f32) -> f32 {
        self.theme.sp(token)
    }

    /// The gesture bound to an action, for tooltips. `None` = unbound.
    pub fn shortcut(&self, action: UiAction) -> Option<KeyboardShortcut> {
        self.keys.shortcut_for(action)
    }

    /// A button that emits an action.
    ///
    /// This is the idiom panels would otherwise hand-write every time, and
    /// hand-write inconsistently: it greys itself out when the action needs
    /// an engine that is not running, and puts the action's own keyboard
    /// shortcut in its tooltip. Label, key, and enabled-ness all come from
    /// one place, so they cannot drift apart.
    pub fn action_button(&mut self, ui: &mut egui::Ui, label: &str, action: UiAction) -> bool {
        let enabled = !action.needs_engine() || self.vs.engine_running;
        let hint = self
            .shortcut(action)
            .map(|sc| ui.ctx().format_shortcut(&sc));
        let clicked = ui
            .add_enabled_ui(enabled, |ui| {
                kit::button_hint(ui, self.theme, label, hint.as_deref())
            })
            .inner;
        if clicked {
            self.act(action);
        }
        clicked
    }

    /// A toggle that emits an action. Same guarantees as `action_button`.
    pub fn action_toggle(
        &mut self,
        ui: &mut egui::Ui,
        on: bool,
        label: &str,
        action: UiAction,
    ) -> bool {
        let enabled = !action.needs_engine() || self.vs.engine_running;
        let clicked = ui
            .add_enabled_ui(enabled, |ui| kit::toggle(ui, on, label))
            .inner;
        if clicked {
            self.act(action);
        }
        clicked
    }
}

pub trait Panel {
    /// Stable identity. Persisted in preferences — renaming it forgets the
    /// user's layout, so treat it like a database column name.
    fn id(&self) -> &'static str;

    /// Human name, for tab strips and the View menu.
    fn title(&self) -> &'static str;

    fn dock(&self) -> Dock;

    fn sizing(&self) -> Sizing {
        Sizing::side()
    }

    /// May the user hide it? Bars say no.
    fn closable(&self) -> bool {
        true
    }

    fn show(&mut self, ui: &mut egui::Ui, cx: &mut PanelCx<'_>);
}

/// What a View menu needs to render itself from the registry.
#[derive(Debug, Clone, Copy)]
pub struct PanelInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub dock: Dock,
    pub visible: bool,
    pub closable: bool,
}

struct Entry {
    panel: Box<dyn Panel>,
    visible: bool,
}

#[derive(Default)]
pub struct PanelHost {
    entries: Vec<Entry>,
    /// Which center panel is frontmost. `None` = the first visible one.
    focused_center: Option<String>,
}

impl PanelHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a panel. Registration order is show order within each dock.
    pub fn register(&mut self, panel: impl Panel + 'static) -> &mut Self {
        debug_assert!(
            !self.entries.iter().any(|e| e.panel.id() == panel.id()),
            "duplicate panel id `{}` — ids are persisted, they must be unique",
            panel.id()
        );
        self.entries.push(Entry {
            panel: Box::new(panel),
            visible: true,
        });
        self
    }

    pub fn is_visible(&self, id: &str) -> bool {
        self.entries.iter().any(|e| e.panel.id() == id && e.visible)
    }

    /// Show or hide a panel. Returns false for an unknown id or a panel that
    /// refuses to close — the caller learns nothing happened.
    pub fn set_visible(&mut self, id: &str, visible: bool) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| e.panel.id() == id) else {
            return false;
        };
        if !visible && !entry.panel.closable() {
            return false;
        }
        entry.visible = visible;
        true
    }

    pub fn toggle(&mut self, id: &str) -> bool {
        let now = self.is_visible(id);
        self.set_visible(id, !now)
    }

    /// Bring a center panel to the front. Also un-hides it, because asking
    /// for a hidden panel plainly means "show me that".
    pub fn focus(&mut self, id: &str) -> bool {
        let Some(entry) = self.entries.iter().find(|e| e.panel.id() == id) else {
            return false;
        };
        if entry.panel.dock() != Dock::Center {
            return false;
        }
        let id = entry.panel.id();
        self.focused_center = Some(id.to_owned());
        self.set_visible(id, true);
        true
    }

    pub fn focused_center(&self) -> Option<&str> {
        self.focused_center.as_deref()
    }

    pub fn set_focused_center(&mut self, id: Option<String>) {
        self.focused_center = id;
    }

    /// Ids of panels the user has hidden — exactly what preferences store.
    /// Stores the hidden set, not the visible one, so a panel added in a
    /// later version defaults to visible instead of invisible.
    pub fn hidden_ids(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| !e.visible)
            .map(|e| e.panel.id().to_owned())
            .collect()
    }

    /// Restore a hidden set. Unknown ids are ignored (a panel that no longer
    /// exists must not be an error), and non-closable panels stay shown.
    pub fn apply_hidden(&mut self, hidden: &[String]) {
        for entry in &mut self.entries {
            entry.visible = !hidden.iter().any(|h| h == entry.panel.id());
            if !entry.panel.closable() {
                entry.visible = true;
            }
        }
    }

    pub fn infos(&self) -> Vec<PanelInfo> {
        self.entries
            .iter()
            .map(|e| PanelInfo {
                id: e.panel.id(),
                title: e.panel.title(),
                dock: e.panel.dock(),
                visible: e.visible,
                closable: e.panel.closable(),
            })
            .collect()
    }

    /// Draw the whole application shell for this frame.
    ///
    /// Order is fixed and deliberate: egui panels claim space in show order,
    /// so bars run edge-to-edge and side panes sit between them.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        vs: &ViewState,
        keys: &Keymap,
        out: &mut Vec<UiAction>,
    ) {
        let mut cx = PanelCx {
            theme,
            vs,
            keys,
            actions: out,
        };
        for dock in [Dock::Top, Dock::Bottom, Dock::Left, Dock::Right] {
            show_dock(&mut self.entries, ui, dock, &mut cx);
        }
        show_center(
            &mut self.entries,
            self.focused_center.as_deref(),
            ui,
            &mut cx,
        );
    }
}

fn show_dock(entries: &mut [Entry], ui: &mut egui::Ui, dock: Dock, cx: &mut PanelCx<'_>) {
    for entry in entries
        .iter_mut()
        .filter(|e| e.visible && e.panel.dock() == dock)
    {
        let id = egui::Id::new(entry.panel.id());
        let mut builder = match dock {
            Dock::Top => egui::Panel::top(id),
            Dock::Bottom => egui::Panel::bottom(id),
            Dock::Left => egui::Panel::left(id),
            Dock::Right => egui::Panel::right(id),
            Dock::Center => continue,
        };
        builder = match entry.panel.sizing() {
            Sizing::Bar => builder
                .resizable(false)
                .exact_size(kit::bar_height(cx.theme))
                .frame(kit::bar_frame(cx.theme)),
            Sizing::Pane { default, min, max } => builder
                .resizable(true)
                .default_size(cx.theme.sp(default))
                .size_range(cx.theme.sp(min)..=cx.theme.sp(max))
                .frame(kit::pane_frame(cx.theme)),
            // A Fill panel docked to an edge has no meaning; treat it as a
            // pane rather than silently dropping the panel.
            Sizing::Fill => builder.resizable(true).frame(kit::pane_frame(cx.theme)),
        };
        builder.show(ui, |ui| entry.panel.show(ui, cx));
    }
}

fn show_center(
    entries: &mut [Entry],
    focused: Option<&str>,
    ui: &mut egui::Ui,
    cx: &mut PanelCx<'_>,
) {
    let centers: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.visible && e.panel.dock() == Dock::Center)
        .map(|(i, _)| i)
        .collect();

    egui::CentralPanel::default()
        .frame(kit::central_frame(cx.theme))
        .show(ui, |ui| {
            let Some(&first) = centers.first() else {
                kit::empty_state(ui, cx.theme, "no view open");
                return;
            };
            let active = centers
                .iter()
                .copied()
                .find(|&i| Some(entries[i].panel.id()) == focused)
                .unwrap_or(first);

            if centers.len() > 1 {
                let tabs: Vec<(&'static str, &'static str, bool)> = centers
                    .iter()
                    .map(|&i| (entries[i].panel.id(), entries[i].panel.title(), i == active))
                    .collect();
                if let Some(picked) = kit::tab_bar(ui, cx.theme, &tabs) {
                    cx.act(UiAction::FocusPanel(picked));
                }
            }
            entries[active].panel.show(ui, cx);
        });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    struct Stub(&'static str, Dock, bool);

    impl Panel for Stub {
        fn id(&self) -> &'static str {
            self.0
        }
        fn title(&self) -> &'static str {
            self.0
        }
        fn dock(&self) -> Dock {
            self.1
        }
        fn closable(&self) -> bool {
            self.2
        }
        fn show(&mut self, _ui: &mut egui::Ui, _cx: &mut PanelCx<'_>) {}
    }

    fn host() -> PanelHost {
        let mut h = PanelHost::new();
        h.register(Stub("bar", Dock::Top, false));
        h.register(Stub("tree", Dock::Left, true));
        h.register(Stub("arrange", Dock::Center, true));
        h
    }

    #[test]
    fn everything_starts_visible() {
        let h = host();
        assert!(h.infos().iter().all(|i| i.visible));
    }

    #[test]
    fn non_closable_panels_refuse_to_hide() {
        let mut h = host();
        assert!(!h.set_visible("bar", false));
        assert!(h.is_visible("bar"));
        assert!(h.set_visible("tree", false));
        assert!(!h.is_visible("tree"));
    }

    #[test]
    fn unknown_ids_are_a_no_op_not_a_panic() {
        let mut h = host();
        assert!(!h.set_visible("ghost", false));
        assert!(!h.toggle("ghost"));
        assert!(!h.focus("ghost"));
        h.apply_hidden(&["ghost".to_owned()]);
        assert!(h.infos().iter().all(|i| i.visible));
    }

    /// The reason prefs store the HIDDEN set: a panel that did not exist when
    /// the prefs were written must come up visible.
    #[test]
    fn hidden_set_round_trips_and_new_panels_default_visible() {
        let mut h = host();
        h.set_visible("tree", false);
        let saved = h.hidden_ids();
        assert_eq!(saved, vec!["tree".to_owned()]);

        let mut later = host();
        later.register(Stub("mixer", Dock::Right, true));
        later.apply_hidden(&saved);
        assert!(!later.is_visible("tree"));
        assert!(later.is_visible("mixer"));
        assert!(later.is_visible("bar"));
    }

    #[test]
    fn focus_only_takes_center_panels_and_unhides_them() {
        let mut h = host();
        h.set_visible("arrange", false);
        assert!(!h.focus("tree"), "a left pane is not a center tab");
        assert!(h.focus("arrange"));
        assert_eq!(h.focused_center(), Some("arrange"));
        assert!(h.is_visible("arrange"), "focusing a hidden view shows it");
    }
}
