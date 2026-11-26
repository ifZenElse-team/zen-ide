use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use roxmltree::Node;
use zen_project::settings::{ForeignRunConfiguration, ForeignTask, ReadFiles, SettingsProblem};

use super::xml::{self, Macros};
use super::{IDEA, RUN, has_suffix};

const MANAGER: &str = "ProjectRunConfigurationManager";
const FOLDER: &str = "runConfigurations";
const OWN_ATTRIBUTES: &[&str] = &["name", "type", "factoryName", "default"];
const MAX_OPTION_DEPTH: usize = 16;

pub(super) fn read(
    root: &Path,
    files: &dyn ReadFiles,
    macros: &Macros,
) -> (Vec<ForeignRunConfiguration>, Vec<SettingsProblem>) {
    let idea = files.list(&Path::new(IDEA).join(FOLDER)).into_iter().filter(|path| has_suffix(path, ".xml"));
    let run = files.list(Path::new(RUN)).into_iter().filter(|path| has_suffix(path, ".run.xml"));
    let mut configurations = Vec::new();
    let mut problems = Vec::new();
    for relative in idea.chain(run) {
        let Some(text) = files.read(&relative) else { continue };
        let file = root.join(&relative);
        match parse(&file, &text, macros) {
            Ok(read) => configurations.extend(read),
            Err(message) => problems.push(SettingsProblem { file, message }),
        }
    }
    (configurations, problems)
}

fn parse(file: &Path, text: &str, macros: &Macros) -> Result<Vec<ForeignRunConfiguration>, String> {
    let document = xml::parse(text).map_err(|error| format!("cannot parse the run configuration: {error}"))?;
    let manager = xml::component(&document, MANAGER).ok_or_else(|| format!("no {MANAGER} component"))?;
    let mut read = Vec::new();
    for node in manager.children().filter(|node| node.has_tag_name("configuration")) {
        if node.attribute("default") == Some("true") {
            continue;
        }
        let (Some(name), Some(type_id)) = (node.attribute("name"), node.attribute("type")) else {
            return Err("a run configuration without a name or a type".to_owned());
        };
        read.push(configuration(file.to_owned(), name, type_id, node, macros));
    }
    Ok(read)
}

fn configuration(
    source_file: PathBuf,
    name: &str,
    type_id: &str,
    node: Node<'_, '_>,
    macros: &Macros,
) -> ForeignRunConfiguration {
    let mut options = BTreeMap::new();
    for attribute in node.attributes().filter(|attribute| !OWN_ATTRIBUTES.contains(&attribute.name())) {
        options.insert(attribute.name().to_owned(), macros.expand(attribute.value()));
    }
    let mut env = Vec::new();
    for child in node.children().filter(|child| child.has_tag_name("envs")) {
        for variable in child.children().filter(|variable| variable.has_tag_name("env")) {
            if let Some(key) = variable.attribute("name") {
                env.push((key.to_owned(), macros.expand(variable.attribute("value").unwrap_or_default())));
            }
        }
    }
    flatten(node, "", macros, &mut options, 0);
    ForeignRunConfiguration {
        source_file,
        name: name.to_owned(),
        type_id: type_id.to_owned(),
        factory: node.attribute("factoryName").map(str::to_owned),
        options,
        env,
        shared: true,
        before_launch: before_launch(node, macros),
    }
}

fn before_launch(node: Node<'_, '_>, macros: &Macros) -> Vec<ForeignTask> {
    node.children()
        .filter(|child| child.has_tag_name("method"))
        .flat_map(|method| method.children().filter(|child| child.has_tag_name("option")))
        .filter_map(|option| {
            let id = option.attribute("name")?;
            let options = option
                .attributes()
                .filter(|attribute| !matches!(attribute.name(), "name" | "enabled"))
                .map(|attribute| (attribute.name().to_owned(), macros.expand(attribute.value())))
                .collect();
            Some(ForeignTask { id: id.to_owned(), enabled: option.attribute("enabled") == Some("true"), options })
        })
        .collect()
}

fn key<'a>(node: Node<'a, '_>) -> &'a str {
    let named = match node.tag_name().name() {
        "option" => node.attribute("name"),
        "entry" => node.attribute("key"),
        _ => None,
    };
    named.unwrap_or_else(|| node.tag_name().name())
}

fn flatten(node: Node<'_, '_>, prefix: &str, macros: &Macros, options: &mut BTreeMap<String, String>, depth: usize) {
    if depth > MAX_OPTION_DEPTH {
        return;
    }
    let top = depth == 0;
    let children: Vec<Node<'_, '_>> =
        node.children().filter(|child| child.is_element() && !(top && child.has_tag_name("envs"))).collect();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for child in &children {
        *counts.entry(key(*child)).or_default() += 1;
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for child in children {
        let own = key(child);
        let index = seen.entry(own).or_default();
        let mut path = if prefix.is_empty() { own.to_owned() } else { format!("{prefix}.{own}") };
        if counts[own] > 1 {
            path = format!("{path}.{index}");
        }
        *index += 1;
        let named = matches!(child.tag_name().name(), "option" | "entry");
        for attribute in child.attributes() {
            let label = attribute.name();
            if named && (label == "name" || label == "key") {
                continue;
            }
            let value = macros.expand(attribute.value());
            if label == "value" {
                options.insert(path.clone(), value);
            } else {
                options.insert(format!("{path}.{label}"), value);
            }
        }
        flatten(child, &path, macros, options, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Memory(BTreeMap<PathBuf, String>);

    impl Memory {
        fn with(files: &[(&str, &str)]) -> Self {
            Self(files.iter().map(|(path, text)| (PathBuf::from(path), (*text).to_owned())).collect())
        }
    }

    impl ReadFiles for Memory {
        fn read(&self, relative: &Path) -> Option<String> {
            self.0.get(relative).cloned()
        }

        fn list(&self, relative_dir: &Path) -> Vec<PathBuf> {
            let mut entries: Vec<PathBuf> = self
                .0
                .keys()
                .filter_map(|path| {
                    let rest = path.strip_prefix(relative_dir).ok()?;
                    let first = rest.components().next()?;
                    Some(relative_dir.join(first))
                })
                .collect();
            entries.dedup();
            entries
        }
    }

    const BUILD: &str = r#"<component name="ProjectRunConfigurationManager">
  <configuration default="false" name="Build server" type="GoApplicationRunConfiguration" factoryName="Go Application" folderName="Servers">
    <module name="shop" />
    <working_directory value="$PROJECT_DIR$/cmd" />
    <go_parameters value="-race -tags dev" />
    <parameters value="--port 8080" />
    <kind value="PACKAGE" />
    <package value="example.org/shop/cmd/server" />
    <directory value="$PROJECT_DIR$" />
    <filePath value="$PROJECT_DIR$/cmd/server/main.go" />
    <output_directory value="$PROJECT_DIR$/bin" />
    <envs>
      <env name="PORT" value="8080" />
      <env name="DATA" value="$PROJECT_DIR$/data" />
      <env name="EMPTY" />
    </envs>
    <method v="2">
      <option name="Make" enabled="true" />
    </method>
  </configuration>
</component>"#;

    const TEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectRunConfigurationManager">
    <configuration default="true" type="GoTestRunConfiguration" factoryName="Go Test">
      <framework value="gotest" />
    </configuration>
    <configuration name="Add tests" type="GoTestRunConfiguration" factoryName="Go Test">
      <kind value="PACKAGE" />
      <package value="example.org/shop/calc" />
      <framework value="gotest" />
      <pattern value="^TestAdd$" />
      <option name="EXTRA" value="x" />
      <option name="TAGS">
        <list>
          <option value="unit" />
          <option value="fast" />
        </list>
      </option>
    </configuration>
  </component>
</project>"#;

    const UNKNOWN: &str = r#"<component name="ProjectRunConfigurationManager">
  <configuration name="Lint" type="ShConfigurationType">
    <option name="SCRIPT_TEXT" value="make lint" />
    <option name="INDEPENDENT_SCRIPT_PATH" value="true" />
    <envs />
  </configuration>
</component>"#;

    fn load(files: &[(&str, &str)]) -> (Vec<ForeignRunConfiguration>, Vec<SettingsProblem>) {
        let macros = Macros::new(Path::new("/p"), None);
        read(Path::new("/p"), &Memory::with(files), &macros)
    }

    fn strings(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
    }

    #[test]
    fn a_go_build_configuration_is_flattened_with_macros_expanded() {
        let (read, problems) = load(&[(".idea/runConfigurations/Build_server.xml", BUILD)]);
        assert!(problems.is_empty(), "{problems:?}");
        let [build] = read.as_slice() else { panic!("{read:?}") };
        assert_eq!(build.source_file, PathBuf::from("/p/.idea/runConfigurations/Build_server.xml"));
        assert_eq!(build.name, "Build server");
        assert_eq!(build.type_id, "GoApplicationRunConfiguration");
        assert_eq!(build.factory.as_deref(), Some("Go Application"));
        assert!(build.shared);
        assert_eq!(
            build.options,
            strings(&[
                ("folderName", "Servers"),
                ("module.name", "shop"),
                ("working_directory", "/p/cmd"),
                ("go_parameters", "-race -tags dev"),
                ("parameters", "--port 8080"),
                ("kind", "PACKAGE"),
                ("package", "example.org/shop/cmd/server"),
                ("directory", "/p"),
                ("filePath", "/p/cmd/server/main.go"),
                ("output_directory", "/p/bin"),
                ("method.v", "2"),
                ("method.Make.enabled", "true"),
            ])
        );
        let env: Vec<(&str, &str)> = build.env.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
        assert_eq!(env, [("PORT", "8080"), ("DATA", "/p/data"), ("EMPTY", "")]);
    }

    #[test]
    fn before_launch_tasks_are_read_in_order_with_their_attributes() {
        let text = r#"<component name="ProjectRunConfigurationManager">
  <configuration name="Serve" type="GoApplicationRunConfiguration">
    <method v="2">
      <option name="ToolBeforeRunTask" enabled="true" actionId="Tool_External Tools_fmt" />
      <option name="Make" enabled="false" />
      <option name="RunConfigurationTask" enabled="true" run_configuration_name="Build" run_configuration_type="GoApplicationRunConfiguration" />
      <option name="Maven.BeforeRunTask" enabled="true" file="$PROJECT_DIR$/pom.xml" goal="clean" />
    </method>
  </configuration>
</component>"#;
        let (read, problems) = load(&[(".run/Serve.run.xml", text)]);
        assert!(problems.is_empty(), "{problems:?}");
        let tasks: Vec<(&str, bool)> =
            read[0].before_launch.iter().map(|task| (task.id.as_str(), task.enabled)).collect();
        assert_eq!(
            tasks,
            [
                ("ToolBeforeRunTask", true),
                ("Make", false),
                ("RunConfigurationTask", true),
                ("Maven.BeforeRunTask", true)
            ]
        );
        assert_eq!(read[0].before_launch[0].options, strings(&[("actionId", "Tool_External Tools_fmt")]));
        assert_eq!(read[0].before_launch[3].options, strings(&[("file", "/p/pom.xml"), ("goal", "clean")]));
    }

    #[test]
    fn templates_are_skipped_and_lists_are_numbered() {
        let (read, problems) = load(&[(".run/Add_tests.run.xml", TEST)]);
        assert!(problems.is_empty(), "{problems:?}");
        let [test] = read.as_slice() else { panic!("{read:?}") };
        assert_eq!(test.name, "Add tests");
        assert_eq!(test.factory.as_deref(), Some("Go Test"));
        assert!(test.env.is_empty());
        assert_eq!(
            test.options,
            strings(&[
                ("kind", "PACKAGE"),
                ("package", "example.org/shop/calc"),
                ("framework", "gotest"),
                ("pattern", "^TestAdd$"),
                ("EXTRA", "x"),
                ("TAGS.list.option.0", "unit"),
                ("TAGS.list.option.1", "fast"),
            ])
        );
    }

    #[test]
    fn an_unknown_type_is_kept_as_written() {
        let (read, problems) = load(&[(".run/Lint.run.xml", UNKNOWN)]);
        assert!(problems.is_empty(), "{problems:?}");
        let [lint] = read.as_slice() else { panic!("{read:?}") };
        assert_eq!(lint.type_id, "ShConfigurationType");
        assert_eq!(lint.factory, None);
        assert_eq!(lint.options, strings(&[("SCRIPT_TEXT", "make lint"), ("INDEPENDENT_SCRIPT_PATH", "true")]));
    }

    #[test]
    fn broken_and_foreign_files_are_problems_and_other_files_are_not_read() {
        let (read, problems) = load(&[
            (".idea/runConfigurations/Broken.xml", "<component name=\"ProjectRunConfigurationManager\">"),
            (".idea/runConfigurations/Other.xml", "<component name=\"Other\" />"),
            (
                ".idea/runConfigurations/Nameless.xml",
                "<component name=\"ProjectRunConfigurationManager\"><configuration type=\"X\"/></component>",
            ),
            (".idea/runConfigurations/notes.txt", BUILD),
            (".idea/misc.xml", BUILD),
            (".idea/workspace.xml", BUILD),
            (".run/Plain.xml", BUILD),
            (".run/Lint.run.xml", UNKNOWN),
        ]);
        let names: Vec<&str> = read.iter().map(|configuration| configuration.name.as_str()).collect();
        assert_eq!(names, ["Lint"]);
        let files: Vec<&Path> = problems.iter().map(|problem| problem.file.as_path()).collect();
        assert_eq!(
            files,
            [
                Path::new("/p/.idea/runConfigurations/Broken.xml"),
                Path::new("/p/.idea/runConfigurations/Nameless.xml"),
                Path::new("/p/.idea/runConfigurations/Other.xml"),
            ]
        );
        assert!(problems[0].message.starts_with("cannot parse the run configuration"), "{}", problems[0].message);
    }

    #[test]
    fn deep_nesting_is_cut_off() {
        let depth = 20_000;
        let text = format!(
            "<component name=\"ProjectRunConfigurationManager\"><configuration name=\"Deep\" type=\"T\">{}{}</configuration></component>",
            "<a>".repeat(depth),
            "</a>".repeat(depth)
        );
        let macros = Macros::new(Path::new("/p"), None);
        assert!(parse(Path::new("/p/.run/deep.run.xml"), &text, &macros).unwrap_err().contains("nested"));
        let nested = format!(
            "<component name=\"ProjectRunConfigurationManager\"><configuration name=\"Deep\" type=\"T\">{}{}</configuration></component>",
            "<a value=\"1\">".repeat(100),
            "</a>".repeat(100)
        );
        let read = parse(Path::new("/p/.run/deep.run.xml"), &nested, &macros).unwrap();
        assert!(read[0].options.keys().all(|key| key.split('.').count() <= MAX_OPTION_DEPTH + 1));
    }
}
