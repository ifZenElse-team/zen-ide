use std::ops::Range;

const SOURCES: &[&str] = &[
    "stream",
    "parallelStream",
    "of",
    "ofNullable",
    "iterate",
    "generate",
    "concat",
    "empty",
    "range",
    "rangeClosed",
    "chars",
    "codePoints",
    "ints",
    "longs",
    "doubles",
    "splitAsStream",
];

const STATIC_SOURCES: &[&str] = &["of", "ofNullable", "iterate", "generate", "concat", "empty", "range", "rangeClosed"];

const INTERMEDIATE: &[&str] = &[
    "filter",
    "map",
    "mapToInt",
    "mapToLong",
    "mapToDouble",
    "mapToObj",
    "flatMap",
    "flatMapToInt",
    "flatMapToLong",
    "flatMapToDouble",
    "flatMapToObj",
    "mapMulti",
    "mapMultiToInt",
    "mapMultiToLong",
    "mapMultiToDouble",
    "mapMultiToObj",
    "distinct",
    "sorted",
    "limit",
    "skip",
    "takeWhile",
    "dropWhile",
    "boxed",
    "asLongStream",
    "asDoubleStream",
    "parallel",
    "sequential",
    "unordered",
    "gather",
];

const SIDE_EFFECTS: &[&str] = &["peek", "onClose"];

const PURE_TERMINALS: &[&str] = &[
    "collect",
    "toList",
    "toArray",
    "reduce",
    "count",
    "sum",
    "average",
    "min",
    "max",
    "anyMatch",
    "allMatch",
    "noneMatch",
    "findFirst",
    "findAny",
    "summaryStatistics",
];

const BOUNDING: &[&str] = &["limit", "takeWhile"];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamCall {
    pub name: String,
    pub label: String,
    pub end: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamChain {
    pub text: String,
    pub source: StreamCall,
    pub calls: Vec<StreamCall>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainRefusal {
    NoStream,
    NotCreatedHere,
    NoOperations,
    SideEffect(String),
    Infinite,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepKind {
    Elements,
    Result,
    Unbounded,
    NotRun,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceStep {
    pub label: String,
    pub kind: StepKind,
    pub expression: Option<String>,
}
pub fn chain_at(text: &str, line: usize) -> Result<StreamChain, ChainRefusal> {
    let chars: Vec<char> = text.chars().collect();
    let masked = mask(&chars);
    let start = line_start(&chars, line).ok_or(ChainRefusal::NoStream)?;
    let window = statement(&masked, start);
    let mut fallback = None;
    let mut at = window.start;
    while let Some(found) = next_source(&masked, at, window.end) {
        at = found.end;
        if let Some(chain) = chain_from(&chars, &masked, found) {
            if !chain.calls.is_empty() {
                return Ok(chain);
            }
            fallback.get_or_insert(chain);
        }
    }
    let Some(chain) = fallback else {
        let code: String = masked[window].iter().filter(|c| !c.is_whitespace()).collect();
        let operated = INTERMEDIATE.iter().chain(SIDE_EFFECTS).any(|name| code.contains(&format!(".{name}(")));
        return Err(if operated { ChainRefusal::NotCreatedHere } else { ChainRefusal::NoStream });
    };
    if chain.calls.is_empty() { Err(ChainRefusal::NoOperations) } else { Ok(chain) }
}
pub fn trace_steps(chain: &StreamChain) -> Result<Vec<TraceStep>, ChainRefusal> {
    if let Some(call) = chain.calls.iter().find(|call| SIDE_EFFECTS.contains(&call.name.as_str())) {
        return Err(ChainRefusal::SideEffect(call.name.clone()));
    }
    let terminal_at = chain.calls.iter().position(|call| !INTERMEDIATE.contains(&call.name.as_str()));
    let intermediate = &chain.calls[..terminal_at.unwrap_or(chain.calls.len())];
    let mut bounded = !unbounded(&chain.source);
    if !bounded && !intermediate.iter().any(|call| BOUNDING.contains(&call.name.as_str())) {
        return Err(ChainRefusal::Infinite);
    }
    let prefix = |end: usize| chain.text.chars().take(end).collect::<String>();
    let mut steps = vec![TraceStep {
        label: chain.source.label.clone(),
        kind: if bounded { StepKind::Elements } else { StepKind::Unbounded },
        expression: bounded.then(|| format!("{}.toArray()", prefix(chain.source.end))),
    }];
    for call in intermediate {
        bounded |= BOUNDING.contains(&call.name.as_str());
        steps.push(TraceStep {
            label: call.label.clone(),
            kind: if bounded { StepKind::Elements } else { StepKind::Unbounded },
            expression: bounded.then(|| format!("{}.toArray()", prefix(call.end))),
        });
    }
    if let Some(terminal) = terminal_at.map(|at| &chain.calls[at]) {
        let pure = PURE_TERMINALS.contains(&terminal.name.as_str());
        steps.push(TraceStep {
            label: terminal.label.clone(),
            kind: if pure { StepKind::Result } else { StepKind::NotRun },
            expression: pure.then(|| prefix(terminal.end)),
        });
    }
    Ok(steps)
}

fn unbounded(source: &StreamCall) -> bool {
    let arguments = arguments_of(&source.label);
    match source.name.as_str() {
        "generate" => true,
        "iterate" => arguments == 2,
        "ints" | "longs" | "doubles" => arguments == 0 || arguments == 2,
        _ => false,
    }
}

fn arguments_of(label: &str) -> usize {
    let chars: Vec<char> = label.chars().collect();
    let masked = mask(&chars);
    let Some(open) = masked.iter().position(|c| *c == '(') else { return 0 };
    let mut depth = 0usize;
    let mut count = 0usize;
    let mut any = false;
    for c in &masked[open + 1..] {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => break,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => count += 1,
            c if !c.is_whitespace() => any = true,
            _ => {}
        }
    }
    if any { count + 1 } else { 0 }
}

fn mask(chars: &[char]) -> Vec<char> {
    let mut out = chars.to_vec();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    out[i] = ' ';
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                let begin = i;
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                let end = (i + 2).min(chars.len());
                for slot in &mut out[begin..end] {
                    if *slot != '\n' {
                        *slot = ' ';
                    }
                }
                i = end;
            }
            '"' if chars.get(i + 1) == Some(&'"') && chars.get(i + 2) == Some(&'"') => {
                i += 3;
                while i < chars.len()
                    && !(chars[i] == '"' && chars.get(i + 1) == Some(&'"') && chars.get(i + 2) == Some(&'"'))
                {
                    if chars[i] == '\\' {
                        out[i] = 'x';
                        i += 1;
                    }
                    if i < chars.len() && chars[i] != '\n' {
                        out[i] = 'x';
                    }
                    i += 1;
                }
                i += 3;
            }
            quote @ ('"' | '\'') => {
                i += 1;
                while i < chars.len() && chars[i] != quote && chars[i] != '\n' {
                    if chars[i] == '\\' {
                        out[i] = 'x';
                        i += 1;
                    }
                    if i < chars.len() {
                        out[i] = 'x';
                    }
                    i += 1;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

fn line_start(chars: &[char], line: usize) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    let mut seen = 0;
    for (index, c) in chars.iter().enumerate() {
        if *c == '\n' {
            seen += 1;
            if seen == line {
                return Some(index + 1);
            }
        }
    }
    None
}

fn statement(masked: &[char], from: usize) -> Range<usize> {
    let mut start = from;
    let mut depth = 0i32;
    while start > 0 {
        let c = masked[start - 1];
        match c {
            ')' | ']' => depth += 1,
            '(' | '[' => depth -= 1,
            ';' | '{' | '}' if depth <= 0 => break,
            _ => {}
        }
        start -= 1;
    }
    let mut end = from;
    let mut depth = 0i32;
    while end < masked.len() {
        match masked[end] {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => break,
            ')' | ']' | '}' => depth -= 1,
            ';' if depth == 0 => break,
            _ => {}
        }
        end += 1;
    }
    start..end
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

struct Found {
    dot: usize,
    name: Range<usize>,
    end: usize,
}

fn next_source(masked: &[char], from: usize, to: usize) -> Option<Found> {
    let mut i = from;
    while i < to {
        if !is_word(masked[i]) || (i > 0 && is_word(masked[i - 1])) {
            i += 1;
            continue;
        }
        let begin = i;
        while i < to && is_word(masked[i]) {
            i += 1;
        }
        let name: String = masked[begin..i].iter().collect();
        if !SOURCES.contains(&name.as_str()) {
            continue;
        }
        let Some(dot) = before_skipping_space(masked, begin).filter(|at| masked[*at] == '.') else { continue };
        let Some(open) = after_skipping_space(masked, i, to).filter(|at| masked[*at] == '(') else { continue };
        let Some(close) = closing(masked, open, to) else { continue };
        return Some(Found { dot, name: begin..i, end: close + 1 });
    }
    None
}

fn before_skipping_space(masked: &[char], at: usize) -> Option<usize> {
    (0..at).rev().find(|index| !masked[*index].is_whitespace())
}

fn after_skipping_space(masked: &[char], at: usize, to: usize) -> Option<usize> {
    (at..to).find(|index| !masked[*index].is_whitespace())
}

fn closing(masked: &[char], open: usize, to: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, c) in masked.iter().enumerate().take(to).skip(open) {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn receiver_start(masked: &[char], dot: usize) -> usize {
    let mut start = dot;
    loop {
        let Some(at) = before_skipping_space(masked, start) else { return start };
        let spaced = at + 1 < start;
        let c = masked[at];
        if spaced && c != '.' && masked[start] != '.' {
            if at >= 2 && masked[at - 2..=at] == ['n', 'e', 'w'] && (at < 3 || !is_word(masked[at - 3])) {
                start = at - 2;
            }
            break;
        }
        match c {
            '.' => start = at,
            ')' | ']' => {
                let open = if c == ')' { '(' } else { '[' };
                let mut depth = 0usize;
                let mut index = at + 1;
                let mut found = None;
                while index > 0 {
                    index -= 1;
                    let here = masked[index];
                    if here == c {
                        depth += 1;
                    } else if here == open {
                        depth -= 1;
                        if depth == 0 {
                            found = Some(index);
                            break;
                        }
                    }
                }
                let Some(found) = found else { break };
                start = found;
                if let Some(before) = before_skipping_space(masked, start).filter(|at| masked[*at] == '>') {
                    let mut index = before;
                    while index > 0 && masked[index] != '<' {
                        index -= 1;
                    }
                    if masked[index] == '<' {
                        start = index;
                    }
                }
            }
            c if is_word(c) => {
                let mut begin = at;
                while begin > 0 && is_word(masked[begin - 1]) {
                    begin -= 1;
                }
                let word: String = masked[begin..=at].iter().collect();
                if matches!(word.as_str(), "return" | "yield" | "case" | "throw" | "else" | "assert") {
                    break;
                }
                start = begin;
            }
            _ => break,
        }
    }
    start
}

fn chain_from(chars: &[char], masked: &[char], found: Found) -> Option<StreamChain> {
    let begin = receiver_start(masked, found.dot);
    let name: String = masked[found.name.clone()].iter().collect();
    let receiver: String = masked[begin..found.dot].iter().filter(|c| !c.is_whitespace()).collect();
    let fresh = match name.as_str() {
        name if STATIC_SOURCES.contains(&name) => receiver.ends_with("Stream"),
        _ => !receiver.is_empty(),
    };
    if !fresh {
        return None;
    }
    let text_of = |range: Range<usize>| chars[range].iter().collect::<String>();
    let label = |range: Range<usize>| -> String {
        let text = text_of(range);
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace(" .", ".")
            .trim_start_matches('.')
            .trim()
            .to_owned()
    };
    let source = StreamCall { name, label: label(begin..found.end), end: found.end - begin };
    let mut calls = Vec::new();
    let mut at = found.end;
    let to = masked.len();
    while let Some(dot) = after_skipping_space(masked, at, to).filter(|index| masked[*index] == '.') {
        let Some(mut word) = after_skipping_space(masked, dot + 1, to) else { break };
        if masked[word] == '<' {
            let Some(close) = (word..to).find(|index| masked[*index] == '>') else { break };
            let Some(next) = after_skipping_space(masked, close + 1, to) else { break };
            word = next;
        }
        let mut word_end = word;
        while word_end < to && is_word(masked[word_end]) {
            word_end += 1;
        }
        if word_end == word {
            break;
        }
        let Some(open) = after_skipping_space(masked, word_end, to).filter(|index| masked[*index] == '(') else {
            break;
        };
        let Some(close) = closing(masked, open, to) else { break };
        let name: String = masked[word..word_end].iter().collect();
        calls.push(StreamCall { name, label: label(dot..close + 1), end: close + 1 - begin });
        at = close + 1;
    }
    let end = calls.last().map_or(source.end, |call| call.end) + begin;
    Some(StreamChain { text: text_of(begin..end), source, calls })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_with_text_beyond_ascii_is_cut_at_its_calls() {
        let text = "var n = Stream.of(\"привет\", \"мир\").filter(s -> s.length() > 3).toList();\n";
        let steps = trace_steps(&chain_at(text, 0).unwrap()).unwrap();
        let source = "Stream.of(\"привет\", \"мир\")";
        let filtered = format!("{source}.filter(s -> s.length() > 3)");
        let expressions: Vec<_> = steps.iter().map(|step| step.expression.clone().unwrap()).collect();
        assert_eq!(
            expressions,
            [format!("{source}.toArray()"), format!("{filtered}.toArray()"), format!("{filtered}.toList()")]
        );
    }

    fn steps(text: &str, line: usize) -> Vec<(String, StepKind, Option<String>)> {
        let chain = chain_at(text, line).unwrap();
        trace_steps(&chain).unwrap().into_iter().map(|step| (step.label, step.kind, step.expression)).collect()
    }

    #[test]
    fn each_prefix_of_a_chain_on_the_line_is_collected_into_an_array_and_the_result_is_evaluated() {
        let text = "class A {\n  void f() {\n    List<Integer> lengths = names.stream().filter(n -> n.length() == 3).map(String::length).collect(Collectors.toList());\n  }\n}\n";
        assert_eq!(
            steps(text, 2),
            [
                ("names.stream()".into(), StepKind::Elements, Some("names.stream().toArray()".into())),
                (
                    "filter(n -> n.length() == 3)".into(),
                    StepKind::Elements,
                    Some("names.stream().filter(n -> n.length() == 3).toArray()".into())
                ),
                (
                    "map(String::length)".into(),
                    StepKind::Elements,
                    Some("names.stream().filter(n -> n.length() == 3).map(String::length).toArray()".into())
                ),
                (
                    "collect(Collectors.toList())".into(),
                    StepKind::Result,
                    Some(
                        "names.stream().filter(n -> n.length() == 3).map(String::length).collect(Collectors.toList())"
                            .into()
                    )
                ),
            ]
        );
    }

    #[test]
    fn a_chain_over_several_lines_is_found_from_any_of_them() {
        let text = "void f() {\n  int sum = shop.items()\n      .stream() // all\n      .mapToInt(Item::price)\n      .sum();\n}\n";
        for line in 1..5 {
            let chain = chain_at(text, line).unwrap();
            let labels: Vec<&str> = chain.calls.iter().map(|call| call.label.as_str()).collect();
            assert_eq!(
                (chain.source.label.as_str(), labels.as_slice()),
                ("shop.items().stream()", &["mapToInt(Item::price)", "sum()"][..])
            );
        }
        let steps = steps(text, 1);
        assert_eq!(
            steps[2].2.as_deref(),
            Some("shop.items()\n      .stream() // all\n      .mapToInt(Item::price)\n      .sum()")
        );
    }

    #[test]
    fn static_and_array_sources_count_and_strings_do_not_confuse_the_parser() {
        let text = "return IntStream.rangeClosed(1, 5).map(i -> i * 2).boxed().toList();";
        assert_eq!(steps(text, 0).len(), 4);
        let text = "var s = Arrays.stream(words).filter(w -> !w.equals(\").stream(\")).count();";
        let steps = steps(text, 0);
        assert_eq!(steps[0].0, "Arrays.stream(words)");
        assert_eq!(steps[1].0, "filter(w -> !w.equals(\").stream(\"))");
        assert_eq!(steps[2].1, StepKind::Result);
        let text = "List<String> l = new ArrayList<>(set).stream().sorted().toList();";
        assert_eq!(chain_at(text, 0).unwrap().source.label, "new ArrayList<>(set).stream()");
    }

    #[test]
    fn a_side_effect_in_the_middle_is_refused_and_for_each_is_not_run() {
        let peek = "names.stream().peek(System.out::println).count();";
        assert_eq!(trace_steps(&chain_at(peek, 0).unwrap()), Err(ChainRefusal::SideEffect("peek".into())));
        let text = "names.stream().filter(n -> n.isEmpty()).forEach(System.out::println);";
        let steps = steps(text, 0);
        assert_eq!(steps.last().unwrap().1, StepKind::NotRun);
        assert_eq!(steps.last().unwrap().2, None);
    }

    #[test]
    fn an_infinite_stream_is_shown_only_after_it_is_limited() {
        let endless = "Stream.iterate(1, i -> i * 2).map(i -> i + 1).count();";
        assert_eq!(trace_steps(&chain_at(endless, 0).unwrap()), Err(ChainRefusal::Infinite));
        let limited = "Stream.iterate(1, i -> i * 2).map(i -> i + 1).limit(5).toList();";
        let kinds: Vec<StepKind> = steps(limited, 0).into_iter().map(|step| step.1).collect();
        assert_eq!(kinds, [StepKind::Unbounded, StepKind::Unbounded, StepKind::Elements, StepKind::Result]);
        let bounded = "Stream.iterate(1, i -> i < 9, i -> i * 2).toList();";
        assert_eq!(steps(bounded, 0)[0].1, StepKind::Elements);
        assert_eq!(trace_steps(&chain_at("random.ints().sum();", 0).unwrap()), Err(ChainRefusal::Infinite));
    }

    #[test]
    fn a_line_without_a_stream_made_on_it_is_refused() {
        assert_eq!(chain_at("int x = 1;", 0), Err(ChainRefusal::NoStream));
        assert_eq!(chain_at("long n = stream.filter(x -> x > 0).count();", 0), Err(ChainRefusal::NotCreatedHere));
        assert_eq!(chain_at("var s = list.stream();", 0), Err(ChainRefusal::NoOperations));
    }
}
