use std::collections::BTreeMap;

const MOST_FORMS: usize = 4_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flags {
    Short,
    Long,
    Number,
    Utf8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Class {
    Any,
    Char(char),
    Set(Vec<char>, bool),
}

impl Class {
    fn matches(&self, c: char) -> bool {
        match self {
            Class::Any => true,
            Class::Char(wanted) => *wanted == c,
            Class::Set(chars, negated) => chars.contains(&c) != *negated,
        }
    }
}

struct Rule {
    strip: String,
    add: String,
    condition: Vec<Class>,
}

struct Table {
    cross: bool,
    expected: usize,
    rules: Vec<Rule>,
}

#[derive(Default)]
struct Affixes {
    flags: Option<Flags>,
    aliases: Vec<Vec<u32>>,
    aliases_counted: bool,
    tables: BTreeMap<(bool, u32), Table>,
    need_affix: Option<u32>,
    forbidden: Option<u32>,
}

pub fn words(aff: &[u8], dic: &[u8]) -> Result<Vec<String>, String> {
    let utf8 = encoding(aff).is_none_or(|name| {
        let name = name.to_ascii_uppercase();
        name == "UTF-8" || name == "UTF8"
    });
    let decode = |bytes: &[u8]| -> String {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(bytes);
        if utf8 {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            bytes.iter().map(|byte| char::from(*byte)).collect()
        }
    };
    let affixes = affixes(&decode(aff))?;
    let dic = decode(dic);
    let mut forms = Vec::new();
    let mut lines = dic.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).peekable();
    if lines.peek().is_some_and(|first| first.chars().all(|c| c.is_ascii_digit())) {
        lines.next();
    }
    let suffixes: Vec<(&u32, &Table)> =
        affixes.tables.iter().filter(|((prefix, _), _)| !prefix).map(|((_, flag), table)| (flag, table)).collect();
    let prefixes: Vec<(&u32, &Table)> =
        affixes.tables.iter().filter(|((prefix, _), _)| *prefix).map(|((_, flag), table)| (flag, table)).collect();
    for line in lines {
        let (stem, flags) = entry(line, &affixes);
        if stem.is_empty() || affixes.forbidden.is_some_and(|forbidden| flags.contains(&forbidden)) {
            continue;
        }
        if !affixes.need_affix.is_some_and(|need| flags.contains(&need)) {
            forms.push(stem.clone());
        }
        let mut crossing = Vec::new();
        for (flag, table) in &suffixes {
            if !flags.contains(flag) {
                continue;
            }
            for rule in &table.rules {
                if let Some(form) = suffixed(&stem, rule) {
                    if table.cross {
                        crossing.push(form.clone());
                    }
                    forms.push(form);
                }
            }
        }
        for (flag, table) in &prefixes {
            if !flags.contains(flag) {
                continue;
            }
            for rule in &table.rules {
                if let Some(form) = prefixed(&stem, rule) {
                    forms.push(form);
                }
                if table.cross {
                    forms.extend(crossing.iter().filter_map(|form| prefixed(form, rule)));
                }
            }
        }
        if forms.len() > MOST_FORMS {
            return Err(format!("more than {MOST_FORMS} words"));
        }
    }
    Ok(forms)
}

fn encoding(aff: &[u8]) -> Option<String> {
    aff.split(|byte| *byte == b'\n').find_map(|line| {
        let line: String = line.iter().map(|byte| char::from(*byte)).collect();
        let mut parts = line.split_whitespace();
        (parts.next() == Some("SET")).then(|| parts.next().map(str::to_owned)).flatten()
    })
}

fn affixes(text: &str) -> Result<Affixes, String> {
    let mut affixes = Affixes::default();
    for (number, line) in text.lines().enumerate() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let Some(keyword) = parts.first() else { continue };
        let kind = affixes.flags.unwrap_or(Flags::Short);
        match *keyword {
            "FLAG" => {
                affixes.flags = match parts.get(1).copied() {
                    Some("long") => Some(Flags::Long),
                    Some("num") => Some(Flags::Number),
                    Some("UTF-8") => Some(Flags::Utf8),
                    _ => Some(Flags::Short),
                };
            }
            "AF" => {
                let Some(value) = parts.get(1) else { continue };
                if !affixes.aliases_counted && value.chars().all(|c| c.is_ascii_digit()) {
                    affixes.aliases_counted = true;
                } else {
                    affixes.aliases.push(flags_of(value, kind));
                }
            }
            "NEEDAFFIX" | "PSEUDOROOT" => affixes.need_affix = parts.get(1).and_then(|flag| one_flag(flag, kind)),
            "FORBIDDENWORD" => affixes.forbidden = parts.get(1).and_then(|flag| one_flag(flag, kind)),
            "PFX" | "SFX" => {
                let prefix = *keyword == "PFX";
                let flag = parts
                    .get(1)
                    .and_then(|flag| one_flag(flag, kind))
                    .ok_or_else(|| format!("line {}: an affix without a flag", number + 1))?;
                let table = affixes.tables.get_mut(&(prefix, flag));
                match table {
                    Some(table) if table.rules.len() < table.expected => {
                        let (Some(strip), Some(add)) = (parts.get(2), parts.get(3)) else {
                            return Err(format!("line {}: an affix rule needs strip and affix", number + 1));
                        };
                        let add = add.split_once('/').map_or(*add, |(add, _)| add);
                        let empty = |part: &str| if part == "0" { String::new() } else { part.to_owned() };
                        table.rules.push(Rule {
                            strip: empty(strip),
                            add: empty(add),
                            condition: condition(parts.get(4).copied().unwrap_or(".")),
                        });
                    }
                    _ => {
                        let cross = parts.get(2) == Some(&"Y");
                        let expected = parts.get(3).and_then(|count| count.parse().ok()).unwrap_or(0);
                        affixes.tables.insert((prefix, flag), Table { cross, expected, rules: Vec::new() });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(affixes)
}

fn flags_of(text: &str, kind: Flags) -> Vec<u32> {
    match kind {
        Flags::Short | Flags::Utf8 => text.chars().map(u32::from).collect(),
        Flags::Long => {
            let chars: Vec<char> = text.chars().collect();
            chars.chunks(2).map(|pair| pair.iter().fold(0u32, |code, c| (code << 16) | u32::from(*c))).collect()
        }
        Flags::Number => text.split(',').filter_map(|number| number.trim().parse().ok()).collect(),
    }
}

fn one_flag(text: &str, kind: Flags) -> Option<u32> {
    flags_of(text, kind).first().copied()
}

fn entry(line: &str, affixes: &Affixes) -> (String, Vec<u32>) {
    let field = line.split(['\t', ' ']).next().unwrap_or_default();
    let mut stem = String::new();
    let mut flags = None;
    let mut chars = field.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' if chars.peek().is_some_and(|(_, next)| *next == '/') => {
                stem.push('/');
                chars.next();
            }
            '/' if at > 0 => {
                flags = Some(&field[at + 1..]);
                break;
            }
            _ => stem.push(c),
        }
    }
    let kind = affixes.flags.unwrap_or(Flags::Short);
    let flags = match flags {
        None => Vec::new(),
        Some(text) if !affixes.aliases.is_empty() && text.chars().all(|c| c.is_ascii_digit()) => text
            .parse::<usize>()
            .ok()
            .and_then(|index| affixes.aliases.get(index.checked_sub(1)?))
            .cloned()
            .unwrap_or_default(),
        Some(text) => flags_of(text, kind),
    };
    (stem, flags)
}

fn condition(text: &str) -> Vec<Class> {
    let mut classes = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '.' => classes.push(Class::Any),
            '[' => {
                let mut set = Vec::new();
                let mut negated = false;
                let mut first = true;
                for inner in chars.by_ref() {
                    if inner == ']' {
                        break;
                    }
                    if first && inner == '^' {
                        negated = true;
                    } else {
                        set.push(inner);
                    }
                    first = false;
                }
                classes.push(Class::Set(set, negated));
            }
            other => classes.push(Class::Char(other)),
        }
    }
    if classes == [Class::Any] { Vec::new() } else { classes }
}

fn suffixed(stem: &str, rule: &Rule) -> Option<String> {
    let chars: Vec<char> = stem.chars().collect();
    if rule.condition.len() > chars.len() {
        return None;
    }
    let tail = &chars[chars.len() - rule.condition.len()..];
    if !rule.condition.iter().zip(tail).all(|(class, c)| class.matches(*c)) {
        return None;
    }
    let base = stem.strip_suffix(rule.strip.as_str())?;
    if base.is_empty() && rule.add.is_empty() {
        return None;
    }
    Some(format!("{base}{}", rule.add))
}

fn prefixed(stem: &str, rule: &Rule) -> Option<String> {
    let chars: Vec<char> = stem.chars().collect();
    if rule.condition.len() > chars.len() {
        return None;
    }
    if !rule.condition.iter().zip(&chars).all(|(class, c)| class.matches(*c)) {
        return None;
    }
    let base = stem.strip_prefix(rule.strip.as_str())?;
    if base.is_empty() && rule.add.is_empty() {
        return None;
    }
    Some(format!("{}{base}", rule.add))
}
