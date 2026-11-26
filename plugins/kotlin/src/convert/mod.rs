mod ast;
mod lex;
mod parse;
mod print;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversion {
    pub text: String,
    pub problems: Vec<Problem>,
}

pub const SOURCES: &str = "java";

pub fn java_to_kotlin(text: &str) -> Conversion {
    let mut parser = parse::Parser::new(text);
    let unit = parser.unit();
    let mut problems = std::mem::take(&mut parser.problems);
    let comments = std::mem::take(&mut parser.comments);
    let mut printer = print::Printer::new(text, &comments);
    printer.unit(&unit);
    let (out, more) = printer.finish();
    problems.extend(more);
    problems.sort_by_key(|problem| (problem.0, problem.1));
    problems.dedup();
    Conversion {
        text: out,
        problems: problems.into_iter().map(|(start, end, message)| Problem { start, end, message }).collect(),
    }
}

const MEMBERS: (&str, &str, &str) = ("class ZenFragment {\n", "\n}\n", "class ZenFragment {");
const STATEMENTS: (&str, &str, &str) =
    ("class ZenFragment {\nvoid zenFragment() {\n", "\n}\n}\n", "fun zenFragment() {");

fn declares_types(text: &str) -> bool {
    let mut parser = parse::Parser::new(text);
    let unit = parser.unit();
    !unit.types.is_empty() || unit.package.is_some() || !unit.imports.is_empty()
}

fn inside(text: &str, opening: &str, levels: usize) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| line.trim_end().ends_with(opening))?;
    let indent = "    ".repeat(levels);
    let depth = lines[start].len() - lines[start].trim_start().len();
    let end = (start + 1..lines.len()).find(|at| {
        let line = lines[*at];
        line.trim() == "}" && line.len() - line.trim_start().len() == depth
    })?;
    let body: Vec<&str> = lines[start + 1..end]
        .iter()
        .map(|line| line.strip_prefix(indent.as_str()).unwrap_or(line.trim_start()))
        .collect();
    let mut joined = body.join("\n");
    joined.push('\n');
    Some(joined)
}

fn wrapped(text: &str, (prefix, suffix, opening): (&str, &str, &str), levels: usize) -> Option<Conversion> {
    let converted = java_to_kotlin(&format!("{prefix}{text}{suffix}"));
    if !converted.problems.is_empty() {
        return None;
    }
    let text = inside(&converted.text, opening, levels)?;
    Some(Conversion { text, problems: Vec::new() })
}

pub fn fragment(text: &str) -> Conversion {
    let whole = java_to_kotlin(text);
    if whole.problems.is_empty() && declares_types(text) {
        return whole;
    }
    wrapped(text, MEMBERS, 1).or_else(|| wrapped(text, STATEMENTS, 2)).unwrap_or(whole)
}

pub fn convert(from: &str, text: &str) -> Option<Conversion> {
    (from == "java").then(|| fragment(text))
}
