use std::path::{Path, PathBuf};

use zen_project::settings::{ProjectSettings, ReadFiles, SourceFolder, SourceFolderKind};

use super::xml::{self, Macros};

pub(super) const ROOT_MANAGER: &str = "NewModuleRootManager";

pub(super) const RESOURCE_TYPE: &str = "java-resource";

pub(super) const TEST_RESOURCE_TYPE: &str = "java-test-resource";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleRoots {
    pub contents: Vec<PathBuf>,
    pub excluded: Vec<PathBuf>,
    pub patterns: Vec<String>,
    pub source_folders: Vec<SourceFolder>,
}

pub fn read(root: &Path, files: &dyn ReadFiles, sources: &[PathBuf], macros: &Macros, settings: &mut ProjectSettings) {
    for source in sources.iter().filter(|source| source.extension().is_some_and(|extension| extension == "iml")) {
        let Some(text) = files.read(source) else { continue };
        match roots(&text, &macros.for_module(&module_dir(root, source))) {
            Ok(found) => {
                settings.content_roots.extend(found.contents);
                settings.excluded.extend(found.excluded);
                settings.exclude_patterns.extend(found.patterns);
                settings.source_folders.extend(found.source_folders);
            }
            Err(error) => settings.problems.push(super::parse_problem(root, source, &error)),
        }
    }
    settings.excluded.sort();
    settings.excluded.dedup();
    settings.content_roots.sort();
    settings.content_roots.dedup();
    settings.source_folders.sort_by(|a, b| a.path.cmp(&b.path));
    settings.source_folders.dedup_by(|a, b| a.path == b.path);
    let mut seen = Vec::new();
    settings.exclude_patterns.retain(|pattern| {
        let new = !seen.contains(pattern);
        seen.push(pattern.clone());
        new
    });
}

pub fn module_dir(root: &Path, source: &Path) -> PathBuf {
    let folder = root.join(source).parent().map_or_else(|| root.to_owned(), Path::to_owned);
    if folder.file_name().is_some_and(|name| name == super::IDEA) {
        return folder.parent().map_or_else(|| root.to_owned(), Path::to_owned);
    }
    folder
}

pub(super) fn source_kind(node: &roxmltree::Node<'_, '_>) -> SourceFolderKind {
    match node.attribute("type") {
        Some(RESOURCE_TYPE) => SourceFolderKind::Resources,
        Some(TEST_RESOURCE_TYPE) => SourceFolderKind::TestResources,
        _ if node.attribute("isTestSource") == Some("true") => SourceFolderKind::TestSources,
        _ => SourceFolderKind::Sources,
    }
}

pub fn roots(text: &str, macros: &Macros) -> Result<ModuleRoots, String> {
    let document = xml::parse(text)?;
    let mut found = ModuleRoots::default();
    let Some(manager) = xml::component(&document, ROOT_MANAGER) else { return Ok(found) };
    for content in manager.children().filter(|node| node.has_tag_name("content")) {
        found.contents.extend(content.attribute("url").and_then(|url| macros.url(url)));
        for node in content.children() {
            if node.has_tag_name("excludeFolder") {
                found.excluded.extend(node.attribute("url").and_then(|url| macros.url(url)));
            } else if node.has_tag_name("excludePattern") {
                let written = node.attribute("pattern").unwrap_or_default();
                found
                    .patterns
                    .extend(written.split(';').map(str::trim).filter(|pattern| !pattern.is_empty()).map(str::to_owned));
            } else if node.has_tag_name("sourceFolder")
                && let Some(path) = node.attribute("url").and_then(|url| macros.url(url))
            {
                found.source_folders.push(SourceFolder { path, kind: source_kind(&node) });
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_dir_and_project_dir_resolve_and_patterns_split() {
        let text = r#"<module type="WEB_MODULE" version="4">
  <component name="NewModuleRootManager">
    <content url="file://$MODULE_DIR$">
      <sourceFolder url="file://$MODULE_DIR$/src" isTestSource="false" />
      <sourceFolder url="file://$MODULE_DIR$/test" isTestSource="true" />
      <sourceFolder url="file://$MODULE_DIR$/static" type="java-resource" />
      <sourceFolder url="file://$MODULE_DIR$/fixtures" type="java-test-resource" />
      <excludeFolder url="file://$MODULE_DIR$/out" />
      <excludeFolder url="file://$PROJECT_DIR$/shared/gen" />
      <excludeFolder url="jar://$MODULE_DIR$/lib.jar!/" />
      <excludePattern pattern="*.log; *.bak" />
      <excludePattern pattern="" />
    </content>
    <content url="file://$MODULE_DIR$/../extra">
      <excludeFolder url="file:///abs/elsewhere" />
    </content>
  </component>
</module>"#;
        let macros = Macros::new(Path::new("/p"), None).for_module(Path::new("/p/svc"));
        let found = roots(text, &macros).unwrap();
        assert_eq!(
            found.excluded,
            [PathBuf::from("/p/svc/out"), PathBuf::from("/p/shared/gen"), PathBuf::from("/abs/elsewhere")]
        );
        assert_eq!(found.patterns, ["*.log", "*.bak"]);
        assert_eq!(found.contents, [PathBuf::from("/p/svc"), PathBuf::from("/p/extra")]);
        let kinds: Vec<(PathBuf, SourceFolderKind)> =
            found.source_folders.iter().map(|folder| (folder.path.clone(), folder.kind)).collect();
        assert_eq!(
            kinds,
            [
                (PathBuf::from("/p/svc/src"), SourceFolderKind::Sources),
                (PathBuf::from("/p/svc/test"), SourceFolderKind::TestSources),
                (PathBuf::from("/p/svc/static"), SourceFolderKind::Resources),
                (PathBuf::from("/p/svc/fixtures"), SourceFolderKind::TestResources),
            ]
        );
    }

    #[test]
    fn a_module_file_in_dot_idea_stands_for_the_project_root() {
        let root = Path::new("/p");
        assert_eq!(module_dir(root, Path::new(".idea/p.iml")), PathBuf::from("/p"));
        assert_eq!(module_dir(root, Path::new("svc/svc.iml")), PathBuf::from("/p/svc"));
        assert_eq!(module_dir(root, Path::new("p.iml")), PathBuf::from("/p"));
    }

    #[test]
    fn a_module_without_a_root_manager_excludes_nothing_and_broken_xml_is_an_error() {
        let macros = Macros::new(Path::new("/p"), None);
        assert_eq!(roots(r#"<module><component name="Go" /></module>"#, &macros), Ok(ModuleRoots::default()));
        assert!(roots("<module", &macros).is_err());
    }
}
