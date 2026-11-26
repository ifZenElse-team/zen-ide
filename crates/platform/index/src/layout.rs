pub const LAYOUT: [(char, char); 33] = [
    ('й', 'q'),
    ('ц', 'w'),
    ('у', 'e'),
    ('к', 'r'),
    ('е', 't'),
    ('н', 'y'),
    ('г', 'u'),
    ('ш', 'i'),
    ('щ', 'o'),
    ('з', 'p'),
    ('х', '['),
    ('ъ', ']'),
    ('ф', 'a'),
    ('ы', 's'),
    ('в', 'd'),
    ('а', 'f'),
    ('п', 'g'),
    ('р', 'h'),
    ('о', 'j'),
    ('л', 'k'),
    ('д', 'l'),
    ('ж', ';'),
    ('э', '\''),
    ('я', 'z'),
    ('ч', 'x'),
    ('с', 'c'),
    ('м', 'v'),
    ('и', 'b'),
    ('т', 'n'),
    ('ь', 'm'),
    ('б', ','),
    ('ю', '.'),
    ('ё', '`'),
];

pub fn swap(text: &str, from_russian: bool) -> String {
    text.chars()
        .map(|c| {
            let lower = c.to_lowercase().next().unwrap_or(c);
            let found = LAYOUT.iter().find(|(ru, en)| if from_russian { *ru == lower } else { *en == lower });
            match found {
                Some(&(ru, en)) => {
                    let other = if from_russian { en } else { ru };
                    if c.is_uppercase() { other.to_uppercase().next().unwrap_or(other) } else { other }
                }
                None => c,
            }
        })
        .collect()
}

pub fn forms(query: &str) -> Vec<String> {
    let mut forms = vec![query.to_owned()];
    for form in [swap(query, true), swap(query, false)] {
        if !forms.contains(&form) {
            forms.push(form);
        }
    }
    forms
}

pub fn other_forms(query: &str) -> Vec<String> {
    forms(query).into_iter().skip(1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_other_layout_is_read_both_ways() {
        assert_eq!(forms("ишд"), ["ишд", "bil"]);
        assert_eq!(forms("bil"), ["bil", "ишд"]);
        assert_eq!(forms("ьфшт"), ["ьфшт", "main"]);
        assert_eq!(forms("Ифк"), ["Ифк", "Bar"], "case is kept");
        assert_eq!(forms("zen-ide"), ["zen-ide", "яут-шву"]);
        assert_eq!(forms("ьф,"), ["ьф,", "ma,", "ьфб"], "a key's punctuation is read too");
        assert_eq!(swap(&swap("йцукенгшщзхъфывапролджэячсмитьбюё", true), false), "йцукенгшщзхъфывапролджэячсмитьбюё");
        assert_eq!(other_forms("main"), ["ьфшт"]);
        assert!(other_forms("123 -").is_empty(), "nothing to read otherwise");
    }
}
