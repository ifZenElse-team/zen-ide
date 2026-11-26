use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{CommandError, Session};

use super::{LiveTemplates, Template, insert, is_word};
use crate::Workbench;
use crate::code_style;
use crate::language::{byte_offset, char_offset};

/// Which word of a parameter without a `:` is its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ParameterName {
    /// The first, as in Go's `count int`.
    First,
    /// The last, as in Java's `int count`.
    #[default]
    Last,
}

/// A documentation comment stub a language puts above a declaration when its opening is typed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocComment {
    /// The language id, e.g. `rust`.
    pub language: String,
    /// What starts the comment, e.g. `///` or `/**`.
    pub opening: String,
    /// Whether the stub comes on Enter after the opening rather than as soon as the opening is typed.
    #[serde(default)]
    pub on_enter: bool,
    /// The words a declaration starts with, after its modifiers; its name follows, e.g. `fn`, `struct`.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Whether a line with `name(` and no keyword declares something too, as a method does.
    #[serde(default)]
    pub calls: bool,
    /// Whether a line `Type name;` or `Type name = value` with no keyword declares a field.
    #[serde(default)]
    pub fields: bool,
    /// Line starts passed over between the comment and the declaration, e.g. `#[` or `@`.
    #[serde(default)]
    pub skip: Vec<String>,
    /// Which word of a parameter without `:` is its name.
    #[serde(default)]
    pub parameter_name: ParameterName,
    /// Whether the result type comes before the name, as in Java, rather than after the parameters.
    #[serde(default)]
    pub result_before: bool,
    /// Words that mean there is no result where a result type would be, e.g. `void`, `()`, `public`.
    #[serde(default)]
    pub no_result: Vec<String>,
    /// Only a result type that holds this counts, e.g. `Result`.
    #[serde(default)]
    pub result_contains: String,
    /// The stub as a live template with `$END$`: `$NAME$` is the declared name, a line that is only
    /// `$PARAMETERS$` becomes a `parameter` line per parameter, a line that is only `$RESULT$` becomes
    /// `result` when the declaration has a result; such a line is left out otherwise.
    pub text: String,
    /// A parameter's line, `$PARAMETER$` being its name, e.g. ` * @param $PARAMETER$`.
    #[serde(default)]
    pub parameter: String,
    /// The result's lines, e.g. ` * @return`.
    #[serde(default)]
    pub result: String,
    /// The word after the parameters that lists what the declaration throws, e.g. `throws`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub throws: String,
    /// The start of an annotation line above the declaration that lists what it throws, e.g. `@Throws(`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub throws_annotation: String,
    /// The line of each thrown type, `$EXCEPTION$` being its name, e.g. ` * @throws $EXCEPTION$`; a line
    /// of the text that is only `$THROWS$` becomes one per thrown type.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thrown: String,
}

impl DocComment {
    pub fn parse_list(json: &str) -> Result<Vec<DocComment>, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub parameters: Vec<String>,
    pub result: Option<String>,
    pub thrown: Vec<String>,
}

fn group_end(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, c) in text[open..].char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + at + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn angle_end(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, c) in text[open..].char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + at + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn type_before(text: &str) -> &str {
    let text = text.trim_end();
    let mut depth = 0i32;
    for (at, c) in text.char_indices().rev() {
        match c {
            '>' => depth += 1,
            '<' => depth -= 1,
            c if c.is_whitespace() && depth <= 0 => return &text[at + c.len_utf8()..],
            _ => {}
        }
    }
    text
}

fn thrown_types(list: &str) -> Vec<String> {
    split_top(list)
        .into_iter()
        .map(|thrown| thrown.trim_end_matches("::class").trim_end_matches(".class").trim().to_owned())
        .filter(|thrown| !thrown.is_empty())
        .collect()
}

fn head_of(text: &str) -> &str {
    let mut depth = 0i32;
    for (at, c) in text.char_indices() {
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' if depth > 0 => depth -= 1,
            '{' | ';' if depth == 0 => return &text[..at],
            '=' if depth == 0 && !text[at + 1..].starts_with('>') => return &text[..at],
            _ => {}
        }
    }
    text
}

fn words_with_places(text: &str) -> Vec<(usize, &str)> {
    let mut words = Vec::new();
    let mut start = None;
    for (at, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (is_word(c), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                words.push((from, &text[from..at]));
                start = None;
            }
            _ => {}
        }
    }
    words
}

fn split_top(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut from = 0;
    for (at, c) in text.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[from..at]);
                from = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[from..]);
    parts.into_iter().map(str::trim).filter(|part| !part.is_empty()).collect()
}

fn parameter_name(parameter: &str, rule: ParameterName) -> Option<String> {
    let colon = parameter
        .char_indices()
        .find(|(at, c)| *c == ':' && !parameter[at + 1..].starts_with(':') && !parameter[..*at].ends_with(':'));
    let words = words_with_places(parameter);
    let name = match colon {
        Some((at, _)) => words.iter().rev().find(|(start, _)| *start < at).map(|(_, word)| *word),
        None if rule == ParameterName::First => words.first().map(|(_, word)| *word),
        None => words.last().map(|(_, word)| *word),
    }?;
    let receiver = words.last().is_some_and(|(_, word)| *word == "self" || *word == "this") && colon.is_none();
    (!receiver && name != "self" && name != "this").then(|| name.to_owned())
}

pub fn declaration(after: &str, stub: &DocComment) -> Option<Declaration> {
    let mut offset = 0;
    let mut thrown = Vec::new();
    for line in after.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() && offset == 0 && line.ends_with('\n') && line.len() <= 2 {
            offset += line.len();
            continue;
        }
        if stub.skip.iter().any(|prefix| trimmed.starts_with(prefix.as_str())) {
            if let Some(list) =
                trimmed.strip_prefix(stub.throws_annotation.as_str()).filter(|_| !stub.throws_annotation.is_empty())
            {
                let list = list.rsplit_once(')').map_or(list, |(inside, _)| inside);
                thrown.extend(thrown_types(list));
            }
            offset += line.len();
            continue;
        }
        break;
    }
    let start = after[offset..].trim_start_matches([' ', '\t']);
    if start.starts_with("//") || start.starts_with("/*") || start.trim().is_empty() {
        return None;
    }
    let head = head_of(&start[..start.len().min(4096)]);
    let words = words_with_places(head);
    let is_keyword = |word: &str| stub.keywords.iter().any(|keyword| keyword == word);
    let keyword = (0..words.len())
        .find(|index| is_keyword(words[*index].1) && words.get(index + 1).is_none_or(|(_, next)| !is_keyword(next)));
    let (name_at, name) = match keyword {
        Some(index) => {
            let (at, word) = words[index];
            let mut rest = at + word.len();
            let after_keyword = head[rest..].trim_start();
            if after_keyword.starts_with('(') {
                let open = rest + head[rest..].find('(')?;
                rest = group_end(head, open)?;
            } else if after_keyword.starts_with('<') {
                let open = rest + head[rest..].find('<')?;
                rest = angle_end(head, open)?;
            }
            let mut found = words.iter().find(|(start, _)| *start >= rest).map(|(start, word)| (*start, *word))?;
            loop {
                let mut end = found.0 + found.1.len();
                if head[end..].starts_with('<') {
                    end = angle_end(head, end).unwrap_or(end);
                }
                if !head[end..].starts_with('.') {
                    break;
                }
                match words.iter().find(|(start, _)| *start == end + 1) {
                    Some((start, word)) => found = (*start, *word),
                    None => break,
                }
            }
            found
        }
        None if stub.calls && head.contains('(') => {
            let open = head.find('(')?;
            words.iter().rev().find(|(start, word)| start + word.len() <= open).map(|(start, word)| (*start, *word))?
        }
        None if stub.fields && words.len() >= 2 && !head.contains('(') => {
            let (at, word) = *words.last()?;
            if !head[..at].ends_with(char::is_whitespace) {
                return None;
            }
            (at, word)
        }
        None => return None,
    };
    let after_name = name_at + name.len();
    let open = head[after_name..].find('(').map(|at| after_name + at);
    let (parameters, close) = match open {
        Some(open) if !head[after_name..open].contains(['{', ';']) => {
            let close = group_end(head, open);
            let inner = close.map_or(&head[open + 1..], |close| &head[open + 1..close - 1]);
            let close = close.unwrap_or(head.len());
            let names = split_top(inner).into_iter().filter_map(|part| parameter_name(part, stub.parameter_name));
            (names.collect(), close)
        }
        _ => (Vec::new(), after_name),
    };
    let result = if stub.result_before {
        let token = type_before(&head[..name_at]);
        (!token.is_empty() && keyword.is_none() && open.is_some()).then(|| token.to_owned())
    } else {
        let tail = head[close..].trim();
        let tail = tail.strip_prefix("->").or_else(|| tail.strip_prefix(':')).unwrap_or(tail).trim();
        let tail = tail.split(" where").next().unwrap_or(tail).trim();
        (!tail.is_empty() && open.is_some()).then(|| tail.to_owned())
    };
    let result = result
        .filter(|result| !stub.no_result.iter().any(|none| none == result))
        .filter(|result| stub.result_contains.is_empty() || result.contains(stub.result_contains.as_str()));
    if !stub.throws.is_empty() && open.is_some() {
        let tail = &head[close..];
        if let Some((at, word)) = words_with_places(tail).into_iter().find(|(_, word)| *word == stub.throws) {
            thrown.extend(thrown_types(&tail[at + word.len()..]));
        }
    }
    Some(Declaration { name: name.to_owned(), parameters, result, thrown })
}

pub fn stub_text(stub: &DocComment, declaration: &Declaration) -> String {
    let escape = |text: &str| text.replace('$', "$$");
    let mut lines: Vec<String> = Vec::new();
    for line in stub.text.split('\n') {
        match line.trim() {
            "$PARAMETERS$" => lines.extend(
                declaration
                    .parameters
                    .iter()
                    .map(|parameter| stub.parameter.replace("$PARAMETER$", &escape(parameter))),
            ),
            "$RESULT$" => lines.extend(declaration.result.as_ref().map(|_| stub.result.clone())),
            "$THROWS$" => lines
                .extend(declaration.thrown.iter().map(|thrown| stub.thrown.replace("$EXCEPTION$", &escape(thrown)))),
            _ => lines.push(line.replace("$NAME$", &escape(&declaration.name))),
        }
    }
    lines.join("\n")
}

fn stub_at(session: &Session, on_enter: bool) -> Option<(std::path::PathBuf, std::ops::Range<usize>, Template)> {
    let document = session.state::<Workbench>()?.active_document()?;
    let selections = document.selections();
    let primary = selections.primary();
    if selections.len() != 1 || primary.anchor != primary.head || document.image().is_some() {
        return None;
    }
    let language = code_style::language_of(document.path());
    let stubs = session.state::<LiveTemplates>()?.doc_comments_for(&language);
    let text = document.text();
    let caret = byte_offset(text, primary.head);
    let line_start = text[..caret].rfind('\n').map_or(0, |at| at + 1);
    let line_end = text[caret..].find('\n').map_or(text.len(), |at| caret + at);
    if !text[caret..line_end].trim().is_empty() {
        return None;
    }
    let typed = text[line_start..caret].trim_start();
    let stub = stubs.into_iter().find(|stub| stub.on_enter == on_enter && typed == stub.opening)?;
    let previous = text[..line_start.saturating_sub(1)].rsplit('\n').next().unwrap_or_default().trim_start();
    let lead: String = stub.opening.chars().take(2).collect();
    if line_start > 0 && previous.starts_with(lead.as_str()) {
        return None;
    }
    let declared = declaration(&text[line_end..], &stub)?;
    let start = caret - typed.len();
    let template = Template {
        language,
        abbreviation: stub.opening.clone(),
        text: stub_text(&stub, &declared),
        ..Template::default()
    };
    Some((document.path().to_owned(), char_offset(text, start)..char_offset(text, caret), template))
}

pub(crate) fn after_typing(session: &mut Session) -> Result<bool, CommandError> {
    let Some((path, range, template)) = stub_at(session, false) else { return Ok(false) };
    insert(session, Some(&path), range, &template)?;
    Ok(true)
}

pub(crate) fn on_enter(session: &mut Session) -> Result<bool, CommandError> {
    let Some((path, range, template)) = stub_at(session, true) else { return Ok(false) };
    insert(session, Some(&path), range, &template)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust() -> DocComment {
        DocComment {
            language: "rust".to_owned(),
            opening: "///".to_owned(),
            on_enter: false,
            keywords: ["fn", "struct", "enum", "trait"].map(str::to_owned).to_vec(),
            calls: false,
            skip: vec!["#[".to_owned()],
            parameter_name: ParameterName::Last,
            result_before: false,
            no_result: vec!["()".to_owned()],
            result_contains: "Result".to_owned(),
            text: "/// $END$\n$RESULT$".to_owned(),
            parameter: String::new(),
            result: "///\n/// # Errors\n///\n/// ".to_owned(),
            ..DocComment::default()
        }
    }

    fn java() -> DocComment {
        DocComment {
            language: "java".to_owned(),
            opening: "/**".to_owned(),
            on_enter: true,
            keywords: ["class", "interface", "enum", "record"].map(str::to_owned).to_vec(),
            calls: true,
            skip: vec!["@".to_owned()],
            parameter_name: ParameterName::Last,
            result_before: true,
            no_result: ["void", "public", "private", "protected", "static", "final"].map(str::to_owned).to_vec(),
            result_contains: String::new(),
            text: "/**\n * $END$\n$PARAMETERS$\n$RESULT$\n */".to_owned(),
            parameter: " * @param $PARAMETER$".to_owned(),
            result: " * @return".to_owned(),
            ..DocComment::default()
        }
    }

    #[test]
    fn the_declaration_below_gives_its_name_parameters_and_result() {
        let found =
            declaration("\n#[inline]\npub(crate) async fn load(&mut self, path: &Path) -> io::Result<u8> {", &rust());
        assert_eq!(
            found,
            Some(Declaration {
                name: "load".to_owned(),
                parameters: vec!["path".to_owned()],
                result: Some("io::Result<u8>".to_owned()),
                thrown: Vec::new(),
            })
        );
        assert_eq!(declaration("\nfn f() {}", &rust()).unwrap().result, None);
        assert_eq!(declaration("\npub struct Store<T> {", &rust()).unwrap().name, "Store");
        let constant = DocComment { keywords: vec!["const".to_owned(), "fn".to_owned()], ..rust() };
        assert_eq!(declaration("\npub const fn size() -> usize {", &constant).unwrap().name, "size");
        assert_eq!(declaration("\nlet x = 1;", &rust()), None);
        assert_eq!(declaration("\n/// already\nfn f() {}", &rust()), None);
        let method = declaration("\n  @Override\n  public List<String> names(int count, String... rest) {", &java());
        assert_eq!(
            method.map(|found| (found.name, found.parameters, found.result)),
            Some(("names".to_owned(), vec!["count".to_owned(), "rest".to_owned()], Some("List<String>".to_owned())))
        );
        assert_eq!(declaration("\npublic Point(int x) {", &java()).unwrap().result, None, "a constructor");
        let go = DocComment {
            keywords: vec!["func".to_owned(), "type".to_owned()],
            parameter_name: ParameterName::First,
            ..rust()
        };
        let found = declaration("\nfunc (s *Server) Serve(a, b int, rest ...string) error {", &go).unwrap();
        assert_eq!(
            (found.name.as_str(), found.parameters),
            ("Serve", vec!["a".to_owned(), "b".to_owned(), "rest".to_owned()])
        );
    }

    #[test]
    fn the_stub_has_a_line_per_parameter_and_the_result_only_when_there_is_one() {
        let method = Declaration {
            name: "names".to_owned(),
            parameters: vec!["count".to_owned(), "re$t".to_owned()],
            result: Some("int".to_owned()),
            thrown: Vec::new(),
        };
        assert_eq!(stub_text(&java(), &method), "/**\n * $END$\n * @param count\n * @param re$$t\n * @return\n */");
        let plain = Declaration { name: "f".to_owned(), parameters: Vec::new(), result: None, thrown: Vec::new() };
        assert_eq!(stub_text(&java(), &plain), "/**\n * $END$\n */");
        assert_eq!(stub_text(&rust(), &plain), "/// $END$");
    }

    fn kotlin() -> DocComment {
        DocComment {
            language: "kotlin".to_owned(),
            opening: "/**".to_owned(),
            on_enter: true,
            keywords: ["fun", "class", "interface", "object", "val", "var", "typealias"].map(str::to_owned).to_vec(),
            skip: vec!["@".to_owned()],
            no_result: vec!["Unit".to_owned()],
            throws_annotation: "@Throws(".to_owned(),
            ..DocComment::default()
        }
    }

    #[test]
    fn a_java_declaration_gives_what_it_throws_and_a_generic_result() {
        let thrower = DocComment { throws: "throws".to_owned(), fields: true, ..java() };
        let found = declaration(
            "\n    public Map<String, List<Integer>> read(Path path) throws IOException, InterruptedException {",
            &thrower,
        )
        .unwrap();
        assert_eq!(found.name, "read");
        assert_eq!(found.result.as_deref(), Some("Map<String, List<Integer>>"));
        assert_eq!(found.thrown, ["IOException", "InterruptedException"]);
        let generic = declaration("\npublic static <T> T first(List<T> items) {", &thrower).unwrap();
        assert_eq!((generic.name.as_str(), generic.result.as_deref()), ("first", Some("T")));
        let field = declaration("\n    private final List<String> names = new ArrayList<>();", &thrower).unwrap();
        assert_eq!((field.name.as_str(), field.result, field.parameters.len()), ("names", None, 0));
        assert_eq!(declaration("\nprivate int count;", &java()), None, "fields only when the language says so");
        assert_eq!(declaration("\n    count = 1;", &thrower), None, "an assignment is not a field");
        let record = declaration("\npublic record Point(int x, int y) {", &thrower).unwrap();
        assert_eq!((record.name.as_str(), record.parameters), ("Point", vec!["x".to_owned(), "y".to_owned()]));
    }

    #[test]
    fn a_kotlin_declaration_gives_its_name_past_type_parameters_and_receivers() {
        let found = declaration(
            "\n@Throws(IOException::class, TimeoutException::class)\nfun <T> List<T>.second(index: Int): T? {",
            &kotlin(),
        )
        .unwrap();
        assert_eq!(found.name, "second");
        assert_eq!(found.parameters, ["index"]);
        assert_eq!(found.result.as_deref(), Some("T?"));
        assert_eq!(found.thrown, ["IOException", "TimeoutException"]);
        assert_eq!(declaration("\nfun run() {", &kotlin()).unwrap().result, None);
        assert_eq!(declaration("\noverride fun close(): Unit {", &kotlin()).unwrap().result, None);
        let class = declaration("\ndata class Box<T>(val value: T, private var count: Int = 0) {", &kotlin()).unwrap();
        assert_eq!((class.name.as_str(), class.parameters), ("Box", vec!["value".to_owned(), "count".to_owned()]));
        assert_eq!(declaration("\nval size: Int get() = 1", &kotlin()).unwrap().name, "size");
    }

    #[test]
    fn the_stub_has_a_line_per_thrown_type() {
        let stub = DocComment {
            text: "/**\n * $END$\n$PARAMETERS$\n$RESULT$\n$THROWS$\n */".to_owned(),
            thrown: " * @throws $EXCEPTION$".to_owned(),
            ..java()
        };
        let read = Declaration {
            name: "read".to_owned(),
            parameters: vec!["path".to_owned()],
            result: Some("String".to_owned()),
            thrown: vec!["IOException".to_owned()],
        };
        assert_eq!(stub_text(&stub, &read), "/**\n * $END$\n * @param path\n * @return\n * @throws IOException\n */");
    }
}
