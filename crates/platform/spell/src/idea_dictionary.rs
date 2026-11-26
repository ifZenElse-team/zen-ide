use std::ops::Range;

use roxmltree::{Document, Node};

pub const COMPONENT: &str = "ProjectDictionaryState";

pub fn words(xml: &str) -> Result<Vec<String>, String> {
    let document = Document::parse(xml).map_err(|error| error.to_string())?;
    Ok(document
        .descendants()
        .filter(|node| node.has_tag_name("dictionary"))
        .flat_map(|dictionary| dictionary.children().filter(|node| node.has_tag_name("words")))
        .flat_map(|words| words.children().filter(|node| node.has_tag_name("w")))
        .filter_map(|word| word.text().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned))
        .collect())
}

pub fn new_file(name: &str, words: &[String]) -> String {
    let mut xml = format!("<component name=\"{COMPONENT}\">\n  <dictionary name=\"{}\">\n", escape(name));
    if words.is_empty() {
        xml.push_str("  </dictionary>\n</component>\n");
        return xml;
    }
    xml.push_str(&words_element(words, "    ", "      "));
    xml.push_str("\n  </dictionary>\n</component>\n");
    xml
}

pub fn edited(xml: &str, add: &[&str], remove: &[&str]) -> Result<Option<String>, String> {
    let document = Document::parse(xml).map_err(|error| error.to_string())?;
    let dictionaries: Vec<Node<'_, '_>> =
        document.descendants().filter(|node| node.has_tag_name("dictionary")).collect();
    if dictionaries.is_empty() {
        return Err("the file has no <dictionary> element".to_owned());
    }
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for (index, dictionary) in dictionaries.iter().enumerate() {
        let element = dictionary.children().find(|node| node.has_tag_name("words"));
        let before: Vec<String> = element
            .into_iter()
            .flat_map(|words| words.children().filter(|node| node.has_tag_name("w")))
            .filter_map(|word| word.text().map(|text| text.trim().to_owned()))
            .collect();
        let mut after: Vec<String> =
            before.iter().filter(|word| !remove.iter().any(|gone| gone.eq_ignore_ascii_case(word))).cloned().collect();
        if index == 0 {
            for word in add {
                if word.is_empty() || after.iter().any(|kept| kept.eq_ignore_ascii_case(word)) {
                    continue;
                }
                let lower = word.to_ascii_lowercase();
                let at = after.iter().position(|kept| kept.to_ascii_lowercase() > lower).unwrap_or(after.len());
                after.insert(at, (*word).to_owned());
            }
        }
        if after == before {
            continue;
        }
        edits.push(match element {
            Some(words) => replaced_words(xml, words, &after),
            None => added_words(xml, *dictionary, &after),
        });
    }
    if edits.is_empty() {
        return Ok(None);
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut text = xml.to_owned();
    for (range, replacement) in edits {
        text.replace_range(range, &replacement);
    }
    Ok(Some(text))
}

fn replaced_words(xml: &str, words: Node<'_, '_>, after: &[String]) -> (Range<usize>, String) {
    let range = words.range();
    let indent = indent_of(xml, range.start);
    if after.is_empty() {
        let line_start = indent.map_or(range.start, |indent| range.start - indent.len());
        let end = if xml[range.end..].starts_with('\n') { range.end + 1 } else { range.end };
        return (line_start..end, String::new());
    }
    let indent = indent.unwrap_or_default().to_owned();
    let child = words
        .children()
        .find(|node| node.has_tag_name("w"))
        .and_then(|word| indent_of(xml, word.range().start))
        .map_or_else(|| format!("{indent}  "), str::to_owned);
    let block = words_element(after, &indent, &child);
    (range, block.trim_start().to_owned())
}

fn added_words(xml: &str, dictionary: Node<'_, '_>, after: &[String]) -> (Range<usize>, String) {
    let range = dictionary.range();
    let indent = indent_of(xml, range.start).unwrap_or_default().to_owned();
    let inner = format!("{indent}  ");
    let block = words_element(after, &inner, &format!("{inner}  "));
    let element = &xml[range.clone()];
    if let Some(open) = element.strip_suffix("/>") {
        let replacement = format!("{}>\n{block}\n{indent}</dictionary>", open.trim_end());
        return (range, replacement);
    }
    let close = range.end - element.len() + element.rfind("</").unwrap_or(element.len());
    let line_start = xml[..close].rfind('\n').map_or(0, |at| at + 1);
    if xml[line_start..close].trim().is_empty() {
        (line_start..line_start, format!("{block}\n"))
    } else {
        (close..close, format!("\n{block}\n{indent}"))
    }
}

fn words_element(words: &[String], indent: &str, child: &str) -> String {
    if words.is_empty() {
        return String::new();
    }
    let mut block = format!("{indent}<words>\n");
    for word in words {
        block.push_str(&format!("{child}<w>{}</w>\n", escape(word)));
    }
    block.push_str(&format!("{indent}</words>"));
    block
}

fn indent_of(xml: &str, at: usize) -> Option<&str> {
    let line_start = xml[..at].rfind('\n').map_or(0, |found| found + 1);
    let indent = &xml[line_start..at];
    indent.chars().all(|c| c == ' ' || c == '\t').then_some(indent)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
