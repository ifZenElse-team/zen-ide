use super::*;

const YAML: &str = "spring:\n  datasource:\n    url: jdbc:h2:mem:db\n    username: sa\n  banner:\n    text: |\n      hello: world\n      line\nserver:\n  port: 8080\n---\nspring.config.activate.on-profile: dev\n";

const PROPERTIES: &str =
    "# Server\nserver.port=8080\nspring.application.name = shop\nspring.datasource.url=jdbc:h2:mem:db\n";

fn names(keys: &[Key]) -> Vec<String> {
    let mut found = Vec::new();
    fn walk(keys: &[Key], prefix: &str, found: &mut Vec<String>) {
        for key in keys {
            let name = if prefix.is_empty() { key.name.clone() } else { format!("{prefix}.{}", key.name) };
            found.push(name.clone());
            walk(&key.children, &name, found);
        }
    }
    walk(keys, "", &mut found);
    found
}

#[test]
fn tells_yaml_from_properties_by_the_text() {
    assert!(is_yaml(YAML));
    assert!(is_yaml("# comment\n---\na: 1\n"));
    assert!(is_yaml("list:\n- a\n"));
    assert!(!is_yaml(PROPERTIES));
    assert!(!is_yaml("a=1\n  continued: 2\n"));
    assert!(!is_yaml("server.port: 8080\n"));
    assert!(!is_yaml(""));
}

#[test]
fn reads_the_nested_keys_of_yaml_and_skips_block_scalars() {
    let keys = yaml_keys(YAML);
    assert_eq!(
        names(&keys),
        [
            "spring",
            "spring.datasource",
            "spring.datasource.url",
            "spring.datasource.username",
            "spring.banner",
            "spring.banner.text",
            "server",
            "server.port",
            "spring.config.activate.on-profile",
        ]
    );
    let spring = &keys[0];
    assert_eq!(&YAML[spring.start..spring.end], &YAML[..YAML.find("line").unwrap() + 4]);
}

#[test]
fn quoted_keys_and_values_with_colons_are_read_as_yaml_reads_them() {
    let text = "\"a.b\":\n  url: http://x:80/y\nc: 'd: e'\n";
    assert_eq!(names(&yaml_keys(text)), ["a.b", "a.b.url", "c"]);
}

#[test]
fn folds_yaml_mappings_and_properties_continuations() {
    let folds: Vec<(u32, u32)> = fold(YAML).iter().map(|fold| (fold.start_line, fold.end_line)).collect();
    assert_eq!(folds, [(0, 7), (1, 3), (4, 7), (5, 7), (8, 9)]);
    let folds: Vec<(u32, u32)> = fold("a = 1, \\\n  2\n").iter().map(|fold| (fold.start_line, fold.end_line)).collect();
    assert_eq!(folds, [(0, 1)]);
}

#[test]
fn highlights_each_format_with_its_own_reader() {
    let kinds = |text: &str| -> Vec<(String, u32)> {
        highlight(text).into_iter().map(|span| (text[span.start..span.end].to_owned(), span.kind)).collect()
    };
    assert!(kinds(PROPERTIES).contains(&("server.port".to_owned(), 17)));
    assert!(kinds(YAML).iter().any(|(text, _)| text == "8080"));
    assert!(!kinds(YAML).contains(&("spring".to_owned(), 17)));
}

#[test]
fn outlines_and_reports_properties_as_the_properties_plugin_does() {
    let outline = outline_json(&outline(PROPERTIES));
    let parsed: serde_json::Value = serde_json::from_str(&outline).unwrap();
    assert_eq!(parsed[0]["name"], "server.port");
    let found = problems("a=1\na=2\n");
    assert_eq!(found.len(), 2);
    assert!(!found[0].error);
    assert!(problems_json(&found).starts_with("[{\"start\":0"));
    assert!(inlays("a=\\u0041\n").iter().any(|inlay| inlay.text == "= A"));
    assert!(problems(YAML).is_empty() && inlays(YAML).is_empty());
}

#[test]
fn yaml_outline_is_valid_json_with_children() {
    let parsed: serde_json::Value = serde_json::from_str(&outline_json(&outline(YAML))).unwrap();
    assert_eq!(parsed[0]["children"][0]["name"], "datasource");
    assert_eq!(parsed[0]["children"][0]["children"][1]["name"], "username");
}

#[test]
fn survives_any_input() {
    for text in
        ["", ":", "a:\n\t- b\n", "\"unclosed:\n", "  - x: 1\n    y: 2\n", "k: |\n", "é: ü\n  ö: 1\n", "\r\n\r\n"]
    {
        let _ = highlight(text);
        let _ = fold(text);
        let _ = outline_json(&outline(text));
        for key in yaml_keys(text) {
            assert!(key.start <= key.end && key.end <= text.len());
        }
    }
}
