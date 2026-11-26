use super::*;

fn kinds(text: &str) -> Vec<(&str, Kind)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

fn labels(text: &str) -> Vec<String> {
    let caret = text.find('|').expect("a caret");
    let text = text.replacen('|', "", 1);
    complete(&text, caret)
        .map(|completion| completion.items.into_iter().map(|item| item.label).collect())
        .unwrap_or_default()
}

fn completion(text: &str) -> Option<Completion> {
    let caret = text.find('|').expect("a caret");
    complete(&text.replacen('|', "", 1), caret)
}

fn expanded(text: &str) -> Option<String> {
    let caret = text.find('|').expect("a caret");
    let source = text.replacen('|', "", 1);
    let expansion = expand(&source, caret)?;
    assert_eq!(expansion.to, caret);
    Some(format!("{}{}{}", &source[..expansion.from], expansion.text, &source[caret..]))
}

fn abbr(abbreviation: &str) -> Option<String> {
    expand_abbreviation(abbreviation, "", "    ")
}

#[test]
fn highlights_tags_attributes_and_values() {
    assert_eq!(
        kinds("<a href=\"/x?a=1&amp;b=2\" hidden class='c' id=u>Tom &amp; Jerry</a>"),
        [
            ("<", Kind::Punctuation),
            ("a", Kind::Keyword),
            ("href", Kind::Attribute),
            ("=", Kind::Punctuation),
            ("\"/x?a=1", Kind::String),
            ("&amp;", Kind::Constant),
            ("b=2\"", Kind::String),
            ("hidden", Kind::Attribute),
            ("class", Kind::Attribute),
            ("=", Kind::Punctuation),
            ("'c'", Kind::String),
            ("id", Kind::Attribute),
            ("=", Kind::Punctuation),
            ("u", Kind::String),
            (">", Kind::Punctuation),
            ("&amp;", Kind::Constant),
            ("</", Kind::Punctuation),
            ("a", Kind::Keyword),
            (">", Kind::Punctuation),
        ]
    );
}

#[test]
fn highlights_doctype_comments_and_entities() {
    assert_eq!(
        kinds("<!DOCTYPE html>\n<!-- a <b> -->\n&copy; &#169; &#xA9; &nope & x;<br/><?xml v?><![CDATA[x]]><!bogus>"),
        [
            ("<!DOCTYPE html>", Kind::Macro),
            ("<!-- a <b> -->", Kind::Comment),
            ("&copy;", Kind::Constant),
            ("&#169;", Kind::Constant),
            ("&#xA9;", Kind::Constant),
            ("<", Kind::Punctuation),
            ("br", Kind::Keyword),
            ("/>", Kind::Punctuation),
            ("<?xml v?>", Kind::Macro),
            ("<![CDATA[x]]>", Kind::Macro),
            ("<!bogus>", Kind::Comment),
        ]
    );
}

#[test]
fn leaves_script_and_style_plain() {
    assert_eq!(
        kinds("<script>if (a < b && c) x = '</p>';</script><style>a > b {}</STYLE >"),
        [
            ("<", Kind::Punctuation),
            ("script", Kind::Keyword),
            (">", Kind::Punctuation),
            ("</", Kind::Punctuation),
            ("script", Kind::Keyword),
            (">", Kind::Punctuation),
            ("<", Kind::Punctuation),
            ("style", Kind::Keyword),
            (">", Kind::Punctuation),
            ("</", Kind::Punctuation),
            ("STYLE", Kind::Keyword),
            (">", Kind::Punctuation),
        ]
    );
    assert_eq!(
        kinds("<title>A &amp; <b></title>"),
        [
            ("<", Kind::Punctuation),
            ("title", Kind::Keyword),
            (">", Kind::Punctuation),
            ("&amp;", Kind::Constant),
            ("</", Kind::Punctuation),
            ("title", Kind::Keyword),
            (">", Kind::Punctuation),
        ]
    );
}

#[test]
fn text_with_angle_brackets_stays_text() {
    assert_eq!(kinds("a < b, 1<2, <> </ > <3"), []);
    assert_eq!(
        kinds("<p\n  class=\"x\n y\"\n>"),
        [
            ("<", Kind::Punctuation),
            ("p", Kind::Keyword),
            ("class", Kind::Attribute),
            ("=", Kind::Punctuation),
            ("\"x\n y\"", Kind::String),
            (">", Kind::Punctuation),
        ]
    );
}

#[test]
fn an_unclosed_tag_ends_at_the_next_tag() {
    assert_eq!(
        kinds("<div class\n<p>"),
        [
            ("<", Kind::Punctuation),
            ("div", Kind::Keyword),
            ("class", Kind::Attribute),
            ("<", Kind::Punctuation),
            ("p", Kind::Keyword),
            (">", Kind::Punctuation),
        ]
    );
}

#[test]
fn highlighting_survives_any_input() {
    let pieces = [
        "<",
        ">",
        "/",
        "</",
        "<!",
        "<!--",
        "-->",
        "<?",
        "=",
        "\"",
        "'",
        "&",
        ";",
        "#",
        "x",
        "a ",
        "script",
        "\n",
        "é",
        "日",
        "]]>",
        "<![CDATA[",
        "&amp;",
    ];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                let text = format!("{a}{b}{c}");
                let mut previous = 0;
                for span in highlight(&text) {
                    assert!(
                        span.start >= previous
                            && span.end > span.start
                            && span.end <= text.len()
                            && text.is_char_boundary(span.start)
                            && text.is_char_boundary(span.end),
                        "{text:?}"
                    );
                    previous = span.end;
                }
                for fold in fold(&text) {
                    assert!(fold.end_line > fold.start_line, "{text:?}");
                }
                outline(&text);
                for caret in 0..=text.len() {
                    if let Some(completion) = complete(&text, caret) {
                        assert!(completion.from <= caret, "{text:?} {caret}");
                    }
                    if let Some(expansion) = expand(&text, caret) {
                        assert!(expansion.from < expansion.to && text.is_char_boundary(expansion.from), "{text:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn folds_multi_line_elements_and_comments() {
    let text = "\
<html>
<head>
  <title>x</title>
</head>
<body>
  <!--
    note
  -->
  <ul>
    <li>one
    <li>two
      and more
  </ul>
  <p>one line</p>
  <img
    src=a>
</body>
</html>
";
    let folds: Vec<(u32, u32)> = fold(text).into_iter().map(|fold| (fold.start_line, fold.end_line)).collect();
    assert_eq!(folds, [(0, 17), (1, 3), (4, 16), (5, 7), (8, 12), (10, 11), (14, 15)]);
}

#[test]
fn outlines_the_element_tree() {
    let text = "<!doctype html><body><div id=\"main\" class=\"a  b\"><p>x<p>y</div><br><x-el/></body>";
    let symbols = outline(text);
    assert_eq!(symbols.len(), 1);
    let body = &symbols[0];
    assert_eq!((body.name.as_str(), body.kind, body.start, body.end), ("body", "element", 15, text.len()));
    let names: Vec<&str> = body.children.iter().map(|child| child.name.as_str()).collect();
    assert_eq!(names, ["div#main.a.b", "br", "x-el"]);
    let div = &body.children[0];
    let paragraphs: Vec<&str> = div.children.iter().map(|child| &text[child.start..child.end]).collect();
    assert_eq!(paragraphs, ["<p>x", "<p>y"]);
    assert_eq!(&text[div.start..div.end], "<div id=\"main\" class=\"a  b\"><p>x<p>y</div>");
    assert_eq!(
        outline_json(&outline("<a title='\"q\"'><b>")),
        "[{\"name\":\"a\",\"kind\":\"element\",\"start\":0,\"end\":18,\"children\":[{\"name\":\"b\",\"kind\":\"element\",\"start\":15,\"end\":18,\"children\":[]}]}]"
    );
}

#[test]
fn implied_end_tags() {
    let text =
        "<table><tr><td>1<td>2<tr><th>3</table><dl><dt>a<dd>b<dt>c</dl><select><option>1<optgroup><option>2</select>";
    let symbols = outline(text);
    let tree: Vec<String> = symbols
        .iter()
        .map(|symbol| {
            let rows: Vec<String> = symbol
                .children
                .iter()
                .map(|row| {
                    format!(
                        "{}({})",
                        row.name,
                        row.children.iter().map(|cell| cell.name.as_str()).collect::<Vec<_>>().join(",")
                    )
                })
                .collect();
            format!("{}[{}]", symbol.name, rows.join(" "))
        })
        .collect();
    assert_eq!(tree, ["table[tr(td,td) tr(th)]", "dl[dt() dd() dt()]", "select[option() optgroup(option)]"]);
}

#[test]
fn completes_tag_names() {
    let found = labels("<div><s|");
    assert!(found.starts_with(&["style".to_owned(), "section".to_owned()]), "{found:?}");
    assert!(found.iter().all(|label| label.starts_with('s')), "{found:?}");
    assert_eq!(
        completion("<|").map(|completion| (completion.from, completion.items.len())),
        Some((1, data::ELEMENTS.len()))
    );
    let item = &completion("x <Bu|").expect("items").items[0];
    assert_eq!((item.label.as_str(), item.kind, item.insert.as_str()), ("button", "tag", "button"));
}

#[test]
fn completes_end_tags() {
    let found = completion("<ul><li><a href=x>text</|");
    let found = found.expect("items");
    assert_eq!(found.from, 24);
    let inserts: Vec<&str> = found.items.iter().map(|item| item.insert.as_str()).collect();
    assert_eq!(inserts, ["a>", "li>", "ul>"]);
    assert_eq!(labels("<div><p></p><Section></s|"), ["Section"]);
    assert_eq!(labels("<div></div></|"), Vec::<String>::new());
    assert_eq!(labels("<div><br></|"), ["div"]);
}

#[test]
fn completes_attribute_names() {
    let found = completion("<img src=\"a.png\" |").expect("items");
    assert_eq!(found.from, 17);
    assert_eq!(found.items[0].label, "alt");
    assert!(!found.items.iter().any(|item| item.label == "src"));
    assert!(found.items.iter().any(|item| item.label == "class"));
    let found = completion("<input ty|").expect("items");
    assert_eq!(found.from, 7);
    assert_eq!(found.items.iter().map(|item| item.insert.as_str()).collect::<Vec<_>>(), ["type=\"$0\""]);
    assert_eq!(labels("<input req|"), ["required"]);
    assert_eq!(completion("<input req|").expect("items").items[0].insert, "required");
    assert_eq!(labels("<custom-el aria|"), Vec::<String>::new());
    assert!(labels("<custom-el |").contains(&"id".to_owned()));
}

#[test]
fn completes_enumerated_values() {
    assert_eq!(labels("<input type=\"c|"), ["checkbox", "color"]);
    let found = completion("<a target=|").expect("items");
    assert_eq!(found.from, 10);
    assert_eq!(found.items[0].insert, "\"_blank\"");
    assert_eq!(labels("<link rel='style|"), ["stylesheet"]);
    assert_eq!(labels("<button type=\"|"), ["button", "submit", "reset"]);
    assert_eq!(labels("<div dir=\"|"), ["ltr", "rtl", "auto"]);
    assert_eq!(labels("<div title=\"|"), Vec::<String>::new());
}

#[test]
fn completes_entities_and_nothing_elsewhere() {
    let found = completion("a &hel|").expect("items");
    assert_eq!((found.from, found.items[0].insert.as_str()), (2, "&hellip;"));
    assert_eq!(labels("plain text|"), Vec::<String>::new());
    assert_eq!(labels("<!-- <d|"), Vec::<String>::new());
    assert_eq!(labels("<script>x <d|"), Vec::<String>::new());
    assert_eq!(complete("<d", 99).map(|completion| completion.from), Some(1));
}

#[test]
fn encodes_completion_json() {
    let completion =
        Completion { from: 3, items: vec![Item { label: "a\"b".to_owned(), kind: "tag", insert: "x\n$0".to_owned() }] };
    assert_eq!(
        completion_json(&completion),
        "{\"from\":3,\"items\":[{\"label\":\"a\\\"b\",\"kind\":\"tag\",\"insert\":\"x\\n$0\"}]}"
    );
}

#[test]
fn expands_elements_ids_classes_and_attributes() {
    assert_eq!(abbr("div").as_deref(), Some("<div>$0</div>"));
    assert_eq!(abbr("#main").as_deref(), Some("<div id=\"main\">$0</div>"));
    assert_eq!(abbr(".a.b").as_deref(), Some("<div class=\"a b\">$0</div>"));
    assert_eq!(abbr("a").as_deref(), Some("<a href=\"$0\"></a>"));
    assert_eq!(
        abbr("a#x.y[title=\"T t\" target=_blank]{go}").as_deref(),
        Some("<a href=\"$0\" id=\"x\" class=\"y\" title=\"T t\" target=\"_blank\">go</a>")
    );
    assert_eq!(abbr("img").as_deref(), Some("<img src=\"$0\" alt=\"\">"));
    assert_eq!(abbr("input:email[required]").as_deref(), Some("<input type=\"email\" required>"));
    assert_eq!(abbr("a[href=/home]").as_deref(), Some("<a href=\"/home\">$0</a>"));
    assert_eq!(abbr("p[data-x]").as_deref(), Some("<p data-x=\"$0\"></p>"));
    assert_eq!(abbr("br").as_deref(), Some("<br>"));
    assert_eq!(abbr("{hello}"), None, "text alone stays as it is");
    assert_eq!(abbr("btn:s").as_deref(), Some("<button type=\"submit\">$0</button>"));
}

#[test]
fn expands_children_siblings_and_climbing() {
    assert_eq!(abbr("ul>li").as_deref(), Some("<ul>\n    <li>$0</li>\n</ul>"));
    assert_eq!(abbr("p>span+em").as_deref(), Some("<p><span>$0</span><em></em></p>"));
    assert_eq!(abbr("div+p").as_deref(), Some("<div>$0</div>\n<p></p>"));
    assert_eq!(
        abbr("div>p>span^h1^^footer").as_deref(),
        Some("<div>\n    <p><span>$0</span></p>\n    <h1></h1>\n</div>\n<footer></footer>")
    );
    assert_eq!(
        abbr("nav>(ul>li*2>a)+p").as_deref(),
        Some(
            "<nav>\n    <ul>\n        <li><a href=\"$0\"></a></li>\n        <li><a href=\"\"></a></li>\n    </ul>\n    <p></p>\n</nav>"
        )
    );
    assert_eq!(abbr("(dt+dd)*2").as_deref(), Some("<dt>$0</dt>\n<dd></dd>\n<dt></dt>\n<dd></dd>"));
}

#[test]
fn expands_multiplication_and_numbering() {
    assert_eq!(
        abbr("ul>li.item$*3").as_deref(),
        Some(
            "<ul>\n    <li class=\"item1\">$0</li>\n    <li class=\"item2\"></li>\n    <li class=\"item3\"></li>\n</ul>"
        )
    );
    assert_eq!(abbr("h$*2").as_deref(), Some("<h1>$0</h1>\n<h2></h2>"));
    assert_eq!(abbr("i.n$$$*2").as_deref(), Some("<i class=\"n001\">$0</i><i class=\"n002\"></i>"));
    assert_eq!(abbr("b{$@-}*3").as_deref(), Some("<b>3</b><b>2</b><b>1</b>"));
    assert_eq!(abbr("b{$@5}*2").as_deref(), Some("<b>5</b><b>6</b>"));
    assert_eq!(abbr("b{\\$}").as_deref(), Some("<b>$</b>"));
    assert_eq!(
        abbr("ul>li*2>a{item $}").as_deref(),
        Some("<ul>\n    <li><a href=\"$0\">item 1</a></li>\n    <li><a href=\"\">item 2</a></li>\n</ul>")
    );
}

#[test]
fn expands_implicit_tag_names() {
    assert_eq!(abbr("ul>.x").as_deref(), Some("<ul>\n    <li class=\"x\">$0</li>\n</ul>"));
    assert_eq!(
        abbr("table>.row>.cell").as_deref(),
        Some("<table>\n    <tr class=\"row\">\n        <td class=\"cell\">$0</td>\n    </tr>\n</table>")
    );
    assert_eq!(abbr("select>.o").as_deref(), Some("<select>\n    <option class=\"o\">$0</option>\n</select>"));
    assert_eq!(abbr("em>.x").as_deref(), Some("<em><span class=\"x\">$0</span></em>"));
}

#[test]
fn expands_the_html5_skeleton() {
    let skeleton = abbr("!").expect("a skeleton");
    assert!(skeleton.starts_with("<!doctype html>\n<html lang=\"en\">\n<head>\n    <meta charset=\"UTF-8\">"));
    assert!(skeleton.contains("<body>\n    $0\n</body>\n</html>"));
    assert_eq!(abbr("html:5"), Some(skeleton));
}

#[test]
fn rejects_what_is_not_an_abbreviation() {
    for text in [
        "hello",
        "e.g.",
        "a>",
        ">",
        "div*0",
        "(div",
        "div)",
        "a[x",
        "a{x",
        "me@example.com",
        "$5",
        "5",
        "div..x",
        "lorem",
    ] {
        assert_eq!(abbr(text), None, "{text}");
    }
}

#[test]
fn finds_the_abbreviation_before_the_caret() {
    assert_eq!(
        expanded("<body>\n    ul>li|\n</body>").as_deref(),
        Some("<body>\n    <ul>\n        <li>$0</li>\n    </ul>\n</body>")
    );
    assert_eq!(expanded("<p>see div.x| now</p>").as_deref(), Some("<p>see <div class=\"x\">$0</div> now</p>"));
    assert_eq!(expanded("<p>a{x y}|").as_deref(), Some("<p><a href=\"$0\">x y</a>"));
    assert_eq!(expanded("\t\tul>li|").as_deref(), Some("\t\t<ul>\n\t\t\t<li>$0</li>\n\t\t</ul>"));
    assert_eq!(expanded("<div>\n  <p>\n  p>b|").as_deref(), Some("<div>\n  <p>\n  <p><b>$0</b></p>"));
    assert_eq!(expanded("<div class=\"a|\">"), None);
    assert_eq!(expanded("<div |"), None);
    assert_eq!(expanded("<!-- ul>li|"), None);
    assert_eq!(expanded("text |"), None);
    assert_eq!(expanded("hello|"), None);
    assert_eq!(expanded("x, div|").as_deref(), Some("x, <div>$0</div>"));
}

#[test]
fn encodes_records_and_input() {
    assert_eq!(encode_spans(&[Span { start: 1, end: 3, kind: Kind::Attribute }]), [1, 0, 0, 0, 3, 0, 0, 0, 9, 0, 0, 0]);
    assert_eq!(encode_folds(&[Fold { start_line: 1, end_line: 258 }]), [1, 0, 0, 0, 2, 1, 0, 0]);
    assert_eq!(split_caret_input(&[2, 0, 0, 0, b'<', b'a']), Some((2, "<a")));
    assert_eq!(split_caret_input(&[2, 0]), None);
    assert_eq!(
        expansion_json(&Expansion { from: 0, to: 2, text: "<b>\t</b>".to_owned() }),
        "{\"from\":0,\"to\":2,\"text\":\"<b>\\t</b>\"}"
    );
    assert_eq!(zen_abi(), 1);
}

#[test]
fn hostile_input_stays_bounded() {
    let deep = "<div>".repeat(5000);
    let mut depth = 0;
    let mut level = outline(&deep);
    while let Some(first) = level.first() {
        depth += 1;
        level = first.children.clone();
    }
    assert_eq!(depth, 32, "the outline stops at 32 levels");
    let unmatched = format!("{}{}", "<div>\n".repeat(30_000), "</span>\n".repeat(30_000));
    assert_eq!(fold(&unmatched).len(), 29_999, "the unclosed elements, with unmatched end tags skipped fast");
    let groups = format!("{}a{}", "(".repeat(1100), ")".repeat(1100));
    assert_eq!(abbr(&groups), None);
    assert_eq!(abbr(&format!("{}a", "div>".repeat(1100))), None);
    assert_eq!(abbr(&format!("li{}*2", "$".repeat(70_000))), None);
    assert_eq!(abbr(&format!("p{{{}}}*1000", "x".repeat(1500))), None, "more than 1 MiB");
    assert_eq!(abbr("li$@18446744073709551615*3").as_deref(), Some("<li1>$0</li1>\n<li2></li2>\n<li3></li3>"));
    assert_eq!(abbr("a[href=\\$0]"), None, "a literal caret mark");
    assert_eq!(labels("<div></div cl|"), Vec::<String>::new(), "no attributes in an end tag");
    let comments = fold("<!-->\n<div>\n</div>");
    assert_eq!(comments, [Fold { start_line: 1, end_line: 2 }], "`<!-->` is an empty comment");
}

#[test]
fn answers_typing_and_colour_requests() {
    let mut input = 5u32.to_le_bytes().to_vec();
    input.extend_from_slice(&u32::from('>').to_le_bytes());
    input.extend_from_slice(b"<div>");
    assert_eq!(String::from_utf8(on_type_request(&input)).unwrap(), r#"{"from":5,"to":5,"text":"$0</div>"}"#);
    assert!(on_type_request(&[1, 0, 0]).is_empty());
    let colors = colors_request(b"<b style=\"color:red\">");
    assert_eq!(colors, [16u32, 19, 0xff0000ff].iter().flat_map(|value| value.to_le_bytes()).collect::<Vec<u8>>());
}
