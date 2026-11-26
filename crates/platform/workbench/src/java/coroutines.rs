use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::Settings;

use zen_run::{ClasspathAgent, Configuration, DebugPlan};

pub const DEBUG_MODE: &str = "debugger.kotlinCoroutines.debugMode";

pub const DEBUG_MODE_FLAG: &str = "-Dkotlinx.coroutines.debug";

pub const OPTION: &str = "coroutineDebug";

pub const CHOICES: [&str; 3] = ["default", "on", "off"];

pub const TRACK: &str = "trackCoroutines";

pub const AGENT_PREFIX: &str = "kotlinx-coroutines-debug-";

pub const AGENT_OPTION: &str = "-javaagent:";

pub fn jvm_kind(kind: &str) -> bool {
    [
        crate::java_run::APPLICATION,
        crate::java_run::JAR,
        crate::java_run::FILE,
        crate::java::junit::JUNIT,
        crate::spring_boot::run::KIND,
    ]
    .contains(&kind)
}

pub fn tracks(configuration: &Configuration) -> bool {
    configuration.kind == crate::java_run::APPLICATION
        && configuration.options.get(TRACK).and_then(serde_json::Value::as_bool) == Some(true)
}

fn classpath_file(args: &[String]) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == "-cp" || arg == "-classpath" || arg == "--class-path")?;
    args.get(at + 1)?.strip_prefix('@').map(PathBuf::from)
}

pub fn with_probes(mut plan: DebugPlan) -> DebugPlan {
    if let Some(debuggee) = plan.debuggee.as_mut()
        && let Some(classpath) = classpath_file(&debuggee.line.args)
    {
        debuggee.agent = Some(ClasspathAgent {
            classpath,
            prefix: AGENT_PREFIX.to_owned(),
            option: AGENT_OPTION.to_owned(),
            after: "-agentlib:jdwp".to_owned(),
        });
    }
    plan
}

pub fn debug_mode(settings: &Settings, configuration: &Configuration) -> bool {
    if configuration.kind != crate::java_run::APPLICATION {
        return false;
    }
    match configuration.options.get(OPTION).and_then(serde_json::Value::as_str) {
        Some("on") => true,
        Some("off") => false,
        _ => settings.get_or(DEBUG_MODE, true),
    }
}

pub fn prepare(settings: &Settings, configuration: &Configuration, plan: DebugPlan, trusted: bool) -> DebugPlan {
    let plan = if debug_mode(settings, configuration) { with_debug_mode(plan) } else { plan };
    if trusted && tracks(configuration) { with_probes(plan) } else { plan }
}

fn mentions_debug_mode(args: &[String]) -> bool {
    args.iter().any(|arg| arg == DEBUG_MODE_FLAG || arg.starts_with(&format!("{DEBUG_MODE_FLAG}=")))
}

pub fn with_debug_mode(mut plan: DebugPlan) -> DebugPlan {
    let Some(debuggee) = plan.debuggee.as_mut() else { return plan };
    let line = &mut debuggee.line;
    if mentions_debug_mode(&line.args) {
        return plan;
    }
    let at = line.args.iter().position(|arg| arg.starts_with("-agentlib:jdwp")).map_or(0, |index| index + 1);
    line.args.insert(at, DEBUG_MODE_FLAG.to_owned());
    let agent = line.args.get(at.wrapping_sub(1)).filter(|_| at > 0).cloned();
    line.display = match agent.and_then(|agent| line.display.find(&agent).map(|found| found + agent.len())) {
        Some(end) => format!("{} {DEBUG_MODE_FLAG}{}", &line.display[..end], &line.display[end..]),
        None => match line.display.split_once(' ') {
            Some((java, rest)) => format!("{java} {DEBUG_MODE_FLAG} {rest}"),
            None => format!("{} {DEBUG_MODE_FLAG}", line.display),
        },
    };
    plan
}

pub const FRAME_LIMIT: usize = 8;

pub const COROUTINE_LIMIT: usize = 200;

pub const STACK_LIMIT: usize = 32;

const FIELD: char = '\u{1e}';

const FRAME: char = '\u{1f}';

const MORE: char = '\u{1d}';

pub const DUMP: &str = r#"java.lang.StringBuilder zenOut = new java.lang.StringBuilder();
int zenCount = 0;
for (java.lang.Object zenItem : kotlinx.coroutines.debug.DebugProbes.INSTANCE.dumpCoroutinesInfo()) {
    if (zenCount++ == 200) {
        zenOut.append('\u001d');
        break;
    }
    kotlinx.coroutines.debug.CoroutineInfo zenInfo = (kotlinx.coroutines.debug.CoroutineInfo) zenItem;
    java.lang.Object zenName = zenInfo.getContext().get(kotlinx.coroutines.CoroutineName.Key);
    zenOut.append(zenInfo.getState()).append('\u001e');
    zenOut.append(zenName == null ? "" : ((kotlinx.coroutines.CoroutineName) zenName).getName()).append('\u001e');
    zenOut.append(zenInfo.getContext().get(kotlin.coroutines.ContinuationInterceptor.Key)).append('\u001e');
    zenOut.append(zenInfo.getJob()).append('\u001e');
    int zenDepth = 0;
    for (java.lang.Object zenFrame : zenInfo.lastObservedStackTrace()) {
        if (zenDepth++ == 32) {
            break;
        }
        zenOut.append(zenFrame).append('\u001f');
    }
    zenOut.append('\n');
}
return zenOut.toString();"#;

/// A coroutine of the debugged program, as kotlinx-coroutines-debug last saw it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Coroutine {
    /// `CREATED`, `RUNNING` or `SUSPENDED`.
    pub state: String,
    /// Its `CoroutineName`; absent when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its dispatcher, such as `Dispatchers.Default`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatcher: Option<String>,
    /// Its job as the library prints it, such as `"worker#2":StandaloneCoroutine{Active}@40a76a0c`.
    pub job: String,
    /// Where it last ran or suspended, the innermost first, at most 32.
    pub frames: Vec<String>,
}

pub fn parse_dump(shown: &str) -> (Vec<Coroutine>, bool) {
    let text = shown.strip_prefix('"').and_then(|text| text.strip_suffix('"')).unwrap_or(shown);
    let mut more = false;
    let mut coroutines = Vec::new();
    for line in text.split('\n') {
        if line.starts_with(MORE) {
            more = true;
            continue;
        }
        let fields: Vec<&str> = line.split(FIELD).collect();
        let [state, name, dispatcher, job, frames] = fields[..] else { continue };
        let known = |text: &str| (!text.is_empty() && text != "null").then(|| text.to_owned());
        coroutines.push(Coroutine {
            state: state.to_owned(),
            name: known(name),
            dispatcher: known(dispatcher),
            job: job.to_owned(),
            frames: frames.split(FRAME).filter(|frame| !frame.is_empty()).map(str::to_owned).collect(),
        });
    }
    (coroutines, more)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zen_run::{CommandLine, Debuggee};

    use super::*;

    fn application(option: Option<&str>) -> Configuration {
        let mut options = serde_json::json!({ "mainClass": "shop.AppKt" });
        if let Some(option) = option {
            options[OPTION] = serde_json::json!(option);
        }
        Configuration { kind: crate::java_run::APPLICATION.to_owned(), options, ..Configuration::default() }
    }

    fn plan() -> DebugPlan {
        let line = CommandLine {
            program: PathBuf::from("/jdk/bin/java"),
            args: vec![
                "-agentlib:jdwp=transport=dt_socket,address=1".into(),
                "-cp".into(),
                "@/run/classpath.args".into(),
                "shop.AppKt".into(),
            ],
            cwd: PathBuf::from("/p"),
            env: Vec::new(),
            display: "java -agentlib:jdwp=transport=dt_socket,address=1 -cp @/run/classpath.args shop.AppKt".into(),
        };
        DebugPlan { debuggee: Some(Debuggee { line, ready: String::new(), agent: None }), ..DebugPlan::default() }
    }

    #[test]
    fn the_setting_decides_unless_the_configuration_says_otherwise() {
        let mut settings = Settings::default();
        assert!(debug_mode(&settings, &application(None)), "on by default");
        settings.set(DEBUG_MODE, false).unwrap();
        assert!(!debug_mode(&settings, &application(None)));
        assert!(!debug_mode(&settings, &application(Some("default"))));
        assert!(debug_mode(&settings, &application(Some("on"))), "the configuration wins");
        settings.set(DEBUG_MODE, true).unwrap();
        assert!(!debug_mode(&settings, &application(Some("off"))));
        let other = Configuration { kind: "gradle.task".into(), ..Configuration::default() };
        assert!(!debug_mode(&settings, &other), "only Java and Kotlin applications");
    }

    #[test]
    fn a_debug_plan_gets_the_flag_only_when_debug_mode_is_on_for_it() {
        let mut settings = Settings::default();
        let flagged =
            |plan: &DebugPlan| plan.debuggee.as_ref().unwrap().line.args.contains(&DEBUG_MODE_FLAG.to_owned());
        assert!(flagged(&prepare(&settings, &application(None), plan(), true)));
        assert!(!flagged(&prepare(&settings, &application(Some("off")), plan(), true)));
        settings.set(DEBUG_MODE, false).unwrap();
        assert!(!flagged(&prepare(&settings, &application(None), plan(), true)));
        let other = Configuration { kind: "gradle.task".into(), ..Configuration::default() };
        assert!(!flagged(&prepare(&Settings::default(), &other, plan(), true)));
    }

    #[test]
    fn probes_are_asked_for_only_by_the_configuration_and_only_in_a_trusted_project() {
        let settings = Settings::default();
        let mut tracking = application(None);
        tracking.options[TRACK] = serde_json::json!(true);
        let agent = |plan: DebugPlan| plan.debuggee.unwrap().agent;
        assert_eq!(agent(prepare(&settings, &application(None), plan(), true)), None, "off by default");
        assert_eq!(agent(prepare(&settings, &tracking, plan(), false)), None, "never in an untrusted project");
        let asked = agent(prepare(&settings, &tracking, plan(), true)).expect("asked for");
        assert_eq!(asked.classpath, PathBuf::from("/run/classpath.args"), "the run's own resolved classpath file");
        assert_eq!((asked.prefix.as_str(), asked.option.as_str()), (AGENT_PREFIX, AGENT_OPTION));
    }

    #[test]
    fn the_flag_goes_after_the_debugger_agent_and_is_shown_in_the_command_line() {
        let plan = with_debug_mode(plan());
        let line = &plan.debuggee.as_ref().unwrap().line;
        assert_eq!(line.args[1], DEBUG_MODE_FLAG);
        assert_eq!(line.args.last().map(String::as_str), Some("shop.AppKt"));
        assert_eq!(
            line.display,
            "java -agentlib:jdwp=transport=dt_socket,address=1 -Dkotlinx.coroutines.debug -cp @/run/classpath.args shop.AppKt"
        );
        let again = with_debug_mode(plan.clone());
        assert_eq!(again, plan, "never twice, and a flag the user gave is kept as it is");
    }

    #[test]
    fn the_dump_reads_one_coroutine_a_line_and_says_when_there_are_more() {
        let shown = "\"RUNNING\u{1e}root\u{1e}BlockingEventLoop@44ea0a44\u{1e}\"root#1\":BlockingCoroutine{Active}@659ea564\u{1e}probe.ProbeKt$start$1.invokeSuspend(probe.kt:9)\u{1f}\n\
             SUSPENDED\u{1e}\u{1e}null\u{1e}StandaloneCoroutine{Active}@40a76a0c\u{1e}\n\
             broken line\n\
             \u{1d}\"";
        let (coroutines, more) = parse_dump(shown);
        assert!(more);
        assert_eq!(
            coroutines,
            [
                Coroutine {
                    state: "RUNNING".into(),
                    name: Some("root".into()),
                    dispatcher: Some("BlockingEventLoop@44ea0a44".into()),
                    job: "\"root#1\":BlockingCoroutine{Active}@659ea564".into(),
                    frames: vec!["probe.ProbeKt$start$1.invokeSuspend(probe.kt:9)".into()],
                },
                Coroutine {
                    state: "SUSPENDED".into(),
                    name: None,
                    dispatcher: None,
                    job: "StandaloneCoroutine{Active}@40a76a0c".into(),
                    frames: Vec::new(),
                },
            ]
        );
        assert_eq!(parse_dump("\"\""), (Vec::new(), false));
    }

    #[test]
    fn the_coroutines_view_is_offered_for_the_kinds_that_run_a_jvm() {
        assert!(jvm_kind("java.application") && jvm_kind("junit") && jvm_kind("springBoot"));
        assert!(!jvm_kind("go.application") && !jvm_kind("cargo.run"));
    }

    #[test]
    fn the_dump_is_bounded_by_the_limits_it_states() {
        assert!(DUMP.contains(&format!("zenCount++ == {COROUTINE_LIMIT}")));
        assert!(DUMP.contains(&format!("zenDepth++ == {STACK_LIMIT}")));
        assert!(DUMP.contains("kotlinx.coroutines.debug.DebugProbes.INSTANCE.dumpCoroutinesInfo()"));
    }
}
