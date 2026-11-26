use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Platform(u8),
    Lang,
    App,
    Plugin,
    Driver,
    Testkit,
    Tool,
}

const LANG_LAYER: u8 = 5;
const CLIENT_LAYER: u8 = 6;

impl Kind {
    fn directory(self) -> Option<&'static str> {
        match self {
            Kind::Platform(_) | Kind::Testkit => Some("crates/platform/"),
            Kind::Lang => Some("crates/lang/"),
            Kind::App => Some("apps/"),
            Kind::Plugin => Some("plugins/"),
            Kind::Driver => Some("drivers/"),
            Kind::Tool => None,
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Platform(layer) => write!(f, "L{layer}"),
            Kind::Lang => write!(f, "L{LANG_LAYER}, language module"),
            Kind::App => write!(f, "app"),
            Kind::Plugin => write!(f, "plug-in"),
            Kind::Driver => write!(f, "driver"),
            Kind::Testkit => write!(f, "testkit"),
            Kind::Tool => write!(f, "tool"),
        }
    }
}

pub(crate) const REGISTRY: &[(&str, Kind)] = &[
    ("zen-text", Kind::Platform(0)),
    ("zen-vfs", Kind::Platform(0)),
    ("zen-env", Kind::Platform(0)),
    ("zen-i18n", Kind::Platform(0)),
    ("zen-image", Kind::Platform(0)),
    ("zen-macos", Kind::Platform(0)),
    ("zen-mascot", Kind::Platform(0)),
    ("zen-spell", Kind::Platform(0)),
    ("zen-secrets", Kind::Platform(0)),
    ("zen-net", Kind::Platform(1)),
    ("zen-syntax", Kind::Platform(1)),
    ("zen-project", Kind::Platform(1)),
    ("zen-theme", Kind::Platform(1)),
    ("zen-index", Kind::Platform(2)),
    ("zen-lsp", Kind::Platform(2)),
    ("zen-dap", Kind::Platform(2)),
    ("zen-vcs", Kind::Platform(2)),
    ("zen-terminal", Kind::Platform(2)),
    ("zen-run", Kind::Platform(2)),
    ("zen-update", Kind::Platform(2)),
    ("zen-http", Kind::Platform(2)),
    ("zen-automation", Kind::Platform(2)),
    ("zen-db", Kind::Platform(2)),
    ("zen-editor", Kind::Platform(3)),
    ("zen-core", Kind::Platform(3)),
    ("zen-modules", Kind::Platform(4)),
    ("zen-plugins", Kind::Platform(5)),
    ("zen-jb-import", Kind::Platform(5)),
    ("zen-toolbox", Kind::Platform(5)),
    ("zen-workbench", Kind::Platform(5)),
    ("zen-ui", Kind::Platform(6)),
    ("zen-lang-rust", Kind::Lang),
    ("zen-lang-go", Kind::Lang),
    ("zen-plugin-markdown", Kind::Plugin),
    ("zen-plugin-sql", Kind::Plugin),
    ("zen-plugin-ignore", Kind::Plugin),
    ("zen-plugin-env", Kind::Plugin),
    ("zen-plugin-dockerfile", Kind::Plugin),
    ("zen-plugin-kubernetes", Kind::Plugin),
    ("zen-plugin-yaml", Kind::Plugin),
    ("zen-plugin-gitlab-ci", Kind::Plugin),
    ("zen-plugin-json", Kind::Plugin),
    ("zen-plugin-toml", Kind::Plugin),
    ("zen-plugin-shell", Kind::Plugin),
    ("zen-plugin-html", Kind::Plugin),
    ("zen-plugin-css", Kind::Plugin),
    ("zen-plugin-scss", Kind::Plugin),
    ("zen-plugin-javascript", Kind::Plugin),
    ("zen-plugin-typescript", Kind::Plugin),
    ("zen-plugin-http", Kind::Plugin),
    ("zen-plugin-vue", Kind::Plugin),
    ("zen-plugin-svelte", Kind::Plugin),
    ("zen-plugin-nuxt", Kind::Plugin),
    ("zen-plugin-npm", Kind::Plugin),
    ("zen-plugin-nodejs", Kind::Plugin),
    ("zen-plugin-ai-agents", Kind::Plugin),
    ("zen-plugin-maven", Kind::Plugin),
    ("zen-plugin-gradle", Kind::Plugin),
    ("zen-plugin-ant", Kind::Plugin),
    ("zen-plugin-properties", Kind::Plugin),
    ("zen-plugin-terraform", Kind::Plugin),
    ("zen-plugin-gitops", Kind::Plugin),
    ("zen-plugin-spring-boot", Kind::Plugin),
    ("zen-plugin-kotlin", Kind::Plugin),
    ("zen-plugin-react", Kind::Plugin),
    ("zen-plugin-next", Kind::Plugin),
    ("zen-plugin-eslint", Kind::Plugin),
    ("zen-plugin-prettier", Kind::Plugin),
    ("zen-plugin-stylelint", Kind::Plugin),
    ("zen-plugin-biome", Kind::Plugin),
    ("zen-plugin-sveltekit", Kind::Plugin),
    ("zen-plugin-xml", Kind::Plugin),
    ("zen-plugin-java", Kind::Plugin),
    ("zen-plugin-sqlserver", Kind::Plugin),
    ("zen-plugin-mongodb", Kind::Plugin),
    ("zen-plugin-duckdb", Kind::Plugin),
    ("zen-plugin-oracle", Kind::Plugin),
    ("zen-db-driver-kit", Kind::Driver),
    ("zen-db-sqlserver", Kind::Driver),
    ("zen-db-mongodb", Kind::Driver),
    ("zen-db-duckdb", Kind::Driver),
    ("zen-db-oracle", Kind::Driver),
    ("rust-ronin", Kind::App),
    ("go-garden", Kind::App),
    ("web-wave", Kind::App),
    ("jvm-jizo", Kind::App),
    ("sql-scroll", Kind::App),
    ("devops-daruma", Kind::App),
    ("zen-cli", Kind::App),
    ("zen-pty-host", Kind::App),
    ("zen-toolbox-app", Kind::App),
    ("zen-releases", Kind::App),
    ("zen-testkit", Kind::Testkit),
    ("xtask", Kind::Tool),
];

const UI_CRATES: &[&str] =
    &["egui", "eframe", "egui-wgpu", "egui-winit", "egui_kittest", "wgpu", "winit", "rfd", "tray-icon"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Clone, Debug)]
pub(crate) struct Dep {
    pub(crate) name: String,
    pub(crate) kind: DepKind,
}

#[derive(Clone, Debug)]
pub(crate) struct Crate {
    pub(crate) name: String,
    pub(crate) dir: String,
    pub(crate) deps: Vec<Dep>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Violation(pub(crate) String);

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn kind_of(registry: &[(&str, Kind)], name: &str) -> Option<Kind> {
    registry.iter().find(|(n, _)| *n == name).map(|&(_, kind)| kind)
}

pub(crate) fn check(registry: &[(&str, Kind)], crates: &[Crate]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let workspace: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();

    for krate in crates {
        let Some(kind) = kind_of(registry, &krate.name) else {
            violations.push(Violation(format!(
                "{}: not registered; add it with its layer to xtask/src/layers.rs (ADR-0014, rule 7)",
                krate.name
            )));
            continue;
        };

        if let Some(dir) = kind.directory()
            && !krate.dir.starts_with(dir)
        {
            violations.push(Violation(format!(
                "{} ({kind}): lives in `{}` but must live in `{dir}` (ADR-0014, rule 7)",
                krate.name, krate.dir
            )));
        }

        for dep in &krate.deps {
            if workspace.contains(&dep.name.as_str()) {
                let dep_kind = kind_of(registry, &dep.name);
                if let Some(message) = workspace_dep_violation(&krate.name, kind, &dep.name, dep_kind, dep.kind) {
                    violations.push(Violation(message));
                }
            } else if UI_CRATES.contains(&dep.name.as_str()) && !may_use_ui(kind) {
                violations.push(Violation(format!(
                    "{} ({kind}) depends on `{}`: only the client layer (L{CLIENT_LAYER}) and apps may use a UI \
                     toolkit (ADR-0014, rule 3)",
                    krate.name, dep.name
                )));
            }
        }
    }
    violations
}

pub(crate) fn portable(registry: &[(&str, Kind)], crates: &[Crate]) -> Vec<String> {
    crates
        .iter()
        .filter(|krate| match kind_of(registry, &krate.name) {
            Some(Kind::Platform(layer)) => layer < CLIENT_LAYER,
            Some(Kind::Lang | Kind::Plugin) => true,
            Some(Kind::App | Kind::Driver | Kind::Testkit | Kind::Tool) | None => false,
        })
        .map(|krate| krate.name.clone())
        .collect()
}

fn may_use_ui(kind: Kind) -> bool {
    match kind {
        Kind::App | Kind::Tool => true,
        Kind::Testkit | Kind::Plugin | Kind::Driver => false,
        Kind::Platform(layer) => layer >= CLIENT_LAYER,
        Kind::Lang => false,
    }
}

fn workspace_dep_violation(
    from: &str,
    from_kind: Kind,
    to: &str,
    to_kind: Option<Kind>,
    dep_kind: DepKind,
) -> Option<String> {
    let to_kind = to_kind?;
    let edge = format!("{from} ({from_kind}) depends on {to} ({to_kind})");
    match (from_kind, to_kind) {
        (_, Kind::Tool) => Some(format!("{edge}: nothing may depend on a development tool (ADR-0014, rule 6)")),
        (_, Kind::Testkit) if dep_kind != DepKind::Dev => {
            Some(format!("{edge}: zen-testkit may be used only as a dev-dependency (ADR-0014, rule 6)"))
        }
        (_, Kind::Testkit) | (Kind::Tool, _) => None,
        (Kind::Plugin, _) => Some(format!(
            "{edge}: a plug-in may depend on no workspace crate; it talks to the IDE through the plug-in ABI (ADR-0031)"
        )),
        (_, Kind::Plugin) if dep_kind == DepKind::Dev => None,
        (_, Kind::Plugin) => {
            Some(format!("{edge}: nothing may depend on a plug-in; the IDE loads it at run time (ADR-0031)"))
        }
        (Kind::Driver, Kind::Driver) => None,
        (Kind::Driver, _) => Some(format!(
            "{edge}: a driver program may depend only on other drivers; it talks to the IDE through the driver \
             protocol (ADR-0174)"
        )),
        (_, Kind::Driver) if dep_kind == DepKind::Dev => None,
        (_, Kind::Driver) => {
            Some(format!("{edge}: nothing may depend on a driver program; the IDE starts it (ADR-0174)"))
        }
        (_, Kind::App) => Some(format!("{edge}: nothing may depend on an application (ADR-0014, rule 5)")),
        (Kind::App, _) => None,
        (Kind::Testkit, Kind::Platform(_)) => None,
        (Kind::Testkit, Kind::Lang) => {
            Some(format!("{edge}: zen-testkit must not depend on a language module (ADR-0014, rule 2)"))
        }
        (Kind::Platform(_), Kind::Lang) => {
            Some(format!("{edge}: a platform crate must not depend on a language module (ADR-0014, rule 2)"))
        }
        (Kind::Lang, Kind::Lang) => Some(format!(
            "{edge}: a language module must not depend on another language module without an ADR (ADR-0014, rule 2)"
        )),
        (Kind::Lang, Kind::Platform(layer)) if layer >= LANG_LAYER => {
            Some(format!("{edge}: a language module may depend only on platform layers L0 to L4 (ADR-0014, rule 2)"))
        }
        (Kind::Lang, Kind::Platform(_)) => None,
        (Kind::Platform(from_layer), Kind::Platform(to_layer)) if to_layer >= from_layer => {
            Some(format!("{edge}: a crate may depend only on crates in strictly lower layers (ADR-0014, rule 1)"))
        }
        (Kind::Platform(_), Kind::Platform(_)) => None,
    }
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

fn load_workspace() -> Result<Vec<Crate>, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .output()
        .map_err(|error| format!("cannot run `cargo metadata`: {error}"))?;
    if !output.status.success() {
        return Err(format!("`cargo metadata` failed:\n{}", String::from_utf8_lossy(&output.stderr)));
    }
    let metadata: Metadata =
        serde_json::from_slice(&output.stdout).map_err(|error| format!("cannot parse `cargo metadata`: {error}"))?;

    let crates = metadata
        .packages
        .into_iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .map(|package| Crate {
            dir: relative_dir(&metadata.workspace_root, &package.manifest_path),
            name: package.name,
            deps: package
                .dependencies
                .into_iter()
                .map(|dep| Dep {
                    name: dep.name,
                    kind: match dep.kind.as_deref() {
                        Some("dev") => DepKind::Dev,
                        Some("build") => DepKind::Build,
                        _ => DepKind::Normal,
                    },
                })
                .collect(),
        })
        .collect();
    Ok(crates)
}

fn relative_dir(root: &Path, manifest: &Path) -> String {
    let dir = manifest.parent().unwrap_or(manifest);
    let relative = dir.strip_prefix(root).unwrap_or(dir);
    let mut text = relative.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    text.push('/');
    text
}

pub(crate) fn run() -> Result<(), String> {
    let crates = load_workspace()?;
    let violations = check(REGISTRY, &crates);
    if violations.is_empty() {
        println!("layers: ok ({} crates checked)", crates.len());
        Ok(())
    } else {
        for violation in &violations {
            eprintln!("error: {violation}");
        }
        Err(format!("layers: {} violation(s); see docs/adr/0014-refined-layer-model.md", violations.len()))
    }
}

pub(crate) fn portable_crates() -> Result<Vec<String>, String> {
    Ok(portable(REGISTRY, &load_workspace()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_REGISTRY: &[(&str, Kind)] = &[
        ("zen-text", Kind::Platform(0)),
        ("zen-net", Kind::Platform(1)),
        ("zen-syntax", Kind::Platform(1)),
        ("zen-core", Kind::Platform(4)),
        ("zen-plugins", Kind::Platform(5)),
        ("zen-ui", Kind::Platform(6)),
        ("zen-lang-rust", Kind::Lang),
        ("zen-lang-go", Kind::Lang),
        ("rust-ronin", Kind::App),
        ("zen-cli", Kind::App),
        ("zen-testkit", Kind::Testkit),
        ("xtask", Kind::Tool),
    ];

    fn dir_for(name: &str) -> String {
        match kind_of(TEST_REGISTRY, name) {
            Some(Kind::Lang) => format!("crates/lang/{}/", name.trim_start_matches("zen-lang-")),
            Some(Kind::App) => format!("apps/{name}/"),
            Some(Kind::Tool) => format!("{name}/"),
            _ => format!("crates/platform/{}/", name.trim_start_matches("zen-")),
        }
    }

    fn krate(name: &str, deps: &[(&str, DepKind)]) -> Crate {
        Crate {
            name: name.to_owned(),
            dir: dir_for(name),
            deps: deps.iter().map(|&(name, kind)| Dep { name: name.to_owned(), kind }).collect(),
        }
    }

    fn workspace(extra: Vec<Crate>) -> Vec<Crate> {
        let mut crates: Vec<Crate> = TEST_REGISTRY
            .iter()
            .filter(|(name, _)| !extra.iter().any(|c| c.name == *name))
            .map(|(name, _)| krate(name, &[]))
            .collect();
        crates.extend(extra);
        crates
    }

    fn violations(extra: Vec<Crate>) -> Vec<Violation> {
        check(TEST_REGISTRY, &workspace(extra))
    }

    fn assert_single_violation(extra: Vec<Crate>, expected_fragment: &str) {
        let found = violations(extra);
        assert_eq!(found.len(), 1, "expected one violation, got {found:?}");
        assert!(found[0].0.contains(expected_fragment), "`{}` does not mention `{expected_fragment}`", found[0]);
    }

    #[test]
    fn a_clean_workspace_passes() {
        let found = violations(vec![
            krate("zen-syntax", &[("zen-text", DepKind::Normal), ("zen-testkit", DepKind::Dev)]),
            krate("zen-core", &[("zen-text", DepKind::Normal), ("zen-syntax", DepKind::Normal)]),
            krate("zen-plugins", &[("zen-core", DepKind::Normal)]),
            krate("zen-lang-rust", &[("zen-core", DepKind::Normal), ("zen-syntax", DepKind::Normal)]),
            krate("zen-ui", &[("zen-core", DepKind::Normal), ("egui", DepKind::Normal)]),
            krate(
                "rust-ronin",
                &[("zen-ui", DepKind::Normal), ("zen-lang-rust", DepKind::Normal), ("eframe", DepKind::Normal)],
            ),
            krate("zen-testkit", &[("zen-text", DepKind::Normal)]),
            krate("xtask", &[("serde_json", DepKind::Normal)]),
        ]);
        assert!(found.is_empty(), "unexpected violations: {found:?}");
    }

    #[test]
    fn the_real_registry_has_unique_names() {
        for (index, (name, _)) in REGISTRY.iter().enumerate() {
            assert!(!REGISTRY[index + 1..].iter().any(|(other, _)| other == name), "{name} is registered twice");
        }
    }

    #[test]
    fn depending_on_a_higher_layer_fails() {
        assert_single_violation(vec![krate("zen-text", &[("zen-core", DepKind::Normal)])], "strictly lower layers");
    }

    #[test]
    fn depending_on_the_same_layer_fails() {
        let registry = &[("zen-text", Kind::Platform(0)), ("zen-vfs", Kind::Platform(0))];
        let crates = vec![
            Crate { name: "zen-text".into(), dir: "crates/platform/text/".into(), deps: vec![] },
            Crate {
                name: "zen-vfs".into(),
                dir: "crates/platform/vfs/".into(),
                deps: vec![Dep { name: "zen-text".into(), kind: DepKind::Normal }],
            },
        ];
        let found = check(registry, &crates);
        assert_eq!(found.len(), 1);
        assert!(found[0].0.contains("strictly lower layers"));
    }

    #[test]
    fn dev_and_build_dependencies_follow_the_same_rules() {
        assert_single_violation(vec![krate("zen-text", &[("zen-core", DepKind::Dev)])], "strictly lower layers");
        assert_single_violation(vec![krate("zen-text", &[("zen-core", DepKind::Build)])], "strictly lower layers");
    }

    #[test]
    fn a_platform_crate_depending_on_a_language_module_fails() {
        assert_single_violation(
            vec![krate("zen-ui", &[("zen-lang-rust", DepKind::Normal)])],
            "must not depend on a language module",
        );
    }

    #[test]
    fn a_language_module_depending_on_another_fails() {
        assert_single_violation(
            vec![krate("zen-lang-go", &[("zen-lang-rust", DepKind::Normal)])],
            "another language module",
        );
    }

    #[test]
    fn a_language_module_depending_on_features_or_clients_fails() {
        assert_single_violation(vec![krate("zen-lang-rust", &[("zen-plugins", DepKind::Normal)])], "L0 to L4");
        assert_single_violation(vec![krate("zen-lang-rust", &[("zen-ui", DepKind::Normal)])], "L0 to L4");
    }

    #[test]
    fn a_ui_toolkit_below_the_client_layer_fails() {
        for ui in UI_CRATES {
            assert_single_violation(vec![krate("zen-core", &[(ui, DepKind::Normal)])], "UI toolkit");
        }
        assert_single_violation(vec![krate("zen-lang-rust", &[("egui", DepKind::Dev)])], "UI toolkit");
    }

    #[test]
    fn an_unregistered_crate_fails() {
        let mut crates = workspace(vec![]);
        crates.push(Crate { name: "zen-mystery".into(), dir: "crates/platform/mystery/".into(), deps: vec![] });
        let found = check(TEST_REGISTRY, &crates);
        assert_eq!(found.len(), 1);
        assert!(found[0].0.contains("not registered"));
    }

    #[test]
    fn a_crate_in_the_wrong_directory_fails() {
        let mut crates = workspace(vec![]);
        crates.retain(|c| c.name != "zen-lang-rust");
        crates.push(Crate { name: "zen-lang-rust".into(), dir: "crates/platform/lang-rust/".into(), deps: vec![] });
        let found = check(TEST_REGISTRY, &crates);
        assert_eq!(found.len(), 1);
        assert!(found[0].0.contains("must live in `crates/lang/`"));
    }

    #[test]
    fn testkit_only_as_a_dev_dependency() {
        assert_single_violation(vec![krate("zen-core", &[("zen-testkit", DepKind::Normal)])], "dev-dependency");
    }

    #[test]
    fn plugins_only_as_dev_dependencies() {
        let registry = &[("zen-ui", Kind::Platform(6)), ("zen-plugin-html", Kind::Plugin)];
        let crates = |kind| {
            vec![
                Crate { name: "zen-plugin-html".into(), dir: "plugins/html/".into(), deps: vec![] },
                Crate {
                    name: "zen-ui".into(),
                    dir: "crates/platform/ui/".into(),
                    deps: vec![Dep { name: "zen-plugin-html".into(), kind }],
                },
            ]
        };
        assert!(check(registry, &crates(DepKind::Dev)).is_empty(), "tests run the real plug-ins");
        let found = check(registry, &crates(DepKind::Normal));
        assert_eq!(found.len(), 1);
        assert!(found[0].0.contains("nothing may depend on a plug-in"));
    }

    #[test]
    fn drivers_depend_only_on_drivers_and_nothing_depends_on_them() {
        let registry =
            &[("zen-db", Kind::Platform(2)), ("zen-db-driver-kit", Kind::Driver), ("zen-db-sqlserver", Kind::Driver)];
        let driver = |deps: Vec<Dep>| Crate { name: "zen-db-sqlserver".into(), dir: "drivers/sqlserver/".into(), deps };
        let kit = Crate { name: "zen-db-driver-kit".into(), dir: "drivers/kit/".into(), deps: vec![] };
        let platform = |kind| Crate {
            name: "zen-db".into(),
            dir: "crates/platform/db/".into(),
            deps: vec![Dep { name: "zen-db-sqlserver".into(), kind }],
        };
        let uses_kit = vec![Dep { name: "zen-db-driver-kit".into(), kind: DepKind::Normal }];
        let fine = vec![driver(uses_kit), kit.clone(), platform(DepKind::Dev)];
        assert!(check(registry, &fine).is_empty(), "a driver uses the kit; the engine's tests run a driver");
        let uses_engine = vec![Dep { name: "zen-db".into(), kind: DepKind::Normal }];
        let found = check(registry, &[driver(uses_engine), kit.clone(), platform(DepKind::Dev)]);
        assert_eq!(found.len(), 1);
        assert!(found[0].0.contains("only on other drivers"), "{found:?}");
        let found = check(registry, &[driver(vec![]), kit, platform(DepKind::Normal)]);
        assert_eq!(found.len(), 1);
        assert!(found[0].0.contains("nothing may depend on a driver program"), "{found:?}");
        assert!(portable(registry, &[driver(vec![])]).is_empty(), "a driver program is built for the desktop only");
    }

    #[test]
    fn nothing_depends_on_apps_or_tools() {
        assert_single_violation(vec![krate("zen-cli", &[("rust-ronin", DepKind::Normal)])], "an application");
        assert_single_violation(vec![krate("zen-core", &[("xtask", DepKind::Dev)])], "development tool");
    }

    #[test]
    fn apps_may_depend_on_everything_below_them() {
        let found = violations(vec![krate(
            "rust-ronin",
            &[("zen-text", DepKind::Normal), ("zen-ui", DepKind::Normal), ("zen-lang-rust", DepKind::Normal)],
        )]);
        assert!(found.is_empty(), "unexpected violations: {found:?}");
    }

    #[test]
    fn relative_dir_is_normalised() {
        let root = Path::new("/work/zen-ide");
        assert_eq!(
            relative_dir(root, Path::new("/work/zen-ide/crates/platform/text/Cargo.toml")),
            "crates/platform/text/"
        );
        assert_eq!(relative_dir(root, Path::new("/work/zen-ide/xtask/Cargo.toml")), "xtask/");
    }

    #[test]
    fn portable_crates_are_everything_below_the_client_layer() {
        let mut found = portable(TEST_REGISTRY, &workspace(vec![]));
        found.sort();
        assert_eq!(
            found,
            ["zen-core", "zen-lang-go", "zen-lang-rust", "zen-net", "zen-plugins", "zen-syntax", "zen-text"]
        );
    }
}
