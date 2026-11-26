use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Severity {
    pub score: Option<f64>,
    pub rating: Option<String>,
}

pub fn severity_of(osv: &Value) -> Option<Severity> {
    let score = osv["severity"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|severity| severity["type"].as_str().is_some_and(|kind| kind.starts_with("CVSS_V3")))
        .find_map(|severity| crate::cvss::base_score(severity["score"].as_str()?));
    let rating = osv["database_specific"]["severity"].as_str().map(str::to_owned);
    (score.is_some() || rating.is_some()).then_some(Severity { score, rating })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub introduced: Option<String>,
    pub fixed: Option<String>,
    pub last_affected: Option<String>,
}

impl Range {
    pub fn contains(&self, version: &str, compare: impl Fn(&str, &str) -> Ordering) -> bool {
        self.introduced.as_deref().is_none_or(|introduced| compare(version, introduced).is_ge())
            && self.fixed.as_deref().is_none_or(|fixed| compare(version, fixed).is_lt())
            && self.last_affected.as_deref().is_none_or(|last| compare(version, last).is_le())
    }
}

pub fn ranges_of(affected: &Value, kind: &str, normalize: &dyn Fn(&str) -> String) -> Vec<Range> {
    let mut ranges = Vec::new();
    for range in affected["ranges"].as_array().into_iter().flatten().filter(|range| range["type"] == kind) {
        let mut current: Option<Range> = None;
        for event in range["events"].as_array().into_iter().flatten() {
            if let Some(introduced) = event["introduced"].as_str() {
                ranges.extend(current.take());
                let introduced = (introduced != "0").then(|| normalize(introduced));
                current = Some(Range { introduced, ..Range::default() });
            } else if let Some(fixed) = event["fixed"].as_str() {
                let mut closed = current.take().unwrap_or_default();
                closed.fixed = Some(normalize(fixed));
                ranges.push(closed);
            } else if let Some(last) = event["last_affected"].as_str() {
                let mut closed = current.take().unwrap_or_default();
                closed.last_affected = Some(normalize(last));
                ranges.push(closed);
            }
        }
        ranges.extend(current);
    }
    ranges
}
