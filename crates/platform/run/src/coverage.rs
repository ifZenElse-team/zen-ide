use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CommandLine, Configuration, RunContext};

/// How much of a line the tests ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LineStatus {
    /// Every part of the line ran.
    Covered,
    /// Some parts or branches of the line ran and others did not.
    Partly,
    /// Nothing on the line ran.
    NotCovered,
}

/// Branches of a line: how many ran out of how many there are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BranchCounts {
    pub covered: u32,
    pub total: u32,
}

/// One line with code the coverage tool measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineCoverage {
    /// The line, from 1.
    pub line: u32,
    /// How many times the line ran; the most of its parts.
    pub hits: u64,
    pub status: LineStatus,
    /// The line's branches, when the tool counts them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<BranchCounts>,
}

/// Lines in a row with the same status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoverageRange {
    /// The first line, from 1.
    pub start: u32,
    /// The last line, included.
    pub end: u32,
    pub status: LineStatus,
}

/// How many measured lines are covered, partly covered and not covered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoverageCounts {
    pub covered: u32,
    pub partly: u32,
    pub not_covered: u32,
    /// Branches that ran and branches in all, when the tool counts them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<BranchCounts>,
}

impl CoverageCounts {
    pub fn total(&self) -> u32 {
        self.covered + self.partly + self.not_covered
    }

    pub fn hit(&self) -> u32 {
        self.covered + self.partly
    }

    pub fn percent(&self) -> f64 {
        match self.total() {
            0 => 0.0,
            total => f64::from(self.hit()) * 100.0 / f64::from(total),
        }
    }

    pub fn add(&mut self, other: &CoverageCounts) {
        self.covered += other.covered;
        self.partly += other.partly;
        self.not_covered += other.not_covered;
        if let Some(branches) = other.branches {
            let mine = self.branches.get_or_insert_default();
            mine.covered += branches.covered;
            mine.total += branches.total;
        }
    }

    fn count(&mut self, line: &LineCoverage) {
        match line.status {
            LineStatus::Covered => self.covered += 1,
            LineStatus::Partly => self.partly += 1,
            LineStatus::NotCovered => self.not_covered += 1,
        }
        if let Some(branches) = line.branches {
            let mine = self.branches.get_or_insert_default();
            mine.covered += branches.covered;
            mine.total += branches.total;
        }
    }
}

/// The measured lines of one file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileCoverage {
    pub path: PathBuf,
    /// The measured lines, in order.
    pub lines: Vec<LineCoverage>,
}

impl FileCoverage {
    pub fn counts(&self) -> CoverageCounts {
        let mut counts = CoverageCounts::default();
        for line in &self.lines {
            counts.count(line);
        }
        counts
    }

    pub fn line(&self, line: u32) -> Option<&LineCoverage> {
        self.lines.binary_search_by_key(&line, |shown| shown.line).ok().map(|index| &self.lines[index])
    }

    pub fn ranges(&self) -> Vec<CoverageRange> {
        let mut ranges: Vec<CoverageRange> = Vec::new();
        for line in &self.lines {
            match ranges.last_mut() {
                Some(range) if range.status == line.status && range.end + 1 == line.line => range.end = line.line,
                _ => ranges.push(CoverageRange { start: line.line, end: line.line, status: line.status }),
            }
        }
        ranges
    }
}

/// What one run with coverage measured, file by file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoverageReport {
    /// The files, sorted by path.
    pub files: Vec<FileCoverage>,
}

impl CoverageReport {
    pub fn file(&self, path: &Path) -> Option<&FileCoverage> {
        self.files.binary_search_by(|file| file.path.as_path().cmp(path)).ok().map(|index| &self.files[index])
    }

    pub fn counts(&self) -> CoverageCounts {
        let mut counts = CoverageCounts::default();
        for file in &self.files {
            counts.add(&file.counts());
        }
        counts
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Measured {
    hits: u64,
    parts: u32,
    covered_parts: u32,
    branches: Option<BranchCounts>,
}

#[derive(Debug, Default)]
pub struct CoverageBuilder {
    files: BTreeMap<PathBuf, BTreeMap<u32, Measured>>,
}

impl CoverageBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn part(&mut self, path: &Path, lines: std::ops::RangeInclusive<u32>, hits: u64) {
        let file = self.files.entry(path.to_owned()).or_default();
        for line in lines {
            let measured = file.entry(line).or_default();
            measured.hits = measured.hits.max(hits);
            measured.parts += 1;
            measured.covered_parts += u32::from(hits > 0);
        }
    }

    pub fn branch(&mut self, path: &Path, line: u32, taken: bool) {
        let measured = self.files.entry(path.to_owned()).or_default().entry(line).or_default();
        let branches = measured.branches.get_or_insert_default();
        branches.total += 1;
        branches.covered += u32::from(taken);
    }

    pub fn build(self) -> CoverageReport {
        let files = self
            .files
            .into_iter()
            .map(|(path, lines)| FileCoverage {
                path,
                lines: lines
                    .into_iter()
                    .map(|(line, measured)| LineCoverage {
                        line,
                        hits: measured.hits,
                        status: status_of(&measured),
                        branches: measured.branches,
                    })
                    .collect(),
            })
            .collect();
        CoverageReport { files }
    }
}

fn status_of(measured: &Measured) -> LineStatus {
    let branches_missed = measured.branches.is_some_and(|branches| branches.covered < branches.total);
    let branches_hit = measured.branches.is_some_and(|branches| branches.covered > 0);
    let hit = measured.covered_parts > 0 || measured.hits > 0 || branches_hit;
    if !hit {
        LineStatus::NotCovered
    } else if measured.covered_parts < measured.parts || branches_missed {
        LineStatus::Partly
    } else {
        LineStatus::Covered
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub text: String,
}

pub fn parse_go_profile(text: &str, resolve: impl Fn(&str) -> PathBuf) -> Result<CoverageReport, ParseError> {
    let mut lines = text.lines().enumerate().filter(|(_, line)| !line.trim().is_empty());
    let set = match lines.next() {
        Some((_, first)) if first.starts_with("mode:") => first["mode:".len()..].trim() == "set",
        Some((index, first)) => return Err(ParseError { line: index + 1, text: first.to_owned() }),
        None => return Ok(CoverageReport::default()),
    };
    let mut blocks: BTreeMap<(String, u32, u32, u32, u32), u64> = BTreeMap::new();
    for (index, line) in lines {
        let error = || ParseError { line: index + 1, text: line.to_owned() };
        let block = go_block(line).ok_or_else(error)?;
        let (file, start, start_column, end, end_column, hits) = block;
        let kept = blocks.entry((file.to_owned(), start, start_column, end, end_column)).or_default();
        *kept = if set { (*kept).max(hits) } else { *kept + hits };
    }
    let mut builder = CoverageBuilder::new();
    let mut resolved: BTreeMap<String, PathBuf> = BTreeMap::new();
    for ((file, start, _, end, end_column), hits) in blocks {
        let path = resolved.entry(file.clone()).or_insert_with(|| resolve(&file));
        let end = if end_column <= 1 && end > start { end - 1 } else { end.max(start) };
        builder.part(path, start..=end, hits);
    }
    Ok(builder.build())
}

fn go_block(line: &str) -> Option<(&str, u32, u32, u32, u32, u64)> {
    let (place, numbers) = line.rsplit_once(':')?;
    let mut words = numbers.split_whitespace();
    let range = words.next()?;
    let _statements: u32 = words.next()?.parse().ok()?;
    let hits: u64 = words.next()?.parse().ok()?;
    let (from, to) = range.split_once(',')?;
    let (start, start_column) = from.split_once('.')?;
    let (end, end_column) = to.split_once('.')?;
    Some((place, start.parse().ok()?, start_column.parse().ok()?, end.parse().ok()?, end_column.parse().ok()?, hits))
}

pub fn parse_lcov(text: &str, base: &Path) -> Result<CoverageReport, ParseError> {
    let mut builder = CoverageBuilder::new();
    let mut file: Option<PathBuf> = None;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        let error = || ParseError { line: index + 1, text: line.to_owned() };
        if let Some(path) = line.strip_prefix("SF:") {
            let path = Path::new(path.trim());
            file = Some(if path.is_absolute() { path.to_owned() } else { base.join(path) });
        } else if line == "end_of_record" {
            file = None;
        } else if let Some(data) = line.strip_prefix("DA:") {
            let path = file.as_ref().ok_or_else(error)?;
            let mut fields = data.split(',');
            let number: u32 = fields.next().and_then(|field| field.trim().parse().ok()).ok_or_else(error)?;
            let hits: u64 = fields.next().and_then(|field| field.trim().parse().ok()).ok_or_else(error)?;
            builder.part(path, number..=number, hits);
        } else if let Some(data) = line.strip_prefix("BRDA:") {
            let path = file.as_ref().ok_or_else(error)?;
            let fields: Vec<&str> = data.split(',').map(str::trim).collect();
            let [number, _, _, taken] = fields.as_slice() else { return Err(error()) };
            let number: u32 = number.parse().map_err(|_| error())?;
            let taken = *taken != "-" && taken.parse::<u64>().map_err(|_| error())? > 0;
            builder.branch(path, number, taken);
        }
    }
    Ok(builder.build())
}

pub trait CoverageProvider: Send + Sync {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String>;

    fn read(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        report: &Path,
    ) -> Result<CoverageReport, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statuses(file: &FileCoverage) -> Vec<(u32, LineStatus, u64)> {
        file.lines.iter().map(|line| (line.line, line.status, line.hits)).collect()
    }

    #[test]
    fn a_go_profile_gives_lines_that_ran_partly_and_not_at_all() {
        let profile = "mode: count\n\
            example.com/calc/calc.go:3.24,5.2 1 4\n\
            example.com/calc/calc.go:7.24,8.12 1 2\n\
            example.com/calc/calc.go:8.12,10.3 1 0\n\
            example.com/calc/calc.go:11.2,11.10 1 2\n\
            example.com/calc/calc.go:14.20,16.2 1 0\n\
            example.com/calc/calc.go:18.2,20.1 1 0\n";
        let report = parse_go_profile(profile, |file| PathBuf::from("/src").join(file)).unwrap();
        assert_eq!(report.files.len(), 1);
        let file = &report.files[0];
        assert_eq!(file.path, PathBuf::from("/src/example.com/calc/calc.go"));
        use LineStatus::*;
        assert_eq!(
            statuses(file),
            [
                (3, Covered, 4),
                (4, Covered, 4),
                (5, Covered, 4),
                (7, Covered, 2),
                (8, Partly, 2),
                (9, NotCovered, 0),
                (10, NotCovered, 0),
                (11, Covered, 2),
                (14, NotCovered, 0),
                (15, NotCovered, 0),
                (16, NotCovered, 0),
                (18, NotCovered, 0),
                (19, NotCovered, 0),
            ]
        );
        let counts = file.counts();
        assert_eq!((counts.covered, counts.partly, counts.not_covered), (5, 1, 7));
        assert!((counts.percent() - 600.0 / 13.0).abs() < 1e-9);
        assert_eq!(
            file.ranges(),
            [
                CoverageRange { start: 3, end: 5, status: Covered },
                CoverageRange { start: 7, end: 7, status: Covered },
                CoverageRange { start: 8, end: 8, status: Partly },
                CoverageRange { start: 9, end: 10, status: NotCovered },
                CoverageRange { start: 11, end: 11, status: Covered },
                CoverageRange { start: 14, end: 16, status: NotCovered },
                CoverageRange { start: 18, end: 19, status: NotCovered },
            ]
        );
        assert_eq!(report.file(Path::new("/src/example.com/calc/calc.go")).and_then(|f| f.line(8)).unwrap().hits, 2);
    }

    #[test]
    fn repeated_go_blocks_add_up_in_count_mode_and_not_in_set_mode() {
        let blocks = "a/x.go:1.1,1.5 1 1\na/x.go:1.1,1.5 1 2\n";
        let count = parse_go_profile(&format!("mode: count\n{blocks}"), |file: &str| PathBuf::from(file)).unwrap();
        assert_eq!(count.files[0].lines[0].hits, 3);
        let set = parse_go_profile(&format!("mode: set\n{blocks}"), |file: &str| PathBuf::from(file)).unwrap();
        assert_eq!(set.files[0].lines[0].hits, 2);
    }

    #[test]
    fn a_broken_go_profile_names_its_line() {
        assert_eq!(parse_go_profile("", |file: &str| PathBuf::from(file)).unwrap(), CoverageReport::default());
        let error = parse_go_profile("x.go:1.1,2.2 1 1\n", |file: &str| PathBuf::from(file)).unwrap_err();
        assert_eq!(error.line, 1);
        let error = parse_go_profile("mode: set\nx.go:1.1,2.2 one 1\n", |file: &str| PathBuf::from(file)).unwrap_err();
        assert_eq!(error, ParseError { line: 2, text: "x.go:1.1,2.2 one 1".to_owned() });
    }

    #[test]
    fn lcov_gives_lines_and_their_branches() {
        let lcov = "TN:\nSF:src/lib.rs\nFN:1,add\nDA:1,3\nDA:2,3\nBRDA:2,0,0,3\nBRDA:2,0,1,-\nDA:3,0\nDA:5,1\n\
            BRDA:5,0,0,1\nBRDA:5,0,1,2\nend_of_record\nSF:/abs/main.rs\nDA:1,0\nend_of_record\n";
        let report = parse_lcov(lcov, Path::new("/project")).unwrap();
        let paths: Vec<&Path> = report.files.iter().map(|file| file.path.as_path()).collect();
        assert_eq!(paths, [Path::new("/abs/main.rs"), Path::new("/project/src/lib.rs")]);
        let lib = report.file(Path::new("/project/src/lib.rs")).unwrap();
        use LineStatus::*;
        assert_eq!(statuses(lib), [(1, Covered, 3), (2, Partly, 3), (3, NotCovered, 0), (5, Covered, 1)]);
        assert_eq!(lib.line(2).unwrap().branches, Some(BranchCounts { covered: 1, total: 2 }));
        let counts = report.counts();
        assert_eq!((counts.covered, counts.partly, counts.not_covered), (2, 1, 2));
        assert_eq!(counts.branches, Some(BranchCounts { covered: 3, total: 4 }));
        assert!(parse_lcov("DA:1,1\n", Path::new("/")).is_err(), "a line before its file");
        assert!(parse_lcov("SF:a.rs\nDA:x,1\n", Path::new("/")).is_err());
    }

    #[test]
    fn coverage_serialises_for_agents() {
        let line = LineCoverage { line: 4, hits: 2, status: LineStatus::NotCovered, branches: None };
        assert_eq!(
            serde_json::to_value(line).unwrap(),
            serde_json::json!({ "line": 4, "hits": 2, "status": "notCovered" })
        );
    }
}
