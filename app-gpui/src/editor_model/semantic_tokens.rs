//! The language hooks of semantic token decoding ([`crate::editor_lsp::semantic`]) as Go and
//! Kotlin servers exercise them.

#[cfg(test)]
mod tests {
    use crate::editor_lsp::semantic::{SemanticRange, semantic_ranges};
    use ide_lsp::types::SemanticTokens;
    use ropey::Rope;

    fn tokens(types: &[&str], modifiers: &[&str], data: &[u32]) -> SemanticTokens {
        SemanticTokens {
            data: data.to_vec(),
            token_types: types.iter().map(|name| (*name).to_owned()).collect(),
            token_modifiers: modifiers.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn ranges(input: &SemanticTokens, rainbow: bool, language: &str) -> Vec<SemanticRange> {
        semantic_ranges(&Rope::from_str("name"), input, rainbow, Some(language))
    }

    fn classes(input: &SemanticTokens, rainbow: bool, language: &str) -> Vec<String> {
        ranges(input, rainbow, language)
            .first()
            .map(|range| range.class_name.split(' ').map(str::to_owned).collect())
            .unwrap_or_default()
    }

    #[test]
    fn classifies_go_variable_modifiers() {
        let cases: [(&[&str], &[&str]); 4] = [
            (&["readonly"], &["cm-sem-constant"]),
            (&["static"], &["cm-sem-global", "cm-sem-static"]),
            (&["readonly", "static"], &["cm-sem-constant", "cm-sem-static"]),
            (&["constant", "static"], &["cm-sem-constant", "cm-sem-static"]),
        ];
        for (modifiers, expected) in cases {
            let bits = (1u32 << modifiers.len()) - 1;
            let mut found = classes(&tokens(&["variable"], modifiers, &[0, 0, 4, 0, bits]), false, "go");
            found.sort();
            assert_eq!(found, expected, "{modifiers:?}");
        }
    }

    #[test]
    fn keeps_local_constants_out_of_rainbow_coloring_and_scopes_readonly_to_go() {
        let readonly = tokens(&["variable"], &["readonly"], &[0, 0, 4, 0, 1]);
        assert_eq!(classes(&readonly, true, "go"), ["cm-sem-constant"]);
        assert!(ranges(&readonly, false, "typescript").is_empty());
        let plain = tokens(&["variable"], &["readonly"], &[0, 0, 4, 0, 0]);
        assert!(ranges(&plain, true, "go")[0].class_name.contains("cm-sem-rainbow-"));
        let parameter = tokens(&["parameter"], &["readonly"], &[0, 0, 4, 0, 1]);
        assert!(ranges(&parameter, true, "go")[0].class_name.contains("cm-sem-rainbow-"));
    }

    #[test]
    fn retains_package_field_and_function_roles_from_the_common_decoder() {
        for (token_type, expected) in [
            ("namespace", "cm-sem-namespace"),
            ("property", "cm-sem-property"),
            ("function", "cm-sem-function"),
            ("type", "cm-sem-type"),
            ("typeParameter", "cm-sem-typeParameter"),
        ] {
            let found = ranges(&tokens(&[token_type], &[], &[0, 0, 4, 0, 0]), false, "go");
            assert_eq!(found[0].class_name, expected, "{token_type}");
        }
    }

    #[test]
    fn leaves_branch_labels_and_missing_server_tokens_to_syntax_highlighting() {
        assert!(ranges(&tokens(&["label"], &[], &[0, 0, 4, 0, 0]), false, "go").is_empty());
        assert!(ranges(&tokens(&[], &[], &[]), true, "go").is_empty());
    }

    #[test]
    fn recognizes_kotlin_token_types_with_their_modifiers() {
        let cases: [(&str, &str, &[&str]); 5] = [
            ("variable", "modification", &["cm-sem-mutable"]),
            ("property", "modification", &["cm-sem-property", "cm-sem-mutable"]),
            ("parameter", "modification", &["cm-sem-parameter", "cm-sem-mutable"]),
            ("operator", "declaration", &["cm-sem-declaration", "cm-sem-function"]),
            ("label", "", &["cm-sem-constant"]),
        ];
        for (token_type, modifier, expected) in cases {
            let found = classes(&tokens(&[token_type], &[modifier], &[0, 0, 4, 0, 1]), false, "kotlin");
            assert_eq!(found, expected, "{token_type} with {modifier}");
        }
    }

    #[test]
    fn keeps_modification_language_specific_and_leaves_val_properties_as_properties() {
        assert!(ranges(&tokens(&["variable"], &["modification"], &[0, 0, 4, 0, 1]), false, "typescript").is_empty());
        let val = tokens(&["property"], &["readonly"], &[0, 0, 4, 0, 1]);
        assert_eq!(ranges(&val, false, "kotlin")[0].class_name, "cm-sem-property");
    }
}
