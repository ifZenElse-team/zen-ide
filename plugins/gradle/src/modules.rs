use serde_json::{Value, json};

const FORBIDDEN: [char; 5] = ['"', '\'', '$', '\\', '\n'];

fn texts(language: &str, english: &'static str, russian: &'static str) -> &'static str {
    if language == "ru" { russian } else { english }
}

fn is_kotlin(file: &str) -> bool {
    file.ends_with(".kts")
}

fn project_path(path: &str) -> String {
    path.replace('/', ":")
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn include_lines(path: &str, kotlin: bool) -> String {
    let above = path.starts_with("..");
    let name = if above { name_of(path).to_owned() } else { project_path(path) };
    let include = if kotlin { format!("include(\"{name}\")\n") } else { format!("include '{name}'\n") };
    if !above {
        return include;
    }
    if kotlin {
        format!("{include}project(\":{name}\").projectDir = file(\"{path}\")\n")
    } else {
        format!("{include}project(':{name}').projectDir = file('{path}')\n")
    }
}

fn is_listed(text: &str, path: &str) -> bool {
    let name = if path.starts_with("..") { name_of(path).to_owned() } else { project_path(path) };
    let quoted = [format!("\"{name}\""), format!("'{name}'"), format!("\":{name}\""), format!("':{name}'")];
    text.lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("include"))
        .any(|line| quoted.iter().any(|name| line.contains(name)))
}

fn add_include(text: &str, path: &str, kotlin: bool) -> Option<String> {
    if is_listed(text, path) {
        return None;
    }
    let mut edited = text.to_owned();
    if !edited.is_empty() && !edited.ends_with('\n') {
        edited.push('\n');
    }
    if !edited.is_empty() && !edited.lines().last().is_some_and(|line| line.trim_start().starts_with("include")) {
        edited.push('\n');
    }
    edited.push_str(&include_lines(path, kotlin));
    Some(edited)
}

fn new_settings(project: &str, path: &str, kotlin: bool) -> String {
    let name =
        if kotlin { format!("rootProject.name = \"{project}\"") } else { format!("rootProject.name = '{project}'") };
    format!("{name}\n\n{}", include_lines(path, kotlin))
}

fn add_module(input: &Value) -> Value {
    let text = |key: &str| input[key].as_str().unwrap_or_default();
    let (file, path, project) = (text("file"), text("path"), text("project"));
    if path.contains(FORBIDDEN) || project.contains(FORBIDDEN) {
        let language = text("language");
        return json!({ "error": texts(
            language,
            "The module's path has characters a Gradle script cannot hold in a string.",
            "В пути модуля есть символы, которые нельзя записать в строку скрипта Gradle.",
        ) });
    }
    let kotlin = is_kotlin(file);
    if file.starts_with("settings.gradle") {
        return match add_include(text("text"), path, kotlin) {
            Some(edited) => json!({ "files": [{ "path": file, "text": edited }] }),
            None => json!({ "files": [] }),
        };
    }
    let settings = if kotlin { "settings.gradle.kts" } else { "settings.gradle" };
    json!({ "files": [{ "path": settings, "text": new_settings(project, path, kotlin) }] })
}

pub fn answer(input: &[u8]) -> Vec<u8> {
    let Ok(input) = serde_json::from_slice::<Value>(input) else { return Vec::new() };
    if input["step"] == "addModule" { add_module(&input) } else { json!({ "error": "unknown step" }) }
        .to_string()
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn added(file: &str, text: &str, path: &str) -> Value {
        let input = json!({ "step": "addModule", "file": file, "text": text, "path": path, "project": "shop" });
        serde_json::from_slice(&answer(input.to_string().as_bytes())).unwrap()
    }

    fn edited(file: &str, text: &str, path: &str) -> String {
        added(file, text, path)["files"][0]["text"].as_str().unwrap().to_owned()
    }

    #[test]
    fn a_kotlin_settings_file_gets_an_include_after_the_others() {
        let text = "rootProject.name = \"shop\"\n\ninclude(\"core\")\n";
        assert_eq!(edited("settings.gradle.kts", text, "api"), format!("{text}include(\"api\")\n"));
    }

    #[test]
    fn a_groovy_settings_file_gets_a_groovy_include_in_a_block_of_its_own() {
        assert_eq!(
            edited("settings.gradle", "rootProject.name = 'shop'", "api"),
            "rootProject.name = 'shop'\n\ninclude 'api'\n"
        );
    }

    #[test]
    fn a_nested_module_is_a_gradle_path() {
        assert_eq!(edited("settings.gradle.kts", "", "services/api"), "include(\"services:api\")\n");
    }

    #[test]
    fn a_module_beside_the_project_says_where_it_is() {
        assert_eq!(
            edited("settings.gradle", "", "../api"),
            "include 'api'\nproject(':api').projectDir = file('../api')\n"
        );
    }

    #[test]
    fn a_listed_module_changes_nothing() {
        for text in ["include(\"api\")\n", "include 'core', 'api'\n", "include(\":api\")\n"] {
            assert_eq!(added("settings.gradle.kts", text, "api")["files"], json!([]), "{text}");
        }
        let commented = edited("settings.gradle.kts", "// include(\"api\")\n", "api");
        assert!(commented.ends_with("\ninclude(\"api\")\n"), "{commented}");
    }

    #[test]
    fn a_build_without_settings_gets_them_in_its_own_language() {
        let files = added("build.gradle.kts", "plugins {}\n", "api")["files"].clone();
        assert_eq!(files[0]["path"], "settings.gradle.kts");
        assert_eq!(files[0]["text"], "rootProject.name = \"shop\"\n\ninclude(\"api\")\n");
    }

    #[test]
    fn a_path_a_script_cannot_hold_is_refused() {
        assert!(added("settings.gradle", "", "a'b")["error"].is_string());
        assert!(added("settings.gradle.kts", "", "a$b")["error"].is_string());
    }
}
