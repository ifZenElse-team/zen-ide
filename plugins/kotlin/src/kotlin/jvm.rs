use super::{Lexer, Tok};

const TEST_ANNOTATIONS: &[&str] = &["Test", "ParameterizedTest", "RepeatedTest", "TestFactory", "TestTemplate"];

const MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "internal",
    "abstract",
    "open",
    "final",
    "sealed",
    "data",
    "enum",
    "annotation",
    "inner",
    "value",
    "override",
    "suspend",
    "inline",
    "operator",
    "infix",
    "tailrec",
    "external",
    "expect",
    "actual",
    "const",
    "lateinit",
    "companion",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JvmFile {
    pub package: String,
    pub jvm_name: String,
    pub classes: Vec<JvmClass>,
    pub mains: Vec<JvmMain>,
    pub tests: Vec<JvmTest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmClass {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmMain {
    pub class: String,
    pub owner: Option<usize>,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmTest {
    pub name: String,
    pub start: usize,
    pub testng: bool,
    pub methods: Vec<(String, usize)>,
}

#[derive(Clone, Debug)]
struct Class {
    binary: String,
    outer: Option<String>,
    outer_at: Option<usize>,
    object: bool,
    companion: bool,
    runnable: bool,
    name_at: usize,
    index: usize,
    all_methods: bool,
}

#[derive(Clone, Debug)]
enum Frame {
    Class(Class),
    Code { main: Option<usize> },
}

struct Pending {
    class: Class,
}

fn last_segment(annotation: &str) -> &str {
    let name = annotation.trim_start_matches('@');
    let name = name.rsplit(':').next().unwrap_or(name);
    name.rsplit('.').next().unwrap_or(name)
}

fn main_parameters(params: &[&str], static_only: bool) -> bool {
    let params: Vec<&str> = params.iter().copied().filter(|token| *token != "kotlin" && *token != ".").collect();
    match params.as_slice() {
        [] => !static_only,
        [_, ":", "Array", "<", "String", ">"] | [_, ":", "Array", "<", "out", "String", ">"] => true,
        ["vararg", _, ":", "String"] => true,
        _ => false,
    }
}

pub fn jvm(text: &str) -> JvmFile {
    let mut lexer = Lexer::new(text);
    lexer.run();
    let tokens = &lexer.tokens;
    let slice = |index: usize| tokens.get(index).map_or("", |(_, from, to)| &text[*from..*to]);
    let kind = |index: usize| tokens.get(index).map(|(token, _, _)| *token);
    let word = |index: usize| if kind(index) == Some(Tok::Word) { slice(index).trim_matches('`') } else { "" };
    let skip_group = |mut index: usize| {
        let mut depth = 0usize;
        while let Some(token) = kind(index) {
            match token {
                Tok::Paren => depth += 1,
                Tok::CloseParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            }
            index += 1;
        }
        index
    };

    let testng = text.contains("org.testng");
    let mut file = JvmFile::default();
    let mut stack: Vec<Frame> = Vec::new();
    let mut pending: Option<Pending> = None;
    let mut annotations: Vec<String> = Vec::new();
    let mut modifiers: Vec<String> = Vec::new();
    let mut statement_start: Option<usize> = None;
    let mut seen_declaration = false;
    let qualify =
        |package: &str, name: &str| if package.is_empty() { name.to_owned() } else { format!("{package}.{name}") };
    let mut index = 0;
    while index < tokens.len() {
        let (token, start, end) = tokens[index];
        let current = stack.last().cloned();
        let in_code = matches!(current, Some(Frame::Code { .. }));
        match token {
            Tok::Other if slice(index).starts_with('@') => {
                statement_start.get_or_insert(start);
                let annotation = slice(index);
                let file_level = annotation.starts_with("@file:");
                let mut next = index + 1;
                if kind(next) == Some(Tok::Paren) && slice(next) == "(" {
                    let close = skip_group(next);
                    if file_level && last_segment(annotation) == "JvmName" && kind(next + 1) == Some(Tok::Other) {
                        file.jvm_name = slice(next + 1).trim_matches('"').to_owned();
                    }
                    next = close;
                }
                if !file_level && !in_code {
                    annotations.push(last_segment(annotation).to_owned());
                }
                index = next;
                continue;
            }
            Tok::Word => {
                let text_word = word(index);
                if in_code {
                    index += 1;
                    continue;
                }
                match text_word {
                    "package" if stack.is_empty() && !seen_declaration && file.package.is_empty() => {
                        let mut parts = String::new();
                        let mut at = index + 1;
                        while let Some(token) = kind(at) {
                            match token {
                                Tok::Word => parts.push_str(word(at)),
                                Tok::Other if slice(at) == "." => parts.push('.'),
                                _ => break,
                            }
                            at += 1;
                        }
                        file.package = parts;
                        index = at;
                        continue;
                    }
                    "import" if stack.is_empty() => {
                        while kind(index).is_some_and(|token| token != Tok::End) {
                            index += 1;
                        }
                        continue;
                    }
                    "class" | "interface" | "object" if index == 0 || !matches!(slice(index - 1), ":" | ".") => {
                        seen_declaration = true;
                        statement_start.get_or_insert(start);
                        let named = word(index + 1);
                        let companion = text_word == "object" && modifiers.iter().any(|m| m == "companion");
                        let name = if named.is_empty() || named == "interface" {
                            if companion { "Companion".to_owned() } else { String::new() }
                        } else {
                            named.to_owned()
                        };
                        let name_at = if named.is_empty() { start } else { tokens[index + 1].1 };
                        let outer = match &current {
                            Some(Frame::Class(class)) => Some(class.binary.clone()),
                            _ => None,
                        };
                        if !name.is_empty() {
                            let binary = match &outer {
                                Some(outer) => format!("{outer}${name}"),
                                None => qualify(&file.package, &name),
                            };
                            let abstract_class = modifiers.iter().any(|m| m == "abstract" || m == "sealed");
                            let test_class = annotations.iter().any(|a| a == "Test") && testng;
                            pending = Some(Pending {
                                class: Class {
                                    binary,
                                    outer_at: match &current {
                                        Some(Frame::Class(class)) => Some(class.name_at),
                                        _ => None,
                                    },
                                    outer,
                                    object: text_word == "object",
                                    companion,
                                    runnable: text_word != "interface" && !abstract_class,
                                    name_at,
                                    index: file.classes.len(),
                                    all_methods: test_class,
                                },
                            });
                            file.classes.push(JvmClass {
                                name: pending.as_ref().map(|p| p.class.binary.clone()).unwrap_or_default(),
                                start: statement_start.unwrap_or(start),
                                end: end.max(name_at),
                            });
                        }
                        annotations.clear();
                        modifiers.clear();
                        index += if named.is_empty() { 1 } else { 2 };
                        continue;
                    }
                    "fun" if word(index + 1) == "interface" => {
                        index += 1;
                        continue;
                    }
                    "fun" => {
                        seen_declaration = true;
                        let mut at = index + 1;
                        let mut name = (String::new(), start);
                        let mut receiver = false;
                        while let Some(token) = kind(at) {
                            match token {
                                Tok::Word => name = (word(at).to_owned(), tokens[at].1),
                                Tok::Other if slice(at) == "." => receiver = true,
                                Tok::Other if slice(at) == "<" => {
                                    let mut depth = 0usize;
                                    while let Some(token) = kind(at) {
                                        match slice(at) {
                                            "<" => depth += 1,
                                            ">" => {
                                                depth -= 1;
                                                if depth == 0 {
                                                    break;
                                                }
                                            }
                                            _ if matches!(token, Tok::Open | Tok::End) => break,
                                            _ => {}
                                        }
                                        at += 1;
                                    }
                                }
                                Tok::Paren if slice(at) == "(" => break,
                                Tok::Open | Tok::End | Tok::Assign | Tok::Close => break,
                                _ => {}
                            }
                            at += 1;
                        }
                        if kind(at) != Some(Tok::Paren) {
                            annotations.clear();
                            modifiers.clear();
                            index = at;
                            continue;
                        }
                        let close = skip_group(at);
                        let params: Vec<&str> = (at + 1..close.saturating_sub(1)).map(slice).collect();
                        let private = modifiers.iter().any(|m| m == "private");
                        let mut main = None;
                        if name.0 == "main" && !receiver && !private {
                            match &current {
                                None if main_parameters(&params, false) => {
                                    main =
                                        Some(JvmMain { class: String::new(), owner: None, start: name.1, end: name.1 });
                                }
                                Some(Frame::Class(class))
                                    if class.object
                                        && annotations.iter().any(|a| a == "JvmStatic")
                                        && main_parameters(&params, true) =>
                                {
                                    let target =
                                        if class.companion { class.outer.clone() } else { Some(class.binary.clone()) };
                                    if let Some(target) = target {
                                        main = Some(JvmMain {
                                            class: target,
                                            owner: Some(if class.companion {
                                                class.outer_at.unwrap_or(class.name_at)
                                            } else {
                                                class.name_at
                                            }),
                                            start: name.1,
                                            end: name.1,
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                        let main = main.map(|main| {
                            let at = file.mains.len();
                            if !file.mains.iter().any(|known| known.class == main.class) {
                                file.mains.push(main);
                            }
                            at
                        });
                        if let Some(Frame::Class(class)) = &current
                            && !private
                            && !class.object
                            && class.runnable
                            && (annotations.iter().any(|a| TEST_ANNOTATIONS.contains(&a.as_str())) || class.all_methods)
                        {
                            let binary = class.binary.clone();
                            let class_start = class.name_at;
                            let test = match file.tests.iter_mut().position(|test| test.name == binary) {
                                Some(found) => found,
                                None => {
                                    file.tests.push(JvmTest {
                                        name: binary.clone(),
                                        start: class_start,
                                        testng,
                                        methods: Vec::new(),
                                    });
                                    file.tests.len() - 1
                                }
                            };
                            if !file.tests[test].methods.iter().any(|(known, _)| *known == name.0) {
                                file.tests[test].methods.push((name.0.clone(), name.1));
                            }
                        }
                        annotations.clear();
                        modifiers.clear();
                        let mut next = close;
                        while let Some(token) = kind(next) {
                            match token {
                                Tok::Open => {
                                    stack.push(Frame::Code { main: main.filter(|at| *at < file.mains.len()) });
                                    next += 1;
                                    break;
                                }
                                Tok::Assign | Tok::End | Tok::Close => break,
                                _ => next += 1,
                            }
                        }
                        statement_start = None;
                        index = next;
                        continue;
                    }
                    other if MODIFIERS.contains(&other) => {
                        statement_start.get_or_insert(start);
                        modifiers.push(other.to_owned());
                    }
                    "val" | "var" | "typealias" | "init" | "constructor" => {
                        seen_declaration = true;
                        annotations.clear();
                        modifiers.clear();
                    }
                    _ => {}
                }
                index += 1;
                continue;
            }
            Tok::Open => {
                match pending.take() {
                    Some(Pending { class }) if !in_code => stack.push(Frame::Class(class)),
                    _ => stack.push(Frame::Code { main: None }),
                }
                annotations.clear();
                modifiers.clear();
                statement_start = None;
            }
            Tok::Close => {
                match stack.pop() {
                    Some(Frame::Class(class)) => {
                        if let Some(known) = file.classes.get_mut(class.index) {
                            known.end = end;
                        }
                    }
                    Some(Frame::Code { main: Some(main) }) => {
                        if let Some(found) = file.mains.get_mut(main) {
                            found.end = start;
                        }
                    }
                    _ => {}
                }
                pending = None;
                annotations.clear();
                modifiers.clear();
                statement_start = None;
            }
            Tok::End => {
                let previous = index.checked_sub(1).map_or("", slice);
                if !matches!(previous, "," | ":") && word(index.wrapping_sub(1)) != "where" {
                    pending = None;
                }
                if annotations.is_empty() && modifiers.is_empty() {
                    statement_start = None;
                }
            }
            Tok::Assign => {
                pending = None;
                annotations.clear();
                modifiers.clear();
            }
            Tok::Paren if pending.is_some() => {
                index = skip_group(index);
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    file
}
