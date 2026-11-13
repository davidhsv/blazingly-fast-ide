import sys,os
R='/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/'
A=[
# stage 1
('editor/mod.rs','pub mod command_actions;\n'),
('editor/mod.rs','pub mod find_model;\n'),
('editor/mod.rs','pub mod nav_history;\n'),
('editor/actions.rs','    super::command_actions::register(cx);\n'),
('workbench/mod.rs','mod editor_intelligence;\n'),
('workbench/mod.rs','    light_find_bar: Option<TabFindBar>,\n'),
('workbench/mod.rs','            light_find_bar: None,\n'),
('workbench/mod.rs','    project_files::register_actions(registry);\n'),
('workbench/mod.rs','        set_context(window, cx, "findBarHasResults", focused_bar.is_some_and(|bar| bar.read(cx).has_results()));\n'),
('workbench/editor_intelligence.rs','        let intelligence = EditorIntelligence::install(editor, cx);\n'),
('editor/find_bar.rs','''    let Some((line, painted)) = center.zip(editor.painted_lines()) else { return };
    let visible = painted.end.saturating_sub(painted.start);
    let last_top = editor.line_count().saturating_sub(visible.saturating_sub(1).max(1));
    editor.scroll_to_line(line.saturating_sub(visible / 2).min(last_top), cx);
'''),
('workbench/plugin_contributions.rs','''            // The editor's top: where its first painted line would start unscrolled.
            let top = editor
                .position_of(editor.offset_of(first, 0))
                .map(|at| at.y - line_height * first as f32 + editor.scroll_top());
            let bottom = top.map(|top| top + line_height * (painted.end - painted.start).saturating_sub(1) as f32);
'''),
# view.rs
('editor/view.rs','''    /// Counts edits, so that a highlight request an edit has overtaken is dropped.
    generation: u64,
    created: std::time::Instant,
}
'''),
('editor/view.rs','            created: std::time::Instant::now(),\n'),
('editor/view.rs','''    line_height: Pixels,
    first_line: u32,
    lines: Vec<(String, ShapedLine)>,
}
'''),
('editor/view.rs','''    fn metrics(cx: &App) -> (Pixels, Pixels, Pixels) {
        (theme::length(cx, "fs-code"), theme::length(cx, "lh-code"), theme::length(cx, "w-gutter"))
    }
'''),
('editor/view.rs','        (self.scroll_top / Self::metrics(cx).1).floor() as u32\n'),
('editor/view.rs','        self.scroll_top = Self::metrics(cx).1 * line as f32;\n'),
('editor/view.rs','''    /// The lines the last frame shaped: the first one's index and how many.
    pub fn painted_lines(&self) -> Option<Range<u32>> {
        self.layout.as_ref().map(|l| l.first_line..l.first_line + l.lines.len() as u32)
    }
'''),
('editor/view.rs','''    pub fn line_height(&self, cx: &App) -> Pixels {
        Self::metrics(cx).1
    }
'''),
('editor/view.rs','''        let (_, line_height, _) = Self::metrics(cx);
        let line_top = line_height * self.caret_position().0 as f32;
'''),
('editor/view.rs','''        let (_, line_height, _) = Self::metrics(cx);
        let viewport = self.layout.as_ref().map_or(px(0.0), |l| l.bounds.size.height);
        let content = line_height * self.line_count() as f32;
'''),
('editor/view.rs','        (((viewport / Self::metrics(cx).1).floor() as i64) - 1).max(1)\n'),
('editor/view.rs','''        self.put_selections(ranges, main);
        self.history.seal();
'''),
('editor/view.rs','''        self.put_selections(ranges, main);
        self.marked = None;
'''),
('editor/view.rs','''        if self.large {
            self.highlights.clear();
            return;
        }
        let (host, window, doc) = (self.host.clone(), self.window_label.clone(), self.doc);
'''),
('editor/view.rs','''            "left" => self.move_carets(extend, cx, |editor, range| {
                if !extend && !range.is_caret() { range.start() } else { editor.previous_offset(range.head()) }
            }),
            "right" => self.move_carets(extend, cx, |editor, range| {
                if !extend && !range.is_caret() { range.end() } else { editor.next_offset(range.head()) }
            }),
'''),
('editor/view.rs','''        for _ in 0..lines.unsigned_abs() {
            let Some(motion) = step(&state, &goals) else { break };
            state = commands::apply(&state, &motion.edit);
            goals = motion.goals;
            moved = true;
        }
'''),
('editor/view.rs','''                // The whole line with its break, so that typing replaces the line.
                let end = if line + 1 < self.line_count() {
                    buffer::line_range(&self.rope, line + 1).start
                } else {
                    range.end
                };
'''),
('editor/view.rs','''    pub fn position_of(&self, offset: u32) -> Option<Point<Pixels>> {
        let layout = self.layout.as_ref()?;
        let (line, col) = buffer::line_col(&self.rope, offset);
        let (text, shaped) = layout.lines.get(line.checked_sub(layout.first_line)? as usize)?;
        let x = shaped.x_for_index(buffer::byte_of_col(text, col));
        let y = layout.line_height * line as f32 - self.scroll_top;
        Some(point(layout.bounds.left() + layout.text_left + x, layout.bounds.top() + y))
    }
'''),
('editor/view.rs','''        let line = ((y / layout.line_height) as u32).min(self.line_count().saturating_sub(1));
        let x = position.x - layout.bounds.left() - layout.text_left;
        let col = match layout.lines.get(line.checked_sub(layout.first_line)? as usize) {
'''),
('editor/view.rs','''        let line = (y / layout.line_height) as u32;
        (line < self.line_count()).then_some(line)
'''),
('editor/view.rs','''            let ranges = commands::column_mode::rectangle_ranges(&self.edit_state(), start, head);
'''),
('editor/view.rs','''        let painted = line.checked_sub(layout.first_line).and_then(|index| layout.lines.get(index as usize));
'''),
('editor/view.rs','''        let line = (y / layout.line_height) as u32;
        let (text, shaped) = layout.lines.get(line.checked_sub(layout.first_line)? as usize)?;
'''),
('editor/view.rs','''struct Prepared {
    layout: Layout,
    numbers: Vec<ShapedLine>,
    gutter: Pixels,
    /// The marks that touch the painted lines.
    marks: Vec<Mark>,
}
'''),
('editor/view.rs','''        let (font_size, line_height, gutter) = Editor::metrics(cx);
        let code_font = Editor::code_font(cx);
        let plain = theme::color(cx, "fg-editor");
        let number_color = theme::color(cx, "fg-gutter");
        let match_color = theme::color(cx, "fg-search-match");
        let editor = self.editor.read(cx);
        let total = editor.line_count();
        let first_line = ((editor.scroll_top / line_height).floor() as u32).min(total.saturating_sub(1));
        let visible = (bounds.size.height / line_height).ceil() as u32 + 1;
        let last_line = (first_line + visible).min(total);
'''),
('editor/view.rs','''        // Spans are sorted: skip to the first one that can touch the first visible line.
        let first_offset = buffer::line_range(&editor.rope, first_line).start;
'''),
('editor/view.rs','''        let mut span = editor.highlights.partition_point(|s| s.to <= first_offset);
        let mut lines = Vec::with_capacity((last_line - first_line) as usize);
        let mut numbers = Vec::with_capacity(lines.capacity());
        for line in first_line..last_line {
            let range = buffer::line_range(&editor.rope, line);
            let text = buffer::slice(&editor.rope, range.start, range.end);
            let mut runs = Vec::new();
            let mut at = 0usize;
'''),
('editor/view.rs','''                &[run(label.len(), number_color)],
                None,
            ));
        }
        Prepared {
            layout: Layout { bounds, text_left: gutter, line_height, first_line, lines },
            numbers,
            gutter,
            marks,
        }
'''),
('editor/view.rs','                let line_height = Editor::metrics(cx).1;\n'),
('editor/view.rs','        let Prepared { layout, numbers, gutter, marks } = prepared;\n'),
('editor/view.rs','''                let line = layout.first_line + index as u32;
                let top = bounds.top() + line_height * line as f32 - scroll_top;
'''),
('editor/view.rs','                let _ = shaped.paint(text_origin, line_height, TextAlign::Left, None, window, cx);\n'),
('editor/view.rs','            Layout { bounds, text_left: *gutter, line_height, first_line: 0, lines: Vec::new() },\n'),
('editor/view.rs','(layout.bounds.size.height / Editor::metrics(cx).1).ceil() as u32)'),
# stage 2
('editor_lsp/mod.rs','pub mod configuration_navigation;\n'),
('workbench/mod.rs','''pub fn active_editor(window: &Window, cx: &App) -> Option<Entity<Editor>> {
    for_window(window, cx)?.read(cx).editor().cloned()
}
'''),
('workbench/mod.rs','''    /// The dialog a Git operation waits on.
    vcs_dialog: Option<branches::VcsDialog>,
'''),
('workbench/mod.rs','            vcs_dialog: None,\n'),
('workbench/mod.rs','    code_insight::register_actions(registry);\n'),
('workbench/mod.rs','            EntryAction::Own(own) if own.command.starts_with("vcs.deleted.") => {\n'),
('editor/intelligence.rs','    fn show_hint_text(&mut self, text: &str, cx: &mut Context<Self>) {\n'),
('editor/intelligence.rs',"            'codeActionProvider': {'resolveProvider': True},\n"),
('editor/marks.rs','''    /// The line where the debugger is suspended.
    Execution,
'''),
('editor/actions.rs','''    registry.handle("edit.undo", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.undo(cx);
        })
    });
    registry.handle("edit.redo", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.redo(cx);
        })
    });
'''),
('editor/view.rs','''                ("z", false) => Some(self.undo(cx)),
                ("z", true) => Some(self.redo(cx)),
'''),
]
bad=0
for f,a in A:
    p=R+f
    if not os.path.exists(p): print('NOFILE',f); bad+=1; continue
    n=open(p).read().count(a)
    if n!=1: print(n,f,repr(a[:70])); bad+=1
print('anchors',len(A),'bad',bad)
