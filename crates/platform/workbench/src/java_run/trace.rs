use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zen_run::{OutputParser, ParsedLine, Stream, TestEvent};

const SOURCE_FOLDERS: [&str; 6] = ["src/main/java", "src/test/java", "src/main/kotlin", "src/test/kotlin", "src", "."];

pub(super) struct TraceParser {
    roots: Vec<PathBuf>,
    found: HashMap<(String, String), Option<PathBuf>>,
}

impl TraceParser {
    pub(super) fn new(root: &Path, module: &str) -> Self {
        let mut modules = vec![root.join(module), root.to_owned()];
        if let Ok(entries) = std::fs::read_dir(root) {
            let mut children: Vec<PathBuf> =
                entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
            children.sort();
            modules.extend(children);
        }
        let mut roots = Vec::new();
        for module in modules {
            for folder in SOURCE_FOLDERS {
                let folder = module.join(folder);
                if folder.is_dir() && !roots.contains(&folder) {
                    roots.push(folder);
                }
            }
        }
        Self { roots, found: HashMap::new() }
    }

    fn source(&mut self, class: &str, file: &str) -> Option<PathBuf> {
        let key = (class.to_owned(), file.to_owned());
        if let Some(found) = self.found.get(&key) {
            return found.clone();
        }
        let package: Vec<&str> = class.split('.').collect();
        let folder: PathBuf = package[..package.len().saturating_sub(1)].iter().collect();
        let found = self.roots.iter().map(|root| root.join(&folder).join(file)).find(|path| path.is_file());
        self.found.insert(key, found.clone());
        found
    }
}

pub(super) fn frame(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix("at ")?;
    let (method, location) = rest.split_once('(')?;
    let method = method.rsplit('/').next()?;
    let (class, _) = method.rsplit_once('.')?;
    let file = location.split(':').next()?;
    (file.ends_with(".java") || file.ends_with(".kt")).then(|| (class.to_owned(), file.to_owned()))
}

impl OutputParser for TraceParser {
    fn line(&mut self, stream: Stream, text: &str) -> ParsedLine {
        let mut parsed = ParsedLine::console(stream, text);
        if let Some((class, file)) = frame(text)
            && let Some(source) = self.source(&class, &file)
            && let Some(console) = &mut parsed.console
        {
            for link in console.links.iter_mut().filter(|link| link.file == Path::new(&file)) {
                link.file = source.clone();
            }
        }
        parsed
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        Vec::new()
    }
}
