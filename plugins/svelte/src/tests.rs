use super::*;

const COMPONENT: &str = r#"<script lang="ts">
  import Button from './Button.svelte'
  let { items = [] }: { items: string[] } = $props()
  let count = $state(0)
  const doubled = $derived(count * 2)
  $effect(() => {
    console.log(count)
  })
</script>

<h1 class="title" style="color: tan">Count: {count}</h1>
{#if count > 1}
  <p>{doubled}</p>
{:else if count === 1}
  <p>one</p>
{:else}
  <Button on:click|once={() => count++} bind:this={button} {...rest}>Add</Button>
{/if}
<ul>
  {#each items as item, i (item)}
    <li class:active={i === count} transition:fade>{@html item}</li>
  {/each}
</ul>

<style lang="scss">
  $accent: #ff3e00;
  .title {
    color: $accent;
  }
</style>
"#;

const KEYWORD: u32 = 0;
const STRING: u32 = 1;
const MACRO: u32 = 5;
const TYPE: u32 = 6;
const ATTRIBUTE: u32 = 9;
const PUNCTUATION: u32 = 11;

fn kinds(text: &str) -> Vec<(&str, u32)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

fn kind_of(spans: &[(&str, u32)], piece: &str) -> Vec<u32> {
    spans.iter().filter(|(text, _)| *text == piece).map(|(_, kind)| *kind).collect()
}

#[test]
fn spans_are_sorted_apart_and_on_character_boundaries() {
    for text in
        [COMPONENT, "<p>Привет, {имя} ✓</p>\n<style>a{color:red}</style>", "<", "{#if", "<b on:click={() => a > b}>"]
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
fn an_arrow_in_an_attribute_does_not_end_the_tag() {
    let text = "<button on:click={() => count++} class=\"big\">Add</button>";
    let spans = kinds(text);
    assert_eq!(kind_of(&spans, "class"), [ATTRIBUTE], "the tag goes on after the arrow: {spans:?}");
    assert_eq!(kind_of(&spans, "\"big\""), [STRING]);
    assert!(kind_of(&spans, "=>").iter().all(|kind| *kind == PUNCTUATION));
}

#[test]
fn blocks_tags_and_expressions_are_svelte() {
    let spans = kinds(COMPONENT);
    for word in ["#if", ":else if", ":else", "/if", "#each", "/each", "@html"] {
        assert_eq!(kind_of(&spans, word), [KEYWORD], "{word}: {spans:?}");
    }
    assert!(kind_of(&spans, "{").iter().filter(|kind| **kind == MACRO).count() >= 10, "{spans:?}");
    assert!(kind_of(&spans, "1").contains(&2), "the condition is script");
    assert_eq!(kind_of(&spans, "as"), [KEYWORD], "each … as …");
    assert!(kind_of(&spans, "item").iter().all(|kind| *kind != TYPE), "the item is no type: {spans:?}");
}

#[test]
fn directives_and_components() {
    let spans = kinds(COMPONENT);
    assert_eq!(kind_of(&spans, "Button"), [TYPE, TYPE], "a component's tags: {spans:?}");
    for directive in ["on", "bind", "transition"] {
        assert_eq!(kind_of(&spans, directive), [KEYWORD], "{directive}");
    }
    assert_eq!(kind_of(&spans, "class"), [ATTRIBUTE, KEYWORD], "an attribute, then a directive");
    for argument in ["click", "once", "this", "active", "fade"] {
        assert_eq!(kind_of(&spans, argument), [ATTRIBUTE], "{argument}");
    }
    assert!(kind_of(&spans, "|").contains(&PUNCTUATION));
    assert!(kind_of(&spans, "...").contains(&PUNCTUATION), "a spread: {spans:?}");
}

#[test]
fn the_script_has_its_runes_and_the_style_its_lang() {
    let spans = kinds(COMPONENT);
    for rune in ["$props", "$state", "$derived", "$effect"] {
        assert_eq!(kind_of(&spans, rune), [MACRO], "{rune}: {spans:?}");
    }
    assert_eq!(kind_of(&spans, "import"), [KEYWORD]);
    assert!(kind_of(&spans, "string").contains(&KEYWORD), "TypeScript by lang");
    assert_eq!(kind_of(&spans, "$accent").len(), 2, "SCSS variables");
    let raw = kinds("<script>\nlet a = $state.raw([])\nlet b = $derived.by(() => 1)\nlet $c = 1\n</script>");
    assert_eq!(kind_of(&raw, "$state.raw"), [MACRO]);
    assert_eq!(kind_of(&raw, "$derived.by"), [MACRO]);
    assert!(kind_of(&raw, "$c").iter().all(|kind| *kind != MACRO), "a variable named with $ is not a rune");
}

#[test]
fn the_structure_lists_the_script_the_markup_and_the_style() {
    let symbols = outline(COMPONENT);
    let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(names, ["script lang=\"ts\"", "h1", "{#if count > 1}", "ul", "style lang=\"scss\""]);
    let script: Vec<&str> = symbols[0].children.iter().map(|symbol| symbol.name.as_str()).collect();
    assert!(script.contains(&"count") && script.contains(&"doubled"), "{script:?}");
    let in_if: Vec<&str> = symbols[2].children.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(in_if, ["p", "p", "Button"]);
    let each = &symbols[3].children[0];
    assert_eq!((each.name.as_str(), each.kind), ("{#each items as item, i (item)}", "module"));
    assert_eq!(each.children[0].name, "li");
    for symbol in &symbols {
        for child in &symbol.children {
            assert!(symbol.start <= child.start && child.end <= symbol.end, "{child:?} outside {}", symbol.name);
        }
    }
    assert!(outline_json(&symbols).starts_with("[{\"name\":\"script lang=\\\"ts\\\"\",\"kind\":\"module\""));
}

#[test]
fn folds_cover_blocks_elements_script_and_style() {
    let folds: Vec<(u32, u32)> = fold(COMPONENT).iter().map(|fold| (fold.start_line, fold.end_line)).collect();
    for (fold, what) in [
        ((0, 8), "the script"),
        ((5, 7), "the effect"),
        ((11, 17), "the if block"),
        ((18, 22), "the list"),
        ((19, 21), "the each block"),
        ((24, 29), "the style"),
        ((26, 28), "a rule"),
    ] {
        assert!(folds.contains(&fold), "{what}: {folds:?}");
    }
}

fn labels(text: &str) -> Vec<String> {
    let caret = text.find('|').expect("a caret");
    complete(&text.replacen('|', "", 1), caret)
        .map(|completion| completion.items.into_iter().map(|item| item.label).collect())
        .unwrap_or_default()
}

#[test]
fn blocks_tags_and_their_ends_are_completed() {
    assert_eq!(labels("<p>{#e|</p>"), ["each"]);
    assert_eq!(labels("{#if a}\n  x\n{:e|"), ["else", "else if"]);
    assert_eq!(labels("<p>{@|</p>"), ["html", "const", "render", "debug"]);
    assert_eq!(labels("{#each list as x}\n  {#if x}\n  {/if}\n{/|"), ["each"]);
    let caret = "<p>{#i".len();
    let completion = complete("<p>{#i</p>", caret).unwrap();
    assert_eq!((completion.from, completion.items[0].insert.as_str()), ("<p>{#".len(), "if $0}"));
}

#[test]
fn directives_runes_script_and_style_are_completed() {
    assert_eq!(labels("<input bi|>"), ["bind:"]);
    assert!(labels("<div t|>").contains(&"transition:".to_owned()));
    let runes = labels("<script>\nlet a = $st|\n</script>");
    assert_eq!(runes, ["$state", "$state.raw"]);
    let caret = "<script>\nlet a = $st".len();
    assert_eq!(complete("<script>\nlet a = $st\n</script>", caret).unwrap().from, caret - 3);
    assert!(labels("<script>\nconst total = 1\ntot|\n</script>").contains(&"total".to_owned()));
    assert!(labels("<style>\na { backgr| }\n</style>").iter().any(|label| label == "background"));
    assert!(labels("<sc|").contains(&"script lang=\"ts\"".to_owned()));
    assert!(labels("<div>\n<sc|</div>").is_empty(), "inside an element, not the blocks");
}

#[test]
fn typing_adds_end_tags_and_block_ends() {
    let typed = |text: &str, typed: char| {
        let caret = text.find('|').unwrap();
        on_type(&text.replacen('|', "", 1), caret, typed)
    };
    assert_eq!(typed("<div>|", '>').as_deref(), Some("$0</div>"));
    assert_eq!(typed("<Button on:click={() => a > b}>|", '>').as_deref(), Some("$0</Button>"));
    assert_eq!(typed("<br>|", '>'), None, "a void element");
    assert_eq!(typed("<div>|</div>", '>'), None, "already closed");
    assert_eq!(typed("<ul>\n  <li>x\n</|", '/').as_deref(), Some("li>"));
    assert_eq!(typed("{#if ok}|", '}').as_deref(), Some("$0{/if}"));
    assert_eq!(typed("{#each a as b}|\n{/each}", '}'), None, "already closed");
    assert_eq!(typed("{value}|", '}'), None);
    assert_eq!(typed("<script>\nconst a = <div>|\n</script>", '>'), None, "not in the script");
    let mut input = 5u32.to_le_bytes().to_vec();
    input.extend_from_slice(&('>' as u32).to_le_bytes());
    input.extend_from_slice(b"<div>");
    assert_eq!(String::from_utf8(on_type_request(&input)).unwrap(), "{\"from\":5,\"to\":5,\"text\":\"$0</div>\"}");
}

#[test]
fn colours_come_from_styles() {
    let found: Vec<&str> = suggest::colors(COMPONENT).iter().map(|color| &COMPONENT[color.start..color.end]).collect();
    assert_eq!(found, ["tan", "#ff3e00"]);
}
