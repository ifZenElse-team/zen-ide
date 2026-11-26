use std::sync::mpsc;

use super::*;

const VET_OUTPUT: &str = r#"# example.com/vet
# [example.com/vet]
{
	"example.com/vet": {
		"printf": [
			{
				"posn": "/w/fix.go:3:13",
				"end": "/w/fix.go:3:14",
				"message": "non-constant format string in call to fmt.Printf",
				"suggested_fixes": [
					{
						"message": "Insert \"%s\" format string",
						"edits": [
							{ "filename": "/w/fix.go", "start": 25, "end": 25, "new": "\"%s\", " }
						]
					}
				]
			}
		]
	},
	"example.com/vet [example.com/vet.test]": {
		"printf": [
			{
				"posn": "/w/fix.go:3:13",
				"end": "/w/fix.go:3:14",
				"message": "non-constant format string in call to fmt.Printf"
			},
			{ "posn": "/w/a_test.go:6:12", "message": "wrong type" }
		]
	}
}
"#;

const STATICCHECK_OUTPUT: &str = concat!(
    r#"{"code":"U1000","severity":"error","location":{"file":"/w/fix.go","line":5,"column":6},"end":{"file":"","line":0,"column":0},"message":"func other is unused"}"#,
    "\n",
    r#"{"code":"ST1005","severity":"error","location":{"file":"/w/fix.go","line":6,"column":2},"end":{"file":"/w/fix.go","line":6,"column":7},"message":"error strings should not be capitalized"}"#,
    "\n",
    r#"{"code":"compile","severity":"error","location":{"file":"/w/fix.go","line":7,"column":2},"end":{"file":"","line":0,"column":0},"message":"broken"}"#,
    "\n",
);

#[test]
fn go_vet_output_lists_each_problem_once_with_its_fixes() {
    let issues = parse_vet(VET_OUTPUT.as_bytes());
    assert_eq!(issues.len(), 2);
    assert_eq!(
        (issues[0].tool, issues[0].code.as_str(), issues[0].line, issues[0].column),
        ("go vet", "printf", 3, 13)
    );
    assert_eq!(issues[0].end, Some((3, 14)));
    assert_eq!(
        issues[0].fixes,
        [ToolFix {
            title: "Insert \"%s\" format string".to_owned(),
            edits: vec![ToolEdit { start: 25, end: 25, new: "\"%s\", ".to_owned() }],
        }]
    );
    assert_eq!(issues[1].end, None);
}

#[test]
fn a_fix_that_edits_another_file_is_left_out() {
    let text = VET_OUTPUT.replace(r#""filename": "/w/fix.go""#, r#""filename": "/w/other.go""#);
    assert!(parse_vet(text.as_bytes())[0].fixes.is_empty());
}

#[test]
fn a_build_error_from_go_vet_is_not_a_finding() {
    assert!(parse_vet(b"# example.com/vet\nvet: ./a.go:1:1: expected 'package'\n").is_empty());
}

#[test]
fn staticcheck_output_marks_style_checks_weak_and_skips_compile_errors() {
    let issues = parse_staticcheck(STATICCHECK_OUTPUT.as_bytes());
    assert_eq!(issues.len(), 2);
    assert_eq!((issues[0].code.as_str(), issues[0].weak, issues[0].end), ("U1000", false, None));
    assert_eq!((issues[1].code.as_str(), issues[1].weak, issues[1].end), ("ST1005", true, Some((6, 7))));
}

fn issue(line: u32, column: u32, end: Option<(u32, u32)>) -> Issue {
    Issue {
        file: PathBuf::from("/w/a.go"),
        line,
        column,
        end,
        message: "wrong".to_owned(),
        tool: "go vet",
        code: "printf".to_owned(),
        weak: false,
        fixes: Vec::new(),
    }
}

#[test]
fn a_finding_covers_the_tool_range_or_else_the_word() {
    let text = "package a\n\nfunc other() {}\n";
    let ranged = finding(&issue(3, 6, Some((3, 9))), text, None).unwrap();
    assert_eq!(&text[ranged.start..ranged.end], "oth");
    let word = finding(&issue(3, 6, None), text, None).unwrap();
    assert_eq!(&text[word.start..word.end], "other");
    assert!(finding(&issue(9, 1, None), text, None).is_none());
}

#[test]
fn a_style_check_is_a_weak_warning() {
    let mut weak = issue(1, 1, None);
    weak.weak = true;
    assert_eq!(finding(&weak, "package a\n", None).unwrap().severity, FindingSeverity::WeakWarning);
    assert_eq!(finding(&issue(1, 1, None), "package a\n", None).unwrap().severity, FindingSeverity::Warning);
}

#[cfg(unix)]
mod programs {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        file: PathBuf,
        tool: PathBuf,
        staticcheck: PathBuf,
        log: PathBuf,
        inspector: GoVetInspector,
        done: mpsc::Receiver<()>,
    }

    fn script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    const SOURCE: &str = "package a\n\nfunc f(s string) { Printf(s) }\n";

    fn fixture() -> Fixture {
        zen_i18n::register(crate::LOCALES);
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("a.go");
        let log = root.join("log");
        std::fs::write(&file, SOURCE).unwrap();
        let json = format!(
            r#"{{"a":{{"printf":[{{"posn":"{0}:3:27","end":"{0}:3:28","message":"non-constant","suggested_fixes":[{{"message":"Insert","edits":[{{"filename":"{0}","start":37,"end":37,"new":"\"%s\", "}}]}}]}}]}}}}"#,
            file.display()
        );
        let vet = root.join("vet.json");
        std::fs::write(&vet, json).unwrap();
        let tool = root.join("go");
        script(&tool, &format!("echo \"go $GOPROXY $@\" >> {}\ncat {}", log.display(), vet.display()));
        let staticcheck = root.join("staticcheck");
        let line = format!(
            r#"{{"code":"SA4018","severity":"error","location":{{"file":"{}","line":3,"column":6}},"end":{{"file":"","line":0,"column":0}},"message":"unused f"}}"#,
            file.display()
        );
        let found = root.join("static.json");
        std::fs::write(&found, line).unwrap();
        script(&staticcheck, &format!("echo \"staticcheck $@\" >> {}\ncat {}", log.display(), found.display()));
        let located = staticcheck.clone();
        let inspector = GoVetInspector::new(Arc::new(move || Some(located.clone())));
        let (sender, done) = mpsc::channel();
        inspector.set_wake(Arc::new(move || {
            let _ = sender.send(());
        }));
        Fixture { _temp: temp, root, file, tool, staticcheck, log, inspector, done }
    }

    impl Fixture {
        fn inspect(&self, text: &str, settings: &[(&str, bool)]) -> Vec<Finding> {
            let setting = |key: &str| settings.iter().find(|(name, _)| *name == key).map(|(_, on)| json!(on));
            self.inspector.inspect(&InspectContext {
                root: &self.root,
                path: &self.file,
                text,
                tool: &self.tool,
                env: &[],
                setting: &setting,
            })
        }

        fn wait(&self) {
            self.done.recv_timeout(Duration::from_secs(20)).unwrap();
        }

        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_owned).collect()
        }
    }

    #[test]
    fn a_file_is_checked_by_both_tools_in_the_background_without_the_network() {
        let fixture = fixture();
        assert!(fixture.inspect(SOURCE, &[]).is_empty(), "the first answer never waits");
        fixture.wait();
        let found = fixture.inspect(SOURCE, &[]);
        assert_eq!(fixture.calls(), ["go off vet -json .", "staticcheck -f json ."]);
        let sources: Vec<&str> = found.iter().map(|finding| finding.source.as_str()).collect();
        assert_eq!(sources, ["go vet", "staticcheck"]);
        assert_eq!(found[0].fixes[0].title, "Insert");
        assert_eq!(&SOURCE[found[1].start..found[1].end], "f");
    }

    #[test]
    fn a_saved_file_is_checked_again_and_an_unsaved_edit_is_not() {
        let fixture = fixture();
        fixture.inspect(SOURCE, &[]);
        fixture.wait();
        let edited = format!("{SOURCE}\n// more\n");
        assert_eq!(fixture.inspect(&edited, &[]).len(), 2);
        assert!(fixture.inspector.running().is_empty(), "typing runs nothing");
        std::fs::write(&fixture.file, &edited).unwrap();
        fixture.inspect(&edited, &[]);
        fixture.wait();
        assert_eq!(fixture.calls().len(), 4);
    }

    #[test]
    fn with_on_save_off_only_a_demand_runs_the_tools() {
        let fixture = fixture();
        let off = [(ON_SAVE, false)];
        fixture.inspect(SOURCE, &off);
        assert!(fixture.inspector.running().is_empty(), "opening runs nothing");
        fixture.inspector.demand();
        fixture.wait();
        fixture.inspect(SOURCE, &off);
        fixture.wait();
        assert_eq!(fixture.calls().len(), 2);
        let edited = format!("{SOURCE}\n// more\n");
        std::fs::write(&fixture.file, &edited).unwrap();
        fixture.inspect(&edited, &off);
        assert!(fixture.inspector.running().is_empty(), "the save runs nothing");
        fixture.inspector.demand();
        fixture.wait();
        fixture.inspect(&edited, &off);
        fixture.wait();
        assert_eq!(fixture.calls().len(), 4);
    }

    #[test]
    fn each_tool_can_be_turned_off() {
        let fixture = fixture();
        fixture.inspect(SOURCE, &[(STATICCHECK, false)]);
        fixture.wait();
        assert_eq!(fixture.calls(), ["go off vet -json ."]);
        assert!(fixture.inspect(SOURCE, &[(VET, false), (STATICCHECK, false)]).is_empty());
    }

    #[test]
    fn staticcheck_being_missing_leaves_go_vet_working() {
        let fixture = fixture();
        std::fs::remove_file(&fixture.staticcheck).unwrap();
        fixture.inspect(SOURCE, &[]);
        fixture.wait();
        assert_eq!(fixture.inspect(SOURCE, &[]).len(), 1);
    }

    #[test]
    fn a_file_outside_the_project_is_not_checked() {
        let fixture = fixture();
        let setting = |_: &str| None;
        let found = fixture.inspector.inspect(&InspectContext {
            root: Path::new("/elsewhere"),
            path: &fixture.file,
            text: SOURCE,
            tool: &fixture.tool,
            env: &[],
            setting: &setting,
        });
        assert!(found.is_empty());
        assert!(fixture.inspector.running().is_empty());
    }

    #[test]
    fn an_edited_line_loses_its_finding_and_others_keep_theirs_without_fixes() {
        let fixture = fixture();
        fixture.inspect(SOURCE, &[]);
        fixture.wait();
        let above = format!("// note\n{SOURCE}");
        assert!(fixture.inspect(&above, &[]).is_empty(), "the lines moved");
        let below = format!("{SOURCE}// more\n");
        let found = fixture.inspect(&below, &[]);
        assert_eq!(found.len(), 2);
        assert!(found[0].fixes.is_empty(), "a fix needs the text the tool read");
    }

    fn apply(fixture: &Fixture, fix: &InspectionFix) -> Result<Box<dyn FixJob>, String> {
        let setting = |_: &str| None;
        fixture.inspector.apply(
            &InspectContext {
                root: &fixture.root,
                path: &fixture.file,
                text: SOURCE,
                tool: &fixture.tool,
                env: &[],
                setting: &setting,
            },
            std::slice::from_ref(fix),
        )
    }

    #[test]
    fn a_quick_fix_writes_the_tool_edit_and_refuses_a_changed_file() {
        let fixture = fixture();
        fixture.inspect(SOURCE, &[]);
        fixture.wait();
        let fix = fixture.inspect(SOURCE, &[])[0].fixes[0].clone();
        std::fs::write(&fixture.file, format!("{SOURCE}\n")).unwrap();
        assert_eq!(apply(&fixture, &fix).err(), Some(t!("go-inspections-fix-stale")));
        std::fs::write(&fixture.file, SOURCE).unwrap();
        let mut job = apply(&fixture, &fix).unwrap();
        let writes = job.writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].text, "package a\n\nfunc f(s string) { Printf(\"%s\", s) }\n");
        assert_eq!(writes[0].read.as_deref(), Some(SOURCE));
        assert_eq!(std::fs::read_to_string(&fixture.file).unwrap(), SOURCE, "the IDE writes it, with its guards");
        assert_eq!(job.poll(), FixState::Done { changed: vec![fixture.file.clone()] });
    }

    #[test]
    fn an_agent_does_not_install_staticcheck() {
        let mut session = Session::new();
        let refused = zen_core::macros::as_agent(&mut session, |session| InstallStaticcheck.run(session, NoParams {}))
            .unwrap_err();
        assert_eq!(refused.code.as_deref(), Some("agentNotAllowed"));
    }

    #[test]
    fn the_settings_page_offers_to_install_staticcheck_and_says_what_it_downloads() {
        let fixture = fixture();
        std::fs::remove_file(&fixture.staticcheck).unwrap();
        let mut session = Session::new();
        let missing = GoVetInspector::new(Arc::new(|| None));
        session.set_state(missing);
        let mut form = Form { items: Vec::new(), busy: false };
        add_settings(&session, &mut form);
        assert!(form.texts().contains(&t!("settings-go-staticcheck-missing").as_str()));
        assert!(form.texts().contains(&t!("settings-go-staticcheck-note").as_str()));
        assert!(t!("settings-go-staticcheck-note").contains(STATICCHECK_PACKAGE.trim_end_matches("@latest")));
    }
}
