use std::time::Duration;

use serde_json::{Value, json};
use zen_i18n::t;
use zen_lsp::fake::Reply;

use crate::language::testing::{Fixture, fixture};

const CART: &str = "package cart\n\n// Сумма 😀\nfunc Total(prices []int) int { return sum(prices) }\n\nfunc sum(values []int) int { return len(values) }\n";
const MAIN: &str = "package cart\n\nfunc Run() int { return Total(nil) }\n";
const TEST: &str = "package cart\n\nfunc TestTotal(t *testing.T) { Total(nil) }\n";
const LIBRARY: &str = "package fmt\n\nfunc Print() { cart.Total(nil) }\n";
const TYPES: &str =
    "package cart\n\ntype Store interface { Get() }\n\ntype Memory struct{}\n\nfunc (Memory) Get() {}\n";
const WAIT: Duration = Duration::from_secs(5);

fn settled(fixture: &mut Fixture, command: &str, params: Value) -> Value {
    let mut state = fixture.dispatch(command, params).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while state["pending"] == true && std::time::Instant::now() < deadline {
        let tab = state["tab"].clone();
        let get = if tab.is_null() { json!({ "waitMs": 10_000 }) } else { json!({ "tab": tab, "waitMs": 10_000 }) };
        state = fixture.dispatch("navigate.hierarchy.get", get).unwrap();
    }
    state
}

fn hierarchies() -> Value {
    json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "definitionProvider": true,
        "callHierarchyProvider": true,
        "typeHierarchyProvider": true
    })
}

fn project() -> (Fixture, tempfile::TempDir) {
    let fixture = fixture(&[("cart.go", CART), ("main.go", MAIN), ("cart_test.go", TEST), ("types.go", TYPES)]);
    let library = tempfile::tempdir().unwrap();
    std::fs::write(library.path().join("print.go"), LIBRARY).unwrap();
    fixture.server.set_capabilities(hierarchies());
    (fixture, library)
}

fn chars(text: &str, needle: &str) -> usize {
    text[..text.find(needle).unwrap()].chars().count()
}

fn utf16(text: &str, at: usize) -> Value {
    let before = &text[..at];
    let line = before.matches('\n').count();
    let column = before[before.rfind('\n').map_or(0, |at| at + 1)..].encode_utf16().count();
    json!({ "line": line, "character": column })
}

fn range(text: &str, needle: &str) -> Value {
    let at = text.find(needle).unwrap();
    json!({ "start": utf16(text, at), "end": utf16(text, at + needle.len()) })
}

fn item(path: &std::path::Path, text: &str, name: &str, kind: u64, declaration: &str) -> Value {
    let line = text.lines().find(|line| line.contains(declaration)).unwrap();
    json!({
        "name": name,
        "kind": kind,
        "detail": "example.com/shop/cart • cart.go",
        "uri": zen_lsp::position::file_uri(path),
        "range": range(text, line),
        "selectionRange": range(text, declaration),
    })
}

fn names(state: &Value) -> Vec<String> {
    state["nodes"].as_array().unwrap().iter().map(|node| node["name"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn callers_are_asked_for_one_node_at_a_time_and_filtered_by_scope() {
    let (mut fixture, library) = project();
    let total = item(&fixture.path("cart.go"), CART, "Total", 12, "Total");
    let run = item(&fixture.path("main.go"), MAIN, "Run", 12, "Run");
    let test = item(&fixture.path("cart_test.go"), TEST, "TestTotal", 12, "TestTotal");
    let print = item(&library.path().join("print.go"), LIBRARY, "Print", 12, "Print");
    fixture.server.on("textDocument/prepareCallHierarchy", json!([total]));
    let callers = json!([
        { "from": run, "fromRanges": [range(MAIN, "Total")] },
        { "from": test, "fromRanges": [range(TEST, "Total")] },
        { "from": print, "fromRanges": [range(LIBRARY, "Total")] },
    ]);
    fixture.server.on_with("callHierarchy/incomingCalls", move |params| {
        if params["item"]["name"] == "Total" { callers.clone() } else { json!(null) }
    });
    fixture.open("cart.go");
    let cart = fixture.path("cart.go");
    let at = chars(CART, "Total") + 2;
    let state = settled(&mut fixture, "navigate.hierarchy.call", json!({ "path": cart, "offset": at, "waitMs": 5000 }));
    assert_eq!(
        fixture.server.requests("textDocument/prepareCallHierarchy")[0]["position"],
        utf16(CART, CART.find("Total").unwrap() + 2)
    );
    assert_eq!(
        (state["open"].clone(), state["kind"].clone(), state["direction"].clone()),
        (json!(true), json!("call"), json!("callers"))
    );
    assert_eq!(
        (state["pending"].clone(), state["supported"].clone(), state["symbol"].clone()),
        (json!(false), json!(true), json!("Total"))
    );
    assert_eq!(names(&state), ["Total", "Run", "TestTotal", "Print"]);
    let root = &state["nodes"][0];
    assert_eq!(root["kind"], "function");
    assert_eq!(root["detail"], "example.com/shop/cart • cart.go");
    assert_eq!(root["path"], json!(cart));
    assert_eq!(root["selection"]["line"], 4);
    assert_eq!(root["selection"]["column"], 6);
    assert_eq!(root["range"], json!([chars(CART, "func Total"), chars(CART, "\n\nfunc sum")]));
    assert_eq!(root["children"], json!([1, 2, 3]));
    let locations: Vec<&Value> = state["nodes"].as_array().unwrap().iter().map(|node| &node["location"]).collect();
    assert_eq!(locations, [&json!("project"), &json!("project"), &json!("test"), &json!("library")]);
    let run = &state["nodes"][1];
    assert_eq!(run["parent"], 0);
    assert_eq!(run["children"], json!(null), "a child is expanded only when asked");
    assert_eq!(run["calls"][0]["path"], json!(fixture.path("main.go")));
    assert_eq!(run["calls"][0]["start"], chars(MAIN, "Total"));
    assert_eq!(run["calls"][0]["preview"], "func Run() int { return Total(nil) }");
    assert_eq!(fixture.server.requests("callHierarchy/incomingCalls").len(), 1);

    let state = settled(&mut fixture, "navigate.hierarchy.expand", json!({ "node": 1, "waitMs": 5000 }));
    let asked = fixture.server.requests("callHierarchy/incomingCalls");
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[1]["item"]["name"], "Run");
    assert_eq!(state["nodes"][1]["children"], json!([]));
    settled(&mut fixture, "navigate.hierarchy.expand", json!({ "node": 1, "waitMs": 5000 }));
    assert_eq!(fixture.server.requests("callHierarchy/incomingCalls").len(), 2, "children are asked for once");
    assert!(fixture.dispatch("navigate.hierarchy.expand", json!({ "node": 9 })).is_err());

    let state = fixture.dispatch("navigate.hierarchy.setOptions", json!({ "scope": "test" })).unwrap();
    assert_eq!(names(&state), ["Total", "TestTotal"]);
    assert_eq!((state["scope"].clone(), state["hidden"].clone()), (json!("test"), json!(2)));
    assert_eq!(state["nodes"][0]["children"], json!([2]));
    let state = fixture.dispatch("navigate.hierarchy.setOptions", json!({ "scope": "project" })).unwrap();
    assert_eq!(names(&state), ["Total", "Run"]);
    let state = fixture.dispatch("navigate.hierarchy.get", json!({})).unwrap();
    assert_eq!(state["scope"], "project", "the scope stays");
    let state = fixture.dispatch("navigate.hierarchy.setOptions", json!({ "scope": "all" })).unwrap();
    assert_eq!(names(&state).len(), 4);
}

#[test]
fn callees_replace_the_children_and_keep_the_roots() {
    let (mut fixture, _library) = project();
    let cart = fixture.path("cart.go");
    let total = item(&cart, CART, "Total", 12, "Total");
    let sum = item(&cart, CART, "sum", 12, "sum(values");
    fixture.server.on("textDocument/prepareCallHierarchy", json!([total]));
    fixture.server.on("callHierarchy/incomingCalls", json!([]));
    fixture.server.on("callHierarchy/outgoingCalls", json!([{ "to": sum, "fromRanges": [range(CART, "sum(prices")] }]));
    fixture.open("cart.go");
    let state =
        settled(&mut fixture, "navigate.hierarchy.call", json!({ "offset": chars(CART, "Total"), "waitMs": 5000 }));
    assert_eq!(state["nodes"][0]["children"], json!([]));
    assert!(fixture.dispatch("navigate.hierarchy.setOptions", json!({ "direction": "subtypes" })).is_err());
    let state =
        settled(&mut fixture, "navigate.hierarchy.setOptions", json!({ "direction": "callees", "waitMs": 5000 }));
    assert_eq!(state["direction"], "callees");
    assert_eq!(names(&state), ["Total", "sum"]);
    assert_eq!(state["nodes"][1]["id"], 1);
    let calls = &state["nodes"][1]["calls"];
    assert_eq!(calls[0]["path"], json!(cart), "a callee's calls are in its caller's file");
    assert_eq!(calls[0]["start"], chars(CART, "sum(prices"));
    assert_eq!(fixture.server.requests("callHierarchy/outgoingCalls")[0]["item"]["name"], "Total");
    let state = settled(
        &mut fixture,
        "navigate.hierarchy.call",
        json!({ "offset": chars(CART, "Total"), "direction": "callees", "waitMs": 5000 }),
    );
    assert_eq!(names(&state), ["Total", "sum"]);
}

#[test]
fn a_type_shows_its_interfaces_and_an_interface_its_types() {
    let (mut fixture, _library) = project();
    let types = fixture.path("types.go");
    let store = item(&types, TYPES, "Store", 11, "Store");
    let memory = item(&types, TYPES, "Memory", 23, "Memory struct");
    let (on_store, on_memory) = (store.clone(), memory.clone());
    fixture.server.on_with("textDocument/prepareTypeHierarchy", move |params| {
        if params["position"]["line"] == 2 { json!([on_store]) } else { json!([on_memory]) }
    });
    fixture.server.on("typeHierarchy/subtypes", json!([memory]));
    fixture.server.on("typeHierarchy/supertypes", json!([store]));
    fixture.open("types.go");

    let state =
        settled(&mut fixture, "navigate.hierarchy.type", json!({ "offset": chars(TYPES, "Store"), "waitMs": 5000 }));
    assert_eq!((state["kind"].clone(), state["direction"].clone()), (json!("type"), json!("subtypes")));
    assert_eq!(names(&state), ["Store", "Memory"]);
    assert_eq!(state["nodes"][0]["kind"], "interface");
    assert_eq!(state["nodes"][1]["kind"], "struct");
    assert_eq!(state["nodes"][1]["calls"], json!([]));

    let state =
        settled(&mut fixture, "navigate.hierarchy.type", json!({ "offset": chars(TYPES, "Memory"), "waitMs": 5000 }));
    assert_eq!(state["direction"], "supertypes");
    assert_eq!(names(&state), ["Memory", "Store"]);
    let state =
        settled(&mut fixture, "navigate.hierarchy.setOptions", json!({ "direction": "subtypes", "waitMs": 5000 }));
    assert_eq!(names(&state), ["Memory", "Memory"]);
    assert_eq!(fixture.server.requests("typeHierarchy/subtypes").len(), 2);
    assert!(fixture.dispatch("navigate.hierarchy.type", json!({ "direction": "callers" })).is_err());
}

#[test]
fn nothing_at_the_place_is_said_in_words() {
    let (mut fixture, _library) = project();
    fixture.server.on("textDocument/prepareCallHierarchy", json!(null));
    fixture.open("main.go");
    let state = settled(&mut fixture, "navigate.hierarchy.call", json!({ "offset": 0, "waitMs": 5000 }));
    assert_eq!(state["error"], t!("workbench-hierarchy-no-call"));
    assert_eq!(state["roots"], json!([]));
    assert_eq!(state["supported"], true);
}

#[test]
fn a_server_without_the_hierarchies_is_reported_not_failed() {
    let (mut fixture, _library) = project();
    fixture.server.set_capabilities(
        json!({ "textDocumentSync": { "openClose": true, "change": 2 }, "typeHierarchyProvider": true }),
    );
    fixture.server.reply("typeHierarchy/supertypes", Reply::Error { code: -32601, message: "no".to_owned() });
    let types = fixture.path("types.go");
    fixture.server.on("textDocument/prepareTypeHierarchy", json!([item(&types, TYPES, "Memory", 23, "Memory struct")]));
    fixture.open("types.go");
    let state = settled(&mut fixture, "navigate.hierarchy.call", json!({ "offset": 3, "waitMs": 5000 }));
    assert_eq!((state["open"].clone(), state["supported"].clone()), (json!(true), json!(false)));
    assert_eq!(state["error"], t!("workbench-hierarchy-unsupported-call", server = "go".to_owned()));
    assert!(fixture.server.requests("textDocument/prepareCallHierarchy").is_empty(), "nothing is asked");

    let state =
        settled(&mut fixture, "navigate.hierarchy.type", json!({ "offset": chars(TYPES, "Memory"), "waitMs": 5000 }));
    assert_eq!(state["supported"], false);
    assert_eq!(state["nodes"][0]["error"], t!("workbench-hierarchy-unsupported-type", server = "go".to_owned()));
}

#[test]
fn requests_are_cancelled_by_node_by_hierarchy_and_by_a_new_one() {
    let (mut fixture, _library) = project();
    let total = item(&fixture.path("cart.go"), CART, "Total", 12, "Total");
    fixture.server.reply("textDocument/prepareCallHierarchy", Reply::Hold);
    fixture.open("cart.go");
    let params = json!({ "offset": chars(CART, "Total") });
    let state = fixture.dispatch("navigate.hierarchy.call", params.clone()).unwrap();
    assert_eq!(state["pending"], true);
    fixture.server.wait_for("textDocument/prepareCallHierarchy", 1, WAIT);
    let state = fixture.dispatch("navigate.hierarchy.cancel", json!({})).unwrap();
    assert_eq!(state["pending"], false);
    assert_eq!(fixture.server.wait_for("$/cancelRequest", 1, WAIT).len(), 1);

    fixture.server.on("textDocument/prepareCallHierarchy", json!([total]));
    fixture.server.reply("callHierarchy/incomingCalls", Reply::Hold);
    let state =
        fixture.dispatch("navigate.hierarchy.call", json!({ "offset": chars(CART, "Total"), "waitMs": 200 })).unwrap();
    assert_eq!(state["nodes"][0]["pending"], true);
    fixture.server.wait_for("callHierarchy/incomingCalls", 1, WAIT);
    let state = fixture.dispatch("navigate.hierarchy.cancel", json!({ "node": 0 })).unwrap();
    assert_eq!(
        (state["nodes"][0]["pending"].clone(), state["nodes"][0]["children"].clone()),
        (json!(false), json!(null))
    );
    assert_eq!(fixture.server.wait_for("$/cancelRequest", 2, WAIT).len(), 2);

    fixture.dispatch("navigate.hierarchy.expand", json!({ "node": 0 })).unwrap();
    fixture.server.wait_for("callHierarchy/incomingCalls", 2, WAIT);
    fixture.dispatch("navigate.hierarchy.call", params).unwrap();
    assert_eq!(fixture.server.wait_for("$/cancelRequest", 3, WAIT).len(), 3, "a new hierarchy cancels the old one's");
    fixture.dispatch("navigate.hierarchy.close", json!({})).unwrap();
    assert_eq!(fixture.dispatch("navigate.hierarchy.get", json!({})).unwrap()["open"], false);
    assert!(fixture.dispatch("navigate.hierarchy.expand", json!({ "node": 0 })).is_err());
}

#[test]
fn ids_are_never_reused_and_a_refresh_after_edits_expands_the_same_nodes_again() {
    let (mut fixture, _library) = project();
    let main = fixture.path("main.go");
    let total = item(&fixture.path("cart.go"), CART, "Total", 12, "Total");
    let run = item(&main, MAIN, "Run", 12, "Run");
    fixture.server.on("textDocument/prepareCallHierarchy", json!([total]));
    let callers = json!([{ "from": run, "fromRanges": [range(MAIN, "Total")] }]);
    fixture.server.on_with("callHierarchy/incomingCalls", move |params| {
        if params["item"]["name"] == "Total" { callers.clone() } else { json!([]) }
    });
    fixture.server.on("callHierarchy/outgoingCalls", json!([]));
    fixture.open("cart.go");
    settled(&mut fixture, "navigate.hierarchy.call", json!({ "offset": chars(CART, "Total"), "waitMs": 5000 }));
    let state = settled(&mut fixture, "navigate.hierarchy.expand", json!({ "node": 1, "waitMs": 5000 }));
    assert_eq!(state["nodes"][1]["children"], json!([]));

    let state =
        settled(&mut fixture, "navigate.hierarchy.setOptions", json!({ "direction": "callees", "waitMs": 5000 }));
    assert_eq!(names(&state), ["Total"]);
    assert!(fixture.dispatch("navigate.hierarchy.expand", json!({ "node": 1 })).is_err(), "the callers are gone");
    let state =
        settled(&mut fixture, "navigate.hierarchy.setOptions", json!({ "direction": "callers", "waitMs": 5000 }));
    assert_eq!(state["nodes"][1]["id"], 2, "a caller had again gets a new id");
    settled(&mut fixture, "navigate.hierarchy.expand", json!({ "node": 2, "waitMs": 5000 }));
    assert_eq!(fixture.server.requests("callHierarchy/incomingCalls").len(), 4);

    fixture.with(|_, workbench| {
        workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "// Totals.\n\n".to_owned())]);
    });
    let state = settled(&mut fixture, "navigate.hierarchy.refresh", json!({ "waitMs": 5000 }));
    let edited = format!("// Totals.\n\n{CART}");
    let asked = fixture.server.requests("textDocument/prepareCallHierarchy");
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[1]["position"], utf16(&edited, edited.find("Total(").unwrap()), "the root is found again");
    assert_eq!((state["kind"].clone(), state["direction"].clone()), (json!("call"), json!("callers")));
    assert_eq!(names(&state), ["Total", "Run"]);
    assert_eq!(state["roots"], json!([3]));
    assert_eq!(state["nodes"][1]["children"], json!([]), "what was expanded is expanded again");
    let asked = fixture.server.requests("callHierarchy/incomingCalls");
    assert_eq!(asked.len(), 6);
    assert_eq!(asked[5]["item"]["name"], "Run");
    assert!(fixture.dispatch("navigate.hierarchy.expand", json!({ "node": 0 })).is_err());

    fixture.dispatch("navigate.hierarchy.close", json!({})).unwrap();
    let state =
        settled(&mut fixture, "navigate.hierarchy.call", json!({ "offset": chars(CART, "Total"), "waitMs": 5000 }));
    assert_eq!(state["roots"], json!([5]), "nor after a close");
}

#[test]
fn an_error_of_the_server_at_the_place_means_nothing_to_show() {
    let (mut fixture, _library) = project();
    fixture
        .server
        .reply("textDocument/prepareTypeHierarchy", Reply::Error { code: 0, message: "not a type name".to_owned() });
    fixture.open("types.go");
    let state = settled(&mut fixture, "navigate.hierarchy.type", json!({ "offset": 0, "waitMs": 5000 }));
    assert_eq!(
        (state["error"].clone(), state["supported"].clone()),
        (json!(t!("workbench-hierarchy-no-type")), json!(true))
    );
}

const RECURSIVE: &str =
    "package cart\n\nfunc Walk(n int) int { if n == 0 { return 0 }; return Walk(n - 1) }\n\nfunc Use() { Walk(3) }\n";

#[test]
fn a_new_hierarchy_replaces_the_unpinned_tab_and_pinned_tabs_stay_and_work() {
    let (mut fixture, _library) = project();
    let cart = fixture.path("cart.go");
    let types = fixture.path("types.go");
    let total = item(&cart, CART, "Total", 12, "Total");
    let sum = item(&cart, CART, "sum", 12, "sum(values");
    let run = item(&fixture.path("main.go"), MAIN, "Run", 12, "Run");
    let (on_total, on_sum) = (total.clone(), sum.clone());
    fixture.server.on_with("textDocument/prepareCallHierarchy", move |params| {
        if params["position"]["line"] == 3 { json!([on_total]) } else { json!([on_sum]) }
    });
    let callers = json!([{ "from": run, "fromRanges": [range(MAIN, "Total")] }]);
    fixture.server.on_with("callHierarchy/incomingCalls", move |params| {
        if params["item"]["name"] == "Total" { callers.clone() } else { json!([]) }
    });
    fixture.server.on("textDocument/prepareTypeHierarchy", json!([item(&types, TYPES, "Store", 11, "Store")]));
    fixture.server.on("typeHierarchy/subtypes", json!([]));
    fixture.open("cart.go");
    let at_total = json!({ "path": cart, "offset": chars(CART, "Total"), "waitMs": 5000 });
    let first = fixture.dispatch("navigate.hierarchy.call", at_total.clone()).unwrap();
    assert_eq!(
        (first["tab"].clone(), first["active"].clone(), first["pinned"].clone()),
        (json!(0), json!(0), json!(false))
    );
    let second = settled(
        &mut fixture,
        "navigate.hierarchy.call",
        json!({ "path": cart, "offset": chars(CART, "sum(values"), "waitMs": 5000 }),
    );
    assert_eq!(second["tab"], 0, "an unpinned tab is replaced");
    assert_eq!(second["tabs"].as_array().unwrap().len(), 1);
    assert_eq!(second["symbol"], "sum");

    let pinned = fixture.dispatch("navigate.hierarchy.pin", json!({})).unwrap();
    assert_eq!((pinned["tab"].clone(), pinned["pinned"].clone()), (json!(0), json!(true)));
    let third = fixture.dispatch("navigate.hierarchy.call", at_total).unwrap();
    assert_eq!((third["tab"].clone(), third["active"].clone()), (json!(1), json!(1)), "a pinned tab stays");
    let tabs = third["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 2);
    assert_eq!((tabs[0]["symbol"].clone(), tabs[0]["pinned"].clone()), (json!("sum"), json!(true)));
    assert_eq!((tabs[1]["symbol"].clone(), tabs[1]["pinned"].clone()), (json!("Total"), json!(false)));
    let run_id = third["nodes"][1]["id"].as_u64().unwrap();

    let selected = fixture.dispatch("navigate.hierarchy.select", json!({ "tab": 0 })).unwrap();
    assert_eq!(
        (selected["tab"].clone(), selected["active"].clone(), selected["symbol"].clone()),
        (json!(0), json!(0), json!("sum"))
    );
    let typed = settled(
        &mut fixture,
        "navigate.hierarchy.type",
        json!({ "path": types, "offset": chars(TYPES, "Store"), "waitMs": 5000 }),
    );
    assert_eq!(typed["tab"], 1, "with the pinned tab selected, the unpinned one is replaced");
    assert_eq!(typed["kind"], "type");
    assert!(fixture.dispatch("navigate.hierarchy.expand", json!({ "node": run_id })).is_err(), "its nodes are gone");

    let refreshed = settled(&mut fixture, "navigate.hierarchy.refresh", json!({ "tab": 0, "waitMs": 5000 }));
    assert_eq!(
        (refreshed["tab"].clone(), refreshed["pinned"].clone()),
        (json!(0), json!(true)),
        "a pinned tab refreshes"
    );
    assert_eq!(
        fixture.dispatch("navigate.hierarchy.get", json!({})).unwrap()["active"],
        1,
        "refresh leaves the selection"
    );

    fixture.dispatch("navigate.hierarchy.close", json!({ "tab": 1 })).unwrap();
    let state = fixture.dispatch("navigate.hierarchy.get", json!({})).unwrap();
    assert_eq!((state["active"].clone(), state["tab"].clone()), (json!(0), json!(0)), "the tab before is selected");
    fixture.dispatch("navigate.hierarchy.pin", json!({ "pinned": false })).unwrap();
    let state = settled(
        &mut fixture,
        "navigate.hierarchy.call",
        json!({ "path": cart, "offset": chars(CART, "Total"), "waitMs": 5000 }),
    );
    assert_eq!(
        (state["tab"].clone(), state["tabs"].as_array().unwrap().len()),
        (json!(0), 1),
        "an unpinned tab is replaced again"
    );
    assert!(fixture.dispatch("navigate.hierarchy.get", json!({ "tab": 7 })).is_err());
}

#[test]
fn recursion_is_never_expanded_and_expand_all_and_collapse_all_work_on_the_tree() {
    let (mut fixture, _library) = project();
    std::fs::write(fixture.path("walk.go"), RECURSIVE).unwrap();
    let walk_path = fixture.path("walk.go");
    let walk = item(&walk_path, RECURSIVE, "Walk", 12, "Walk(n int)");
    let using = item(&walk_path, RECURSIVE, "Use", 12, "Use()");
    fixture.server.on("textDocument/prepareCallHierarchy", json!([walk.clone()]));
    let callers = json!([
        { "from": using, "fromRanges": [range(RECURSIVE, "Walk(3)")] },
        { "from": walk, "fromRanges": [range(RECURSIVE, "Walk(n - 1)")] },
    ]);
    fixture.server.on_with("callHierarchy/incomingCalls", move |params| {
        if params["item"]["name"] == "Walk" { callers.clone() } else { json!([]) }
    });
    fixture.open("walk.go");
    let state = settled(
        &mut fixture,
        "navigate.hierarchy.call",
        json!({ "offset": chars(RECURSIVE, "Walk(n int)"), "waitMs": 5000 }),
    );
    assert_eq!(names(&state), ["Walk", "Use", "Walk"]);
    let recursive = &state["nodes"][2];
    assert_eq!((recursive["recursive"].clone(), recursive["children"].clone()), (json!(true), json!([])));
    let recursive_id = recursive["id"].as_u64().unwrap();
    settled(&mut fixture, "navigate.hierarchy.expand", json!({ "node": recursive_id, "waitMs": 5000 }));
    assert_eq!(
        fixture.server.requests("callHierarchy/incomingCalls").len(),
        1,
        "a recursive call is never asked about"
    );

    let state = settled(&mut fixture, "navigate.hierarchy.expandAll", json!({ "waitMs": 5000 }));
    assert_eq!(state["pending"], false);
    assert_eq!(
        fixture.server.requests("callHierarchy/incomingCalls").len(),
        2,
        "Use is expanded, the recursion is not"
    );
    assert_eq!(state["nodes"][1]["children"], json!([]));

    let state = fixture.dispatch("navigate.hierarchy.collapse", json!({})).unwrap();
    assert_eq!(names(&state), ["Walk"], "Collapse All leaves the roots");
    assert_eq!(state["nodes"][0]["collapsed"], true);
    let state = fixture.dispatch("navigate.hierarchy.expand", json!({ "node": state["roots"][0] })).unwrap();
    assert_eq!(names(&state), ["Walk", "Use", "Walk"], "expanded again without asking");
    assert_eq!(fixture.server.requests("callHierarchy/incomingCalls").len(), 2);
    let use_id = state["nodes"][1]["id"].clone();
    let state = fixture.dispatch("navigate.hierarchy.collapse", json!({ "node": use_id })).unwrap();
    assert_eq!(state["nodes"][1]["collapsed"], true);
    assert_eq!(state["nodes"][0]["collapsed"], false);
}

#[test]
fn sort_and_single_click_are_options_kept_for_the_next_hierarchy() {
    let (mut fixture, _library) = project();
    let state =
        fixture.dispatch("navigate.hierarchy.setOptions", json!({ "sort": true, "singleClick": true })).unwrap();
    assert_eq!(
        (state["open"].clone(), state["sort"].clone(), state["singleClick"].clone()),
        (json!(false), json!(true), json!(true))
    );
    let total = item(&fixture.path("cart.go"), CART, "Total", 12, "Total");
    let run = item(&fixture.path("main.go"), MAIN, "Run", 12, "Run");
    let test = item(&fixture.path("cart_test.go"), TEST, "TestTotal", 12, "TestTotal");
    fixture.server.on("textDocument/prepareCallHierarchy", json!([total]));
    fixture.server.on(
        "callHierarchy/incomingCalls",
        json!([
            { "from": test, "fromRanges": [range(TEST, "Total")] },
            { "from": run, "fromRanges": [range(MAIN, "Total")] },
        ]),
    );
    fixture.open("cart.go");
    let state =
        settled(&mut fixture, "navigate.hierarchy.call", json!({ "offset": chars(CART, "Total"), "waitMs": 5000 }));
    assert_eq!(names(&state), ["Total", "Run", "TestTotal"], "sorted by name");
    assert_eq!(state["sort"], true);
    let state = fixture.dispatch("navigate.hierarchy.setOptions", json!({ "sort": false })).unwrap();
    assert_eq!(names(&state), ["Total", "TestTotal", "Run"], "as the server lists them");
    assert_eq!(state["singleClick"], true);
}

#[test]
fn answers_taken_in_by_any_reader_change_the_revision() {
    let (mut fixture, _library) = project();
    fixture
        .server
        .on("textDocument/prepareCallHierarchy", json!([item(&fixture.path("cart.go"), CART, "Total", 12, "Total")]));
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = std::sync::Mutex::new(gate);
    fixture.server.on_with("callHierarchy/incomingCalls", move |_| {
        let _ = gate.lock().unwrap().recv();
        json!([])
    });
    fixture.open("cart.go");
    fixture.dispatch("navigate.hierarchy.call", json!({ "offset": chars(CART, "Total") })).unwrap();
    let revision = fixture.session.state_mut::<super::Hierarchies>().revision();
    release.send(()).unwrap();
    let state = settled(&mut fixture, "navigate.hierarchy.get", json!({ "waitMs": 5000 }));
    assert_eq!(state["pending"], false);
    assert!(fixture.session.state_mut::<super::Hierarchies>().revision() > revision, "the tool window reads it again");
    let revision = fixture.session.state_mut::<super::Hierarchies>().revision();
    fixture.dispatch("navigate.hierarchy.get", json!({})).unwrap();
    assert_eq!(fixture.session.state_mut::<super::Hierarchies>().revision(), revision, "nothing new, nothing to read");
}

fn waiting(fixture: &Fixture) -> Vec<(usize, usize)> {
    let all = fixture.session.state::<super::Hierarchies>().unwrap();
    all.tabs.values().map(|hierarchy| (hierarchy.tab, hierarchy.waiting.len())).collect()
}

fn held_tabs(fixture: &mut Fixture) -> (Value, Value) {
    let cart = fixture.path("cart.go");
    let total = item(&cart, CART, "Total", 12, "Total");
    let sum = item(&cart, CART, "sum", 12, "sum(values");
    fixture.server.on_with("textDocument/prepareCallHierarchy", move |params| {
        if params["position"]["line"] == 3 { json!([total]) } else { json!([sum]) }
    });
    fixture.server.reply("callHierarchy/incomingCalls", Reply::Hold);
    fixture.open("cart.go");
    let at_total = json!({ "path": cart, "offset": chars(CART, "Total"), "waitMs": 2000 });
    let at_sum = json!({ "path": cart, "offset": chars(CART, "sum(values"), "waitMs": 2000 });
    (at_total, at_sum)
}

#[test]
fn closing_or_replacing_a_tab_cancels_only_its_requests_and_nothing_is_left_waiting() {
    let (mut fixture, _library) = project();
    let (at_total, at_sum) = held_tabs(&mut fixture);
    fixture.dispatch("navigate.hierarchy.call", at_total.clone()).unwrap();
    fixture.server.wait_for("callHierarchy/incomingCalls", 1, WAIT);
    fixture.dispatch("navigate.hierarchy.pin", json!({})).unwrap();
    fixture.dispatch("navigate.hierarchy.call", at_sum.clone()).unwrap();
    fixture.server.wait_for("callHierarchy/incomingCalls", 2, WAIT);
    assert_eq!(waiting(&fixture), [(0, 1), (1, 1)], "each tab waits for its root's callers");

    fixture.dispatch("navigate.hierarchy.select", json!({ "tab": 0 })).unwrap();
    fixture.dispatch("navigate.hierarchy.call", at_total).unwrap();
    assert_eq!(fixture.server.wait_for("$/cancelRequest", 1, WAIT).len(), 1, "the unpinned tab replaced is cancelled");
    fixture.server.wait_for("callHierarchy/incomingCalls", 3, WAIT);
    assert_eq!(waiting(&fixture), [(0, 1), (1, 1)], "the pinned tab still waits");

    fixture.dispatch("navigate.hierarchy.close", json!({ "tab": 1 })).unwrap();
    assert_eq!(fixture.server.wait_for("$/cancelRequest", 2, WAIT).len(), 2, "closing a tab cancels its requests");
    assert_eq!(waiting(&fixture), [(0, 1)]);
    let pinned = fixture.dispatch("navigate.hierarchy.get", json!({ "tab": 0 })).unwrap();
    assert_eq!((pinned["pending"].clone(), pinned["nodes"][0]["pending"].clone()), (json!(true), json!(true)));

    fixture.dispatch("navigate.hierarchy.close", json!({ "tab": 0 })).unwrap();
    assert_eq!(fixture.server.wait_for("$/cancelRequest", 3, WAIT).len(), 3);
    assert!(waiting(&fixture).is_empty(), "no request is left behind");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fixture.server.requests("$/cancelRequest").len(), 3, "each request is cancelled once");
}

#[test]
fn late_answers_of_cancelled_requests_are_dropped() {
    let (mut fixture, _library) = project();
    let cart = fixture.path("cart.go");
    let total = item(&cart, CART, "Total", 12, "Total");
    let run = item(&fixture.path("main.go"), MAIN, "Run", 12, "Run");
    fixture.server.on("textDocument/prepareCallHierarchy", json!([total]));
    let late = json!([{ "from": run, "fromRanges": [] }]);
    fixture.server.reply("callHierarchy/incomingCalls", Reply::Delayed(late, Duration::from_millis(800)));
    fixture.open("cart.go");
    let state = fixture
        .dispatch("navigate.hierarchy.call", json!({ "path": cart, "offset": chars(CART, "Total"), "waitMs": 100 }))
        .unwrap();
    assert_eq!(state["nodes"][0]["pending"], true);
    fixture.server.wait_for("callHierarchy/incomingCalls", 1, WAIT);
    fixture.dispatch("navigate.hierarchy.cancel", json!({})).unwrap();
    let revision = fixture.session.state::<super::Hierarchies>().unwrap().revision();
    std::thread::sleep(Duration::from_millis(1200));
    let state = fixture.dispatch("navigate.hierarchy.get", json!({ "waitMs": 200 })).unwrap();
    assert_eq!(names(&state), ["Total"], "the answer that came after the cancel is not shown");
    assert_eq!((state["pending"].clone(), state["nodes"][0]["children"].clone()), (json!(false), json!(null)));
    assert_eq!(fixture.session.state::<super::Hierarchies>().unwrap().revision(), revision);
}

#[test]
fn a_server_restart_ends_the_requests_of_every_tab_and_a_refresh_builds_them_again() {
    let (mut fixture, _library) = project();
    let (at_total, at_sum) = held_tabs(&mut fixture);
    fixture.dispatch("navigate.hierarchy.call", at_total).unwrap();
    fixture.server.wait_for("callHierarchy/incomingCalls", 1, WAIT);
    fixture.dispatch("navigate.hierarchy.pin", json!({})).unwrap();
    fixture.dispatch("navigate.hierarchy.call", at_sum).unwrap();
    fixture.server.wait_for("callHierarchy/incomingCalls", 2, WAIT);
    let servers = fixture.dispatch("lsp.servers.list", json!({})).unwrap();
    let id = servers["servers"][0]["id"].clone();
    fixture.dispatch("lsp.server.restart", json!({ "id": id })).unwrap();
    for tab in [0, 1] {
        let state = settled(&mut fixture, "navigate.hierarchy.get", json!({ "tab": tab, "waitMs": 5000 }));
        assert_eq!(state["pending"], false, "tab {tab} no longer waits: {state}");
        assert_eq!(state["nodes"][0]["error"], t!("workbench-language-not-running"), "{state}");
    }
    assert!(waiting(&fixture).iter().all(|(_, count)| *count == 0), "{:?}", waiting(&fixture));
    assert_eq!(fixture.server.requests("$/cancelRequest").len(), 0, "the new server is not told about old requests");

    let run = item(&fixture.path("main.go"), MAIN, "Run", 12, "Run");
    fixture.server.on("callHierarchy/incomingCalls", json!([{ "from": run, "fromRanges": [] }]));
    fixture.open("cart.go");
    for tab in [0, 1] {
        let state = fixture.dispatch_once_running("navigate.hierarchy.refresh", json!({ "tab": tab, "waitMs": 5000 }));
        let state = state.unwrap();
        assert_eq!(state["nodes"][0]["error"], Value::Null, "{state}");
        assert_eq!(state["nodes"][1]["name"], "Run", "tab {tab} works again: {state}");
    }
}

const WIDE_CALLERS: usize = 30;

fn wide_file() -> String {
    let mut text = String::from("package cart\n\n");
    for index in 0..WIDE_CALLERS {
        text.push_str(&format!("func Caller{index}() {{}}\n"));
    }
    text
}

#[test]
fn expand_all_of_a_wide_tree_asks_the_server_a_bounded_number_of_times() {
    let (mut fixture, _library) = project();
    let wide = wide_file();
    std::fs::write(fixture.path("wide.go"), &wide).unwrap();
    let path = fixture.path("wide.go");
    let callers: Vec<Value> = (0..WIDE_CALLERS)
        .map(|index| {
            let name = format!("Caller{index}");
            json!({ "from": item(&path, &wide, &name, 12, &format!("{name}()")), "fromRanges": [] })
        })
        .collect();
    fixture
        .server
        .on("textDocument/prepareCallHierarchy", json!([item(&fixture.path("cart.go"), CART, "Total", 12, "Total")]));
    fixture.server.on("callHierarchy/incomingCalls", Value::Array(callers));
    fixture.open("cart.go");
    settled(&mut fixture, "navigate.hierarchy.call", json!({ "offset": chars(CART, "Total"), "waitMs": 5000 }));
    let mut state = settled(&mut fixture, "navigate.hierarchy.expandAll", json!({ "waitMs": 10000 }));
    while state["pending"] == true {
        state = fixture.dispatch("navigate.hierarchy.get", json!({ "waitMs": 1000 })).unwrap();
    }
    let asked = fixture.server.requests("callHierarchy/incomingCalls").len();
    assert_eq!(asked, 1 + super::EXPAND_ALL_REQUESTS, "the root, then at most the budget of Expand All");
    let nodes = state["nodes"].as_array().unwrap();
    let parents: std::collections::HashMap<u64, u64> =
        nodes.iter().filter_map(|node| Some((node["id"].as_u64()?, node["parent"].as_u64()?))).collect();
    let depth = |node: &Value| std::iter::successors(node["id"].as_u64(), |id| parents.get(id).copied()).count() - 1;
    let deepest = nodes.iter().map(depth).max();
    assert_eq!(deepest, Some(3), "Expand All goes three levels deep");
    let unexpanded =
        nodes.iter().find(|node| depth(node) == 2 && node["children"].is_null() && node["recursive"] == false);
    let Some(unexpanded) = unexpanded else { panic!("a node of the second level is left to expand by hand") };
    let state = settled(&mut fixture, "navigate.hierarchy.expand", json!({ "node": unexpanded["id"], "waitMs": 5000 }));
    assert_eq!(fixture.server.requests("callHierarchy/incomingCalls").len(), asked + 1, "{}", state["pending"]);
}

#[test]
fn a_thousand_callers_are_read_at_once() {
    let (mut fixture, _library) = project();
    let mut wide = String::from("package cart\n\n");
    for index in 0..1000 {
        wide.push_str(&format!("func C{index}() int {{ return Total(nil) }}\n"));
    }
    std::fs::write(fixture.path("wide.go"), &wide).unwrap();
    let path = fixture.path("wide.go");
    let callers: Vec<Value> = (0..1000)
        .map(|index| json!({ "from": item(&path, &wide, &format!("C{index}"), 12, &format!("C{index}()")), "fromRanges": [range(&wide, &format!("C{index}()"))] }))
        .collect();
    fixture
        .server
        .on("textDocument/prepareCallHierarchy", json!([item(&fixture.path("cart.go"), CART, "Total", 12, "Total")]));
    fixture.server.on("callHierarchy/incomingCalls", Value::Array(callers));
    fixture.open("cart.go");
    let started = std::time::Instant::now();
    let state =
        settled(&mut fixture, "navigate.hierarchy.call", json!({ "offset": chars(CART, "Total"), "waitMs": 10000 }));
    let took = started.elapsed();
    assert_eq!(state["nodes"].as_array().unwrap().len(), 1001, "{}", state["pending"]);
    assert_eq!(state["nodes"][1000]["calls"][0]["line"], 1002);
    if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
        assert!(took < Duration::from_secs(2), "1000 callers took {took:?}");
    }
}

const LIB_RS: &str = "pub fn total(prices: &[i32]) -> i32 {\n    walk(prices.len())\n}\n\nfn walk(n: usize) -> i32 {\n    if n == 0 { 0 } else { walk(n - 1) }\n}\n\npub fn run() -> i32 {\n    total(&[])\n}\n";
const TEST_RS: &str = "#[test]\nfn totals() {\n    assert_eq!(shop::total(&[]), 0);\n}\n";

#[test]
fn a_rust_server_gives_the_call_hierarchy_and_reports_the_type_hierarchy_it_lacks() {
    let mut fixture =
        fixture(&[("Cargo.toml", "[package]\nname = \"shop\"\n"), ("src/lib.rs", LIB_RS), ("tests/total.rs", TEST_RS)]);
    let launched = fixture.server.clone();
    fixture.session.state_mut::<crate::language::Languages>().set_specs(vec![crate::language::ServerSpec {
        language_id: "rust".to_owned(),
        extensions: vec!["rs".to_owned()],
        launch: std::sync::Arc::new(move |_| Ok(launched.launcher())),
        initialization_options: json!({}),
    }]);
    fixture.server.set_capabilities(json!({
        "textDocumentSync": { "openClose": true, "change": 2 },
        "callHierarchyProvider": true
    }));
    let lib = fixture.path("src/lib.rs");
    let tests = fixture.path("tests/total.rs");
    let detail = |item: Value, detail: &str| {
        let mut item = item;
        item["detail"] = json!(detail);
        item
    };
    let total = detail(item(&lib, LIB_RS, "total", 12, "total(prices"), "pub fn total(prices: &[i32]) -> i32");
    let walk = detail(item(&lib, LIB_RS, "walk", 12, "walk(n: usize)"), "fn walk(n: usize) -> i32");
    let run = detail(item(&lib, LIB_RS, "run", 12, "run()"), "pub fn run() -> i32");
    let totals = detail(item(&tests, TEST_RS, "totals", 12, "totals()"), "fn totals()");
    let (on_total, on_walk) = (total.clone(), walk.clone());
    fixture.server.on_with("textDocument/prepareCallHierarchy", move |params| {
        if params["position"]["line"] == 0 { json!([on_total]) } else { json!([on_walk]) }
    });
    let callers = json!([
        { "from": run, "fromRanges": [range(LIB_RS, "total(&[])")] },
        { "from": totals, "fromRanges": [range(TEST_RS, "total(&[])")] },
    ]);
    let walk_callers = json!([
        { "from": total, "fromRanges": [range(LIB_RS, "walk(prices")] },
        { "from": walk, "fromRanges": [range(LIB_RS, "walk(n - 1)")] },
    ]);
    fixture.server.on_with("callHierarchy/incomingCalls", move |params| match params["item"]["name"].as_str() {
        Some("total") => callers.clone(),
        Some("walk") => walk_callers.clone(),
        _ => json!([]),
    });
    fixture.server.on("callHierarchy/outgoingCalls", json!([]));
    fixture.open("src/lib.rs");

    let state = settled(
        &mut fixture,
        "navigate.hierarchy.call",
        json!({ "path": lib, "offset": chars(LIB_RS, "total(prices"), "waitMs": 5000 }),
    );
    assert_eq!(names(&state), ["total", "run", "totals"], "{state}");
    assert_eq!(state["nodes"][1]["detail"], "pub fn run() -> i32");
    assert_eq!(state["nodes"][2]["calls"][0]["line"], 3, "{state}");
    let state = settled(
        &mut fixture,
        "navigate.hierarchy.call",
        json!({ "path": lib, "offset": chars(LIB_RS, "walk(n: usize)"), "waitMs": 5000 }),
    );
    assert_eq!(names(&state), ["walk", "total", "walk"]);
    assert_eq!(state["nodes"][2]["recursive"], true, "a recursive Rust function ends");
    let state = settled(&mut fixture, "navigate.hierarchy.expandAll", json!({ "waitMs": 5000 }));
    assert_eq!(names(&state), ["walk", "total", "run", "totals", "walk"], "{state}");

    let state = settled(
        &mut fixture,
        "navigate.hierarchy.type",
        json!({ "path": lib, "offset": chars(LIB_RS, "total(prices"), "waitMs": 5000 }),
    );
    assert_eq!(state["supported"], false);
    assert_eq!(state["error"], t!("workbench-hierarchy-unsupported-type", server = "rust".to_owned()));
    assert!(fixture.server.requests("textDocument/prepareTypeHierarchy").is_empty());
}
