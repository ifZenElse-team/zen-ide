use serde_json::json;

use super::*;

fn module() -> Value {
    json!({
        "id": "pom.xml", "name": "shop", "scopes": ["compile", "test"],
        "dependencies": [
            { "group": "a", "artifact": "plain", "version": "1", "scope": "compile",
              "versionPlace": { "kind": "literal" }, "status": { "latest": "1" } },
            { "group": "a", "artifact": "old", "version": "1", "scope": "test",
              "versionPlace": { "kind": "property", "name": "old.version" },
              "status": { "latest": "2", "newer": ["2", "1.5"] } },
            { "group": "a", "artifact": "bad", "version": "1", "scope": "compile",
              "versionPlace": { "kind": "managed", "by": "b:c:1" },
              "status": { "newer": ["2"], "safe": "2", "advisories": [{ "id": "CVE-1", "summary": "s", "url": "u" }] } },
        ],
    })
}

#[test]
fn rows_mark_outdated_and_vulnerable_versions() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let rows = dependency_rows(&module());
    let shown: Vec<(&str, Mark, &str)> =
        rows.iter().map(|row| (row.id.as_str(), row.mark, row.note.as_str())).collect();
    assert_eq!(
        shown,
        [
            ("a:plain:compile", Mark::Plain, ""),
            ("a:old:test", Mark::Outdated, "2 is available"),
            ("a:bad:compile", Mark::Vulnerable, "1 vulnerability"),
        ]
    );
}

#[test]
fn a_version_a_repository_manages_offers_no_upgrade_and_says_who_manages_it() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    assert!(changes(&json!({ "kind": "literal" })) && changes(&json!({ "kind": "managedHere", "by": "pom.xml" })));
    assert!(!changes(&json!({ "kind": "managed", "by": "b" })) && !changes(&json!({ "kind": "unknown" })));
    assert_eq!(place_note(&json!({ "kind": "managed", "by": "b:c:1" })).unwrap(), "The version is managed by b:c:1.");
    assert_eq!(place_note(&json!({ "kind": "literal" })), None);
}

#[test]
fn choosing_a_result_starts_with_its_latest_stable_version() {
    let mut window = DependenciesToolWindow::default();
    let result = json!({ "group": "a", "artifact": "b", "latest": "1.0", "versions": ["2.0-rc1", "1.0"] });
    window.choose_result(&result, "compile".to_owned());
    let adding = window.adding.unwrap();
    assert_eq!((adding.version.as_str(), adding.scope.as_str()), ("1.0", "compile"));
    assert_eq!(adding.versions, ["2.0-rc1", "1.0"]);
}
