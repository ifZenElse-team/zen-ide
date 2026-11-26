use std::ops::Range;

use super::Template;
use super::expressions::{self, Place};

pub const END: &str = "END";

pub const SELECTION: &str = "SELECTION";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expanded {
    pub text: String,
    pub variables: Vec<(String, Vec<Range<usize>>)>,
    pub end: usize,
    pub skipped: Vec<String>,
    pub dependents: Vec<(String, String)>,
    pub place: Place,
}

enum Piece<'a> {
    Char(char),
    Variable(&'a str),
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '$' {
            let after = &rest[1..];
            if let Some(stripped) = after.strip_prefix('$') {
                pieces.push(Piece::Char('$'));
                rest = stripped;
                continue;
            }
            if let Some(close) = after.find('$')
                && is_name(&after[..close])
            {
                pieces.push(Piece::Variable(&after[..close]));
                rest = &after[close + 1..];
                continue;
            }
        }
        pieces.push(Piece::Char(c));
        rest = &rest[c.len_utf8()..];
    }
    pieces
}

pub fn variables_of(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for piece in pieces(text) {
        if let Piece::Variable(name) = piece
            && name != END
            && name != SELECTION
            && !names.iter().any(|known| known == name)
        {
            names.push(name.to_owned());
        }
    }
    names
}

pub fn surrounds(text: &str) -> bool {
    pieces(text).iter().any(|piece| matches!(piece, Piece::Variable(SELECTION)))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Around {
    pub text: String,
    pub verbatim: Vec<usize>,
    pub crlf: bool,
    pub place: Place,
}

pub fn values_of(template: &Template, place: &Place) -> Vec<(String, String)> {
    let mut names: Vec<String> = template.variables.iter().map(|variable| variable.name.clone()).collect();
    for name in variables_of(&template.text) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let mut values: Vec<(String, String)> = Vec::with_capacity(names.len());
    for name in names {
        let default = template.default_of(&name).to_owned();
        let expression = template
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .map_or("", |variable| variable.expression.as_str());
        let value = if expression.trim().is_empty() {
            default
        } else {
            let known = |wanted: &str| values.iter().find(|(known, _)| known == wanted).map(|(_, value)| value.clone());
            let computed = expressions::evaluate(expression, &known, place);
            if computed.is_empty() { default } else { computed }
        };
        values.push((name, value));
    }
    values
}

pub fn expand(template: &Template, indent: &str, unit: &str) -> Expanded {
    expand_around(template, indent, unit, &Around::default())
}

pub fn expand_around(template: &Template, indent: &str, unit: &str, around: &Around) -> Expanded {
    let newline = if around.crlf { "\r\n" } else { "\n" };
    let mut text = String::new();
    let mut len = 0;
    let mut end = None;
    let mut found: Vec<(String, Vec<Range<usize>>)> = Vec::new();
    let mut line_start = true;
    let values = values_of(template, &around.place);
    let push = |text: &mut String, len: &mut usize, piece: &str| {
        text.push_str(piece);
        *len += piece.chars().count();
    };
    for piece in pieces(&template.text) {
        match piece {
            Piece::Char('\n') => {
                push(&mut text, &mut len, newline);
                push(&mut text, &mut len, indent);
                line_start = true;
            }
            Piece::Char('\t') if line_start => push(&mut text, &mut len, unit),
            Piece::Char(c) => {
                push(&mut text, &mut len, c.encode_utf8(&mut [0; 4]));
                line_start = false;
            }
            Piece::Variable(END) => {
                end.get_or_insert(len);
                line_start = false;
            }
            Piece::Variable(SELECTION) => {
                let line = text.rfind('\n').map_or_else(|| format!("{indent}{text}"), |at| text[at + 1..].to_owned());
                let prefix: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                for (index, part) in around.text.split_inclusive('\n').enumerate() {
                    if index == 0 || around.verbatim.contains(&index) {
                        push(&mut text, &mut len, part);
                    } else if part.trim().is_empty() {
                        push(&mut text, &mut len, &part[part.trim_end_matches(['\r', '\n']).len()..]);
                    } else {
                        push(&mut text, &mut len, &prefix);
                        push(&mut text, &mut len, part);
                    }
                }
                line_start = false;
            }
            Piece::Variable(name) => {
                let value = values.iter().find(|(known, _)| known == name).map_or("", |(_, value)| value.as_str());
                let start = len;
                push(&mut text, &mut len, value);
                match found.iter_mut().find(|(known, _)| known == name) {
                    Some((_, ranges)) => ranges.push(start..len),
                    None => found.push((name.to_owned(), std::iter::once(start..len).collect())),
                }
                line_start = false;
            }
        }
    }
    let mut variables = Vec::with_capacity(found.len());
    for declared in &template.variables {
        if let Some(index) = found.iter().position(|(name, _)| *name == declared.name) {
            variables.push(found.remove(index));
        }
    }
    variables.extend(found);
    let names: Vec<String> = variables.iter().map(|(name, _)| name.clone()).collect();
    let mut skipped = Vec::new();
    let mut dependents = Vec::new();
    for declared in template.variables.iter().filter(|declared| names.contains(&declared.name)) {
        let value = values.iter().find(|(name, _)| *name == declared.name).map_or("", |(_, value)| value.as_str());
        if declared.skip_if_defined && !value.is_empty() {
            skipped.push(declared.name.clone());
        }
        let refers = expressions::references(&declared.expression)
            .iter()
            .any(|name| *name != declared.name && names.contains(name));
        if refers {
            dependents.push((declared.name.clone(), declared.expression.clone()));
        }
    }
    Expanded { text, variables, end: end.unwrap_or(len), skipped, dependents, place: around.place.clone() }
}

impl Expanded {
    pub fn with_line_separator(self, separator: zen_text::LineSeparator) -> Self {
        let std::borrow::Cow::Owned(text) = zen_text::line_separator::convert(&self.text, separator) else {
            return self;
        };
        let offsets: Vec<usize> = std::iter::once(self.end)
            .chain(
                self.variables.iter().flat_map(|(_, ranges)| ranges.iter().flat_map(|range| [range.start, range.end])),
            )
            .collect();
        let mapped = zen_text::line_separator::convert_offsets(&self.text, separator, &offsets);
        let mut places = mapped[1..].chunks(2);
        let variables = self
            .variables
            .into_iter()
            .map(|(name, ranges)| {
                let ranges = ranges.iter().filter_map(|_| places.next().map(|pair| pair[0]..pair[1])).collect();
                (name, ranges)
            })
            .collect();
        Self { text, variables, end: mapped[0], ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::Variable;

    fn template(text: &str, variables: &[(&str, &str)]) -> Template {
        Template {
            language: "go".to_owned(),
            abbreviation: "t".to_owned(),
            description: String::new(),
            text: text.to_owned(),
            variables: variables
                .iter()
                .map(|(name, default)| Variable {
                    name: (*name).to_owned(),
                    default: (*default).to_owned(),
                    ..Variable::default()
                })
                .collect(),
            imports: Vec::new(),
            contexts: Vec::new(),
            enabled: true,
        }
    }

    #[test]
    fn variables_take_their_defaults_and_link_their_occurrences() {
        let expanded =
            expand(&template("for $I$ := 0; $I$ < $N$; $I$++ {\n\t$END$\n}", &[("I", "i"), ("N", "n")]), "", "\t");
        assert_eq!(expanded.text, "for i := 0; i < n; i++ {\n\t\n}");
        assert_eq!(
            expanded.variables,
            vec![("I".to_owned(), vec![4..5, 12..13, 19..20]), ("N".to_owned(), std::iter::once(16..17).collect())]
        );
        assert_eq!(expanded.end, 26);
    }

    #[test]
    fn the_declared_order_comes_first_and_the_indentation_follows_the_line() {
        let expanded = expand(&template("$A$ $B$\n\t$C$", &[("C", ""), ("A", "a")]), "  ", "    ");
        assert_eq!(expanded.text, "a \n      ");
        let names: Vec<&str> = expanded.variables.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["C", "A", "B"]);
        assert_eq!(expanded.variables[0].1, std::slice::from_ref(&(9..9)));
        assert_eq!(expanded.end, 9, "without $END$ the caret goes to the end");
    }

    fn around(text: &str) -> Around {
        Around { text: text.to_owned(), ..Around::default() }
    }

    #[test]
    fn the_selection_takes_the_indentation_of_its_line() {
        let wrap = template("if $C$ {\n\t$SELECTION$\n}", &[]);
        let expanded = expand_around(&wrap, "  ", "    ", &around("a()\n\n\tb()\n    "));
        assert_eq!(expanded.text, "if  {\n      a()\n\n      \tb()\n\n  }");
        assert_eq!(expanded.variables, vec![("C".to_owned(), std::iter::once(3..3).collect())]);
        let not = template("!($SELECTION$)", &[]);
        assert_eq!(expand_around(&not, "\t", "\t", &around("x &&\n\ty")).text, "!(x &&\n\t\ty)");
        assert!(surrounds("($SELECTION$)") && !surrounds("$$SELECTION$$"));
        assert_eq!(variables_of("$SELECTION$ $A$"), ["A"]);
    }

    #[test]
    fn verbatim_lines_and_line_breaks_stay_as_they_are() {
        let wrap = template("if $C$ {\n\t$SELECTION$\n}", &[]);
        let code = Around {
            text: "s := `a\r\n  b\r\n`\r\nuse(s)".to_owned(),
            verbatim: vec![1, 2],
            crlf: true,
            ..Around::default()
        };
        let expanded = expand_around(&wrap, "\t", "\t", &code);
        assert_eq!(expanded.text, "if  {\r\n\t\ts := `a\r\n  b\r\n`\r\n\t\tuse(s)\r\n\t}");
        let blank =
            Around { text: "a()\r\n \t\r\nb()".to_owned(), verbatim: Vec::new(), crlf: true, ..Around::default() };
        assert_eq!(expand_around(&wrap, "", "\t", &blank).text, "if  {\r\n\ta()\r\n\r\n\tb()\r\n}");
    }

    #[test]
    fn line_breaks_take_the_files_separator_and_the_variables_follow() {
        let template = template("if $X$ {\n\t$END$\n} $X$", &[("X", "ok")]);
        let expanded = expand(&template, "", "\t").with_line_separator(zen_text::LineSeparator::Crlf);
        assert_eq!(expanded.text, "if ok {\r\n\t\r\n} ok");
        assert_eq!(expanded.variables, vec![("X".to_owned(), vec![3..5, 14..16])]);
        assert_eq!(expanded.end, 10);
        let same = expand(&template, "", "\t");
        assert_eq!(same.clone().with_line_separator(zen_text::LineSeparator::Lf), same);
    }

    #[test]
    fn dollars_that_are_not_variables_stay() {
        let expanded = expand(&template("cost $$5 $not a var$ é$X$", &[]), "", "\t");
        assert_eq!(expanded.text, "cost $5 $not a var$ é");
        assert_eq!(expanded.variables, vec![("X".to_owned(), std::iter::once(21..21).collect())]);
        assert_eq!(variables_of("$A$ $END$ $B$ $A$"), ["A", "B"]);
    }
}
