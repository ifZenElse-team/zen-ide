use std::path::Path;

use serde::{Deserialize, Serialize};
use zen_i18n::t;
use zen_run::{
    CommandLine, Configuration, DebugPlan, FieldKind, FieldSpec, KindSpec, OutputParser, RunContext, RunProvider,
    RunTarget, TargetKind,
};

use crate::java_run::{APPLICATION, ApplicationOptions, JavaRun, module_of};

pub const KIND: &str = "springBoot";

const ANNOTATIONS: &[&str] =
    &["@SpringBootApplication", "@org.springframework.boot.autoconfigure.SpringBootApplication"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpringBootOptions {
    pub main_class: String,
    pub module: String,
    pub jre: String,
    pub build: bool,
    pub active_profiles: String,
    pub overrides: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct SpringBootRun {
    java: JavaRun,
}

fn is_annotation(line: &str) -> bool {
    ANNOTATIONS.iter().any(|annotation| {
        line.strip_prefix(annotation).is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '(', '\t']))
    })
}

pub fn annotated_class(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let at = lines.iter().position(|line| is_annotation(line))?;
    lines[at..].iter().take(20).find_map(|line| {
        let mut words = line.split_whitespace();
        words.find(|word| *word == "class")?;
        let name: String =
            words.next()?.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
        (!name.is_empty()).then_some(name)
    })
}

fn quoted(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | '$' | '`')) {
        return arg.to_owned();
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

pub fn spring_arguments(options: &SpringBootOptions) -> Vec<String> {
    let mut args = Vec::new();
    let profiles: Vec<&str> =
        options.active_profiles.split([',', ' ']).map(str::trim).filter(|profile| !profile.is_empty()).collect();
    if !profiles.is_empty() {
        args.push(format!("--spring.profiles.active={}", profiles.join(",")));
    }
    for (name, value) in &options.overrides {
        let name = name.trim();
        if !name.is_empty() {
            args.push(format!("--{name}={value}"));
        }
    }
    args
}

impl SpringBootRun {
    pub fn new(java: JavaRun) -> Self {
        Self { java }
    }

    pub fn configuration(name: &str, main_class: &str, module: &str) -> Configuration {
        let options = SpringBootOptions {
            main_class: main_class.to_owned(),
            module: module.to_owned(),
            build: true,
            ..SpringBootOptions::default()
        };
        Configuration {
            name: name.to_owned(),
            kind: KIND.to_owned(),
            options: serde_json::to_value(options).unwrap_or_default(),
            ..Configuration::default()
        }
    }

    pub fn as_java(configuration: &Configuration) -> Configuration {
        let options: SpringBootOptions = serde_json::from_value(configuration.options.clone()).unwrap_or_default();
        let java = ApplicationOptions {
            main_class: options.main_class.clone(),
            module: options.module.clone(),
            jre: options.jre.clone(),
            build: options.build,
            coroutine_debug: String::new(),
            track_coroutines: false,
        };
        let mut common = configuration.common.clone();
        let added: Vec<String> = spring_arguments(&options).iter().map(|arg| quoted(arg)).collect();
        if !added.is_empty() {
            let own = common.program_arguments.trim();
            common.program_arguments =
                if own.is_empty() { added.join(" ") } else { format!("{own} {}", added.join(" ")) };
        }
        Configuration {
            kind: APPLICATION.to_owned(),
            options: serde_json::to_value(java).unwrap_or_default(),
            common,
            ..configuration.clone()
        }
    }

    pub fn applications(&self, root: &Path, path: &Path, text: &str) -> Vec<(u32, Configuration)> {
        let Some(class) = annotated_class(text) else { return Vec::new() };
        let module = module_of(root, path);
        let mains = self.java.mains(path, text);
        let only = mains.len() == 1;
        let file_class = path.file_stem().map(|stem| format!("{}Kt", stem.to_string_lossy())).unwrap_or_default();
        mains
            .into_iter()
            .filter(|main| {
                let simple = main.class.rsplit(['.', '$']).next().unwrap_or_default();
                only || simple == class || simple == file_class
            })
            .map(|main| (main.class_line, Self::configuration(&class, &main.class, &module)))
            .collect()
    }
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

impl RunProvider for SpringBootRun {
    fn id(&self) -> &str {
        "springBoot"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: KIND.to_owned(),
            name_message: "workbench-spring-boot-kind".to_owned(),
            fields: vec![
                field("mainClass", "workbench-java-field-main-class", FieldKind::Text),
                field("module", "workbench-java-field-module", FieldKind::Path),
                field("activeProfiles", "workbench-spring-boot-field-profiles", FieldKind::Text),
                field("overrides", "workbench-spring-boot-field-overrides", FieldKind::Env),
                field("jre", "workbench-java-field-jre", FieldKind::Path),
                field("build", "workbench-java-field-build", FieldKind::Bool),
            ],
            defaults: serde_json::to_value(SpringBootOptions { build: true, ..SpringBootOptions::default() })
                .unwrap_or_default(),
        }]
    }

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
        let Some(class) = annotated_class(text) else { return Vec::new() };
        let mains = self.java.mains(path, text);
        let configurations = self.applications(&context.root, path, text);
        mains
            .into_iter()
            .filter_map(|main| {
                let (_, configuration) = configurations.iter().find(|(line, _)| *line == main.class_line)?.clone();
                Some((main, configuration))
            })
            .flat_map(|(main, configuration)| {
                let class = class.clone();
                [main.class_line, main.line].into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().map(
                    move |line| RunTarget {
                        line,
                        kind: TargetKind::Main,
                        name: class.clone(),
                        package: String::new(),
                        configuration: configuration.clone(),
                    },
                )
            })
            .collect()
    }

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
        let java = self.java.context(root, path, text, offset)?;
        let options: ApplicationOptions = serde_json::from_value(java.options).ok()?;
        self.applications(root, path, text).into_iter().map(|(_, configuration)| configuration).find(|configuration| {
            serde_json::from_value::<SpringBootOptions>(configuration.options.clone())
                .is_ok_and(|spring| spring.main_class == options.main_class)
        })
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        if configuration.kind != KIND {
            return Err(t!("workbench-java-unknown-kind", kind = configuration.kind.clone()));
        }
        self.java.command_lines(&Self::as_java(configuration), context)
    }

    fn debug(&self, configuration: &Configuration, context: &RunContext) -> Option<Result<DebugPlan, String>> {
        self.java.debug(&Self::as_java(configuration), context)
    }

    fn parser(&self, configuration: &Configuration, context: &RunContext) -> Box<dyn OutputParser> {
        self.java.parser(&Self::as_java(configuration), context)
    }

    fn plugin(&self) -> Option<&str> {
        Some(super::PLUGIN)
    }

    fn replaces(&self) -> Option<&str> {
        Some("java")
    }
}
