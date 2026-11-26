use serde_json::json;

use super::*;

fn tab(total: usize, files: usize, elsewhere: usize) -> ResultsTab {
    let tree = serde_json::from_value(json!({
        "tab": 1, "search": 1, "title": "Usages of orders", "kind": "database", "query": "orders",
        "total": total, "files": files, "elsewhere": elsewhere, "done": true, "more": false,
        "pinned": false, "request": {}, "options": {}, "nodes": []
    }))
    .unwrap();
    ResultsTab { tree: Some(tree), ..ResultsTab::new(1) }
}

#[test]
fn the_summary_counts_usages_in_the_database_apart_from_files_in_english_and_russian() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    assert_eq!(tab(3, 3, 0).summary().unwrap(), "3 usages in 3 files");
    assert_eq!(tab(3, 2, 1).summary().unwrap(), "2 usages in 2 files and 1 in the database");
    assert_eq!(tab(1, 0, 1).summary().unwrap(), "1 usage in the database");
    assert_eq!(tab(0, 0, 0).summary(), None);
    zen_i18n::set_language(Some("ru"));
    assert_eq!(tab(3, 2, 1).summary().unwrap(), "2 использования в 2 файлах и 1 в базе данных");
    assert_eq!(tab(5, 0, 5).summary().unwrap(), "5 использований в базе данных");
    assert_eq!(tab(22, 21, 1).summary().unwrap(), "21 использование в 21 файле и 1 в базе данных");
    zen_i18n::set_language(Some("en"));
}
