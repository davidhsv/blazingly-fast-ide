//! PHP › CLI Interpreter's Version and Xdebug rows, and Share JDT LS across builds in a workspace,
//! each drawn in a window over a dialog model. The questions the PHP rows ask are kept and
//! answered when the test says, through the toolchain answers the test gives: what the rows show
//! while a check is under way is read before it answers.

use super::*;
use crate::settings_model::staging::SettingsDialog;
use crate::settings_view::pages::languages_sdk::{SdkDetection, answer};
use std::cell::{Cell, RefCell};

const PAGE: &str = "languages.php";
const PHP83: &str = "/opt/php83/bin/php";
const PHP81: &str = "/opt/php81/bin/php";

/// The questions a view asked, not answered yet.
type Asks = Rc<RefCell<Vec<LanguageAsk>>>;

fn php(path: &str, version: &str) -> Toolchain {
    Toolchain {
        path: path.to_owned(),
        name: format!("PHP {version}"),
        version: Some(version.to_owned()),
        source: "system".to_owned(),
        ..Toolchain::default()
    }
}

/// Two PHPs found, the 8.1 one first on the PATH; Xdebug 3.4.7 in 8.3, none in 8.1, and the check
/// of any other PHP fails.
fn detection() -> SdkDetection {
    SdkDetection {
        found: vec![(ToolchainKind::Php, vec![php(PHP83, "8.3.14"), php(PHP81, "8.1.33")])],
        located: vec![("php", PHP81.to_owned())],
        xdebug: vec![(PHP83.to_owned(), Some("3.4.7".to_owned())), (PHP81.to_owned(), None)],
        ..SdkDetection::default()
    }
}

/// The PHP rows of the page over `stored`, with the installations of `test` detected; the
/// questions the rows ask from their next drawing on are kept in the returned list.
fn php_rows<'a>(
    cx: &'a mut TestAppContext,
    stored: &[(&str, Value)],
    test: SdkDetection,
) -> (Entity<PhpInterpreterView>, Entity<SettingsDialog>, Asks, &'a mut VisualTestContext) {
    let model = cx.update(|cx| dialog_model(PAGE, stored, cx));
    let toolchains = cx.update(|cx| {
        cx.set_global(test);
        let toolchains = Toolchains::new(&model, "w1", cx);
        toolchains.detect(ToolchainKind::Php, false, cx);
        toolchains
    });
    let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
    let (view, cx) = open(cx, move |_, cx| PhpInterpreterView::new(binding, toolchains, cx));
    let asks: Asks = Rc::default();
    let kept = asks.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &LanguageEvent, _| {
            if let LanguageEvent::Ask(ask) = event {
                kept.borrow_mut().push(ask.clone());
            }
        })
        .detach();
    });
    // The first drawing asked before anyone listened: the rows ask again.
    view.update(cx, |view, cx| {
        view.xdebug = None;
        view.asked_path = false;
        cx.notify();
    });
    cx.run_until_parked();
    (view, model, asks, cx)
}

/// Answers the questions kept, through the toolchain answers of the test, and names them.
fn answer_kept(asks: &Asks, cx: &mut VisualTestContext) -> Vec<String> {
    let pending: Vec<LanguageAsk> = asks.borrow_mut().drain(..).collect();
    let mut named = Vec::new();
    cx.update(|_, cx| {
        for ask in pending {
            named.push(match &ask {
                LanguageAsk::PhpXdebug { php, .. } => format!("xdebug {php}"),
                LanguageAsk::LocateTool { name, .. } => format!("locate {name}"),
                _ => "other".to_owned(),
            });
            answer(ask, "w1", cx);
        }
    });
    cx.run_until_parked();
    named
}

fn rows(view: &Entity<PhpInterpreterView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

// PHP settings: Xdebug row › shows the Xdebug version of the selected PHP, re-checked when the
// interpreter changes
#[gpui_kit::test]
async fn shows_the_xdebug_version_of_the_selected_php_rechecked_when_the_interpreter_changes(cx: &mut TestAppContext) {
    let (view, model, asks, cx) = php_rows(cx, &[(PHP_INTERPRETER, json!(PHP83))], detection());
    assert_eq!(answer_kept(&asks, cx), [format!("xdebug {PHP83}")]);
    assert_eq!(rows(&view, cx), ["Version: 8.3.14", "Xdebug: 3.4.7"]);
    model.update(cx, |dialog, cx| {
        dialog.stage(PAGE, Scope::Project, PHP_INTERPRETER, json!(PHP81));
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), ["Version: 8.1.33"], "not the previous PHP's Xdebug");
    assert!(cx.debug_bounds("settings-php-xdebug").is_none());
    assert_eq!(answer_kept(&asks, cx), [format!("xdebug {PHP81}")]);
    assert_eq!(rows(&view, cx), ["Version: 8.1.33", "Xdebug: Not installed"]);
    assert!(cx.debug_bounds("settings-php-xdebug").is_some());
}

// PHP settings: Xdebug row › checks the php on the search path for the Default interpreter
#[gpui_kit::test]
async fn checks_the_php_on_the_search_path_for_the_default_interpreter(cx: &mut TestAppContext) {
    let (view, _model, asks, cx) = php_rows(cx, &[(PHP_INTERPRETER, json!(""))], detection());
    assert!(rows(&view, cx).is_empty(), "nothing is shown before the PATH is searched");
    assert_eq!(answer_kept(&asks, cx), ["locate php"]);
    assert_eq!(answer_kept(&asks, cx), [format!("xdebug {PHP81}")]);
    assert_eq!(rows(&view, cx), ["Version: 8.1.33", "Xdebug: Not installed"]);
}

// PHP settings: Xdebug row › hides the row while checking and when the check fails
#[gpui_kit::test]
async fn hides_the_row_while_checking_and_when_the_check_fails(cx: &mut TestAppContext) {
    let failing = SdkDetection { xdebug: vec![(PHP81.to_owned(), None)], ..detection() };
    let (view, _model, asks, cx) = php_rows(cx, &[(PHP_INTERPRETER, json!(PHP83))], failing);
    assert_eq!(rows(&view, cx), ["Version: 8.3.14"], "while checking");
    assert!(cx.debug_bounds("settings-php-version").is_some());
    assert!(cx.debug_bounds("settings-php-xdebug").is_none());
    assert_eq!(answer_kept(&asks, cx), [format!("xdebug {PHP83}")]);
    assert_eq!(rows(&view, cx), ["Version: 8.3.14"], "after the check failed");
    assert!(cx.debug_bounds("settings-php-xdebug").is_none());
}

// toolchain settings with a project open › disables shared JDT LS only in a native Workspace's
// project scope
#[gpui_kit::test]
async fn disables_shared_jdt_ls_only_in_a_native_workspaces_project_scope(cx: &mut TestAppContext) {
    const JAVA: &str = "languages.java";
    let model = cx.update(|cx| dialog_model(JAVA, &[], cx));
    let scope = Rc::new(Cell::new(Scope::Project));
    let edited = scope.clone();
    let binding = Binding::new(model.clone(), JAVA, Rc::new(move |_: &App| edited.get()));
    let project = Binding::fixed(model.clone(), JAVA, Scope::Project);
    let (view, cx) = open(cx, move |_, cx| JdtlsSharedView::new(binding, project, cx));
    view.update(cx, |view, cx| view.set_workspace(true, cx));
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines, [format!("[ ] {SHARED_LABEL} (disabled)"), format!("    {SHARED_IN_WORKSPACE}")]);
    click(cx, "settings-toggle-languages.java.jdtlsShared");
    assert!(staged(&model, cx).is_empty(), "the project's checkbox takes no click");
    scope.set(Scope::Global);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines, [format!("[ ] {SHARED_LABEL}"), format!("    {SHARED_HINT}")]);
    click(cx, "settings-toggle-languages.java.jdtlsShared");
    assert_eq!(staged(&model, cx), [("global:languages.java.jdtlsShared".to_owned(), "true".to_owned())]);
}
