use std::path::Path;

pub fn parse(text: &str) -> Vec<(String, String)> {
    let mut variables = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let Some((name, value)) = line.split_once('=') else { continue };
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }
        variables.push((name.to_owned(), value_of(value.trim())));
    }
    variables
}

fn value_of(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix('"') {
        let mut value = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => value.push('\n'),
                    Some('t') => value.push('\t'),
                    Some(other) => value.push(other),
                    None => value.push('\\'),
                },
                other => value.push(other),
            }
        }
        return value;
    }
    if let Some(rest) = raw.strip_prefix('\'') {
        return rest.split('\'').next().unwrap_or_default().to_owned();
    }
    let plain = match raw.find(" #") {
        Some(comment) => &raw[..comment],
        None => raw,
    };
    plain.trim().to_owned()
}

pub fn read(path: &Path) -> std::io::Result<Vec<(String, String)>> {
    std::fs::read_to_string(path).map(|text| parse(&text))
}

pub fn expand_references(text: &str, parent: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let named = after.find('$').filter(|&end| is_name(&after[..end]));
        match named.and_then(|end| parent(&after[..end]).map(|value| (end, value))) {
            Some((end, value)) => {
                out.push_str(&value);
                rest = &after[end + 1..];
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dotenv_file_gives_its_variables_in_order() {
        let text = "# settings\n\nPORT=8080\nexport MODE = dev \nQUOTED=\"two words\\n#not a comment\"\nRAW='$HOME stays'\nTRAILING=x # a comment\nnot a variable\nEMPTY=\n";
        let parsed = parse(text);
        let parsed: Vec<(&str, &str)> = parsed.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        assert_eq!(
            parsed,
            [
                ("PORT", "8080"),
                ("MODE", "dev"),
                ("QUOTED", "two words\n#not a comment"),
                ("RAW", "$HOME stays"),
                ("TRAILING", "x"),
                ("EMPTY", ""),
            ]
        );
    }

    #[test]
    fn references_to_the_parent_environment_expand_case_sensitively() {
        let parent = |name: &str| match name {
            "PATH" => Some("/usr/bin".to_owned()),
            "Home" => Some("/h".to_owned()),
            _ => None,
        };
        assert_eq!(expand_references("/x:$PATH$", parent), "/x:/usr/bin");
        assert_eq!(expand_references("$path$ $Home$ $HOME$", parent), "$path$ /h $HOME$");
        assert_eq!(expand_references("cost $5 and $$PATH$", parent), "cost $5 and $/usr/bin");
        assert_eq!(expand_references("$PATH", parent), "$PATH");
    }
}
