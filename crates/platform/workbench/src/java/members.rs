use super::scan::{Token, tokens};

const MODIFIERS: &[&str] = &[
    "public",
    "protected",
    "private",
    "static",
    "final",
    "abstract",
    "default",
    "synchronized",
    "native",
    "transient",
    "volatile",
    "strictfp",
    "sealed",
];
const TYPE_KEYWORDS: &[&str] = &["class", "interface", "enum", "record"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemberKind {
    Field,
    Method,
    Constructor,
    Type,
    Initializer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parameter {
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) name_at: usize,
    pub(crate) start: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Member {
    pub(crate) kind: MemberKind,
    pub(crate) name: String,
    pub(crate) names: Vec<String>,
    pub(crate) first: usize,
    pub(crate) end: usize,
    pub(crate) name_at: usize,
    pub(crate) modifiers: Vec<(String, usize)>,
    pub(crate) head: usize,
    pub(crate) kind_text: String,
    pub(crate) parameters: Vec<Parameter>,
    pub(crate) parens: Option<(usize, usize)>,
    pub(crate) body: Option<(usize, usize)>,
}

impl Member {
    pub(crate) fn has(&self, modifier: &str) -> bool {
        self.modifiers.iter().any(|(name, _)| name == modifier)
    }

    pub(crate) fn visibility(&self) -> Option<(&str, usize)> {
        self.modifiers
            .iter()
            .find(|(name, _)| matches!(name.as_str(), "public" | "protected" | "private"))
            .map(|(name, at)| (name.as_str(), *at))
    }

    pub(crate) fn signature(&self) -> String {
        match self.kind {
            MemberKind::Method | MemberKind::Constructor => {
                let types: Vec<&str> = self.parameters.iter().map(|parameter| parameter.kind.as_str()).collect();
                format!("{}({})", self.name, types.join(", "))
            }
            _ => self.name.clone(),
        }
    }

    pub(crate) fn label(&self) -> String {
        match self.kind {
            MemberKind::Method => format!("{}: {}", self.signature(), self.kind_text),
            MemberKind::Field => format!("{}: {}", self.names.join(", "), self.kind_text),
            _ => self.signature(),
        }
    }

    pub(crate) fn same(&self, other: &Member) -> bool {
        match (self.kind, other.kind) {
            (MemberKind::Method, MemberKind::Method) => {
                self.name == other.name
                    && self.parameters.len() == other.parameters.len()
                    && self.parameters.iter().zip(&other.parameters).all(|(a, b)| erased(&a.kind) == erased(&b.kind))
            }
            (MemberKind::Field, MemberKind::Field) => self.names.iter().any(|name| other.names.contains(name)),
            (MemberKind::Type, MemberKind::Type) => self.name == other.name,
            _ => false,
        }
    }
}

fn erased(kind: &str) -> String {
    let mut depth = 0usize;
    let mut out = String::new();
    for c in kind.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 && !c.is_whitespace() => out.push(c),
            _ => {}
        }
    }
    out.rsplit('.').next().unwrap_or_default().to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TypeName {
    pub(crate) name: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClassDecl {
    pub(crate) keyword: String,
    pub(crate) keyword_at: usize,
    pub(crate) name: String,
    pub(crate) name_at: usize,
    pub(crate) modifiers: Vec<(String, usize)>,
    pub(crate) after_name: usize,
    pub(crate) extends: Vec<TypeName>,
    pub(crate) implements: Vec<TypeName>,
    pub(crate) open: usize,
    pub(crate) close: usize,
    pub(crate) members: Vec<Member>,
}

impl ClassDecl {
    pub(crate) fn has(&self, modifier: &str) -> bool {
        self.modifiers.iter().any(|(name, _)| name == modifier)
    }

    pub(crate) fn is_class(&self) -> bool {
        self.keyword == "class"
    }

    pub(crate) fn is_interface(&self) -> bool {
        self.keyword == "interface"
    }

    pub(crate) fn contains(&self, at: usize) -> bool {
        let start = self.modifiers.first().map_or(self.keyword_at, |(_, at)| *at);
        start <= at && at <= self.close
    }

    pub(crate) fn member_at(&self, at: usize) -> Option<&Member> {
        self.members.iter().find(|member| member.first <= at && at <= member.end)
    }
}

fn offset(text: &str, token: &Token<'_>) -> usize {
    token.text.as_ptr() as usize - text.as_ptr() as usize
}

fn is_word(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
}

fn matching(tokens: &[Token<'_>], open: usize, up: &str, down: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.text == up {
            depth += 1;
        } else if token.text == down {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn skip_annotation(tokens: &[Token<'_>], at: usize) -> usize {
    let mut at = at + 2;
    while tokens.get(at).is_some_and(|token| token.text == ".") && tokens.get(at + 1).is_some_and(|t| is_word(t.text)) {
        at += 2;
    }
    if tokens.get(at).is_some_and(|token| token.text == "(") {
        return matching(tokens, at, "(", ")").map_or(tokens.len(), |close| close + 1);
    }
    at
}

fn skip_angles(tokens: &[Token<'_>], at: usize) -> usize {
    if tokens.get(at).is_none_or(|token| token.text != "<") {
        return at;
    }
    matching(tokens, at, "<", ">").map_or(tokens.len(), |close| close + 1)
}

pub(crate) fn classes(text: &str) -> Vec<ClassDecl> {
    let tokens = tokens(text);
    let mut found = Vec::new();
    for index in 0..tokens.len() {
        if let Some(class) = class_from(text, &tokens, index) {
            found.push(class);
        }
    }
    found
}

pub(crate) fn class_at(text: &str, at: usize) -> Option<ClassDecl> {
    classes(text).into_iter().filter(|class| class.contains(at)).min_by_key(|class| class.close - class.keyword_at)
}

#[cfg(test)]
pub(crate) fn class_named(text: &str, name: &str) -> Option<ClassDecl> {
    classes(text).into_iter().find(|class| class.name == name)
}

fn class_from(text: &str, tokens: &[Token<'_>], index: usize) -> Option<ClassDecl> {
    let keyword = tokens[index].text;
    if !TYPE_KEYWORDS.contains(&keyword) {
        return None;
    }
    let before = index.checked_sub(1).map(|at| tokens[at].text);
    if matches!(before, Some(".") | Some("@")) {
        return None;
    }
    let name = tokens.get(index + 1).filter(|token| is_word(token.text))?;
    let after = skip_angles(tokens, index + 2);
    if keyword == "record" && tokens.get(after).is_none_or(|token| token.text != "(") {
        return None;
    }
    let mut at = after;
    let mut extends = Vec::new();
    let mut implements = Vec::new();
    let mut list: Option<&mut Vec<TypeName>> = None;
    let mut depth = 0usize;
    let open = loop {
        let token = tokens.get(at)?;
        match token.text {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            "{" if depth == 0 => break at,
            ";" if depth == 0 => return None,
            "extends" if depth == 0 => list = Some(&mut extends),
            "implements" if depth == 0 => list = Some(&mut implements),
            "permits" if depth == 0 => list = None,
            word if depth == 0 && is_word(word) => {
                if let Some(names) = list.as_deref_mut() {
                    let start = offset(text, token);
                    let mut end_token = at;
                    while tokens.get(end_token + 1).is_some_and(|next| next.text == ".")
                        && tokens.get(end_token + 2).is_some_and(|next| is_word(next.text))
                    {
                        end_token += 2;
                    }
                    let skipped = skip_angles(tokens, end_token + 1);
                    let end_byte = tokens.get(skipped - 1).map_or(start, |last| offset(text, last) + last.text.len());
                    let simple = tokens[end_token].text.to_owned();
                    names.push(TypeName { name: simple, start, end: end_byte });
                    at = skipped;
                    continue;
                }
            }
            _ => {}
        }
        at += 1;
    };
    let close = matching(tokens, open, "{", "}")?;
    let mut modifiers = Vec::new();
    let mut back = index;
    while back > 0 && MODIFIERS.contains(&tokens[back - 1].text) {
        back -= 1;
        modifiers.push((tokens[back].text.to_owned(), offset(text, &tokens[back])));
    }
    modifiers.reverse();
    let after_name = tokens.get(after - 1).map_or(0, |last| offset(text, last) + last.text.len());
    let members = members(text, tokens, open, close, keyword == "enum", name.text);
    Some(ClassDecl {
        keyword: keyword.to_owned(),
        keyword_at: offset(text, &tokens[index]),
        name: name.text.to_owned(),
        name_at: offset(text, name),
        modifiers,
        after_name,
        extends,
        implements,
        open: offset(text, &tokens[open]),
        close: offset(text, &tokens[close]),
        members,
    })
}

fn members(text: &str, tokens: &[Token<'_>], open: usize, close: usize, enumeration: bool, class: &str) -> Vec<Member> {
    let mut found = Vec::new();
    let mut at = open + 1;
    if enumeration {
        let mut depth = 0usize;
        while at < close {
            match tokens[at].text {
                "(" | "{" => depth += 1,
                ")" | "}" => depth = depth.saturating_sub(1),
                ";" if depth == 0 => break,
                _ => {}
            }
            at += 1;
        }
        if at >= close {
            return found;
        }
        at += 1;
    }
    while at < close {
        if tokens[at].text == ";" {
            at += 1;
            continue;
        }
        let start = at;
        let mut assigned = false;
        let mut paren = 0usize;
        let mut end = None;
        while at < close {
            match tokens[at].text {
                "(" => paren += 1,
                ")" => paren = paren.saturating_sub(1),
                "=" if paren == 0 => assigned = true,
                ";" if paren == 0 => {
                    end = Some(at);
                    break;
                }
                "{" if paren == 0 => {
                    let Some(closing) = matching(tokens, at, "{", "}") else { return found };
                    if !assigned {
                        end = Some(closing);
                        break;
                    }
                    at = closing;
                }
                _ => {}
            }
            at += 1;
        }
        let Some(last) = end else { break };
        if let Some(member) = member(text, tokens, start, last, class) {
            found.push(member);
        }
        at = last + 1;
    }
    found
}

fn member(text: &str, tokens: &[Token<'_>], start: usize, last: usize, class: &str) -> Option<Member> {
    let end = offset(text, &tokens[last]) + 1;
    let first = offset(text, &tokens[start]);
    let mut at = start;
    let mut modifiers = Vec::new();
    loop {
        let token = tokens.get(at).filter(|_| at <= last)?;
        if token.text == "@" && tokens.get(at + 1).is_some_and(|next| next.text != "interface") {
            at = skip_annotation(tokens, at);
        } else if MODIFIERS.contains(&token.text) {
            modifiers.push((token.text.to_owned(), offset(text, token)));
            at += 1;
        } else if token.text == "non" && tokens.get(at + 1).is_some_and(|next| next.text == "-") {
            at += 3;
        } else {
            break;
        }
    }
    let head = offset(text, tokens.get(at)?);
    let blank = |kind: MemberKind, name: &str, name_at: usize| Member {
        kind,
        name: name.to_owned(),
        names: vec![name.to_owned()],
        first,
        end,
        name_at,
        modifiers: modifiers.clone(),
        head,
        kind_text: String::new(),
        parameters: Vec::new(),
        parens: None,
        body: None,
    };
    let token = &tokens[at];
    if token.text == "@" || TYPE_KEYWORDS.contains(&token.text) {
        let keyword = if token.text == "@" { at + 1 } else { at };
        let name = tokens.get(keyword + 1)?;
        return Some(blank(MemberKind::Type, name.text, offset(text, name)));
    }
    if token.text == "{" {
        return Some(blank(MemberKind::Initializer, "", head));
    }
    at = skip_angles(tokens, at);
    let word = tokens.get(at).filter(|token| is_word(token.text))?;
    if word.text == class && tokens.get(at + 1).is_some_and(|next| next.text == "(" || next.text == "{") {
        let mut constructor = blank(MemberKind::Constructor, word.text, offset(text, word));
        if tokens[at + 1].text == "(" {
            let close = matching(tokens, at + 1, "(", ")")?;
            constructor.parameters = parameters(text, tokens, at + 1, close);
            constructor.parens = Some((offset(text, &tokens[at + 1]), offset(text, &tokens[close])));
        }
        if tokens[last].text == "}" {
            let open = (at + 1..last).find(|index| tokens[*index].text == "{")?;
            constructor.body = Some((offset(text, &tokens[open]), end - 1));
        }
        return Some(constructor);
    }
    let type_start = offset(text, word);
    let mut name_index = at;
    loop {
        name_index += 1;
        name_index = skip_angles(tokens, name_index);
        match tokens.get(name_index).map(|token| token.text) {
            Some(".") if tokens.get(name_index + 1).is_some_and(|next| next.text == ".") => name_index += 2,
            Some(".") => {}
            Some("[") | Some("]") => {}
            Some("@") => name_index = skip_annotation(tokens, name_index) - 1,
            Some(word) if is_word(word) && tokens[name_index - 1].text != "." => break,
            Some(_) => {}
            None => return None,
        }
        if name_index >= last {
            return None;
        }
    }
    let name = &tokens[name_index];
    let name_at = offset(text, name);
    let kind_text = text[type_start..name_at].trim().to_owned();
    if tokens.get(name_index + 1).is_some_and(|next| next.text == "(") {
        let close = matching(tokens, name_index + 1, "(", ")")?;
        let mut method = blank(MemberKind::Method, name.text, name_at);
        method.kind_text = kind_text;
        method.parameters = parameters(text, tokens, name_index + 1, close);
        method.parens = Some((offset(text, &tokens[name_index + 1]), offset(text, &tokens[close])));
        if tokens[last].text == "}" {
            let open = (close..last).find(|index| tokens[*index].text == "{")?;
            method.body = Some((offset(text, &tokens[open]), end - 1));
        }
        return Some(method);
    }
    let mut field = blank(MemberKind::Field, name.text, name_at);
    field.kind_text = kind_text;
    let mut depth = 0usize;
    for index in name_index + 1..last {
        match tokens[index].text {
            "(" | "{" | "[" => depth += 1,
            ")" | "}" | "]" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                if let Some(next) = tokens.get(index + 1).filter(|next| is_word(next.text)) {
                    field.names.push(next.text.to_owned());
                }
            }
            _ => {}
        }
    }
    Some(field)
}

fn parameters(text: &str, tokens: &[Token<'_>], open: usize, close: usize) -> Vec<Parameter> {
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut from = open + 1;
    for index in open + 1..=close {
        let token = tokens[index].text;
        match token {
            "<" | "(" => depth += 1,
            ">" | ")" if index != close => depth = depth.saturating_sub(1),
            _ => {}
        }
        if (token == "," && depth == 0) || index == close {
            if index > from {
                let mut start = from;
                while start < index {
                    if tokens[start].text == "@" {
                        start = skip_annotation(tokens, start);
                    } else if tokens[start].text == "final" {
                        start += 1;
                    } else {
                        break;
                    }
                }
                let name = &tokens[index - 1];
                if start < index - 1 {
                    let begin = offset(text, &tokens[start]);
                    let name_at = offset(text, name);
                    found.push(Parameter {
                        kind: text[begin..name_at].trim().to_owned(),
                        name: name.text.to_owned(),
                        name_at,
                        start: offset(text, &tokens[from]),
                    });
                }
            }
            from = index + 1;
        }
    }
    found
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Word {
    pub(crate) text: String,
    pub(crate) at: usize,
    pub(crate) qualified: bool,
}

pub(crate) fn words(text: &str, from: usize, to: usize) -> Vec<Word> {
    let tokens = tokens(text);
    let mut found = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let at = offset(text, token);
        if at < from || at >= to || !is_word(token.text) {
            continue;
        }
        let previous = index.checked_sub(1).map(|before| tokens[before].text);
        let this = index >= 2 && previous == Some(".") && tokens[index - 2].text == "this";
        found.push(Word { text: token.text.to_owned(), at, qualified: previous == Some(".") && !this });
    }
    found
}

pub(crate) fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |index| index + 1)
}

pub(crate) fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |index| at + index)
}

pub(crate) fn indent_of(text: &str, at: usize) -> String {
    let start = line_start(text, at);
    text[start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

pub(crate) fn member_indent(text: &str, class: &ClassDecl) -> String {
    match class.members.first() {
        Some(member) => indent_of(text, member.first),
        None => {
            let outer = indent_of(text, class.keyword_at);
            let unit = if outer.starts_with('\t') { "\t" } else { "    " };
            format!("{outer}{unit}")
        }
    }
}

pub(crate) fn block(text: &str, member: &Member) -> (usize, usize) {
    let (from, _) = crate::language::safe_delete::deletion(text, member.first, member.end);
    let from = if text[from..member.first].trim().is_empty() { line_start(text, member.first) } else { from };
    let mut from = from;
    while from < member.first && text[from..].starts_with('\n') {
        from += 1;
    }
    let after = line_end(text, member.end);
    let to = if text[member.end..after].trim().is_empty() { (after + 1).min(text.len()) } else { member.end };
    (from, to)
}

pub(crate) fn removal(text: &str, member: &Member) -> (usize, usize) {
    let (from, to) = crate::language::safe_delete::deletion(text, member.first, member.end);
    let closing = text[to..].trim_start_matches([' ', '\t']).starts_with('}');
    if closing && from >= 1 && text[..from].ends_with('\n') {
        let above = line_start(text, from - 1);
        if text[above..from].trim().is_empty() && above > 0 {
            return (above, to);
        }
    }
    (from, to)
}

pub(crate) fn reindent(block: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    for line in block.split_inclusive('\n') {
        if line.trim().is_empty() {
            out.push_str(if line.ends_with('\n') { "\n" } else { "" });
        } else if let Some(rest) = line.strip_prefix(from) {
            out.push_str(to);
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub(crate) fn insertion(text: &str, class: &ClassDecl, members: &str) -> (usize, usize, String) {
    let inside = &text[class.open + 1..class.close];
    let close_indent = indent_of(text, class.close);
    if inside.trim().is_empty() {
        let tail = if text[line_start(text, class.close)..class.close].trim().is_empty() {
            close_indent
        } else {
            indent_of(text, class.keyword_at)
        };
        return (class.open + 1, class.close, format!("\n{members}{tail}"));
    }
    let start = line_start(text, class.close);
    if text[start..class.close].trim().is_empty() {
        let before = text[..start].trim_end_matches([' ', '\t']);
        let blank = before.ends_with("\n\n") || before.ends_with("{\n");
        let lead = if blank { "" } else { "\n" };
        (start, start, format!("{lead}{members}"))
    } else {
        (class.close, class.close, format!("\n{members}{close_indent}"))
    }
}

pub(crate) fn package(text: &str) -> String {
    let tokens = tokens(text);
    let Some(index) = tokens.iter().position(|token| token.text == "package") else { return String::new() };
    tokens[index + 1..].iter().take_while(|token| token.text != ";").map(|token| token.text).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Import {
    pub(crate) name: String,
    pub(crate) simple: String,
    pub(crate) statik: bool,
    pub(crate) end: usize,
}

pub(crate) fn imports(text: &str) -> Vec<Import> {
    let tokens = tokens(text);
    let mut found = Vec::new();
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.text {
            "{" => depth += 1,
            "}" => depth = depth.saturating_sub(1),
            "import" if depth == 0 => {
                let statik = tokens.get(index + 1).is_some_and(|next| next.text == "static");
                let from = index + 1 + usize::from(statik);
                let Some(semicolon) = tokens[from..].iter().position(|next| next.text == ";").map(|at| from + at)
                else {
                    continue;
                };
                let name: String = tokens[from..semicolon].iter().map(|next| next.text).collect();
                let simple = name.rsplit('.').next().unwrap_or_default().to_owned();
                found.push(Import { name, simple, statik, end: offset(text, &tokens[semicolon]) + 1 });
            }
            _ => {}
        }
    }
    found
}

pub(crate) fn needed_imports(source: &str, destination: &str, moved: &[&str]) -> Vec<String> {
    let have: Vec<Import> = imports(destination);
    let destination_package = package(destination);
    let mut needed = Vec::new();
    for import in imports(source) {
        if have.iter().any(|known| known.name == import.name && known.statik == import.statik) {
            continue;
        }
        if !import.statik && import.name.rsplit_once('.').is_some_and(|(owner, _)| owner == destination_package) {
            continue;
        }
        let used = import.simple == "*"
            || moved.iter().any(|text| tokens(text).iter().any(|token| token.text == import.simple));
        if used {
            let line = if import.statik {
                format!("import static {};", import.name)
            } else {
                format!("import {};", import.name)
            };
            if !needed.contains(&line) {
                needed.push(line);
            }
        }
    }
    needed
}

pub(crate) fn import_insertion(text: &str, lines: &[String]) -> Option<(usize, String)> {
    if lines.is_empty() {
        return None;
    }
    let block = lines.join("\n");
    if let Some(last) = imports(text).last() {
        return Some((last.end, format!("\n{block}")));
    }
    let tokens = tokens(text);
    if let Some(index) = tokens.iter().position(|token| token.text == "package")
        && let Some(semicolon) = tokens[index..].iter().position(|token| token.text == ";")
    {
        let end = offset(text, &tokens[index + semicolon]) + 1;
        return Some((end, format!("\n\n{block}")));
    }
    Some((0, format!("{block}\n\n")))
}

pub(crate) fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

pub(crate) fn split_arguments(text: &str, open: usize) -> Option<(usize, Vec<(usize, usize)>)> {
    let tokens = tokens(text);
    let index = tokens.iter().position(|token| offset(text, token) == open)?;
    let close = matching(&tokens, index, "(", ")")?;
    let close_at = offset(text, &tokens[close]);
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut from = open + 1;
    for token in &tokens[index + 1..close] {
        match token.text {
            "(" | "{" | "[" => depth += 1,
            ")" | "}" | "]" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                found.push((from, offset(text, token)));
                from = offset(text, token) + 1;
            }
            _ => {}
        }
    }
    if !text[from..close_at].trim().is_empty() || !found.is_empty() {
        found.push((from, close_at));
    }
    Some((close_at, found))
}

pub(crate) fn trimmed(text: &str, (from, to): (usize, usize)) -> (usize, usize) {
    let slice = &text[from..to];
    let start = from + (slice.len() - slice.trim_start().len());
    let end = to - (slice.len() - slice.trim_end().len());
    (start, end.max(start))
}

#[cfg(test)]
mod tests;
