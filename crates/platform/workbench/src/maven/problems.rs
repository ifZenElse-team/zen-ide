use std::path::{Path, PathBuf};

use zen_i18n::t;

use crate::build_sync::{Severity, SyncFix, SyncMessage, attach_fixes, jdk_fix, jdk_trouble, network_trouble};

const NOISE: [&str; 10] = [
    "[ERROR]",
    "Some problems were encountered",
    "-> [Help",
    "To see the full stack trace",
    "Re-run Maven",
    "For more information",
    "[Help ",
    "After correcting the problems",
    "The build could not read",
    "Failed to execute goal",
];

pub fn download_fix() -> SyncFix {
    SyncFix {
        title: t!("workbench-sync-fix-download-maven"),
        command: "maven.distribution.download".to_owned(),
        params: serde_json::json!({}),
        reload: false,
    }
}

pub fn offline_fix() -> SyncFix {
    SyncFix {
        title: t!("workbench-sync-fix-offline"),
        command: "maven.options.set".to_owned(),
        params: serde_json::json!({ "offline": true }),
        reload: true,
    }
}

fn number_after(text: &str, marker: &str) -> Option<u32> {
    let at = text.rfind(marker)? + marker.len();
    let digits: String = text[at..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn pom_in(text: &str) -> Option<PathBuf> {
    let end = text.find("pom.xml")? + "pom.xml".len();
    let start = text[..end].rfind([' ', '(', '\'', '"']).map_or(0, |at| at + 1);
    let path = PathBuf::from(&text[start..end]);
    path.is_absolute().then_some(path)
}

fn project_pom(line: &str) -> Option<PathBuf> {
    let rest = line.split_once("The project ")?.1;
    rest.contains(" has ").then(|| pom_in(rest)).flatten()
}

pub fn problems(text: &str, root: &Path) -> Vec<SyncMessage> {
    let fallback = text.lines().find_map(project_pom).unwrap_or_else(|| root.join("pom.xml"));
    let mut found: Vec<SyncMessage> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let (severity, rest) = match trimmed.strip_prefix("[ERROR]").or_else(|| trimmed.strip_prefix("[FATAL]")) {
            Some(rest) => (Severity::Error, rest.trim()),
            None => match trimmed.strip_prefix("[WARNING]") {
                Some(rest) if rest.contains("pom.xml") || rest.contains("@ line") => (Severity::Warning, rest.trim()),
                _ => continue,
            },
        };
        let rest = rest.find(" -> [Help").map_or(rest, |at| rest[..at].trim_end());
        if rest.is_empty() || NOISE.iter().any(|noise| rest.starts_with(noise)) || project_pom(rest).is_some() {
            continue;
        }
        if found.iter().any(|known| known.text == rest) {
            continue;
        }
        let line_number = number_after(rest, "@ line ").or_else(|| number_after(rest, ", line "));
        let column = number_after(rest, ", column ");
        let file = rest.rsplit_once("@ ").and_then(|(_, tail)| pom_in(tail)).unwrap_or_else(|| fallback.clone());
        found.push(SyncMessage::new(severity, rest).at(file, line_number, column));
    }
    attach_fixes(&mut found, text, &[(network_trouble, offline_fix()), (jdk_trouble, jdk_fix())]);
    found
}

pub fn pom_errors(project: &super::pom::PomProject) -> Vec<SyncMessage> {
    let mut found = Vec::new();
    let mut stack = vec![project];
    while let Some(project) = stack.pop() {
        if let Some(error) = &project.error {
            let line = error.rsplit_once(" at ").and_then(|(_, place)| place.split(':').next()?.trim().parse().ok());
            let column = error.rsplit_once(" at ").and_then(|(_, place)| place.split(':').nth(1)?.trim().parse().ok());
            found.push(SyncMessage::new(Severity::Error, error.clone()).at(project.pom.clone(), line, column));
        }
        stack.extend(project.modules.iter());
    }
    found
}
