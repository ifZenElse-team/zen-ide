use zen_lang_go::GoOutliner;
use zen_syntax::{OutlineItem, Outliner, SymbolKind};

const SAMPLE: &str = include_str!("fixtures/sample.go");

fn tree(items: &[OutlineItem]) -> Vec<String> {
    let mut lines = Vec::new();
    fn walk(items: &[OutlineItem], depth: usize, lines: &mut Vec<String>) {
        for item in items {
            let detail = item.detail.as_deref().map(|detail| format!(" : {detail}")).unwrap_or_default();
            lines.push(format!("{}{:?} {}{detail}", "  ".repeat(depth), item.kind, item.name));
            walk(&item.children, depth + 1, lines);
        }
    }
    walk(items, 0, &mut lines);
    lines
}

#[test]
fn the_sample_file() {
    let items = GoOutliner.outline(SAMPLE);
    assert_eq!(
        tree(&items),
        [
            "Constant Max",
            "Constant Small",
            "Constant Medium",
            "Constant Large",
            "Variable count : int",
            "Variable name",
            "Variable title",
            "Variable handler",
            "Variable Default : *Plant",
            "Struct Plant",
            "  Field Name : string",
            "  Field Kind : string",
            "  Field height : float64",
            "  Field Reader",
            "  Field Soil",
            "  Field Leaves : map[string]struct{ Size int }",
            "  Method Grow : *Plant",
            "  Method Height : Plant",
            "Interface Grower",
            "  Method Grow : (days int) error",
            "  Method Height : () float64",
            "Type ID : int",
            "Type Alias : string",
            "Type List : []T",
            "  Method Len : List[T]",
            "Type Grid : [5]int",
            "Function New : (name string) *Plant",
            "Method Wet : *Soil",
        ]
    );
}

#[test]
fn offsets_point_at_the_declarations() {
    let items = GoOutliner.outline(SAMPLE);
    let at = |item: &OutlineItem| &SAMPLE[item.selection_start..item.selection_start + item.name.len()];
    let plant = items.iter().find(|item| item.name == "Plant").unwrap();
    assert_eq!(at(plant), "Plant");
    assert!(SAMPLE[plant.start..].starts_with("type Plant struct {"));
    assert!(SAMPLE[..plant.end].ends_with("Leaves map[string]struct{ Size int }\n}"));
    let grow = &plant.children[6];
    assert!(SAMPLE[grow.start..].starts_with("func (p *Plant) Grow"));
    assert!(SAMPLE[..grow.end].ends_with("return nil\n}"));
    assert_eq!(at(grow), "Grow");
    let medium = items.iter().find(|item| item.name == "Medium").unwrap();
    assert_eq!(&SAMPLE[medium.start..medium.end], "Medium");
    let max = &items[0];
    assert_eq!(&SAMPLE[max.start..max.end], "const Max = 10");
    let handler = items.iter().find(|item| item.name == "handler").unwrap();
    assert!(SAMPLE[..handler.end].ends_with("fmt.Println(\"inline\")\n\t}"));
    for item in &items {
        assert!(item.start <= item.selection_start && item.selection_start < item.end, "{item:?}");
    }
}

#[test]
fn offsets_are_in_characters() {
    let text = "// Ünïcödé\nfunc Größe() {}\n";
    let items = GoOutliner.outline(text);
    assert_eq!(items[0].name, "Größe");
    assert_eq!(items[0].start, 11);
    assert_eq!(items[0].selection_start, 16);
    assert_eq!(items[0].end, text.chars().count() - 1);
}

#[test]
fn broken_code_still_has_an_outline() {
    let text =
        "package p\n\ntype Half struct {\n\tA int\n\nfunc After() {\n\tif x {\n}\n\nvar tail = \n\nconst (\n\tOpen\n";
    let items = GoOutliner.outline(text);
    assert_eq!(
        tree(&items),
        ["Struct Half", "  Field A : int", "Function After : ()", "Variable tail", "Constant Open"]
    );
    for text in [
        "func",
        "func (",
        "func (r *T",
        "type",
        "type (",
        "type X struct {",
        "var (",
        "const",
        "}",
        "{{{",
        "func f[",
        "type I interface { M(",
        "type A [",
        "var a, = 1",
        "type T struct { *",
        "",
    ] {
        let _ = GoOutliner.outline(text);
    }
}

#[test]
fn methods_of_types_from_other_files_stay_on_top() {
    let items = GoOutliner.outline("func (s Server) Run() {}\nfunc (Server) Stop() {}\n");
    assert_eq!(tree(&items), ["Method Run : Server", "Method Stop : Server"]);
    assert_eq!(items[0].kind, SymbolKind::Method);
    assert!(GoOutliner.handles("main.go") && !GoOutliner.handles("go.mod"));
}
