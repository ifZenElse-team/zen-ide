use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    pub alias: String,
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub version_range: Option<Range<usize>>,
    pub version_ref: Option<String>,
    pub line: Range<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub versions: BTreeMap<String, (String, Range<usize>)>,
    pub libraries: Vec<Library>,
    pub libraries_table: Option<usize>,
    pub versions_table: Option<usize>,
}

fn lines(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    let mut at = 0;
    text.split_inclusive('\n').map(move |line| {
        let range = at..at + line.len();
        at += line.len();
        (range, line.trim_end_matches(['\r', '\n']))
    })
}

fn without_comment(line: &str) -> &str {
    let mut quoted = false;
    for (at, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..at],
            _ => {}
        }
    }
    line
}

fn quoted(line: &str, from: usize) -> Option<Range<usize>> {
    let open = line[from..].find('"')? + from + 1;
    let close = line[open..].find('"')? + open;
    Some(open..close)
}

fn key_value(line: &str, key: &str) -> Option<Range<usize>> {
    let mut from = 0;
    while let Some(found) = line[from..].find(key) {
        let start = from + found;
        from = start + key.len();
        let before = line[..start].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_') {
            continue;
        }
        let rest = line[from..].trim_start();
        if let Some(after) = rest.strip_prefix('=') {
            let value = after.trim_start();
            if value.starts_with('"') {
                return quoted(line, line.len() - value.len());
            }
        }
    }
    None
}

fn library(line: &str, base: usize, range: Range<usize>) -> Option<Library> {
    let (alias, rest) = line.split_once('=')?;
    let alias = alias.trim().to_owned();
    let value = rest.trim_start();
    let value_at = line.len() - value.len();
    let shift = |inside: Range<usize>| base + inside.start..base + inside.end;
    let mut found = Library { alias, line: range, ..Library::default() };
    if value.starts_with('"') {
        let inside = quoted(line, value_at)?;
        let text = &line[inside.clone()];
        let mut parts = text.splitn(3, ':');
        found.group = parts.next()?.to_owned();
        found.artifact = parts.next()?.to_owned();
        if let Some(version) = parts.next() {
            let start = inside.start + found.group.len() + found.artifact.len() + 2;
            found.version_range = Some(base + start..base + start + version.len());
            found.version = Some(version.to_owned());
        }
        return Some(found);
    }
    if let Some(module) = key_value(line, "module") {
        let (group, artifact) = line[module].split_once(':')?;
        found.group = group.to_owned();
        found.artifact = artifact.to_owned();
    } else {
        found.group = line[key_value(line, "group")?].to_owned();
        found.artifact = line[key_value(line, "name")?].to_owned();
    }
    if let Some(reference) = key_value(line, "version.ref").or_else(|| key_value(line, "ref")) {
        found.version_range = Some(shift(reference.clone()));
        found.version_ref = Some(line[reference].to_owned());
    } else if let Some(version) = key_value(line, "version") {
        found.version_range = Some(shift(version.clone()));
        found.version = Some(line[version].to_owned());
    }
    Some(found)
}

pub fn parse(text: &str) -> Catalog {
    let mut catalog = Catalog::default();
    let mut table = String::new();
    for (range, line) in lines(text) {
        let code = without_comment(line);
        let trimmed = code.trim();
        if trimmed.starts_with('[') {
            table = trimmed.trim_matches(['[', ']']).trim().to_owned();
            match table.as_str() {
                "libraries" => catalog.libraries_table = Some(range.end),
                "versions" => catalog.versions_table = Some(range.end),
                _ => {}
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        match table.as_str() {
            "versions" => {
                let Some((name, _)) = code.split_once('=') else { continue };
                if let Some(value) = key_value(code, name.trim()) {
                    let range = range.start + value.start..range.start + value.end;
                    catalog.versions.insert(name.trim().to_owned(), (code[value].to_owned(), range));
                }
            }
            "libraries" => catalog.libraries.extend(library(code, range.start, range.clone())),
            _ => {}
        }
    }
    catalog
}

pub fn accessor(alias: &str) -> String {
    alias.replace(['-', '_'], ".")
}

impl Library {
    pub fn resolved(&self, catalog: &Catalog) -> Option<String> {
        self.version.clone().or_else(|| {
            let reference = self.version_ref.as_ref()?;
            catalog.versions.get(reference).map(|(value, _)| value.clone())
        })
    }
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn new_alias(catalog: &Catalog, group: &str, artifact: &str) -> String {
    let base = artifact.replace('.', "-");
    if catalog.libraries.iter().all(|library| library.alias != base) {
        return base;
    }
    let owner = group.rsplit('.').next().unwrap_or(group);
    let longer = format!("{owner}-{base}");
    if catalog.libraries.iter().all(|library| library.alias != longer) {
        return longer;
    }
    (2..)
        .map(|at| format!("{longer}{at}"))
        .find(|alias| catalog.libraries.iter().all(|library| library.alias != *alias))
        .unwrap_or(longer)
}

fn insert(text: &mut String, at: usize, line: &str, separator: &str) {
    let mut lines = format!("{line}{separator}");
    if at > 0 && !text[..at].ends_with('\n') {
        lines = format!("{separator}{lines}");
    }
    text.insert_str(at, &lines);
}

fn table_end(text: &str, table: usize) -> usize {
    let mut end = table;
    for (range, line) in lines(&text[table..]) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break;
        }
        if !trimmed.is_empty() {
            end = table + range.end;
        }
    }
    end
}

pub fn add(text: &str, group: &str, artifact: &str, version: &str) -> (String, String) {
    let catalog = parse(text);
    let alias = new_alias(&catalog, group, artifact);
    let separator = newline(text);
    let by_reference = catalog.libraries.iter().any(|library| library.version_ref.is_some())
        || (catalog.libraries.is_empty() && catalog.versions_table.is_some());
    let mut out = text.to_owned();
    let entry = if version.is_empty() {
        format!("{alias} = {{ module = \"{group}:{artifact}\" }}")
    } else if by_reference {
        format!("{alias} = {{ module = \"{group}:{artifact}\", version.ref = \"{alias}\" }}")
    } else {
        format!("{alias} = {{ module = \"{group}:{artifact}\", version = \"{version}\" }}")
    };
    match catalog.libraries_table {
        Some(table) => {
            let at = table_end(&out, table);
            insert(&mut out, at, &entry, separator);
        }
        None => {
            let at = out.len();
            insert(&mut out, at, &format!("[libraries]{separator}{entry}"), separator);
        }
    }
    if by_reference && !version.is_empty() {
        let line = format!("{alias} = \"{version}\"");
        match parse(&out).versions_table {
            Some(table) => {
                let at = table_end(&out, table);
                insert(&mut out, at, &line, separator);
            }
            None => out.insert_str(0, &format!("[versions]{separator}{line}{separator}{separator}")),
        }
    }
    (out, alias)
}

pub fn set_version(text: &str, group: &str, artifact: &str, version: &str) -> Option<String> {
    let catalog = parse(text);
    let library = catalog.libraries.iter().find(|library| library.group == group && library.artifact == artifact)?;
    let range = match &library.version_ref {
        Some(reference) => catalog.versions.get(reference)?.1.clone(),
        None => library.version_range.clone()?,
    };
    let mut out = text.to_owned();
    out.replace_range(range, version);
    Some(out)
}

pub fn remove(text: &str, alias: &str) -> Option<String> {
    let catalog = parse(text);
    let library = catalog.libraries.iter().find(|library| library.alias == alias)?;
    let mut out = text.to_owned();
    out.replace_range(library.line.clone(), "");
    Some(out)
}
