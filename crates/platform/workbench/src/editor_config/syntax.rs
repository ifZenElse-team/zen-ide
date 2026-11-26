use zen_project::editorconfig::{FILE_NAME, UNSET, known_names, known_values};
use zen_syntax::{
    Expansion, Feature, HighlightKind, Highlighter, LineFold, OutlineItem, Outliner, Span, Suggestion, Suggestions,
    TextFeatures,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct EditorConfigSyntax;

fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut start = 0;
    text.split_inclusive('\n').map(move |line| {
        let at = start;
        start += line.len();
        (at, line.trim_end_matches(['\n', '\r']))
    })
}

fn is_known(name: &str) -> bool {
    known_values(name).is_some()
}

fn value_kind(name: &str, value: &str) -> HighlightKind {
    if value.parse::<u32>().is_ok() {
        HighlightKind::Number
    } else if value.eq_ignore_ascii_case(UNSET)
        || known_values(name).is_some_and(|values| values.iter().any(|known| known.eq_ignore_ascii_case(value)))
    {
        HighlightKind::Constant
    } else {
        HighlightKind::String
    }
}

fn line_spans(at: usize, line: &str, spans: &mut Vec<Span>) {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let start = at + indent;
    let content = trimmed.trim_end();
    if content.is_empty() {
        return;
    }
    if content.starts_with(['#', ';']) {
        spans.push(Span { range: start..start + content.len(), kind: HighlightKind::Comment });
        return;
    }
    if content.starts_with('[') {
        spans.push(Span { range: start..start + content.len(), kind: HighlightKind::Heading });
        return;
    }
    let Some(equals) = content.find('=') else { return };
    let name = content[..equals].trim_end();
    if !name.is_empty() {
        let kind = if is_known(name) { HighlightKind::Keyword } else { HighlightKind::Attribute };
        spans.push(Span { range: start..start + name.len(), kind });
    }
    spans.push(Span { range: start + equals..start + equals + 1, kind: HighlightKind::Punctuation });
    let after = &content[equals + 1..];
    let value = after.trim_start();
    if !value.is_empty() {
        let from = start + equals + 1 + (after.len() - value.len());
        spans.push(Span { range: from..from + value.len(), kind: value_kind(name, value) });
    }
}

impl Highlighter for EditorConfigSyntax {
    fn handles(&self, file_name: &str) -> bool {
        file_name == FILE_NAME
    }

    fn specificity(&self, _file_name: &str) -> u8 {
        2
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        for (at, line) in lines(text) {
            line_spans(at, line, &mut spans);
        }
        spans
    }
}

impl Outliner for EditorConfigSyntax {
    fn handles(&self, file_name: &str) -> bool {
        file_name == FILE_NAME
    }

    fn outline(&self, _text: &str) -> Vec<OutlineItem> {
        Vec::new()
    }
}

fn suggestion(label: &str, kind: &str, insert: String) -> Suggestion {
    Suggestion { label: label.to_owned(), kind: kind.to_owned(), insert, caret: None, detail: None }
}

fn matching<'a>(candidates: impl Iterator<Item = &'a str>, typed: &str) -> impl Iterator<Item = &'a str> {
    let typed = typed.to_ascii_lowercase();
    candidates.filter(move |candidate| candidate.starts_with(&typed))
}

impl TextFeatures for EditorConfigSyntax {
    fn specificity(&self, _file_name: &str) -> u8 {
        2
    }

    fn supports(&self, feature: Feature) -> bool {
        feature == Feature::Completion
    }

    fn folds(&self, _text: &str) -> Vec<LineFold> {
        Vec::new()
    }

    fn complete(&self, text: &str, caret: usize) -> Option<Suggestions> {
        let before = text.get(..caret)?;
        let line_start = before.rfind(['\n', '\r']).map_or(0, |at| at + 1);
        let line = &before[line_start..];
        let trimmed = line.trim_start();
        if trimmed.starts_with(['#', ';', '[']) {
            return None;
        }
        match trimmed.split_once('=') {
            None => {
                let from = caret - trimmed.len();
                let names = known_names();
                let items: Vec<Suggestion> = matching(names.iter().map(String::as_str), trimmed)
                    .map(|name| suggestion(name, "property", format!("{name} = ")))
                    .collect();
                (!items.is_empty()).then_some(Suggestions { from, items })
            }
            Some((name, value)) => {
                let name = name.trim();
                let typed = value.trim_start();
                let from = caret - typed.len();
                let values = known_values(name)?;
                let unset = (name != "root").then_some(UNSET);
                let items: Vec<Suggestion> = matching(values.iter().copied().chain(unset), typed)
                    .map(|value| suggestion(value, "value", value.to_owned()))
                    .collect();
                (!items.is_empty()).then_some(Suggestions { from, items })
            }
        }
    }

    fn expand(&self, _text: &str, _caret: usize) -> Option<Expansion> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, HighlightKind)> {
        EditorConfigSyntax.highlight(text).into_iter().map(|span| (&text[span.range], span.kind)).collect()
    }

    #[test]
    fn sections_comments_known_keys_and_values_are_coloured() {
        let text = "# top\r\nroot = true\r\n\r\n[*.{go,mod}]\r\nindent_style = tab\r\nindent_size=4\r\nmy_key = whatever\r\n; end\r\n";
        assert_eq!(
            kinds(text),
            [
                ("# top", HighlightKind::Comment),
                ("root", HighlightKind::Keyword),
                ("=", HighlightKind::Punctuation),
                ("true", HighlightKind::Constant),
                ("[*.{go,mod}]", HighlightKind::Heading),
                ("indent_style", HighlightKind::Keyword),
                ("=", HighlightKind::Punctuation),
                ("tab", HighlightKind::Constant),
                ("indent_size", HighlightKind::Keyword),
                ("=", HighlightKind::Punctuation),
                ("4", HighlightKind::Number),
                ("my_key", HighlightKind::Attribute),
                ("=", HighlightKind::Punctuation),
                ("whatever", HighlightKind::String),
                ("; end", HighlightKind::Comment),
            ]
        );
        assert!(Highlighter::handles(&EditorConfigSyntax, ".editorconfig"));
        assert!(!Highlighter::handles(&EditorConfigSyntax, "editorconfig.txt"));
        assert_eq!(kinds("ij_go_visual_guides = none")[0].1, HighlightKind::Keyword);
    }

    fn labels(text: &str) -> Option<(usize, Vec<String>)> {
        let suggestions = EditorConfigSyntax.complete(text, text.len())?;
        Some((suggestions.from, suggestions.items.into_iter().map(|item| item.label).collect()))
    }

    #[test]
    fn keys_complete_before_the_equals_sign_and_values_after_it() {
        let (from, names) = labels("[*]\nind").unwrap();
        assert_eq!((from, names), (4, vec!["indent_size".to_owned(), "indent_style".to_owned()]));
        let item = EditorConfigSyntax.complete("[*]\nend", 7).unwrap().items.remove(0);
        assert_eq!(
            (item.label.as_str(), item.insert.as_str(), item.kind.as_str()),
            ("end_of_line", "end_of_line = ", "property")
        );
        assert_eq!(labels("[*]\nend_of_line = c"), Some((18, vec!["crlf".to_owned(), "cr".to_owned()])));
        assert_eq!(labels("[*]\ncharset = utf-8").unwrap().1, ["utf-8", "utf-8-bom"]);
        assert_eq!(labels("[*]\nindent_style = ").unwrap().1, ["space", "tab", "unset"]);
        assert_eq!(labels("root = ").unwrap().1, ["true", "false"], "root cannot be unset");
        assert!(labels("[*]\nij_go_vis").unwrap().1.contains(&"ij_go_visual_guides".to_owned()));
        assert_eq!(labels("[*]\nij_go_visual_guides = ").unwrap().1, ["none", "unset"]);
        assert_eq!(labels("# ind"), None, "not in a comment");
        assert_eq!(labels("[ind"), None, "not in a section header");
        assert_eq!(labels("[*]\nmy_key = "), None, "an unknown key has no values");
    }
}
