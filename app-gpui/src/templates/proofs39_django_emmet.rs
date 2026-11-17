//! Emmet's context in Django templates.

use super::emmet_model::{EmmetSyntax, EmmetType, has_emmet, syntax_at};

// emmet context › expands markup in Django templates, not inside template tags, variables or comments
#[test]
fn expands_markup_in_django_templates_not_inside_template_tags_variables_or_comments() {
    let html = Some(EmmetSyntax { kind: EmmetType::Markup, syntax: "html" });
    assert_eq!(syntax_at("django-html", "{% block content %}\n  ul>li"), html);
    assert_eq!(syntax_at("django-html", "{% if user"), None);
    assert_eq!(syntax_at("django-html", "{{ value|date"), None);
    assert_eq!(syntax_at("django-html", "{# note"), None);
    assert_eq!(syntax_at("django-html", "<a href=\"{% url \"home\" %}\">x</a>\nnav"), html);
    assert!(has_emmet("django-html"));
    // The template syntax means nothing in plain HTML.
    assert_eq!(syntax_at("html", "{% if user"), html);
}
