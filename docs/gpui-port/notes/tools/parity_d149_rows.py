import json, re
ROOT = "/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01"
S = "/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad"
path = f"{ROOT}/docs/gpui-port/parity.json"
p = json.load(open(path))
tests = set(open(f"{S}/rust_tests.txt").read().split())
def names(*ns):
    for n in ns: assert n in tests, n
    return list(ns)
def under(prefix, *leaves):
    return names(*[prefix + l for l in leaves])
def cite(ns): return ", ".join(f"`{n}`" for n in ns)
def alive(n): return n in tests or any(t.startswith(n + "::") for t in tests)
def cited(key):
    """The names an existing row cites that the build still has; a name that is gone is printed."""
    if key not in p: return []
    pr = p[key]["proof"]; i = pr.find(";")
    old = re.findall(r"`([^`]+)`", pr[:i] if i > 0 else pr)
    for n in old:
        if not alive(n): print("gone:", key, n)
    return [n for n in old if alive(n)]
def row(key, status, ns, text):
    assert key in p, key
    old = cited(key)
    p[key] = {"status": status, "proof": cite(old + [n for n in ns if n not in old]) + "; " + text}
def new(key, status, ns, text):
    assert key not in p, key
    p[key] = {"status": status, "proof": cite(ns) + "; " + text}

def sub(key, status, ns, old, new):
    """Keeps the row's citations and text, replacing the fragment `old` of the text by `new`."""
    assert key in p, key
    pr = p[key]["proof"]; i = pr.find(";")
    head, text = (pr[:i], pr[i + 1:]) if i > 0 else (pr, "")
    assert old in text, (key, old)
    kept = cited(key)
    proof = cite(kept + [n for n in ns if n not in kept]) + ";" + text.replace(old, new)
    p[key] = {"status": status, "proof": proof}


def recite(key, old_name, new_name):
    pass

def addcite(key, ns, extra=""):
    """Adds citations to a row, keeping its status and text; `extra` is appended to the text."""
    assert key in p, key
    pr = p[key]["proof"]; i = pr.find(";")
    head, text = (pr[:i], pr[i:]) if i > 0 else (pr, "")
    kept = cited(key)
    if extra: text = (text + " " + extra) if text else "; " + extra
    p[key]["proof"] = cite(kept + [n for n in ns if n not in kept]) + text

TA = "workbench::tests::proofs34_template_actions::"
TF = "workbench::tests::proofs34_template_files::"
EM = "workbench::tests::proofs34_emmet::"
ND = "templates::proofs34_new_file_dialog::"
UT = "templates::proofs34_update_tag_dialog::"
RD = "workbench::debug::tests::proofs34_remote_debug::"
PLUGINS = "missing: file templates contributed by plugins are never in effect in the app (nothing hands them to the window), so they are in no list, menu or action"

new("action:templates.newFromTemplate", "partial",
    under(TA, "new_from_template_lists_the_enabled_templates_and_creates_the_chosen_one", "new_from_template_with_a_template_of_its_own_names_a_file_of_it", "new_from_template_with_an_extension_opens_that_templates_dialog_with_the_kind_and_the_name"),
    "from the registry it lists the enabled templates of the window's scheme and creates the chosen one in the folder it was used in; with a template or an extension as its argument it opens that template's dialog; " + PLUGINS)
new("action:(dynamic) src/features/templates/index.ts:81", "partial",
    under(TA, "the_template_actions_and_rows_follow_the_templates_in_effect", "a_template_action_without_an_argument_uses_the_active_files_folder", "two_windows_on_projects_of_different_schemes_each_offer_their_own_templates", "the_template_actions_and_dialogs_are_titled_in_the_ui_language", "the_template_actions_follow_the_templates_given"),
    "one action per template in effect, added and removed with the templates without leaking registrations, titled in the UI language; each window's menu shows its own scheme's templates while the registered actions are the union over the windows; " + PLUGINS + "; the actions are not synced again when a window closes without closing its project")
new("point.registerMenuGroup:templates.new", "partial",
    under(TA, "the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog", "the_new_menu_offers_the_templates_of_the_projects_type", "is_listed_in_the_new_menu_with_its_icon_without_duplicating_entries"),
    "the New popup and menu list the templates of the project's type with their icons and a row opens its dialog; " + PLUGINS + "; the project's type is read from the window's root only, not from a workspace member")
new("point.registerMenuGroup:templates.newFromTemplate", "done",
    under(TA, "the_new_popup_lists_the_templates_and_a_template_row_opens_its_dialog", "new_from_template_lists_the_enabled_templates_and_creates_the_chosen_one"),
    "the New from Template row of the New popup opens the list")
new("point.registerOverlay:templates.newFile", "partial",
    under(TF, "a_file_from_a_template_is_created_opened_with_the_caret_at_its_end_marker_and_saved", "no_name_template_or_kind_creates_a_file_outside_the_folder_new_was_used_in", "nothing_is_created_outside_the_folder_whatever_the_name_or_the_extension_say", "a_taken_name_is_reported_in_the_dialog_and_the_file_is_left_as_it_was", "an_existing_notebook_is_not_overwritten_by_a_new_one_of_the_same_name", "a_folder_that_cannot_be_written_reports_the_failure_and_creates_nothing", "new_on_a_linked_folder_creates_the_file_in_the_folder_the_link_points_to", "a_plugin_templates_package_goes_into_the_body_never_into_the_path", "in_restricted_mode_new_starts_no_formatter_after_creating_the_file", "in_a_trusted_project_new_reformats_the_file_with_the_formatter", "a_java_class_gets_the_package_of_the_folder_its_name_puts_it_in", "a_csharp_class_gets_the_namespace_of_its_csproj_in_a_block_before_csharp_10", "writes_the_titled_notebook_body_before_the_notebook_editor_takes_the_file", "escape_closes_the_dialog_without_creating_anything")
    + under(ND, "shows_the_template_chooser_then_the_name_step", "prefills_fixed_names_and_follows_the_kinds_extension", "the_arrows_go_round_the_list_and_a_bad_name_is_reported", "the_hint_shows_the_planned_path_and_the_package_found_for_it", "does_not_submit_while_confirming_ime_text"),
    "the New File dialog with kinds, the hint of the planned path, validation, variables asked together; the file is created through the backend in the folder New was used in and nowhere else, never over an existing file, opened with the caret at the template's end marker and reformatted only where the project may start a formatter; missing: the notice that offers to add the created file to Git, the notification when the new file cannot be filled, the choosers' title, start folder and filter")
new("point.registerOverlay:templates.emmetUpdateTag", "done",
    under(EM, "the_editor_menus_row_opens_the_prompt_and_one_undo_reverts_the_update", "escape_closes_the_update_tag_prompt_without_editing") + under(UT, "the_prompt_names_the_tag_and_ok_waits_for_an_abbreviation", "escape_and_cancel_close_the_prompt_and_composing_text_is_not_applied"),
    "the prompt names the tag, OK waits for an abbreviation, Escape and Cancel close it without editing")
new("action:emmet.updateTag", "done",
    under(EM, "update_tag_with_emmet_renames_the_element_and_sets_its_attributes", "the_editor_menus_row_opens_the_prompt_and_one_undo_reverts_the_update", "update_tag_is_off_outside_markup_and_a_caret_outside_any_tag_is_told", "an_abbreviation_of_several_elements_leaves_the_tag_and_warns"),
    "from the registry and the editor menu's row in markup files: both tags of the pair change in one undo step; off outside markup; a caret outside any tag and an abbreviation of several elements are told")
new("point.registerMenuGroup:templates.emmet", "done",
    under(EM, "the_editor_menu_offers_update_tag_for_markup_files_only", "the_editor_menus_row_opens_the_prompt_and_one_undo_reverts_the_update"),
    "the editor menu offers Update Tag for markup files only and its row opens the prompt")
new("unit:src/features/templates/__tests__/builtinNames.test.ts", "done",
    names("templates::files_builtins::tests::are_translated_where_the_new_menu_and_dialog_show_them"), "1 of 1 case, by its name")
new("unit:src/features/templates/__tests__/pluginFiles.test.tsx", "partial",
    under(ND, "creates_a_fixed_dotfile_without_asking_for_a_name", "asks_all_variables_together_validates_them_and_creates_only_on_submit", "cancels_fixed_name_variables_without_creating_anything", "preserves_plugin_prompts_when_they_collide_with_the_ide_text_catalog", "does_not_submit_while_confirming_ime_text", "does_not_create_when_the_variables_dialog_closes_with_escape"),
    "6 of 6 cases by their names, on the dialog given a plugin's template; " + PLUGINS)
new("unit:src/features/templates/__tests__/templateCreate.test.ts", "partial",
    under(TF, "writes_the_titled_notebook_body_before_the_notebook_editor_takes_the_file", "a_file_from_a_template_is_created_opened_with_the_caret_at_its_end_marker_and_saved") + under(TA, "is_listed_in_the_new_menu_with_its_icon_without_duplicating_entries") + names("templates::files_create::tests::a_file_a_viewer_opens_gets_its_body_on_creation_and_an_empty_body_an_empty_file"),
    "2 of 6 cases by their names; the integrator maps three more to the other cited tests (the body written with a notebook file, a text template inserted with the caret at its end marker, the notebook body rendered): not re-read by me; not covered through the app: creates an empty file when the template renders nothing (on the model only)")
row("unit:src/features/templates/__tests__/views.test.tsx", "done",
    under(ND, "shows_the_template_chooser_then_the_name_step", "prefills_fixed_names_and_follows_the_kinds_extension", "names_an_ad_hoc_template_from_another_feature") + names("templates::proofs33_chooser_view::typing_filters_the_templates_and_enter_picks_the_selected_one", "templates::proofs33_chooser_view::a_mnemonic_picks_its_item_and_section_rows_are_skipped"),
    "12 of 12 cases: ten by their names on the three Settings pages and the New File dialog, the two chooser cases by the chooser view's tests of filtering with Enter and of mnemonics skipping section rows")
row("unit:src/features/templates/__tests__/pluginPackages.test.ts", "done",
    names("templates::files_create::tests::uses_the_nearest_csproj_for_csharp") + under(TF, "a_java_class_gets_the_package_of_the_folder_its_name_puts_it_in", "go_and_php_files_get_the_package_their_folder_declares", "a_csharp_class_gets_the_namespace_of_its_csproj_in_a_block_before_csharp_10"),
    "8 of 8 cases by their names on the model, and Java, Go, PHP and C# packages through the app's New File dialog")
row("unit:src/features/templates/__tests__/emmet.test.ts", "partial", [],
    "4 of 8 cases by their names; not covered: expands markup in Django templates, expands abbreviations into snippets with fields, fills attribute placeholders and escapes snippet syntax, expands CSS properties and ignores unknown words: the expansions need the Emmet engine's tables, which are behind a trait with no implementation yet")
row("unit:src/features/templates/__tests__/files.test.ts", "partial",
    names("templates::files_model::tests::come_from_the_projects_root_namespace_and_the_folders_below_the_csproj") + under(TF, "a_file_from_a_template_is_created_opened_with_the_caret_at_its_end_marker_and_saved", "no_name_template_or_kind_creates_a_file_outside_the_folder_new_was_used_in"),
    "planning and rendering a file are on the model and now reached through the app's New File dialog; the 32 cases are not mapped one by one since upstream added seven: a match by name finds 20, and the titles of the other 12 hold quotes or parentheses my match cut, so they are neither confirmed nor missing")
row("action:navigate.gotoTest", "partial", [],
    "run through the registry from src/calc.py it opens tests/test_calc.py, and from the test the subject again; with two tests it offers both in a chooser; Create New Test… is offered with the file templates and opens the New File dialog on the test kind. Not checked: the chooser of a file without tests (Choose Test for X, 0 found); the Rust test module insertion is untested")
row("module:src/editor/navigation/gotoTest.ts", "partial", [],
    "from a subject to its only test and back, the chooser of two tests, and Create New Test… opening the New File dialog on the test kind. Not checked: the chooser of a file without tests; the Rust test module insertion")
REFUSED = under(RD, "debugging_a_configuration_on_an_ssh_target_is_refused_and_starts_no_adapter", "debug_configuration_in_a_container_is_refused_and_its_button_edits_the_configuration", "a_configuration_on_this_machine_is_still_debugged")
addcite("action:run.debug", REFUSED + under(RD, "the_run_widgets_debug_button_on_a_run_target_is_refused"))
addcite("action:run.debugConfig", REFUSED, "Debug of a configuration on a run target is refused with the notice and starts nothing.")
addcite("action:debug.rerun", under(RD, "rerun_in_the_debugger_of_a_configuration_now_on_a_run_target_is_refused"), "Rerun of a configuration now on a run target is refused.")
new("unit:src/features/run/remote.test.tsx", "partial",
    names("run::remote_debug::tests::refuses_a_configuration_on_another_machine_and_names_its_target", "run::remote_debug::tests::in_a_container_only_python_is_debugged_and_only_with_the_container_path", "run::remote_debug::tests::the_settings_target_applies_to_a_configuration_without_its_own") + REFUSED,
    "only the refusal of Debug on a run target (the notice's title, text and buttons, and that no adapter starts); the other cases of the 20 are not mapped; Python under debugpy in a container, which the webview debugs, is refused natively because the debugger has no container path")
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
