//! CMake colours, each stated as the colour token a CMake editor paints: commands as built-ins, argument
//! keywords as keywords, predefined variables as built-ins, variable expansions as constants.

use super::proofs_editor_highlight_support::{Expect, highlighted};
use super::*;

const COMMAND: Option<&str> = Some("syn-builtin");
const BUILTIN_VARIABLE: Option<&str> = Some("syn-builtin");
const KEYWORD: Option<&str> = Some("syn-keyword");
const NUMBER: Option<&str> = Some("syn-number");
const EXPANSION: Option<&str> = Some("syn-constant");

#[gpui_kit::test]
async fn styles_the_screenshots_commands_argument_keywords_and_built_in_variables(cx: &mut TestAppContext) {
    let text = "cmake_minimum_required(VERSION 3.20)\nproject(OpenIdeaSmoke LANGUAGES C CXX)\n\
                set(CMAKE_CXX_STANDARD 17)\nset(CMAKE_EXPORT_COMPILE_COMMANDS ON)\n\
                add_executable(cpp_smoke main.cpp math.cpp)\nenable_testing()\n\
                add_test(NAME cpp_result COMMAND cpp_smoke)\n";
    let (f, cx) = highlighted(cx, "CMakeLists.txt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        for command in ["cmake_minimum_required", "project", "set", "add_executable", "enable_testing", "add_test"] {
            expect.painted(editor, command, 0, COMMAND);
        }
        for keyword in ["VERSION", "LANGUAGES", "NAME", "«COMMAND» cpp_smoke"] {
            expect.painted(editor, keyword, 0, KEYWORD);
        }
        for variable in ["CMAKE_CXX_STANDARD", "CMAKE_EXPORT_COMPILE_COMMANDS"] {
            expect.painted(editor, variable, 0, BUILTIN_VARIABLE);
        }
        expect.painted(editor, "3.20", 0, NUMBER);
        expect.painted(editor, "17", 0, NUMBER);
        expect.painted(editor, "«ON»)", 0, KEYWORD);
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn styles_version_ranges_and_the_project_ctest_and_cpack_predefined_variables(cx: &mut TestAppContext) {
    let text = "find_package(Qt6 6.2...<7.0 REQUIRED)\n\
                message(${PROJECT_NAME} ${CTEST_SOURCE_DIRECTORY} ${CPACK_GENERATOR})\nset(PROJECT_VERSION 1.2.3)";
    let (f, cx) = highlighted(cx, "CMakeLists.txt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.painted(editor, "6.2...<7.0", 0, NUMBER);
        expect.painted(editor, "1.2.3", 0, NUMBER);
        expect.painted(editor, "PROJECT_VERSION", 0, BUILTIN_VARIABLE);
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn recognizes_case_insensitive_commands_control_flow_and_user_defined_functions(cx: &mut TestAppContext) {
    let text = "FUNCTION(Build target)\n  MESSAGE(STATUS hello)\nENDFUNCTION()\nBuild(app)\n\
                if(DEFINED CMAKE_BUILD_TYPE AND NOT OFF)\nendif()";
    let (f, cx) = highlighted(cx, "CMakeLists.txt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        for keyword in ["«FUNCTION»(Build", "ENDFUNCTION", "«if»(", "DEFINED", "AND", "NOT", "endif"] {
            expect.painted(editor, keyword, 0, KEYWORD);
        }
        expect.painted(editor, "MESSAGE", 0, COMMAND);
        // The function's name where it is defined, then where it is called.
        expect.painted(editor, "Build", 0, Some("syn-function"));
        expect.painted(editor, "Build", 1, Some("syn-function-call"));
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn separates_quoted_text_nested_variable_expansions_environment_cache_lookups_and_escapes(
    cx: &mut TestAppContext,
) {
    let text = "message(\"hello ${${PREFIX}_NAME} $ENV{HOME} $CACHE{SDK} \\\"quote\\\" # text\")";
    let (f, cx) = highlighted(cx, "CMakeLists.txt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        for expansion in ["${${PREFIX}_NAME}", "$ENV{HOME}", "$CACHE{SDK}"] {
            expect.painted(editor, expansion, 0, EXPANSION);
        }
        expect.painted(editor, "\\\"", 0, Some("syn-escape"));
        expect.nowhere(editor, "syn-comment");
        expect.partly(editor, "# text", 0, "syn-string");
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn keeps_multiline_bracket_comments_and_literal_bracket_strings_opaque(cx: &mut TestAppContext) {
    let text = "#[=[\nset(CMAKE_FAKE ON)\n]=]\nset(TEXT [==[\n${LITERAL} # raw\n]==])\n# line comment\nmessage(hello)";
    let (f, cx) = highlighted(cx, "CMakeLists.txt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.painted(editor, "set", 1, COMMAND);
        expect.partly(editor, "CMAKE_FAKE", 0, "syn-comment");
        expect.partly(editor, "${LITERAL}", 0, "syn-string");
        expect.nowhere(editor, "syn-constant");
        expect.painted(editor, "message", 0, COMMAND);
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn handles_nested_generator_expressions_without_swallowing_following_commands(cx: &mut TestAppContext) {
    let expression = "$<IF:$<BOOL:${ENABLE}>,a,b>";
    let text = format!("target_link_libraries(app PRIVATE {expression})\nmessage(done)");
    let (f, cx) = highlighted(cx, "CMakeLists.txt", &text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.painted(editor, expression, 0, EXPANSION);
        expect.painted(editor, "message", 0, COMMAND);
        expect.check(editor);
    });
    f.host.close_project("w1");
}

/// An edit that opens a quoted argument over a bracket argument's text colours what follows again, and
/// `CMakeLists.txt`, `*.cmake` and `*.cmake.in` files are CMake.
#[gpui_kit::test]
async fn reparses_multiline_string_edits_and_resolves_cmake_filename_associations(cx: &mut TestAppContext) {
    let text = "set(TEXT [=[\nmessage(hidden)\n]=])\nmessage(visible)";
    let (f, cx) = highlighted(cx, "CMakeLists.txt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.never(editor, "message", 0, "syn-builtin");
        expect.painted(editor, "message", 1, COMMAND);
        expect.check(editor);
    });
    // `[=[` becomes `"`: the quoted argument runs to the end, taking both commands' text.
    let edited = f.editor.update(cx, |editor, cx| {
        editor.apply_edit(&[crate::editor_commands::change(9, 12, "\"")], vec![(10, 10)], 0, cx)
    });
    assert!(edited);
    let current = |e: &Editor| e.highlights_version == e.version();
    settle(cx, |cx| f.editor.read_with(cx, |e, _| current(e)));
    f.editor.read_with(cx, |editor, _| {
        assert_eq!(editor.text(), "set(TEXT \"\nmessage(hidden)\n]=])\nmessage(visible)");
        let mut expect = Expect::default();
        expect.never(editor, "message", 0, "syn-builtin");
        expect.never(editor, "message", 1, "syn-builtin");
        expect.check(editor);
    });
    assert_eq!(f.editor.read_with(cx, |e, _| e.language().clone()), LanguageId::new("cmake"));
    for name in ["module.cmake", "toolchain.cmake.in"] {
        assert_eq!(f.host.languages.detect(std::path::Path::new(name), None), LanguageId::new("cmake"), "{name}");
    }
    f.host.close_project("w1");
}
