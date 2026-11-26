use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_plugins::native::{Exports, NativePlugin};
use zen_syntax::TextFeatures;
use zen_workbench::Workbench;
use zen_workbench::plugins::PluginFeatures;

fn text_of(input: &[u8]) -> &str {
    std::str::from_utf8(input).unwrap()
}

fn caret_input(input: &[u8]) -> (usize, &str) {
    let (caret, text) = input.split_first_chunk::<4>().unwrap();
    (u32::from_le_bytes(*caret) as usize, text_of(text))
}

fn html() -> NativePlugin {
    use zen_plugin_html as html;
    NativePlugin::new(
        &["html", "htm", "xhtml"],
        Exports {
            fold: Some(|input| html::encode_folds(&html::fold(text_of(input)))),
            outline: Some(|input| html::outline_json(&html::outline(text_of(input))).into_bytes()),
            complete: Some(|input| {
                let (caret, text) = caret_input(input);
                html::complete(text, caret).map_or_else(Vec::new, |found| html::completion_json(&found).into_bytes())
            }),
            expand: Some(|input| {
                let (caret, text) = caret_input(input);
                html::expand(text, caret).map_or_else(Vec::new, |found| html::expansion_json(&found).into_bytes())
            }),
            colors: Some(html::colors_request),
            on_type: Some(html::on_type_request),
            triggers: Some(|_| html::TRIGGERS.as_bytes().to_vec()),
            ..Exports::default()
        },
    )
}

fn css() -> NativePlugin {
    use zen_plugin_css as css;
    NativePlugin::new(
        &["css"],
        Exports {
            fold: Some(|input| css::encode_folds(&css::fold(text_of(input)))),
            outline: Some(|input| css::outline_json(&css::outline(text_of(input))).into_bytes()),
            complete: Some(css::complete_request),
            colors: Some(css::colors_request),
            ..Exports::default()
        },
    )
}

fn scss() -> NativePlugin {
    use zen_plugin_scss as scss;
    NativePlugin::new(
        &["scss"],
        Exports {
            fold: Some(|input| scss::encode_folds(&scss::fold(text_of(input)))),
            outline: Some(|input| scss::outline_json(&scss::outline(text_of(input))).into_bytes()),
            complete: Some(scss::complete_request),
            colors: Some(scss::colors_request),
            ..Exports::default()
        },
    )
}

fn javascript() -> NativePlugin {
    use zen_plugin_javascript as js;
    const DIALECT: js::Dialect = js::Dialect::JavaScript;
    NativePlugin::new(
        &["js", "mjs", "cjs", "jsx"],
        Exports {
            fold: Some(|input| js::encode_folds(&js::fold(text_of(input), DIALECT))),
            outline: Some(|input| js::outline_json(&js::outline(text_of(input), DIALECT)).into_bytes()),
            complete: Some(js::complete_request),
            on_type: Some(js::on_type_request),
            triggers: Some(|_| js::JSX_TRIGGERS.as_bytes().to_vec()),
            ..Exports::default()
        },
    )
}

fn typescript() -> NativePlugin {
    use zen_plugin_typescript as ts;
    const DIALECT: ts::Dialect = ts::Dialect::TypeScript;
    NativePlugin::new(
        &["ts", "mts", "cts", "tsx"],
        Exports {
            fold: Some(|input| ts::encode_folds(&ts::fold(text_of(input), DIALECT))),
            outline: Some(|input| ts::outline_json(&ts::outline(text_of(input), DIALECT)).into_bytes()),
            complete: Some(ts::complete_request),
            on_type: Some(ts::on_type_request),
            triggers: Some(|_| ts::JSX_TRIGGERS.as_bytes().to_vec()),
            ..Exports::default()
        },
    )
}

fn vue() -> NativePlugin {
    use zen_plugin_vue as vue;
    NativePlugin::new(
        &["vue"],
        Exports {
            fold: Some(|input| vue::encode_folds(&vue::fold(text_of(input)))),
            outline: Some(|input| vue::outline_json(&vue::outline(text_of(input))).into_bytes()),
            complete: Some(vue::complete_request),
            colors: Some(vue::colors_request),
            ..Exports::default()
        },
    )
}

fn svelte() -> NativePlugin {
    use zen_plugin_svelte as svelte;
    NativePlugin::new(
        &["svelte"],
        Exports {
            fold: Some(|input| svelte::encode_folds(&svelte::fold(text_of(input)))),
            outline: Some(|input| svelte::outline_json(&svelte::outline(text_of(input))).into_bytes()),
            complete: Some(svelte::complete_request),
            colors: Some(svelte::colors_request),
            on_type: Some(svelte::on_type_request),
            triggers: Some(|_| svelte::TRIGGERS.as_bytes().to_vec()),
            ..Exports::default()
        },
    )
}

fn xml() -> NativePlugin {
    use zen_plugin_xml as xml;
    NativePlugin::new(
        &["xml", "plist"],
        Exports {
            fold: Some(|input| xml::encode_folds(&xml::fold(text_of(input)))),
            outline: Some(|input| xml::outline_json(&xml::outline(text_of(input))).into_bytes()),
            on_type: Some(xml::on_type_request),
            triggers: Some(|_| xml::TRIGGERS.as_bytes().to_vec()),
            problems: Some(|input| xml::problems_json(&xml::problems(text_of(input))).into_bytes()),
            ..Exports::default()
        },
    )
}

fn java() -> NativePlugin {
    use zen_plugin_java as java;
    NativePlugin::new(
        &["java"],
        Exports {
            fold: Some(|input| java::encode_folds(&java::fold(text_of(input)))),
            outline: Some(|input| java::outline_json(&java::outline(text_of(input))).into_bytes()),
            complete: Some(java::complete_request),
            ..Exports::default()
        },
    )
}

fn declare_xml() {
    let manifest: Value = serde_json::from_str(include_str!("../../../../plugins/xml/plugin.json")).unwrap();
    let contents: Vec<zen_plugins::ContentDeclaration> = serde_json::from_value(manifest["contents"].clone()).unwrap();
    let extensions = manifest["extensions"].as_array().unwrap().iter().filter_map(Value::as_str);
    zen_vfs::file_types::declare([zen_vfs::file_types::FileType {
        id: "xml".to_owned(),
        name: "XML".to_owned(),
        localized: false,
        patterns: extensions.map(|extension| format!("*.{extension}")).collect(),
        hashbangs: Vec::new(),
        contents: contents.iter().map(zen_plugins::ContentDeclaration::marker).collect(),
    }]);
}

struct Window {
    registry: Registry,
    session: Session,
    folder: tempfile::TempDir,
}

impl Window {
    fn new(files: &[(&str, &str)]) -> Self {
        let folder = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(folder.path().join(name), text).unwrap();
        }
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        let plugins: Vec<Arc<dyn TextFeatures>> = vec![
            Arc::new(html()),
            Arc::new(css()),
            Arc::new(scss()),
            Arc::new(javascript()),
            Arc::new(typescript()),
            Arc::new(vue()),
            Arc::new(svelte()),
            Arc::new(xml()),
            Arc::new(java()),
        ];
        session.state_mut::<PluginFeatures>().set(plugins);
        Self { registry, session, folder }
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, command, params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    fn open(&mut self, name: &str) {
        let path = self.folder.path().join(name);
        self.run("file.document.open", json!({ "path": path }));
    }

    fn caret_at(&mut self, offset: usize) {
        self.run("editor.selection.set", json!({ "selections": [{ "head": offset }] }));
    }

    fn text(&self) -> String {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn caret(&self) -> usize {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head
    }

    fn fold_lines(&mut self) -> Vec<(u64, u64)> {
        let folds = self.run("editor.fold.list", json!({}));
        folds
            .as_array()
            .unwrap()
            .iter()
            .map(|fold| (fold["startLine"].as_u64().unwrap(), fold["endLine"].as_u64().unwrap()))
            .collect()
    }

    fn labels(list: &Value) -> Vec<String> {
        list["items"].as_array().unwrap().iter().map(|item| item["label"].as_str().unwrap().to_owned()).collect()
    }

    fn accept(&mut self, label: &str, replace: bool) {
        let list = self.run("editor.completion.list", json!({}));
        let item = list["items"].as_array().unwrap().iter().find(|item| item["label"] == label).cloned();
        let index = item.unwrap_or_else(|| panic!("no {label} in {list}"))["index"].clone();
        self.run("editor.completion.accept", json!({ "index": index, "replace": replace }));
    }

    fn structure(&mut self) -> Value {
        let state = self.run("editor.structure.get", json!({}));
        assert_eq!((state["status"].as_str(), state["source"].as_str()), (Some("ready"), Some("syntax")), "{state}");
        state["symbols"].clone()
    }
}

fn names(symbols: &Value) -> Vec<String> {
    symbols.as_array().unwrap().iter().map(|symbol| symbol["name"].as_str().unwrap().to_owned()).collect()
}

const PAGE: &str = "<html>\n<body>\n  <ul>\n    <li>one</li>\n  </ul>\n  <!--\n  note\n  -->\n</body>\n</html>\n";

#[test]
fn html_folds_outlines_completes_and_expands() {
    let mut window = Window::new(&[("page.html", PAGE), ("list.html", "<p>\n  \n</p>\n")]);
    window.open("page.html");
    assert_eq!(window.fold_lines(), [(0, 9), (1, 8), (2, 4), (5, 7)]);
    let collapsed = window.run("editor.fold.collapseAll", json!({}));
    assert!(collapsed.as_array().unwrap().iter().all(|fold| fold["folded"] == true), "{collapsed}");
    window.run("editor.fold.expandAll", json!({}));

    let symbols = window.structure();
    assert_eq!(names(&symbols), ["html"]);
    assert_eq!(symbols[0]["kind"], "element");
    assert_eq!(names(&symbols[0]["children"]), ["body"]);
    assert_eq!(names(&symbols[0]["children"][0]["children"]), ["ul"]);

    window.open("list.html");
    window.caret_at(6);
    window.run("editor.edit.type", json!({ "text": "<se" }));
    let list = window.run("editor.completion.start", json!({}));
    assert!(Window::labels(&list).contains(&"section".to_owned()), "{list}");
    window.accept("section", false);
    assert_eq!(window.text(), "<p>\n  <section\n</p>\n");

    window.run("editor.edit.type", json!({ "text": "></section>\n  ul>li.item$*2" }));
    window.run("editor.edit.tab", json!({}));
    let text = window.text();
    assert!(text.contains("<ul>\n"), "{text}");
    assert!(text.contains("<li class=\"item1\"></li>") && text.contains("<li class=\"item2\"></li>"), "{text}");
    let caret = text.find("item1\">").unwrap() + "item1\">".len();
    assert_eq!(window.caret(), caret, "the caret in the first empty element: {text}");
    window.run("editor.edit.undo", json!({}));
    assert!(window.text().ends_with("ul>li.item$*2\n</p>\n"), "one undo step");

    window.caret_at(4);
    window.run("editor.edit.tab", json!({}));
    assert!(window.text().starts_with("<p>\n    "), "Tab indents where there is nothing to expand");

    let path = window.folder.path().join("list.html");
    let end = window.text().find("$*2").unwrap() + 3;
    let applied = window.run("editor.emmet.expand", json!({ "path": path, "offset": end }));
    assert_eq!(applied, json!({ "applied": true }));
}

#[test]
fn css_folds_outlines_and_completes() {
    let sheet = "/* a\n   b */\n.a {\n  color: red;\n}\n@media print {\n  .b {\n    margin: 0;\n  }\n}\n";
    let mut window = Window::new(&[("site.css", sheet)]);
    window.open("site.css");
    assert_eq!(window.fold_lines(), [(0, 1), (2, 4), (5, 9), (6, 8)]);

    let symbols = window.structure();
    assert_eq!(names(&symbols), [".a", "@media print"]);
    assert_eq!(symbols[0]["kind"], "rule");
    assert_eq!(names(&symbols[1]["children"]), [".b"]);

    let at = sheet.find("color").unwrap();
    window.caret_at(at);
    window.run("editor.edit.type", json!({ "text": "backg" }));
    let list = window.run("editor.completion.start", json!({}));
    assert!(Window::labels(&list).contains(&"background-color".to_owned()), "{list}");
    window.run("editor.edit.type", json!({ "text": "round-c" }));
    let list = window.run("editor.completion.list", json!({}));
    assert_eq!(list["prefix"], "background-c", "a CSS word holds `-`");
    window.accept("background-color", true);
    assert!(
        window.text().contains(".a {\n  background-color: red;\n}"),
        "Tab replaces the rest of the word: {}",
        window.text()
    );
}

#[test]
fn javascript_and_typescript_fill_the_structure_and_fold() {
    let script = "import a from \"a\";\nimport b from \"b\";\n\nclass Cart {\n  add(item) {\n    this.items.push(item);\n  }\n}\n\nfunction total() {\n  return 0;\n}\n";
    let typed = "interface Item {\n  price: number;\n}\n\nexport function sum(items: Item[]): number {\n  return items.length;\n}\n";
    let mut window = Window::new(&[("cart.js", script), ("sum.ts", typed)]);
    window.open("cart.js");
    let symbols = window.structure();
    assert_eq!(names(&symbols), ["Cart", "total"], "{symbols}");
    assert_eq!(symbols[0]["kind"], "class");
    assert_eq!(names(&symbols[0]["children"]), ["add"]);
    let folds = window.fold_lines();
    for range in [(0, 1), (3, 7), (4, 6), (9, 11)] {
        assert!(folds.contains(&range), "{range:?} in {folds:?}");
    }
    window.caret_at(script.find("return 0").unwrap());
    window.run("editor.edit.type", json!({ "text": "ret" }));
    let list = window.run("editor.completion.start", json!({}));
    assert!(Window::labels(&list).contains(&"return".to_owned()), "{list}");
    window.run("editor.completion.cancel", json!({}));

    window.open("sum.ts");
    let symbols = window.structure();
    assert_eq!(names(&symbols), ["Item", "sum"], "{symbols}");
    assert_eq!((symbols[0]["kind"].as_str(), symbols[1]["kind"].as_str()), (Some("interface"), Some("function")));
    assert_eq!(window.fold_lines(), [(0, 2), (4, 6)]);
}

#[test]
fn html_closes_tags_as_they_are_typed() {
    let mut window = Window::new(&[("page.html", "<ul>\n</ul>\n")]);
    window.open("page.html");
    window.caret_at(5);
    for typed in ["<", "l", "i", ">"] {
        window.run("editor.edit.type", json!({ "text": typed }));
    }
    assert_eq!(window.text(), "<ul>\n<li></li></ul>\n");
    assert_eq!(window.caret(), 9, "the caret between the tags");
    window.run("editor.edit.type", json!({ "text": "a" }));
    window.caret_at(window.text().find("</ul>").unwrap() + 5);
    window.run("editor.edit.type", json!({ "text": "\n" }));
    for typed in ["<", "/"] {
        window.run("editor.edit.type", json!({ "text": typed }));
    }
    assert_eq!(window.text(), "<ul>\n<li>a</li></ul>\n</\n", "nothing is open after the list");
    window.run("editor.edit.undo", json!({}));
    window.run("editor.edit.undo", json!({}));
    window.caret_at(10);
    for typed in ["<", "/"] {
        window.run("editor.edit.type", json!({ "text": typed }));
    }
    assert!(window.text().starts_with("<ul>\n<li>a</li></li>"), "{}", window.text());
}

#[test]
fn colours_are_listed_for_css_scss_and_html() {
    let mut window = Window::new(&[
        ("site.css", "a { color: #ff8000; }\n"),
        ("theme.scss", "$accent: hsl(120, 100%, 25%);\n.a { color: rgba($accent, .5); b { fill: lime } }\n"),
        ("page.html", "<p style=\"color: red\">blue</p>\n"),
    ]);
    let colors = |window: &mut Window, name: &str| {
        let path = window.folder.path().join(name);
        let listed = window.run("editor.colors.list", json!({ "path": path }));
        listed["colors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|color| (color["text"].as_str().unwrap().to_owned(), color["color"].as_str().unwrap().to_owned()))
            .collect::<Vec<_>>()
    };
    let pair = |text: &str, color: &str| (text.to_owned(), color.to_owned());
    assert_eq!(colors(&mut window, "site.css"), [pair("#ff8000", "#ff8000ff")]);
    assert_eq!(
        colors(&mut window, "theme.scss"),
        [pair("hsl(120, 100%, 25%)", "#008000ff"), pair("lime", "#00ff00ff")]
    );
    assert_eq!(colors(&mut window, "page.html"), [pair("red", "#ff0000ff")]);
    window.open("site.css");
    let path = window.folder.path().join("site.css");
    let set = window.run("editor.colors.set", json!({ "path": path, "offset": 11, "color": "#0000ff" }));
    assert_eq!(set["text"], "#0000ff");
    assert_eq!(window.text(), "a { color: #0000ff; }\n");
}

#[test]
fn scss_folds_outlines_and_completes() {
    let sheet = "// tokens\n$gap: 4px;\n@mixin pad($x) {\n  padding: $x;\n}\n.card {\n  @include pad($gap);\n  &-title {\n    margin: 0;\n  }\n}\n";
    let mut window = Window::new(&[("theme.scss", sheet)]);
    window.open("theme.scss");
    assert_eq!(window.fold_lines(), [(2, 4), (5, 10), (7, 9)]);
    let symbols = window.structure();
    assert_eq!(names(&symbols), ["$gap", "@mixin pad($x)", ".card"], "{symbols}");
    assert_eq!(symbols[1]["kind"], "function");
    assert_eq!(names(&symbols[2]["children"]), ["&-title"]);
    window.caret_at(sheet.find("margin").unwrap());
    window.run("editor.edit.type", json!({ "text": "padding: $g" }));
    let list = window.run("editor.completion.start", json!({}));
    assert_eq!(Window::labels(&list), ["$gap"], "{list}");
}

#[test]
fn vue_folds_outlines_completes_and_lists_colours() {
    let component = "<template>\n  <ul>\n    <li v-for=\"item in items\">{{ item }}</li>\n  </ul>\n</template>\n\n<script setup lang=\"ts\">\nfunction add(a: number) {\n  return a + 1\n}\n</script>\n\n<style scoped lang=\"scss\">\n$accent: #42b883;\n.list {\n  color: $accent;\n}\n</style>\n";
    let mut window = Window::new(&[("App.vue", component)]);
    window.open("App.vue");
    assert_eq!(window.fold_lines(), [(0, 4), (1, 3), (6, 10), (7, 9), (12, 17), (14, 16)]);
    let symbols = window.structure();
    assert_eq!(names(&symbols), ["template", "script setup lang=\"ts\"", "style scoped lang=\"scss\""], "{symbols}");
    assert_eq!(names(&symbols[0]["children"]), ["ul"]);
    assert_eq!(names(&symbols[1]["children"]), ["add"]);
    assert_eq!(names(&symbols[2]["children"]), ["$accent", ".list"]);
    let path = window.folder.path().join("App.vue");
    let listed = window.run("editor.colors.list", json!({ "path": path }));
    assert_eq!(listed["colors"][0]["text"], "#42b883", "{listed}");
    window.caret_at(component.find("v-for").unwrap());
    window.run("editor.edit.type", json!({ "text": "v-sh " }));
    window.caret_at(component.find("v-for").unwrap() + 4);
    let list = window.run("editor.completion.start", json!({}));
    assert_eq!(Window::labels(&list), ["v-show"], "{list}");
    window.accept("v-show", false);
    assert!(window.text().contains("<li v-show=\"\" v-for"), "{}", window.text());
}

#[test]
fn svelte_folds_outlines_completes_and_closes_tags() {
    let component = "<script lang=\"ts\">\n  let count = $state(0)\n</script>\n\n{#if count > 1}\n  <p>{count}</p>\n{/if}\n\n<style>\n  p { color: tan }\n</style>\n";
    let mut window = Window::new(&[("Counter.svelte", component)]);
    window.open("Counter.svelte");
    assert_eq!(window.fold_lines(), [(0, 2), (4, 6), (8, 10)]);
    let symbols = window.structure();
    assert_eq!(names(&symbols), ["script lang=\"ts\"", "{#if count > 1}", "style"], "{symbols}");
    assert_eq!(names(&symbols[1]["children"]), ["p"]);
    let path = window.folder.path().join("Counter.svelte");
    let listed = window.run("editor.colors.list", json!({ "path": path }));
    assert_eq!(listed["colors"][0]["text"], "tan", "{listed}");
    let at = component.find("</style>").unwrap() + "</style>\n".len();
    window.caret_at(at);
    for typed in ["<", "d", "i", "v", ">"] {
        window.run("editor.edit.type", json!({ "text": typed }));
    }
    assert!(window.text().ends_with("<div></div>"), "{}", window.text());
    window.run("editor.edit.type", json!({ "text": "{#each" }));
    let list = window.run("editor.completion.start", json!({}));
    assert_eq!(Window::labels(&list), ["each"], "{list}");
}

struct JavaScriptColours;

impl zen_syntax::Highlighter for JavaScriptColours {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".js")
    }

    fn highlight(&self, text: &str) -> Vec<zen_syntax::Span> {
        zen_plugin_javascript::highlight(text, zen_plugin_javascript::Dialect::JavaScript)
            .into_iter()
            .filter_map(|span| {
                Some(zen_syntax::Span {
                    range: span.start..span.end,
                    kind: zen_plugins::kind_from_abi(span.kind as u32)?,
                })
            })
            .collect()
    }
}

#[test]
fn javascript_region_comments_in_template_literals_and_block_comments_do_not_count() {
    let text = "// region Real\nconst raw = `\n// endregion\n`;\n/*\n// endregion\n  //#region Not one\n*/\nfunction f() {\n  //#region Inner\n  return '// endregion';\n  //#endregion\n}\n// endregion\n";
    let mut window = Window::new(&[("app.js", text)]);
    let colours: Arc<dyn zen_syntax::Highlighter> = Arc::new(JavaScriptColours);
    window
        .session
        .state_mut::<zen_workbench::brackets::Highlighters>()
        .set_source(Arc::new(move |name| colours.handles(name).then(|| Arc::clone(&colours))));
    window.open("app.js");
    let regions = window.run("editor.fold.regions", json!({}));
    let held: Vec<(&str, String)> = regions
        .as_array()
        .unwrap()
        .iter()
        .map(|region| {
            let (start, end) = (region["start"].as_u64().unwrap() as usize, region["end"].as_u64().unwrap() as usize);
            (region["description"].as_str().unwrap(), text.chars().skip(start).take(end - start).collect())
        })
        .collect();
    assert_eq!(
        held,
        [
            ("Real", text.trim_end().to_owned()),
            ("Inner", "//#region Inner\n  return '// endregion';\n  //#endregion".to_owned()),
        ]
    );
}

#[test]
fn typing_a_jsx_start_tag_adds_its_end_tag_in_tsx_and_jsx() {
    for name in ["App.tsx", "App.jsx"] {
        let source = "export const App = () => (\n  \n)\n";
        let mut window = Window::new(&[(name, source)]);
        window.open(name);
        window.caret_at(source.find("\n  \n").unwrap() + 3);
        for typed in "<main className=\"app\">".chars() {
            window.run("editor.edit.type", json!({ "text": typed.to_string() }));
        }
        assert_eq!(window.text(), "export const App = () => (\n  <main className=\"app\"></main>\n)\n", "{name}");
        assert_eq!(window.caret(), source.find("\n  \n").unwrap() + 3 + "<main className=\"app\">".len(), "{name}");
        for typed in "<br />".chars() {
            window.run("editor.edit.type", json!({ "text": typed.to_string() }));
        }
        assert!(window.text().contains("<main className=\"app\"><br /></main>"), "{}", window.text());
    }
}

const PROFILE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Protocol</key>\n\t<string>s3</string>\n</dict>\n</plist>\n";

#[test]
fn a_property_list_with_an_unknown_extension_is_xml_by_its_content() {
    declare_xml();
    let name = "timeweb-cloud.cyberduckprofile";
    let mut window = Window::new(&[(name, PROFILE)]);
    let path = window.folder.path().join(name);
    let typed = window.run("editor.fileTypes.typeOf", json!({ "path": path }));
    assert_eq!((&typed["id"], &typed["byContent"]), (&json!("xml"), &json!(true)), "{typed}");
    assert_eq!(zen_vfs::file_types::known_name_at(&path, None).as_deref(), Some("file.xml"), "icons and the tree too");
    window.open(name);
    assert_eq!(window.fold_lines(), [(2, 7), (3, 6)]);
    let symbols = window.structure();
    assert_eq!(names(&symbols), ["plist"], "{symbols}");
    assert_eq!(names(&symbols[0]["children"]), ["dict"]);
    let problems = window.run("problems.file.list", json!({ "path": path }));
    assert_eq!(
        (&problems["checked"], &problems["problems"]),
        (&json!(true), &json!([])),
        "checked by the plug-in: {problems}"
    );

    let at = PROFILE.find("</dict>").unwrap();
    window.caret_at(at);
    for typed in "<key>".chars() {
        window.run("editor.edit.type", json!({ "text": typed.to_string() }));
    }
    assert!(window.text().contains("\t<string>s3</string>\n<key></key></dict>"), "{}", window.text());
    let end = window.text().find("<key></key>").unwrap() + "<key></key>".len();
    window.caret_at(end);
    for typed in "</".chars() {
        window.run("editor.edit.type", json!({ "text": typed.to_string() }));
    }
    assert!(window.text().contains("<key></key></dict></dict>"), "{}", window.text());
    let problems = window.run("problems.file.list", json!({ "path": path }));
    let listed = problems.get("problems").unwrap_or(&problems);
    let messages: Vec<&str> =
        listed.as_array().unwrap().iter().filter_map(|problem| problem["message"].as_str()).collect();
    assert_eq!(messages, ["End tag </dict> does not match the open element <plist>"], "{problems}");
}

#[test]
fn java_folds_outlines_completes_comments_and_pairs() {
    let source = "package com.acme;\n\nimport java.util.List;\nimport java.util.Map;\n\npublic record Order(String id, List<String> lines) {\n    public int size() {\n        return lines.size();\n    }\n}\n";
    let mut window = Window::new(&[("Order.java", source)]);
    window.open("Order.java");
    assert_eq!(window.fold_lines(), [(2, 3), (5, 9), (6, 8)]);
    let symbols = window.structure();
    assert_eq!(names(&symbols), ["Order"], "{symbols}");
    assert_eq!(names(&symbols[0]["children"]), ["id", "lines", "size"]);

    let at = source.find("return").unwrap();
    window.caret_at(at);
    window.run("editor.edit.type", json!({ "text": "Ord" }));
    let list = window.run("editor.completion.start", json!({}));
    assert!(Window::labels(&list).contains(&"Order".to_owned()), "{list}");
    window.run("editor.completion.cancel", json!({}));
    window.run("editor.edit.undo", json!({}));

    window.caret_at(at);
    window.run("editor.comment.line", json!({}));
    assert!(window.text().contains("//         return lines.size();"), "{}", window.text());
    window.run("editor.comment.line", json!({}));
    assert_eq!(window.text(), source);
    window.caret_at(source.find("size();").unwrap() + "size();".len());
    window.run("editor.edit.type", json!({ "text": "(" }));
    window.run("editor.edit.type", json!({ "text": "\"" }));
    assert!(window.text().contains("size();(\"\")\n"), "{}", window.text());
}
