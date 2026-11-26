use std::path::{Path, PathBuf};

use zen_run::{Common, Configuration, ForeignRunConfiguration};

use super::{APPLICATION, ApplicationOptions, JAR, JarOptions};

pub(super) fn adopt(foreign: &ForeignRunConfiguration, root: &Path) -> Option<Configuration> {
    let option = |key: &str| foreign.options.get(key).map(String::as_str).unwrap_or_default().trim().to_owned();
    let path = |key: &str| relative(root, &option(key));
    let jre =
        if option("ALTERNATIVE_JRE_PATH_ENABLED") == "true" { option("ALTERNATIVE_JRE_PATH") } else { String::new() };
    let (kind, options) = match foreign.type_id.as_str() {
        "Application" => {
            let module = option("module.name");
            let module = if module.is_empty() || root.join(&module).is_dir() { module } else { String::new() };
            let options = ApplicationOptions {
                main_class: option("MAIN_CLASS_NAME"),
                module,
                jre,
                build: true,
                coroutine_debug: String::new(),
                track_coroutines: false,
            };
            (APPLICATION, serde_json::to_value(options).ok()?)
        }
        "JarApplication" => (JAR, serde_json::to_value(JarOptions { jar: path("JAR_PATH"), jre }).ok()?),
        _ => return None,
    };
    let working = path("WORKING_DIRECTORY");
    let common = Common {
        working_directory: Some(working).filter(|folder| !folder.is_empty()).map(PathBuf::from),
        env: foreign.env.clone(),
        program_arguments: option("PROGRAM_PARAMETERS"),
        tool_arguments: option("VM_PARAMETERS"),
        ..Common::default()
    };
    Some(Configuration {
        name: foreign.name.clone(),
        kind: kind.to_owned(),
        common,
        options,
        ..Configuration::default()
    })
}

fn relative(root: &Path, value: &str) -> String {
    let path = Path::new(value);
    match path.strip_prefix(root) {
        Ok(inside) => inside.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        Err(_) => value.to_owned(),
    }
}
