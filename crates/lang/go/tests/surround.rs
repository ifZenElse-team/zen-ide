#[test]
fn every_surrounder_is_go_and_titled_in_both_languages() {
    let surrounders: Vec<serde_json::Value> = serde_json::from_str(zen_lang_go::SURROUNDERS).unwrap();
    let ids: Vec<&str> = surrounders.iter().map(|surrounder| surrounder["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["if", "ifElse", "for", "func", "parentheses", "not"]);
    for surrounder in &surrounders {
        assert_eq!(surrounder["language"], "go");
        assert!(surrounder["text"].as_str().unwrap().contains("$SELECTION$"), "{surrounder}");
        let expression = surrounder["expression"] == true;
        assert_eq!(expression, ["parentheses", "not"].contains(&surrounder["id"].as_str().unwrap()));
        let title = surrounder["title"].as_str().unwrap();
        for language in ["en", "ru"] {
            let titled = zen_lang_go::LOCALES
                .iter()
                .filter(|(code, _)| *code == language)
                .any(|(_, source)| source.lines().any(|line| line.starts_with(&format!("{title} = "))));
            assert!(titled, "{title} in {language}");
        }
    }
}
