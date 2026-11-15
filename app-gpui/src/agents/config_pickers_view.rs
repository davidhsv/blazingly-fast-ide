//! The settings chips of a composer, their pickers and the compact Agent settings popup, for any view that
//! shows an agent's settings: a task's composer (the session's settings, sent to the agent) and the New Task
//! page (the draft's settings, kept for the next task).
//!
//! A chip opens its picker above the composer. The Model & effort popup (a model with an effort option) has the
//! effort's steps, a search field over the models (↑/↓ move the highlight, Enter picks it, Escape closes) and the
//! models' descriptions beside their names; it stays open through an effort step and a model pick, as the user
//! often sets both. The mode list carries the Allow All switch, which keeps it open; a mode pick and a plain list
//! close on a choice. The Agent settings button opens every setting but the model and its effort, each with its
//! values listed under its row; a choice keeps the popup open, Escape closes it and gives the keyboard back to
//! the button.
//!
//! The owning view implements [`PickerHost`]: where the settings come from, where a choice goes, where the
//! keyboard returns. Everything else is here.

use crate::agents::config_bar::{
    Chip, Choice, ListKey, OptionList, Picker, SettingsButton, Values, config_bar, effort_slider, effort_step,
    mode_chip, model_chip, select_icon, settings_button, switch_choice, value_of,
};
use crate::agents::config_pickers::{config_picker_options, switch_values};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::tooltip;
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, Keystroke, MouseButton, SharedString, Stateful,
    Subscription, Window, deferred, div, prelude::*, px, relative,
};
use ide_agent::AgentConfigOption;

/// A view that shows an agent's settings as chips.
pub trait PickerHost: Sized + 'static {
    fn pickers(&self) -> &Pickers;
    fn pickers_mut(&mut self) -> &mut Pickers;
    /// The settings shown, with their current values.
    fn picker_config(&self, cx: &App) -> Vec<AgentConfigOption>;
    /// A setting was chosen: `option` takes `value`.
    fn picker_choose(&mut self, option: String, value: String, cx: &mut Context<Self>);
    /// Where the keyboard goes when a picker closes.
    fn picker_return_focus(&self, cx: &App) -> FocusHandle;
    /// The popups open below the owner (at the top of its pane) instead of above it.
    fn picker_opens_down(&self) -> bool {
        false
    }
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// Runs `f` with the UI text of the active language.
fn with_i18n<R>(cx: &App, f: impl FnOnce(&I18n) -> R) -> R {
    let english = I18n::default();
    f(cx.try_global::<Localization>().map_or(&english, |localization| &localization.0))
}

fn find<'a>(config: &'a [AgentConfigOption], id: &str) -> Option<&'a AgentConfigOption> {
    config.iter().find(|o| o.id == id)
}

/// The effort and Allow All options a chip of `option` combines with it, by id.
fn combined(config: &[AgentConfigOption], option: &str) -> (Option<String>, Option<String>) {
    let (mut effort, mut allow_all) = (None, None);
    for picker in config_bar(config).inline {
        match picker {
            Picker::ModelEffort { model, effort: e } if model.id == option => effort = Some(e.id.clone()),
            Picker::ModeAllowAll { mode, allow_all: a } if mode.id == option => allow_all = Some(a.id.clone()),
            _ => {}
        }
    }
    (effort, allow_all)
}

/// A chip of the settings row.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ConfigChip {
    /// The option the chip opens (the model for the model and effort chip, the mode for the mode chip).
    option: String,
    label: String,
    detail: Option<String>,
    icon: Option<&'static str>,
    title: Option<String>,
    /// A switch flips on a click instead of opening a list.
    switch: Option<bool>,
}

impl ConfigChip {
    fn of(option: &str, chip: Chip) -> Self {
        Self {
            option: option.to_owned(),
            label: chip.value,
            detail: chip.detail,
            icon: chip.icon,
            title: Some(chip.title),
            switch: None,
        }
    }
}

fn config_chips(config: &[AgentConfigOption], i18n: &I18n) -> Vec<ConfigChip> {
    let values = Values::new();
    config_bar(config)
        .inline
        .into_iter()
        .map(|picker| match picker {
            Picker::ModelEffort { model, effort } => {
                ConfigChip::of(&model.id, model_chip(model, effort, &values, i18n))
            }
            Picker::ModeAllowAll { mode, allow_all } => {
                ConfigChip::of(&mode.id, mode_chip(mode, allow_all, &values, i18n))
            }
            Picker::Switch(option) => {
                let on = switch_values(option).is_some_and(|(on, _)| value_of(option, &values) == Some(on));
                ConfigChip {
                    option: option.id.clone(),
                    label: i18n.tx(&option.name),
                    detail: None,
                    icon: None,
                    title: None,
                    switch: Some(on),
                }
            }
            Picker::Select(option) => {
                let current = value_of(option, &values);
                let label = option
                    .values
                    .iter()
                    .find(|v| Some(v.value.as_str()) == current)
                    .map_or_else(|| i18n.tx("Default"), |v| v.name.clone());
                ConfigChip {
                    option: option.id.clone(),
                    label,
                    detail: None,
                    icon: current.and_then(|v| select_icon(option, v)),
                    title: Some(i18n.tx(&option.name)),
                    switch: None,
                }
            }
        })
        .collect()
}

/// The settings chips as text: `<label>` with ` · <detail>`, ` [on]` / ` [off]` for a switch.
pub fn chip_texts(config: &[AgentConfigOption], i18n: &I18n) -> Vec<String> {
    config_chips(config, i18n)
        .into_iter()
        .map(|chip| {
            let mut text = chip.label;
            if let Some(detail) = chip.detail {
                text.push_str(&format!(" · {detail}"));
            }
            if let Some(on) = chip.switch {
                text.push_str(if on { " [on]" } else { " [off]" });
            }
            text
        })
        .collect()
}

/// The open picker of a chip: the list of its option, and the extra rows of a combined chip.
struct OpenPicker {
    chip: usize,
    /// The option as it was when the list was built: a different one rebuilds the list.
    option: AgentConfigOption,
    list: OptionList,
    /// The effort option shown as level steps above the model list.
    effort: Option<String>,
    /// The Allow All switch under the mode list.
    allow_all: Option<String>,
}

impl OpenPicker {
    /// The Model & effort popup stays open through its choices; other lists close on a choice.
    fn stays_open(&self) -> bool {
        self.effort.is_some()
    }

    /// The model list has a search field.
    fn searchable(&self) -> bool {
        self.option.category.as_deref() == Some("model")
    }
}

/// The open Agent settings popup.
struct OpenSettings {
    /// The setting whose values are listed under its row.
    expanded: Option<String>,
}

/// The state of a view's pickers and Agent settings popup.
pub struct Pickers {
    open: Option<OpenPicker>,
    /// The keyboard of a list without a search field.
    list_focus: FocusHandle,
    /// The search field of the model list, emptied each time a list opens.
    search: Entity<TextField>,
    settings: Option<OpenSettings>,
    /// The Agent settings button, which gets the keyboard back when its popup closes.
    settings_focus: FocusHandle,
    settings_popup_focus: FocusHandle,
    /// The keys of the list, the search field and the Agent settings popup, taken ahead of the IDE's
    /// shortcuts (Escape closes the popup instead of leaving the tool window).
    claims: [u64; 3],
    _search_events: Subscription,
}

impl Pickers {
    pub fn new<V: PickerHost>(cx: &mut Context<V>) -> Self {
        let placeholder = tx(cx, "Search…");
        let search = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let search_events = cx.subscribe(&search, |view: &mut V, field, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed {
                let query = field.read(cx).text().to_owned();
                if let Some(open) = &mut view.pickers_mut().open {
                    open.list.set_query(&query);
                    cx.notify();
                }
            }
        });
        let (list_focus, settings_popup_focus) = (cx.focus_handle(), cx.focus_handle());
        let weak = cx.weak_entity();
        let list_claim = input::claim_keys(
            list_focus.clone(),
            move |keystroke, window, cx| {
                weak.update(cx, |view, cx| picker_key(view, keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let weak = cx.weak_entity();
        let search_claim = input::claim_keys(
            search.read(cx).focus_handle(cx),
            move |keystroke, window, cx| {
                weak.update(cx, |view, cx| picker_key(view, keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let weak = cx.weak_entity();
        let settings_claim = input::claim_keys(
            settings_popup_focus.clone(),
            move |keystroke, window, cx| {
                weak.update(cx, |view, cx| settings_key(view, keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        Self {
            open: None,
            list_focus,
            search,
            settings: None,
            settings_focus: cx.focus_handle(),
            settings_popup_focus,
            claims: [list_claim, search_claim, settings_claim],
            _search_events: search_events,
        }
    }

    /// Releases the key claims; the owner calls it when it is released.
    pub fn release(&self, cx: &mut App) {
        for claim in self.claims {
            input::release_keys(claim, cx);
        }
    }

    /// A picker or the Agent settings popup is open.
    pub fn is_open(&self) -> bool {
        self.open.is_some() || self.settings.is_some()
    }

    /// The entries of the open picker, as `<label>` with ` *` on the highlighted one.
    pub fn entries(&self) -> Option<Vec<String>> {
        let picker = self.open.as_ref()?;
        let active = picker.list.active();
        Some(
            picker
                .list
                .filtered()
                .iter()
                .enumerate()
                .map(|(i, entry)| if active == Some(i) { format!("{} *", entry.label) } else { entry.label.clone() })
                .collect(),
        )
    }

    /// The description drawn beside each entry of the open picker, in the order of [`Self::entries`].
    pub fn details(&self) -> Option<Vec<Option<String>>> {
        let picker = self.open.as_ref()?;
        Some(picker.list.filtered().iter().map(|entry| entry.detail.clone()).collect())
    }

    /// The text of the open picker's search field, when it has one.
    pub fn query(&self, cx: &App) -> Option<String> {
        let picker = self.open.as_ref()?;
        picker.searchable().then(|| self.search.read(cx).text().to_owned())
    }

    /// The Agent settings button has the keyboard.
    pub fn settings_button_focused(&self, window: &Window) -> bool {
        self.settings_focus.is_focused(window)
    }
}

/// An entry of the open picker, as it is drawn.
struct EntryRow {
    value: String,
    label: String,
    detail: Option<String>,
    icon: Option<&'static str>,
    section: bool,
    highlighted: bool,
    /// The setting's value now: drawn with a check.
    current: bool,
}

/// A row of the Agent settings popup.
struct SettingRow {
    option: String,
    label: String,
    /// The name of the value, `Default` without one.
    value: String,
    icon: Option<&'static str>,
    /// A switch, and whether it is on.
    switch: Option<bool>,
    /// The values listed under the row while it is expanded: the value, its name, and whether it is the current one.
    values: Option<Vec<(String, String, bool)>>,
}

// ----- what keys and clicks do -----

/// A chip was clicked: a switch flips, a list opens (or closes when it is the open one).
pub fn chip_clicked<V: PickerHost>(view: &mut V, index: usize, window: &mut Window, cx: &mut Context<V>) {
    if view.pickers().open.as_ref().is_some_and(|p| p.chip == index) {
        close_picker(view, window, cx);
        return;
    }
    view.pickers_mut().settings = None;
    let config = view.picker_config(cx);
    let chip = with_i18n(cx, |i18n| config_chips(&config, i18n)).into_iter().nth(index);
    let Some(chip) = chip else { return };
    let Some(option) = find(&config, &chip.option).cloned() else { return };
    if let Some(on) = chip.switch {
        if let Some((option, value)) = switch_choice(&option, !on) {
            view.picker_choose(option, value, cx);
        }
        return;
    }
    let (effort, allow_all) = combined(&config, &option.id);
    let list = OptionList::new(&option, &Values::new());
    let picker = OpenPicker { chip: index, option, list, effort, allow_all };
    let searchable = picker.searchable();
    let pickers = view.pickers_mut();
    pickers.open = Some(picker);
    if searchable {
        pickers.search.update(cx, |field, cx| field.set_text("", cx));
        let focus = pickers.search.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    } else {
        window.focus(&pickers.list_focus, cx);
    }
    cx.notify();
}

/// Closes the open picker and gives the keyboard back to the owner.
fn close_picker<V: PickerHost>(view: &mut V, window: &mut Window, cx: &mut Context<V>) {
    if view.pickers_mut().open.take().is_some() {
        let focus = view.picker_return_focus(cx);
        window.focus(&focus, cx);
    }
    cx.notify();
}

/// Closes the Agent settings popup and gives the keyboard back to its button.
fn close_settings<V: PickerHost>(view: &mut V, window: &mut Window, cx: &mut Context<V>) {
    let pickers = view.pickers_mut();
    if pickers.settings.take().is_some() {
        window.focus(&pickers.settings_focus, cx);
    }
    cx.notify();
}

/// Escape in the owner's text field: closes whichever popup is open.
pub fn close_popups<V: PickerHost>(view: &mut V, window: &mut Window, cx: &mut Context<V>) {
    if view.pickers().settings.is_some() {
        close_settings(view, window, cx);
    } else {
        close_picker(view, window, cx);
    }
}

/// The settings changed: an open list whose option changed is rebuilt (keeping the search), so its highlight
/// follows the new value. A list whose option is gone, or whose chip is now of another kind (a model that has no
/// effort levels, a mode without Allow All), closes.
pub fn refresh<V: PickerHost>(view: &mut V, cx: &mut Context<V>) {
    let Some(id) = view.pickers().open.as_ref().map(|p| p.option.id.clone()) else { return };
    let config = view.picker_config(cx);
    let fresh = find(&config, &id).cloned();
    let parts = combined(&config, &id);
    let pickers = view.pickers_mut();
    let query = pickers.search.read(cx).text().to_owned();
    let Some(open) = &mut pickers.open else { return };
    match fresh {
        Some(_) if (open.effort.clone(), open.allow_all.clone()) != parts => pickers.open = None,
        Some(option) if option != open.option => {
            let mut list = OptionList::new(&option, &Values::new());
            if open.searchable() && !query.is_empty() {
                list.set_query(&query);
            }
            open.list = list;
            open.option = option;
        }
        Some(_) => {}
        None => pickers.open = None,
    }
}

/// A choice of the open picker: applied, and the picker closes unless it is the Model & effort popup, whose
/// search field keeps the keyboard.
fn picker_chose<V: PickerHost>(view: &mut V, choice: Option<Choice>, window: &mut Window, cx: &mut Context<V>) {
    let Some((option, value)) = choice else { return };
    let open = view.pickers().open.as_ref();
    let (stays, searchable) = open.map_or((false, false), |p| (p.stays_open(), p.searchable()));
    if !stays {
        close_picker(view, window, cx);
    } else if searchable {
        let focus = view.pickers().search.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }
    view.picker_choose(option, value, cx);
    cx.notify();
}

fn entry_clicked<V: PickerHost>(view: &mut V, value: &str, window: &mut Window, cx: &mut Context<V>) {
    let choice = view.pickers().open.as_ref().and_then(|p| p.list.click(value));
    picker_chose(view, choice, window, cx);
}

/// The keys of the open list (and of its search field, ahead of the field): ↑/↓ move the highlight, Enter
/// picks it, Escape closes the list. Returns whether the key was the list's.
fn picker_key<V: PickerHost>(view: &mut V, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<V>) -> bool {
    if view.pickers().open.is_none() || keystroke.modifiers.modified() {
        return false;
    }
    let key = match keystroke.key.as_str() {
        "up" => ListKey::Up,
        "down" => ListKey::Down,
        "enter" => ListKey::Enter,
        "escape" => {
            close_picker(view, window, cx);
            return true;
        }
        _ => return false,
    };
    let choice = view.pickers_mut().open.as_mut().and_then(|p| p.list.key(key));
    picker_chose(view, choice, window, cx);
    cx.notify();
    true
}

fn settings_clicked<V: PickerHost>(view: &mut V, window: &mut Window, cx: &mut Context<V>) {
    if view.pickers().settings.is_some() {
        close_settings(view, window, cx);
        return;
    }
    let pickers = view.pickers_mut();
    pickers.open = None;
    pickers.settings = Some(OpenSettings { expanded: None });
    window.focus(&pickers.settings_popup_focus, cx);
    cx.notify();
}

/// A setting's row: a switch flips, a list shows (or hides) its values under the row.
fn setting_clicked<V: PickerHost>(view: &mut V, option: &str, cx: &mut Context<V>) {
    let config = view.picker_config(cx);
    let Some(found) = find(&config, option) else { return };
    let values = Values::new();
    if let Some(on) = switch_values(found).map(|(on, _)| value_of(found, &values) == Some(on)) {
        if let Some((option, value)) = switch_choice(found, !on) {
            view.picker_choose(option, value, cx);
        }
        return;
    }
    if let Some(settings) = &mut view.pickers_mut().settings {
        let expanded = settings.expanded.as_deref() == Some(option);
        settings.expanded = (!expanded).then(|| option.to_owned());
    }
    cx.notify();
}

/// A value listed under a setting's row: applied; the list folds and the popup stays open.
fn setting_chosen<V: PickerHost>(view: &mut V, option: String, value: String, cx: &mut Context<V>) {
    if let Some(settings) = &mut view.pickers_mut().settings {
        settings.expanded = None;
    }
    view.picker_choose(option, value, cx);
    cx.notify();
}

/// Escape in the Agent settings popup folds the listed values, else closes the popup. Returns whether the key
/// was the popup's.
fn settings_key<V: PickerHost>(view: &mut V, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<V>) -> bool {
    if keystroke.key != "escape" || keystroke.modifiers.modified() {
        return false;
    }
    let Some(settings) = &mut view.pickers_mut().settings else { return false };
    if settings.expanded.take().is_none() {
        close_settings(view, window, cx);
    }
    cx.notify();
    true
}

// ----- what is drawn -----

fn chip_button<V: 'static>(
    selector: String,
    cx: &mut Context<V>,
    on_click: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Stateful<Div> {
    let id = selector.clone();
    div()
        .id(SharedString::from(id))
        .debug_selector(move || selector)
        .h(theme::length(cx, "h-control-small"))
        .px(px(6.0))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.0))
        .rounded(theme::length(cx, "radius-control"))
        .text_color(theme::color(cx, "fg-muted"))
        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
        .on_click(cx.listener(move |view, _, window, cx| on_click(view, window, cx)))
}

/// The chips of the settings row (`composer-config-N`), then the Agent settings button when the agent has
/// settings besides the model and its effort.
pub fn chip_elements<V: PickerHost>(view: &V, cx: &mut Context<V>) -> Vec<AnyElement> {
    let config = view.picker_config(cx);
    let chips = with_i18n(cx, |i18n| config_chips(&config, i18n));
    let muted = theme::color(cx, "fg-muted");
    let faint = theme::color(cx, "fg-faint");
    let mut out: Vec<AnyElement> = Vec::with_capacity(chips.len() + 1);
    for (index, chip) in chips.into_iter().enumerate() {
        let selector = format!("composer-config-{index}");
        let mut button =
            chip_button(selector, cx, move |view: &mut V, window, cx| chip_clicked(view, index, window, cx));
        if let Some(title) = chip.title {
            button = button.tooltip(tooltip(title));
        }
        if let Some(icon) = chip.icon {
            button = button.child(theme::icon(cx, icon, 13.0, muted));
        }
        button = button.child(SharedString::from(chip.label));
        if let Some(detail) = chip.detail {
            button = button.child(div().text_color(faint).child(SharedString::from(detail)));
        }
        let mark = match chip.switch {
            Some(true) => "check",
            Some(false) => "minus",
            None => "chevron-down",
        };
        out.push(button.child(theme::icon(cx, mark, 12.0, muted)).into_any_element());
    }
    out.extend(settings_button_element(view, &config, cx));
    out
}

fn settings_summary(config: &[AgentConfigOption], cx: &App) -> Option<SettingsButton> {
    let bar = config_bar(config);
    if bar.secondary.is_empty() {
        return None;
    }
    Some(with_i18n(cx, |i18n| settings_button(&bar.secondary, &Values::new(), i18n)))
}

/// The tooltip of the Agent settings button, when the agent has settings besides the model and its effort.
pub fn settings_title<V: PickerHost>(view: &V, cx: &App) -> Option<String> {
    settings_summary(&view.picker_config(cx), cx).map(|button| button.title)
}

fn settings_button_element<V: PickerHost>(
    view: &V,
    config: &[AgentConfigOption],
    cx: &mut Context<V>,
) -> Option<AnyElement> {
    let summary = settings_summary(config, cx)?;
    let muted = theme::color(cx, "fg-muted");
    let pickers = view.pickers();
    let button = chip_button("composer-settings".to_owned(), cx, |view: &mut V, window, cx| {
        settings_clicked(view, window, cx);
    })
    .track_focus(&pickers.settings_focus)
    .when(pickers.settings.is_some(), |el| el.bg(theme::color(cx, "bg-selected")))
    .tooltip(tooltip(summary.title))
    .child(theme::icon(cx, summary.icon, 13.0, muted));
    Some(button.into_any_element())
}

fn setting_rows<V: PickerHost>(view: &V, cx: &App) -> Vec<SettingRow> {
    let config = view.picker_config(cx);
    let expanded = view.pickers().settings.as_ref().and_then(|s| s.expanded.as_deref());
    let values = Values::new();
    with_i18n(cx, |i18n| {
        config_bar(&config)
            .secondary
            .into_iter()
            .filter_map(|picker| match picker {
                Picker::Switch(o) | Picker::Select(o) => Some(o),
                _ => None,
            })
            .map(|o| {
                let current = value_of(o, &values);
                let switch = switch_values(o).map(|(on, _)| current == Some(on));
                let value = o
                    .values
                    .iter()
                    .find(|v| Some(v.value.as_str()) == current)
                    .map_or_else(|| i18n.tx("Default"), |v| v.name.clone());
                let listed = (switch.is_none() && expanded == Some(o.id.as_str())).then(|| {
                    config_picker_options(o)
                        .into_iter()
                        .filter(|e| !e.section)
                        .map(|e| {
                            let is_current = current == Some(e.value.as_str());
                            (e.value, e.label, is_current)
                        })
                        .collect()
                });
                SettingRow {
                    option: o.id.clone(),
                    label: i18n.tx(&o.name),
                    value,
                    icon: current.and_then(|v| select_icon(o, v)),
                    switch,
                    values: listed,
                }
            })
            .collect()
    })
}

/// The rows of the open Agent settings popup: `<name>: <value>`, a switch as `<name> [on]` / `<name> [off]`, then
/// the values listed under the expanded one, as `  <name>` with ` *` on the current one.
pub fn settings_rows<V: PickerHost>(view: &V, cx: &App) -> Option<Vec<String>> {
    view.pickers().settings.as_ref()?;
    let mut out = Vec::new();
    for row in setting_rows(view, cx) {
        out.push(match row.switch {
            Some(on) => format!("{} [{}]", row.label, if on { "on" } else { "off" }),
            None => format!("{}: {}", row.label, row.value),
        });
        for (_, name, current) in row.values.unwrap_or_default() {
            out.push(if current { format!("  {name} *") } else { format!("  {name}") });
        }
    }
    Some(out)
}

/// The open picker, drawn above the owner's bottom edge.
pub fn picker_element<V: PickerHost>(view: &V, cx: &mut Context<V>) -> Option<AnyElement> {
    let picker = view.pickers().open.as_ref()?;
    let config = view.picker_config(cx);
    let values = Values::new();
    let effort = picker.effort.as_deref().and_then(|id| find(&config, id)).cloned();
    let allow_all = picker.allow_all.as_deref().and_then(|id| find(&config, id)).cloned();
    let fresh = find(&config, &picker.option.id).unwrap_or(&picker.option);
    let current = value_of(fresh, &values).map(str::to_owned);
    let active = picker.list.active();
    let entries: Vec<EntryRow> = picker
        .list
        .filtered()
        .iter()
        .enumerate()
        .map(|(i, entry)| EntryRow {
            value: entry.value.clone(),
            label: entry.label.clone(),
            detail: entry.detail.clone(),
            icon: select_icon(&picker.option, &entry.value),
            section: entry.section,
            highlighted: active == Some(i),
            current: current.as_deref() == Some(entry.value.as_str()),
        })
        .collect();
    let searchable = picker.searchable();
    let muted = theme::color(cx, "fg-muted");
    let faint = theme::color(cx, "fg-faint");
    let border = theme::color(cx, "border");

    let mut top: Vec<AnyElement> = Vec::new();
    if let Some(effort) = effort {
        let slider = effort_slider(&effort, &values);
        let mut steps = Vec::with_capacity(effort.values.len());
        for (index, level) in effort.values.iter().enumerate() {
            let choice = effort_step(&effort, &values, index);
            let pressed = index == slider.index;
            steps.push(
                chip_button(format!("composer-effort-{index}"), cx, move |view: &mut V, window, cx| {
                    picker_chose(view, choice.clone(), window, cx);
                })
                .when(pressed, |el| el.bg(theme::color(cx, "bg-selected")))
                .child(SharedString::from(level.name.clone())),
            );
        }
        let header = div()
            .flex()
            .justify_between()
            .px(px(6.0))
            .child(div().text_color(muted).child(SharedString::from(tx(cx, &effort.name))))
            .child(
                div()
                    .debug_selector(|| "composer-effort-value".to_owned())
                    .child(SharedString::from(slider.value_text.unwrap_or_default())),
            );
        top.push(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .pb(px(4.0))
                .border_b_1()
                .border_color(border)
                .child(header)
                .child(div().flex().gap(px(2.0)).px(px(4.0)).children(steps))
                .into_any_element(),
        );
    }
    if searchable {
        top.push(
            div()
                .debug_selector(|| "composer-picker-search".to_owned())
                .flex_none()
                .my(px(4.0))
                .px(px(6.0))
                .h(theme::length(cx, "h-control"))
                .flex()
                .items_center()
                .rounded(theme::length(cx, "radius-control"))
                .border_1()
                .border_color(border)
                .bg(theme::color(cx, "bg-field"))
                .child(view.pickers().search.clone())
                .into_any_element(),
        );
    }

    let mut rows: Vec<AnyElement> = Vec::with_capacity(entries.len());
    for (index, row) in entries.into_iter().enumerate() {
        if row.section {
            let title = SharedString::from(row.label);
            rows.push(div().flex_none().px(px(8.0)).pt(px(4.0)).text_color(muted).child(title).into_any_element());
            continue;
        }
        let selector = format!("composer-picker-{index}");
        let id = selector.clone();
        let value = row.value;
        let detail = row
            .detail
            .map(|detail| div().flex_1().min_w_0().truncate().text_color(faint).child(SharedString::from(detail)));
        let check =
            div().flex_none().w(px(12.0)).when(row.current, |el| el.child(theme::icon(cx, "check", 12.0, muted)));
        rows.push(
            div()
                .id(SharedString::from(id))
                .debug_selector(move || selector)
                .flex_none()
                .px(px(6.0))
                .h(theme::length(cx, "h-row"))
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded(theme::length(cx, "radius-small"))
                .when(row.highlighted, |el| el.bg(theme::color(cx, "bg-list-selected")))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .child(check)
                .children(row.icon.map(|icon| theme::icon(cx, icon, 14.0, muted)))
                .child(div().flex_none().child(SharedString::from(row.label)))
                .children(detail)
                .on_click(cx.listener(move |view: &mut V, _, window, cx| entry_clicked(view, &value, window, cx)))
                .into_any_element(),
        );
    }
    let list = div().id("composer-picker-list").flex().flex_col().min_h_0().overflow_y_scroll().children(rows);

    let footer = allow_all.map(|allow_all| {
        let on = switch_values(&allow_all).is_some_and(|(on, _)| value_of(&allow_all, &values) == Some(on));
        let choice = switch_choice(&allow_all, !on);
        let label = format!("{}: {}", tx(cx, &allow_all.name), tx(cx, if on { "On" } else { "Off" }));
        let switch = chip_button("composer-allow-all".to_owned(), cx, move |view: &mut V, _, cx| {
            if let Some((option, value)) = choice.clone() {
                view.picker_choose(option, value, cx);
            }
        })
        .child(SharedString::from(label))
        .child(theme::icon(cx, if on { "check" } else { "minus" }, 12.0, muted));
        div().flex_none().pt(px(4.0)).border_t_1().border_color(border).child(switch)
    });

    let popup = div()
        .debug_selector(|| "composer-picker".to_owned())
        .when(!searchable, |el| el.track_focus(&view.pickers().list_focus))
        .absolute()
        .map(|el| if view.picker_opens_down() { el.top(relative(1.0)) } else { el.bottom(px(40.0)) })
        .left(px(8.0))
        .w(px(300.0))
        .max_h(px(360.0))
        .p(px(4.0))
        .flex()
        .flex_col()
        .rounded(theme::length(cx, "radius-popup"))
        .border_1()
        .border_color(border)
        .bg(theme::color(cx, "bg-popup"))
        .shadow(theme::shadow(cx, "shadow-popup"))
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .children(top)
        .child(list)
        .children(footer);
    Some(deferred(popup).with_priority(1).into_any_element())
}

/// The open Agent settings popup, drawn above the owner's bottom edge.
pub fn settings_element<V: PickerHost>(view: &V, cx: &mut Context<V>) -> Option<AnyElement> {
    view.pickers().settings.as_ref()?;
    let rows = setting_rows(view, cx);
    let muted = theme::color(cx, "fg-muted");
    let title = tx(cx, "Agent settings");
    let heading = div().px(px(6.0)).py(px(2.0)).text_color(muted).child(SharedString::from(title));
    let mut children: Vec<AnyElement> = vec![heading.into_any_element()];
    for (index, row) in rows.into_iter().enumerate() {
        let SettingRow { option, label, value, icon, switch, values } = row;
        let clicked = option.clone();
        let selector = format!("composer-setting-{index}");
        let mut button = chip_button(selector, cx, move |view: &mut V, _, cx| setting_clicked(view, &clicked, cx));
        match switch {
            Some(on) => button = button.child(theme::icon(cx, if on { "check" } else { "minus" }, 12.0, muted)),
            None => {
                if let Some(icon) = icon {
                    button = button.child(theme::icon(cx, icon, 13.0, muted));
                }
                button = button.child(SharedString::from(value)).child(theme::icon(cx, "chevron-down", 12.0, muted));
            }
        }
        children.push(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .px(px(4.0))
                .child(div().text_color(muted).child(SharedString::from(label)))
                .child(button)
                .into_any_element(),
        );
        for (position, (value, name, current)) in values.into_iter().flatten().enumerate() {
            let option = option.clone();
            let selector = format!("composer-setting-{index}-{position}");
            let entry = chip_button(selector, cx, move |view: &mut V, _, cx| {
                setting_chosen(view, option.clone(), value.clone(), cx);
            })
            .ml(px(16.0))
            .child(div().w(px(12.0)).when(current, |el| el.child(theme::icon(cx, "check", 12.0, muted))))
            .child(SharedString::from(name));
            children.push(entry.into_any_element());
        }
    }
    let popup = div()
        .debug_selector(|| "composer-settings-popup".to_owned())
        .track_focus(&view.pickers().settings_popup_focus)
        .absolute()
        .map(|el| if view.picker_opens_down() { el.top(relative(1.0)) } else { el.bottom(px(40.0)) })
        .left(px(8.0))
        .w(px(280.0))
        .p(px(4.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .rounded(theme::length(cx, "radius-popup"))
        .border_1()
        .border_color(theme::color(cx, "border"))
        .bg(theme::color(cx, "bg-popup"))
        .shadow(theme::shadow(cx, "shadow-popup"))
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .children(children);
    Some(deferred(popup).with_priority(1).into_any_element())
}
