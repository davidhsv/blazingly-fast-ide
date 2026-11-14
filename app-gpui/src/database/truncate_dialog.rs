//! Truncate: asks before deleting every row of a table, with PostgreSQL's Restart identity and
//! Cascade, and previews the statement [`truncate_sql`] writes for the dialect.

use super::dialog_parts::{button, checkbox, dialog_body, dialog_footer, dialog_frame, sql_preview, tx};
use super::objects::{TruncateOptions, truncate_sql};
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render, SharedString,
    Window, div, prelude::*,
};
use ide_db::DbDriver;

/// What the dialog asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TruncateEvent {
    /// OK: run the statement (`DbStore::run_sql`); answer with [`TruncateDialog::finished`].
    Apply(String),
    Cancelled,
}

pub struct TruncateDialog {
    schema: String,
    table: String,
    driver: DbDriver,
    options: TruncateOptions,
    busy: bool,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<TruncateEvent> for TruncateDialog {}

impl TruncateDialog {
    pub fn new(schema: &str, table: &str, driver: DbDriver, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self {
            schema: schema.to_owned(),
            table: table.to_owned(),
            driver,
            options: TruncateOptions::default(),
            busy: false,
            focus,
            key_claim,
        }
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    /// The statement OK runs.
    pub fn sql(&self) -> String {
        truncate_sql(self.driver, &self.schema, &self.table, self.options)
    }

    /// Restart identity and Cascade are PostgreSQL's.
    pub fn offers_options(&self) -> bool {
        self.driver == DbDriver::Postgres
    }

    pub fn toggle_restart_identity(&mut self, cx: &mut Context<Self>) {
        if self.offers_options() {
            self.options.restart_identity = !self.options.restart_identity;
            cx.notify();
        }
    }

    pub fn toggle_cascade(&mut self, cx: &mut Context<Self>) {
        if self.offers_options() {
            self.options.cascade = !self.options.cascade;
            cx.notify();
        }
    }

    pub fn press_ok(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.emit(TruncateEvent::Apply(self.sql()));
        cx.notify();
    }

    /// The statement ran (`ok`) or failed, the error told by a notification.
    pub fn finished(&mut self, ok: bool, cx: &mut Context<Self>) {
        self.busy = false;
        if ok {
            cx.emit(TruncateEvent::Cancelled);
        }
        cx.notify();
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.press_ok(cx),
            "escape" => cx.emit(TruncateEvent::Cancelled),
            _ => return false,
        }
        true
    }
}

impl Focusable for TruncateDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TruncateDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lead = tx(cx, "Delete all rows of");
        let lead = div().flex().gap(gpui_kit::px(4.0)).child(SharedString::from(lead)).child(
            div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(format!("{}?", self.table))),
        );
        let mut body = dialog_body().child(lead);
        if self.offers_options() {
            let (restart, cascade) = (tx(cx, "Restart identity"), tx(cx, "Cascade"));
            body = body
                .child(
                    checkbox(cx, restart, self.options.restart_identity, true)
                        .id("db-truncate-restart")
                        .debug_selector(|| "db-truncate-restart".to_owned())
                        .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_restart_identity(cx))),
                )
                .child(
                    checkbox(cx, cascade, self.options.cascade, true)
                        .id("db-truncate-cascade")
                        .debug_selector(|| "db-truncate-cascade".to_owned())
                        .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_cascade(cx))),
                );
        }
        let sql = self.sql();
        body = body.child(sql_preview(cx, "Preview", &sql));
        let (ok_label, cancel_label) = (tx(cx, "OK"), tx(cx, "Cancel"));
        let ok = button(cx, ok_label, true, !self.busy)
            .id("db-truncate-ok")
            .debug_selector(|| "db-truncate-ok".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.press_ok(cx)));
        let cancel = button(cx, cancel_label, false, true)
            .id("db-truncate-cancel")
            .debug_selector(|| "db-truncate-cancel".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(TruncateEvent::Cancelled)));
        dialog_frame(cx, "Truncate", 520.0)
            .track_focus(&self.focus)
            .child(body)
            .child(dialog_footer().child(cancel).child(ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<TruncateEvent>>>;

    fn open(cx: &mut TestAppContext, driver: DbDriver) -> (Entity<TruncateDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| TruncateDialog::new("public", "users", driver, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &TruncateEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            window.focus(&view.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    async fn postgres_offers_restart_identity_and_cascade_in_the_statement(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, DbDriver::Postgres);
        click(cx, "db-truncate-restart");
        click(cx, "db-truncate-cascade");
        let sql = view.read_with(cx, |v, _| v.sql());
        assert_eq!(sql, "truncate table public.users restart identity cascade;");
        click(cx, "db-truncate-ok");
        assert_eq!(*events.borrow(), [TruncateEvent::Apply(sql)]);
    }

    #[gpui_kit::test]
    async fn sqlite_deletes_the_rows_and_escape_cancels(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, DbDriver::Sqlite);
        assert!(!view.read_with(cx, |v, _| v.offers_options()));
        assert_eq!(view.read_with(cx, |v, _| v.sql()), "delete from public.users;");
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [TruncateEvent::Cancelled]);
    }

    #[gpui_kit::test]
    async fn enter_runs_it_once_until_it_finished(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, DbDriver::Mysql);
        cx.simulate_keystrokes("enter");
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().len(), 1);
        view.update(cx, |v, cx| v.finished(true, cx));
        assert_eq!(events.borrow().last(), Some(&TruncateEvent::Cancelled));
    }
}
