use std::io::{Read, Write};
use std::sync::OnceLock;

const WEB2: &[u8] = include_bytes!("../data/web2.z");

const EXTRA: &str = include_str!("../data/extra.txt");

const EXCLUDED: &str = include_str!("../data/excluded.txt");

const SHORTEST_STEM: usize = 3;

const SHORTEST_PREFIXED: usize = 5;

const SHORTEST_PART: usize = 4;

const INFLECTIONS: &[(&str, &[&str])] = &[
    ("'s", &[""]),
    ("ies", &["y"]),
    ("ied", &["y"]),
    ("ier", &["y"]),
    ("iers", &["y"]),
    ("iest", &["y"]),
    ("ily", &["y"]),
    ("es", &[""]),
    ("s", &[""]),
    ("ed", &["", "e"]),
    ("d", &[""]),
    ("ing", &["", "e"]),
    ("ings", &["", "e"]),
    ("ers", &["", "e"]),
    ("er", &["", "e"]),
    ("est", &["", "e"]),
    ("ly", &["", "le"]),
];

const DERIVATIONS: &[(&str, &[&str])] = &[
    ("ization", &["ize"]),
    ("izations", &["ize"]),
    ("ations", &["e", "", "ate"]),
    ("ation", &["e", "", "ate"]),
    ("ability", &["", "e", "able"]),
    ("able", &["", "e"]),
    ("ible", &["", "e"]),
    ("ness", &[""]),
    ("ments", &[""]),
    ("ment", &[""]),
    ("izer", &["ize", ""]),
    ("izers", &["ize", ""]),
    ("ize", &["", "e"]),
    ("ized", &["", "e"]),
    ("izes", &["", "e"]),
    ("izing", &["", "e"]),
    ("less", &[""]),
    ("ful", &[""]),
    ("ity", &["", "e"]),
    ("al", &["", "e"]),
    ("ally", &[""]),
];

const PREFIXES: &[&str] =
    &["un", "re", "pre", "non", "de", "dis", "sub", "mis", "multi", "inter", "over", "under", "super", "auto"];

pub struct Dictionary {
    words: String,
    starts: Vec<u32>,
}

impl Dictionary {
    pub fn from_words<I, S>(words: I) -> Dictionary
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let words = normalized(words);
        let mut joined = String::with_capacity(words.iter().map(|word| word.len() + 1).sum());
        let mut starts = Vec::with_capacity(words.len());
        for word in &words {
            starts.push(joined.len() as u32);
            joined.push_str(word);
            joined.push('\n');
        }
        Dictionary { words: joined, starts }
    }

    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        (0..self.starts.len()).map(|index| self.word(index))
    }

    fn word(&self, index: usize) -> &str {
        let start = self.starts[index] as usize;
        let end = self.starts.get(index + 1).map_or(self.words.len(), |next| *next as usize) - 1;
        &self.words[start..end]
    }

    pub fn contains(&self, word: &str) -> bool {
        self.find(word)
    }

    fn find(&self, word: &str) -> bool {
        let (mut low, mut high) = (0, self.starts.len());
        while low < high {
            let middle = (low + high) / 2;
            match self.word(middle).cmp(word) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }

    pub fn knows(&self, word: &str) -> bool {
        let lower = word.to_ascii_lowercase();
        let lower = lower.replace('\u{2019}', "'");
        self.inflected(&lower) || self.compound(&lower)
    }

    fn inflected(&self, word: &str) -> bool {
        if self.base(word) || self.contraction(word) {
            return true;
        }
        INFLECTIONS.iter().any(|(suffix, endings)| {
            let Some(stem) = word.strip_suffix(suffix) else { return false };
            if stem.len() < SHORTEST_STEM || (*suffix == "s" && stem.ends_with('s')) {
                return false;
            }
            if *suffix == "d" && !stem.ends_with('e') {
                return false;
            }
            endings.iter().any(|ending| self.base(&format!("{stem}{ending}")))
                || (doubled(stem) && self.base(&stem[..stem.len() - 1]))
        })
    }

    fn contraction(&self, word: &str) -> bool {
        if let Some(stem) = word.strip_suffix("n't") {
            return matches!(stem, "ca" | "wo" | "sha") || (stem.len() >= 2 && self.base(stem));
        }
        ["'ll", "'d", "'ve", "'re", "'m"]
            .iter()
            .any(|suffix| word.strip_suffix(suffix).is_some_and(|stem| !stem.is_empty() && self.base(stem)))
    }

    fn base(&self, word: &str) -> bool {
        self.find(word) || self.derived(word, 2) || self.british(word)
    }

    fn british(&self, word: &str) -> bool {
        let known = |candidate: String| candidate != word && (self.find(&candidate) || self.derived(&candidate, 2));
        let replaced = |from: &str, to: &str| {
            word.rfind(from).map(|at| format!("{}{to}{}", &word[..at], &word[at + from.len()..]))
        };
        let endings = [
            ("tre", "ter"),
            ("tres", "ters"),
            ("tred", "tered"),
            ("ence", "ense"),
            ("ogue", "og"),
            ("lled", "led"),
            ("lling", "ling"),
            ("ise", "ize"),
            ("ised", "ized"),
            ("ises", "izes"),
            ("ising", "izing"),
            ("iser", "izer"),
            ("isers", "izers"),
            ("isation", "ization"),
            ("isations", "izations"),
            ("isable", "izable"),
            ("isability", "izability"),
        ];
        replaced("our", "or").is_some_and(known)
            || endings.iter().any(|(from, to)| {
                word.strip_suffix(from).is_some_and(|stem| stem.len() >= 2 && known(format!("{stem}{to}")))
            })
    }

    fn derived(&self, word: &str, depth: u8) -> bool {
        if depth == 0 {
            return false;
        }
        let known = |candidate: &str| self.find(candidate) || self.derived(candidate, depth - 1);
        DERIVATIONS.iter().any(|(suffix, endings)| {
            word.strip_suffix(suffix).is_some_and(|stem| {
                stem.len() >= SHORTEST_STEM
                    && (endings.iter().any(|ending| known(&format!("{stem}{ending}")))
                        || (doubled(stem) && known(&stem[..stem.len() - 1])))
            })
        }) || PREFIXES
            .iter()
            .any(|prefix| word.strip_prefix(prefix).is_some_and(|rest| rest.len() >= SHORTEST_PREFIXED && known(rest)))
    }

    fn compound(&self, word: &str) -> bool {
        if word.len() < 2 * SHORTEST_PART || !word.is_ascii() {
            return false;
        }
        (SHORTEST_PART..=word.len() - SHORTEST_PART)
            .any(|split| self.find(&word[..split]) && self.inflected(&word[split..]))
    }
}

fn doubled(stem: &str) -> bool {
    let bytes = stem.as_bytes();
    bytes.len() >= 2 && bytes[bytes.len() - 1] == bytes[bytes.len() - 2] && !b"aeiou".contains(&bytes[bytes.len() - 1])
}

fn normalized<I, S>(words: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut words: Vec<String> = words
        .into_iter()
        .map(|word| word.as_ref().trim().to_ascii_lowercase())
        .filter(|word| !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'\''))
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

pub fn encode<I, S>(words: I) -> Vec<u8>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let words = normalized(words);
    let mut coded = Vec::new();
    let mut previous = "";
    for word in &words {
        let shared = previous.bytes().zip(word.bytes()).take_while(|(a, b)| a == b).count().min(255);
        coded.push(shared as u8);
        coded.extend_from_slice(&word.as_bytes()[shared..]);
        coded.push(b'\n');
        previous = word;
    }
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    let _ = encoder.write_all(&coded);
    encoder.finish().unwrap_or_default()
}

pub fn decode(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut coded = Vec::new();
    flate2::read::DeflateDecoder::new(bytes).read_to_end(&mut coded).map_err(|error| error.to_string())?;
    let mut words = Vec::new();
    let mut previous = String::new();
    let mut rest = coded.as_slice();
    while let Some((&shared, tail)) = rest.split_first() {
        let end = tail.iter().position(|byte| *byte == b'\n').ok_or("a word without its end")?;
        let suffix = std::str::from_utf8(&tail[..end]).map_err(|error| error.to_string())?;
        let shared = usize::from(shared);
        if shared > previous.len() {
            return Err("a word shares more than the word before it has".to_owned());
        }
        let word = format!("{}{suffix}", &previous[..shared]);
        words.push(word.clone());
        previous = word;
        rest = &tail[end + 1..];
    }
    Ok(words)
}

static ENGLISH: OnceLock<Dictionary> = OnceLock::new();

pub fn english() -> &'static Dictionary {
    ENGLISH.get_or_init(|| {
        let excluded: Vec<&str> = EXCLUDED.lines().map(str::trim).collect();
        let mut words = decode(WEB2).unwrap_or_default();
        words.retain(|word| !excluded.contains(&word.as_str()));
        words.extend(EXTRA.lines().map(str::to_owned));
        Dictionary::from_words(words)
    })
}

pub fn english_if_loaded() -> Option<&'static Dictionary> {
    ENGLISH.get()
}

pub fn extra_words() -> impl Iterator<Item = &'static str> {
    EXTRA.lines().map(str::trim).filter(|line| !line.is_empty())
}
