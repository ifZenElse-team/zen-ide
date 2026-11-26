use crate::complete::clamp;
use crate::data::{BOOLEAN_ATTRIBUTES, ELEMENTS, INLINE, VOID};
use crate::lexer::{End, Tok, lex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    pub from: usize,
    pub to: usize,
    pub text: String,
}

const MAX_REPEAT: usize = 1000;
const MAX_ELEMENTS: usize = 10_000;
const MAX_ABBREVIATION: usize = 2000;
const MAX_NESTING: usize = 32;
const MAX_NUMBER_WIDTH: usize = 8;
const MAX_BASE: usize = 1_000_000;
const MAX_OUTPUT: usize = 1 << 20;
const SPOT: char = '\0';

pub fn expand(text: &str, caret: usize) -> Option<Expansion> {
    let caret = clamp(text, caret);
    let prefix = &text[..caret];
    let (tokens, end) = lex(prefix);
    if end != End::Data {
        return None;
    }
    let line_start = prefix.rfind('\n').map_or(0, |at| at + 1);
    let markup_end = tokens.iter().rev().find(|token| token.kind != Tok::Text).map_or(0, |token| token.end);
    let from = abbreviation_start(prefix, line_start.max(markup_end))?;
    let indent: String = prefix[line_start..].chars().take_while(|ch| matches!(ch, ' ' | '\t')).collect();
    let text = expand_abbreviation(&prefix[from..], &indent, &indent_unit(text))?;
    Some(Expansion { from, to: caret, text })
}

fn abbreviation_start(prefix: &str, bound: usize) -> Option<usize> {
    let bytes = prefix.as_bytes();
    let mut depth = 0usize;
    let mut start = prefix.len();
    while start > bound {
        let byte = bytes[start - 1];
        match byte {
            b']' | b'}' => depth += 1,
            b'[' | b'{' if depth == 0 => break,
            b'[' | b'{' => depth -= 1,
            _ if depth > 0 => {}
            _ if byte.is_ascii_alphanumeric() || b"-_:!#.>+^*$@()".contains(&byte) => {}
            _ => break,
        }
        start -= 1;
    }
    if depth > 0 {
        return None;
    }
    while start < prefix.len() && matches!(bytes[start], b'>' | b'+' | b'^') {
        start += 1;
    }
    (start < prefix.len()).then_some(start)
}

fn indent_unit(text: &str) -> String {
    let mut spaces: Option<usize> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            return "\t".to_owned();
        }
        let count = line.bytes().take_while(|&byte| byte == b' ').count();
        if count > 0 {
            spaces = Some(spaces.map_or(count, |spaces| spaces.min(count)));
        }
    }
    " ".repeat(spaces.unwrap_or(4).min(8))
}

const SKELETON: &[(usize, &str)] = &[
    (0, "<!doctype html>"),
    (0, "<html lang=\"en\">"),
    (0, "<head>"),
    (1, "<meta charset=\"UTF-8\">"),
    (1, "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">"),
    (1, "<title>Document</title>"),
    (0, "</head>"),
    (0, "<body>"),
    (1, "$0"),
    (0, "</body>"),
    (0, "</html>"),
];

pub fn expand_abbreviation(abbreviation: &str, indent: &str, unit: &str) -> Option<String> {
    if abbreviation.contains(SPOT) || abbreviation.len() > MAX_ABBREVIATION || too_deep(abbreviation) {
        return None;
    }
    if matches!(abbreviation, "!" | "html:5") {
        let lines: Vec<String> =
            SKELETON.iter().map(|(depth, line)| format!("{}{line}", unit.repeat(*depth))).collect();
        return Some(lines.join(&format!("\n{indent}")));
    }
    if abbreviation.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':' | b'_'))
        && !is_known(abbreviation)
    {
        return None;
    }
    let mut parser = Parser { chars: abbreviation.chars().collect(), pos: 0, nodes: vec![Node::default()] };
    parser.sequence(0)?;
    if parser.pos != parser.chars.len() {
        return None;
    }
    let mut renderer = Renderer { nodes: &parser.nodes, numbers: Vec::new(), budget: MAX_ELEMENTS };
    let mut out = Vec::new();
    for &child in &parser.nodes[0].children {
        renderer.render(child, "", &mut out)?;
    }
    let format = Format { indent, unit };
    if out.iter().all(|node| matches!(node, Out::Text(_))) {
        return None;
    }
    let mut html = String::new();
    format.siblings(&out, 0, &mut html);
    if html.len() > MAX_OUTPUT || html.contains("$0") {
        return None;
    }
    let html = match html.find(SPOT) {
        Some(at) => format!("{}$0{}", &html[..at], &html[at + SPOT.len_utf8()..]),
        None => html,
    };
    Some(html.replace(SPOT, ""))
}

fn too_deep(abbreviation: &str) -> bool {
    let mut groups = 0usize;
    let mut deepest = 0;
    let mut dollars = 0;
    for byte in abbreviation.bytes() {
        match byte {
            b'(' => groups += 1,
            b')' => groups = groups.saturating_sub(1),
            _ => {}
        }
        deepest = deepest.max(groups);
        dollars = if byte == b'$' { dollars + 1 } else { 0 };
        if dollars > MAX_NUMBER_WIDTH {
            return true;
        }
    }
    deepest > MAX_NESTING || abbreviation.bytes().filter(|&byte| byte == b'>').count() > MAX_NESTING
}

fn is_known(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    ELEMENTS.contains(&lower.as_str()) || alias(&lower).is_some()
}

type Attributes = Vec<(String, Option<String>)>;

fn attrs(pairs: &[(&str, &str)]) -> Attributes {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), Some((*value).to_owned()))).collect()
}

fn alias(name: &str) -> Option<(String, Attributes)> {
    let plain = |name: &str| Some((name.to_owned(), Vec::new()));
    match name {
        "a" => Some(("a".to_owned(), attrs(&[("href", "")]))),
        "a:link" => Some(("a".to_owned(), attrs(&[("href", "https://")]))),
        "a:mail" => Some(("a".to_owned(), attrs(&[("href", "mailto:")]))),
        "img" => Some(("img".to_owned(), attrs(&[("src", ""), ("alt", "")]))),
        "link" | "link:css" => Some(("link".to_owned(), attrs(&[("rel", "stylesheet"), ("href", "")]))),
        "link:favicon" => Some(("link".to_owned(), attrs(&[("rel", "icon"), ("href", "favicon.ico")]))),
        "script:src" => Some(("script".to_owned(), attrs(&[("src", "")]))),
        "script:module" => Some(("script".to_owned(), attrs(&[("type", "module"), ("src", "")]))),
        "meta:utf" => Some(("meta".to_owned(), attrs(&[("charset", "UTF-8")]))),
        "meta:vp" => Some((
            "meta".to_owned(),
            attrs(&[("name", "viewport"), ("content", "width=device-width, initial-scale=1.0")]),
        )),
        "input" => Some(("input".to_owned(), attrs(&[("type", "text")]))),
        "form" => Some(("form".to_owned(), attrs(&[("action", "")]))),
        "label" => Some(("label".to_owned(), attrs(&[("for", "")]))),
        "iframe" | "ifr" => Some(("iframe".to_owned(), attrs(&[("src", "")]))),
        "embed" => Some(("embed".to_owned(), attrs(&[("src", "")]))),
        "audio" | "video" => Some((name.to_owned(), attrs(&[("src", "")]))),
        "object" => Some(("object".to_owned(), attrs(&[("data", "")]))),
        "abbr" => Some(("abbr".to_owned(), attrs(&[("title", "")]))),
        "html" => Some(("html".to_owned(), attrs(&[("lang", "en")]))),
        "btn" => plain("button"),
        "btn:s" => Some(("button".to_owned(), attrs(&[("type", "submit")]))),
        "btn:r" => Some(("button".to_owned(), attrs(&[("type", "reset")]))),
        "btn:b" => Some(("button".to_owned(), attrs(&[("type", "button")]))),
        "bq" => plain("blockquote"),
        "fig" => plain("figure"),
        "figc" => plain("figcaption"),
        "pic" => plain("picture"),
        "str" => plain("strong"),
        "sect" => plain("section"),
        "art" => plain("article"),
        "hdr" => plain("header"),
        "ftr" => plain("footer"),
        "opt" => plain("option"),
        "tarea" => plain("textarea"),
        _ => {
            let kind = name.strip_prefix("input:")?;
            (!kind.is_empty()).then(|| ("input".to_owned(), attrs(&[("type", kind)])))
        }
    }
}

fn implicit_name(parent: &str) -> &'static str {
    match parent {
        "ul" | "ol" | "menu" => "li",
        "table" | "tbody" | "thead" | "tfoot" => "tr",
        "tr" => "td",
        "select" | "datalist" | "optgroup" => "option",
        "colgroup" => "col",
        "map" => "area",
        "audio" | "video" | "picture" => "source",
        parent if INLINE.contains(&parent) => "span",
        _ => "div",
    }
}

#[derive(Clone, Debug, Default)]
struct Spec {
    name: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attributes: Attributes,
    text: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Node {
    spec: Option<Spec>,
    repeat: usize,
    children: Vec<usize>,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    nodes: Vec<Node>,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn eat(&mut self, ch: char) -> bool {
        let found = self.peek() == Some(ch);
        if found {
            self.pos += 1;
        }
        found
    }

    fn sequence(&mut self, base: usize) -> Option<()> {
        let mut stack = vec![base];
        loop {
            let item = self.item()?;
            let parent = *stack.last()?;
            self.nodes[parent].children.push(item);
            match self.peek() {
                None | Some(')') => return Some(()),
                Some('>') => {
                    self.pos += 1;
                    let mut target = item;
                    while self.nodes[target].spec.is_none() {
                        target = *self.nodes[target].children.last()?;
                    }
                    stack.push(target);
                }
                Some('+') => self.pos += 1,
                Some('^') => {
                    while self.eat('^') {
                        if stack.len() > 1 {
                            stack.pop();
                        }
                    }
                }
                Some(_) => return None,
            }
        }
    }

    fn item(&mut self) -> Option<usize> {
        let index = self.nodes.len();
        self.nodes.push(Node::default());
        if self.eat('(') {
            self.sequence(index)?;
            if !self.eat(')') {
                return None;
            }
        } else {
            let spec = self.element()?;
            self.nodes[index].spec = Some(spec);
        }
        self.nodes[index].repeat = self.multiplier()?;
        Some(index)
    }

    fn multiplier(&mut self) -> Option<usize> {
        if !self.eat('*') {
            return Some(1);
        }
        let digits = self.word(|ch| ch.is_ascii_digit());
        let count: usize = digits.parse().ok()?;
        (1..=MAX_REPEAT).contains(&count).then_some(count)
    }

    fn name(&mut self) -> String {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | ':' | '_') {
                self.pos += 1;
            } else if ch == '$' {
                while self.eat('$') {}
                if self.eat('@') {
                    self.eat('-');
                    self.word(|ch| ch.is_ascii_digit());
                }
            } else {
                break;
            }
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn word(&mut self, accept: impl Fn(char) -> bool) -> String {
        let start = self.pos;
        while self.peek().is_some_and(&accept) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn element(&mut self) -> Option<Spec> {
        let mut spec = Spec::default();
        if self.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            spec.name = Some(self.name());
        }
        let mut any = spec.name.is_some();
        loop {
            match self.peek() {
                Some('#') => {
                    self.pos += 1;
                    let id = self.name();
                    if id.is_empty() {
                        return None;
                    }
                    spec.id = Some(id);
                }
                Some('.') => {
                    self.pos += 1;
                    let class = self.name();
                    if class.is_empty() {
                        return None;
                    }
                    spec.classes.push(class);
                }
                Some('[') => {
                    self.pos += 1;
                    self.attribute_list(&mut spec.attributes)?;
                }
                Some('{') => {
                    self.pos += 1;
                    let mut depth = 1;
                    let start = self.pos;
                    loop {
                        match self.peek()? {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                    let text: String = self.chars[start..self.pos].iter().collect();
                    self.pos += 1;
                    spec.text.get_or_insert_with(String::new).push_str(&text);
                }
                _ => break,
            }
            any = true;
        }
        any.then_some(spec)
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn attribute_list(&mut self, attributes: &mut Attributes) -> Option<()> {
        loop {
            self.skip_spaces();
            if self.eat(']') {
                return Some(());
            }
            let name = self.word(|ch| !ch.is_whitespace() && !matches!(ch, '=' | ']' | '"' | '\''));
            if name.is_empty() {
                return None;
            }
            let value = if self.eat('=') {
                match self.peek()? {
                    quote @ ('"' | '\'') => {
                        self.pos += 1;
                        let value = self.word(|ch| ch != quote);
                        if !self.eat(quote) {
                            return None;
                        }
                        Some(value)
                    }
                    _ => Some(self.word(|ch| !ch.is_whitespace() && ch != ']')),
                }
            } else {
                None
            };
            attributes.push((name, value));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Out {
    Element { name: String, attributes: Attributes, children: Vec<Out> },
    Text(String),
}

struct Renderer<'a> {
    nodes: &'a [Node],
    numbers: Vec<(usize, usize)>,
    budget: usize,
}

impl Renderer<'_> {
    fn number(&self, template: &str) -> String {
        let (index, count) = self.numbers.last().copied().unwrap_or((1, 1));
        let chars: Vec<char> = template.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && chars.get(i + 1) == Some(&'$') {
                out.push('$');
                i += 2;
                continue;
            }
            if chars[i] != '$' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let width = chars[i..].iter().take_while(|&&ch| ch == '$').count();
            i += width;
            let mut reverse = false;
            let mut base = 1;
            if chars.get(i) == Some(&'@') {
                i += 1;
                if chars.get(i) == Some(&'-') {
                    reverse = true;
                    i += 1;
                }
                let digits: String = chars[i..].iter().take_while(|ch| ch.is_ascii_digit()).collect();
                i += digits.len();
                base = digits.parse().ok().filter(|base| *base <= MAX_BASE).unwrap_or(1);
            }
            let value = if reverse { count - index + base } else { index + base - 1 };
            out.push_str(&format!("{value:0width$}"));
        }
        out
    }

    fn render(&mut self, index: usize, parent: &str, out: &mut Vec<Out>) -> Option<()> {
        let node = &self.nodes[index];
        for copy in 1..=node.repeat {
            self.budget = self.budget.checked_sub(1)?;
            if node.repeat > 1 {
                self.numbers.push((copy, node.repeat));
            }
            match &node.spec {
                None => {
                    for &child in &node.children {
                        self.render(child, parent, out)?;
                    }
                }
                Some(spec) => self.element(spec, &node.children, parent, out)?,
            }
            if node.repeat > 1 {
                self.numbers.pop();
            }
        }
        Some(())
    }

    fn element(&mut self, spec: &Spec, children: &[usize], parent: &str, out: &mut Vec<Out>) -> Option<()> {
        let text_only =
            spec.name.is_none() && spec.id.is_none() && spec.classes.is_empty() && spec.attributes.is_empty();
        if text_only {
            out.push(Out::Text(self.number(spec.text.as_deref().unwrap_or(""))));
            for &child in children {
                self.render(child, parent, out)?;
            }
            return Some(());
        }
        let written = spec.name.as_deref().map_or_else(|| implicit_name(parent).to_owned(), |name| self.number(name));
        let (name, mut attributes) = alias(&written.to_ascii_lowercase()).unwrap_or((written, Vec::new()));
        let mut set = |key: &str, value: Option<String>| match attributes.iter_mut().find(|(name, _)| name == key) {
            Some(slot) => slot.1 = value,
            None => attributes.push((key.to_owned(), value)),
        };
        if let Some(id) = &spec.id {
            set("id", Some(self.number(id)));
        }
        if !spec.classes.is_empty() {
            let classes: Vec<String> = spec.classes.iter().map(|class| self.number(class)).collect();
            set("class", Some(classes.join(" ")));
        }
        for (key, value) in &spec.attributes {
            set(key, value.as_deref().map(|value| self.number(value)));
        }
        let mut inner = Vec::new();
        if let Some(text) = &spec.text {
            inner.push(Out::Text(self.number(text)));
        }
        let lower = name.to_ascii_lowercase();
        for &child in children {
            self.render(child, &lower, &mut inner)?;
        }
        out.push(Out::Element { name, attributes, children: inner });
        Some(())
    }
}

struct Format<'a> {
    indent: &'a str,
    unit: &'a str,
}

fn is_inline(node: &Out) -> bool {
    match node {
        Out::Text(_) => true,
        Out::Element { name, .. } => INLINE.contains(&name.to_ascii_lowercase().as_str()),
    }
}

impl Format<'_> {
    fn newline(&self, depth: usize, html: &mut String) {
        html.push('\n');
        html.push_str(self.indent);
        for _ in 0..depth {
            html.push_str(self.unit);
        }
    }

    fn siblings(&self, nodes: &[Out], depth: usize, html: &mut String) {
        let inline = nodes.iter().all(is_inline);
        for (k, node) in nodes.iter().enumerate() {
            if k > 0 && !inline {
                self.newline(depth, html);
            }
            self.node(node, depth, html);
        }
    }

    fn node(&self, node: &Out, depth: usize, html: &mut String) {
        let (name, attributes, children) = match node {
            Out::Text(text) => {
                html.push_str(text);
                return;
            }
            Out::Element { name, attributes, children } => (name, attributes, children),
        };
        html.push('<');
        html.push_str(name);
        for (key, value) in attributes {
            html.push(' ');
            html.push_str(key);
            match value {
                Some(value) => {
                    html.push_str("=\"");
                    html.push_str(if value.is_empty() { "\0" } else { value });
                    html.push('"');
                }
                None if BOOLEAN_ATTRIBUTES.contains(&key.to_ascii_lowercase().as_str()) => {}
                None => html.push_str("=\"\0\""),
            }
        }
        html.push('>');
        let lower = name.to_ascii_lowercase();
        if VOID.contains(&lower.as_str()) {
            return;
        }
        if children.is_empty() {
            html.push(SPOT);
        } else if children.iter().all(is_inline) {
            self.siblings(children, depth, html);
        } else {
            self.newline(depth + 1, html);
            self.siblings(children, depth + 1, html);
            self.newline(depth, html);
        }
        html.push_str("</");
        html.push_str(name);
        html.push('>');
    }
}
