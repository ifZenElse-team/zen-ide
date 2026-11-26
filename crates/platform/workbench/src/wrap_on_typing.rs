use zen_core::Session;
use zen_editor::{Selection, Selections};
use zen_text::Edit;

use crate::{Workbench, project_settings, visual_aids};

fn column_of(chars: &[char], tab_size: usize) -> usize {
    chars.iter().fold(0, |column, c| if *c == '\t' { column + tab_size - column % tab_size } else { column + 1 })
}

pub(crate) fn break_at(line: &str, caret: usize, margin: usize, tab_size: usize) -> Option<std::ops::Range<usize>> {
    let chars: Vec<char> = line.chars().collect();
    let caret = caret.min(chars.len());
    if column_of(&chars[..caret], tab_size) <= margin {
        return None;
    }
    let indent = chars.iter().take_while(|c| c.is_whitespace()).count();
    let end =
        (indent..caret).rev().find(|at| chars[*at].is_whitespace() && column_of(&chars[..*at], tab_size) <= margin)?
            + 1;
    let start = (indent..end).rev().take_while(|at| chars[*at].is_whitespace()).last()?;
    Some(start..end)
}

pub(crate) fn after_typing(session: &mut Session) {
    if !project_settings::wrap_on_typing(session) {
        return;
    }
    let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return };
    let selection = document.selections().primary();
    if document.selections().len() != 1 || !selection.is_empty() {
        return;
    }
    let text = document.snapshot().clone();
    let path = document.path().to_owned();
    let Ok(point) = text.offset_to_point(selection.head) else { return };
    let Ok(line) = text.line(point.line).map(std::borrow::Cow::into_owned) else { return };
    let style = project_settings::code_style_of(session, document);
    let (margin, _) = visual_aids::margins_of(session, &path);
    let Some(gap) = break_at(&line, point.column, usize::from(margin), usize::from(style.tab_size.max(1))) else {
        return;
    };
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let Ok(start) = text.point_to_offset(zen_text::Point { line: point.line, column: 0 }) else { return };
    let inserted = format!("\n{indent}");
    let caret = selection.head - gap.len() + inserted.chars().count();
    let edit = Edit::replace(start + gap.start..start + gap.end, inserted);
    if let Some(document) = session.state_mut::<Workbench>().active_document_mut() {
        document.apply_action(vec![edit], Selections::from_vec(vec![Selection::caret(caret)], 0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_gap_before_the_margin_breaks_the_line() {
        assert_eq!(break_at("one two three", 13, 10, 4), Some(7..8));
        assert_eq!(break_at("one  two three", 14, 7, 4), Some(3..5));
    }

    #[test]
    fn a_line_within_the_margin_or_without_a_gap_stays() {
        assert_eq!(break_at("one two", 7, 10, 4), None);
        assert_eq!(break_at("onetwothreefour", 15, 10, 4), None);
        assert_eq!(break_at("    onetwothreefour", 19, 10, 4), None, "the indent is no gap");
    }

    #[test]
    fn tabs_count_as_wide_as_their_size() {
        assert_eq!(break_at("\tab cd", 6, 6, 4), Some(3..4));
    }
}
