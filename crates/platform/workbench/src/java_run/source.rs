use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainMethod {
    pub class: String,
    pub class_line: u32,
    pub line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    text: String,
    line: u32,
}

fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut line = 0u32;
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        if c == '\n' {
            line += 1;
            at += 1;
        } else if c.is_whitespace() {
            at += 1;
        } else if c == '/' && next == Some('/') {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
        } else if c == '/' && next == Some('*') {
            at += 2;
            while at < chars.len() && !(chars[at] == '*' && chars.get(at + 1) == Some(&'/')) {
                line += u32::from(chars[at] == '\n');
                at += 1;
            }
            at += 2;
        } else if c == '"' && next == Some('"') && chars.get(at + 2) == Some(&'"') {
            at += 3;
            while at < chars.len()
                && !(chars[at] == '"' && chars.get(at + 1) == Some(&'"') && chars.get(at + 2) == Some(&'"'))
            {
                line += u32::from(chars[at] == '\n');
                at += if chars[at] == '\\' { 2 } else { 1 };
            }
            at += 3;
            tokens.push(Token { text: "\"\"".to_owned(), line });
        } else if c == '"' || c == '\'' {
            at += 1;
            while at < chars.len() && chars[at] != c && chars[at] != '\n' {
                at += if chars[at] == '\\' { 2 } else { 1 };
            }
            at += 1;
            tokens.push(Token { text: "\"\"".to_owned(), line });
        } else if c.is_alphanumeric() || c == '_' || c == '$' {
            let start = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_' || chars[at] == '$') {
                at += 1;
            }
            tokens.push(Token { text: chars[start..at].iter().collect(), line });
        } else if c == '.' && next == Some('.') && chars.get(at + 2) == Some(&'.') {
            tokens.push(Token { text: "...".to_owned(), line });
            at += 3;
        } else {
            tokens.push(Token { text: c.to_string(), line });
            at += 1;
        }
    }
    tokens
}

fn main_parameters(params: &[&str]) -> bool {
    let params: Vec<&str> = params.iter().copied().filter(|token| *token != "final").collect();
    let params: Vec<&str> = match params.as_slice() {
        ["java", ".", "lang", ".", rest @ ..] => rest.to_vec(),
        other => other.to_vec(),
    };
    matches!(params.as_slice(), [] | ["String", "[", "]", _] | ["String", "...", _] | ["String", _, "[", "]"])
}

pub fn main_methods(path: &Path, text: &str) -> Vec<MainMethod> {
    let tokens = tokens(text);
    let package = tokens
        .iter()
        .position(|token| token.text == "package")
        .map(|start| {
            tokens[start + 1..]
                .iter()
                .take_while(|token| token.text != ";")
                .map(|token| token.text.as_str())
                .collect::<String>()
        })
        .unwrap_or_default();
    let qualify = |name: &str| if package.is_empty() { name.to_owned() } else { format!("{package}.{name}") };
    let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();

    let mut types: Vec<(String, usize, u32)> = Vec::new();
    let mut pending: Option<(String, u32)> = None;
    let mut found: Vec<(MainMethod, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut statement_start = 0usize;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        match token.text.as_str() {
            "class" | "interface" | "enum" | "record"
                if index == 0 || tokens[index - 1].text != "." && tokens[index - 1].text != "@" =>
            {
                if let Some(name) = tokens
                    .get(index + 1)
                    .filter(|next| next.text.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$'))
                {
                    pending = Some((name.text.clone(), token.line));
                }
            }
            "{" => {
                depth += 1;
                if let Some((name, line)) = pending.take() {
                    let binary = match types.last() {
                        Some((outer, _, _)) => format!("{outer}${name}"),
                        None => qualify(&name),
                    };
                    types.push((binary, depth, line));
                }
                statement_start = index + 1;
            }
            "}" => {
                if let Some((_, body, _)) = types.last()
                    && *body == depth
                {
                    let (name, _, _) = types.pop().unwrap_or_default();
                    for (main, _) in found.iter_mut().filter(|(main, _)| main.class == name) {
                        main.end_line = token.line;
                    }
                }
                depth = depth.saturating_sub(1);
                statement_start = index + 1;
            }
            ";" => statement_start = index + 1,
            "main"
                if index > 0
                    && tokens[index - 1].text == "void"
                    && tokens.get(index + 1).is_some_and(|next| next.text == "(") =>
            {
                let in_body = match types.last() {
                    Some((_, body, _)) => *body == depth,
                    None => depth == 0,
                };
                let close = tokens[index + 1..].iter().position(|token| token.text == ")").map(|at| index + 1 + at);
                if let (true, Some(close)) = (in_body, close) {
                    let params: Vec<&str> = tokens[index + 2..close].iter().map(|token| token.text.as_str()).collect();
                    let modifiers: Vec<&str> =
                        tokens[statement_start..index].iter().map(|token| token.text.as_str()).collect();
                    let is_static = modifiers.contains(&"static");
                    let private = modifiers.contains(&"private");
                    if main_parameters(&params) && !private {
                        let (class, class_line) = match types.last() {
                            Some((name, _, line)) => (name.clone(), *line),
                            None => (qualify(&stem), token.line),
                        };
                        let rank = usize::from(is_static) * 2 + usize::from(!params.is_empty());
                        let main = MainMethod { class, class_line, line: token.line, end_line: u32::MAX };
                        match found.iter_mut().find(|(other, _)| other.class == main.class) {
                            Some(existing) if existing.1 < rank => *existing = (main, rank),
                            Some(_) => {}
                            None => found.push((main, rank)),
                        }
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    found
        .into_iter()
        .map(|(mut main, _)| {
            if main.end_line == u32::MAX {
                main.end_line = tokens.last().map_or(main.line, |token| token.line);
            }
            main
        })
        .collect()
}

pub fn class_at(path: &Path, text: &str, line: u32) -> Option<String> {
    let tokens = tokens(text);
    let package = tokens
        .iter()
        .position(|token| token.text == "package")
        .map(|start| {
            tokens[start + 1..]
                .iter()
                .take_while(|token| token.text != ";")
                .map(|token| token.text.as_str())
                .collect::<String>()
        })
        .unwrap_or_default();
    let qualify = |name: &str| if package.is_empty() { name.to_owned() } else { format!("{package}.{name}") };
    let mut open: Vec<(String, usize, u32)> = Vec::new();
    let mut spans: Vec<(String, u32, u32, usize)> = Vec::new();
    let mut pending: Option<(String, u32)> = None;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "class" | "interface" | "enum" | "record"
                if index == 0 || tokens[index - 1].text != "." && tokens[index - 1].text != "@" =>
            {
                if let Some(name) = tokens
                    .get(index + 1)
                    .filter(|next| next.text.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$'))
                {
                    pending = Some((name.text.clone(), token.line));
                }
            }
            ";" if pending.is_some() && open.is_empty() && depth == 0 => pending = None,
            "{" => {
                depth += 1;
                if let Some((name, start)) = pending.take() {
                    let binary = match open.last() {
                        Some((outer, _, _)) => format!("{outer}${name}"),
                        None => qualify(&name),
                    };
                    open.push((binary, depth, start));
                }
            }
            "}" => {
                if open.last().is_some_and(|(_, body, _)| *body == depth)
                    && let Some((name, _, start)) = open.pop()
                {
                    spans.push((name, start, token.line, open.len()));
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    let end = tokens.last().map_or(0, |token| token.line);
    spans.extend(open.into_iter().enumerate().map(|(nesting, (name, _, start))| (name, start, end, nesting)));
    let inside = spans
        .iter()
        .filter(|(_, start, stop, _)| (*start..=*stop).contains(&line))
        .max_by_key(|(_, _, _, nesting)| *nesting);
    if let Some((name, ..)) = inside {
        return Some(name.clone());
    }
    if let Some((name, ..)) = spans.iter().filter(|(.., nesting)| *nesting == 0).min_by_key(|(_, start, ..)| *start) {
        return Some(name.clone());
    }
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    (!stem.is_empty() && !tokens.is_empty()).then(|| qualify(&stem))
}

pub(super) fn source_files(module: &Path) -> Vec<PathBuf> {
    let root = [module.join("src/main/java"), module.join("src")]
        .into_iter()
        .find(|folder| folder.is_dir())
        .unwrap_or_else(|| module.to_owned());
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    files
}

fn collect(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !name.starts_with('.') && !["test", "target", "build", "out", "node_modules"].contains(&name.as_str()) {
                collect(&path, files);
            }
        } else if name.ends_with(".java") {
            files.push(path);
        }
    }
}

pub(super) fn has_kotlin(module: &Path) -> bool {
    fn walk(folder: &Path, depth: usize) -> bool {
        let Ok(entries) = std::fs::read_dir(folder) else { return false };
        entries.flatten().any(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                depth < 12
                    && !name.starts_with('.')
                    && !["test", "build", "target", "out"].contains(&name.as_str())
                    && walk(&path, depth + 1)
            } else {
                name.ends_with(".kt")
            }
        })
    }
    module.join("src/main/kotlin").is_dir() || walk(&module.join("src"), 0)
}
