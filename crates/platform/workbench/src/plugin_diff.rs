use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use zen_core::Session;
use zen_i18n::t;
use zen_run::{Stream, output::SharedOutput};

pub const WAIT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffObject {
    pub object: String,
    pub left: String,
    pub right: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffResult {
    pub run_id: u64,
    pub file: String,
    pub place: String,
    pub objects: Vec<DiffObject>,
    pub error: Option<String>,
}

impl DiffResult {
    pub fn left_title(&self, object: &DiffObject) -> String {
        if self.place.is_empty() {
            return t!("workbench-diff-live-plain", object = object.object.as_str());
        }
        t!("workbench-diff-live", object = object.object.as_str(), place = self.place.as_str())
    }

    pub fn right_title(&self, object: &DiffObject) -> String {
        t!("workbench-diff-file", object = object.object.as_str(), file = self.file.as_str())
    }
}

#[derive(Clone, Default)]
pub struct PluginDiffs {
    ready: Arc<Mutex<Vec<DiffResult>>>,
}

pub fn take(session: &mut Session) -> Vec<DiffResult> {
    let Some(diffs) = session.state::<PluginDiffs>() else { return Vec::new() };
    std::mem::take(&mut *diffs.ready.lock().unwrap_or_else(PoisonError::into_inner))
}

fn object_name(header: &str) -> String {
    let path = header.split('\t').next().unwrap_or(header).trim();
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

pub fn parse_unified(text: &str) -> Vec<DiffObject> {
    let mut objects: Vec<DiffObject> = Vec::new();
    let mut current: Option<DiffObject> = None;
    let mut in_hunk = false;
    let lines: Vec<&str> = text.lines().collect();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if let (Some(old), Some(new)) =
            (line.strip_prefix("--- "), lines.get(at + 1).and_then(|next| next.strip_prefix("+++ ")))
        {
            if let Some(done) = current.take() {
                objects.push(done);
            }
            let name = object_name(new);
            let name = if name.is_empty() || name == "null" { object_name(old) } else { name };
            current = Some(DiffObject { object: name, ..DiffObject::default() });
            in_hunk = false;
            at += 2;
            continue;
        }
        if line.starts_with("@@") {
            in_hunk = current.is_some();
        } else if in_hunk && let Some(object) = current.as_mut() {
            match line.chars().next() {
                Some(' ') => {
                    object.left.push_str(&line[1..]);
                    object.left.push('\n');
                    object.right.push_str(&line[1..]);
                    object.right.push('\n');
                }
                Some('-') => {
                    object.left.push_str(&line[1..]);
                    object.left.push('\n');
                }
                Some('+') => {
                    object.right.push_str(&line[1..]);
                    object.right.push('\n');
                }
                Some('\\') => {}
                None => {
                    object.left.push('\n');
                    object.right.push('\n');
                }
                _ => in_hunk = false,
            }
        }
        at += 1;
    }
    if let Some(done) = current.take() {
        objects.push(done);
    }
    objects
}

pub(crate) fn watch(session: &mut Session, run_id: u64, output: SharedOutput, file: String, place: String) {
    let ready = Arc::clone(&session.state_mut::<PluginDiffs>().ready);
    std::thread::spawn(move || {
        output.wait_end(WAIT);
        let (text, exit, error) = output.read(|output| {
            let text: String = output
                .console()
                .lines()
                .iter()
                .filter(|line| line.stream == Stream::Stdout)
                .map(|line| format!("{}\n", line.text))
                .collect();
            (text, output.exit_code(), output.error().map(str::to_owned))
        });
        let mut result = DiffResult { run_id, file, place, ..DiffResult::default() };
        match exit {
            Some(0) => {}
            Some(1) => result.objects = parse_unified(&text),
            _ => result.error = Some(error.unwrap_or_else(|| t!("workbench-diff-failed", code = exit.unwrap_or(-1)))),
        }
        let line = match (&result.error, result.objects.len()) {
            (Some(_), _) => None,
            (None, 0) => Some(t!("workbench-diff-same")),
            (None, count) => Some(t!("workbench-diff-opened", count = count)),
        };
        if let Some(line) = line {
            output.update(|output| output.system(&line));
        }
        ready.lock().unwrap_or_else(PoisonError::into_inner).push(result);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const KUBECTL_DIFF: &str = concat!(
        "diff -N -U1000000 /tmp/LIVE-1/apps.v1.Deployment.shop.web /tmp/MERGED-1/apps.v1.Deployment.shop.web\n",
        "--- /tmp/LIVE-1/apps.v1.Deployment.shop.web\t2026-11-16 10:00:00\n",
        "+++ /tmp/MERGED-1/apps.v1.Deployment.shop.web\t2026-11-16 10:00:01\n",
        "@@ -1,4 +1,4 @@\n",
        " apiVersion: apps/v1\n",
        " kind: Deployment\n",
        "-spec: {replicas: 2}\n",
        "+spec: {replicas: 3}\n",
        " \n",
        "diff -N -U1000000 /tmp/LIVE-1/v1.ConfigMap.shop.settings /tmp/MERGED-1/v1.ConfigMap.shop.settings\n",
        "--- /tmp/LIVE-1/v1.ConfigMap.shop.settings\t2026-11-16 10:00:00\n",
        "+++ /tmp/MERGED-1/v1.ConfigMap.shop.settings\t2026-11-16 10:00:01\n",
        "@@ -0,0 +1,2 @@\n",
        "+kind: ConfigMap\n",
        "+data: {a: b}\n",
        "\\ No newline at end of file\n",
    );

    #[test]
    fn a_full_context_unified_diff_gives_both_sides_of_each_object() {
        let objects = parse_unified(KUBECTL_DIFF);
        assert_eq!(
            objects,
            [
                DiffObject {
                    object: "apps.v1.Deployment.shop.web".into(),
                    left: "apiVersion: apps/v1\nkind: Deployment\nspec: {replicas: 2}\n\n".into(),
                    right: "apiVersion: apps/v1\nkind: Deployment\nspec: {replicas: 3}\n\n".into(),
                },
                DiffObject {
                    object: "v1.ConfigMap.shop.settings".into(),
                    left: String::new(),
                    right: "kind: ConfigMap\ndata: {a: b}\n".into(),
                },
            ],
            "a new object has an empty left side"
        );
        assert!(parse_unified("").is_empty());
        assert!(parse_unified("error: the server could not be reached\n").is_empty());
    }
}
