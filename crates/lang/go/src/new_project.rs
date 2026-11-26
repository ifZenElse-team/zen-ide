use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde_json::Value;
use zen_i18n::t;
use zen_project::generators::{
    Condition, Field, FieldKind, FieldOption, GeneratedFile, GeneratorForm, GeneratorInfo, GeneratorRequest,
    NAME_PLACEHOLDER, ProjectGenerator, Values, write_files,
};

use crate::toolchain::go::{GoSdk, sdk_at};

const ID: &str = "go";

const GOROOT: &str = "goroot";

const GOROOT_PATH: &str = "gorootPath";

const LOCAL: &str = "local";

const MODULE: &str = "module";

const ENVIRONMENT: &str = "environment";

const SAMPLE: &str = "sample";

const SAMPLE_CODE: &str = "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"Hello, World!\")\n}\n";

pub struct FoundSdks {
    pub sdks: Vec<GoSdk>,
    pub chosen: Option<GoSdk>,
}

pub type GoSdks = Arc<dyn Fn() -> FoundSdks + Send + Sync>;

pub struct GoProject {
    sdks: GoSdks,
}

impl GoProject {
    pub fn new(sdks: GoSdks) -> Self {
        Self { sdks }
    }
}

fn text<'a>(values: &'a Values, id: &str) -> &'a str {
    values.get(id).and_then(Value::as_str).unwrap_or_default()
}

fn module_path(request: &GeneratorRequest) -> &str {
    Some(text(&request.values, MODULE)).filter(|module| !module.is_empty()).unwrap_or(&request.name)
}

fn wants_sample(request: &GeneratorRequest) -> bool {
    request.values.get(SAMPLE).and_then(Value::as_bool).unwrap_or(false)
}

fn chosen_root(values: &Values) -> Option<PathBuf> {
    let root = match text(values, GOROOT) {
        LOCAL => text(values, GOROOT_PATH),
        root => root,
    };
    (!root.trim().is_empty()).then(|| PathBuf::from(root.trim()))
}

pub fn environment(text: &str) -> Result<Vec<(String, String)>, String> {
    text.split([';', '\n'])
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) if !name.trim().is_empty() && !name.trim().contains(' ') => {
                Ok((name.trim().to_owned(), value.trim().to_owned()))
            }
            _ => Err(t!("go-new-project-environment-bad", pair = pair)),
        })
        .collect()
}

fn go_mod_init(go: &Path, module: &str, env: &[(String, String)], folder: &Path) -> Result<(), String> {
    let output = std::process::Command::new(go)
        .args(["mod", "init", module])
        .current_dir(folder)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .env("GOTOOLCHAIN", "local")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
}

fn field(id: &str, label: String, kind: FieldKind) -> Field {
    Field {
        id: id.to_owned(),
        label,
        kind,
        required: false,
        help: None,
        visible_when: None,
        advanced: false,
        segmented: false,
        page: 0,
        tooltip: None,
        manage: None,
    }
}

impl GoProject {
    fn fields(&self) -> Vec<Field> {
        let found = (self.sdks)();
        let mut options: Vec<FieldOption> = found
            .sdks
            .iter()
            .map(|sdk| {
                let root = sdk.root.to_string_lossy().into_owned();
                let label = match &sdk.version {
                    Some(version) => format!("{version}  {root}"),
                    None => root.clone(),
                };
                FieldOption { value: root, label, ..FieldOption::default() }
            })
            .collect();
        options.push(FieldOption {
            value: LOCAL.to_owned(),
            label: t!("go-new-project-goroot-local"),
            ..FieldOption::default()
        });
        let default = found.chosen.or_else(|| found.sdks.first().cloned());
        let default = Some(default.map_or_else(|| LOCAL.to_owned(), |sdk| sdk.root.to_string_lossy().into_owned()));
        vec![
            Field {
                required: true,
                ..field(GOROOT, t!("go-new-project-goroot"), FieldKind::Choice { options, default })
            },
            Field {
                required: true,
                visible_when: Some(Condition { field: GOROOT.to_owned(), any_of: vec![LOCAL.to_owned()] }),
                ..field(GOROOT_PATH, t!("go-new-project-goroot-path"), FieldKind::Path { default: String::new() })
            },
            Field {
                required: true,
                ..field(MODULE, t!("go-new-project-module"), FieldKind::Text { default: NAME_PLACEHOLDER.to_owned() })
            },
            Field {
                help: Some(t!("go-new-project-environment-help")),
                ..field(ENVIRONMENT, t!("go-new-project-environment"), FieldKind::Text { default: String::new() })
            },
            field(SAMPLE, t!("go-new-project-sample"), FieldKind::Check { default: true }),
        ]
    }
}

fn blocked(values: &Values) -> Option<String> {
    let root = chosen_root(values)?;
    if sdk_at(&root).is_none() {
        return Some(t!("go-new-project-goroot-invalid", path = root.to_string_lossy().into_owned()));
    }
    shared_environment(text(values, ENVIRONMENT)).err()
}

fn shared_environment(text: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let variables = crate::project_env::parse(text)?;
    for (name, value) in &variables {
        crate::project_env::check(crate::project_env::Scope::Project, name, value)?;
    }
    Ok(variables)
}

impl ProjectGenerator for GoProject {
    fn info(&self) -> GeneratorInfo {
        GeneratorInfo {
            id: ID.to_owned(),
            name: t!("go-new-project"),
            icon: "main.go".to_owned(),
            form_depends_on: vec![GOROOT.to_owned(), GOROOT_PATH.to_owned(), ENVIRONMENT.to_owned()],
            ..Default::default()
        }
    }

    fn form(&self) -> Result<GeneratorForm, String> {
        self.form_with(&Values::new())
    }

    fn form_with(&self, values: &Values) -> Result<GeneratorForm, String> {
        let form = GeneratorForm { fields: self.fields(), ..GeneratorForm::default() };
        let resolved = form.resolved("", values);
        Ok(GeneratorForm { blocked: blocked(&resolved), ..form })
    }

    fn files(&self, request: &GeneratorRequest) -> Result<Vec<GeneratedFile>, String> {
        let mut files = vec![GeneratedFile::text("go.mod", format!("module {}\n", module_path(request)))];
        if wants_sample(request) {
            files.push(GeneratedFile::text("main.go", SAMPLE_CODE));
        }
        Ok(files)
    }

    fn generate(&self, request: &GeneratorRequest, folder: &Path, _stop: &AtomicBool) -> Result<(), String> {
        let sdk =
            chosen_root(&request.values).and_then(|root| sdk_at(&root)).ok_or_else(|| t!("go-new-project-no-sdk"))?;
        let env = shared_environment(text(&request.values, ENVIRONMENT))?;
        let pairs: Vec<(String, String)> = env.clone().into_iter().collect();
        go_mod_init(&sdk.go(), module_path(request), &pairs, folder)?;
        let mut files = self.files(request)?;
        files.retain(|file| file.path != "go.mod");
        write_files(folder, &files)?;
        if env.is_empty() {
            return Ok(());
        }
        crate::project_env::GoProjectEnv::default().set(folder, crate::project_env::Scope::Project, &env)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn none() -> GoSdks {
        Arc::new(|| FoundSdks { sdks: Vec::new(), chosen: None })
    }

    #[cfg(unix)]
    fn fake_sdk(dir: &Path, script: &str) -> GoSdk {
        use std::os::unix::fs::PermissionsExt;
        let root = dir.join("go");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("VERSION"), "go1.27.2\n").unwrap();
        let go = root.join("bin/go");
        std::fs::write(&go, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&go, std::fs::Permissions::from_mode(0o755)).unwrap();
        sdk_at(&root).unwrap()
    }

    fn with(sdk: GoSdk) -> GoSdks {
        Arc::new(move || FoundSdks { sdks: vec![sdk.clone()], chosen: Some(sdk.clone()) })
    }

    fn request(project: &GoProject, values: Value) -> GeneratorRequest {
        let given: Values = serde_json::from_value(values).unwrap();
        let form = project.form_with(&given).unwrap();
        GeneratorRequest { name: "shop".to_owned(), values: form.resolved("shop", &given) }
    }

    #[test]
    fn the_module_path_follows_the_project_s_name_until_it_is_changed() {
        let project = GoProject::new(none());
        assert_eq!(module_path(&request(&project, json!({}))), "shop");
        assert_eq!(module_path(&request(&project, json!({ "module": "acme.dev/shop" }))), "acme.dev/shop");
    }

    #[test]
    fn without_a_go_sdk_the_form_asks_for_one_and_nothing_is_created() {
        let project = GoProject::new(none());
        let form = project.form().unwrap();
        let values = form.resolved("shop", &Values::new());
        assert_eq!(values[GOROOT], LOCAL, "with nothing found, Local… is chosen");
        assert_eq!(form.missing(&values).map(|field| field.id.as_str()), Some(GOROOT_PATH));
        let folder = tempfile::tempdir().unwrap();
        let error = project.generate(&request(&project, json!({})), folder.path(), &AtomicBool::new(false));
        assert_eq!(error.unwrap_err(), t!("go-new-project-no-sdk"));
        assert!(!folder.path().join("go.mod").exists());
    }

    #[test]
    fn a_folder_that_is_not_a_go_sdk_blocks_create_and_says_why() {
        zen_i18n::register(crate::LOCALES);
        let project = GoProject::new(none());
        let empty = tempfile::tempdir().unwrap();
        let given: Values = serde_json::from_value(json!({ "goroot": "local", "gorootPath": empty.path() })).unwrap();
        let blocked = project.form_with(&given).unwrap().blocked.unwrap();
        assert!(blocked.contains("bin/go"), "{blocked}");
    }

    #[test]
    fn the_environment_is_name_value_pairs() {
        assert_eq!(
            environment("GOPROXY=https://proxy.example; GOPRIVATE=acme.dev\n").unwrap(),
            [
                ("GOPROXY".to_owned(), "https://proxy.example".to_owned()),
                ("GOPRIVATE".to_owned(), "acme.dev".to_owned())
            ]
        );
        assert!(environment("GOPROXY").is_err());
        assert_eq!(environment("  ").unwrap(), []);
    }

    #[cfg(unix)]
    #[test]
    fn go_mod_init_runs_with_the_chosen_sdk_the_environment_and_the_local_toolchain() {
        let tools = tempfile::tempdir().unwrap();
        let sdk = fake_sdk(tools.path(), "echo \"$1 $2 $3 $GOTOOLCHAIN $GOPROXY\" > go.mod");
        let project = GoProject::new(with(sdk.clone()));
        let form = project.form().unwrap();
        assert_eq!(form.resolved("shop", &Values::new())[GOROOT], sdk.root.to_string_lossy().as_ref());
        assert_eq!(form.blocked, None);
        let folder = tempfile::tempdir().unwrap();
        let values = json!({ "module": "acme.dev/shop", "environment": "GOPROXY=off" });
        project.generate(&request(&project, values), folder.path(), &AtomicBool::new(false)).unwrap();
        let written = std::fs::read_to_string(folder.path().join("go.mod")).unwrap();
        assert_eq!(written.trim(), "mod init acme.dev/shop local off");
        assert!(folder.path().join("main.go").is_file());
        let shared: Value =
            serde_json::from_slice(&std::fs::read(folder.path().join(".zen/go.json")).unwrap()).unwrap();
        assert_eq!(shared, json!({ "version": 1, "env": { "GOPROXY": "off" } }), "the rows are kept with the project");
    }

    #[cfg(unix)]
    #[test]
    fn the_wizard_refuses_what_the_shared_file_may_not_hold() {
        zen_i18n::register(crate::LOCALES);
        let tools = tempfile::tempdir().unwrap();
        let project = GoProject::new(with(fake_sdk(tools.path(), "touch go.mod")));
        let given: Values = serde_json::from_value(json!({ "environment": "GOTOOLCHAIN=go1.30" })).unwrap();
        let blocked = project.form_with(&given).unwrap().blocked;
        assert_eq!(blocked, Some(t!("go-env-error-personal-only", name = "GOTOOLCHAIN")));
        let given: Values = serde_json::from_value(json!({ "environment": "GOPROXY=https://me:pw@corp" })).unwrap();
        assert_eq!(project.form_with(&given).unwrap().blocked, Some(t!("go-env-error-credentials", name = "GOPROXY")));
    }

    #[cfg(unix)]
    #[test]
    fn no_environment_means_no_shared_file() {
        let tools = tempfile::tempdir().unwrap();
        let project = GoProject::new(with(fake_sdk(tools.path(), "touch go.mod")));
        let folder = tempfile::tempdir().unwrap();
        project.generate(&request(&project, json!({})), folder.path(), &AtomicBool::new(false)).unwrap();
        assert!(!folder.path().join(".zen/go.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn no_sample_code_means_no_main_go() {
        let tools = tempfile::tempdir().unwrap();
        let project = GoProject::new(with(fake_sdk(tools.path(), "touch go.mod")));
        let folder = tempfile::tempdir().unwrap();
        project
            .generate(&request(&project, json!({ "sample": false })), folder.path(), &AtomicBool::new(false))
            .unwrap();
        assert!(!folder.path().join("main.go").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_go_reports_what_it_said() {
        let tools = tempfile::tempdir().unwrap();
        let project = GoProject::new(with(fake_sdk(tools.path(), "echo 'invalid module path' >&2; exit 1")));
        let folder = tempfile::tempdir().unwrap();
        let error =
            project.generate(&request(&project, json!({})), folder.path(), &AtomicBool::new(false)).unwrap_err();
        assert_eq!(error, "invalid module path");
    }
}
