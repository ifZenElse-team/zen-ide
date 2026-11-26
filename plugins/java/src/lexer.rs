#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Code,
    Comment(bool),
    TextBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raw {
    Word,
    Number,
    String,
    Comment,
    Doc,
    At,
    Punct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub raw: Raw,
}

pub fn lex_line(line: &str, state: State) -> (Vec<Token>, State) {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    let mut state = state;
    let push = |tokens: &mut Vec<Token>, start: usize, end: usize, raw: Raw| {
        if end > start {
            tokens.push(Token { start, end, raw });
        }
    };
    while at < bytes.len() {
        match state {
            State::Comment(doc) => {
                let raw = if doc { Raw::Doc } else { Raw::Comment };
                match find(bytes, at, b"*/") {
                    Some(end) => {
                        push(&mut tokens, at, end + 2, raw);
                        at = end + 2;
                        state = State::Code;
                    }
                    None => {
                        push(&mut tokens, at, bytes.len(), raw);
                        at = bytes.len();
                    }
                }
                continue;
            }
            State::TextBlock => {
                let mut end = at;
                let mut closed = None;
                while end < bytes.len() {
                    if bytes[end] == b'\\' {
                        end += 2;
                        continue;
                    }
                    if bytes[end..].starts_with(b"\"\"\"") {
                        closed = Some(end + 3);
                        break;
                    }
                    end += 1;
                }
                match closed {
                    Some(end) => {
                        push(&mut tokens, at, end, Raw::String);
                        at = end;
                        state = State::Code;
                    }
                    None => {
                        push(&mut tokens, at, bytes.len(), Raw::String);
                        at = bytes.len();
                    }
                }
                continue;
            }
            State::Code => {}
        }
        let byte = bytes[at];
        if byte.is_ascii_whitespace() {
            at += 1;
        } else if bytes[at..].starts_with(b"//") {
            push(&mut tokens, at, bytes.len(), Raw::Comment);
            at = bytes.len();
        } else if bytes[at..].starts_with(b"/*") {
            let doc = bytes[at..].starts_with(b"/**") && !bytes[at..].starts_with(b"/**/");
            state = State::Comment(doc);
            match find(bytes, at + 2, b"*/") {
                Some(end) => {
                    push(&mut tokens, at, end + 2, if doc { Raw::Doc } else { Raw::Comment });
                    at = end + 2;
                    state = State::Code;
                }
                None => {
                    push(&mut tokens, at, bytes.len(), if doc { Raw::Doc } else { Raw::Comment });
                    at = bytes.len();
                }
            }
        } else if bytes[at..].starts_with(b"\"\"\"") {
            push(&mut tokens, at, bytes.len(), Raw::String);
            at = bytes.len();
            state = State::TextBlock;
        } else if byte == b'"' || byte == b'\'' {
            let end = literal_end(bytes, at, byte);
            push(&mut tokens, at, end, Raw::String);
            at = end;
        } else if byte.is_ascii_digit() || (byte == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)) {
            let end = number_end(bytes, at);
            push(&mut tokens, at, end, Raw::Number);
            at = end;
        } else if is_word_start(line, at) {
            let end = word_end(line, at);
            push(&mut tokens, at, end, Raw::Word);
            at = end;
        } else if byte == b'@' {
            push(&mut tokens, at, at + 1, Raw::At);
            at += 1;
        } else {
            let width = line[at..].chars().next().map_or(1, char::len_utf8);
            push(&mut tokens, at, at + width, Raw::Punct);
            at += width;
        }
    }
    (tokens, state)
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(needle.len()).position(|window| window == needle).map(|at| from + at)
}

fn literal_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut at = start + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            byte if byte == quote => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

fn number_end(bytes: &[u8], start: usize) -> usize {
    let mut at = start;
    let hex = bytes[at..].starts_with(b"0x") || bytes[at..].starts_with(b"0X");
    if hex || bytes[at..].starts_with(b"0b") || bytes[at..].starts_with(b"0B") {
        at += 2;
    }
    while at < bytes.len() {
        let byte = bytes[at];
        let exponent = if hex { matches!(byte, b'p' | b'P') } else { matches!(byte, b'e' | b'E') };
        if exponent && bytes.get(at + 1).is_some_and(|next| matches!(next, b'+' | b'-')) {
            at += 2;
        } else if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' {
            if byte == b'.' && !bytes.get(at + 1).is_some_and(u8::is_ascii_digit) && !hex {
                at += 1;
                break;
            }
            at += 1;
        } else {
            break;
        }
    }
    at
}

fn is_word_start(line: &str, at: usize) -> bool {
    line[at..].chars().next().is_some_and(|c| c == '_' || c == '$' || c.is_alphabetic())
}

fn word_end(line: &str, start: usize) -> usize {
    line[start..]
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '$'))
        .map_or(line.len(), |(offset, _)| start + offset)
}

pub const KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
    "true",
    "false",
    "null",
    "_",
];

pub const MODULE_WORDS: &[&str] =
    &["module", "open", "requires", "transitive", "exports", "opens", "to", "uses", "provides", "with"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Keyword,
    Type,
    Function,
    Constant,
    Annotation,
    Plain,
}

pub fn classify(line: &str, tokens: &[Token], module: bool) -> Vec<Class> {
    let text = |index: usize| tokens.get(index).map_or("", |token| &line[token.start..token.end]);
    let significant =
        |index: usize| tokens.get(index).is_some_and(|token| !matches!(token.raw, Raw::Comment | Raw::Doc));
    let next = |index: usize| (index + 1..tokens.len()).find(|&next| significant(next));
    let previous = |index: usize| (0..index).rev().find(|&previous| significant(previous));
    let mut classes = Vec::with_capacity(tokens.len());
    let mut in_case = false;
    for (index, token) in tokens.iter().enumerate() {
        if token.raw != Raw::Word {
            if token.raw == Raw::Punct && matches!(text(index), ":" | ";") {
                in_case = false;
            }
            if token.raw == Raw::Punct && text(index) == "-" && next(index).is_some_and(|n| text(n) == ">") {
                in_case = false;
            }
            classes.push(Class::Plain);
            continue;
        }
        let word = text(index);
        let after = next(index).map_or("", text);
        let before = previous(index).map_or("", text);
        let before_raw = previous(index).map(|previous| tokens[previous].raw);
        let class = if before_raw == Some(Raw::At) || (before == "." && annotation_name(line, tokens, index)) {
            if word == "interface" { Class::Keyword } else { Class::Annotation }
        } else if KEYWORDS.contains(&word) {
            if word == "case" {
                in_case = true;
            }
            Class::Keyword
        } else if (module && MODULE_WORDS.contains(&word) && before != "." && !matches!(after, "." | ";" | "(" | "="))
            || contextual_keyword(word, before, after, in_case)
        {
            Class::Keyword
        } else if after == "(" {
            Class::Function
        } else if is_constant(word) {
            Class::Constant
        } else if word.chars().next().is_some_and(char::is_uppercase) {
            Class::Type
        } else {
            Class::Plain
        };
        classes.push(class);
    }
    for index in 0..tokens.len() {
        if text(index) == "non"
            && text(index + 1) == "-"
            && text(index + 2) == "sealed"
            && tokens[index].end == tokens[index + 1].start
            && tokens[index + 1].end == tokens[index + 2].start
        {
            classes[index] = Class::Keyword;
            classes[index + 2] = Class::Keyword;
        }
    }
    classes
}

fn annotation_name(line: &str, tokens: &[Token], index: usize) -> bool {
    let mut at = index;
    while at >= 2 {
        let dot = &line[tokens[at - 1].start..tokens[at - 1].end];
        if dot != "." || tokens[at - 2].raw != Raw::Word {
            return false;
        }
        at -= 2;
        if at >= 1 && tokens[at - 1].raw == Raw::At {
            return true;
        }
    }
    false
}

fn contextual_keyword(word: &str, before: &str, after: &str, in_case: bool) -> bool {
    let name_follows = after.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$');
    match word {
        "record" => name_follows && !matches!(before, "." | "new"),
        "sealed" | "permits" => name_follows || matches!(after, "class" | "interface"),
        "var" => name_follows && !matches!(before, "." | "new"),
        "yield" => matches!(before, "" | ";" | "{" | "}" | ":" | ">") && !matches!(after, "=" | "(" | "." | ";"),
        "when" => in_case && !matches!(after, "=" | "." | ";" | ")" | ","),
        _ => false,
    }
}

fn is_constant(word: &str) -> bool {
    word.chars().count() > 1
        && word.chars().any(char::is_alphabetic)
        && word.chars().all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_')
}

pub fn is_module_declaration(text: &str) -> bool {
    let mut state = State::Code;
    for line in text.lines() {
        let (tokens, next) = lex_line(line, state);
        state = next;
        let mut words = tokens
            .iter()
            .filter(|token| !matches!(token.raw, Raw::Comment | Raw::Doc))
            .map(|token| &line[token.start..token.end]);
        if let Some(first) = words.next() {
            return match first {
                "module" => true,
                "open" => words.next() == Some("module"),
                "import" | "@" => continue,
                _ => false,
            };
        }
    }
    false
}
