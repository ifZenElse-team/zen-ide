#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::HashSet;

const SSL_POLICY: &str = "KoZIhvdjZAED";

#[cfg(target_os = "macos")]
const SYSTEM_ROOTS: &str = "/System/Library/Keychains/SystemRootCertificates.keychain";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Trusted,
    Denied,
    Silent,
}

fn verdict_of(settings: &str) -> Verdict {
    let entries: Vec<&str> = settings.split("<dict>").skip(1).collect();
    if entries.is_empty() {
        return Verdict::Trusted;
    }
    for entry in entries {
        let policy = entry.contains("<key>kSecTrustSettingsPolicy</key>");
        let narrower = entry.contains("<key>kSecTrustSettingsPolicyString</key>")
            || entry.contains("<key>kSecTrustSettingsApplication</key>");
        if narrower || (policy && !entry.contains(SSL_POLICY)) {
            continue;
        }
        let result = entry.split("<key>kSecTrustSettingsResult</key>").nth(1).and_then(|rest| {
            let (_, value) = rest.split_once("<integer>")?;
            value.split_once("</integer>").map(|(value, _)| value.trim().to_owned())
        });
        match result.as_deref() {
            None | Some("1" | "2") => return Verdict::Trusted,
            Some("3") => return Verdict::Denied,
            _ => {}
        }
    }
    Verdict::Silent
}

fn verdicts(plist: &str) -> Vec<(String, Verdict)> {
    let Some(list) = plist.split("<key>trustList</key>").nth(1) else { return Vec::new() };
    let mut found = Vec::new();
    let mut rest = list;
    let is_hash = |key: &str| key.len() == 40 && key.bytes().all(|byte| byte.is_ascii_hexdigit());
    let mut keys: Vec<(usize, String)> = Vec::new();
    let mut offset = 0;
    while let Some(at) = rest.find("<key>") {
        let start = at + "<key>".len();
        let Some(end) = rest[start..].find("</key>") else { break };
        let key = &rest[start..start + end];
        if is_hash(key) {
            keys.push((offset + start + end, key.to_ascii_uppercase()));
        }
        offset += start + end;
        rest = &rest[start + end..];
    }
    for (index, (from, hash)) in keys.iter().enumerate() {
        let to = keys.get(index + 1).map_or(list.len(), |(next, _)| *next);
        let part = &list[*from..to];
        let settings = part
            .split_once("<key>trustSettings</key>")
            .map_or("", |(_, settings)| settings.split_once("</array>").map_or(settings, |(array, _)| array));
        found.push((hash.clone(), verdict_of(settings)));
    }
    found
}

pub(crate) fn trusted_hashes(plist: &str) -> HashSet<String> {
    verdicts(plist).into_iter().filter(|(_, verdict)| *verdict == Verdict::Trusted).map(|(hash, _)| hash).collect()
}

pub(crate) fn denied_hashes(plist: &str) -> HashSet<String> {
    verdicts(plist).into_iter().filter(|(_, verdict)| *verdict == Verdict::Denied).map(|(hash, _)| hash).collect()
}

pub(crate) fn hashed_certificates(listing: &str) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut hash = None;
    let mut rest = listing;
    loop {
        let next_hash = rest.find("SHA-1 hash: ");
        let next_pem = rest.find("-----BEGIN CERTIFICATE-----");
        match (next_hash, next_pem) {
            (Some(at), pem) if pem.is_none_or(|pem| at < pem) => {
                let start = at + "SHA-1 hash: ".len();
                let end = rest[start..].find('\n').map_or(rest.len(), |end| start + end);
                hash = Some(rest[start..end].trim().to_ascii_uppercase());
                rest = &rest[end..];
            }
            (_, Some(at)) => {
                let Some(length) = rest[at..].find("-----END CERTIFICATE-----") else { break };
                let end = at + length + "-----END CERTIFICATE-----".len();
                if let (Some(hash), Some(der)) =
                    (hash.take(), crate::trust::parse_certificates(&rest.as_bytes()[at..end]).pop())
                {
                    found.push((hash, der));
                }
                rest = &rest[end..];
            }
            _ => break,
        }
    }
    found
}

#[cfg(target_os = "macos")]
pub(crate) fn roots() -> Vec<Vec<u8>> {
    use std::process::Command;
    let security = |arguments: &[&str]| {
        Command::new("/usr/bin/security")
            .args(arguments)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| output.stdout)
    };
    let mut trusted = HashSet::new();
    let mut denied = HashSet::new();
    let folder = std::env::temp_dir().join(format!("zen-trust-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    for (name, flags) in [("user.plist", &[][..]), ("admin.plist", &["-d"][..])] {
        let file = folder.join(name);
        let path = file.display().to_string();
        let mut arguments = vec!["trust-settings-export"];
        arguments.extend_from_slice(flags);
        arguments.push(&path);
        if security(&arguments).is_some()
            && let Ok(plist) = std::fs::read_to_string(&file)
        {
            trusted.extend(trusted_hashes(&plist));
            denied.extend(denied_hashes(&plist));
        }
    }
    let _ = std::fs::remove_dir_all(&folder);
    let listed = |arguments: &[&str]| {
        let listing = security(arguments).unwrap_or_default();
        hashed_certificates(&String::from_utf8_lossy(&listing))
    };
    let system = listed(&["find-certificate", "-a", "-Z", "-p", SYSTEM_ROOTS]);
    let mut roots: Vec<Vec<u8>> =
        system.into_iter().filter(|(hash, _)| !denied.contains(hash)).map(|(_, der)| der).collect();
    let marked = listed(&["find-certificate", "-a", "-Z", "-p"]);
    roots.extend(
        marked.into_iter().filter(|(hash, _)| trusted.contains(hash) && !denied.contains(hash)).map(|(_, der)| der),
    );
    roots
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn roots() -> Vec<Vec<u8>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789ABCDEF0123456789ABCDEF01234567";
    const DENIED: &str = "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF";

    #[test]
    fn trust_settings_name_the_trusted_certificates_and_leave_out_the_denied() {
        let plist = format!(
            "<plist><dict><key>trustList</key><dict>\
             <key>{HASH}</key><dict><key>issuerName</key><data>AA</data></dict>\
             <key>{DENIED}</key><dict><key>trustSettings</key><array><dict>\
             <key>kSecTrustSettingsResult</key>\n<integer>3</integer></dict></array></dict>\
             </dict><key>trustVersion</key><integer>1</integer></dict></plist>"
        );
        assert_eq!(trusted_hashes(&plist), HashSet::from([HASH.to_owned()]));
    }

    fn entry(fields: &str) -> String {
        format!(
            "<plist><dict><key>trustList</key><dict><key>{HASH}</key><dict><key>trustSettings</key><array>\
             {fields}</array></dict></dict></dict></plist>"
        )
    }

    const SSL: &str = "<key>kSecTrustSettingsPolicy</key><data>KoZIhvdjZAED</data>\
         <key>kSecTrustSettingsPolicyName</key><string>sslServer</string>";
    const CODE: &str = "<key>kSecTrustSettingsPolicy</key><data>KoZIhvdjZAEQ</data>\
         <key>kSecTrustSettingsPolicyName</key><string>CodeSigning</string>";
    const ROOT: &str = "<key>kSecTrustSettingsResult</key><integer>1</integer>";

    #[test]
    fn only_trust_for_tls_makes_a_tls_root() {
        let trusted = |fields: &str| trusted_hashes(&entry(fields)).contains(HASH);
        assert!(!trusted(&format!("<dict>{CODE}{ROOT}</dict>")), "trusted for code signing only");
        assert!(trusted(&format!("<dict>{SSL}{ROOT}</dict>")));
        assert!(trusted(&format!("<dict>{CODE}{ROOT}</dict><dict>{SSL}{ROOT}</dict>")));
        assert!(!trusted(&format!("<dict>{SSL}<key>kSecTrustSettingsResult</key><integer>3</integer></dict>")));
        assert!(
            !trusted(&format!("<dict>{SSL}<key>kSecTrustSettingsResult</key><integer>4</integer></dict>")),
            "unspecified"
        );
        let for_one_host = "<key>kSecTrustSettingsPolicyString</key><string>a.example</string>";
        assert!(!trusted(&format!("<dict>{SSL}{for_one_host}{ROOT}</dict>")), "narrower than any host");
        let for_one_app = "<key>kSecTrustSettingsApplication</key><data>AA</data>";
        assert!(!trusted(&format!("<dict>{SSL}{for_one_app}{ROOT}</dict>")), "narrower than any program");
        assert!(trusted(""), "no settings: trusted for everything");
        assert!(trusted(&format!("<dict>{ROOT}</dict>")), "no policy: every policy");
        let denied = entry(&format!("<dict>{SSL}<key>kSecTrustSettingsResult</key><integer>3</integer></dict>"));
        assert_eq!(denied_hashes(&denied), HashSet::from([HASH.to_owned()]), "a denied system root stays out too");
    }

    #[test]
    fn an_empty_trust_list_trusts_nothing() {
        let plist =
            "<plist><dict><key>trustList</key><dict/><key>trustVersion</key><integer>1</integer></dict></plist>";
        assert!(trusted_hashes(plist).is_empty());
    }

    #[test]
    fn a_listing_pairs_each_hash_with_its_certificate() {
        let pem = include_str!("../tests/fixtures/ca.pem");
        let listing = format!("SHA-256 hash: AA\nSHA-1 hash: abcd\n{pem}SHA-1 hash: EF01\n{pem}");
        let found = hashed_certificates(&listing);
        assert_eq!(found.iter().map(|(hash, _)| hash.as_str()).collect::<Vec<_>>(), ["ABCD", "EF01"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_keychain_gives_the_system_roots() {
        assert!(roots().len() > 50);
    }
}
