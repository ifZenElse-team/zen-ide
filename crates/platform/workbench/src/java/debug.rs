use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zen_dap::SourceBreakpoint;
use zen_run::{AdapterLine, CommandLine, DebugPlan, Debuggee, LaunchResolve};

use crate::build_tools::BuildEnv;

use super::junit::server_adapter;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JavaProgram {
    pub main_class: String,
    pub args: Vec<String>,
    pub vm_args: String,
    pub env: Vec<(String, String)>,
}

pub fn application_plan(program: &JavaProgram, root: &Path, cwd: &Path) -> DebugPlan {
    let mut adapter = server_adapter();
    if let Some(server) = adapter.server.as_mut() {
        server.resolve.push(LaunchResolve {
            command: "vscode.java.resolveClasspath".to_owned(),
            arguments: vec![serde_json::Value::from(program.main_class.as_str()), serde_json::Value::Null],
            fields: BTreeMap::from([
                ("/0".to_owned(), "modulePaths".to_owned()),
                ("/1".to_owned(), "classPaths".to_owned()),
            ]),
        });
    }
    let env: serde_json::Map<String, serde_json::Value> =
        program.env.iter().map(|(name, value)| (name.clone(), serde_json::Value::from(value.as_str()))).collect();
    let mut launch = serde_json::json!({
        "request": "launch",
        "mainClass": program.main_class,
        "args": join_arguments(&program.args),
        "vmArgs": program.vm_args,
        "cwd": cwd.to_string_lossy(),
        "env": env,
        "console": "internalConsole",
        "stopOnEntry": false,
    });
    if let Some(home) = crate::jdk::project_home(root) {
        let java = home.join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
        launch["javaExec"] = serde_json::Value::from(java.to_string_lossy().into_owned());
    }
    DebugPlan { build: Vec::new(), adapter, launch, debuggee: None }
}

pub fn attach_launch(port: u16, root: &Path) -> serde_json::Value {
    let mut launch =
        serde_json::json!({ "request": "attach", "hostName": "localhost", "port": port, "timeout": 60_000 });
    let sources: Vec<String> =
        super::parser::source_roots(root).iter().map(|folder| folder.to_string_lossy().into_owned()).collect();
    if !sources.is_empty() {
        launch["sourcePaths"] = serde_json::Value::from(sources);
    }
    launch
}

pub fn attach_plan(build: Vec<CommandLine>, program: CommandLine, root: &Path) -> Result<DebugPlan, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    let port = listener.local_addr().map_err(|error| error.to_string())?.port();
    drop(listener);
    let mut line = program;
    let agent = format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:{port}");
    line.args.insert(0, agent.clone());
    line.display = match line.display.split_once(' ') {
        Some((java, rest)) => format!("{java} {agent} {rest}"),
        None => format!("{} {agent}", line.display),
    };
    Ok(DebugPlan {
        build,
        adapter: server_adapter(),
        launch: attach_launch(port, root),
        debuggee: Some(Debuggee { line, ready: super::junit::DEBUG_READY.to_owned(), agent: None }),
    })
}

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    listener.local_addr().map(|address| address.port()).map_err(|error| error.to_string())
}

fn suspended_plan(line: CommandLine, port: u16, root: &Path) -> DebugPlan {
    DebugPlan {
        build: Vec::new(),
        adapter: server_adapter(),
        launch: attach_launch(port, root),
        debuggee: Some(Debuggee { line, ready: super::junit::DEBUG_READY.to_owned(), agent: None }),
    }
}

pub fn gradle_plan(program: CommandLine, script: &Path, root: &Path) -> Result<DebugPlan, String> {
    let port = free_port()?;
    let mut line = program;
    line.args.extend(["-I".to_owned(), script.display().to_string(), format!("-Dzen.debug.port={port}")]);
    Ok(suspended_plan(line, port, root))
}

pub fn options_plan(program: CommandLine, variable: &str, root: &Path) -> Result<DebugPlan, String> {
    let port = free_port()?;
    let mut line = program;
    let agent = format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:{port}");
    let inherited = line
        .env
        .iter()
        .rev()
        .find(|(name, _)| name == variable)
        .map(|(_, value)| value.clone())
        .or_else(|| std::env::var(variable).ok())
        .unwrap_or_default();
    let value = if inherited.trim().is_empty() { agent } else { format!("{} {agent}", inherited.trim()) };
    line.env.retain(|(name, _)| name != variable);
    line.env.push((variable.to_owned(), value));
    Ok(suspended_plan(line, port, root))
}

pub struct KotlinBreakpoints {
    env: BuildEnv,
    folder: PathBuf,
}

pub fn source_map(adapter: &AdapterLine, env: BuildEnv, scratch: &Path) -> Option<Arc<dyn zen_dap::SourceMap>> {
    let server = adapter.server.as_ref()?;
    (server.server == super::junit::JDTLS).then(|| {
        Arc::new(KotlinBreakpoints { env, folder: scratch.join("kotlin-breakpoints") }) as Arc<dyn zen_dap::SourceMap>
    })
}

pub fn shadow_source(class: &str, lines: &[i64]) -> Option<String> {
    let (package, simple) = class.rsplit_once('.').unwrap_or(("", class));
    let identifier = |name: &str| {
        name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
            && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    };
    if !identifier(simple) || !package.split('.').all(|part| package.is_empty() || identifier(part)) {
        return None;
    }
    let last = lines.iter().copied().max().filter(|last| *last >= 1)?;
    let mut out = String::new();
    for line in 1..=last {
        if line == 1 {
            if !package.is_empty() {
                out.push_str(&format!("package {package}; "));
            }
            out.push_str(&format!("class {simple} {{ void zen() {{ "));
        }
        if lines.contains(&line) {
            out.push_str(&format!("int zen{line} = {line}; "));
        }
        if line == last {
            out.push_str("} }");
        }
        out.push('\n');
    }
    Some(out)
}

impl KotlinBreakpoints {
    fn link_classes(&self, folder: &Path, links: &Path, depth: usize) {
        let Ok(entries) = std::fs::read_dir(folder) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 16 {
                    self.link_classes(&path, links, depth + 1);
                }
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "kt") {
                continue;
            }
            let Some(file) = std::fs::read_to_string(&path).ok().and_then(|text| self.env.jvm_file(&path, &text))
            else {
                continue;
            };
            for class in std::iter::once(&file.file_class).chain(file.classes.iter().map(|class| &class.name)) {
                let link = links.join(class.replace('.', "/"));
                if let Some(parent) = link.parent()
                    && std::fs::create_dir_all(parent).is_ok()
                {
                    let _ = std::fs::remove_file(&link);
                    #[cfg(unix)]
                    let _ = std::os::unix::fs::symlink(&path, &link);
                }
            }
        }
    }
}

impl zen_dap::SourceMap for KotlinBreakpoints {
    fn prepare(&self, launch: &mut serde_json::Value) {
        let roots: Vec<PathBuf> = launch["sourcePaths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|root| root.as_str().map(PathBuf::from))
            .collect();
        let links = self.folder.join("classes");
        for root in &roots {
            self.link_classes(root, &links, 0);
        }
        if links.is_dir()
            && let Some(paths) = launch["sourcePaths"].as_array_mut()
        {
            paths.push(serde_json::Value::from(links.to_string_lossy().into_owned()));
        }
    }

    fn split(&self, path: &Path, breakpoints: &[SourceBreakpoint]) -> Option<Vec<(PathBuf, Vec<usize>)>> {
        if path.extension().is_none_or(|extension| extension != "kt") {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let file = self.env.jvm_file(path, &text)?;
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for (index, breakpoint) in breakpoints.iter().enumerate() {
            let line = u32::try_from(breakpoint.line - 1).unwrap_or(0);
            let class = file.class_at(line).to_owned();
            match groups.iter_mut().find(|(known, _)| *known == class) {
                Some((_, indices)) => indices.push(index),
                None => groups.push((class, vec![index])),
            }
        }
        let hash = path
            .to_string_lossy()
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
        let folder = self.folder.join(format!("{hash:016x}"));
        std::fs::create_dir_all(&folder).ok()?;
        let mut parts = Vec::new();
        for (class, indices) in groups {
            let lines: Vec<i64> = indices.iter().map(|index| breakpoints[*index].line).collect();
            let simple = class.rsplit('.').next().unwrap_or(&class).to_owned();
            let target = folder.join(format!("{simple}.java"));
            let Some(source) = shadow_source(&class, &lines) else { continue };
            std::fs::write(&target, source).ok()?;
            parts.push((target, indices));
        }
        Some(parts)
    }
}

pub fn join_arguments(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains([' ', '\t', '"', '\'']) {
                format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_application_is_launched_by_java_debug_with_the_resolved_class_path() {
        let program = JavaProgram {
            main_class: "com.acme.App".into(),
            args: vec!["--port".into(), "8080".into()],
            vm_args: "-Xmx256m".into(),
            env: vec![("MODE".into(), "dev".into())],
        };
        let plan = application_plan(&program, Path::new("/nowhere"), Path::new("/work"));
        assert_eq!(plan.launch["mainClass"], "com.acme.App");
        assert_eq!(plan.launch["env"]["MODE"], "dev");
        assert_eq!(plan.launch["args"], "--port 8080");
        assert_eq!(join_arguments(&["a b".into(), "c".into(), String::new()]), "\"a b\" c \"\"");
        let server = plan.adapter.server.unwrap();
        assert_eq!((server.server.as_str(), server.command.as_str()), ("jdtls", "vscode.java.startDebugSession"));
        assert_eq!(server.resolve[0].command, "vscode.java.resolveClasspath");
        assert_eq!(server.resolve[0].fields["/1"], "classPaths");
        assert!(plan.debuggee.is_none() && plan.build.is_empty());
        let views = plan.adapter.views.unwrap();
        assert_eq!(views.command, "vscode.java.updateDebugSettings");
        let settings = |arguments: &[serde_json::Value]| -> serde_json::Value {
            serde_json::from_str(arguments[0].as_str().unwrap()).unwrap()
        };
        assert_eq!(settings(&views.logical)["showLogicalStructure"], true);
        assert_eq!(settings(&views.raw)["showLogicalStructure"], false);
    }

    #[test]
    fn kotlin_breakpoints_reach_java_debug_through_a_java_file_of_their_class() {
        assert_eq!(
            shadow_source("com.acme.AppKt", &[2, 4]).unwrap(),
            "package com.acme; class AppKt { void zen() { \nint zen2 = 2; \n\nint zen4 = 4; } }\n"
        );
        assert_eq!(shadow_source("Top", &[1]).unwrap(), "class Top { void zen() { int zen1 = 1; } }\n");
        assert!(shadow_source("a.two words", &[3]).is_none());
        assert!(shadow_source("a.B", &[]).is_none());

        let dir = tempfile::tempdir().unwrap();
        let kotlin = dir.path().join("App.kt");
        std::fs::write(&kotlin, "package a\n\nfun main() {\n}\n\nclass Shop {\n    fun f() {}\n}\n").unwrap();
        let env = BuildEnv::default();
        env.set_jvm_reader(Some(Arc::new(|_: &Path, _: &str| {
            let shop = zen_syntax::JvmClass { name: "a.Shop".into(), first_line: 5, last_line: 7 };
            let file = zen_syntax::JvmFile { package: "a".into(), classes: vec![shop], ..Default::default() };
            Some(file.named_after("App"))
        })));
        let java = AdapterLine { server: Some(server_adapter().server.unwrap()), ..AdapterLine::default() };
        assert!(source_map(&AdapterLine::default(), env.clone(), dir.path()).is_none(), "only java-debug needs it");
        let map = source_map(&java, env, dir.path()).unwrap();
        let at = |line: i64| SourceBreakpoint { line, ..SourceBreakpoint::default() };
        let parts = map.split(&kotlin, &[at(4), at(7), at(3)]).unwrap();
        let shown: Vec<(String, Vec<usize>)> = parts
            .iter()
            .map(|(path, indices)| (path.file_name().unwrap().to_string_lossy().into_owned(), indices.clone()))
            .collect();
        assert_eq!(shown, [("AppKt.java".to_owned(), vec![0, 2]), ("Shop.java".to_owned(), vec![1])]);
        assert!(std::fs::read_to_string(&parts[1].0).unwrap().starts_with("package a; class Shop {"));
        assert!(map.split(&dir.path().join("App.java"), &[at(1)]).is_none(), "Java files go as they are");
        let mut launch = serde_json::json!({ "sourcePaths": [dir.path()] });
        map.prepare(&mut launch);
        let links = dir.path().join("kotlin-breakpoints/classes");
        assert_eq!(launch["sourcePaths"][1], serde_json::json!(links), "frames of classes with inline code find them");
        #[cfg(unix)]
        for class in ["a/AppKt", "a/Shop"] {
            assert_eq!(std::fs::read_link(links.join(class)).unwrap(), kotlin, "{class}");
        }
    }

    #[test]
    fn a_java_command_is_debugged_by_attaching_to_it_as_it_runs() {
        let build = CommandLine { program: "mvn".into(), args: vec!["compile".into()], ..CommandLine::default() };
        let program = CommandLine {
            program: "/jdk/bin/java".into(),
            args: vec!["-Xmx1g".into(), "-cp".into(), "@cp.args".into(), "shop.App".into()],
            display: "java -Xmx1g -cp @cp.args shop.App".into(),
            ..CommandLine::default()
        };
        let root = tempfile::tempdir().unwrap();
        for folder in ["src/main/kotlin", "src/test/java", "build/src/main/java"] {
            std::fs::create_dir_all(root.path().join(folder)).unwrap();
        }
        let plan = attach_plan(vec![build.clone()], program, root.path()).unwrap();
        let sources: Vec<String> = [root.path().join("src/main/kotlin"), root.path().join("src/test/java")]
            .iter()
            .map(|folder| folder.to_string_lossy().into_owned())
            .collect();
        assert_eq!(plan.launch["sourcePaths"], serde_json::json!(sources), "Kotlin frames find their files");
        assert_eq!(plan.build, [build]);
        let debuggee = plan.debuggee.unwrap();
        assert!(
            debuggee.line.args[0]
                .starts_with("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:")
        );
        assert_eq!(&debuggee.line.args[1..], ["-Xmx1g", "-cp", "@cp.args", "shop.App"]);
        assert!(debuggee.line.display.starts_with("java -agentlib:jdwp="));
        assert_eq!(debuggee.ready, "Listening for transport dt_socket at address");
        assert_eq!(plan.launch["request"], "attach");
        assert!(plan.adapter.server.is_some());
    }
}
