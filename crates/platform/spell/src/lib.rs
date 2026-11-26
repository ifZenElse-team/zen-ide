mod dictionary;
pub mod hunspell;
pub mod idea_dictionary;
mod suggest;
mod words;

use std::collections::HashMap;
use std::ops::Range;

pub use dictionary::{Dictionary, decode, encode, english, english_if_loaded, extra_words};
pub use suggest::suggestions;
pub use words::{Context, Region, SHORTEST_WORD, words};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Typo {
    pub range: Range<usize>,
    pub word: String,
    pub context: Context,
}

pub fn typos(dictionary: &Dictionary, text: &str, regions: &[Region]) -> Vec<Typo> {
    typos_with(&[dictionary], text, regions)
}

pub fn typos_with(dictionaries: &[&Dictionary], text: &str, regions: &[Region]) -> Vec<Typo> {
    let mut judged: HashMap<&str, bool> = HashMap::new();
    let mut found = Vec::new();
    for region in regions {
        for range in words(text, region) {
            let word = &text[range.clone()];
            let known =
                *judged.entry(word).or_insert_with(|| dictionaries.iter().any(|dictionary| dictionary.knows(word)));
            if !known {
                found.push(Typo { range, word: word.to_owned(), context: region.context });
            }
        }
    }
    found
}

pub fn word_list(text: &str) -> Vec<String> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests;
