use super::*;

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

#[test]
fn only_go_variables_are_taken_and_some_only_on_this_computer() {
    zen_i18n::register(crate::LOCALES);
    for name in ["GOPROXY", "GOPRIVATE", "GOFLAGS", "GO111MODULE", "CGO_ENABLED", "CGO_CFLAGS"] {
        assert_eq!(check(Scope::Project, name, "x"), Ok(()), "{name}");
    }
    for name in ["GOROOT", "PATH"] {
        assert_eq!(check(Scope::Personal, name, "x"), Err(t!("go-env-error-refused", name = name)), "{name}");
    }
    for name in ["HOME", "CC", "GoProxy", "GO-X", "CGO"] {
        assert_eq!(check(Scope::Personal, name, "x"), Err(t!("go-env-error-name", name = name)), "{name}");
    }
    for name in ["GOENV", "GOTOOLCHAIN"] {
        assert_eq!(check(Scope::Personal, name, "local"), Ok(()), "{name}");
        assert_eq!(check(Scope::Project, name, "local"), Err(t!("go-env-error-personal-only", name = name)));
    }
}

#[test]
fn a_url_with_a_user_or_password_never_goes_into_the_shared_file() {
    zen_i18n::register(crate::LOCALES);
    let secret = "https://proxy.example,https://me:token@corp.example/go|direct";
    assert_eq!(check(Scope::Project, "GOPROXY", secret), Err(t!("go-env-error-credentials", name = "GOPROXY")));
    assert_eq!(check(Scope::Personal, "GOPROXY", secret), Ok(()));
    assert_eq!(check(Scope::Project, "GOPROXY", "https://proxy.example/a@b,direct"), Ok(()), "an @ in the path");
    assert_eq!(check(Scope::Project, "GOPRIVATE", "*.corp.example,git@x"), Ok(()), "no URL");
}

#[test]
fn the_personal_layer_wins_and_the_shared_one_waits_for_trust() {
    let project = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = GoProjectEnv::new(Some(data.path().join(PERSONAL_FILE)));
    store.set(project.path(), Scope::Project, &map(&[("GOPROXY", "https://team"), ("GOPRIVATE", "acme.dev")])).unwrap();
    store.set(project.path(), Scope::Personal, &map(&[("GOPROXY", "https://mine"), ("GOFLAGS", "-tags=dev")])).unwrap();
    let shared: Value = serde_json::from_slice(&std::fs::read(project.path().join(SHARED_FILE)).unwrap()).unwrap();
    assert_eq!(shared, json!({ "version": 1, "env": { "GOPRIVATE": "acme.dev", "GOPROXY": "https://team" } }));
    assert_eq!(
        store.variables(project.path(), true),
        [
            ("GOFLAGS".to_owned(), "-tags=dev".to_owned()),
            ("GOPRIVATE".to_owned(), "acme.dev".to_owned()),
            ("GOPROXY".to_owned(), "https://mine".to_owned()),
        ]
    );
    let sources: Vec<Scope> = store.effective(project.path(), true).into_iter().map(|v| v.source).collect();
    assert_eq!(sources, [Scope::Personal, Scope::Project, Scope::Personal]);
    assert_eq!(
        store.variables(project.path(), false),
        [("GOFLAGS".to_owned(), "-tags=dev".to_owned()), ("GOPROXY".to_owned(), "https://mine".to_owned())],
        "an untrusted project's file is left out"
    );
    let other = tempfile::tempdir().unwrap();
    assert!(store.variables(other.path(), true).is_empty(), "the personal layer is per project");
}

#[test]
fn a_hand_written_shared_file_keeps_its_other_keys_and_loses_what_it_may_not_hold() {
    zen_i18n::register(crate::LOCALES);
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".zen")).unwrap();
    let written = json!({
        "version": 1,
        "note": "kept",
        "env": { "GOPROXY": "https://u:p@corp.example", "GOTOOLCHAIN": "go1.30", "GOFLAGS": "-mod=mod", "HOME": "/x" }
    });
    std::fs::write(project.path().join(SHARED_FILE), written.to_string()).unwrap();
    let store = GoProjectEnv::new(None);
    assert_eq!(store.shared(project.path()), map(&[("GOFLAGS", "-mod=mod")]));
    store.set(project.path(), Scope::Project, &map(&[("GOFLAGS", "-tags=ci")])).unwrap();
    let shared: Value = serde_json::from_slice(&std::fs::read(project.path().join(SHARED_FILE)).unwrap()).unwrap();
    assert_eq!(shared, json!({ "version": 1, "note": "kept", "env": { "GOFLAGS": "-tags=ci" } }));
    let refused = store.set(project.path(), Scope::Project, &map(&[("GOPROXY", "https://u:p@corp.example")]));
    assert_eq!(refused, Err(t!("go-env-error-credentials", name = "GOPROXY")));
    assert_eq!(
        store.set(project.path(), Scope::Personal, &map(&[("GOFLAGS", "-x")])),
        Err(t!("go-env-error-no-data")),
        "no data folder, nowhere to keep it"
    );
}

#[test]
fn emptying_a_layer_removes_what_held_it() {
    let project = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = GoProjectEnv::new(Some(data.path().join(PERSONAL_FILE)));
    store.set(project.path(), Scope::Project, &map(&[("GOFLAGS", "-tags=x")])).unwrap();
    store.set(project.path(), Scope::Personal, &map(&[("GOFLAGS", "-tags=y")])).unwrap();
    store.set(project.path(), Scope::Project, &BTreeMap::new()).unwrap();
    store.set(project.path(), Scope::Personal, &BTreeMap::new()).unwrap();
    assert!(!project.path().join(SHARED_FILE).exists());
    let personal: Value = serde_json::from_slice(&std::fs::read(data.path().join(PERSONAL_FILE)).unwrap()).unwrap();
    assert_eq!(personal, json!({ "version": 1, "projects": {} }));
    assert!(store.variables(project.path(), true).is_empty());
}

#[test]
fn the_ide_s_own_checks_add_readonly_after_the_project_s_flags() {
    assert_eq!(readonly_flags(&[]), "-mod=readonly");
    let env = [("GOFLAGS".to_owned(), "-mod=mod".to_owned()), ("GOFLAGS".to_owned(), " -tags=dev ".to_owned())];
    assert_eq!(readonly_flags(&env), "-tags=dev -mod=readonly", "the last one set wins, and readonly after it");
}

#[test]
fn a_user_and_password_in_a_url_are_hidden_from_agents() {
    assert_eq!(
        redact("https://me:token@corp.example/go,https://proxy.golang.org|direct"),
        "https://***@corp.example/go,https://proxy.golang.org|direct"
    );
    assert_eq!(redact("acme.dev,*.corp"), "acme.dev,*.corp");
}

fn session_for(root: &Path, data: &Path) -> (zen_core::Registry, zen_core::Session) {
    zen_i18n::register(crate::LOCALES);
    let mut registry = zen_core::Registry::new();
    commands::register(&mut registry).unwrap();
    let mut session = zen_core::Session::new();
    session.set_state(crate::toolchain::Toolchain::load(Some(data.join("toolchain.json"))));
    session.set_state(zen_project::trust::OpenRoot(root.to_owned()));
    (registry, session)
}

#[test]
fn only_a_person_sets_the_go_environment_and_gopls_restarts_with_it() {
    let project = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session) = session_for(project.path(), data.path());
    let personal = json!({ "scope": "personal", "environment": "GOPROXY=https://me:pw@corp.example; GOFLAGS=-tags=x" });
    let refused =
        zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, "go.env.set", personal.clone()));
    assert!(format!("{refused:?}").contains(commands::AGENT_NOT_ALLOWED), "{refused:?}");
    let toolchain = session.state::<crate::toolchain::Toolchain>().cloned().unwrap();
    let generation = toolchain.generation();
    let answer = registry.dispatch(&mut session, "go.env.set", personal).unwrap();
    assert_eq!(answer["personal"]["GOPROXY"], "https://me:pw@corp.example");
    assert!(toolchain.generation() > generation, "windows restart gopls with it");
    let generation = toolchain.generation();
    registry
        .dispatch(
            &mut session,
            "go.env.set",
            json!({ "scope": "personal", "environment": "GOFLAGS=-tags=x; GOPROXY=https://me:pw@corp.example" }),
        )
        .unwrap();
    assert_eq!(toolchain.generation(), generation, "nothing changed, nothing restarts");
    let seen = zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, "go.env.get", json!({})))
        .unwrap();
    assert_eq!(seen["personal"]["GOPROXY"], "https://***@corp.example");
    assert_eq!(
        seen["effective"][1],
        json!({ "name": "GOPROXY", "value": "https://***@corp.example", "source": "personal" })
    );
    let shared = registry.dispatch(
        &mut session,
        "go.env.set",
        json!({ "scope": "project", "environment": "GOTOOLCHAIN=go1.30" }),
    );
    assert!(format!("{shared:?}").contains(&t!("go-env-error-personal-only", name = "GOTOOLCHAIN")), "{shared:?}");
}

#[test]
fn the_go_modules_page_shows_both_layers_and_why_the_shared_one_waits() {
    let project = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (registry, mut session) = session_for(project.path(), data.path());
    registry
        .dispatch(&mut session, "go.env.set", json!({ "scope": "project", "environment": "GOPRIVATE=acme.dev" }))
        .unwrap();
    let form: zen_modules::form::Form =
        serde_json::from_value(registry.dispatch(&mut session, "go.env.settings", json!({})).unwrap()).unwrap();
    let texts = form.texts();
    for text in [
        t!("settings-go-env-shared"),
        t!("settings-go-env-personal"),
        t!("settings-go-env-untrusted"),
        t!("settings-go-env-none"),
        t!("settings-go-env-checks"),
    ] {
        assert!(texts.contains(&text.as_str()), "no {text:?} in {texts:?}");
    }
    let fields: Vec<(&str, &zen_modules::form::Action)> = form
        .items
        .iter()
        .filter_map(|item| match item {
            zen_modules::form::Item::Field { value, action, .. } => Some((value.as_str(), action)),
            _ => None,
        })
        .collect();
    assert_eq!(fields[0].0, "GOPRIVATE=acme.dev");
    assert_eq!(fields[0].1.params_with(Some(json!("GOFLAGS=-x")))["scope"], "project");
    assert_eq!(fields[1].1.params_with(Some(json!("GOFLAGS=-x")))["scope"], "personal");
    let info = commands::info(session.state::<crate::toolchain::Toolchain>().unwrap(), project.path());
    let trusted = commands::GoEnvInfo { shared_applies: true, effective: info.effective.clone(), ..info };
    let trusted = commands::GoEnvInfo {
        effective: vec![Variable {
            name: "GOPRIVATE".to_owned(),
            value: "acme.dev".to_owned(),
            source: Scope::Project,
        }],
        ..trusted
    };
    let texts = commands::settings_form(&trusted).texts().into_iter().map(str::to_owned).collect::<Vec<_>>();
    assert!(!texts.contains(&t!("settings-go-env-untrusted")));
    assert!(texts.contains(&format!("GOPRIVATE=acme.dev   {}", t!("settings-go-env-from-project"))), "{texts:?}");
}

#[test]
fn the_text_form_reads_and_writes_name_value_pairs() {
    let variables = parse("GOPROXY=https://p; GOPRIVATE=acme.dev\n").unwrap();
    assert_eq!(variables, map(&[("GOPRIVATE", "acme.dev"), ("GOPROXY", "https://p")]));
    assert_eq!(text_of(&variables), "GOPRIVATE=acme.dev; GOPROXY=https://p");
    assert!(parse("GOPROXY").is_err());
}
