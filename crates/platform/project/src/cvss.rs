pub fn base_score(vector: &str) -> Option<f64> {
    let mut parts = vector.split('/');
    if !parts.next()?.starts_with("CVSS:3") {
        return None;
    }
    let metric = |name: &str| -> Option<String> {
        vector.split('/').find_map(|part| part.strip_prefix(name)?.strip_prefix(':').map(str::to_owned))
    };
    let changed = match metric("S")?.as_str() {
        "U" => false,
        "C" => true,
        _ => return None,
    };
    let attack_vector = match metric("AV")?.as_str() {
        "N" => 0.85,
        "A" => 0.62,
        "L" => 0.55,
        "P" => 0.2,
        _ => return None,
    };
    let complexity = match metric("AC")?.as_str() {
        "L" => 0.77,
        "H" => 0.44,
        _ => return None,
    };
    let privileges = match (metric("PR")?.as_str(), changed) {
        ("N", _) => 0.85,
        ("L", false) => 0.62,
        ("L", true) => 0.68,
        ("H", false) => 0.27,
        ("H", true) => 0.5,
        _ => return None,
    };
    let interaction = match metric("UI")?.as_str() {
        "N" => 0.85,
        "R" => 0.62,
        _ => return None,
    };
    let impact_of = |name: &str| -> Option<f64> {
        match metric(name)?.as_str() {
            "H" => Some(0.56),
            "L" => Some(0.22),
            "N" => Some(0.0),
            _ => None,
        }
    };
    let (c, i, a) = (impact_of("C")?, impact_of("I")?, impact_of("A")?);
    let iss = 1.0 - (1.0 - c) * (1.0 - i) * (1.0 - a);
    let impact = if changed { 7.52 * (iss - 0.029) - 3.25 * (iss - 0.02).powi(15) } else { 6.42 * iss };
    let exploitability = 8.22 * attack_vector * complexity * privileges * interaction;
    if impact <= 0.0 {
        return Some(0.0);
    }
    let sum = if changed { 1.08 * (impact + exploitability) } else { impact + exploitability };
    Some(round_up(sum.min(10.0)))
}

fn round_up(value: f64) -> f64 {
    let whole = (value * 100_000.0).round() as i64;
    if whole % 10_000 == 0 { whole as f64 / 100_000.0 } else { ((whole / 10_000) + 1) as f64 / 10.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors_score_as_published() {
        let score = |vector| base_score(vector).unwrap();
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:N"), 9.1);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"), 9.8);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H"), 7.5);
        assert_eq!(score("CVSS:3.0/AV:N/AC:L/PR:L/UI:R/S:C/C:L/I:L/A:N"), 5.4);
        assert_eq!(score("CVSS:3.1/AV:L/AC:H/PR:H/UI:R/S:U/C:N/I:N/A:N"), 0.0);
        assert_eq!(base_score("CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N/VC:H/VI:H/VA:H/SC:N/SI:N/SA:N"), None);
        assert_eq!(base_score("CVSS:3.1/AV:N"), None);
    }
}
