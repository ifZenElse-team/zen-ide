use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::*;

fn project(filter: bool) -> Value {
    json!({
        "filterTargets": filter,
        "buildFiles": [
            {
                "path": "/p/build.xml", "name": "shop", "default": "jar",
                "settings": { "path": "build.xml" },
                "targets": [
                    { "name": "init", "description": "" },
                    { "name": "compile", "description": "Compiles the sources" },
                    { "name": "jar", "description": "" }
                ]
            },
            { "path": "/p/broken.xml", "name": "broken.xml", "error": "unexpected end of stream\nat 1:9", "settings": { "path": "broken.xml" }, "targets": [] }
        ]
    })
}

fn labels(rows: &[AntRow]) -> Vec<String> {
    rows.iter()
        .map(|entry| format!("{}{} {}", "  ".repeat(entry.row.depth), entry.row.label, entry.row.detail))
        .collect()
}

#[test]
fn build_files_list_their_targets_with_the_default_marked() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let rows = ant_rows(&project(false), &BTreeSet::new());
    assert_eq!(
        labels(&rows),
        [
            "shop build.xml",
            "  init ",
            "  compile Compiles the sources",
            "  jar (default)",
            "broken.xml unexpected end of stream"
        ]
    );
    assert!(rows[0].row.parent && !rows[4].row.parent, "a build file that does not read has nothing under it");
    assert_eq!(rows[0].action, Some(AntAction::Open("/p/build.xml".into())));
    assert_eq!(rows[2].action, Some(AntAction::Run { build_file: "build.xml".into(), target: "compile".into() }));
    assert_eq!(rows[3].build_file, "build.xml");
}

#[test]
fn the_filter_keeps_described_targets_and_the_default_and_a_closed_file_hides_them() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let rows = ant_rows(&project(true), &BTreeSet::new());
    assert_eq!(labels(&rows)[1..3], ["  compile Compiles the sources", "  jar (default)"]);
    let closed: BTreeSet<String> = ["build.xml".to_owned()].into_iter().collect();
    let rows = ant_rows(&project(false), &closed);
    assert_eq!(rows.len(), 2);
}
