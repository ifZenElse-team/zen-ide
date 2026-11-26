use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Number(String),
    Word(String),
}

const UNSTABLE: [&str; 14] = [
    "alpha",
    "beta",
    "milestone",
    "rc",
    "cr",
    "snapshot",
    "pre",
    "preview",
    "ea",
    "eap",
    "dev",
    "nightly",
    "canary",
    "incubating",
];

const SHORT_UNSTABLE: [&str; 3] = ["a", "b", "m"];

const NEUTRAL: [&str; 3] = ["final", "ga", "release"];

fn tokens(version: &str) -> Vec<Token> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut digits = false;
    let flush = |current: &mut String, digits: bool, found: &mut Vec<Token>| {
        if !current.is_empty() {
            let text = std::mem::take(current);
            found.push(if digits { Token::Number(text) } else { Token::Word(text) });
        }
    };
    for c in version.chars() {
        if c == '.' || c == '-' || c == '_' || c == '+' {
            flush(&mut current, digits, &mut found);
            continue;
        }
        let digit = c.is_ascii_digit();
        if digit != digits {
            flush(&mut current, digits, &mut found);
            digits = digit;
        }
        current.extend(c.to_lowercase());
    }
    flush(&mut current, digits, &mut found);
    found
}

fn expanded(tokens: Vec<Token>) -> Vec<Token> {
    let followed: Vec<bool> =
        tokens.iter().enumerate().map(|(at, _)| matches!(tokens.get(at + 1), Some(Token::Number(_)))).collect();
    tokens
        .into_iter()
        .zip(followed)
        .map(|(token, numbered)| match token {
            Token::Word(word) if numbered && word == "a" => Token::Word("alpha".to_owned()),
            Token::Word(word) if numbered && word == "b" => Token::Word("beta".to_owned()),
            Token::Word(word) if numbered && word == "m" => Token::Word("milestone".to_owned()),
            Token::Word(word) if word == "cr" => Token::Word("rc".to_owned()),
            other => other,
        })
        .collect()
}

fn is_null(token: &Token) -> bool {
    match token {
        Token::Number(number) => number.bytes().all(|byte| byte == b'0'),
        Token::Word(word) => word.is_empty() || NEUTRAL.contains(&word.as_str()),
    }
}

fn trimmed(version: &str) -> Vec<Token> {
    let mut found = expanded(tokens(version));
    while found.last().is_some_and(is_null) {
        found.pop();
    }
    found
}

fn rank(word: &str) -> usize {
    match word {
        "alpha" => 0,
        "beta" => 1,
        "milestone" => 2,
        "rc" => 3,
        "snapshot" => 4,
        "" | "final" | "ga" | "release" => 5,
        "sp" => 6,
        _ => 7,
    }
}

fn compare_numbers(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn compare_tokens(a: Option<&Token>, b: Option<&Token>) -> Ordering {
    match (a, b) {
        (Some(Token::Number(a)), Some(Token::Number(b))) => compare_numbers(a, b),
        (Some(Token::Number(a)), None) => compare_numbers(a, ""),
        (None, Some(Token::Number(b))) => compare_numbers("", b),
        (Some(Token::Number(_)), Some(Token::Word(_))) => Ordering::Greater,
        (Some(Token::Word(_)), Some(Token::Number(_))) => Ordering::Less,
        (Some(Token::Word(a)), Some(Token::Word(b))) => rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)),
        (Some(Token::Word(a)), None) => rank(a).cmp(&rank("")).then_with(|| a.as_str().cmp("")),
        (None, Some(Token::Word(b))) => rank("").cmp(&rank(b)).then_with(|| "".cmp(b.as_str())),
        (None, None) => Ordering::Equal,
    }
}

pub fn compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (trimmed(a), trimmed(b));
    (0..a.len().max(b.len()))
        .map(|at| compare_tokens(a.get(at), b.get(at)))
        .find(|order| order.is_ne())
        .unwrap_or(Ordering::Equal)
}

pub fn is_stable(version: &str) -> bool {
    let tokens = tokens(version);
    !tokens.iter().enumerate().any(|(at, token)| match token {
        Token::Word(word) => {
            UNSTABLE.contains(&word.as_str())
                || (SHORT_UNSTABLE.contains(&word.as_str()) && matches!(tokens.get(at + 1), Some(Token::Number(_))))
        }
        Token::Number(_) => false,
    })
}

fn flavour(version: &str) -> Vec<String> {
    expanded(tokens(version))
        .into_iter()
        .filter_map(|token| match token {
            Token::Word(word) if rank(&word) == 7 => Some(word),
            _ => None,
        })
        .collect()
}

fn is_date(version: &str) -> bool {
    matches!(tokens(version).first(), Some(Token::Number(number)) if number.len() >= 8)
}

pub fn offered_for(current: &str, candidate: &str) -> bool {
    let wanted = flavour(current);
    (is_stable(candidate) || !is_stable(current))
        && (wanted.is_empty() || flavour(candidate) == wanted)
        && (!is_date(candidate) || is_date(current))
}

pub fn newest_first(mut versions: Vec<String>) -> Vec<String> {
    versions.sort_by(|a, b| compare(b, a));
    versions.dedup();
    versions
}

pub fn latest_stable(versions: &[String]) -> Option<String> {
    let usual = versions.iter().filter(|version| is_stable(version) && !is_date(version));
    usual.max_by(|a, b| compare(a, b)).or_else(|| versions.iter().max_by(|a, b| compare(a, b))).cloned()
}

pub fn newer_than(current: &str, versions: &[String]) -> Vec<String> {
    let newer = versions.iter().filter(|candidate| offered_for(current, candidate));
    newest_first(newer.filter(|candidate| compare(candidate, current).is_gt()).cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_sort_in_the_order_maven_documents() {
        for (older, newer) in [
            ("1.0-alpha-1", "1.0-beta-1"),
            ("1.0-beta2", "1.0-M1"),
            ("1.0-M1", "1.0-RC1"),
            ("1.0-rc1", "1.0-SNAPSHOT"),
            ("1.0-SNAPSHOT", "1.0"),
            ("1.0", "1.0-sp1"),
            ("1.0-sp1", "1.0.1"),
            ("1.9", "1.10"),
            ("2.0.0.Final", "2.0.1.Final"),
            ("3.5.3", "4.1.2"),
        ] {
            assert_eq!(compare(older, newer), Ordering::Less, "{older} < {newer}");
            assert_eq!(compare(newer, older), Ordering::Greater, "{newer} > {older}");
        }
        for (a, b) in [("1", "1.0.0"), ("1.0.RELEASE", "1.0"), ("1.0-ga", "1"), ("1.0-CR1", "1.0-rc1")] {
            assert_eq!(compare(a, b), Ordering::Equal, "{a} = {b}");
        }
    }

    #[test]
    fn pre_releases_are_not_stable_and_release_words_are() {
        for version in ["1.0-SNAPSHOT", "1.0-rc.1", "1.0.0-M3", "1.0b2", "1.0-alpha", "2.0-EA1", "1.0-beta-1"] {
            assert!(!is_stable(version), "{version}");
        }
        for version in ["1.0", "6.1.0.Final", "5.3.20.RELEASE", "33.0.0-jre", "1.0-GA", "1.0-m"] {
            assert!(is_stable(version), "{version}");
        }
    }

    #[test]
    fn the_latest_stable_version_skips_pre_releases_and_dates() {
        let versions: Vec<String> =
            ["20030203.000550", "2.11.0", "2.16.1", "3.0.0-M1"].into_iter().map(str::to_owned).collect();
        assert_eq!(latest_stable(&versions).as_deref(), Some("2.16.1"));
        assert_eq!(latest_stable(&["3.0-rc1".to_owned()]).as_deref(), Some("3.0-rc1"));
    }

    #[test]
    fn newer_versions_keep_the_flavour_of_the_current_one() {
        let versions: Vec<String> = ["32.0.0-jre", "33.0.0-android", "33.0.0-jre", "34.0.0-rc1-jre", "31.0-jre"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(newer_than("31.1-jre", &versions), ["33.0.0-jre", "32.0.0-jre"]);
    }

    #[test]
    fn pre_releases_are_offered_to_those_who_already_use_one() {
        let versions: Vec<String> = ["2.0-M2", "2.0-rc1", "1.9"].into_iter().map(str::to_owned).collect();
        assert_eq!(newer_than("2.0-M1", &versions), ["2.0-rc1", "2.0-M2"]);
        assert!(newer_than("1.9", &versions).is_empty());
    }
}
