use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::edits::*;
use super::*;
use crate::build_tools::{BuildEnv, Load, LoadState};
use crate::gradle::analysis as gradle;
use crate::maven::analysis as maven;

const MAVEN: &str = include_str!("testdata/maven-tree-verbose.out");
const GRADLE: &str = include_str!("testdata/gradle-init-script.out");

fn app_nodes() -> Vec<DependencyNode> {
    maven::parse(MAVEN).into_iter().find(|(project, _)| project.artifact_id == "app").unwrap().1
}

fn find<'a>(nodes: &'a [DependencyNode], name: &str) -> Option<&'a DependencyNode> {
    nodes.iter().find_map(|node| if node.name() == name { Some(node) } else { find(&node.children, name) })
}

fn gradle_app() -> AnalyzedModule {
    gradle::parse(GRADLE).into_iter().find(|module| module.id == ":app").unwrap()
}

#[test]
fn a_verbose_maven_tree_has_a_module_per_project_with_what_lost_and_why() {
    let modules = maven::parse(MAVEN);
    let names: Vec<&str> = modules.iter().map(|(project, _)| project.artifact_id.as_str()).collect();
    assert_eq!(names, ["parent", "core", "app"]);
    let nodes = app_nodes();
    let core = &nodes[0];
    assert_eq!(core.name(), "demo:core");
    let guava = &core.children[0];
    assert_eq!(
        (guava.requested_version.as_str(), guava.selected_version.as_str(), guava.scope.as_str()),
        ("33.0.0-jre", "32.1.3-jre", "compile")
    );
    assert!(guava.conflict && guava.omitted);
    assert_eq!(guava.reason, Some(Reason::Conflict));
    assert_eq!(guava.note, "omitted for conflict with 32.1.3-jre");
}

#[test]
fn a_managed_version_is_marked_and_a_duplicate_is_omitted_without_a_conflict() {
    let nodes = app_nodes();
    let logback = nodes.iter().find(|node| node.artifact_id == "logback-classic").unwrap();
    let managed = &logback.children[1];
    assert_eq!((managed.requested_version.as_str(), managed.selected_version.as_str()), ("2.0.7", "2.0.13"));
    assert_eq!(managed.reason, Some(Reason::Managed));
    assert!(managed.conflict && !managed.omitted);
    let simple = nodes.iter().find(|node| node.artifact_id == "slf4j-simple").unwrap();
    let duplicate = &simple.children[0];
    assert_eq!(duplicate.reason, Some(Reason::Duplicate));
    assert!(duplicate.omitted && !duplicate.conflict, "{duplicate:?}");
}

#[test]
fn a_gradle_report_has_requested_and_selected_versions_per_configuration() {
    let module = gradle_app();
    assert_eq!(module.name, "app");
    let scopes = scopes(&module.nodes);
    assert_eq!(scopes, ["compileClasspath", "runtimeClasspath", "testCompileClasspath", "testRuntimeClasspath"]);
    let codec = module.nodes.iter().find(|node| node.artifact_id == "commons-codec").unwrap();
    assert_eq!((codec.requested_version.as_str(), codec.selected_version.as_str()), ("1.10", "1.11"));
    assert_eq!(codec.reason, Some(Reason::Conflict));
    assert!(codec.conflict && !codec.omitted, "the higher version stands at the place that asked for less");
    assert!(codec.note.contains("between versions"), "{}", codec.note);
    let logging = find(&module.nodes, "commons-logging:commons-logging").unwrap();
    assert_eq!(logging.selected_version, "1.3.0");
    assert_eq!(logging.reason, Some(Reason::Constraint));
    let lib = find(&module.nodes, "demo:lib").unwrap();
    assert_eq!(lib.project.as_deref(), Some(":lib"));
    assert!(!lib.conflict);
    let missing = find(&module.nodes, "does.not:exist").unwrap();
    assert_eq!(missing.reason, Some(Reason::Failed));
    assert!(missing.omitted && missing.selected_version.is_empty());
    assert!(missing.note.contains("Could not find"), "{}", missing.note);
}

fn component(id: &str, dependencies: Value) -> Value {
    let (group, rest) = id.split_once(':').unwrap();
    let (module, version) = rest.split_once(':').unwrap();
    json!({ "id": id, "group": group, "module": module, "version": version, "dependencies": dependencies })
}

fn edge(version: &str, selected: &str) -> Value {
    json!({ "requested": selected, "requestedVersion": version, "selected": selected })
}

#[test]
fn a_gradle_cycle_ends_marked_and_a_dynamic_version_is_no_conflict() {
    let report = json!({ "path": ":", "name": "x", "configurations": [{ "name": "compileClasspath", "root": "r:r:1", "components": [
        component("r:r:1", json!([edge("1.+", "a:a:1")])),
        component("a:a:1", json!([edge("1", "b:b:1")])),
        component("b:b:1", json!([edge("1", "a:a:1")])),
    ] }] });
    let modules = gradle::parse(&format!("noise\nzen-dependencies {report}\n"));
    let a = &modules[0].nodes[0];
    assert!(!a.conflict && a.reason.is_none(), "1.+ is not a lost version: {a:?}");
    let again = &a.children[0].children[0];
    assert_eq!(again.reason, Some(Reason::Cycle));
    assert!(again.omitted && again.children.is_empty());
}

#[test]
fn the_list_has_each_library_once_with_the_winner_and_every_path() {
    let list = listed(&app_nodes(), &Filter::default());
    let guava = list.iter().find(|entry| entry.artifact_id == "guava").unwrap();
    assert_eq!(guava.version, "32.1.3-jre");
    assert_eq!(guava.requested, ["33.0.0-jre", "32.1.3-jre"]);
    assert_eq!(guava.scopes, ["compile", "test"]);
    assert!(guava.conflict && guava.direct);
    assert_eq!(guava.reason, Some(Reason::Conflict));
    assert_eq!(guava.usages.len(), 2);
    assert_eq!(guava.usages[0].path, ["demo:core:1.0", "com.google.guava:guava:32.1.3-jre"]);
    assert_eq!(guava.usages[1].path.len(), 1, "the module's own");
    let names: Vec<String> = list.iter().map(|entry| format!("{}:{}", entry.group_id, entry.artifact_id)).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn filters_leave_conflicts_a_scope_or_a_text_in_the_list_and_what_leads_to_them_in_the_tree() {
    let nodes = app_nodes();
    let conflicts = Filter { conflicts_only: true, ..Filter::default() };
    let list = listed(&nodes, &conflicts);
    assert!(list.iter().all(|entry| entry.conflict) && list.iter().any(|entry| entry.artifact_id == "guava"));
    assert!(!list.iter().any(|entry| entry.artifact_id == "httpcore"));
    let tree = filtered_tree(&nodes, &conflicts);
    let core = tree.iter().find(|node| node.artifact_id == "core").unwrap();
    assert!(!core.conflict && core.children[0].conflict, "the path to a conflict stays");
    assert!(!tree.iter().any(|node| node.artifact_id == "commons-text"), "and nothing else");

    let test = Filter { scope: Some("test".to_owned()), ..Filter::default() };
    assert!(listed(&nodes, &test).iter().all(|entry| entry.scopes.contains(&"test".to_owned())));
    assert!(filtered_tree(&nodes, &test).iter().all(|node| node.scope == "test"));

    let text = Filter { search: "HTTPCORE".to_owned(), ..Filter::default() };
    assert_eq!(listed(&nodes, &text).len(), 1);
    assert_eq!(filtered_tree(&nodes, &text)[0].artifact_id, "httpclient");
}

const POM: &str = r#"<?xml version="1.0"?>
<!-- the project -->
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <artifactId>app</artifactId>
  <dependencyManagement>
    <dependencies>
      <dependency><groupId>c</groupId><artifactId>managed</artifactId></dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <!-- the client -->
    <dependency>
      <groupId>org.apache.httpcomponents</groupId>
      <artifactId>httpclient</artifactId>
      <version>${http.version}</version> <!-- pinned -->
    </dependency>
    <dependency><groupId>a</groupId><artifactId>one-line</artifactId><version>1</version></dependency>
    <dependency>
		<groupId>a</groupId>
		<artifactId>tabbed</artifactId>
		<exclusions>
			<exclusion>
				<groupId>x</groupId>
				<artifactId>y</artifactId>
			</exclusion>
		</exclusions>
    </dependency>
    <dependency>
      <groupId>a</groupId>
      <artifactId>empty</artifactId>
      <exclusions/>
    </dependency>
  </dependencies>
</project>
"#;

fn edited(result: Result<Exclusion, String>) -> String {
    match result.unwrap() {
        Exclusion::Edited(text) => text,
        other => panic!("not edited: {other:?}"),
    }
}

#[test]
fn a_pom_exclusion_goes_under_the_direct_dependency_and_keeps_comments_and_formatting() {
    let text = edited(pom_exclusion(POM, "org.apache.httpcomponents", "httpclient", "commons-codec", "commons-codec"));
    let added = "<version>${http.version}</version> <!-- pinned -->
      <exclusions>
        <exclusion>
          <groupId>commons-codec</groupId>
          <artifactId>commons-codec</artifactId>
        </exclusion>
      </exclusions>
    </dependency>";
    assert!(text.contains(added), "{text}");
    assert_eq!(
        text.replace(&added["<version>${http.version}</version> <!-- pinned -->\n".len()..], "    </dependency>"),
        POM
    );
}

#[test]
fn a_pom_exclusion_joins_the_exclusions_there_and_is_not_written_twice() {
    let text = edited(pom_exclusion(POM, "a", "tabbed", "p", "q"));
    assert!(
        text.contains(
            "</exclusion>\n\t\t\t<exclusion>\n\t\t\t\t<groupId>p</groupId>\n\t\t\t\t<artifactId>q</artifactId>\n\t\t\t</exclusion>\n\t\t</exclusions>"
        ),
        "{text}"
    );
    assert_eq!(pom_exclusion(POM, "a", "tabbed", "x", "y"), Ok(Exclusion::Present));
    assert_eq!(pom_exclusion(POM, "a", "absent", "x", "y"), Ok(Exclusion::NotDeclared));
    assert_eq!(pom_exclusion(POM, "c", "managed", "x", "y"), Ok(Exclusion::NotDeclared), "only a real dependency");
}

#[test]
fn a_pom_exclusion_stays_on_a_line_written_on_one_and_fills_an_empty_list() {
    let text = edited(pom_exclusion(POM, "a", "one-line", "p", "q"));
    assert!(
        text.contains("<version>1</version><exclusions><exclusion><groupId>p</groupId><artifactId>q</artifactId></exclusion></exclusions></dependency>"),
        "{text}"
    );
    let text = edited(pom_exclusion(POM, "a", "empty", "p", "q"));
    assert!(text.contains("      <exclusions>\n        <exclusion>\n          <groupId>p</groupId>"), "{text}");
    assert!(!text.contains("<exclusions/>"));
}

#[test]
fn a_pom_keeps_windows_line_endings_and_a_broken_one_is_refused() {
    let windows = POM.replace('\n', "\r\n");
    let text = edited(pom_exclusion(&windows, "a", "empty", "p", "q"));
    assert_eq!(text.matches('\n').count(), text.matches("\r\n").count());
    assert!(pom_exclusion("<project>", "a", "b", "p", "q").is_err());
}

#[test]
fn the_declaration_of_a_pom_is_the_dependency_not_its_management() {
    let at = pom_declaration(POM, "a", "tabbed").unwrap();
    assert!(POM[at..].starts_with("<dependency>\n\t\t<groupId>a"));
    assert!(pom_declaration(POM, "c", "managed").is_none());
    let parent =
        pom_parent(Path::new("/p/app/pom.xml"), "<project><parent><artifactId>x</artifactId></parent></project>");
    assert_eq!(parent, Some(PathBuf::from("/p/pom.xml")));
    let none = "<project><parent><relativePath/></parent></project>";
    assert_eq!(pom_parent(Path::new("/p/pom.xml"), none), None);
}

fn dependency(group: &str, artifact: &str) -> GradleDependency {
    GradleDependency { group: group.to_owned(), artifact: artifact.to_owned(), ..GradleDependency::default() }
}

const GROOVY: &str = r#"plugins { id 'java' }

// implementation 'a:commented:1'
dependencies {
    implementation 'a:plain:1.0' // kept
    implementation('a:called:1.0')
    api group: 'a', name: 'mapped', version: '1'
    implementation('a:closed:1') {
        transitive = false
    }
    implementation('a:inline:1') { transitive = false }
    implementation libs.foo.bar
    implementation project(':lib')
    testImplementation 'a:plain:1.0'
    constraints {
        implementation 'a:constrained:1'
    }
}
"#;

fn groovy(wanted: &GradleDependency) -> String {
    match gradle_exclusion(GROOVY, false, wanted, "x", "y") {
        Exclusion::Edited(text) => text,
        other => panic!("not edited: {other:?}"),
    }
}

#[test]
fn a_groovy_string_notation_gets_parentheses_and_a_closure_on_every_line_that_declares_it() {
    let text = groovy(&dependency("a", "plain"));
    assert!(
        text.contains("    implementation('a:plain:1.0') {\n        exclude group: 'x', module: 'y'\n    } // kept\n"),
        "{text}"
    );
    assert!(text.contains("    testImplementation('a:plain:1.0') {\n        exclude group: 'x', module: 'y'\n    }\n"));
    assert!(text.contains("// implementation 'a:commented:1'"));
}

#[test]
fn a_groovy_call_map_closure_alias_and_project_each_take_the_exclusion() {
    let text = groovy(&dependency("a", "called"));
    assert!(
        text.contains("implementation('a:called:1.0') {\n        exclude group: 'x', module: 'y'\n    }\n"),
        "{text}"
    );
    let text = groovy(&dependency("a", "mapped"));
    assert!(text.contains("api(group: 'a', name: 'mapped', version: '1') {\n        exclude"), "{text}");
    let text = groovy(&dependency("a", "closed"));
    assert!(text.contains("transitive = false\n        exclude group: 'x', module: 'y'\n    }\n"), "{text}");
    let text = groovy(&dependency("a", "inline"));
    assert!(text.contains("{ transitive = false; exclude group: 'x', module: 'y' }"), "{text}");
    let alias = GradleDependency { accessors: vec!["libs.foo.bar".to_owned()], ..dependency("g", "bar") };
    assert!(groovy(&alias).contains("implementation(libs.foo.bar) {\n        exclude"));
    let project = GradleDependency { project: Some(":lib".to_owned()), ..dependency("demo", "lib") };
    assert!(groovy(&project).contains("implementation(project(':lib')) {\n        exclude"));
}

#[test]
fn gradle_comments_constraints_and_absent_libraries_are_not_declarations() {
    assert_eq!(gradle_exclusion(GROOVY, false, &dependency("a", "commented"), "x", "y"), Exclusion::NotDeclared);
    assert_eq!(gradle_exclusion(GROOVY, false, &dependency("a", "constrained"), "x", "y"), Exclusion::NotDeclared);
    assert_eq!(gradle_declaration(GROOVY, &dependency("a", "plain")), GROOVY.find("implementation 'a:plain"));
}

const KOTLIN: &str = r#"plugins { java }

dependencies {
    implementation("a:plain:1.0")
    implementation("a:versioned:$version")
    implementation(libs.foo.bar)
    implementation(platform("a:bom:1"))
    "api"("a:quoted:1")
	testImplementation("a:tabbed:1") {
		exclude(group = "x", module = "y")
	}
}
"#;

fn kotlin(wanted: &GradleDependency) -> String {
    match gradle_exclusion(KOTLIN, true, wanted, "p", "q") {
        Exclusion::Edited(text) => text,
        other => panic!("not edited: {other:?}"),
    }
}

#[test]
fn the_kotlin_dsl_gets_exclude_in_its_own_syntax() {
    let text = kotlin(&dependency("a", "plain"));
    assert!(
        text.contains("implementation(\"a:plain:1.0\") {\n        exclude(group = \"p\", module = \"q\")\n    }\n"),
        "{text}"
    );
    assert_eq!(text.replace(" {\n        exclude(group = \"p\", module = \"q\")\n    }", ""), KOTLIN);
    assert!(kotlin(&dependency("a", "versioned")).contains("\"a:versioned:$version\") {\n"));
    let alias = GradleDependency { accessors: vec!["libs.foo.bar".to_owned()], ..dependency("g", "bar") };
    assert!(kotlin(&alias).contains("implementation(libs.foo.bar) {\n"));
    assert!(kotlin(&dependency("a", "bom")).contains("implementation(platform(\"a:bom:1\")) {\n"));
    assert!(kotlin(&dependency("a", "quoted")).contains("\"api\"(\"a:quoted:1\") {\n"));
}

#[test]
fn a_kotlin_closure_keeps_its_tabs_and_an_exclusion_is_not_written_twice() {
    let text = kotlin(&dependency("a", "tabbed"));
    assert!(text.contains("module = \"y\")\n\t\texclude(group = \"p\", module = \"q\")\n\t}\n"), "{text}");
    assert_eq!(gradle_exclusion(KOTLIN, true, &dependency("a", "tabbed"), "x", "y"), Exclusion::Present);
}

#[test]
fn a_script_with_text_beyond_ascii_is_edited_at_the_right_place() {
    let script = "// зависимости\ndependencies {\n    // клиент\n    implementation(\"a:b:1\") // é\n}\n";
    let wanted = dependency("a", "b");
    let Exclusion::Edited(text) = gradle_exclusion(script, true, &wanted, "x", "y") else { panic!("not edited") };
    assert!(
        text.contains("implementation(\"a:b:1\") {\n        exclude(group = \"x\", module = \"y\")\n    } // é\n"),
        "{text}"
    );
}

#[test]
fn a_version_catalog_gives_the_accessors_of_a_library() {
    let catalog = r#"[versions]
foo = "1"

[libraries]
foo-bar = { module = "g:bar", version.ref = "foo" }
baz_qux = "g:qux:1"
apart = { group = "g", name = "apart", version = "1" }
"#;
    assert_eq!(catalog_accessors(catalog, "g", "bar"), ["libs.foo.bar"]);
    assert_eq!(catalog_accessors(catalog, "g", "qux"), ["libs.baz.qux"]);
    assert_eq!(catalog_accessors(catalog, "g", "apart"), ["libs.apart"]);
    assert!(catalog_accessors(catalog, "g", "foo").is_empty());
    assert!(catalog[catalog_declaration(catalog, "g", "bar").unwrap()..].starts_with("g:bar"));
    assert!(catalog[catalog_declaration(catalog, "g", "apart").unwrap()..].starts_with("apart ="));
}

fn session(root: &Path) -> (Registry, Session, BuildEnv) {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    let env = BuildEnv::default();
    env.set_folders(Some(Vec::new()));
    session.set_state(env.clone());
    crate::trust::trust_in_tests(&env, root);
    (registry, session, env)
}

fn loaded_output(env: &BuildEnv, key: String, output: &str) {
    env.set_load(
        &key,
        Load {
            state: LoadState::Loaded,
            output: output.to_owned(),
            error: None,
            task: None,
            started: Instant::now(),
            took: None,
        },
    );
}

fn write_maven_project(root: &Path) {
    std::fs::write(
        root.join("pom.xml"),
        "<project><groupId>demo</groupId><artifactId>parent</artifactId><version>1.0</version><packaging>pom</packaging>
  <modules><module>app</module></modules>
  <dependencies>
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-simple</artifactId>
    </dependency>
  </dependencies>
</project>
",
    )
    .unwrap();
    std::fs::create_dir(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/pom.xml"),
        "<project>
  <parent><groupId>demo</groupId><artifactId>parent</artifactId><version>1.0</version></parent>
  <artifactId>app</artifactId>
  <dependencies>
    <!-- the client -->
    <dependency>
      <groupId>org.apache.httpcomponents</groupId>
      <artifactId>httpclient</artifactId>
      <version>4.5.14</version>
    </dependency>
  </dependencies>
</project>
",
    )
    .unwrap();
}

#[test]
fn the_commands_analyze_a_maven_module_and_exclude_from_the_direct_dependency() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().canonicalize().unwrap();
    write_maven_project(&root);
    let (registry, mut session, env) = session(&root);
    let nothing = registry.dispatch(&mut session, Analyze::ID, json!({})).unwrap();
    assert_eq!(nothing["system"], "maven");
    assert!(nothing.get("state").is_none());
    assert!(registry.dispatch(&mut session, Exclude::ID, json!({ "groupId": "a", "artifactId": "b" })).is_err());

    loaded_output(&env, MavenAnalyzer.key(&root), MAVEN);
    let analysis =
        registry.dispatch(&mut session, Analyze::ID, json!({ "module": "demo:app", "conflictsOnly": true })).unwrap();
    assert_eq!(analysis["state"], "loaded");
    assert_eq!(analysis["module"], "demo:app");
    assert_eq!(analysis["modules"].as_array().unwrap().len(), 3);
    assert!(analysis["modules"][2]["buildFile"].as_str().unwrap().ends_with("app/pom.xml"));
    assert_eq!(analysis["scopes"], json!(["compile", "test", "runtime"]));
    assert!(analysis["dependencies"].as_array().unwrap().iter().all(|entry| entry["conflict"] == true));
    let brief = registry.dispatch(&mut session, Analyze::ID, json!({ "brief": true })).unwrap();
    assert_eq!(brief["modules"], json!([]));
    assert!(registry.dispatch(&mut session, Analyze::ID, json!({ "module": "no:such" })).is_err());

    let place = json!({ "module": "demo:app", "groupId": "commons-codec", "artifactId": "commons-codec", "via": "org.apache.httpcomponents:httpclient", "noOpen": true });
    let declared = registry.dispatch(&mut session, GoToDeclaration::ID, place).unwrap();
    assert_eq!(declared["line"], 6);
    assert_eq!(declared["direct"], "org.apache.httpcomponents:httpclient");
    let inherited =
        json!({ "module": "demo:app", "groupId": "org.slf4j", "artifactId": "slf4j-simple", "noOpen": true });
    let declared = registry.dispatch(&mut session, GoToDeclaration::ID, inherited).unwrap();
    assert_eq!(Path::new(declared["path"].as_str().unwrap()).file_name().unwrap(), "pom.xml");
    assert!(!declared["path"].as_str().unwrap().contains("/app/"), "found in the parent POM: {declared}");

    let direct = json!({ "module": "demo:app", "groupId": "commons-text", "artifactId": "nothing" });
    assert!(registry.dispatch(&mut session, Exclude::ID, direct).is_err());
    let own = json!({ "module": "demo:app", "groupId": "org.mockito", "artifactId": "mockito-core" });
    let refused = registry.dispatch(&mut session, Exclude::ID, own).unwrap_err().to_string();
    assert!(refused.contains("does not come with another"), "{refused}");
    let bad = json!({ "module": "demo:app", "groupId": "a</groupId>", "artifactId": "b" });
    assert!(registry.dispatch(&mut session, Exclude::ID, bad).is_err());

    let exclude = json!({ "module": "demo:app", "groupId": "commons-codec", "artifactId": "commons-codec", "via": "org.apache.httpcomponents:httpclient" });
    let done = registry.dispatch(&mut session, Exclude::ID, exclude.clone()).unwrap();
    assert_eq!(done["from"][0]["added"], true);
    assert_eq!(done["reloaded"], false, "no Maven to run in this test: {done}");
    assert!(done["reloadError"].is_string());
    let pom = std::fs::read_to_string(root.join("app/pom.xml")).unwrap();
    assert!(pom.contains("<!-- the client -->"));
    assert!(pom.contains("<version>4.5.14</version>\n      <exclusions>\n        <exclusion>\n          <groupId>commons-codec</groupId>"), "{pom}");
    let again = registry.dispatch(&mut session, Exclude::ID, exclude).unwrap();
    assert_eq!(again["from"][0]["added"], false, "already there");
}

#[test]
fn the_commands_exclude_in_a_gradle_script_and_the_catalog_declares_an_alias() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().canonicalize().unwrap();
    std::fs::write(root.join("settings.gradle"), "include 'app'\n").unwrap();
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::create_dir_all(root.join("gradle")).unwrap();
    std::fs::write(
        root.join("gradle/libs.versions.toml"),
        "[libraries]\nclient = \"org.apache.httpcomponents:httpclient:4.5.14\"\n",
    )
    .unwrap();
    let script = root.join("app/build.gradle.kts");
    std::fs::write(&script, "dependencies {\n    implementation(libs.client)\n}\n").unwrap();
    let (registry, mut session, env) = session(&root);
    let output = GRADLE.replace("/project/app/build.gradle.kts", &script.display().to_string());
    loaded_output(&env, GradleAnalyzer.key(&root), &output);
    let analysis = registry
        .dispatch(
            &mut session,
            Analyze::ID,
            json!({ "system": "gradle", "module": ":app", "scope": "runtimeClasspath" }),
        )
        .unwrap();
    assert_eq!(analysis["byConfiguration"], true);
    assert!(analysis["tree"].as_array().unwrap().iter().all(|node| node["scope"] == "runtimeClasspath"));
    let exclude =
        json!({ "module": ":app", "groupId": "commons-codec", "artifactId": "commons-codec", "noReload": true });
    let done = registry.dispatch(&mut session, Exclude::ID, exclude).unwrap();
    let direct: Vec<&str> =
        done["from"].as_array().unwrap().iter().map(|edit| edit["direct"].as_str().unwrap()).collect();
    assert_eq!(direct, ["org.apache.httpcomponents:httpclient"], "{done}");
    assert_eq!(
        std::fs::read_to_string(&script).unwrap(),
        "dependencies {\n    implementation(libs.client) {\n        exclude(group = \"commons-codec\", module = \"commons-codec\")\n    }\n}\n"
    );
    let place =
        json!({ "module": ":app", "groupId": "org.apache.httpcomponents", "artifactId": "httpclient", "noOpen": true });
    assert_eq!(registry.dispatch(&mut session, GoToDeclaration::ID, place).unwrap()["line"], 2);
}

fn program(name: &str) -> Option<PathBuf> {
    let folder = std::env::var_os("ZEN_TEST_BUILD_TOOLS")?;
    Some(PathBuf::from(folder)).map(|folder| folder.join(name)).filter(|path| path.is_file())
}

fn wait_real(registry: &Registry, session: &mut Session, params: Value) -> Value {
    let analysis = registry.dispatch(session, Analyze::ID, params).unwrap();
    assert_eq!(analysis["state"], "loaded", "{analysis}");
    analysis
}

#[test]
#[ignore = "needs ZEN_TEST_BUILD_TOOLS, a folder with mvn and gradle, and JAVA_HOME; may download"]
fn real_maven_marks_a_conflict_and_real_gradle_after_an_exclusion_drops_it() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().canonicalize().unwrap();
    let tools = PathBuf::from(std::env::var_os("ZEN_TEST_BUILD_TOOLS").unwrap());
    assert!(program("mvn").is_some() && program("gradle").is_some());
    std::fs::write(
        root.join("pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion><groupId>demo</groupId><artifactId>app</artifactId><version>1</version>
  <dependencies>
    <dependency><groupId>org.apache.httpcomponents</groupId><artifactId>httpclient</artifactId><version>4.5.14</version></dependency>
    <dependency><groupId>commons-codec</groupId><artifactId>commons-codec</artifactId><version>1.10</version></dependency>
  </dependencies>
</project>
",
    )
    .unwrap();
    std::fs::write(root.join("settings.gradle"), "rootProject.name = 'app'\n").unwrap();
    let script = root.join("build.gradle");
    std::fs::write(
        &script,
        "plugins { id 'java' }\nrepositories { mavenCentral() }\ndependencies {\n    implementation 'org.apache.httpcomponents:httpclient:4.5.14'\n    implementation 'commons-codec:commons-codec:1.10'\n}\n",
    )
    .unwrap();
    let (registry, mut session, env) = session(&root);
    env.set_folders(Some(vec![tools]));
    let maven = json!({ "system": "maven", "load": true, "waitMs": 300_000, "conflictsOnly": true });
    let analysis = wait_real(&registry, &mut session, maven);
    assert_eq!(analysis["dependencies"][0]["artifactId"], "commons-codec", "{analysis}");
    assert_eq!(analysis["dependencies"][0]["version"], "1.10");
    let gradle = json!({ "system": "gradle", "load": true, "waitMs": 300_000, "conflictsOnly": true });
    let analysis = wait_real(&registry, &mut session, gradle);
    assert_eq!(analysis["dependencies"][0]["version"], "1.11", "{analysis}");
    let exclude = json!({ "system": "gradle", "groupId": "commons-codec", "artifactId": "commons-codec", "via": "org.apache.httpcomponents:httpclient", "noReload": true });
    registry.dispatch(&mut session, Exclude::ID, exclude).unwrap();
    let again = json!({ "system": "gradle", "load": true, "waitMs": 300_000, "conflictsOnly": true });
    let analysis = wait_real(&registry, &mut session, again);
    assert_eq!(analysis["dependencies"], json!([]), "{analysis}");
}

#[test]
fn an_agent_gets_neither_the_place_nor_an_exclusion_in_a_pom_hidden_from_it() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().canonicalize().unwrap();
    write_maven_project(&root);
    let (registry, mut session, env) = session(&root);
    loaded_output(&env, MavenAnalyzer.key(&root), MAVEN);
    let before = std::fs::read_to_string(root.join("app/pom.xml")).unwrap();
    session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["app/pom.xml".to_owned()]);
    let via = "org.apache.httpcomponents:httpclient";
    let place = json!({ "module": "demo:app", "groupId": "commons-codec", "artifactId": "commons-codec", "via": via, "noOpen": true });
    let declared = zen_core::macros::as_agent(&mut session, |session| {
        registry.dispatch(session, GoToDeclaration::ID, place.clone())
    });
    assert!(declared.is_err(), "{declared:?}");
    let exclude = json!({ "module": "demo:app", "groupId": "commons-codec", "artifactId": "commons-codec", "via": via, "noReload": true });
    let excluded =
        zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, Exclude::ID, exclude.clone()));
    assert!(excluded.is_err(), "{excluded:?}");
    assert_eq!(std::fs::read_to_string(root.join("app/pom.xml")).unwrap(), before);
    let person = registry.dispatch(&mut session, GoToDeclaration::ID, place).unwrap();
    assert!(person["path"].as_str().unwrap().ends_with("app/pom.xml"), "{person}");
}
