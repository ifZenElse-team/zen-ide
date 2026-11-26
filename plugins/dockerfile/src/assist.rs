use crate::INSTRUCTIONS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub kind: &'static str,
    pub insert: String,
    pub detail: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub from: usize,
    pub items: Vec<Suggestion>,
}

const FLAGS: &[(&str, &[&str])] = &[
    ("FROM", &["platform"]),
    ("RUN", &["mount", "network", "security"]),
    ("COPY", &["from", "chown", "chmod", "link", "parents", "exclude"]),
    ("ADD", &["chown", "chmod", "link", "keep-git-dir", "checksum", "exclude"]),
    ("HEALTHCHECK", &["interval", "timeout", "start-period", "start-interval", "retries"]),
];

const DETAILS: &[(&str, &str)] = &[
    ("FROM", "Starts a build stage from a base image"),
    ("RUN", "Runs a command while building"),
    ("CMD", "The default command of the container"),
    ("LABEL", "Adds metadata to the image"),
    ("MAINTAINER", "Deprecated: use LABEL maintainer="),
    ("EXPOSE", "Ports the container listens on"),
    ("ENV", "Sets environment variables"),
    ("ADD", "Adds files, URLs or archives"),
    ("COPY", "Copies files into the image"),
    ("ENTRYPOINT", "The program the container runs"),
    ("VOLUME", "Creates a mount point"),
    ("USER", "The user the next instructions run as"),
    ("WORKDIR", "The working folder"),
    ("ARG", "A variable given at build time"),
    ("ONBUILD", "An instruction for builds based on this image"),
    ("STOPSIGNAL", "The signal that stops the container"),
    ("HEALTHCHECK", "How to check that the container works"),
    ("SHELL", "The shell of the shell form"),
];

const NOT_IN_ONBUILD: &[&str] = &["ONBUILD", "FROM", "MAINTAINER"];

const DIRECTIVE_NAMES: &[&str] = &["syntax", "escape", "check"];

struct Word<'a> {
    start: usize,
    text: &'a str,
}

struct Logical<'a> {
    keyword: Word<'a>,
    words: Vec<Word<'a>>,
    end: usize,
}

fn flags_of(instruction: &str) -> &'static [&'static str] {
    FLAGS.iter().find(|(name, _)| *name == instruction).map_or(&[], |(_, flags)| flags)
}

fn known(word: &str) -> Option<&'static str> {
    INSTRUCTIONS.iter().copied().find(|name| name.eq_ignore_ascii_case(word))
}

fn directive_escape(line: &str) -> Option<Option<u8>> {
    let rest = line.strip_prefix('#')?.trim_start_matches([' ', '\t']);
    let length = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    let key = &rest[..length];
    let value = rest[length..].trim_start_matches([' ', '\t']).strip_prefix('=')?.trim();
    if !DIRECTIVE_NAMES.iter().any(|name| name.eq_ignore_ascii_case(key)) {
        return None;
    }
    Some(if key.eq_ignore_ascii_case("escape") {
        match value {
            "`" => Some(b'`'),
            _ => Some(b'\\'),
        }
    } else {
        None
    })
}

fn heredoc_names(words: &[Word<'_>]) -> Vec<(String, bool)> {
    words
        .iter()
        .filter_map(|word| {
            let rest = word.text.strip_prefix("<<")?;
            let (strip, rest) = match rest.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, rest),
            };
            let name = rest.trim_matches(['"', '\'']);
            let plain = !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            plain.then(|| (name.to_owned(), strip))
        })
        .collect()
}

fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    text.split('\n').map(move |piece| {
        let start = at;
        at += piece.len() + 1;
        (start, piece.trim_end_matches('\r'))
    })
}

fn logical_lines(text: &str) -> Vec<Logical<'_>> {
    let mut escape = b'\\';
    let mut directives = true;
    let mut out: Vec<Logical<'_>> = Vec::new();
    let mut current: Option<Logical<'_>> = None;
    let mut heredocs: Vec<(String, bool)> = Vec::new();
    for (start, line) in lines(text) {
        if current.is_none()
            && let Some((name, strip)) = heredocs.first()
        {
            let delimiter = if *strip { line.trim_start_matches('\t') } else { line };
            if delimiter == name {
                heredocs.remove(0);
            }
            continue;
        }
        let content = line.trim_start_matches([' ', '\t']);
        let offset = start + line.len() - content.len();
        if directives {
            if let Some(found) = directive_escape(content) {
                if let Some(found) = found {
                    escape = found;
                }
                continue;
            }
            directives = false;
        }
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let trimmed = content.trim_end_matches([' ', '\t']);
        let continues = trimmed.as_bytes().last() == Some(&escape);
        let body = if continues { &trimmed[..trimmed.len() - 1] } else { trimmed };
        let mut words = Vec::new();
        let mut at = 0;
        for piece in body.split([' ', '\t']) {
            if !piece.is_empty() {
                words.push(Word { start: offset + at, text: piece });
            }
            at += piece.len() + 1;
        }
        let end = offset + trimmed.len();
        match current.as_mut() {
            Some(logical) => {
                logical.words.extend(words);
                logical.end = end;
            }
            None => {
                let mut words = words.into_iter();
                let Some(keyword) = words.next() else { continue };
                current = Some(Logical { keyword, words: words.collect(), end });
            }
        }
        if !continues && let Some(logical) = current.take() {
            heredocs.extend(heredoc_names(&logical.words));
            out.push(logical);
        }
    }
    out.extend(current);
    out
}

fn problem(start: usize, end: usize, severity: Severity, message: String) -> Problem {
    Problem { start, end, severity, message }
}

fn check_instruction(name: &'static str, keyword: &Word<'_>, words: &[Word<'_>], end: usize, out: &mut Vec<Problem>) {
    let allowed = flags_of(name);
    let mut rest = words;
    while let Some(word) = rest.first().filter(|word| word.text.starts_with("--")) {
        let flag = word.text[2..].split('=').next().unwrap_or_default();
        if !allowed.contains(&flag) {
            let message = if allowed.is_empty() {
                format!("{name} takes no flags")
            } else {
                format!("{name} does not take --{flag}; it takes --{}", allowed.join(", --"))
            };
            out.push(problem(word.start, word.start + word.text.len(), Severity::Error, message));
        }
        rest = &rest[1..];
    }
    if rest.is_empty() {
        let at = keyword.start;
        out.push(problem(at, end.max(at + keyword.text.len()), Severity::Error, format!("{name} needs arguments")));
    }
}

pub fn problems(text: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut seen_from = false;
    for logical in logical_lines(text) {
        let keyword = &logical.keyword;
        let keyword_end = keyword.start + keyword.text.len();
        let Some(name) = known(keyword.text) else {
            let message = format!("Unknown instruction: {}", keyword.text.to_ascii_uppercase());
            out.push(problem(keyword.start, keyword_end, Severity::Error, message));
            continue;
        };
        if name == "FROM" {
            seen_from = true;
        } else if !seen_from && name != "ARG" {
            let message = format!("{name} comes before the first FROM; only ARG may");
            out.push(problem(keyword.start, keyword_end, Severity::Error, message));
            seen_from = true;
        }
        if name == "MAINTAINER" {
            let message = "MAINTAINER is deprecated: use LABEL maintainer=\"…\"".to_owned();
            out.push(problem(keyword.start, keyword_end, Severity::Warning, message));
        }
        if name == "ONBUILD" {
            match logical.words.first() {
                Some(inner) => match known(inner.text) {
                    Some(inner_name) if NOT_IN_ONBUILD.contains(&inner_name) => {
                        let message = format!("ONBUILD cannot hold {inner_name}");
                        out.push(problem(inner.start, inner.start + inner.text.len(), Severity::Error, message));
                    }
                    Some(inner_name) => {
                        check_instruction(inner_name, inner, &logical.words[1..], logical.end, &mut out)
                    }
                    None => {
                        let message = format!("Unknown instruction: {}", inner.text.to_ascii_uppercase());
                        out.push(problem(inner.start, inner.start + inner.text.len(), Severity::Error, message));
                    }
                },
                None => check_instruction(name, keyword, &[], logical.end, &mut out),
            }
            continue;
        }
        check_instruction(name, keyword, &logical.words, logical.end, &mut out);
    }
    out
}

fn instruction_items(prefix: &str) -> Vec<Suggestion> {
    INSTRUCTIONS
        .iter()
        .filter(|name| name.starts_with(&prefix.to_ascii_uppercase()))
        .map(|name| Suggestion {
            label: (*name).to_owned(),
            kind: "keyword",
            insert: format!("{name} "),
            detail: DETAILS.iter().find(|(known, _)| known == name).map(|(_, detail)| *detail),
        })
        .collect()
}

pub fn complete(text: &str, caret: usize) -> Option<Completion> {
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let line_start = text[..caret].rfind('\n').map_or(0, |at| at + 1);
    let before = &text[line_start..caret];
    let word_start = line_start + before.rfind([' ', '\t']).map_or(0, |at| at + 1);
    let word = &text[word_start..caret];
    let previous_continues =
        text[..line_start].trim_end_matches(['\n', '\r']).trim_end_matches([' ', '\t']).ends_with('\\');
    let leading = before[..word_start - line_start].trim_matches([' ', '\t']);
    if before.trim_start().starts_with('#') {
        let rest = before.trim_start().trim_start_matches('#').trim_start();
        let instructions_before = logical_lines(&text[..line_start]).is_empty();
        if instructions_before && rest.bytes().all(|b| b.is_ascii_alphabetic()) {
            let items = DIRECTIVE_NAMES
                .iter()
                .filter(|name| name.starts_with(rest))
                .map(|name| Suggestion {
                    label: (*name).to_owned(),
                    kind: "keyword",
                    insert: format!("{name}="),
                    detail: Some("A parser directive"),
                })
                .collect();
            return Some(Completion { from: caret - rest.len(), items });
        }
        return None;
    }
    if leading.is_empty() && !previous_continues {
        return Some(Completion { from: word_start, items: instruction_items(word) });
    }
    let logical = logical_lines(&text[..caret]);
    let current = logical.last()?;
    let name = known(current.keyword.text)?;
    let positional: Vec<&Word<'_>> = current.words.iter().filter(|candidate| candidate.start < word_start).collect();
    if name == "ONBUILD" && positional.is_empty() {
        return Some(Completion { from: word_start, items: instruction_items(word) });
    }
    if word.starts_with("--") || (word.is_empty() || word == "-") && positional.iter().all(|w| w.text.starts_with("--"))
    {
        let items: Vec<Suggestion> = flags_of(name)
            .iter()
            .filter(|flag| format!("--{flag}").starts_with(word))
            .map(|flag| Suggestion {
                label: format!("--{flag}"),
                kind: "attribute",
                insert: if *flag == "link" || *flag == "keep-git-dir" {
                    format!("--{flag} ")
                } else {
                    format!("--{flag}=")
                },
                detail: None,
            })
            .collect();
        if name == "HEALTHCHECK" && !word.starts_with('-') {
            let mut items = items;
            for value in ["CMD", "NONE"] {
                items.push(Suggestion {
                    label: value.to_owned(),
                    kind: "keyword",
                    insert: format!("{value} "),
                    detail: None,
                });
            }
            return Some(Completion { from: word_start, items });
        }
        if !items.is_empty() {
            return Some(Completion { from: word_start, items });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(&str, Severity, String)> {
        problems(text).into_iter().map(|p| (&text[p.start..p.end], p.severity, p.message)).collect()
    }

    fn labels(text: &str) -> Vec<String> {
        let caret = text.find('|').unwrap();
        let text = text.replacen('|', "", 1);
        complete(&text, caret).map(|found| found.items.into_iter().map(|item| item.label).collect()).unwrap_or_default()
    }

    #[test]
    fn a_good_multi_stage_build_has_no_problems() {
        let text = "# syntax=docker/dockerfile:1\nARG GO=1.27\nFROM --platform=$BUILDPLATFORM golang:${GO} AS build\n\
                    RUN --mount=type=cache,target=/root/.cache \\\n    go build -o /app .\n\
                    COPY --from=build --chmod=755 /app /app\nRUN <<EOF\nset -e\nFROMAGE not an instruction\nEOF\n\
                    HEALTHCHECK --interval=30s CMD curl -f http://localhost/\nCMD [\"/app\"]\n";
        assert_eq!(found(text), []);
    }

    #[test]
    fn unknown_instructions_and_flags_are_errors() {
        let text = "FROM alpine\nCOPPY a b\nCOPY --form=build a b\nUSER --platform=x root\n";
        let problems = found(text);
        assert_eq!(problems[0], ("COPPY", Severity::Error, "Unknown instruction: COPPY".to_owned()));
        assert_eq!(problems[1].0, "--form=build");
        assert!(problems[1].2.contains("--from"), "{:?}", problems[1]);
        assert_eq!(problems[2], ("--platform=x", Severity::Error, "USER takes no flags".to_owned()));
        assert_eq!(problems.len(), 3);
    }

    #[test]
    fn only_arg_comes_before_from() {
        let problems = found("ARG V=1\nRUN echo\nFROM alpine\n");
        assert_eq!(problems, [("RUN", Severity::Error, "RUN comes before the first FROM; only ARG may".to_owned())]);
    }

    #[test]
    fn maintainer_is_deprecated_and_empty_instructions_need_arguments() {
        let problems = found("FROM alpine\nMAINTAINER me\nWORKDIR\nONBUILD FROM x\n");
        assert_eq!(problems[0].1, Severity::Warning);
        assert_eq!(problems[1], ("WORKDIR", Severity::Error, "WORKDIR needs arguments".to_owned()));
        assert_eq!(problems[2], ("FROM", Severity::Error, "ONBUILD cannot hold FROM".to_owned()));
    }

    #[test]
    fn the_escape_directive_changes_the_line_continuation() {
        assert_eq!(found("# escape=`\nFROM mcr.example/windows\nRUN dir `\n  c:\\\n"), []);
    }

    #[test]
    fn completion_offers_instructions_flags_and_directives() {
        assert_eq!(labels("FROM alpine\nCO|"), ["COPY"]);
        assert_eq!(
            labels("FROM alpine\nCOPY --|"),
            ["--from", "--chown", "--chmod", "--link", "--parents", "--exclude"]
        );
        assert_eq!(labels("FROM alpine\nRUN --m|"), ["--mount"]);
        assert_eq!(labels("FROM alpine\nONBUILD R|"), ["RUN"]);
        assert_eq!(labels("# sy|\nFROM alpine\n"), ["syntax"]);
        assert_eq!(labels("FROM alpine\nRUN echo \\\n  |"), Vec::<String>::new(), "a continued line is the command");
        assert!(labels("FROM alpine\nHEALTHCHECK |").contains(&"NONE".to_owned()));
    }
}
