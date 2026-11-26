use std::cmp::Ordering;

use serde_json::Value;

pub const DEFAULT_RELEASES: &str = "https://go.dev/dl/?mode=json";

pub fn stable(json: &[u8]) -> Vec<String> {
    let Ok(releases) = serde_json::from_slice::<Value>(json) else { return Vec::new() };
    let mut newest: Vec<String> = Vec::new();
    let versions = releases.as_array().into_iter().flatten().filter(|release| release["stable"] == true);
    for version in versions.filter_map(|release| release["version"].as_str()) {
        let version = version.trim_start_matches("go").to_owned();
        match newest.iter_mut().find(|kept| line(kept) == line(&version)) {
            Some(kept) if compare(&version, kept).is_gt() => *kept = version,
            Some(_) => {}
            None => newest.push(version),
        }
    }
    newest.sort_by(|a, b| compare(b, a));
    newest
}

pub fn latest(stable: &[String]) -> Option<&str> {
    stable.iter().max_by(|a, b| compare(a, b)).map(String::as_str)
}

pub fn newest_patch<'a>(stable: &'a [String], version: &str) -> Option<&'a str> {
    let wanted = line(version);
    stable.iter().filter(|release| line(release) == wanted).max_by(|a, b| compare(a, b)).map(String::as_str)
}

pub fn line(version: &str) -> (u64, u64) {
    let (numbers, _) = parts(version);
    (numbers.first().copied().unwrap_or(0), numbers.get(1).copied().unwrap_or(0))
}

pub fn all_releases(releases: &str) -> String {
    format!("{releases}{}include=all", if releases.contains('?') { '&' } else { '?' })
}

fn parts(version: &str) -> (Vec<u64>, Option<(String, u64)>) {
    let version = version.trim().trim_start_matches("go");
    let split = version.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(version.len());
    let (numbers, pre) = version.split_at(split);
    let numbers = numbers.split('.').filter(|part| !part.is_empty()).map(|part| part.parse().unwrap_or(0)).collect();
    let pre = (!pre.is_empty()).then(|| {
        let digits = pre.find(|c: char| c.is_ascii_digit()).unwrap_or(pre.len());
        (pre[..digits].to_owned(), pre[digits..].parse().unwrap_or(0))
    });
    (numbers, pre)
}

pub fn compare(a: &str, b: &str) -> Ordering {
    let (a_numbers, a_pre) = parts(a);
    let (b_numbers, b_pre) = parts(b);
    for index in 0..a_numbers.len().max(b_numbers.len()).max(2) {
        let order = a_numbers.get(index).unwrap_or(&0).cmp(b_numbers.get(index).unwrap_or(&0));
        if order != Ordering::Equal {
            return order;
        }
    }
    let rank = |numbers: &[u64], pre: &Option<(String, u64)>| match (numbers.len() <= 2, pre) {
        (true, None) => (0, String::new(), 0),
        (_, Some((kind, number))) => (1, kind.clone(), *number),
        (false, None) => (2, String::new(), 0),
    };
    rank(&a_numbers, &a_pre).cmp(&rank(&b_numbers, &b_pre))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_stable_release_is_found() {
        let json = br#"[{"version":"go1.28rc1","stable":false},{"version":"go1.27.2","stable":true},
            {"version":"go1.26.9","stable":true},{"version":"go1.26.10","stable":true}]"#;
        let releases = stable(json);
        assert_eq!(releases, ["1.27.2", "1.26.10"]);
        assert_eq!(latest(&releases), Some("1.27.2"));
        assert!(stable(b"not json").is_empty());
    }

    #[test]
    fn the_newest_patch_of_a_release_line_is_found() {
        let releases = vec!["1.27.3".to_owned(), "1.26.10".to_owned()];
        for version in ["1.26", "1.26.0", "1.26rc1", "go1.26.4"] {
            assert_eq!(newest_patch(&releases, version), Some("1.26.10"), "{version}");
        }
        assert_eq!(newest_patch(&releases, "1.27"), Some("1.27.3"));
        assert_eq!(newest_patch(&releases, "1.24.1"), None);
        assert_eq!(all_releases("https://go.dev/dl/?mode=json"), "https://go.dev/dl/?mode=json&include=all");
    }

    #[test]
    fn go_versions_are_ordered_as_the_go_command_orders_them() {
        assert_eq!(compare("1.21", "1.21rc1"), Ordering::Less);
        assert_eq!(compare("1.21rc1", "1.21rc2"), Ordering::Less);
        assert_eq!(compare("1.21rc2", "1.21.0"), Ordering::Less);
        assert_eq!(compare("1.21.0", "1.21.1"), Ordering::Less);
        assert_eq!(compare("1.9", "1.21"), Ordering::Less);
        assert_eq!(compare("go1.27.2", "1.27.2"), Ordering::Equal);
        assert_eq!(compare("1.27.2", "1.27"), Ordering::Greater);
    }
}
