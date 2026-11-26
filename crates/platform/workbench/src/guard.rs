use std::path::{Component, Path};

use zen_i18n::t;

pub fn refuses_foreign_settings(path: &Path) -> bool {
    let in_folder = path.components().any(|component| match component {
        Component::Normal(name) => name == ".idea" || name == ".run",
        _ => false,
    });
    in_folder || path.extension().is_some_and(|extension| extension == "iml")
}

pub fn refuses_in(root: Option<&Path>, path: &Path) -> bool {
    refuses_foreign_settings(root.and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path))
}

pub fn refusal(path: &Path) -> String {
    t!("workbench-foreign-settings-refused", path = path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idea_project_files_are_refused() {
        for path in [
            "/p/.idea/workspace.xml",
            "/p/.idea/codeStyles/Project.xml",
            "/p/.idea",
            "/p/.run/Serve.run.xml",
            "/p/app.iml",
            "/p/sub/tools.iml",
            ".idea/misc.xml",
        ] {
            assert!(refuses_foreign_settings(Path::new(path)), "{path}");
        }
        for path in ["/p/main.go", "/p/idea/notes.md", "/p/.ideas/x", "/p/run/x.xml", "/p/app.iml.txt", "/p/.idea.go"] {
            assert!(!refuses_foreign_settings(Path::new(path)), "{path}");
        }
        let root = Path::new("/home/me/.run/project");
        assert!(!refuses_in(Some(root), &root.join("main.go")), "a project inside a .run folder is still writable");
        assert!(refuses_in(Some(root), &root.join(".run/Serve.run.xml")));
        assert!(refuses_in(None, Path::new("/p/.idea/misc.xml")));
    }
}
