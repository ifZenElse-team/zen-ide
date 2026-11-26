use std::process::Command;

const FIRST_YEAR: u64 = 26;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Calendar {
    pub(crate) year: u64,
    pub(crate) month: u64,
    pub(crate) number: u64,
}

impl std::fmt::Display for Calendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.year, self.month, self.number)
    }
}

pub(crate) fn parse(text: &str) -> Option<Calendar> {
    let parts: Vec<&str> = text.split('.').collect();
    let [year, month, number] = parts.as_slice() else { return None };
    let number_of = |part: &str| {
        (!part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) && !(part.len() > 1 && part.starts_with('0')))
            .then(|| part.parse::<u64>().ok())
            .flatten()
    };
    let calendar = Calendar { year: number_of(year)?, month: number_of(month)?, number: number_of(number)? };
    (calendar.year >= FIRST_YEAR && calendar.year < 100 && (1..=12).contains(&calendar.month) && calendar.number >= 1)
        .then_some(calendar)
}

pub(crate) fn is_calendar(version: &str) -> bool {
    version.split('.').next().and_then(|year| year.parse::<u64>().ok()).is_some_and(|year| year >= FIRST_YEAR)
}

pub(crate) fn next(year: u64, month: u64, published: &[String]) -> Calendar {
    let number = published
        .iter()
        .filter_map(|version| parse(version))
        .filter(|calendar| calendar.year == year && calendar.month == month)
        .map(|calendar| calendar.number)
        .max()
        .unwrap_or(0)
        + 1;
    Calendar { year, month, number }
}

pub(crate) fn this_month() -> Result<(u64, u64), String> {
    let output = Command::new("date").arg("+%y %m").output().map_err(|error| format!("cannot run date: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.split_whitespace().map(str::parse::<u64>);
    match (parts.next(), parts.next()) {
        (Some(Ok(year)), Some(Ok(month))) => Ok((year, month)),
        _ => Err(format!("cannot read today's date from `{}`", text.trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_versions_are_year_month_number() {
        assert_eq!(parse("26.10.1"), Some(Calendar { year: 26, month: 10, number: 1 }));
        assert_eq!(parse("27.1.12").unwrap().to_string(), "27.1.12");
        for bad in ["26.01.1", "26.13.1", "26.10.0", "25.10.1", "26.10", "26.10.1.1", "0.38.49", "26.x.1", "126.1.1"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert!(is_calendar("26.10.1") && !is_calendar("0.38.49"));
        assert!(parse("26.11.1") > parse("26.10.9") && parse("27.1.1") > parse("26.12.4"));
        let semver = |text: &str| semver::Version::parse(text).unwrap();
        assert!(semver("26.10.1") > semver("0.38.49"), "updates from before still work");
    }

    #[test]
    fn the_next_version_counts_within_the_month() {
        let published: Vec<String> = ["0.38.49", "26.10.1", "26.10.2", "26.9.7"].map(str::to_owned).to_vec();
        assert_eq!(next(26, 10, &published).to_string(), "26.10.3");
        assert_eq!(next(26, 11, &published).to_string(), "26.11.1", "a new month starts at 1");
        assert_eq!(next(27, 1, &[]).to_string(), "27.1.1");
    }
}
