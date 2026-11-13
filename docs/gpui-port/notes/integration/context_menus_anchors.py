import sys
A=[
('workbench/mod.rs','\nmod conflicts;\n'),
('workbench/mod.rs','        let body: Option<gpui_kit::AnyElement> = match (view, host, editor) {'),
('workbench/mod.rs','            .child(div().flex_1().min_h_0().children(body))'),
('workbench/mod.rs','                    .children(layout_actions)'),
('workbench/mod.rs','    CloseAllButPinned,\n'),
('workbench/mod.rs','    pub fn tab_menu(&self, pane: &str, tab: &str, cx: &App) -> Vec<MenuItem<TabCommand>> {'),
('workbench/mod.rs','''        if open.tabs.iter().any(|other| other.pinned) {
            items.push(row("Close All but Pinned", TabCommand::CloseAllButPinned, true));
        }'''),
('workbench/mod.rs','''            items.extend([
                MenuItem::action_with("diff.compareWith", path.clone().into()).labelled("Compare With…"),'''),
('workbench/mod.rs','    /// Carries out a row of the menu of the tab `tab` of `pane`.'),
('workbench/mod.rs','        self.with_tabs(window, cx, |tabs, _| match command {'),
('workbench/mod.rs','            TabCommand::CloseAllButPinned => tabs.close_all(&pane, true),'),
('workbench/mod.rs','                                    let items = workbench.tab_menu(&menu_pane, &menu_tab, cx);'),
('workbench/mod.rs','                    let hidden = id.to_owned();'),
('workbench/mod.rs','''                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("hide:{id}")))'''),
('workbench/mod.rs','                    let action = format!("toolwindow.{}", declared.id);'),
('workbench/mod.rs','''                        .on_click(cx.listener(move |_, _, window, cx| {
                            execute_action_from(ActionSource::Toolbar, &action, None, window, cx);
                        }))'''),
('workbench/mod.rs','''                    let opens = cx.subscribe_in(&find, window, |workbench, _, event: &FindPanelEvent, window, cx| {
                        let FindPanelEvent::Open { path, line, col } = event;
                        workbench.open_project_file_at(path.clone(), *line, *col, window, cx);
                    });'''),
('workbench/mod.rs','''                "Split Down",
                // The rows of the file actions that are registered: Copy Absolute Path, Rename File…
                "Copy Path/Reference…",'''),
('workbench/mod.rs','        let menu = cx.new(|cx| PopupMenu::new(items, position, window, cx));'),
('workbench/mod.rs','        self.open_popup(&menu, events, window, cx);'),
('workbench/code_insight.rs','''            ProblemsViewEvent::ShowMenu { position, items } => {
                let target = view.clone();
                let run = move |_: &mut Self, command: ProblemCommand, _: &mut Window, cx: &mut Context<Self>| {
                    target.update(cx, |view, cx| view.run_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);'''),
('workbench/project_files.rs','        let items = self.with_menu_context(cx, |ctx, _| new_menu_items(ctx, Some(&folder), outside_tree));'),
('workbench/plugin_contributions.rs','''    // ---------------------------------------------------------------------------------------------
    // Menus

    /// The plugin group of the Project view's context menu for the selected `paths`.
    pub(super) fn plugin_project_view_items<C: Clone>(
        &self,
        paths: &[String],
        window: &Window,
        cx: &App,
    ) -> Vec<MenuItem<C>> {
        plugin_menu_items(PluginMenu::ProjectView, paths, None, &window_context(window, cx), cx)
    }
'''),
('workbench/plugin_contributions.rs','use crate::platform::context::{ContextKeys, evaluate_when, window_context};'),
('workbench/plugin_contributions.rs','    use crate::platform::context::ContextValue;\n'),
('find/view.rs','''use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};'''),
('find/view.rs','''#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindPanelEvent {
    /// Open `path` with the caret at the 0-based `line` and UTF-16 column `col`.
    Open { path: PathBuf, line: u32, col: u32 },
}'''),
('find/view.rs','''    fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected {
            self.open(index, cx);
        }
    }'''),
('find/view.rs','''                                    .on_click(cx.listener(move |panel, event: &gpui_kit::ClickEvent, _, cx| {
                                        panel.selected = Some(index);
                                        if event.click_count() >= 2 {
                                            panel.open(index, cx);
                                        }
                                        cx.notify();
                                    }))'''),
('workbench/title_bar.rs','''                    .children(right)
                    .children(gear)'''),
('vcs/commit_box.rs','    fn render_bar(&self, cx: &mut Context<Self>) -> Div {'),
('vcs/commit_box.rs','''            .child(div().flex_1())
            .children(
                length.map('''),
('vcs/commit_box.rs','    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {'),
('vcs/commit_box.rs','        let bar = self.render_bar(cx);'),
]
bad=0
for f,t in A:
    n=open(f).read().count(t)
    if n!=1: bad+=1
    print(n, f, t.strip().splitlines()[0][:90])
print('NOT ONE:',bad)
