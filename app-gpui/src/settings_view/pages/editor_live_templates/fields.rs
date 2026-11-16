//! The fields of the template being edited: abbreviation, description, group, template text,
//! contexts and one expression and default value per variable of the text. They are made for one
//! template and made again when another is selected, when the variables of its text change, and
//! when its stored form changes elsewhere; while the user types they are never written back, so the
//! caret stays where it is.

use super::super::new_field;
use crate::platform::input;
use crate::settings_view::controls::tx;
use crate::templates::live_model::{LiveTemplate, TemplateVariable, body_variables};
use crate::ui::text_area::TextArea;
use crate::ui::text_field::TextField;
use gpui_kit::{App, AppContext, Entity, EntityInputHandler, Focusable, Subscription};

/// Which field of the editor a text belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Abbreviation,
    Description,
    Group,
    Contexts,
    /// The expression of the variable named so.
    Expression(String),
    /// The default value of the variable named so.
    DefaultValue(String),
}

/// The variables a template's text uses, in order of first use, each with what the template says
/// of it (a variable the template says nothing of is a bare name).
pub fn text_variables(template: &LiveTemplate) -> Vec<TemplateVariable> {
    body_variables(&template.body)
        .into_iter()
        .map(|name| {
            template.variables.iter().find(|v| v.name == name).cloned().unwrap_or_else(|| TemplateVariable::new(&name))
        })
        .collect()
}

/// The contexts typed in the Contexts field: the words between commas and blanks.
pub fn parse_contexts(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace()).filter(|word| !word.is_empty()).map(str::to_owned).collect()
}

/// The fields of one variable's row.
pub struct VariableFields {
    pub name: String,
    pub expression: Entity<TextField>,
    pub default_value: Entity<TextField>,
}

/// The editor's fields for one template.
pub struct EditorFields {
    pub abbreviation: Entity<TextField>,
    pub description: Entity<TextField>,
    pub group: Entity<TextField>,
    pub contexts: Entity<TextField>,
    pub body: Entity<TextArea>,
    pub variables: Vec<VariableFields>,
    /// Tab in the template text inserts a tab: the claim taken for it, given back with the fields.
    pub claim: u64,
    /// The subscriptions to the fields above the variables, and to the variables' fields.
    pub _events: Vec<Subscription>,
    pub _variable_events: Vec<Subscription>,
}

impl EditorFields {
    /// Fields showing `template`; a plugin's template is shown read-only.
    pub fn new(template: &LiveTemplate, cx: &mut App) -> Self {
        let read_only = template.plugin_origin.is_some();
        let text = |text: &str, placeholder: Option<String>, cx: &mut App| {
            let field = new_field(text, placeholder, cx);
            field.update(cx, |field, cx| field.set_read_only(read_only, cx));
            field
        };
        let abbreviation = text(&template.abbrev, None, cx);
        let description = text(&template.description, None, cx);
        let group = text(&template.group, None, cx);
        let placeholder = tx(cx, "any, rust, typescript…");
        let contexts = text(&template.contexts.join(", "), Some(placeholder), cx);
        let body_text = template.body.clone();
        let body = cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_code_font(true, cx);
            area.set_soft_wrap(false, cx);
            area.set_rows(8, 20, cx);
            area.set_text(&body_text, cx);
            area.set_read_only(read_only, cx);
            area
        });
        // Tab in the text is a tab character at the caret, not a move to the next field.
        let area = body.clone();
        let claim = input::claim_keys(
            body.focus_handle(cx),
            move |keystroke, window, cx| {
                if keystroke.key != "tab" || keystroke.modifiers.modified() {
                    return false;
                }
                area.update(cx, |area, cx| area.replace_text_in_range(None, "\t", window, cx));
                true
            },
            cx,
        );
        let mut fields = Self {
            abbreviation,
            description,
            group,
            contexts,
            body,
            variables: Vec::new(),
            claim,
            _events: Vec::new(),
            _variable_events: Vec::new(),
        };
        fields.set_variables(template, cx);
        fields
    }

    /// One row of fields per variable of `template`'s text.
    pub fn set_variables(&mut self, template: &LiveTemplate, cx: &mut App) {
        let read_only = template.plugin_origin.is_some();
        let expression_hint = tx(cx, "className()");
        let default_hint = tx(cx, "\"value\"");
        self._variable_events.clear();
        self.variables = text_variables(template)
            .into_iter()
            .map(|variable| {
                let expression =
                    new_field(variable.expression.as_deref().unwrap_or(""), Some(expression_hint.clone()), cx);
                let default_value =
                    new_field(variable.default_value.as_deref().unwrap_or(""), Some(default_hint.clone()), cx);
                for field in [&expression, &default_value] {
                    field.update(cx, |field, cx| field.set_read_only(read_only, cx));
                }
                VariableFields { name: variable.name, expression, default_value }
            })
            .collect();
    }

    /// The names of the variables the rows are for.
    pub fn variable_names(&self) -> Vec<&str> {
        self.variables.iter().map(|variable| variable.name.as_str()).collect()
    }

    /// The text field of `kind`.
    pub fn field(&self, kind: &FieldKind) -> Option<&Entity<TextField>> {
        let variable = |name: &str| self.variables.iter().find(|variable| variable.name == name);
        match kind {
            FieldKind::Abbreviation => Some(&self.abbreviation),
            FieldKind::Description => Some(&self.description),
            FieldKind::Group => Some(&self.group),
            FieldKind::Contexts => Some(&self.contexts),
            FieldKind::Expression(name) => variable(name).map(|variable| &variable.expression),
            FieldKind::DefaultValue(name) => variable(name).map(|variable| &variable.default_value),
        }
    }

    /// Gives back the key claim; called before the fields are dropped.
    pub fn release(&self, cx: &mut App) {
        input::release_keys(self.claim, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: the variables of the text come in order of first use, repeated ones once,
    // each with what the template says of it.
    #[test]
    fn the_variables_of_the_text_keep_what_the_template_says_of_them() {
        let template = LiveTemplate {
            body: "$A$ $B$ $A$ $END$ $$".to_owned(),
            variables: vec![TemplateVariable::new("B").expression("user()"), TemplateVariable::new("Z")],
            ..LiveTemplate::default()
        };
        let expected = [TemplateVariable::new("A"), TemplateVariable::new("B").expression("user()")];
        assert_eq!(text_variables(&template), expected);
        assert_eq!(parse_contexts(" rust, typescript  go,,"), ["rust", "typescript", "go"]);
        assert!(parse_contexts(" , ").is_empty());
    }
}
