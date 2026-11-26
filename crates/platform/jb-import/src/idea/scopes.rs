use std::path::{Path, PathBuf};

use zen_project::settings::{ProjectSettings, ReadFiles, Scope};

use super::xml;

const SCOPES: &str = ".idea/scopes";
const MANAGER: &str = "DependencyValidationManager";

pub fn read(root: &Path, files: &dyn ReadFiles, sources: &[PathBuf], settings: &mut ProjectSettings) {
    for source in sources.iter().filter(|source| source.parent() == Some(Path::new(SCOPES))) {
        let Some(text) = files.read(source) else { continue };
        match scopes(&text) {
            Ok(found) => settings.scopes.extend(found),
            Err(error) => settings.problems.push(super::parse_problem(root, source, &error)),
        }
    }
}

pub fn scopes(text: &str) -> Result<Vec<Scope>, String> {
    let document = xml::parse(text)?;
    let Some(manager) = xml::component(&document, MANAGER) else { return Ok(Vec::new()) };
    Ok(manager
        .children()
        .filter(|node| node.has_tag_name("scope"))
        .filter_map(|node| {
            let name = node.attribute("name")?.to_owned();
            Some(Scope { name, pattern: node.attribute("pattern").unwrap_or_default().to_owned() })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_keep_their_patterns_as_written() {
        let text = r#"<component name="DependencyValidationManager">
  <scope name="Tests" pattern="file:*_test.go&amp;&amp;!file:vendor//*" />
  <scope pattern="file:*.go" />
  <scope name="Empty" />
</component>"#;
        let found = scopes(text).unwrap();
        assert_eq!(
            found,
            [
                Scope { name: "Tests".to_owned(), pattern: "file:*_test.go&&!file:vendor//*".to_owned() },
                Scope { name: "Empty".to_owned(), pattern: String::new() },
            ]
        );
        assert_eq!(scopes(r#"<project><component name="Other" /></project>"#), Ok(vec![]));
        assert!(scopes("<component name=").is_err());
    }
}
