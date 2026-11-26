use super::*;

pub(crate) const README: &str = r#"# Zen Notes

Text with **bold**, *emphasis*, ~~strike~~, `code` and a [link](docs/guide.md#install).
A bare address: https://example.com/path. And www.rust-lang.org!

## Getting started

1. First
2. Second
   - nested *item*
   - [x] done task
   - [ ] open task

> A quote
> over two lines

> [!WARNING]
> Mind the gap.

| Left | Centre | Right |
|:-----|:------:|------:|
| a    | **b**  | 3     |

```rust
fn main() {}
```

```mermaid
graph TD; A-->B
```

$$
E = mc^2
$$

Inline $x^2$ math and a note[^1].

<details><summary>More</summary>Hidden <b>bold</b><script>alert(1)</script> text<br>next</details>

![Logo](images/logo.png)

---

## Getting started

[^1]: The footnote's text.
"#;

fn kinds(rendered: &Rendered) -> Vec<&'static str> {
    rendered
        .blocks
        .iter()
        .map(|block| match block.kind {
            BlockKind::Heading { .. } => "heading",
            BlockKind::Paragraph { .. } => "paragraph",
            BlockKind::Code { .. } => "code",
            BlockKind::Math { .. } => "math",
            BlockKind::Table { .. } => "table",
            BlockKind::Rule => "rule",
            BlockKind::Footnotes => "footnotes",
        })
        .collect()
}

fn inlines(block: &Block) -> &[Inline] {
    match &block.kind {
        BlockKind::Heading { inlines, .. } | BlockKind::Paragraph { inlines } => inlines,
        _ => &[],
    }
}

#[test]
fn a_readme_becomes_blocks_with_their_source_lines() {
    let rendered = render(README);
    assert_eq!(
        kinds(&rendered),
        [
            "heading",
            "paragraph",
            "heading",
            "paragraph",
            "paragraph",
            "paragraph",
            "paragraph",
            "paragraph",
            "paragraph",
            "paragraph",
            "table",
            "code",
            "code",
            "math",
            "paragraph",
            "paragraph",
            "paragraph",
            "rule",
            "heading",
            "footnotes",
            "paragraph"
        ]
    );
    let lines: Vec<u32> = rendered.blocks.iter().map(|block| block.line).collect();
    assert_eq!(lines[..4], [1, 3, 6, 8], "{lines:?}");
    let table = rendered.blocks.iter().find(|block| matches!(block.kind, BlockKind::Table { .. })).unwrap();
    assert_eq!((table.line, table.end_line), (20, 22));
}

#[test]
fn inline_styles_links_and_bare_addresses() {
    let rendered = render(README);
    let first = inlines(&rendered.blocks[1]);
    assert!(first.iter().any(|inline| inline.text == "bold" && inline.strong));
    assert!(first.iter().any(|inline| inline.text == "emphasis" && inline.emphasis));
    assert!(first.iter().any(|inline| inline.text == "strike" && inline.strike));
    assert!(first.iter().any(|inline| inline.text == "code" && inline.code));
    assert!(
        first.iter().any(|inline| inline.text == "link" && inline.link.as_deref() == Some("docs/guide.md#install"))
    );
    let address = first.iter().find(|inline| inline.text == "https://example.com/path").unwrap();
    assert_eq!(address.link.as_deref(), Some("https://example.com/path"), "the period stays out");
    let www = first.iter().find(|inline| inline.text == "www.rust-lang.org").unwrap();
    assert_eq!(www.link.as_deref(), Some("https://www.rust-lang.org"));
}

#[test]
fn lists_nest_with_markers_and_tasks() {
    let rendered = render(README);
    let items: Vec<&Block> = rendered
        .blocks
        .iter()
        .filter(|block| block.nesting.iter().any(|c| matches!(c, Container::Item { .. })))
        .collect();
    assert_eq!(items.len(), 5);
    assert_eq!(items[0].nesting, [Container::Item { marker: Some("1.".to_owned()), task: None }]);
    assert_eq!(items[1].nesting[0], Container::Item { marker: Some("2.".to_owned()), task: None });
    assert_eq!(items[2].nesting.len(), 2, "nested");
    assert_eq!(items[2].nesting[1], Container::Item { marker: Some("◦".to_owned()), task: None });
    assert_eq!(
        items[3].nesting[1],
        Container::Item { marker: Some("◦".to_owned()), task: Some(Task { checked: true, line: 11 }) }
    );
    assert_eq!(
        items[4].nesting[1],
        Container::Item { marker: Some("◦".to_owned()), task: Some(Task { checked: false, line: 12 }) }
    );
    assert_eq!(plain_text(inlines(items[3])), "done task");
}

#[test]
fn quotes_alerts_tables_code_and_math() {
    let rendered = render(README);
    let quote = rendered.blocks.iter().find(|block| plain_text(inlines(block)) == "A quote over two lines").unwrap();
    assert_eq!(quote.nesting, [Container::Quote { alert: None, first: true }]);
    let alert = rendered.blocks.iter().find(|block| plain_text(inlines(block)) == "Mind the gap.").unwrap();
    assert_eq!(alert.nesting, [Container::Quote { alert: Some(Alert::Warning), first: true }]);
    let BlockKind::Table { align, head, rows } =
        &rendered.blocks.iter().find(|b| matches!(b.kind, BlockKind::Table { .. })).unwrap().kind
    else {
        unreachable!()
    };
    assert_eq!(align, &[Align::Left, Align::Center, Align::Right]);
    assert_eq!(head.iter().map(|cell| plain_text(cell)).collect::<Vec<_>>(), ["Left", "Centre", "Right"]);
    assert!(rows[0][1][0].strong);
    let code: Vec<(&str, &str)> = rendered
        .blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::Code { language, text } => Some((language.as_str(), text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(code, [("rust", "fn main() {}\n"), ("mermaid", "graph TD; A-->B\n")]);
    assert!(rendered.blocks.iter().any(|block| block.kind == BlockKind::Math { text: "E = mc^2".to_owned() }));
    let inline_math = rendered.blocks.iter().flat_map(inlines).find(|inline| inline.math).unwrap();
    assert_eq!(inline_math.text, "x^2");
}

#[test]
fn footnotes_are_numbered_and_gathered_at_the_end() {
    let rendered = render(README);
    let reference = rendered.blocks.iter().flat_map(inlines).find(|inline| inline.footnote.is_some()).unwrap();
    assert_eq!((reference.footnote, reference.text.as_str()), (Some(1), "1"));
    let last = rendered.blocks.last().unwrap();
    assert_eq!(last.nesting, [Container::Footnote { number: 1, first: true }]);
    assert_eq!(plain_text(inlines(last)), "The footnote's text.");
    assert_eq!(rendered.blocks[rendered.blocks.len() - 2].kind, BlockKind::Footnotes);
}

#[test]
fn headings_get_githubs_anchors_and_the_outline() {
    let rendered = render(README);
    let anchors: Vec<&str> = rendered.headings.iter().map(|heading| heading.anchor.as_str()).collect();
    assert_eq!(anchors, ["zen-notes", "getting-started", "getting-started-1"]);
    assert_eq!(rendered.headings[1].line, 6);
    assert_eq!(block_of_anchor(&rendered, "#getting-started-1"), Some(rendered.blocks.len() - 3));
    assert_eq!(slug("What's new in 0.2?"), "whats-new-in-02");
    assert_eq!(slug("Árvíztűrő tükörfúrógép"), "árvíztűrő-tükörfúrógép");
}

#[test]
fn html_is_kept_safe() {
    let rendered = render(README);
    let details = rendered.blocks.iter().find(|block| plain_text(inlines(block)).starts_with("More")).unwrap();
    let text = plain_text(inlines(details));
    assert!(!text.contains("alert"), "a script is dropped with its content: {text:?}");
    assert!(
        inlines(details).iter().any(|inline| inline.text.contains("bold") && inline.strong),
        "{:?}",
        inlines(details)
    );
    assert!(inlines(details).iter().any(|inline| inline.text == "\n"), "<br> breaks");
    assert!(inlines(details)[0].strong, "the summary is strong");
    let image = render(
        "<p align=\"center\"><img src=\"logo.svg\" alt=\"Logo\" onload=\"x()\"></p>\n<iframe src=\"x\"></iframe>\n",
    );
    let found: Vec<&Inline> = image.blocks.iter().flat_map(inlines).collect();
    assert!(found.iter().any(|inline| inline.image.as_deref() == Some("logo.svg") && inline.text == "Logo"));
    assert!(!to_html(&image).contains("iframe") && !to_html(&image).contains("onload"));
    let inline = render("Press <kbd>⌘</kbd> and <script>bad()</script>go.");
    let text = plain_text(inlines(&inline.blocks[0]));
    assert_eq!(text, "Press ⌘ and go.");
    assert!(inlines(&inline.blocks[0]).iter().any(|inline| inline.text == "⌘" && inline.code));
}

#[test]
fn images_in_markdown_keep_their_description() {
    let rendered = render(README);
    let image = rendered.blocks.iter().flat_map(inlines).find(|inline| inline.image.is_some()).unwrap();
    assert_eq!((image.image.as_deref(), image.text.as_str()), (Some("images/logo.png"), "Logo"));
}

#[test]
fn the_preview_scrolls_to_the_block_of_a_line() {
    let rendered = render(README);
    assert_eq!(block_at_line(&rendered, 1), Some(0));
    assert_eq!(block_at_line(&rendered, 7), Some(2), "the heading on line 6 is still at the top");
    let table = rendered.blocks.iter().position(|block| matches!(block.kind, BlockKind::Table { .. })).unwrap();
    assert_eq!(block_at_line(&rendered, 21), Some(table));
    assert_eq!(block_at_line(&rendered, 24), Some(table + 1), "the code block after it");
    assert_eq!(block_at_line(&render(""), 3), None);
}

#[test]
fn agents_read_it_as_html_and_text() {
    let rendered = render(README);
    let html = to_html(&rendered);
    for part in [
        "<h1 id=\"zen-notes\" data-line=\"1\">Zen Notes</h1>",
        "<strong>bold</strong>",
        "<del>strike</del>",
        "<a href=\"https://example.com/path\">https://example.com/path</a>",
        "<input type=\"checkbox\" disabled checked>",
        "<th style=\"text-align:center\">Centre</th>",
        "<code class=\"language-rust\">fn main() {}",
        "<p class=\"alert-title\">Warning</p>",
    ] {
        assert!(html.contains(part), "{part} in {html}");
    }
    assert!(!html.contains("<script"));
    let text = to_text(&rendered);
    assert!(text.contains("> [WARNING] Mind the gap."), "{text}");
    assert!(text.contains("[x] done task") && text.contains("[ ] open task"), "{text}");
    assert!(text.contains("| Left | Centre | Right |"));
}

#[test]
fn a_task_is_toggled_in_its_line() {
    assert_eq!(toggled_task("   - [x] done").as_deref(), Some("   - [ ] done"));
    assert_eq!(toggled_task("- [ ] open").as_deref(), Some("- [x] open"));
    assert_eq!(toggled_task("1. [ ] first").as_deref(), Some("1. [x] first"));
    assert_eq!(toggled_task("> - [X] quoted").as_deref(), Some("> - [ ] quoted"));
    assert_eq!(toggled_task("text [ ] not a task"), None);
}

#[test]
fn hostile_input_stays_bounded() {
    let deep = format!("{}text", "> ".repeat(500));
    render(&deep);
    let lists = (0..300).map(|depth| format!("{}- item\n", "  ".repeat(depth))).collect::<String>();
    render(&lists);
    render("<script>");
    render("[^x]: dangling");
    render("| a |\n|---|\n| b | c | d |\n");
}

mod commands {
    use serde_json::{Value, json};
    use zen_core::{Registry, Session};

    use super::super::RemoteImages;
    use super::README;
    use crate::Workbench;

    struct Fixture {
        registry: Registry,
        session: Session,
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            zen_i18n::register(crate::LOCALES);
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join("project")).unwrap();
            std::fs::write(dir.path().join("project/README.md"), README).unwrap();
            let mut registry = Registry::new();
            crate::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry
                .dispatch(&mut session, "file.project.open", json!({ "path": dir.path().join("project") }))
                .unwrap();
            session.state_mut::<RemoteImages>().set_data_dir(dir.path().join("data"));
            let mut fixture = Self { registry, session, dir };
            let readme = fixture.dir.path().join("project/README.md");
            fixture.run("file.document.open", json!({ "path": readme }));
            fixture
        }

        fn run(&mut self, id: &str, params: Value) -> Value {
            self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
        }
    }

    #[test]
    fn agents_read_the_preview_as_text_html_or_blocks() {
        let mut fixture = Fixture::new();
        let text = fixture.run("markdown.preview.render", json!({}));
        assert!(text["text"].as_str().unwrap().contains("> [WARNING] Mind the gap."), "{text}");
        assert_eq!(text["headings"][1]["anchor"], "getting-started");
        let html = fixture.run("markdown.preview.render", json!({ "format": "html", "path": "README.md" }));
        assert!(html["html"].as_str().unwrap().contains("<h2 id=\"getting-started\" data-line=\"6\">"));
        let blocks = fixture.run("markdown.preview.render", json!({ "format": "blocks" }));
        assert_eq!(blocks["rendered"]["blocks"][0]["type"], "heading");
        assert!(
            fixture
                .registry
                .dispatch(&mut fixture.session, "markdown.preview.render", json!({ "format": "pdf" }))
                .is_err()
        );
        let info = fixture.run("editor.preview.getMode", json!({ "waitMs": 5000 }));
        assert_eq!(
            (&info["kind"], &info["mode"], &info["state"]),
            (&json!("markdown"), &json!("editorAndPreview"), &json!("ready"))
        );
        assert_eq!(info["blocks"], json!(21));
    }

    #[test]
    fn a_task_is_ticked_from_the_preview_as_one_edit() {
        let mut fixture = Fixture::new();
        let toggled = fixture.run("markdown.task.toggle", json!({ "line": 12 }));
        assert_eq!(toggled, json!({ "checked": true, "text": "   - [x] open task" }));
        let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert!(document.text().contains("   - [x] open task\n") && document.is_dirty());
        fixture.run("editor.edit.undo", json!({}));
        let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
        assert!(document.text().contains("   - [ ] open task\n"), "one undo step");
        assert!(fixture.registry.dispatch(&mut fixture.session, "markdown.task.toggle", json!({ "line": 1 })).is_err());
    }

    #[test]
    fn remote_images_are_allowed_per_project_and_kept() {
        let mut fixture = Fixture::new();
        assert_eq!(fixture.run("markdown.images.list", json!({})), json!({ "hosts": [] }));
        let allowed = fixture.run("markdown.images.allow", json!({ "host": "https://IMG.shields.io/badge/x.svg" }));
        assert_eq!(allowed, json!({ "hosts": ["img.shields.io"] }));
        let mut again = RemoteImages::default();
        again.set_data_dir(fixture.dir.path().join("data"));
        let root = fixture.session.state::<Workbench>().unwrap().project().unwrap().to_owned();
        assert!(again.allows(&root, "img.shields.io") && !again.allows(&root, "example.com"));
        assert!(!again.allows(std::path::Path::new("/other"), "img.shields.io"), "per project");
        fixture.run("markdown.images.allow", json!({ "host": "img.shields.io", "allowed": false }));
        assert_eq!(fixture.run("markdown.images.list", json!({})), json!({ "hosts": [] }));
        assert_eq!(super::super::host_of("http://user@cdn.example.com:8080/a.png").as_deref(), Some("cdn.example.com"));
        assert_eq!(super::super::host_of("images/logo.png"), None);
    }
}

mod selection_tests {
    use super::super::selection::*;
    use super::super::*;

    const DOC: &str = "# Title\n\nOne `two` three.\n\n- first\n- [x] done\n  - nested\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\nlet x = 1;\n```\n\nLast words.\n";

    fn fragments(rendered: &Rendered) -> Vec<Fragment> {
        let mut fragments = Vec::new();
        for (index, block) in rendered.blocks.iter().enumerate() {
            match &block.kind {
                BlockKind::Heading { inlines, .. } | BlockKind::Paragraph { inlines } => {
                    fragments.push(Fragment::new(Part::of(index), plain_text(inlines)));
                }
                BlockKind::Code { text, .. } => {
                    fragments.push(Fragment::new(Part::of(index), text.strip_suffix('\n').unwrap_or(text)));
                }
                BlockKind::Table { head, rows, .. } => {
                    for (row, cells) in std::iter::once(head).chain(rows).enumerate() {
                        for (column, cell) in cells.iter().enumerate() {
                            fragments.push(Fragment::new(Part::cell(index, row, column), plain_text(cell)));
                        }
                    }
                }
                _ => {}
            }
        }
        fragments
    }

    fn index_of(rendered: &Rendered, text: &str) -> usize {
        rendered
            .blocks
            .iter()
            .position(|block| match &block.kind {
                BlockKind::Paragraph { inlines } | BlockKind::Heading { inlines, .. } => plain_text(inlines) == text,
                _ => false,
            })
            .unwrap_or_else(|| panic!("no block {text:?}"))
    }

    #[test]
    fn select_all_copies_readable_plain_text_with_markers_cells_and_code() {
        let rendered = render(DOC);
        let fragments = fragments(&rendered);
        let all = select_all(&fragments).unwrap();
        assert_eq!(
            selected_text(&rendered, &fragments, &all),
            "Title\n\nOne two three.\n\n• first\n[x] done\n  ◦ nested\n\nA\tB\n1\t2\n\nfn main() {}\nlet x = 1;\n\nLast words."
        );
        assert_eq!(selected_source(DOC, &rendered, &all), DOC.trim_end());
    }

    #[test]
    fn a_drag_across_blocks_copies_from_inside_one_to_inside_another() {
        let rendered = render(DOC);
        let fragments = fragments(&rendered);
        let first = index_of(&rendered, "One two three.");
        let last = index_of(&rendered, "Last words.");
        let selection = Selection::new(Spot { part: Part::of(last), at: 4 }, Spot { part: Part::of(first), at: 8 });
        let text = selected_text(&rendered, &fragments, &selection);
        assert!(text.starts_with("three.\n\n• first\n"), "{text}");
        assert!(text.ends_with("let x = 1;\n\nLast"), "{text}");
        assert_eq!(
            selected_source(DOC, &rendered, &selection),
            "One `two` three.\n\n- first\n- [x] done\n  - nested\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\nlet x = 1;\n```\n\nLast words."
        );
        assert_eq!(selected_text(&rendered, &fragments, &Selection::caret(selection.head)), "");
    }

    #[test]
    fn a_double_click_takes_a_word_and_a_triple_click_its_block() {
        let rendered = render(DOC);
        let fragments = fragments(&rendered);
        let block = index_of(&rendered, "One two three.");
        let word = select_word(&fragments, Spot { part: Part::of(block), at: 9 });
        assert_eq!(selected_text(&rendered, &fragments, &word), "three");
        let inside = select_word(&fragments, Spot { part: Part::of(block), at: 2 });
        assert_eq!(selected_text(&rendered, &fragments, &inside), "One");
        let end = select_word(&fragments, Spot { part: Part::of(block), at: 14 });
        assert_eq!(selected_text(&rendered, &fragments, &end), ".", "past the end takes the last character");
        let dot = select_word(&fragments, Spot { part: Part::of(block), at: 13 });
        assert_eq!(selected_text(&rendered, &fragments, &dot), ".");
        let whole = select_block(&fragments, block).unwrap();
        assert_eq!(selected_text(&rendered, &fragments, &whole), "One two three.");
        let table = rendered.blocks.iter().position(|block| matches!(block.kind, BlockKind::Table { .. })).unwrap();
        assert_eq!(selected_text(&rendered, &fragments, &select_block(&fragments, table).unwrap()), "A\tB\n1\t2");
    }

    #[test]
    fn inline_code_padding_never_reaches_the_clipboard() {
        let rendered = render("Run `ls` now.\n");
        let fragments = vec![Fragment::new(Part::of(0), "Run \u{2009}ls\u{2009} now.")];
        let all = select_all(&fragments).unwrap();
        assert_eq!(selected_text(&rendered, &fragments, &all), "Run ls now.");
    }
}

fn glyph(pieces: &[math::Piece], wanted: char) -> math::Piece {
    *pieces
        .iter()
        .find(|piece| piece.ink == math::Ink::Glyph(wanted))
        .unwrap_or_else(|| panic!("no {wanted:?} in {pieces:?}"))
}

fn centre(piece: &math::Piece) -> f32 {
    piece.left + piece.width / 2.0
}

#[test]
fn a_math_fence_is_display_math() {
    let rendered = render("```math\n\\frac{a}{b}\n```\n\n```Math\nx\n```\n");
    let kinds: Vec<_> = rendered.blocks.iter().map(|block| block.kind.clone()).collect();
    assert_eq!(kinds, [BlockKind::Math { text: "\\frac{a}{b}".to_owned() }, BlockKind::Math { text: "x".to_owned() }]);
}

#[test]
fn math_raises_a_superscript_and_lowers_a_subscript_beside_their_base() {
    let pieces = math::pieces("x^2 y_i", false, 20.0).unwrap();
    let (x, two, y, i) = (glyph(&pieces, '𝑥'), glyph(&pieces, '2'), glyph(&pieces, '𝑦'), glyph(&pieces, '𝑖'));
    assert_eq!(x.raise, 0.0);
    assert!(two.raise > 4.0 && two.left >= x.right() - 1.0, "{two:?} after {x:?}");
    assert!(i.raise < -1.0 && i.left >= y.right() - 1.0, "{i:?} after {y:?}");
    assert!(two.ascent < x.ascent + 6.0 && two.width < 12.0, "a script is smaller: {two:?}");
}

#[test]
fn math_sets_a_fraction_over_its_rule_with_both_parts_centred() {
    let pieces = math::pieces(r"\frac{a+b}{c}", true, 20.0).unwrap();
    let rule = *pieces.iter().find(|piece| piece.ink == math::Ink::Rule).unwrap();
    let (a, c) = (glyph(&pieces, '𝑎'), glyph(&pieces, '𝑐'));
    assert!(a.bottom() > rule.top(), "the numerator is above the rule: {a:?} {rule:?}");
    assert!(c.top() < rule.bottom(), "the denominator is below it: {c:?} {rule:?}");
    assert!((centre(&c) - centre(&rule)).abs() < 1.0, "the denominator is centred: {c:?} {rule:?}");
    assert!(rule.raise > 0.0, "the rule is on the math axis, above the baseline");
    let metrics = math::measure(r"\frac{a+b}{c}", true, 20.0).unwrap();
    assert!((metrics.width - rule.width).abs() < 3.0 && metrics.descent > 5.0, "{metrics:?}");
}

#[test]
fn math_puts_display_limits_over_and_under_a_sum_and_inline_limits_beside_it() {
    let display = math::pieces(r"\sum_{i=1}^{n} i", true, 20.0).unwrap();
    let (sum, n) = (glyph(&display, '∑'), glyph(&display, '𝑛'));
    let one = glyph(&display, '1');
    assert!(n.bottom() > sum.top() && (centre(&n) - centre(&sum)).abs() < 2.0, "{n:?} over {sum:?}");
    assert!(one.top() < sum.bottom(), "{one:?} under {sum:?}");
    let inline = math::pieces(r"\sum_{i=1}^{n} i", false, 20.0).unwrap();
    let (small, n) = (glyph(&inline, '∑'), glyph(&inline, '𝑛'));
    assert!(n.left >= small.right() - 1.0, "{n:?} beside {small:?}");
    assert!(small.ascent < sum.ascent, "the inline sum is smaller");
    let integral = math::pieces(r"\int_0^\infty e^{-x^2}\,dx", true, 20.0).unwrap();
    assert!(glyph(&integral, '∞').raise > 0.0);
    let product = math::pieces(r"\prod_{k=1}^{m} k", true, 20.0).unwrap();
    assert!(glyph(&product, '𝑚').bottom() > glyph(&product, '∏').top());
}

#[test]
fn math_draws_a_root_with_a_rule_over_the_radicand() {
    let pieces = math::pieces(r"\sqrt{x}", false, 20.0).unwrap();
    let x = glyph(&pieces, '𝑥');
    let rule = *pieces.iter().find(|piece| piece.ink == math::Ink::Rule).unwrap();
    assert!(rule.bottom() >= x.top() - 0.5 && rule.left <= x.left && rule.right() >= x.right(), "{rule:?} {x:?}");
    let radical = pieces.iter().find(|piece| piece.right() <= x.left + 0.5 && piece.ink != math::Ink::Rule);
    assert!(radical.is_some(), "the radical sign is left of the radicand: {pieces:?}");
}

#[test]
fn math_grows_left_and_right_delimiters_around_a_fraction() {
    let plain = math::pieces("(x)", true, 20.0).unwrap();
    let grown = math::pieces(r"\left( \frac{a}{b} \right)", true, 20.0).unwrap();
    let height = |pieces: &[math::Piece]| {
        let paren = pieces.iter().find(|piece| matches!(piece.ink, math::Ink::Glyph(_))).unwrap();
        paren.ascent + paren.descent
    };
    assert!(height(&grown) > height(&plain) * 1.3, "{grown:?}");
}

#[test]
fn math_knows_greek_letters_symbols_text_and_styled_alphabets() {
    let pieces = math::pieces(r"\alpha + \Gamma \leq \infty \neq \mathbb{R} \text{ if } x", false, 20.0).unwrap();
    for wanted in ['𝛼', 'Γ', '≤', '∞', '≠', 'ℝ', 'i', 'f', '𝑥'] {
        glyph(&pieces, wanted);
    }
    let lefts: Vec<f32> = pieces.iter().map(|piece| piece.left).collect();
    assert!(lefts.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn math_lays_matrices_and_aligned_rows_in_rows_and_columns() {
    let pieces = math::pieces(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", true, 20.0).unwrap();
    let (a, b, c, d) = (glyph(&pieces, '𝑎'), glyph(&pieces, '𝑏'), glyph(&pieces, '𝑐'), glyph(&pieces, '𝑑'));
    assert!(a.raise > c.raise && (a.raise - b.raise).abs() < 0.5, "rows: {a:?} {b:?} {c:?}");
    assert!(b.left > a.right() && (centre(&a) - centre(&c)).abs() < 1.0, "columns: {a:?} {b:?} {c:?}");
    assert!(d.left > c.right());
    let aligned = math::pieces(r"\begin{aligned} a &= b \\ cc &= d \end{aligned}", true, 20.0).unwrap();
    let equals: Vec<_> = aligned.iter().filter(|piece| piece.ink == math::Ink::Glyph('=')).collect();
    assert_eq!(equals.len(), 2);
    assert!((equals[0].left - equals[1].left).abs() < 0.5, "aligned at &: {equals:?}");
}

#[test]
fn math_that_cannot_be_read_is_an_error_naming_the_trouble() {
    assert!(math::check(r"\frac{1}{").unwrap_err().contains('{'));
    assert!(math::check(r"\nosuchcommand{x}").unwrap_err().contains(r"\nosuchcommand"));
    assert!(math::check(r"\left( x").is_err());
    assert!(math::check(r"e^{i\pi} + 1 = 0").is_ok());
}

#[test]
fn typeset_math_is_a_picture_of_its_size_in_the_colour_asked() {
    let formula = math::typeset(r"\frac{1}{2}", true, 20.0, 2.0, [200, 40, 10]).unwrap();
    let metrics = formula.metrics;
    assert_eq!(metrics, math::measure(r"\frac{1}{2}", true, 20.0).unwrap());
    assert_eq!(formula.picture.width, (metrics.width * 2.0).ceil() as usize);
    assert_eq!(formula.picture.height, (metrics.height() * 2.0).ceil() as usize);
    let inked: Vec<&[u8]> = formula.picture.rgba.chunks(4).filter(|pixel| pixel[3] == 255).collect();
    assert!(!inked.is_empty());
    assert!(inked.iter().all(|pixel| pixel[..3] == [200, 40, 10]), "{:?}", &inked[..4.min(inked.len())]);
}
