use serde_json::{Value, json};

use super::*;

const MAIN: &str = "package main\n\nfunc main() {}\n";
const UTIL: &str = "package main\n\nvar answer = 42\n";
const LIB: &str = "package main\n\nfunc lib() {}\n";

struct Fixture {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [("main.go", MAIN), ("util.go", UTIL), ("lib.go", LIB)] {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        Self { registry, session, dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fails(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
    }

    fn open(&mut self, name: &str) {
        let path = self.path(name);
        self.run("file.document.open", json!({ "path": path }));
    }

    fn edit(&mut self, name: &str, text: &str) {
        self.open(name);
        self.run("editor.edit.insert", json!({ "text": text }));
    }

    fn disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap()
    }

    fn tabs(&self) -> Vec<String> {
        let workbench = self.session.state::<Workbench>().unwrap();
        let group = workbench.groups().group(workbench.groups().focused()).unwrap();
        group.tabs.iter().map(|path| path.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    fn document(&self, name: &str) -> &crate::Document {
        let path = self.path(name);
        let workbench = self.session.state::<Workbench>().unwrap();
        workbench.documents().iter().find(|document| document.path() == path).unwrap()
    }

    fn conflict(&self, name: &str) -> bool {
        self.document(name).has_conflict()
    }

    fn text(&self, name: &str) -> String {
        self.document(name).text().to_owned()
    }

    fn dirty(&self, name: &str) -> bool {
        let path = self.path(name);
        let workbench = self.session.state::<Workbench>().unwrap();
        workbench.documents().iter().any(|document| document.path() == path && document.is_dirty())
    }

    fn names(&self, value: &Value) -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|path| Path::new(path.as_str().unwrap()).file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }
}

#[cfg(unix)]
fn read_only(folder: &Path, locked: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if locked { 0o555 } else { 0o755 };
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn close_tab_saves_unsaved_changes_before_the_tab_closes() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// kept\n");
    let closed = fixture.run("file.document.close", json!({}));
    assert_eq!(fixture.names(&closed["closed"]), ["main.go"]);
    assert_eq!(fixture.names(&closed["saved"]), ["main.go"]);
    assert_eq!(fixture.disk("main.go"), format!("// kept\n{MAIN}"), "the edit is on disk");
    assert!(fixture.tabs().is_empty());

    fixture.open("main.go");
    let clean = fixture.run("file.document.close", json!({ "index": 0 }));
    assert_eq!(clean["saved"], json!([]), "nothing to save");
}

#[test]
fn close_tab_by_index_in_another_group_saves_that_file() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    fixture.edit("util.go", "// util\n");
    let split = fixture.run("editor.split.right", json!({ "move": true }));
    let group = split["focused"].clone();
    fixture.run("editor.split.focus", json!({ "group": 1 }));
    fixture.run("file.document.close", json!({ "group": group, "index": 0 }));
    assert_eq!(fixture.disk("util.go"), format!("// util\n{UTIL}"));
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert!(!workbench.groups().is_split(), "the split left without tabs goes");
    assert!(!workbench.groups().contains(&fixture.path("util.go")));
}

#[cfg(unix)]
#[test]
fn a_tab_that_cannot_be_saved_stays_open_with_its_changes() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// stays\n");
    read_only(fixture.dir.path(), true);
    let refused = fixture.fails("file.document.close", json!({}));
    read_only(fixture.dir.path(), false);
    assert!(refused.contains("main.go"), "the error names the file: {refused}");
    assert_eq!(fixture.tabs(), ["main.go"], "the tab stays");
    assert!(fixture.dirty("main.go"), "with its changes");
    assert_eq!(fixture.disk("main.go"), MAIN);
}

#[test]
fn a_tab_whose_folder_is_gone_stays_open_with_its_changes() {
    let mut fixture = Fixture::new();
    let folder = fixture.path("pkg");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("pkg.go"), "package pkg\n").unwrap();
    fixture.edit("pkg/pkg.go", "// stays\n");
    std::fs::remove_dir_all(&folder).unwrap();
    let refused = fixture.fails("file.document.close", json!({}));
    assert!(refused.contains("pkg.go"), "{refused}");
    assert_eq!(fixture.tabs(), ["pkg.go"]);
    assert!(fixture.dirty("pkg/pkg.go"));
}

#[test]
fn close_other_tabs_saves_the_files_it_closes_and_keeps_the_chosen_tab() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// main\n");
    fixture.edit("util.go", "// util\n");
    fixture.open("lib.go");
    let closed = fixture.run("file.document.closeOthers", json!({ "index": 1 }));
    assert_eq!(fixture.names(&closed["closed"]), ["main.go", "lib.go"]);
    assert_eq!(fixture.names(&closed["saved"]), ["main.go"]);
    assert_eq!(fixture.tabs(), ["util.go"]);
    assert_eq!(fixture.disk("main.go"), format!("// main\n{MAIN}"));
    assert_eq!(fixture.disk("util.go"), UTIL, "the kept tab is not saved");
    assert!(fixture.dirty("util.go"));
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.active_document().map(|document| document.name()), Some("util.go".to_owned()));
}

#[test]
fn close_all_tabs_saves_every_file_it_closes() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// main\n");
    fixture.edit("util.go", "// util\n");
    fixture.open("lib.go");
    let closed = fixture.run("file.document.closeAll", json!({}));
    assert_eq!(fixture.names(&closed["closed"]), ["main.go", "util.go", "lib.go"]);
    assert_eq!(fixture.names(&closed["saved"]), ["main.go", "util.go"]);
    assert!(fixture.tabs().is_empty());
    assert!(fixture.session.state::<Workbench>().unwrap().documents().is_empty());
    assert_eq!(fixture.disk("main.go"), format!("// main\n{MAIN}"));
    assert_eq!(fixture.disk("util.go"), format!("// util\n{UTIL}"));
}

#[cfg(unix)]
#[test]
fn close_all_tabs_keeps_only_the_tabs_that_cannot_be_saved() {
    let mut fixture = Fixture::new();
    let folder = fixture.path("locked");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("locked.go"), "package locked\n").unwrap();
    fixture.edit("main.go", "// main\n");
    fixture.edit("locked/locked.go", "// locked\n");
    fixture.open("lib.go");
    read_only(&folder, true);
    let refused = fixture.fails("file.document.closeAll", json!({}));
    read_only(&folder, false);
    assert!(refused.contains("locked.go") && !refused.contains("main.go"), "{refused}");
    assert_eq!(fixture.tabs(), ["locked.go"], "the others closed");
    assert!(fixture.dirty("locked/locked.go"));
    assert_eq!(fixture.disk("main.go"), format!("// main\n{MAIN}"));
}

#[cfg(unix)]
#[test]
fn save_all_saves_every_file_it_can_and_names_the_one_it_cannot() {
    let mut fixture = Fixture::new();
    let folder = fixture.path("locked");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("locked.go"), "package locked\n").unwrap();
    fixture.edit("locked/locked.go", "// locked\n");
    fixture.edit("main.go", "// main\n");
    read_only(&folder, true);
    let refused = fixture.fails("file.document.saveAll", json!({}));
    read_only(&folder, false);
    assert!(refused.contains("locked.go") && !refused.contains("main.go"), "{refused}");
    assert_eq!(fixture.disk("main.go"), format!("// main\n{MAIN}"), "the file after it is saved");
    assert!(fixture.dirty("locked/locked.go") && !fixture.dirty("main.go"));
    fixture.run("file.document.saveAll", json!({}));
    assert_eq!(std::fs::read_to_string(folder.join("locked.go")).unwrap(), "// locked\npackage locked\n");
}

#[test]
fn save_never_writes_over_a_change_made_outside_the_ide() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::write(fixture.path("main.go"), "// theirs\n").unwrap();
    let refused = fixture.fails("file.document.save", json!({}));
    assert!(refused.contains("main.go"), "{refused}");
    assert_eq!(fixture.disk("main.go"), "// theirs\n");
    assert!(fixture.dirty("main.go"), "the changes stay in the editor");
    assert!(fixture.conflict("main.go"), "the editor asks which to keep");

    fixture.fails("file.document.save", json!({}));
    fixture.run("file.document.keepMine", json!({}));
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.disk("main.go"), format!("// mine\n{MAIN}"));
    assert!(!fixture.dirty("main.go") && !fixture.conflict("main.go"));
}

#[test]
fn save_all_saves_the_others_and_leaves_a_file_changed_outside_alone() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    fixture.edit("util.go", "// util\n");
    std::fs::write(fixture.path("main.go"), "// theirs\n").unwrap();
    let refused = fixture.fails("file.document.saveAll", json!({}));
    assert!(refused.contains("main.go") && !refused.contains("util.go"), "{refused}");
    assert_eq!(fixture.disk("main.go"), "// theirs\n");
    assert_eq!(fixture.disk("util.go"), format!("// util\n{UTIL}"));
    assert!(fixture.dirty("main.go") && !fixture.dirty("util.go"));
}

#[test]
fn closing_tabs_keeps_a_file_changed_outside_open_and_unsaved() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::write(fixture.path("main.go"), "// theirs\n").unwrap();
    let refused = fixture.fails("file.document.close", json!({}));
    assert!(refused.contains("main.go"), "{refused}");
    assert_eq!(fixture.tabs(), ["main.go"]);
    assert_eq!(fixture.disk("main.go"), "// theirs\n");
    assert!(fixture.dirty("main.go"));

    fixture.open("util.go");
    fixture.fails("file.document.closeAll", json!({}));
    assert_eq!(fixture.tabs(), ["main.go"]);
    assert_eq!(fixture.disk("main.go"), "// theirs\n");
}

#[test]
fn a_file_changed_into_something_unreadable_is_a_change_too() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::write(fixture.path("main.go"), [0xff, 0xfe, 0x00]).unwrap();
    fixture.fails("file.document.save", json!({}));
    assert_eq!(std::fs::read(fixture.path("main.go")).unwrap(), [0xff, 0xfe, 0x00]);
    fixture.run("file.document.keepMine", json!({}));
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.disk("main.go"), format!("// mine\n{MAIN}"), "Keep My Edits still settles it");
}

#[test]
fn a_change_after_keeping_mine_is_refused_again() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::write(fixture.path("main.go"), "// theirs\n").unwrap();
    fixture.fails("file.document.save", json!({}));
    fixture.run("file.document.keepMine", json!({}));
    std::fs::write(fixture.path("main.go"), "// theirs again\n").unwrap();
    fixture.fails("file.document.save", json!({}));
    assert_eq!(fixture.disk("main.go"), "// theirs again\n");
    assert!(fixture.conflict("main.go"));
}

#[test]
fn saving_an_unmodified_file_changed_outside_reads_it_instead() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    std::fs::write(fixture.path("main.go"), "// theirs\n").unwrap();
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.disk("main.go"), "// theirs\n");
    assert_eq!(fixture.text("main.go"), "// theirs\n");
    assert!(!fixture.dirty("main.go") && !fixture.conflict("main.go"));
}

#[test]
fn the_same_change_made_outside_is_no_conflict() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::write(fixture.path("main.go"), format!("// mine\n{MAIN}")).unwrap();
    fixture.run("file.document.save", json!({}));
    assert!(!fixture.dirty("main.go") && !fixture.conflict("main.go"));
}

#[test]
fn a_file_deleted_outside_is_written_again_on_save() {
    let mut fixture = Fixture::new();
    fixture.edit("main.go", "// mine\n");
    std::fs::remove_file(fixture.path("main.go")).unwrap();
    fixture.run("file.document.save", json!({}));
    assert_eq!(fixture.disk("main.go"), format!("// mine\n{MAIN}"));
}

#[test]
fn saving_keeps_line_endings_links_and_permissions() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.path("real.txt"), "one\r\ntwo\r\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(fixture.path("real.txt"), std::fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink("real.txt", fixture.path("link.txt")).unwrap();
        fixture.edit("link.txt", "zero\r\n");
        fixture.run("file.document.save", json!({}));
        assert!(std::fs::symlink_metadata(fixture.path("link.txt")).unwrap().file_type().is_symlink());
        let mode = std::fs::metadata(fixture.path("real.txt")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
        assert_eq!(fixture.disk("real.txt"), "zero\r\none\r\ntwo\r\n");
    }
}

#[test]
fn close_unmodified_tabs_keeps_the_tabs_with_unsaved_changes() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    fixture.edit("util.go", "// util\n");
    fixture.open("lib.go");
    let closed = fixture.run("file.document.closeUnmodified", json!({}));
    assert_eq!(fixture.names(&closed["closed"]), ["main.go", "lib.go"]);
    assert_eq!(closed["saved"], json!([]));
    assert_eq!(fixture.tabs(), ["util.go"]);
    assert!(fixture.dirty("util.go"), "still unsaved, and still open");
    assert_eq!(fixture.disk("util.go"), UTIL);
}

#[test]
fn closing_tabs_of_another_group_leaves_the_focused_one_alone() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    fixture.edit("util.go", "// util\n");
    let split = fixture.run("editor.split.right", json!({}));
    let right = split["focused"].as_u64().unwrap();
    fixture.open("lib.go");
    fixture.run("file.document.closeAll", json!({ "group": 1 }));
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert!(!workbench.groups().is_split(), "the emptied group goes");
    let tabs = &workbench.groups().group(right).unwrap().tabs;
    assert_eq!(tabs, &[fixture.path("util.go"), fixture.path("lib.go")]);
    assert_eq!(fixture.disk("util.go"), format!("// util\n{UTIL}"), "saved, though still open on the right");
}

#[test]
fn pinned_tabs_stay_at_the_start_and_survive_the_close_actions_that_spare_them() {
    let mut fixture = Fixture::new();
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    let group = fixture.run("editor.tab.pin", json!({ "index": 2 }));
    assert_eq!(group["pinned"], json!([fixture.path("lib.go")]));
    assert_eq!(fixture.tabs(), ["lib.go", "main.go", "util.go"], "a pinned tab goes to the start");
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.active_document().map(|document| document.name()), Some("lib.go".to_owned()));

    fixture.run("editor.tab.select", json!({ "index": 1 }));
    let closed = fixture.run("file.document.closeOthers", json!({}));
    assert_eq!(fixture.names(&closed["closed"]), ["util.go"]);
    assert_eq!(fixture.tabs(), ["lib.go", "main.go"], "Close Other Tabs keeps the pinned tab");

    fixture.open("util.go");
    fixture.run("file.document.closeAllButPinned", json!({}));
    assert_eq!(fixture.tabs(), ["lib.go"]);

    fixture.run("editor.tab.pin", json!({ "pinned": false }));
    fixture.run("file.document.closeAllButPinned", json!({}));
    assert!(fixture.tabs().is_empty(), "an unpinned tab closes");
}

#[test]
fn a_tab_moves_along_its_row_and_never_before_a_pinned_one() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.path("extra.go"), "package main\n").unwrap();
    for name in ["main.go", "util.go", "lib.go", "extra.go"] {
        fixture.open(name);
    }
    let group = fixture.run("editor.tab.move", json!({ "index": 0, "to": 2 }));
    assert_eq!(fixture.tabs(), ["util.go", "lib.go", "main.go", "extra.go"], "{group}");
    fixture.run("editor.tab.move", json!({ "index": 3, "to": 0 }));
    assert_eq!(fixture.tabs(), ["extra.go", "util.go", "lib.go", "main.go"]);
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.active_document().map(|document| document.name()), Some("extra.go".to_owned()));

    fixture.run("editor.tab.pin", json!({ "index": 2 }));
    assert_eq!(fixture.tabs(), ["lib.go", "extra.go", "util.go", "main.go"]);
    fixture.run("editor.tab.move", json!({ "index": 3, "to": 0 }));
    assert_eq!(fixture.tabs(), ["lib.go", "main.go", "extra.go", "util.go"], "an unpinned tab stops after the pinned");
    fixture.run("editor.tab.move", json!({ "index": 0, "to": 3 }));
    assert_eq!(fixture.tabs(), ["lib.go", "main.go", "extra.go", "util.go"], "a pinned tab stays among the pinned");
    fixture.run("editor.tab.pin", json!({ "index": 2 }));
    fixture.run("editor.tab.move", json!({ "index": 1, "to": 0 }));
    assert_eq!(fixture.tabs(), ["extra.go", "lib.go", "main.go", "util.go"], "among the pinned it moves");
    assert!(
        fixture.registry.dispatch(&mut fixture.session, "editor.tab.move", json!({ "index": 9, "to": 0 })).is_err()
    );
}

#[test]
fn close_tabs_to_the_left_and_right_spare_the_tab_and_the_pinned_ones() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.path("extra.go"), "package main\n").unwrap();
    for name in ["main.go", "util.go", "lib.go", "extra.go"] {
        fixture.open(name);
    }
    fixture.run("editor.tab.pin", json!({ "index": 0 }));
    let closed = fixture.run("file.document.closeLeft", json!({ "index": 2 }));
    assert_eq!(fixture.names(&closed["closed"]), ["util.go"]);
    assert_eq!(fixture.tabs(), ["main.go", "lib.go", "extra.go"]);
    let closed = fixture.run("file.document.closeRight", json!({ "index": 1 }));
    assert_eq!(fixture.names(&closed["closed"]), ["extra.go"]);
    assert_eq!(fixture.tabs(), ["main.go", "lib.go"]);
}

#[cfg(unix)]
#[test]
fn close_all_read_only_tabs_closes_only_files_that_cannot_be_written() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new();
    std::fs::set_permissions(fixture.path("util.go"), std::fs::Permissions::from_mode(0o444)).unwrap();
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    let closed = fixture.run("file.document.closeReadOnly", json!({}));
    assert_eq!(fixture.names(&closed["closed"]), ["util.go"]);
    assert_eq!(fixture.tabs(), ["main.go", "lib.go"]);
}

#[test]
fn reopen_closed_tab_brings_the_last_closed_file_back_at_its_place_and_pin() {
    let mut fixture = Fixture::new();
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    fixture.run("editor.tab.pin", json!({ "index": 1 }));
    fixture.run("file.document.close", json!({ "index": 2 }));
    fixture.run("file.document.close", json!({ "index": 0 }));
    assert_eq!(fixture.tabs(), ["main.go"]);

    let reopened = fixture.run("file.document.reopenClosed", json!({}));
    assert_eq!(reopened["pinned"], json!(true));
    assert_eq!(fixture.tabs(), ["util.go", "main.go"], "pinned again, at the start");
    fixture.run("file.document.reopenClosed", json!({}));
    assert_eq!(fixture.tabs(), ["util.go", "main.go", "lib.go"], "back at its place");
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.active_document().map(|document| document.name()), Some("lib.go".to_owned()));
    assert!(workbench.groups().focused_group().is_pinned(&fixture.path("util.go")));
    let empty = fixture.fails("file.document.reopenClosed", json!({}));
    assert!(!empty.is_empty());
}

#[test]
fn reopen_closed_tab_skips_files_that_are_gone_and_tabs_closed_together_come_back_one_by_one() {
    let mut fixture = Fixture::new();
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    fixture.run("file.document.closeOthers", json!({ "index": 0 }));
    std::fs::remove_file(fixture.path("util.go")).unwrap();
    fixture.run("file.document.close", json!({}));
    fixture.run("file.document.reopenClosed", json!({}));
    assert_eq!(fixture.tabs(), ["main.go"]);
    fixture.run("file.document.reopenClosed", json!({}));
    assert_eq!(fixture.tabs(), ["main.go", "lib.go"], "the removed util.go is skipped");
}

#[test]
fn select_next_and_previous_tab_go_round_the_group() {
    let mut fixture = Fixture::new();
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    let shown = fixture.run("editor.tab.next", json!({}));
    assert_eq!(Path::new(shown.as_str().unwrap()).file_name().unwrap(), "main.go", "from the last to the first");
    let shown = fixture.run("editor.tab.previous", json!({}));
    assert_eq!(Path::new(shown.as_str().unwrap()).file_name().unwrap(), "lib.go");
    let shown = fixture.run("editor.tab.previous", json!({}));
    assert_eq!(Path::new(shown.as_str().unwrap()).file_name().unwrap(), "util.go");
}

#[test]
fn move_to_opposite_group_moves_the_tab_with_its_pin_and_needs_a_split() {
    let mut fixture = Fixture::new();
    fixture.open("main.go");
    fixture.open("util.go");
    let refused = fixture.fails("editor.split.moveToOpposite", json!({}));
    assert!(!refused.is_empty());
    let split = fixture.run("editor.split.right", json!({ "index": 0 }));
    let right = split["focused"].as_u64().unwrap();
    fixture.run("editor.split.focus", json!({ "group": 1 }));
    fixture.run("editor.tab.pin", json!({ "index": 1 }));
    let groups = fixture.run("editor.split.moveToOpposite", json!({ "index": 0 }));
    assert_eq!(groups["focused"].as_u64(), Some(right));
    let workbench = fixture.session.state::<Workbench>().unwrap();
    let target = workbench.groups().group(right).unwrap();
    assert_eq!(target.tabs, [fixture.path("util.go"), fixture.path("main.go")]);
    assert!(target.is_pinned(&fixture.path("util.go")), "the pin goes with the tab");
}

fn tab_settings(fixture: &mut Fixture, change: impl FnOnce(&mut crate::tabs::EditorTabSettings)) {
    let mut settings = crate::tabs::EditorTabSettings::default();
    change(&mut settings);
    fixture.session.settings_mut().set(crate::tabs::SETTINGS, &settings).unwrap();
    crate::tabs::follow_policy(&mut fixture.session);
}

fn tab_names(fixture: &Fixture) -> Vec<String> {
    let workbench = fixture.session.state::<Workbench>().unwrap();
    let group = workbench.groups().focused_group();
    group.tabs.iter().map(|tab| tab.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

fn active_name(fixture: &Fixture) -> String {
    let workbench = fixture.session.state::<Workbench>().unwrap();
    workbench.active_document().unwrap().name()
}

#[test]
fn a_closed_active_tab_hands_over_as_the_closing_policy_says() {
    let mut fixture = Fixture::new();
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    fixture.run("editor.tab.select", json!({ "index": 1 }));
    fixture.run("file.document.close", json!({}));
    assert_eq!(active_name(&fixture), "main.go", "the tab on the left by default");

    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.activate_on_close = crate::tabs::ActivateOnClose::Right);
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    fixture.run("editor.tab.select", json!({ "index": 1 }));
    fixture.run("file.document.close", json!({}));
    assert_eq!(active_name(&fixture), "lib.go", "the tab on the right");

    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.activate_on_close = crate::tabs::ActivateOnClose::Recent);
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    fixture.run("editor.tab.select", json!({ "index": 0 }));
    fixture.run("editor.tab.select", json!({ "index": 1 }));
    fixture.run("file.document.close", json!({}));
    assert_eq!(active_name(&fixture), "main.go", "the tab used before it, not its neighbour on the right");
}

#[test]
fn new_tabs_open_beside_the_active_one_or_at_the_end_or_sorted() {
    let mut fixture = Fixture::new();
    for name in ["main.go", "util.go"] {
        fixture.open(name);
    }
    fixture.run("editor.tab.select", json!({ "index": 0 }));
    fixture.open("lib.go");
    assert_eq!(tab_names(&fixture), ["main.go", "util.go", "lib.go"], "at the end by default");

    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.open_at_end = false);
    for name in ["main.go", "util.go"] {
        fixture.open(name);
    }
    fixture.run("editor.tab.select", json!({ "index": 0 }));
    fixture.open("lib.go");
    assert_eq!(tab_names(&fixture), ["main.go", "lib.go", "util.go"], "right of the active tab");

    let mut fixture = Fixture::new();
    for name in ["util.go", "main.go", "lib.go"] {
        fixture.open(name);
    }
    tab_settings(&mut fixture, |tabs| tabs.sort_alphabetically = true);
    assert_eq!(tab_names(&fixture), ["lib.go", "main.go", "util.go"], "turning sorting on sorts the open tabs");
    assert_eq!(active_name(&fixture), "lib.go", "and keeps the active one");
    fixture.run("editor.tab.pin", json!({ "index": 2 }));
    assert_eq!(tab_names(&fixture), ["util.go", "lib.go", "main.go"], "pinned tabs still go first");
}

#[test]
fn keep_pinned_tabs_on_the_left_off_leaves_a_pinned_tab_in_its_place() {
    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.keep_pinned_left = false);
    for name in ["main.go", "util.go", "lib.go"] {
        fixture.open(name);
    }
    fixture.run("editor.tab.pin", json!({ "index": 2 }));
    assert_eq!(tab_names(&fixture), ["main.go", "util.go", "lib.go"]);
    tab_settings(&mut fixture, |tabs| tabs.keep_pinned_left = true);
    assert_eq!(tab_names(&fixture), ["lib.go", "main.go", "util.go"], "turning it on moves the pin to the left");
}

#[test]
fn the_tab_limit_closes_the_least_used_tab_and_spares_pinned_ones() {
    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.limit = 2);
    fixture.open("main.go");
    fixture.run("editor.tab.pin", json!({}));
    fixture.open("util.go");
    fixture.open("lib.go");
    assert_eq!(tab_names(&fixture), ["main.go", "lib.go"], "the pinned tab stays although it is the oldest");

    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.limit = 2);
    fixture.open("main.go");
    fixture.open("util.go");
    fixture.run("editor.tab.select", json!({ "index": 0 }));
    fixture.open("lib.go");
    assert_eq!(tab_names(&fixture), ["main.go", "lib.go"], "util.go was used last long ago");
    let reopened = fixture.run("file.document.reopenClosed", json!({}));
    assert!(reopened["path"].as_str().unwrap().ends_with("util.go"), "a tab closed by the limit can be reopened");
}

#[test]
fn over_the_limit_close_unchanged_spares_a_modified_tab_and_close_unused_saves_it() {
    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| {
        tabs.limit = 2;
        tabs.over_limit = crate::tabs::OverLimit::CloseUnchanged;
    });
    fixture.edit("main.go", "// edited\n");
    fixture.open("util.go");
    fixture.open("lib.go");
    assert_eq!(tab_names(&fixture), ["main.go", "lib.go"], "the unchanged tab goes, not the older modified one");

    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.limit = 2);
    fixture.edit("main.go", "// edited\n");
    fixture.open("util.go");
    fixture.open("lib.go");
    assert_eq!(tab_names(&fixture), ["util.go", "lib.go"]);
    let saved = std::fs::read_to_string(fixture.path("main.go")).unwrap();
    assert!(saved.contains("// edited"), "its changes are saved, as closing a tab saves them");
}

#[test]
fn a_preview_tab_is_reused_and_an_edit_or_keep_open_makes_it_permanent() {
    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.preview = true);
    fixture.open("lib.go");
    let (main, util) = (fixture.path("main.go"), fixture.path("util.go"));
    fixture.run("file.document.open", json!({ "path": main, "preview": true }));
    fixture.run("file.document.open", json!({ "path": util, "preview": true }));
    assert_eq!(tab_names(&fixture), ["lib.go", "util.go"], "the second file takes over the tab");
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.documents().len(), 2, "and the first one's document goes with it");
    assert!(workbench.groups().focused_group().is_preview(&util));

    fixture.run("editor.tab.keepOpen", json!({}));
    fixture.run("file.document.open", json!({ "path": main, "preview": true }));
    assert_eq!(tab_names(&fixture), ["lib.go", "util.go", "main.go"], "a kept tab is not reused");

    fixture.run("editor.edit.insert", json!({ "text": "// edited\n" }));
    let group = fixture.session.state::<Workbench>().unwrap().groups().focused();
    fixture.session.state_mut::<Workbench>().present(group);
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.groups().focused_group().preview, None, "an edit makes it permanent");

    fixture.run("file.document.open", json!({ "path": fixture.path("lib.go"), "preview": true }));
    assert_eq!(tab_names(&fixture).len(), 3, "a file that has a tab goes there");
}

#[test]
fn opening_a_previewed_file_for_good_keeps_it_and_without_the_setting_there_is_no_preview() {
    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.preview = true);
    let main = fixture.path("main.go");
    fixture.run("file.document.open", json!({ "path": main, "preview": true }));
    fixture.run("file.document.open", json!({ "path": main }));
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.groups().focused_group().preview, None);

    let mut fixture = Fixture::new();
    let main = fixture.path("main.go");
    fixture.run("file.document.open", json!({ "path": main, "preview": true }));
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.groups().focused_group().preview, None, "the setting is off by default");
}

#[test]
fn open_declaration_source_in_the_same_tab_replaces_a_clean_unpinned_tab() {
    let mut fixture = Fixture::new();
    tab_settings(&mut fixture, |tabs| tabs.declaration_in_same_tab = true);
    fixture.open("main.go");
    let target = fixture.path("lib.go");
    let left = crate::tabs::tab_to_replace(&fixture.session, &target).expect("the tab can be replaced");
    fixture.run("navigate.place.goTo", json!({ "path": target, "offset": 0 }));
    crate::tabs::replace_left_tab(&mut fixture.session, &left, &target);
    assert_eq!(tab_names(&fixture), ["lib.go"], "the declaration took the tab over");

    fixture.run("editor.tab.pin", json!({}));
    assert_eq!(crate::tabs::tab_to_replace(&fixture.session, &fixture.path("util.go")), None, "not a pinned tab");
    fixture.run("editor.tab.pin", json!({ "pinned": false }));
    fixture.edit("lib.go", "// edited\n");
    assert_eq!(crate::tabs::tab_to_replace(&fixture.session, &fixture.path("util.go")), None, "nor a modified one");

    tab_settings(&mut fixture, |tabs| tabs.declaration_in_same_tab = false);
    fixture.run("file.document.save", json!({}));
    assert_eq!(crate::tabs::tab_to_replace(&fixture.session, &fixture.path("util.go")), None, "off by default");
}
