use std::path::{Path, PathBuf};

use zen_i18n::t;

use crate::build_sync::{Severity, SyncFix, SyncMessage, attach_fixes, jdk_fix, jdk_trouble, network_trouble};

pub fn download_fix() -> SyncFix {
    SyncFix {
        title: t!("workbench-sync-fix-download-gradle"),
        command: "gradle.distribution.download".to_owned(),
        params: serde_json::json!({}),
        reload: false,
    }
}

pub fn offline_fix() -> SyncFix {
    SyncFix {
        title: t!("workbench-sync-fix-offline"),
        command: "gradle.options.set".to_owned(),
        params: serde_json::json!({ "offline": true }),
        reload: true,
    }
}

fn place(line: &str) -> Option<(PathBuf, Option<u32>)> {
    let start = line.find('\'')? + 1;
    let end = start + line[start..].find('\'')?;
    let number = line[end..].split_once("line:").and_then(|(_, rest)| rest.trim().parse().ok());
    Some((PathBuf::from(&line[start..end]), number))
}

fn compiler_message(line: &str) -> Option<SyncMessage> {
    let (severity, rest) = match line.split_once(": ")? {
        ("e", rest) => (Severity::Error, rest),
        ("w", rest) => (Severity::Warning, rest),
        _ => return None,
    };
    let rest = rest.strip_prefix("file://").unwrap_or(rest);
    let mut parts = rest.splitn(4, ':');
    let (file, line, column) = (parts.next()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok());
    let text = parts.next().map_or(rest, str::trim);
    Some(SyncMessage::new(severity, text).at(PathBuf::from(file), Some(line), column))
}

pub fn problems(text: &str, _root: &Path) -> Vec<SyncMessage> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut found: Vec<SyncMessage> = Vec::new();
    let mut where_at = None;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        if line == "* Where:" {
            where_at = lines.get(index + 1).and_then(|next| place(next));
        } else if line == "* What went wrong:" {
            let said: Vec<&str> = lines[index + 1..]
                .iter()
                .map(|said| said.trim())
                .take_while(|said| !said.is_empty() && !said.starts_with("* "))
                .collect();
            let mut message = SyncMessage::new(Severity::Error, said.join(" "));
            if let Some((file, line)) = where_at.take() {
                message = message.at(file, line, None);
            }
            if !message.text.is_empty() {
                found.push(message);
            }
            index += said.len();
        } else if let Some(message) = compiler_message(line).filter(|m| !found.iter().any(|known| known.text == m.text))
        {
            found.push(message);
        }
        index += 1;
    }
    attach_fixes(&mut found, text, &[(network_trouble, offline_fix()), (jdk_trouble, jdk_fix())]);
    found
}
