use super::*;

const COMPONENT: &str = r#"<template>
  <ul :class="{ open }" @click.stop="toggle($event)">
    <li v-for="item in items" :key="item.id" #default="{ row }">{{ item.name }} &amp; more</li>
    <MyButton v-if="count > 1" label="Save" />
  </ul>
</template>

<script setup lang="ts">
import { ref } from 'vue'
interface Item { id: number; name: string }
const count = ref<number>(0)
function toggle(event: Event) {
  count.value++
}
</script>

<style scoped lang="scss">
$accent: #42b883;
.list {
  color: $accent;
  li { margin: 0 }
}
</style>
"#;

fn kinds(text: &str) -> Vec<(&str, u32)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

fn kind_of(spans: &[(&str, u32)], piece: &str) -> Vec<u32> {
    spans.iter().filter(|(text, _)| *text == piece).map(|(_, kind)| *kind).collect()
}

const KEYWORD: u32 = 0;
const STRING: u32 = 1;
const NUMBER: u32 = 2;
const MACRO: u32 = 5;
const TYPE: u32 = 6;
const FUNCTION: u32 = 7;
const ATTRIBUTE: u32 = 9;
const CONSTANT: u32 = 10;
const PUNCTUATION: u32 = 11;

#[test]
fn spans_are_sorted_apart_and_on_character_boundaries() {
    for text in
        [COMPONENT, "<template><p>Привет, {{ имя }} ✓</p></template>\n<style>a{color:red}</style>", "<", "<template"]
    {
        let spans = highlight(text);
        for pair in spans.windows(2) {
            assert!(pair[0].end <= pair[1].start, "{pair:?} in {text:?}");
        }
        for span in &spans {
            assert!(span.start < span.end && text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
        }
    }
}

#[test]
fn the_blocks_tags_are_html() {
    let spans = kinds(COMPONENT);
    assert_eq!(kind_of(&spans, "template")[..2], [KEYWORD, KEYWORD]);
    assert_eq!(kind_of(&spans, "setup"), [ATTRIBUTE]);
    assert_eq!(kind_of(&spans, "\"ts\""), [STRING]);
    assert_eq!(kind_of(&spans, "scoped"), [ATTRIBUTE]);
}

#[test]
fn components_in_the_template_are_types_and_html_tags_stay_tags() {
    let text = "<template>\n  <div>\n    <MyButton label=\"Save\" />\n    <Form.Field v-model=\"name\"></Form.Field>\n    \
                <keep-alive><component :is=\"view\" /></keep-alive>\n    <transition-group><li>x</li></transition-group>\n    \
                <my-widget></my-widget>\n  </div>\n</template>\n<script setup>\nconst name = 1\n</script>\n";
    let spans = kinds(text);
    assert_eq!(kind_of(&spans, "MyButton"), [TYPE], "{spans:?}");
    assert_eq!(kind_of(&spans, "Form.Field"), [TYPE, TYPE], "a namespaced component, start and end tag");
    for builtin in ["keep-alive", "component", "transition-group"] {
        assert!(kind_of(&spans, builtin).iter().all(|kind| *kind == TYPE), "{builtin}: {spans:?}");
    }
    for tag in ["div", "li", "my-widget"] {
        assert!(kind_of(&spans, tag).iter().all(|kind| *kind == KEYWORD), "{tag} stays a tag: {spans:?}");
    }
    assert_eq!(kind_of(&spans, "template"), [KEYWORD, KEYWORD], "the block's own tags stay tags");
    assert_eq!(kind_of(&spans, "script"), [KEYWORD, KEYWORD]);
}

#[test]
fn directives_and_their_expressions_are_vue() {
    let spans = kinds(COMPONENT);
    assert_eq!(kind_of(&spans, "v-for"), [KEYWORD]);
    assert_eq!(kind_of(&spans, "v-if"), [KEYWORD]);
    assert_eq!(kind_of(&spans, ":").iter().filter(|kind| **kind == KEYWORD).count(), 2, "`:class` and `:key`");
    assert_eq!(kind_of(&spans, "@"), [KEYWORD]);
    assert_eq!(kind_of(&spans, "#"), [KEYWORD]);
    assert_eq!(kind_of(&spans, "click"), [ATTRIBUTE]);
    assert_eq!(kind_of(&spans, "stop"), [ATTRIBUTE]);
    assert_eq!(kind_of(&spans, "key"), [ATTRIBUTE]);
    assert_eq!(kind_of(&spans, "default"), [ATTRIBUTE]);
    assert_eq!(kind_of(&spans, "in"), [KEYWORD], "the v-for expression is script");
    assert_eq!(kind_of(&spans, "1"), [NUMBER]);
    assert_eq!(kind_of(&spans, "\"Save\""), [STRING], "a plain attribute keeps its string");
    assert!(kind_of(&spans, "\"").iter().all(|kind| *kind == STRING), "a directive's quotes are a string");
    assert_eq!(kind_of(&spans, "&amp;"), [CONSTANT]);
}

#[test]
fn interpolations_are_expressions_between_braces() {
    let spans = kinds(COMPONENT);
    assert_eq!(kind_of(&spans, "{{"), [MACRO]);
    assert_eq!(kind_of(&spans, "}}"), [MACRO]);
    assert!(spans.contains(&(".", PUNCTUATION)), "{spans:?}");
    let unclosed = kinds("<template>{{ 42 </template>");
    assert_eq!(kind_of(&unclosed, "42"), [NUMBER]);
}

#[test]
fn the_script_is_read_by_its_lang() {
    let spans = kinds(COMPONENT);
    assert_eq!(kind_of(&spans, "import"), [KEYWORD]);
    assert_eq!(kind_of(&spans, "'vue'"), [STRING]);
    assert_eq!(kind_of(&spans, "Item"), [TYPE], "an interface is TypeScript");
    assert!(kind_of(&spans, "number").contains(&KEYWORD), "a TypeScript type: {spans:?}");
    assert!(kind_of(&spans, "toggle").contains(&FUNCTION), "the declaration in the script (its call too)");
    let plain = kinds("<script>\nconst a: number = 1\n</script>");
    assert!(!kind_of(&plain, "number").contains(&KEYWORD), "without lang it is JavaScript");
}

#[test]
fn the_style_is_read_by_its_lang() {
    let spans = kinds(COMPONENT);
    assert_eq!(kind_of(&spans, "$accent").len(), 2, "SCSS variables: {spans:?}");
    assert!(kind_of(&spans, "color").contains(&ATTRIBUTE));
    let css = kinds("<style>\n$x: 1;\na { color: red }\n</style>");
    assert!(kind_of(&css, "$x").is_empty(), "CSS has no variables");
    assert!(kind_of(&css, "color").contains(&ATTRIBUTE));
    let sass = kinds("<style lang=\"stylus\">\na\n  color red\n</style>");
    assert!(kind_of(&sass, "color").is_empty(), "a language the plug-in does not read stays plain");
}

#[test]
fn blocks_are_found_with_their_attributes() {
    let (tokens, _) = lexer::lex(COMPONENT);
    let blocks = sfc::blocks(COMPONENT, &tokens);
    let labels: Vec<String> = blocks.iter().map(sfc::Block::label).collect();
    assert_eq!(labels, ["template", "script setup lang=\"ts\"", "style scoped lang=\"scss\""]);
    assert!(blocks.iter().all(|block| block.closed));
    let template = &blocks[0];
    assert!(COMPONENT[template.inner.0..template.inner.1].trim_start().starts_with("<ul"));
    assert!(COMPONENT[template.end..].starts_with("\n\n<script"));
    let nested = "<template><template v-if=\"a\"><b/></template><i/></template><style/>";
    let (tokens, _) = lexer::lex(nested);
    let blocks = sfc::blocks(nested, &tokens);
    assert_eq!(blocks.len(), 2);
    assert_eq!(&nested[blocks[0].inner.0..blocks[0].inner.1], "<template v-if=\"a\"><b/></template><i/>");
    let open = "<template>\n<p>";
    let (tokens, _) = lexer::lex(open);
    assert!(!sfc::blocks(open, &tokens)[0].closed);
}

#[test]
fn folds_cover_blocks_elements_script_and_style() {
    let folds: Vec<(u32, u32)> = fold(COMPONENT).iter().map(|fold| (fold.start_line, fold.end_line)).collect();
    assert!(folds.contains(&(0, 5)), "the template: {folds:?}");
    assert!(folds.contains(&(1, 4)), "the list: {folds:?}");
    assert!(folds.contains(&(7, 14)), "the script: {folds:?}");
    assert!(folds.contains(&(11, 13)), "the function: {folds:?}");
    assert!(folds.contains(&(16, 22)), "the style: {folds:?}");
    assert!(folds.contains(&(18, 21)), "a rule: {folds:?}");
    assert!(folds.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn the_outline_lists_blocks_elements_declarations_and_rules() {
    let symbols = outline(COMPONENT);
    let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(names, ["template", "script setup lang=\"ts\"", "style scoped lang=\"scss\""]);
    let list = &symbols[0].children[0];
    assert_eq!(list.name, "ul");
    let items: Vec<&str> = list.children.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(items, ["li", "MyButton"]);
    let script: Vec<(&str, &str)> =
        symbols[1].children.iter().map(|symbol| (symbol.name.as_str(), symbol.kind)).collect();
    assert!(script.contains(&("Item", "interface")) && script.contains(&("toggle", "function")), "{script:?}");
    let toggle = symbols[1].children.iter().find(|symbol| symbol.name == "toggle").unwrap();
    assert!(COMPONENT[toggle.start..].starts_with("function toggle"));
    assert!(symbols[2].children.iter().any(|symbol| symbol.name == ".list"), "{:?}", symbols[2].children);
    for symbol in &symbols {
        for child in &symbol.children {
            assert!(symbol.start <= child.start && child.end <= symbol.end, "{child:?} outside {}", symbol.name);
        }
    }
    let json = outline_json(&symbols);
    assert!(json.starts_with("[{\"name\":\"template\",\"kind\":\"module\""), "{json}");
    assert!(json.contains("\"name\":\"script setup lang=\\\"ts\\\"\""), "{json}");
}

fn labels(text: &str) -> Vec<String> {
    let caret = text.find('|').expect("a caret");
    complete(&text.replacen('|', "", 1), caret)
        .map(|completion| completion.items.into_iter().map(|item| item.label).collect())
        .unwrap_or_default()
}

#[test]
fn directives_are_completed_in_the_templates_tags() {
    assert_eq!(labels("<template><div v-i|></div></template>"), ["v-if"]);
    let all = labels("<template><div v|</template>");
    assert!(all.contains(&"v-for".to_owned()) && all.contains(&"v-model".to_owned()), "{all:?}");
    let caret = "<template><p v-el".len();
    let completion = complete("<template><p v-el></p></template>", caret).unwrap();
    assert_eq!(completion.from, "<template><p ".len());
    let insert: Vec<&str> = completion.items.iter().map(|item| item.insert.as_str()).collect();
    assert_eq!(insert, ["v-else-if=\"$0\"", "v-else"]);
    assert!(labels("<template><p class|></p></template>").is_empty());
}

#[test]
fn the_script_and_the_style_complete_as_their_plug_ins() {
    let script = labels("<script setup lang=\"ts\">\nconst total = 1\ntot|\n</script>");
    assert!(script.contains(&"total".to_owned()), "{script:?}");
    let caret = "<script>\nconst total = 1\ntot".len();
    let completion = complete("<script>\nconst total = 1\ntot\n</script>", caret).unwrap();
    assert_eq!(completion.from, caret - 3);
    let style = labels("<style>\na { backgr| }\n</style>");
    assert!(style.iter().any(|label| label == "background"), "{style:?}");
    let scss = labels("<style lang=\"scss\">\n$accent: red;\na { color: $acc| }\n</style>");
    assert!(scss.iter().any(|label| label == "$accent"), "{scss:?}");
}

#[test]
fn blocks_are_completed_at_the_top_level() {
    let all = labels("<template></template>\n<s|");
    assert!(
        all.contains(&"script setup lang=\"ts\"".to_owned()) && all.contains(&"style scoped".to_owned()),
        "{all:?}"
    );
    assert!(!all.contains(&"template".to_owned()));
    let completion = complete("<te", 3).unwrap();
    assert_eq!((completion.from, completion.items[0].insert.as_str()), (1, "template>\n  $0\n</template>"));
    assert!(labels("<template>\n<p>\n<s|").is_empty(), "not inside the template");
}

#[test]
fn colours_come_from_styles() {
    let found: Vec<(&str, u32)> =
        suggest::colors(COMPONENT).iter().map(|color| (&COMPONENT[color.start..color.end], color.rgba)).collect();
    assert_eq!(found, [("#42b883", 0x42b883ff)]);
    let page =
        "<template><p style=\"color: red\" :style=\"{ color: 'blue' }\"></p></template><style>a{color:#fff}</style>";
    let found: Vec<&str> = suggest::colors(page).iter().map(|color| &page[color.start..color.end]).collect();
    assert_eq!(found, ["red", "#fff"]);
}

#[test]
fn the_abi_encodes_answers() {
    let mut input = 3u32.to_le_bytes().to_vec();
    input.extend_from_slice(b"<te");
    let json = String::from_utf8(complete_request(&input)).unwrap();
    assert!(json.starts_with("{\"from\":1,\"items\":[{\"label\":\"template\",\"kind\":\"snippet\""), "{json}");
    assert!(complete_request(&[1, 0]).is_empty());
    assert_eq!(encode_spans(&highlight("<template/>")).len() % 12, 0);
    assert_eq!(colors_request(b"<style>a{color:red}</style>").len(), 12);
}
