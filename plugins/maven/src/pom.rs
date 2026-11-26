#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: &'static str,
    pub insert: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completions {
    pub from: usize,
    pub items: Vec<Item>,
}

const CHILDREN: &[(&str, &[&str])] = &[
    (
        "project",
        &[
            "modelVersion",
            "parent",
            "groupId",
            "artifactId",
            "version",
            "packaging",
            "name",
            "description",
            "url",
            "inceptionYear",
            "organization",
            "licenses",
            "developers",
            "contributors",
            "mailingLists",
            "prerequisites",
            "modules",
            "scm",
            "issueManagement",
            "ciManagement",
            "distributionManagement",
            "properties",
            "dependencyManagement",
            "dependencies",
            "repositories",
            "pluginRepositories",
            "build",
            "reporting",
            "profiles",
        ],
    ),
    ("parent", &["groupId", "artifactId", "version", "relativePath"]),
    ("dependencyManagement", &["dependencies"]),
    ("dependencies", &["dependency"]),
    (
        "dependency",
        &["groupId", "artifactId", "version", "type", "classifier", "scope", "systemPath", "exclusions", "optional"],
    ),
    ("exclusions", &["exclusion"]),
    ("exclusion", &["groupId", "artifactId"]),
    (
        "build",
        &[
            "defaultGoal",
            "directory",
            "finalName",
            "sourceDirectory",
            "scriptSourceDirectory",
            "testSourceDirectory",
            "outputDirectory",
            "testOutputDirectory",
            "extensions",
            "resources",
            "testResources",
            "filters",
            "pluginManagement",
            "plugins",
        ],
    ),
    ("pluginManagement", &["plugins"]),
    ("plugins", &["plugin"]),
    (
        "plugin",
        &["groupId", "artifactId", "version", "extensions", "executions", "dependencies", "inherited", "configuration"],
    ),
    ("executions", &["execution"]),
    ("execution", &["id", "phase", "goals", "inherited", "configuration"]),
    ("goals", &["goal"]),
    ("modules", &["module"]),
    ("profiles", &["profile"]),
    (
        "profile",
        &[
            "id",
            "activation",
            "build",
            "modules",
            "distributionManagement",
            "properties",
            "dependencyManagement",
            "dependencies",
            "repositories",
            "pluginRepositories",
            "reporting",
        ],
    ),
    ("activation", &["activeByDefault", "jdk", "os", "property", "file"]),
    ("repositories", &["repository"]),
    ("pluginRepositories", &["pluginRepository"]),
    ("repository", &["id", "name", "url", "layout", "releases", "snapshots"]),
    ("pluginRepository", &["id", "name", "url", "layout", "releases", "snapshots"]),
    ("releases", &["enabled", "updatePolicy", "checksumPolicy"]),
    ("snapshots", &["enabled", "updatePolicy", "checksumPolicy"]),
    ("resources", &["resource"]),
    ("testResources", &["testResource"]),
    ("resource", &["targetPath", "filtering", "directory", "includes", "excludes"]),
    ("testResource", &["targetPath", "filtering", "directory", "includes", "excludes"]),
    ("includes", &["include"]),
    ("excludes", &["exclude"]),
    ("licenses", &["license"]),
    ("license", &["name", "url", "distribution", "comments"]),
    ("developers", &["developer"]),
    ("developer", &["id", "name", "email", "url", "organization", "organizationUrl", "roles", "timezone"]),
    ("scm", &["connection", "developerConnection", "tag", "url"]),
    ("organization", &["name", "url"]),
    ("issueManagement", &["system", "url"]),
    ("ciManagement", &["system", "url"]),
    ("distributionManagement", &["repository", "snapshotRepository", "site", "downloadUrl", "relocation"]),
    ("reporting", &["excludeDefaults", "outputDirectory", "plugins"]),
];

const PHASES: &[&str] = &[
    "pre-clean",
    "clean",
    "post-clean",
    "validate",
    "initialize",
    "generate-sources",
    "process-sources",
    "generate-resources",
    "process-resources",
    "compile",
    "process-classes",
    "generate-test-sources",
    "process-test-sources",
    "generate-test-resources",
    "process-test-resources",
    "test-compile",
    "process-test-classes",
    "test",
    "prepare-package",
    "package",
    "pre-integration-test",
    "integration-test",
    "post-integration-test",
    "verify",
    "install",
    "deploy",
    "pre-site",
    "site",
    "post-site",
    "site-deploy",
];

const GOALS: &[(&str, &[&str])] = &[
    ("maven-clean-plugin", &["clean"]),
    ("maven-resources-plugin", &["resources", "testResources", "copy-resources"]),
    ("maven-compiler-plugin", &["compile", "testCompile"]),
    ("maven-surefire-plugin", &["test"]),
    ("maven-failsafe-plugin", &["integration-test", "verify"]),
    ("maven-jar-plugin", &["jar", "test-jar"]),
    ("maven-war-plugin", &["war", "exploded", "inplace"]),
    ("maven-install-plugin", &["install", "install-file"]),
    ("maven-deploy-plugin", &["deploy", "deploy-file"]),
    ("maven-site-plugin", &["site", "deploy", "run", "stage", "attach-descriptor"]),
    ("maven-source-plugin", &["jar", "jar-no-fork", "test-jar", "aggregate"]),
    ("maven-javadoc-plugin", &["javadoc", "jar", "test-javadoc", "aggregate"]),
    (
        "maven-dependency-plugin",
        &["tree", "analyze", "copy", "copy-dependencies", "unpack", "unpack-dependencies", "resolve", "go-offline"],
    ),
    ("maven-enforcer-plugin", &["enforce", "display-info"]),
    ("maven-shade-plugin", &["shade"]),
    ("maven-assembly-plugin", &["single"]),
    ("maven-antrun-plugin", &["run"]),
    ("exec-maven-plugin", &["exec", "java"]),
    ("build-helper-maven-plugin", &["add-source", "add-test-source", "add-resource", "attach-artifact"]),
    ("spring-boot-maven-plugin", &["run", "repackage", "start", "stop", "build-image", "build-info"]),
    ("jacoco-maven-plugin", &["prepare-agent", "report", "check"]),
];

const VALUES: &[(&str, &[&str])] = &[
    ("scope", &["compile", "provided", "runtime", "test", "system", "import"]),
    ("packaging", &["jar", "war", "pom", "ear", "maven-plugin", "ejb", "rar"]),
    ("type", &["jar", "pom", "war", "test-jar", "ejb-client"]),
    ("optional", &["true", "false"]),
    ("activeByDefault", &["true", "false"]),
    ("inherited", &["true", "false"]),
    ("filtering", &["true", "false"]),
    ("enabled", &["true", "false"]),
    ("extensions", &["true", "false"]),
    ("updatePolicy", &["always", "daily", "never"]),
    ("checksumPolicy", &["warn", "fail", "ignore"]),
    ("modelVersion", &["4.0.0"]),
];

const PROJECT_PROPERTIES: &[&str] = &[
    "project.groupId",
    "project.artifactId",
    "project.version",
    "project.basedir",
    "project.build.directory",
    "project.build.outputDirectory",
    "project.build.sourceEncoding",
    "project.parent.version",
    "basedir",
];

#[derive(Clone, Debug)]
struct Open {
    name: String,
    content: usize,
}

fn open_at(text: &str, caret: usize) -> (Vec<Open>, Option<usize>) {
    let bytes = text.as_bytes();
    let mut stack: Vec<Open> = Vec::new();
    let mut at = 0;
    while at < caret {
        if bytes[at] != b'<' {
            at += 1;
            continue;
        }
        let rest = &text[at..];
        let skip_to = |end: &str| rest.find(end).map(|found| at + found + end.len());
        let skipped = if rest.starts_with("<!--") {
            Some(skip_to("-->"))
        } else if rest.starts_with("<![CDATA[") {
            Some(skip_to("]]>"))
        } else if rest.starts_with("<?") {
            Some(skip_to("?>"))
        } else if rest.starts_with("<!") {
            Some(skip_to(">"))
        } else {
            None
        };
        if let Some(end) = skipped {
            match end {
                Some(end) if end <= caret => {
                    at = end;
                    continue;
                }
                _ => return (stack, None),
            }
        }
        let Some(close) = rest.find('>').map(|found| at + found) else { return (stack, Some(at)) };
        if close >= caret {
            return (stack, Some(at));
        }
        let tag = &text[at + 1..close];
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim();
            if let Some(index) = stack.iter().rposition(|open| open.name == name) {
                stack.truncate(index);
            }
        } else if !tag.ends_with('/') {
            let name: String = tag.chars().take_while(|c| !c.is_whitespace() && *c != '/').collect();
            stack.push(Open { name, content: close + 1 });
        }
        at = close + 1;
    }
    (stack, None)
}

fn word_start(text: &str, caret: usize) -> usize {
    text[..caret]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '!'))
        .last()
        .map_or(caret, |(at, _)| at)
}

fn values_of(text: &str, name: &str) -> Vec<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(&close) else { break };
        let value = after[..end].trim();
        if !value.is_empty() && !value.contains('<') && !found.iter().any(|known| known == value) {
            found.push(value.to_owned());
        }
        rest = &after[end..];
    }
    found
}

fn properties(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<properties>") {
        let block = &rest[start + "<properties>".len()..];
        let end = block.find("</properties>").unwrap_or(block.len());
        let mut inner = &block[..end];
        while let Some(open) = inner.find('<') {
            let tag = &inner[open + 1..];
            let name: String = tag.chars().take_while(|c| !c.is_whitespace() && !matches!(c, '>' | '/')).collect();
            if !name.is_empty() && !name.starts_with(['!', '?']) && !names.contains(&name) {
                names.push(name.clone());
            }
            let skip = format!("</{name}>");
            inner = match tag.find(&skip) {
                Some(close) => &tag[close + skip.len()..],
                None => &tag[name.len().min(tag.len())..],
            };
        }
        rest = &block[end..];
    }
    names
}

fn item(label: &str, kind: &'static str, insert: String, detail: &str) -> Item {
    Item { label: label.to_owned(), kind, insert, detail: detail.to_owned() }
}

fn sibling_value(text: &str, content: usize, name: &str) -> Option<String> {
    let rest = &text[content..];
    let end = rest.find("</plugin>").or_else(|| rest.find("</dependency>")).unwrap_or(rest.len());
    let scope = &rest[..end];
    let scope = scope.find("<dependencies>").map_or(scope, |at| &scope[..at]);
    values_of(scope, name).into_iter().next()
}

pub fn complete(text: &str, caret: usize) -> Option<Completions> {
    if caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let before = &text[..caret];
    if let Some(open) = before.rfind("${").filter(|open| !before[*open..].contains('}')) {
        let from = open + 2;
        let typed = &before[from..];
        if typed.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            let mut items: Vec<Item> =
                properties(text).iter().map(|name| item(name, "value", format!("{name}}}$0"), "property")).collect();
            items.extend(PROJECT_PROPERTIES.iter().map(|name| item(name, "value", format!("{name}}}$0"), "project")));
            return Some(Completions { from, items });
        }
    }
    let (stack, in_tag) = open_at(text, caret);
    if let Some(lt) = in_tag {
        let typed = &text[lt + 1..caret];
        if !typed.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return None;
        }
        let parent = stack.last().map_or("", |open| open.name.as_str());
        let children = if parent.is_empty() {
            &["project"][..]
        } else {
            CHILDREN.iter().find(|(name, _)| *name == parent).map(|(_, children)| *children)?
        };
        let items = children.iter().map(|child| item(child, "tag", format!("{child}>$0</{child}>"), parent)).collect();
        return Some(Completions { from: lt + 1, items });
    }
    let current = stack.last()?;
    let content = &text[current.content..caret];
    if content.contains('<') {
        return None;
    }
    let from = word_start(text, caret);
    let parent = stack.len().checked_sub(2).map(|index| stack[index].name.as_str()).unwrap_or_default();
    let values = |list: &[&str], detail: &str| -> Vec<Item> {
        list.iter().map(|value| item(value, "value", (*value).to_owned(), detail)).collect()
    };
    let items = match current.name.as_str() {
        "phase" => values(PHASES, "phase"),
        "goal" => {
            let plugin = stack.iter().rev().find(|open| open.name == "plugin")?;
            let artifact = sibling_value(text, plugin.content, "artifactId")?;
            let goals = GOALS.iter().find(|(id, _)| *id == artifact).map(|(_, goals)| *goals)?;
            values(goals, &artifact)
        }
        "groupId" => {
            let mut groups = values_of(text, "groupId");
            if parent == "plugin" && !groups.iter().any(|group| group == "org.apache.maven.plugins") {
                groups.push("org.apache.maven.plugins".to_owned());
            }
            groups.iter().map(|group| item(group, "value", group.clone(), "groupId")).collect()
        }
        "artifactId" => {
            let mut artifacts = values_of(text, "artifactId");
            if parent == "plugin" {
                for (id, _) in GOALS {
                    if !artifacts.iter().any(|known| known == id) {
                        artifacts.push((*id).to_owned());
                    }
                }
            }
            artifacts.iter().map(|artifact| item(artifact, "value", artifact.clone(), "artifactId")).collect()
        }
        "version" => values_of(text, "version")
            .iter()
            .map(|version| item(version, "value", version.clone(), "version"))
            .collect(),
        "module" => return None,
        name => {
            let list = VALUES.iter().find(|(element, _)| *element == name).map(|(_, list)| *list)?;
            values(list, name)
        }
    };
    Some(Completions { from, items })
}

#[cfg(test)]
mod tests {
    use super::*;

    const POM: &str = r#"<project>
  <properties><slf4j.version>2.0.13</slf4j.version><java.release>21</java.release></properties>
  <dependencies>
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-api</artifactId>
      <version>${slf4j.version}</version>
      <scope>te</scope>
    </dependency>
    <de
  </dependencies>
  <build><plugins><plugin>
    <artifactId>maven-surefire-plugin</artifactId>
    <executions><execution><phase>pa</phase><goals><goal></goal></goals></execution></executions>
  </plugin></plugins></build>
  <!-- <scope></scope> -->
</project>
"#;

    fn labels(text: &str, after: &str) -> (usize, Vec<String>) {
        let caret = text.find(after).unwrap() + after.len();
        let found = complete(text, caret).unwrap_or_else(|| panic!("nothing after {after:?}"));
        (found.from, found.items.into_iter().map(|item| item.label).collect())
    }

    #[test]
    fn values_follow_the_element() {
        let (from, scope) = labels(POM, "<scope>te");
        assert_eq!(from, POM.find("<scope>te").unwrap() + "<scope>".len(), "the typed word is replaced");
        assert!(scope.contains(&"test".to_owned()) && scope.contains(&"provided".to_owned()));
        assert!(labels(POM, "<phase>pa").1.contains(&"package".to_owned()));
        assert_eq!(labels(POM, "<goal>").1, ["test"], "the surefire plug-in's goals");
        assert!(labels(POM, "<groupId>org.slf4j").1.contains(&"org.slf4j".to_owned()));
    }

    #[test]
    fn tags_follow_the_parent() {
        let caret = POM.find("<de\n").unwrap() + 3;
        let found = complete(POM, caret).unwrap();
        assert_eq!(found.from, caret - 2);
        assert_eq!(found.items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["dependency"]);
        let text = "<project>\n  <dependencies><dependency><\n";
        let found = complete(text, text.len() - 1).unwrap();
        assert!(found.items.iter().any(|item| item.label == "scope" && item.insert == "scope>$0</scope>"));
        assert_eq!(complete("<", 1).unwrap().items[0].label, "project");
    }

    #[test]
    fn properties_after_a_dollar_brace() {
        let text = "<project><properties><a.b>1</a.b></properties><version>${a";
        let found = complete(text, text.len()).unwrap();
        assert_eq!(found.from, text.len() - 1);
        assert_eq!(found.items[0].label, "a.b");
        assert_eq!(found.items[0].insert, "a.b}$0");
        assert!(found.items.iter().any(|item| item.label == "project.version"));
    }

    #[test]
    fn nothing_in_comments_or_unknown_places() {
        let caret = POM.find("<!-- <scope>").unwrap() + "<!-- <scope>".len();
        assert_eq!(complete(POM, caret), None);
        assert_eq!(complete("<project><name>x", 16), None, "a name takes any text");
        assert_eq!(complete("abc", 9), None);
    }
}
