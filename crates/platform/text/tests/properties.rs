use proptest::prelude::*;
use zen_text::{Buffer, Edit, EditKind, Text};

fn text_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop::sample::select(vec!["a", "b", " ", "\n", "\r\n", "\r", "é", "😀", "\u{2028}"]),
        0..40,
    )
    .prop_map(|parts| parts.concat())
}

fn edit_strategy() -> impl Strategy<Value = (f64, f64, String, bool)> {
    (0.0..=1.0f64, 0.0..=1.0f64, text_strategy(), any::<bool>())
}

fn concrete(text: &Text, (a, b, insert, typing): &(f64, f64, String, bool)) -> (Edit, EditKind) {
    let len = text.len_chars() as f64;
    let (mut start, mut end) = ((a * len) as usize, (b * len) as usize);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    if *typing {
        let ch: String = insert.chars().take(1).collect();
        (Edit::insert(start, ch), EditKind::Typing)
    } else {
        (Edit::replace(start..end, insert.clone()), EditKind::Other)
    }
}

proptest! {
    #[test]
    fn inverse_edits_restore_the_original(original in text_strategy(), specs in proptest::collection::vec(edit_strategy(), 0..12)) {
        let original = Text::from(original.as_str());
        let mut current = original.clone();
        let mut edits = Vec::new();
        for spec in &specs {
            let (edit, _) = concrete(&current, spec);
            current = current.apply(std::slice::from_ref(&edit)).unwrap().0;
            edits.push(edit);
        }
        let (changed, inverse) = original.apply(&edits).unwrap();
        prop_assert_eq!(&changed, &current);
        prop_assert_eq!(changed.apply(&inverse).unwrap().0, original);
    }

    #[test]
    fn undo_all_then_redo_all(original in text_strategy(), specs in proptest::collection::vec(edit_strategy(), 0..12), closes in proptest::collection::vec(any::<bool>(), 12)) {
        let mut buffer = Buffer::new(original.as_str());
        for (spec, close) in specs.iter().zip(closes) {
            let (edit, kind) = concrete(buffer.text(), spec);
            buffer.edit(vec![edit], kind).unwrap();
            if close {
                buffer.close_group();
            }
        }
        let last = buffer.text().clone();

        while buffer.can_undo() {
            let newer = buffer.text().clone();
            let applied = buffer.undo().unwrap();
            prop_assert_eq!(&newer.apply(&applied).unwrap().0, buffer.text());
        }
        prop_assert_eq!(buffer.text(), &Text::from(original.as_str()));

        while buffer.can_redo() {
            let older = buffer.text().clone();
            let applied = buffer.redo().unwrap();
            prop_assert_eq!(&older.apply(&applied).unwrap().0, buffer.text());
        }
        prop_assert_eq!(buffer.text(), &last);
    }

    #[test]
    fn utf16_columns_round_trip(original in text_strategy()) {
        let text = Text::from(original.as_str());
        for offset in 0..=text.len_chars() {
            let point = text.offset_to_point(offset).unwrap();
            let (line, column) = text.offset_to_utf16(offset).unwrap();
            prop_assert_eq!(line, point.line);
            if let Ok(back) = text.utf16_to_offset(line, column)
                && text.point_to_offset(point) == Ok(offset)
            {
                prop_assert_eq!(back, offset);
            }
        }
    }
}
