use std::ops::Range;

use crate::{Fold, FoldKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegionComments<'a> {
    pub line: Option<&'a str>,
    pub block: Option<(&'a str, &'a str)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegionStyle {
    Region,
    EditorFold,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionMarker {
    Start { style: RegionStyle, description: String, collapsed: bool },
    End { style: RegionStyle },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomRegion {
    pub range: Range<usize>,
    pub end_marker: usize,
    pub style: RegionStyle,
    pub description: String,
    pub collapsed: bool,
}

pub fn region_marker(line: &str, comments: RegionComments<'_>) -> Option<RegionMarker> {
    let body = line.trim();
    let inner = comments
        .line
        .and_then(|start| body.strip_prefix(start))
        .or_else(|| {
            let (open, close) = comments.block?;
            let inner = body.strip_prefix(open)?.strip_suffix(close)?;
            (!inner.contains(close)).then_some(inner)
        })?
        .trim();
    let word = inner.strip_prefix('#').unwrap_or(inner);
    let after = |keyword: &str| {
        let rest = word.strip_prefix(keyword)?;
        (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
    };
    if after("endregion").is_some() {
        return Some(RegionMarker::End { style: RegionStyle::Region });
    }
    if let Some(description) = after("region") {
        return Some(RegionMarker::Start {
            style: RegionStyle::Region,
            description: description.to_owned(),
            collapsed: false,
        });
    }
    let tag = |name: &str| {
        let rest = inner.strip_prefix(name)?;
        rest.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/').then_some(rest)
    };
    if tag("</editor-fold").is_some() {
        return Some(RegionMarker::End { style: RegionStyle::EditorFold });
    }
    let attributes = tag("<editor-fold")?;
    let attribute = |name: &str| {
        let mut rest = attributes;
        while let Some(at) = rest.find(name) {
            let before = rest[..at].chars().next_back();
            let value = rest[at + name.len()..].trim_start().strip_prefix('=').map(str::trim_start);
            if before.is_some_and(char::is_whitespace)
                && let Some(value) = value
                && let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'')
            {
                let value = &value[1..];
                return Some(value[..value.find(quote).unwrap_or(value.len())].to_owned());
            }
            rest = &rest[at + name.len()..];
        }
        None
    };
    Some(RegionMarker::Start {
        style: RegionStyle::EditorFold,
        description: attribute("desc").unwrap_or_default(),
        collapsed: attribute("defaultstate").is_some_and(|state| state == "collapsed"),
    })
}

pub fn custom_regions(text: &str, comments: RegionComments<'_>) -> Vec<CustomRegion> {
    let mut scan = Scan { text, comments, at: 0, mode: Mode::Code };
    custom_regions_where(text, comments, |line, _| scan.in_code_at(line))
}

pub fn custom_regions_where(
    text: &str,
    comments: RegionComments<'_>,
    mut counts: impl FnMut(usize, usize) -> bool,
) -> Vec<CustomRegion> {
    if comments.line.is_none() && comments.block.is_none() {
        return Vec::new();
    }
    let starts: Vec<&str> = comments.line.into_iter().chain(comments.block.map(|(open, _)| open)).collect();
    let mut found: Vec<(usize, usize, usize, RegionStyle, String, bool)> = Vec::new();
    let mut open: Vec<(usize, RegionStyle, String, bool)> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let at = offset;
        offset += line.len();
        let content = line.trim_start();
        if !starts.iter().any(|start| content.starts_with(start)) {
            continue;
        }
        let Some(marker) = region_marker(line, comments) else { continue };
        let start = at + line.len() - content.len();
        if !counts(at, start) {
            continue;
        }
        match marker {
            RegionMarker::Start { style, description, collapsed } => open.push((start, style, description, collapsed)),
            RegionMarker::End { style } => {
                if let Some(index) = open.iter().rposition(|(_, opened, ..)| *opened == style) {
                    let (begin, style, description, collapsed) = open.remove(index);
                    open.truncate(index);
                    let end = start + content.trim_end().len();
                    found.push((begin, start, end, style, description, collapsed));
                }
            }
        }
    }
    let mut places: Vec<usize> = found.iter().flat_map(|(start, marker, end, ..)| [*start, *marker, *end]).collect();
    places.sort_unstable();
    places.dedup();
    let chars = char_offsets(text, &places);
    let char_of = |byte: usize| places.binary_search(&byte).map_or(0, |at| chars[at]);
    let mut regions: Vec<CustomRegion> = found
        .into_iter()
        .map(|(start, marker, end, style, description, collapsed)| CustomRegion {
            range: char_of(start)..char_of(end),
            end_marker: char_of(marker),
            style,
            description,
            collapsed,
        })
        .collect();
    regions.sort_by_key(|region| (region.range.start, std::cmp::Reverse(region.range.end)));
    regions
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Code,
    Block,
    Triple(char),
}

struct Scan<'a> {
    text: &'a str,
    comments: RegionComments<'a>,
    at: usize,
    mode: Mode,
}

impl Scan<'_> {
    fn in_code_at(&mut self, line: usize) -> bool {
        let text = self.text;
        while self.at < line {
            let end = text[self.at..].find('\n').map_or(text.len(), |at| self.at + at + 1).min(line);
            self.scan(&text[self.at..end]);
            self.at = end;
        }
        self.mode == Mode::Code
    }

    fn scan(&mut self, line: &str) {
        let mut rest = line;
        while let Some(c) = rest.chars().next() {
            match self.mode {
                Mode::Block | Mode::Triple(_) => {
                    let close = match self.mode {
                        Mode::Triple('"') => "\"\"\"",
                        Mode::Triple(_) => "'''",
                        _ => self.comments.block.map_or("", |(_, close)| close),
                    };
                    let Some(at) = rest.find(close).filter(|_| !close.is_empty()) else { return };
                    rest = &rest[at + close.len()..];
                    self.mode = Mode::Code;
                }
                Mode::Code if self.comments.line.is_some_and(|start| rest.starts_with(start)) => return,
                Mode::Code if self.comments.block.is_some_and(|(open, _)| rest.starts_with(open)) => {
                    rest = &rest[self.comments.block.map_or(0, |(open, _)| open.len())..];
                    self.mode = Mode::Block;
                }
                Mode::Code if rest.starts_with("\"\"\"") || rest.starts_with("'''") => {
                    rest = &rest[3..];
                    self.mode = Mode::Triple(c);
                }
                Mode::Code if c == '"' || c == '\'' => {
                    let body = &rest[1..];
                    let mut chars = body.char_indices();
                    let mut end = body.len();
                    while let Some((at, next)) = chars.next() {
                        if next == '\\' {
                            chars.next();
                        } else if next == c || next == '\n' {
                            end = at + next.len_utf8();
                            break;
                        }
                    }
                    rest = &body[end..];
                }
                Mode::Code => rest = &rest[c.len_utf8()..],
            }
        }
    }
}

fn char_offsets(text: &str, places: &[usize]) -> Vec<usize> {
    if text.is_ascii() {
        return places.to_vec();
    }
    let mut counted = (0, 0);
    places
        .iter()
        .map(|&place| {
            counted.1 += text[counted.0..place].chars().count();
            counted.0 = place;
            counted.1
        })
        .collect()
}

pub fn with_custom_regions(mut folds: Vec<Fold>, regions: &[CustomRegion]) -> Vec<Fold> {
    if regions.is_empty() {
        return folds;
    }
    let mut markers: Vec<usize> = regions.iter().flat_map(|region| [region.range.start, region.end_marker]).collect();
    markers.sort_unstable();
    folds.retain(|fold| {
        fold.kind != FoldKind::Comment || {
            let at = markers.partition_point(|marker| *marker < fold.range.start);
            markers.get(at).is_none_or(|marker| *marker >= fold.range.end)
        }
    });
    folds.extend(regions.iter().map(|region| Fold::new(region.range.clone(), FoldKind::Region)));
    folds.sort_by_key(|fold| (fold.range.start, std::cmp::Reverse(fold.range.end)));
    folds.dedup_by(|later, earlier| later.range == earlier.range);
    folds
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLASHES: RegionComments<'static> = RegionComments { line: Some("//"), block: Some(("/*", "*/")) };

    fn shown(text: &str, regions: &[CustomRegion]) -> Vec<(String, String)> {
        regions
            .iter()
            .map(|region| {
                let folded: String = text.chars().skip(region.range.start).take(region.range.len()).collect();
                (folded, region.description.clone())
            })
            .collect()
    }

    #[test]
    fn both_styles_nest_and_keep_their_names() {
        let text = "package main\n\n// region Helpers\nfunc a() {}\n\t//<editor-fold desc=\"Inner one\">\n\tx\n\t//</editor-fold>\n// endregion\n";
        let regions = custom_regions(text, SLASHES);
        assert_eq!(
            shown(text, &regions),
            [
                (
                    "// region Helpers\nfunc a() {}\n\t//<editor-fold desc=\"Inner one\">\n\tx\n\t//</editor-fold>\n// endregion"
                        .to_owned(),
                    "Helpers".to_owned()
                ),
                ("//<editor-fold desc=\"Inner one\">\n\tx\n\t//</editor-fold>".to_owned(), "Inner one".to_owned()),
            ]
        );
        assert_eq!(regions[0].style, RegionStyle::Region);
        assert_eq!(regions[1].style, RegionStyle::EditorFold);
        assert_eq!(regions[0].end_marker, text.find("// endregion").unwrap());
    }

    #[test]
    fn region_markers_are_read_in_both_comment_forms() {
        let start = |description: &str| {
            Some(RegionMarker::Start {
                style: RegionStyle::Region,
                description: description.to_owned(),
                collapsed: false,
            })
        };
        assert_eq!(region_marker("  //region", SLASHES), start(""));
        assert_eq!(region_marker("// region Name with spaces  \r\n", SLASHES), start("Name with spaces"));
        assert_eq!(region_marker("//#region Name", SLASHES), start("Name"));
        assert_eq!(region_marker("// #endregion", SLASHES), Some(RegionMarker::End { style: RegionStyle::Region }));
        assert_eq!(region_marker("/* region Styles */", SLASHES), start("Styles"));
        assert_eq!(region_marker("// regional", SLASHES), None);
        assert_eq!(region_marker("x := 1 // region", SLASHES), None, "not alone on its line");
        assert_eq!(region_marker("/// region", SLASHES), None, "a doc comment");
        assert_eq!(
            region_marker("// <editor-fold defaultstate=\"collapsed\" desc='A \"b\"'>", SLASHES),
            Some(RegionMarker::Start {
                style: RegionStyle::EditorFold,
                description: "A \"b\"".to_owned(),
                collapsed: true
            })
        );
        assert_eq!(
            region_marker("//<editor-fold>", SLASHES),
            Some(RegionMarker::Start { style: RegionStyle::EditorFold, description: String::new(), collapsed: false })
        );
        assert_eq!(region_marker("// <editor-folds>", SLASHES), None);
        let hash = RegionComments { line: Some("#"), block: None };
        assert_eq!(region_marker("# region Setup", hash), start("Setup"));
        assert_eq!(region_marker("#region Setup", hash), start("Setup"), "C# and PowerShell");
        let html = RegionComments { line: None, block: Some(("<!--", "-->")) };
        assert_eq!(region_marker("<!-- region Header -->", html), start("Header"));
        assert_eq!(region_marker("<!-- region --> x <!-- -->", html), None);
    }

    #[test]
    fn unpaired_comments_fold_nothing_and_styles_pair_with_their_own() {
        let text = "// endregion\n// region A\n//<editor-fold>\n// endregion\nx\n//</editor-fold>\n// region open\n";
        let regions = custom_regions(text, SLASHES);
        assert_eq!(shown(text, &regions), [("// region A\n//<editor-fold>\n// endregion".to_owned(), "A".to_owned())]);
        assert!(custom_regions(text, RegionComments::default()).is_empty());
    }

    #[test]
    fn offsets_count_characters_and_other_comment_syntaxes_work() {
        let text = "ё = 1\n-- region Запросы\nselect 1;\n-- endregion\n";
        let regions = custom_regions(text, RegionComments { line: Some("--"), block: None });
        assert_eq!(
            shown(text, &regions),
            [("-- region Запросы\nselect 1;\n-- endregion".to_owned(), "Запросы".to_owned())]
        );
        let css = "a {}\n/* region Buttons */\n.b {}\n/* endregion */\n";
        let regions = custom_regions(css, RegionComments { line: None, block: Some(("/*", "*/")) });
        assert_eq!(shown(css, &regions)[0].1, "Buttons");
    }

    #[test]
    fn region_comments_in_strings_and_block_comments_do_not_count() {
        let c = "// region Real\n/*\n// endregion\n*/\nx := \"/*\"\ny := '\"'\n// endregion\n// region\n";
        assert_eq!(
            shown(c, &custom_regions(c, SLASHES)),
            [(c[..c.rfind("// endregion").unwrap() + "// endregion".len()].to_owned(), "Real".to_owned())]
        );
        let python = "# region Real\ndoc = \"\"\"\n# endregion\n\"\"\"\nquote = '# it\\'s' # don't\nraw = '''\n# endregion\n'''\n# endregion\n";
        let hash = RegionComments { line: Some("#"), block: None };
        assert_eq!(shown(python, &custom_regions(python, hash)), [(python.trim_end().to_owned(), "Real".to_owned())]);
        let all = custom_regions_where(c, SLASHES, |_, _| true);
        assert_eq!(all[0].end_marker, c.find("// endregion").unwrap(), "what the caller accepts counts");
    }

    #[test]
    fn regions_join_the_folds_and_replace_the_comment_runs_they_touch() {
        let text = "// region A\n// Run starts\nfunc Run() {\n\tx()\n}\n// endregion\n// region B\n// endregion\n\n// one\n// two\n";
        let regions = custom_regions(text, SLASHES);
        let all = with_custom_regions(crate::folds(text), &regions);
        let kinds: Vec<(String, FoldKind)> = all
            .iter()
            .map(|fold| (text.chars().skip(fold.range.start).take(fold.range.len()).collect(), fold.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("// region A\n// Run starts\nfunc Run() {\n\tx()\n}\n// endregion".to_owned(), FoldKind::Region),
                ("\n\tx()\n".to_owned(), FoldKind::Block),
                ("// region B\n// endregion".to_owned(), FoldKind::Region),
                ("// one\n// two".to_owned(), FoldKind::Comment),
            ]
        );
    }
}
