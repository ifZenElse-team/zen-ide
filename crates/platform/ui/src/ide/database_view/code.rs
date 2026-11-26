use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

use super::super::IdePalette;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lang {
    Plain,
    Sql,
    Json,
    Xml,
}

fn push(job: &mut LayoutJob, text: &str, font: &FontId, color: Color32) {
    if !text.is_empty() {
        job.append(text, 0.0, TextFormat { font_id: font.clone(), color, ..TextFormat::default() });
    }
}

fn sql_line(job: &mut LayoutJob, line: &str, p: &IdePalette, font: &FontId) {
    let mut rest = line;
    while !rest.is_empty() {
        if rest.starts_with("--") {
            push(job, rest, font, p.comment);
            return;
        }
        let first = rest.chars().next().unwrap_or(' ');
        let (piece, color) = if first == '\'' {
            let end = rest[1..].find('\'').map_or(rest.len(), |at| at + 2);
            (&rest[..end], p.string)
        } else if first.is_ascii_digit() {
            let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
            (&rest[..end], p.number)
        } else if first.is_alphabetic() || first == '_' {
            let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
            let word = &rest[..end];
            let keyword = word.len() > 1 && word.chars().all(|c| c.is_ascii_uppercase() || c == '_');
            (word, if keyword { p.keyword } else { p.code })
        } else {
            let end = rest.char_indices().nth(1).map_or(rest.len(), |(at, _)| at);
            (&rest[..end], p.code)
        };
        push(job, piece, font, color);
        rest = &rest[piece.len()..];
    }
}

fn json_line(job: &mut LayoutJob, line: &str, p: &IdePalette, font: &FontId) {
    let mut rest = line;
    while !rest.is_empty() {
        let first = rest.chars().next().unwrap_or(' ');
        let (piece, color) = if first == '"' {
            let mut end = rest.len();
            let mut escaped = false;
            for (at, c) in rest.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    end = at + 1;
                    break;
                }
            }
            let key = rest[end..].trim_start().starts_with(':');
            (&rest[..end], if key { p.constant } else { p.string })
        } else if first == '-' || first.is_ascii_digit() {
            let end = rest[1..]
                .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')))
                .map_or(rest.len(), |at| at + 1);
            (&rest[..end], p.number)
        } else if first.is_ascii_alphabetic() {
            let end = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
            (&rest[..end], p.keyword)
        } else {
            let end = rest.char_indices().nth(1).map_or(rest.len(), |(at, _)| at);
            (&rest[..end], if first == '…' { p.fold_text } else { p.code })
        };
        push(job, piece, font, color);
        rest = &rest[piece.len()..];
    }
}

fn xml_line(job: &mut LayoutJob, line: &str, p: &IdePalette, font: &FontId) {
    let mut rest = line;
    while !rest.is_empty() {
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(rest.len(), |at| at + 3);
            push(job, &rest[..end], font, p.comment);
            rest = &rest[end..];
            continue;
        }
        if rest.starts_with('<') {
            let end = rest.find('>').map_or(rest.len(), |at| at + 1);
            let tag = &rest[..end];
            let name_end = tag
                .char_indices()
                .skip(1)
                .find(|(_, c)| c.is_whitespace() || *c == '>' || (*c == '/' && tag.len() > 2))
                .map_or(tag.len(), |(at, _)| at);
            let name_end = if tag.starts_with("</") { tag.len().min(name_end.max(2)) } else { name_end };
            push(job, &tag[..name_end], font, p.keyword);
            let mut attributes = &tag[name_end..];
            while !attributes.is_empty() {
                if let Some(quote) = attributes.find('"') {
                    let close = attributes[quote + 1..].find('"').map_or(attributes.len(), |at| quote + at + 2);
                    push(job, &attributes[..quote], font, p.attribute);
                    push(job, &attributes[quote..close], font, p.string);
                    attributes = &attributes[close..];
                } else {
                    push(job, attributes, font, p.keyword);
                    attributes = "";
                }
            }
            rest = &rest[end..];
            continue;
        }
        let end = rest.find('<').unwrap_or(rest.len());
        push(job, &rest[..end], font, p.code);
        rest = &rest[end..];
    }
}

pub(super) fn job(text: &str, lang: Lang, p: &IdePalette, font: &FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            push(&mut job, "\n", font, p.code);
        }
        match lang {
            Lang::Plain => push(&mut job, line, font, p.code),
            Lang::Sql => sql_line(&mut job, line, p, font),
            Lang::Json => json_line(&mut job, line, p, font),
            Lang::Xml => xml_line(&mut job, line, p, font),
        }
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(text: &str, lang: Lang, p: &IdePalette) -> Vec<(String, Color32)> {
        let job = job(text, lang, p, &FontId::monospace(12.0));
        job.sections
            .iter()
            .map(|section| {
                let range = section.byte_range.start.0..section.byte_range.end.0;
                (job.text[range].to_owned(), section.format.color)
            })
            .collect()
    }

    #[test]
    fn json_keys_strings_numbers_and_literals_take_the_themes_colours() {
        let p = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let shown = pieces("  \"id\": -12.5e3, \"ok\": true, \"s\": \"a\\\"b\"", Lang::Json, &p);
        assert!(shown.contains(&("\"id\"".to_owned(), p.constant)), "{shown:?}");
        assert!(shown.contains(&("-12.5e3".to_owned(), p.number)), "{shown:?}");
        assert!(shown.contains(&("true".to_owned(), p.keyword)), "{shown:?}");
        assert!(shown.contains(&("\"a\\\"b\"".to_owned(), p.string)), "{shown:?}");
    }

    #[test]
    fn sql_keywords_strings_and_comments_and_xml_tags() {
        let p = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light());
        let shown = pieces("COMMENT ON TABLE public.orders IS 'it''s' -- note", Lang::Sql, &p);
        assert!(shown.contains(&("COMMENT".to_owned(), p.keyword)), "{shown:?}");
        assert!(shown.iter().any(|(text, color)| text.contains("orders") && *color == p.code), "{shown:?}");
        assert!(shown.contains(&("-- note".to_owned(), p.comment)), "{shown:?}");
        let shown = pieces("<a x=\"1\">text</a>", Lang::Xml, &p);
        assert!(shown.contains(&("<a".to_owned(), p.keyword)), "{shown:?}");
        assert!(shown.contains(&("\"1\"".to_owned(), p.string)), "{shown:?}");
        assert!(shown.contains(&("text".to_owned(), p.code)), "{shown:?}");
    }
}
