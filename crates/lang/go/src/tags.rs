use std::collections::BTreeMap;

use zen_i18n::t;
use zen_modules::assist::{AssistContext, LanguageAssist};
use zen_syntax::{Suggestion, Suggestions};

use crate::{Token, tokenize};

pub const ALIGN_OPTION: &str = "alignStructTags";

pub const KEYS: &[&str] = &["json", "yaml", "db", "xml", "toml", "bson", "mapstructure", "form", "validate", "gorm"];

pub const GENERATED_KEYS: &[&str] = &["json", "yaml", "db", "xml", "toml", "bson"];

pub const OMIT_EMPTY: &str = "omitempty";

fn options_of(key: &str) -> &'static [&'static str] {
    match key {
        "json" => &["omitempty", "omitzero", "string"],
        "yaml" => &["omitempty", "flow", "inline"],
        "xml" => &["omitempty", "attr", "chardata", "cdata", "innerxml", "comment", "any"],
        "bson" => &["omitempty", "minsize", "truncate", "inline"],
        "toml" | "mapstructure" => &["omitempty"],
        _ => &[],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Snake,
    Camel,
    Kebab,
    Pascal,
}

impl Case {
    pub const ALL: [Case; 4] = [Case::Snake, Case::Camel, Case::Kebab, Case::Pascal];

    pub fn id(self) -> &'static str {
        match self {
            Case::Snake => "structTags:snake",
            Case::Camel => "structTags:camel",
            Case::Kebab => "structTags:kebab",
            Case::Pascal => "structTags:pascal",
        }
    }

    pub fn title(self) -> String {
        t!("go-generate-tags", case = self.of("FieldName"))
    }

    pub fn of(self, field: &str) -> String {
        let words = words(field);
        let capital = |word: &String| {
            let mut chars = word.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        };
        match self {
            Case::Snake => words.join("_"),
            Case::Kebab => words.join("-"),
            Case::Pascal => words.iter().map(capital).collect(),
            Case::Camel => {
                let rest = words.iter().skip(1).map(capital).collect::<String>();
                format!("{}{rest}", words.first().map(String::as_str).unwrap_or_default())
            }
        }
    }
}

fn words(field: &str) -> Vec<String> {
    let chars: Vec<char> = field.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (at, &c) in chars.iter().enumerate() {
        let previous = at.checked_sub(1).map(|before| chars[before]);
        let next = chars.get(at + 1);
        let starts = c.is_uppercase()
            && previous.is_some_and(|before| {
                before.is_lowercase()
                    || before.is_ascii_digit()
                    || (before.is_uppercase() && next.is_some_and(|after| after.is_lowercase()))
            });
        if c == '_' || starts {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            if c == '_' {
                continue;
            }
        }
        word.extend(c.to_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tag {
    start: usize,
    end: usize,
    line: usize,
    group: usize,
}

fn struct_tags(text: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut open: Vec<Option<usize>> = Vec::new();
    let mut groups = 0;
    let mut line = 0;
    let mut previous = "";
    for (token, start, end) in tokenize(text) {
        let word = &text[start..end];
        match token {
            Token::Whitespace | Token::Comment => {
                line += word.matches('\n').count();
                continue;
            }
            Token::Punct('{') => {
                groups += 1;
                open.push((previous == "struct").then_some(groups));
            }
            Token::Punct('}') => {
                open.pop();
            }
            Token::String if word.starts_with('`') => {
                if let Some(Some(group)) = open.last() {
                    tags.push(Tag { start, end, line, group: *group });
                }
            }
            _ => {}
        }
        line += word.matches('\n').count();
        previous = word;
    }
    tags
}

fn pairs(tag: &str) -> Option<Vec<&str>> {
    let mut found = Vec::new();
    let mut rest = tag.trim_start();
    while !rest.is_empty() {
        let colon = rest.find(':')?;
        if colon == 0 || !rest[colon + 1..].starts_with('"') {
            return None;
        }
        let value = colon + 2;
        let close = closing_quote(&rest[value..])?;
        let end = value + close + 1;
        found.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    Some(found)
}

fn closing_quote(value: &str) -> Option<usize> {
    let mut chars = value.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '"' => return Some(at),
            _ => {}
        }
    }
    None
}

pub fn align(text: &str, column: bool) -> Option<String> {
    let tags = struct_tags(text);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut from = 0;
    while from < tags.len() {
        let mut to = from + 1;
        while to < tags.len() && tags[to].group == tags[from].group && tags[to].line == tags[to - 1].line + 1 {
            to += 1;
        }
        align_run(text, &tags[from..to], column, &mut edits);
        from = to;
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut aligned = text.to_owned();
    for (start, end, new) in edits {
        aligned.replace_range(start..end, &new);
    }
    (aligned != text).then_some(aligned)
}

fn align_run(text: &str, run: &[Tag], column: bool, edits: &mut Vec<(usize, usize, String)>) {
    let parsed: Vec<Option<Vec<&str>>> = run.iter().map(|tag| pairs(&text[tag.start + 1..tag.end - 1])).collect();
    let key = |pair: &str| pair.split(':').next().unwrap_or_default().to_owned();
    let mut widths: Vec<usize> = Vec::new();
    let longest = parsed.iter().flatten().map(Vec::len).max().unwrap_or(0);
    for index in 0..longest {
        let column: Vec<&str> = parsed.iter().flatten().filter_map(|found| found.get(index).copied()).collect();
        if column.iter().any(|pair| key(pair) != key(column[0])) {
            break;
        }
        widths.push(column.iter().map(|pair| pair.chars().count()).max().unwrap_or(0));
    }
    for (tag, found) in run.iter().zip(&parsed) {
        let Some(found) = found else { continue };
        let mut new = String::new();
        for (index, pair) in found.iter().enumerate() {
            new.push_str(pair);
            if index + 1 < found.len() {
                let width = widths.get(index).copied().unwrap_or_else(|| pair.chars().count());
                new.push_str(&" ".repeat(width - pair.chars().count() + 1));
            }
        }
        if new != text[tag.start + 1..tag.end - 1] {
            edits.push((tag.start + 1, tag.end - 1, new));
        }
    }
    if column {
        let before = |tag: &Tag| {
            let line = text[..tag.start].rfind('\n').map_or(0, |at| at + 1);
            text[line..tag.start].trim_end().chars().count()
        };
        let widest = run.iter().map(before).max().unwrap_or(0);
        for tag in run {
            let code = text[..tag.start].trim_end().len();
            let new = " ".repeat(widest - before(tag) + 1);
            if text[code..tag.start] != new {
                edits.push((code, tag.start, new));
            }
        }
    }
}

pub fn with_keys(tag: &str, keys: &[String], field: &str, case: Case, omit_empty: bool) -> Option<String> {
    let mut found: Vec<String> = pairs(tag)?.into_iter().map(str::to_owned).collect();
    for key in keys {
        if !found.iter().any(|pair| pair.split(':').next() == Some(key)) {
            let omit = if omit_empty && options_of(key).contains(&OMIT_EMPTY) { ",omitempty" } else { "" };
            found.push(format!("{key}:\"{}{omit}\"", case.of(field)));
        }
    }
    Some(found.join(" "))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GoTagAssist;

struct Place<'a> {
    content: &'a str,
    field: &'a str,
}

fn place(text: &str, offset: usize) -> Option<Place<'_>> {
    let open = struct_tags(text).into_iter().find(|tag| tag.start < offset && offset <= tag.end)?;
    if offset == open.end && text[open.start + 1..open.end].ends_with('`') {
        return None;
    }
    let content = text.get(open.start + 1..offset)?;
    if content.contains('\n') {
        return None;
    }
    let line = text[..open.start].rfind('\n').map_or(0, |at| at + 1);
    let first = text[line..open.start].split_whitespace().next()?;
    let field = first.trim_end_matches(',').rsplit('.').next()?.trim_start_matches('*');
    Some(Place { content, field })
}

enum Position<'a> {
    Key { typed: &'a str, present: Vec<&'a str> },
    Value { key: &'a str, typed: &'a str },
}

fn position(content: &str) -> Option<Position<'_>> {
    let mut present = Vec::new();
    let mut rest = content.trim_start();
    loop {
        let Some(colon) = rest.find(':') else {
            return (!rest.contains(char::is_whitespace)).then_some(Position::Key { typed: rest, present });
        };
        let key = &rest[..colon];
        let value = rest[colon + 1..].strip_prefix('"')?;
        let Some(close) = closing_quote(value) else { return Some(Position::Value { key, typed: value }) };
        present.push(key);
        rest = value[close + 1..].trim_start();
    }
}

fn item(label: &str, kind: &str, insert: String, caret: Option<usize>) -> Suggestion {
    Suggestion { label: label.to_owned(), kind: kind.to_owned(), insert, caret, detail: None }
}

impl LanguageAssist for GoTagAssist {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn complete(&self, context: &AssistContext<'_>) -> Option<Suggestions> {
        let found = place(context.text, context.offset)?;
        let after = &context.text[context.offset..];
        match position(found.content)? {
            Position::Key { typed, present } => {
                let items: Vec<Suggestion> = KEYS
                    .iter()
                    .filter(|key| !present.contains(key))
                    .map(|key| {
                        if after.starts_with(':') {
                            item(key, "property", (*key).to_owned(), None)
                        } else {
                            item(key, "property", format!("{key}:\"\""), Some(key.len() + 2))
                        }
                    })
                    .collect();
                Some(Suggestions { from: context.offset - typed.len(), items })
            }
            Position::Value { key, typed } => {
                let (from, items) = match typed.rfind(',') {
                    Some(comma) => {
                        let items =
                            options_of(key).iter().map(|option| item(option, "value", (*option).to_owned(), None));
                        (context.offset - (typed.len() - comma - 1), items.collect::<Vec<_>>())
                    }
                    None => {
                        let names: BTreeMap<String, Case> =
                            Case::ALL.iter().rev().map(|case| (case.of(found.field), *case)).collect();
                        let mut names: Vec<(String, Case)> = names.into_iter().collect();
                        names.sort_by_key(|(_, case)| Case::ALL.iter().position(|known| known == case));
                        let mut items: Vec<Suggestion> =
                            names.iter().map(|(name, _)| item(name, "value", name.clone(), None)).collect();
                        items.push(item("-", "value", "-".to_owned(), None));
                        (context.offset - typed.len(), items)
                    }
                };
                (!items.is_empty()).then_some(Suggestions { from, items })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn suggested(text: &str) -> Option<(String, Vec<String>)> {
        let offset = text.find('|').unwrap();
        let source = text.replace('|', "");
        let context = AssistContext { path: Path::new("a.go"), text: &source, offset };
        GoTagAssist.complete(&context).map(|found| {
            (source[found.from..offset].to_owned(), found.items.into_iter().map(|item| item.label).collect())
        })
    }

    const HEAD: &str = "package a\n\ntype User struct {\n";

    #[test]
    fn keys_complete_inside_the_backticks_of_a_field() {
        let (typed, keys) = suggested(&format!("{HEAD}\tName string `js|`\n}}\n")).unwrap();
        assert_eq!(typed, "js");
        assert_eq!(&keys[..3], ["json", "yaml", "db"]);
    }

    #[test]
    fn a_key_is_inserted_with_its_quotes_and_the_caret_between_them() {
        let text = format!("{HEAD}\tName string `|`\n}}\n");
        let offset = text.find('|').unwrap();
        let source = text.replace('|', "");
        let context = AssistContext { path: Path::new("a.go"), text: &source, offset };
        let found = GoTagAssist.complete(&context).unwrap();
        assert_eq!((found.items[0].insert.as_str(), found.items[0].caret), ("json:\"\"", Some(6)));
    }

    #[test]
    fn a_key_already_in_the_tag_is_not_offered_again() {
        let (_, keys) = suggested(&format!("{HEAD}\tName string `json:\"name\" |`\n}}\n")).unwrap();
        assert!(!keys.contains(&"json".to_owned()), "{keys:?}");
        assert!(keys.contains(&"yaml".to_owned()));
    }

    #[test]
    fn a_value_offers_the_field_name_in_each_case_and_after_a_comma_the_options_of_the_key() {
        let (typed, names) = suggested(&format!("{HEAD}\tUserID string `json:\"us|\"`\n}}\n")).unwrap();
        assert_eq!(typed, "us");
        assert_eq!(names, ["user_id", "userId", "user-id", "UserId", "-"]);
        let (typed, options) = suggested(&format!("{HEAD}\tName string `json:\"name,om|\"`\n}}\n")).unwrap();
        assert_eq!(typed, "om");
        assert_eq!(options, ["omitempty", "omitzero", "string"]);
        assert_eq!(suggested(&format!("{HEAD}\tName string `db:\"name,|\"`\n}}\n")), None);
    }

    #[test]
    fn nothing_is_completed_outside_a_struct_tag() {
        for text in [
            "package a\n\nvar s = `js|`\n".to_owned(),
            format!("{HEAD}\tName string `json:\"a\"` |\n}}\n"),
            format!("{HEAD}\tName string `json:\"a\"`|\n}}\n"),
            format!("{HEAD}\tName string // `js|`\n}}\n"),
            "package a\n\nfunc f() {\n\tx := T{Name: `js|`}\n}\n".to_owned(),
        ] {
            assert_eq!(suggested(&text), None, "{text}");
        }
    }

    #[test]
    fn a_tag_in_a_nested_struct_completes_and_a_closing_backtick_is_not_needed() {
        assert!(suggested(&format!("{HEAD}\tInner struct {{\n\t\tA int `js|`\n\t}}\n}}\n")).is_some());
        assert!(suggested(&format!("{HEAD}\tName string `js|")).is_some());
    }

    #[test]
    fn field_names_convert_to_each_case() {
        let converted = |field: &str| Case::ALL.map(|case| case.of(field));
        assert_eq!(converted("UserID"), ["user_id", "userId", "user-id", "UserId"]);
        assert_eq!(converted("HTTPServer"), ["http_server", "httpServer", "http-server", "HttpServer"]);
        assert_eq!(converted("Name"), ["name", "name", "name", "Name"]);
        assert_eq!(converted("Sha256Sum"), ["sha256_sum", "sha256Sum", "sha256-sum", "Sha256Sum"]);
    }

    #[test]
    fn the_values_of_tags_line_up_in_a_run_of_fields() {
        let text = format!(
            "{HEAD}\tID   int    `json:\"id\" yaml:\"id\" db:\"id\"`\n\tName string `json:\"name\" yaml:\"name\"`\n\n\tAge int `json:\"age\" yaml:\"age\"`\n}}\n"
        );
        let aligned = align(&text, false).unwrap();
        assert!(aligned.contains("`json:\"id\"   yaml:\"id\"   db:\"id\"`\n"), "{aligned}");
        assert!(aligned.contains("`json:\"name\" yaml:\"name\"`\n"), "{aligned}");
        assert!(aligned.contains("`json:\"age\" yaml:\"age\"`\n"), "a blank line ends the run: {aligned}");
        assert_eq!(align(&aligned, false), None, "aligning twice changes nothing");
    }

    #[test]
    fn a_tag_it_cannot_read_and_a_string_outside_a_struct_are_left_alone() {
        let text =
            format!("{HEAD}\tA int `not a tag`\n\tB int `json:\"b\"  yaml:\"b\"`\n}}\n\nvar s = `a:\"b\"  c:\"d\"`\n");
        let aligned = align(&text, false).unwrap();
        assert!(aligned.contains("`not a tag`"));
        assert!(aligned.contains("`json:\"b\" yaml:\"b\"`"));
        assert!(aligned.contains("`a:\"b\"  c:\"d\"`"));
    }

    #[test]
    fn the_column_of_the_tags_lines_up_when_asked() {
        let text = format!("{HEAD}\tID int `json:\"id\"`\n\tName string `json:\"name\"`\n}}\n");
        let aligned = align(&text, true).unwrap();
        assert!(aligned.contains("\tID int      `json:\"id\"`\n\tName string `json:\"name\"`\n"), "{aligned}");
    }

    #[test]
    fn keys_are_added_to_a_tag_and_kept_when_they_are_there() {
        let keys = ["json".to_owned(), "yaml".to_owned()];
        let tag = with_keys("json:\"custom\"", &keys, "UserID", Case::Snake, true).unwrap();
        assert_eq!(tag, "json:\"custom\" yaml:\"user_id,omitempty\"");
        assert_eq!(with_keys("not a tag", &keys, "A", Case::Snake, false), None);
    }
}
