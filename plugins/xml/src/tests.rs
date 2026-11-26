use super::*;

const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Protocol</key>
	<string>s3</string>
	<!-- a comment
	     over two lines -->
	<key>Vendor</key>
	<string>Timeweb &amp; Cloud</string>
</dict>
</plist>
"#;

fn kinds(text: &str) -> Vec<(&str, Kind)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

#[test]
fn highlights_every_part_of_xml() {
    let text = "<?xml version=\"1.0\"?>\n<!DOCTYPE note [<!ENTITY a \"b\">]>\n<xsl:template match=\"/\" xmlns:xsl=\"x\">\n<![CDATA[<raw>]]><!-- c -->&amp;&#169;</xsl:template>";
    let found = kinds(text);
    for expected in [
        ("<?xml version=\"1.0\"?>", Kind::Macro),
        ("<!DOCTYPE note [<!ENTITY a \"b\">]>", Kind::Macro),
        ("xsl", Kind::Type),
        ("template", Kind::Keyword),
        ("match", Kind::Attribute),
        ("\"/\"", Kind::String),
        ("xmlns", Kind::Type),
        ("<![CDATA[<raw>]]>", Kind::Macro),
        ("<!-- c -->", Kind::Comment),
        ("&amp;", Kind::Constant),
        ("&#169;", Kind::Constant),
        ("</", Kind::Punctuation),
    ] {
        assert!(found.contains(&expected), "{expected:?} in {found:?}");
    }
    assert!(!found.iter().any(|(text, _)| *text == "raw"), "CDATA content is not markup");
    let mut last = 0;
    for span in highlight(text) {
        assert!(span.start >= last && span.end > span.start, "ordered, not overlapping");
        last = span.end;
    }
}

#[test]
fn script_and_title_are_markup_in_xml() {
    let found = kinds("<svg><title>A &amp; <b>B</b></title><script>x</script></svg>");
    assert!(found.contains(&("b", Kind::Keyword)), "{found:?}");
    assert!(found.contains(&("&amp;", Kind::Constant)));
}

#[test]
fn folds_elements_comments_and_the_doctype_that_span_lines() {
    let folds: Vec<(u32, u32)> = fold(PLIST).into_iter().map(|fold| (fold.start_line, fold.end_line)).collect();
    assert_eq!(folds, [(2, 11), (3, 10), (6, 7)]);
    assert!(fold("<a><b/></a>").is_empty());
}

#[test]
fn the_structure_lists_the_elements_with_their_ids() {
    let symbols = outline("<root><item id=\"one\"/><item xml:id='two'><leaf/></item></root>");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "root");
    let names: Vec<&str> = symbols[0].children.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(names, ["item#one", "item#two"]);
    assert_eq!(symbols[0].children[1].children[0].name, "leaf");
    let json = outline_json(&outline("<a id='q\"'/>"));
    assert_eq!(json, r#"[{"name":"a#q\"","kind":"element","start":0,"end":12,"children":[]}]"#);
    let deep = "<a>".repeat(100);
    let mut depth = 0;
    let mut level = outline(&deep);
    while let Some(first) = level.first() {
        depth += 1;
        level = first.children.clone();
    }
    assert_eq!(depth, 32);
}

fn typed(marked: &str, c: char) -> Option<String> {
    let caret = marked.find('|').unwrap();
    let text = marked.replacen('|', "", 1);
    let added = on_type(&text, caret, c)?;
    assert_eq!(added.at, caret);
    Some(added.text)
}

#[test]
fn typing_closes_tags_case_sensitively_and_without_void_elements() {
    assert_eq!(typed("<dict>|", '>').as_deref(), Some("$0</dict>"));
    assert_eq!(typed("<br>|", '>').as_deref(), Some("$0</br>"), "no void elements in XML");
    assert_eq!(typed("<ns:Item a=\"x>y\">|", '>').as_deref(), Some("$0</ns:Item>"));
    assert_eq!(typed("<a>|</a>", '>'), None, "closed already");
    assert_eq!(typed("<dict>\n<key>|</dict>", '>').as_deref(), Some("$0</key>"), "inside a parent");
    assert_eq!(typed("<a><a>|</a>", '>').as_deref(), Some("$0</a>"));
    assert_eq!(typed("<A>|</a>", '>').as_deref(), Some("$0</A>"), "names are case-sensitive");
    assert_eq!(typed("<a/>|", '>'), None);
    assert_eq!(typed("<?xml version=\"1.0\"?>|", '>'), None);
    assert_eq!(typed("<plist><dict></|", '/').as_deref(), Some("dict>"));
    assert_eq!(typed("<plist><dict></dict></|", '/').as_deref(), Some("plist>"));
    assert_eq!(typed("<a></|b", '/'), None);
    assert_eq!(typed("</|", '/'), None);
}

#[test]
fn tags_that_do_not_match_are_problems() {
    assert!(problems(PLIST).is_empty(), "{:?}", problems(PLIST));
    let text = "<a><b></a></c><d>";
    let listed = problems(text);
    let found: Vec<(&str, &str)> =
        listed.iter().map(|problem| (&text[problem.range.clone()], problem.message.as_str())).collect();
    assert_eq!(
        found,
        [
            ("b", "Element <b> is not closed before </a>"),
            ("</c>", "End tag </c> has no start tag"),
            ("d", "Element <d> is not closed"),
        ]
    );
    let text = "<a></A>";
    let messages: Vec<String> = problems(text).into_iter().map(|problem| problem.message).collect();
    assert_eq!(messages, ["Element <a> is not closed", "End tag </A> does not match the open element <a>"]);
    assert_eq!(problems("<a><!-- open")[0].message, "Element <a> is not closed");
    assert!(problems("<a><!-- open").iter().any(|problem| problem.message == "Comment is not closed"));
    assert!(problems("<a><![CDATA[ open").iter().any(|problem| problem.message == "CDATA section is not closed"));
    assert!(problems("<a").iter().any(|problem| problem.message == "Start tag <a> is not finished"));
    let json = problems_json(&problems("<a>"));
    assert_eq!(json, r#"[{"start":1,"end":2,"severity":"error","message":"Element <a> is not closed"}]"#);
}

#[test]
fn hostile_input_stays_bounded() {
    let many = "<a>".repeat(50_000);
    assert_eq!(problems(&many).len(), 50_000);
    assert_eq!(outline(&many).len(), 1);
    let closes = "</a>".repeat(10_000);
    assert_eq!(problems(&closes).len(), 10_000);
    for text in ["<", "</", "<!", "<?", "<!DOCTYPE [", "<a b=\"", "<a b=", "&#", "é<é>"] {
        highlight(text);
        fold(text);
        outline(text);
        problems(text);
        for (at, _) in text.char_indices() {
            on_type(text, at, '>');
            on_type(text, at, '/');
        }
    }
}

#[test]
fn the_manifest_declares_the_xml_type_by_extension_and_content() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    let extensions: Vec<&str> = manifest["extensions"].as_array().unwrap().iter().filter_map(|e| e.as_str()).collect();
    for extension in ["xml", "plist", "svg", "csproj", "xsd", "iml"] {
        assert!(extensions.contains(&extension), "{extension}");
    }
    assert!(!extensions.contains(&"xhtml"), "XHTML stays with HTML");
    assert_eq!(manifest["contents"][0]["starts"], "<?xml");
    assert_eq!(manifest["contents"][1]["unless"], serde_json::json!(["<!DOCTYPE html"]));
    assert_eq!(manifest["contents"][2]["firstTag"], "xmlns");
}
