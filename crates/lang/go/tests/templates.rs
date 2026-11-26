#[test]
fn every_template_is_go_and_described_in_both_languages() {
    let templates: Vec<serde_json::Value> = serde_json::from_str(zen_lang_go::TEMPLATES).unwrap();
    assert!(templates.len() >= 15, "a useful set");
    let mut seen = std::collections::BTreeSet::new();
    for template in &templates {
        let abbreviation = template["abbreviation"].as_str().unwrap();
        assert!(seen.insert(abbreviation), "{abbreviation} is declared once");
        assert_eq!(template["language"], "go");
        assert!(!template["text"].as_str().unwrap().is_empty());
        let description = template["description"].as_str().unwrap();
        for language in ["en", "ru"] {
            let described = zen_lang_go::LOCALES
                .iter()
                .filter(|(code, _)| *code == language)
                .any(|(_, source)| source.lines().any(|line| line.starts_with(&format!("{description} = "))));
            assert!(described, "{description} in {language}");
        }
    }
    for abbreviation in ["for", "fori", "forr", "if", "iferr", "func", "meth", "main", "test", "bench", "struct"] {
        assert!(seen.contains(abbreviation), "{abbreviation}");
    }
}

#[test]
fn the_templates_import_the_packages_they_use() {
    let templates: Vec<serde_json::Value> = serde_json::from_str(zen_lang_go::TEMPLATES).unwrap();
    for template in &templates {
        let text = template["text"].as_str().unwrap();
        let imports: Vec<&str> = template["imports"]
            .as_array()
            .map(|list| list.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        for package in ["fmt", "testing"] {
            assert_eq!(text.contains(&format!("{package}.")), imports.contains(&package), "{text}");
        }
    }
    let (range, insert) = zen_lang_go::add_import("package main\n\nfunc main() {}\n", "fmt").unwrap();
    assert_eq!((range, insert.as_str()), (12..12, "\n\nimport \"fmt\""));
}

#[test]
fn every_postfix_template_takes_the_expression_and_is_described_in_both_languages() {
    let templates: Vec<serde_json::Value> = serde_json::from_str(zen_lang_go::POSTFIX).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for template in &templates {
        let key = template["key"].as_str().unwrap();
        assert!(seen.insert(key), "{key} is declared once");
        assert_eq!(template["language"], "go");
        let text = template["text"].as_str().unwrap();
        assert!(text.contains("$EXPR$"), "{key}");
        assert_eq!(
            text.contains("fmt."),
            template["imports"].as_array().is_some_and(|list| list.contains(&"fmt".into()))
        );
        let description = template["description"].as_str().unwrap();
        for language in ["en", "ru"] {
            let described = zen_lang_go::LOCALES
                .iter()
                .filter(|(code, _)| *code == language)
                .any(|(_, source)| source.lines().any(|line| line.starts_with(&format!("{description} = "))));
            assert!(described, "{description} in {language}");
        }
    }
    for key in ["var", "if", "not", "for", "nn", "nil", "return"] {
        assert!(seen.contains(key), "{key}");
    }
    let stubs: Vec<serde_json::Value> = serde_json::from_str(zen_lang_go::DOC_COMMENTS).unwrap();
    assert_eq!(stubs[0]["opening"], "//");
    let rules: Vec<serde_json::Value> = serde_json::from_str(zen_lang_go::STATEMENTS).unwrap();
    assert_eq!(rules[0]["language"], "go");
}
