use egui::PointerButton;
use serde_json::json;

use super::{at, button, click_at, shows, window};

#[test]
fn an_open_workspace_has_a_tab_in_the_editor_strip_that_expands_it_and_closes_it() {
    let mut window = window();
    window.run("workspace.tab.open", json!({ "workspace": "test", "kind": "history", "title": "Query History" }));
    let texts = window.frames(2);
    let tab = at(&texts, "Test");
    assert!(tab.y < at(&texts, "Query History").y, "the workspace's tab is in the editor strip above the host");
    assert!(shows(&texts, "other.txt"), "beside the code's tabs");
    assert_eq!(window.state()["open"], true);

    window.run("workspace.host.collapse", json!({}));
    let texts = window.frames(2);
    assert!(shows(&texts, "other text"), "collapsed: the code is back");
    assert_eq!(window.state()["open"], true, "and the workspace's tab stays");
    let tab = at(&texts, "Test");
    click_at(&mut window, tab + egui::vec2(4.0, 0.0));
    assert_eq!(window.state()["expanded"], true, "a click on the tab expands it");
    let texts = window.frames(2);
    assert!(shows(&texts, "View of Query History"), "with its tabs as left");

    let document = at(&texts, "other.txt");
    click_at(&mut window, document + egui::vec2(4.0, 0.0));
    assert_eq!(window.state()["expanded"], false, "a click on a code tab collapses it");

    let texts = window.frames(2);
    let tab = at(&texts, "Test") + egui::vec2(4.0, 0.0);
    window.frame_after(0.05, vec![egui::Event::PointerMoved(tab), button(tab, PointerButton::Middle, true)]);
    window.frame_after(0.05, vec![button(tab, PointerButton::Middle, false)]);
    let texts = window.frames(2);
    let state = window.state();
    assert_eq!((state["open"].clone(), state["expanded"].clone()), (json!(false), json!(false)), "{state}");
    assert!(!texts.iter().any(|(_, text)| text == "Test"), "closing takes the tab away");
    assert_eq!(state["tabs"][0]["title"], "Query History", "and keeps the inner tabs: {state}");
    window.run("workspace.host.toggle", json!({ "id": "test" }));
    let texts = window.frames(2);
    assert!(shows(&texts, "View of Query History"), "the stripe button opens it again as left");
}
