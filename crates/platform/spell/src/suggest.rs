use std::collections::HashMap;

use crate::Dictionary;

const FARTHEST: usize = 2;

const LONGEST: usize = 48;

const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

pub fn suggestions(dictionaries: &[&Dictionary], word: &str, limit: usize) -> Vec<String> {
    let lower = word.to_ascii_lowercase();
    if lower.is_empty()
        || lower.len() > LONGEST
        || !lower.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'\'')
    {
        return Vec::new();
    }
    let knows = |candidate: &str| dictionaries.iter().any(|dictionary| dictionary.knows(candidate));
    let mut found: HashMap<String, usize> = HashMap::new();
    for candidate in one_edit(&lower) {
        if candidate.len() > 1 && candidate != lower && !found.contains_key(&candidate) && knows(&candidate) {
            found.insert(candidate, 1);
        }
    }
    for dictionary in dictionaries {
        for listed in dictionary.iter() {
            if listed.len().abs_diff(lower.len()) > FARTHEST || listed == lower {
                continue;
            }
            if let Some(distance) = distance(lower.as_bytes(), listed.as_bytes(), FARTHEST) {
                let kept = found.entry(listed.to_owned()).or_insert(distance);
                *kept = (*kept).min(distance);
            }
        }
    }
    let first = lower.as_bytes()[0];
    let mut ranked: Vec<(String, usize)> = found.into_iter().collect();
    ranked.sort_by(|(a, a_distance), (b, b_distance)| {
        let key = |candidate: &str, distance: usize| {
            (
                distance,
                !natural(&lower, candidate),
                candidate.as_bytes()[0] != first,
                candidate.len().abs_diff(lower.len()),
                candidate.contains('\''),
            )
        };
        key(a, *a_distance).cmp(&key(b, *b_distance)).then_with(|| a.cmp(b))
    });
    ranked.into_iter().take(limit).map(|(candidate, _)| in_case_of(word, &candidate)).collect()
}

fn natural(word: &str, candidate: &str) -> bool {
    let sorted = |text: &str| {
        let mut letters = text.as_bytes().to_vec();
        letters.sort_unstable();
        letters
    };
    let undoubled = |longer: &str, shorter: &str| {
        let bytes = longer.as_bytes();
        longer.len() == shorter.len() + 1
            && (1..bytes.len()).any(|at| {
                bytes[at] == bytes[at - 1] && longer[..at] == shorter[..at] && longer[at + 1..] == shorter[at..]
            })
    };
    sorted(word) == sorted(candidate) || undoubled(word, candidate) || undoubled(candidate, word)
}

fn one_edit(word: &str) -> Vec<String> {
    let bytes = word.as_bytes();
    let mut edits = Vec::with_capacity(bytes.len() * 54 + 26);
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap_or_default();
    for at in 0..bytes.len() {
        let mut removed = bytes.to_vec();
        removed.remove(at);
        edits.push(text(removed));
        if at + 1 < bytes.len() && bytes[at] != bytes[at + 1] {
            let mut swapped = bytes.to_vec();
            swapped.swap(at, at + 1);
            edits.push(text(swapped));
        }
        for letter in LETTERS {
            if *letter != bytes[at] {
                let mut replaced = bytes.to_vec();
                replaced[at] = *letter;
                edits.push(text(replaced));
            }
        }
    }
    for at in 0..=bytes.len() {
        for letter in LETTERS {
            let mut inserted = bytes.to_vec();
            inserted.insert(at, *letter);
            edits.push(text(inserted));
        }
    }
    edits
}

fn distance(a: &[u8], b: &[u8], most: usize) -> Option<usize> {
    if a.len() > LONGEST || b.len() > LONGEST {
        return None;
    }
    let width = b.len() + 1;
    let mut before = [0usize; LONGEST + 1];
    let mut previous = [0usize; LONGEST + 1];
    let mut current = [0usize; LONGEST + 1];
    for (column, cell) in previous.iter_mut().enumerate().take(width) {
        *cell = column;
    }
    for row in 1..=a.len() {
        current[0] = row;
        let mut lowest = row;
        for column in 1..width {
            let cost = usize::from(a[row - 1] != b[column - 1]);
            let mut value = (previous[column] + 1).min(current[column - 1] + 1).min(previous[column - 1] + cost);
            if row > 1 && column > 1 && a[row - 1] == b[column - 2] && a[row - 2] == b[column - 1] {
                value = value.min(before[column - 2] + 1);
            }
            current[column] = value;
            lowest = lowest.min(value);
        }
        if lowest > most {
            return None;
        }
        before[..width].copy_from_slice(&previous[..width]);
        previous[..width].copy_from_slice(&current[..width]);
    }
    let found = previous[b.len()];
    (found <= most).then_some(found)
}

fn in_case_of(word: &str, candidate: &str) -> String {
    let letters: Vec<char> = word.chars().filter(char::is_ascii_alphabetic).collect();
    if letters.len() > 1 && letters.iter().all(char::is_ascii_uppercase) {
        return candidate.to_ascii_uppercase();
    }
    if word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        let mut chars = candidate.chars();
        return chars.next().map(|first| first.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default();
    }
    candidate.to_owned()
}
