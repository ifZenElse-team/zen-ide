use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::sources::{self, Answer, Endpoints, IndexQuery, Repository, Transport, Validators};
use super::*;
use crate::gradle::dependencies::GradleDependencies;
use crate::gradle::{catalog, script};
use crate::maven::dependencies::MavenDependencies;
use crate::maven::pom_edit::{self, NewBom, NewDependency, NewRepository, VersionEdit};

const CENTRAL: &str = "https://central.test/maven2";
const SEARCH: &str = "https://search.test/select";
const OSV: &str = "https://osv.test/query";
const ALL: Allowed = Allowed { search: true, updates: true, advisories: true };

fn metadata(versions: &[&str]) -> Answer {
    let list: String = versions.iter().map(|version| format!("<version>{version}</version>")).collect();
    body(&format!("<metadata><versioning><versions>{list}</versions></versioning></metadata>"), Some("\"tag\""))
}

fn body(text: &str, etag: Option<&str>) -> Answer {
    Answer::Changed {
        body: text.as_bytes().to_vec(),
        validators: Validators { etag: etag.map(str::to_owned), last_modified: None },
    }
}

#[derive(Default)]
struct Recorded {
    gets: Mutex<HashMap<String, Answer>>,
    posts: Mutex<HashMap<String, String>>,
    asked: Mutex<Vec<(String, Validators, Option<String>)>>,
    posted: Mutex<Vec<String>>,
}

impl Recorded {
    fn get(self: &Arc<Self>, url: &str, answer: Answer) -> Arc<Self> {
        self.gets.lock().unwrap().insert(url.to_owned(), answer);
        Arc::clone(self)
    }

    fn post(self: &Arc<Self>, package: &str, answer: Value) -> Arc<Self> {
        self.posts.lock().unwrap().insert(package.to_owned(), answer.to_string());
        Arc::clone(self)
    }

    fn urls(&self) -> Vec<String> {
        self.asked.lock().unwrap().iter().map(|(url, ..)| url.clone()).collect()
    }
}

impl Transport for Recorded {
    fn get(&self, url: &str, validators: &Validators, authorization: Option<&str>) -> Result<Answer, String> {
        self.asked.lock().unwrap().push((url.to_owned(), validators.clone(), authorization.map(str::to_owned)));
        self.gets.lock().unwrap().get(url).cloned().ok_or_else(|| format!("no network for {url}"))
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        assert_eq!(url, OSV);
        let query: Value = serde_json::from_slice(body).unwrap();
        let package = query["package"]["name"].as_str().unwrap().to_owned();
        assert_eq!(query["package"]["ecosystem"], "Maven");
        self.posted.lock().unwrap().push(package.clone());
        self.posts
            .lock()
            .unwrap()
            .get(&package)
            .map(|answer| answer.clone().into_bytes())
            .ok_or_else(|| "down".to_owned())
    }
}

fn service(transport: &Arc<Recorded>) -> Dependencies {
    let service = Dependencies::default();
    service.set_transport(transport.clone());
    service.set_endpoints(Endpoints { search: SEARCH.to_owned(), central: CENTRAL.to_owned(), osv: OSV.to_owned() });
    service
}

fn central() -> Vec<Repository> {
    vec![Repository::new("central", CENTRAL)]
}

fn settled_status(service: &Dependencies, group: &str, artifact: &str, version: &str) -> DependencyStatus {
    for _ in 0..10 {
        let (status, _) = service.status(group, artifact, version, &central(), ALL);
        if !status.pending {
            return status;
        }
        service.wait_idle(Duration::from_secs(10));
    }
    panic!("the answers never came");
}

fn search_url(query: IndexQuery) -> String {
    query.url(SEARCH).unwrap()
}

fn vulnerability(id: &str, package: &str, events: Value) -> Value {
    json!({
        "id": id, "aliases": [format!("CVE-{id}")], "summary": "Bad thing\nmore",
        "severity": [{ "type": "CVSS_V3", "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H" }],
        "database_specific": { "severity": "CRITICAL" },
        "affected": [{ "package": { "name": package, "ecosystem": "Maven" },
                       "ranges": [{ "type": "ECOSYSTEM", "events": events }] }],
    })
}

#[test]
fn search_finds_artifacts_in_the_index_and_versions_in_the_metadata() {
    let index = json!({ "response": { "docs": [
        { "g": "org.springframework.boot", "a": "spring-boot-starter-web", "latestVersion": "3.5.3" },
        { "g": "gone.example", "a": "gone", "latestVersion": "1.0" },
    ] } });
    let transport = Arc::new(Recorded::default());
    transport
        .get(&search_url(IndexQuery::Text("starter-web".to_owned())), body(&index.to_string(), None))
        .get(
            &format!("{CENTRAL}/org/springframework/boot/spring-boot-starter-web/maven-metadata.xml"),
            metadata(&["3.5.3", "4.1.2", "4.2.0-RC1"]),
        )
        .get(&format!("{CENTRAL}/gone/example/gone/maven-metadata.xml"), Answer::Missing);
    let service = service(&transport);
    let mut answer = service.search("starter-web", &central(), ALL);
    assert!(answer.pending);
    while answer.pending {
        service.wait_idle(Duration::from_secs(10));
        answer = service.search("starter-web", &central(), ALL);
    }
    assert_eq!(answer.results.len(), 1, "what no repository lists is not offered");
    assert_eq!(answer.results[0].latest.as_deref(), Some("4.1.2"), "never the index's 3.5.3");
    assert_eq!(answer.results[0].versions, ["4.2.0-RC1", "4.1.2", "3.5.3"]);
}

#[test]
fn coordinates_are_looked_up_in_every_repository_even_when_the_index_does_not_know_them() {
    let own = Repository::new("own", "https://own.test/repo");
    let transport = Arc::new(Recorded::default());
    let index = json!({ "response": { "docs": [] } });
    transport
        .get(
            &search_url(IndexQuery::Coordinates { group: "acme".to_owned(), artifact: "core".to_owned() }),
            body(&index.to_string(), None),
        )
        .get(&format!("{CENTRAL}/acme/core/maven-metadata.xml"), Answer::Missing)
        .get("https://own.test/repo/acme/core/maven-metadata.xml", metadata(&["1.2"]));
    let service = service(&transport);
    let repositories = vec![Repository::new("central", CENTRAL), own];
    let _ = service.search("acme:core", &repositories, ALL);
    service.wait_idle(Duration::from_secs(10));
    let _ = service.search("acme:core", &repositories, ALL);
    service.wait_idle(Duration::from_secs(10));
    let answer = service.search("acme:core", &repositories, ALL);
    assert_eq!(answer.results[0].latest.as_deref(), Some("1.2"));
}

#[test]
fn a_vulnerable_version_gets_its_advisories_and_the_nearest_safe_version() {
    let events = json!([{ "introduced": "0" }, { "fixed": "2.15.0" }]);
    let later = json!([{ "introduced": "2.15.0" }, { "fixed": "2.17.1" }]);
    let transport = Arc::new(Recorded::default());
    transport
        .get(
            &format!("{CENTRAL}/org/log/core/maven-metadata.xml"),
            metadata(&["2.14.0", "2.15.0", "2.17.1", "2.18.0", "3.0.0-rc1"]),
        )
        .post(
            "org.log:core",
            json!({ "vulns": [vulnerability("1", "org.log:core", events), vulnerability("2", "org.log:core", later)] }),
        );
    let service = service(&transport);
    let status = settled_status(&service, "org.log", "core", "2.14.0");
    assert_eq!(status.advisories.iter().map(|advisory| advisory.id.as_str()).collect::<Vec<_>>(), ["CVE-1"]);
    assert_eq!(status.advisories[0].summary, "Bad thing");
    assert_eq!(status.advisories[0].score, Some(9.8));
    assert_eq!(status.advisories[0].rating.as_deref(), Some("CRITICAL"));
    assert_eq!(status.safe.as_deref(), Some("2.15.0"), "the nearest one without the advisory");
    assert_eq!(status.latest.as_deref(), Some("2.18.0"));
    assert_eq!(status.newer, ["2.18.0", "2.17.1", "2.15.0"]);
    let safe = settled_status(&service, "org.log", "core", "2.18.0");
    assert!(safe.advisories.is_empty() && safe.safe.is_none());
}

#[test]
fn osv_pages_are_followed() {
    let transport = Arc::new(Recorded::default());
    let first = json!({ "vulns": [vulnerability("1", "a:b", json!([{ "introduced": "0" }]))], "next_page_token": "" });
    transport.post("a:b", first);
    let found = sources::fetch_advisories(transport.as_ref(), OSV, "a", "b").unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].affects("9.9"));
}

#[test]
fn what_only_a_private_repository_lists_is_never_sent_to_osv() {
    let transport = Arc::new(Recorded::default());
    transport.get("https://own.test/repo/acme/core/maven-metadata.xml", metadata(&["1.0"]));
    let service = service(&transport);
    let private = vec![Repository::new("own", "https://own.test/repo").basic("me", "secret")];
    let _ = service.status("acme", "core", "1.0", &private, ALL);
    service.wait_idle(Duration::from_secs(10));
    let (status, _) = service.status("acme", "core", "1.0", &private, ALL);
    service.wait_idle(Duration::from_secs(10));
    assert!(!status.pending);
    assert!(transport.posted.lock().unwrap().is_empty());
    let asked = transport.asked.lock().unwrap();
    assert_eq!(asked[0].2.as_deref(), Some("Basic bWU6c2VjcmV0"), "the credentials go to their own repository");
}

#[test]
fn checks_that_are_turned_off_send_nothing() {
    let transport = Arc::new(Recorded::default());
    let service = service(&transport);
    let off = Allowed { search: false, updates: false, advisories: false };
    let (status, failed) = service.status("a", "b", "1", &central(), off);
    assert_eq!((status, failed), (DependencyStatus::default(), false));
    assert!(service.search("guice", &central(), off).disabled);
    service.wait_idle(Duration::from_secs(10));
    assert!(transport.urls().is_empty() && transport.posted.lock().unwrap().is_empty());
    let settings = |key: &str| (key != CHECK_UPDATES).then_some(json!(true));
    assert_eq!(Allowed::of(&settings, false), Allowed { search: true, updates: false, advisories: true });
    assert_eq!(Allowed::of(&|_| None, false), off, "nothing goes to the network until the person turns it on");
    assert_eq!(Allowed::of(&|_| Some(json!(true)), true), off, "work offline wins");
}

#[test]
fn answers_are_kept_with_their_validators_and_stand_in_when_offline() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("{CENTRAL}/a/b/maven-metadata.xml");
    let transport = Arc::new(Recorded::default());
    transport.get(&url, metadata(&["1.0", "1.1"]));
    let service = service(&transport);
    service.set_cache_dir(dir.path().to_owned());
    assert_eq!(settled_status(&service, "a", "b", "1.0").latest.as_deref(), Some("1.1"));
    assert_eq!(transport.urls().len(), 1);

    service.refresh();
    transport.get(&url, Answer::Unchanged);
    std::thread::yield_now();
    let forgotten = service.shared.forgotten.load(Ordering::SeqCst);
    service.shared.forgotten.store(forgotten + 5, Ordering::SeqCst);
    assert_eq!(settled_status(&service, "a", "b", "1.0").latest.as_deref(), Some("1.1"));
    let asked = transport.asked.lock().unwrap().clone();
    assert_eq!(asked.last().unwrap().1.etag.as_deref(), Some("\"tag\""), "asked with the validator");

    let offline = Arc::new(Recorded::default());
    service.set_transport(offline.clone());
    service.refresh();
    let forgotten = service.shared.forgotten.load(Ordering::SeqCst);
    service.shared.forgotten.store(forgotten + 5, Ordering::SeqCst);
    let status = settled_status(&service, "a", "b", "1.0");
    assert_eq!(status.latest.as_deref(), Some("1.1"), "what an earlier run kept");
    assert_eq!(offline.urls().len(), 1);
}

#[test]
fn a_failed_search_says_so_and_does_not_keep_asking() {
    let transport = Arc::new(Recorded::default());
    let service = service(&transport);
    let _ = service.search("guice", &central(), ALL);
    service.wait_idle(Duration::from_secs(10));
    let answer = service.search("guice", &central(), ALL);
    assert!(answer.failed && !answer.pending && answer.results.is_empty());
    service.wait_idle(Duration::from_secs(10));
    assert_eq!(transport.urls().len(), 1);
}

#[test]
fn urls_are_built_as_the_servers_document_them() {
    let repository = Repository::new("r", "https://r.test/m2/");
    assert_eq!(repository.metadata_url("org.a.b", "c"), "https://r.test/m2/org/a/b/c/maven-metadata.xml");
    assert_eq!(repository.pom_url("org.a", "c", "1.0"), "https://r.test/m2/org/a/c/1.0/c-1.0.pom");
    assert_eq!(
        IndexQuery::parse("org.a:b").unwrap().url("https://s"),
        Some("https://s?q=g%3A%22org.a%22%20AND%20a%3A%22b%22&rows=50&wt=json".to_owned())
    );
    assert_eq!(
        IndexQuery::parse("spring boot").unwrap().url("https://s"),
        Some("https://s?q=spring%20boot&rows=50&wt=json".to_owned())
    );
    assert_eq!(IndexQuery::parse("  "), None);
    assert_eq!(
        IndexQuery::Group("org.spr".to_owned()).url("https://s").unwrap(),
        "https://s?q=g%3Aorg.spr%2A&rows=50&wt=json"
    );
    assert_eq!(
        IndexQuery::parse("guava").unwrap().url("https://s").unwrap(),
        "https://s?q=a%3A%22guava%22%20OR%20guava&rows=50&wt=json"
    );
}

#[test]
fn the_artifact_with_the_most_versions_comes_first() {
    let index = json!({ "response": { "docs": [
        { "g": "shaded", "a": "guava", "versionCount": 3 },
        { "g": "com.google.guava", "a": "guava", "versionCount": 150 },
    ] } });
    let found = sources::parse_search(index.to_string().as_bytes()).unwrap();
    assert_eq!(found[0].0, "com.google.guava");
}

const POM: &str = r#"<?xml version="1.0"?>
<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>demo</groupId>
  <artifactId>shop</artifactId>
  <version>1.0</version>

  <properties>
    <!-- the JSON library -->
    <jackson.version>2.17.0</jackson.version>
  </properties>

  <dependencies>
    <!-- logging -->
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-api</artifactId>
      <version>2.0.13</version> <!-- pinned -->
    </dependency>

    <dependency>
      <groupId>com.fasterxml.jackson.core</groupId>
      <artifactId>jackson-core</artifactId>
      <version>${jackson.version}</version>
      <scope>runtime</scope>
    </dependency>
  </dependencies>
  <build><finalName>shop</finalName></build>
</project>
"#;

#[test]
fn a_dependency_is_added_after_the_last_one_in_the_files_own_indentation() {
    let new = NewDependency { group: "a", artifact: "b", version: Some("1"), scope: Some("test"), optional: false };
    let added = pom_edit::add(POM, &new).unwrap();
    let expected = POM.replace(
        "      <scope>runtime</scope>\n    </dependency>\n",
        "      <scope>runtime</scope>\n    </dependency>\n    <dependency>\n      <groupId>a</groupId>\n      <artifactId>b</artifactId>\n      <version>1</version>\n      <scope>test</scope>\n    </dependency>\n",
    );
    assert_eq!(added, expected);
}

#[test]
fn line_endings_and_tabs_of_the_pom_are_kept() {
    let pom = "<project>\r\n\t<artifactId>x</artifactId>\r\n\t<dependencies>\r\n\t\t<dependency>\r\n\t\t\t<groupId>g</groupId>\r\n\t\t\t<artifactId>a</artifactId>\r\n\t\t</dependency>\r\n\t</dependencies>\r\n</project>\r\n";
    let new = NewDependency { group: "h", artifact: "b", version: None, scope: None, optional: false };
    let added = pom_edit::add(pom, &new).unwrap();
    assert!(added.contains("\t\t</dependency>\r\n\t\t<dependency>\r\n\t\t\t<groupId>h</groupId>\r\n\t\t\t<artifactId>b</artifactId>\r\n\t\t</dependency>\r\n\t</dependencies>"), "{added}");
    assert!(!added.replace("\r\n", "").contains('\n'));
}

#[test]
fn a_pom_without_a_list_gets_one_before_the_build() {
    let pom = "<project>\n  <artifactId>x</artifactId>\n\n  <build>\n  </build>\n</project>\n";
    let new = NewDependency { group: "g", artifact: "a", version: Some("1"), scope: None, optional: false };
    let added = pom_edit::add(pom, &new).unwrap();
    assert_eq!(
        added,
        "<project>\n  <artifactId>x</artifactId>\n\n  <dependencies>\n    <dependency>\n      <groupId>g</groupId>\n      <artifactId>a</artifactId>\n      <version>1</version>\n    </dependency>\n  </dependencies>\n\n  <build>\n  </build>\n</project>\n"
    );
    let bare = "<project>\n  <artifactId>x</artifactId>\n</project>\n";
    let added = pom_edit::add(bare, &new).unwrap();
    assert!(added.starts_with("<project>\n  <artifactId>x</artifactId>\n\n  <dependencies>\n"), "{added}");
    assert!(added.ends_with("  </dependencies>\n</project>\n"), "{added}");
    assert!(roxmltree::Document::parse(&added).is_ok());
}

#[test]
fn an_empty_list_is_filled() {
    let new = NewDependency { group: "g", artifact: "a", version: None, scope: None, optional: false };
    for list in ["<dependencies/>", "<dependencies></dependencies>", "<dependencies>\n  </dependencies>"] {
        let pom = format!("<project>\n  <artifactId>x</artifactId>\n  {list}\n</project>\n");
        let added = pom_edit::add(&pom, &new).unwrap();
        assert_eq!(
            added,
            "<project>\n  <artifactId>x</artifactId>\n  <dependencies>\n    <dependency>\n      <groupId>g</groupId>\n      <artifactId>a</artifactId>\n    </dependency>\n  </dependencies>\n</project>\n",
            "{list}"
        );
    }
}

#[test]
fn the_same_dependency_is_refused_but_another_scope_is_not() {
    zen_i18n::register(crate::LOCALES);
    let same = NewDependency { group: "org.slf4j", artifact: "slf4j-api", version: None, scope: None, optional: false };
    assert!(pom_edit::add(POM, &same).is_err());
    let test = NewDependency { scope: Some("test"), ..same };
    assert!(pom_edit::add(POM, &test).is_ok());
}

#[test]
fn removing_takes_only_the_dependency_and_leaves_its_neighbours_and_comments() {
    let removed = pom_edit::remove(POM, "com.fasterxml.jackson.core", "jackson-core", None).unwrap();
    assert!(!removed.contains("jackson-core"));
    assert!(removed.contains("<!-- logging -->") && removed.contains("<!-- pinned -->"));
    assert!(removed.contains("</dependency>\n\n  </dependencies>"), "{removed}");
    assert!(removed.contains("<jackson.version>2.17.0"), "the property stays");
}

#[test]
fn a_version_is_changed_on_the_spot_and_comments_stay() {
    let VersionEdit::Changed(changed) = pom_edit::set_version(POM, "org.slf4j", "slf4j-api", "2.1.0").unwrap() else {
        panic!("not changed");
    };
    assert_eq!(changed, POM.replace("2.0.13", "2.1.0"));
}

#[test]
fn a_version_in_a_property_is_changed_in_the_property() {
    let edit = pom_edit::set_version(POM, "com.fasterxml.jackson.core", "jackson-core", "2.18.0").unwrap();
    assert_eq!(edit, VersionEdit::Changed(POM.replace("2.17.0", "2.18.0")));
    let child = POM.replace("<jackson.version>2.17.0</jackson.version>", "");
    assert_eq!(
        pom_edit::set_version(&child, "com.fasterxml.jackson.core", "jackson-core", "1").unwrap(),
        VersionEdit::InProperty("jackson.version".to_owned())
    );
    assert_eq!(pom_edit::set_version(POM, "no", "such", "1").unwrap(), VersionEdit::Absent);
}

const GROOVY: &str = r#"plugins { id 'java' }

// versions
def guavaVersion = '32.0.0-jre'

repositories {
    mavenCentral()
    maven { url 'https://own.test/repo' }
}

dependencies {
    // the web layer
    implementation 'org.springframework:spring-web:6.1.0'
    implementation "com.google.guava:guava:$guavaVersion"
    testImplementation group: 'junit', name: 'junit', version: '4.13.2'
    /* implementation 'not:this:1' */
    implementation project(':core')
    runtimeOnly('org.postgresql:postgresql:42.7.0') {
        exclude group: 'x'
    }
}
"#;

const KOTLIN: &str = r#"plugins { java }

val kotestVersion = "5.8.0"

dependencies {
  implementation("org.springframework:spring-web:6.1.0")
  implementation(platform("org.springframework.boot:spring-boot-dependencies:3.3.0"))
  implementation("org.springframework.boot:spring-boot-starter")
  testImplementation("io.kotest:kotest-runner-junit5:$kotestVersion") // tests
  testRuntimeOnly(group = "org.junit.platform", name = "junit-platform-launcher", version = "1.10.0")
}
"#;

fn coordinates(text: &str) -> Vec<String> {
    script::statements(text)
        .into_iter()
        .map(|statement| {
            format!(
                "{} {}:{} {}",
                statement.configuration,
                statement.group,
                statement.artifact,
                statement.variable.or(statement.version).unwrap_or_default()
            )
        })
        .collect()
}

#[test]
fn groovy_and_kotlin_dependencies_are_read_in_every_notation() {
    assert_eq!(
        coordinates(GROOVY),
        [
            "implementation org.springframework:spring-web 6.1.0",
            "implementation com.google.guava:guava guavaVersion",
            "testImplementation junit:junit 4.13.2",
            "runtimeOnly org.postgresql:postgresql 42.7.0",
        ]
    );
    assert_eq!(
        coordinates(KOTLIN),
        [
            "implementation org.springframework:spring-web 6.1.0",
            "implementation org.springframework.boot:spring-boot-dependencies 3.3.0",
            "implementation org.springframework.boot:spring-boot-starter ",
            "testImplementation io.kotest:kotest-runner-junit5 kotestVersion",
            "testRuntimeOnly org.junit.platform:junit-platform-launcher 1.10.0",
        ]
    );
    assert!(script::statements(KOTLIN)[1].platform);
    assert_eq!(script::variable_in(GROOVY, "guavaVersion").unwrap().0, "32.0.0-jre");
    assert_eq!(script::variable_in(KOTLIN, "kotestVersion").unwrap().0, "5.8.0");
}

#[test]
fn repositories_of_a_script_are_read() {
    let urls: Vec<String> = script::repository_urls(GROOVY).into_iter().map(|(_, url)| url).collect();
    assert_eq!(urls, [sources::CENTRAL, "https://own.test/repo"]);
    let settings = "pluginManagement { repositories { gradlePluginPortal() } }\ndependencyResolutionManagement {\n  repositories {\n    google()\n    maven(\"https://r.test/m2\")\n  }\n}\n";
    let urls: Vec<String> = script::repository_urls(settings).into_iter().map(|(_, url)| url).collect();
    assert_eq!(urls, [script::GOOGLE, "https://r.test/m2"]);
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn edited<P: DependencyProvider>(
    provider: &P,
    root: &Path,
    home: &Path,
    change: DependencyChange,
) -> Result<Vec<(String, String)>, String> {
    let env = BuildEnv::default();
    env.set_home(Some(home.to_owned()));
    let open = HashMap::new();
    let files = BuildFiles { root, env: &env, open: &open, remote_pom: &|_, _, _, _| None, seen: Default::default() };
    let changes = provider.change(&files, &change)?;
    Ok(changes
        .into_iter()
        .map(|change| (change.path.strip_prefix(root).unwrap().to_string_lossy().into_owned(), change.text))
        .collect())
}

fn listed<P: DependencyProvider>(provider: &P, root: &Path, home: &Path) -> Vec<DependencyModule> {
    let env = BuildEnv::default();
    env.set_home(Some(home.to_owned()));
    let open = HashMap::new();
    let files = BuildFiles { root, env: &env, open: &open, remote_pom: &|_, _, _, _| None, seen: Default::default() };
    provider.modules(&files)
}

fn add(module: &str, coordinates: &str, version: &str, scope: &str) -> DependencyChange {
    let (group, artifact) = coordinates.split_once(':').unwrap();
    DependencyChange::Add {
        module: module.to_owned(),
        group: group.to_owned(),
        artifact: artifact.to_owned(),
        version: version.to_owned(),
        scope: scope.to_owned(),
    }
}

fn set(module: &str, coordinates: &str, version: &str) -> DependencyChange {
    let (group, artifact) = coordinates.split_once(':').unwrap();
    DependencyChange::SetVersion {
        module: module.to_owned(),
        group: group.to_owned(),
        artifact: artifact.to_owned(),
        version: version.to_owned(),
    }
}

fn remove(module: &str, coordinates: &str) -> DependencyChange {
    let (group, artifact) = coordinates.split_once(':').unwrap();
    DependencyChange::Remove {
        module: module.to_owned(),
        group: group.to_owned(),
        artifact: artifact.to_owned(),
        scope: None,
    }
}

#[test]
fn a_groovy_script_gets_a_line_in_its_own_style() {
    let dir = project(&[("build.gradle", GROOVY)]);
    let change = add("build.gradle", "com.squareup:okio", "3.9.0", "testImplementation");
    let changes = edited(&GradleDependencies, dir.path(), dir.path(), change).unwrap();
    assert_eq!(changes.len(), 1);
    let expected =
        GROOVY.replace("version: '4.13.2'\n", "version: '4.13.2'\n    testImplementation 'com.squareup:okio:3.9.0'\n");
    assert_eq!(changes[0].1, expected);
}

#[test]
fn a_kotlin_script_gets_a_line_in_its_own_style() {
    let dir = project(&[("build.gradle.kts", KOTLIN)]);
    let change = add("build.gradle.kts", "com.squareup:okio", "", "runtimeOnly");
    let changes = edited(&GradleDependencies, dir.path(), dir.path(), change).unwrap();
    let expected =
        KOTLIN.replace("version = \"1.10.0\")\n", "version = \"1.10.0\")\n  runtimeOnly(\"com.squareup:okio\")\n");
    assert_eq!(changes[0].1, expected, "a version a platform manages is not written");
}

#[test]
fn a_script_without_a_block_gets_one() {
    let dir = project(&[("build.gradle", "plugins { id 'java' }\n")]);
    let changes = edited(&GradleDependencies, dir.path(), dir.path(), add("build.gradle", "a:b", "1", "api")).unwrap();
    assert_eq!(changes[0].1, "plugins { id 'java' }\n\ndependencies {\n    api 'a:b:1'\n}\n");
}

#[test]
fn a_gradle_version_is_changed_where_it_is_written() {
    let dir = project(&[("build.gradle", GROOVY), ("gradle.properties", "other=1\nkotestVersion = 5.8.0\n")]);
    let script = "build.gradle";
    let line =
        edited(&GradleDependencies, dir.path(), dir.path(), set(script, "org.springframework:spring-web", "6.2.0"));
    assert_eq!(line.unwrap()[0].1, GROOVY.replace("6.1.0", "6.2.0"));
    let map = edited(&GradleDependencies, dir.path(), dir.path(), set(script, "junit:junit", "4.14"));
    assert_eq!(map.unwrap()[0].1, GROOVY.replace("4.13.2", "4.14"));
    let variable =
        edited(&GradleDependencies, dir.path(), dir.path(), set(script, "com.google.guava:guava", "33.0.0-jre"));
    assert_eq!(variable.unwrap()[0].1, GROOVY.replace("32.0.0-jre", "33.0.0-jre"));

    let kotlin = KOTLIN.replace("val kotestVersion = \"5.8.0\"\n", "val kotestVersion: String by project\n");
    let dir = project(&[("build.gradle.kts", &kotlin), ("gradle.properties", "other=1\nkotestVersion = 5.8.0\n")]);
    let changes = edited(
        &GradleDependencies,
        dir.path(),
        dir.path(),
        set("build.gradle.kts", "io.kotest:kotest-runner-junit5", "5.9.0"),
    )
    .unwrap();
    assert_eq!(changes, [("gradle.properties".to_owned(), "other=1\nkotestVersion = 5.9.0\n".to_owned())]);
}

#[test]
fn removing_a_gradle_dependency_takes_its_block_too() {
    let dir = project(&[("build.gradle", GROOVY)]);
    let changes =
        edited(&GradleDependencies, dir.path(), dir.path(), remove("build.gradle", "org.postgresql:postgresql"))
            .unwrap();
    assert_eq!(
        changes[0].1,
        GROOVY
            .replace("    runtimeOnly('org.postgresql:postgresql:42.7.0') {\n        exclude group: 'x'\n    }\n", "")
    );
    let changes =
        edited(&GradleDependencies, dir.path(), dir.path(), remove("build.gradle", "org.springframework:spring-web"))
            .unwrap();
    assert!(changes[0].1.contains("// the web layer\n    implementation \"com.google"), "the comment stays");
}

const CATALOG: &str = r#"# shared versions
[versions]
spring = "6.1.0" # pinned

[libraries]
spring-web = { module = "org.springframework:spring-web", version.ref = "spring" }
okio = "com.squareup:okio:3.9.0"
junit-api = { group = "org.junit.jupiter", name = "junit-jupiter-api", version = "5.10.0" }

[plugins]
boot = { id = "org.springframework.boot", version = "3.3.0" }
"#;

const WITH_CATALOG: &str = "plugins { java }\n\ndependencies {\n    implementation(libs.spring.web)\n    testImplementation(libs.junit.api)\n}\n";

#[test]
fn a_version_catalog_is_read_in_every_notation() {
    let parsed = catalog::parse(CATALOG);
    let shown: Vec<String> = parsed
        .libraries
        .iter()
        .map(|library| {
            format!("{} {}:{} {}", library.alias, library.group, library.artifact, library.resolved(&parsed).unwrap())
        })
        .collect();
    assert_eq!(
        shown,
        [
            "spring-web org.springframework:spring-web 6.1.0",
            "okio com.squareup:okio 3.9.0",
            "junit-api org.junit.jupiter:junit-jupiter-api 5.10.0",
        ]
    );
    assert_eq!(catalog::accessor("spring-web_mvc"), "spring.web.mvc");
}

#[test]
fn a_project_with_a_catalog_lists_its_libraries() {
    let dir = project(&[("build.gradle.kts", WITH_CATALOG), ("gradle/libs.versions.toml", CATALOG)]);
    let modules = listed(&GradleDependencies, dir.path(), dir.path());
    assert_eq!(modules[0].id, "build.gradle.kts");
    assert_eq!(modules[0].dependencies[0].group, "org.springframework");
    assert_eq!(modules[0].dependencies[0].version, "6.1.0");
    assert_eq!(modules[0].dependencies[0].version_place, VersionPlace::Catalog { alias: "spring-web".to_owned() });
}

#[test]
fn a_new_library_goes_into_the_catalog_and_the_script_gets_its_alias() {
    let dir = project(&[("build.gradle.kts", WITH_CATALOG), ("gradle/libs.versions.toml", CATALOG)]);
    let change = add("build.gradle.kts", "com.google.guava:guava", "33.0.0-jre", "implementation");
    let changes = edited(&GradleDependencies, dir.path(), dir.path(), change).unwrap();
    assert_eq!(changes[0].0, "gradle/libs.versions.toml");
    let expected = CATALOG
        .replace("spring = \"6.1.0\" # pinned\n", "spring = \"6.1.0\" # pinned\nguava = \"33.0.0-jre\"\n")
        .replace(
            "version = \"5.10.0\" }\n",
            "version = \"5.10.0\" }\nguava = { module = \"com.google.guava:guava\", version.ref = \"guava\" }\n",
        );
    assert_eq!(changes[0].1, expected);
    assert_eq!(
        changes[1].1,
        WITH_CATALOG.replace("(libs.spring.web)\n", "(libs.spring.web)\n    implementation(libs.guava)\n")
    );
}

#[test]
fn a_catalog_version_is_changed_in_the_catalog() {
    let dir = project(&[("build.gradle.kts", WITH_CATALOG), ("gradle/libs.versions.toml", CATALOG)]);
    let script = "build.gradle.kts";
    let changes =
        edited(&GradleDependencies, dir.path(), dir.path(), set(script, "org.springframework:spring-web", "6.2.0"))
            .unwrap();
    assert_eq!(
        changes,
        [("gradle/libs.versions.toml".to_owned(), CATALOG.replace("spring = \"6.1.0\"", "spring = \"6.2.0\""))]
    );
    let changes = edited(
        &GradleDependencies,
        dir.path(),
        dir.path(),
        set(script, "org.junit.jupiter:junit-jupiter-api", "5.11.0"),
    )
    .unwrap();
    assert_eq!(changes[0].1, CATALOG.replace("5.10.0", "5.11.0"));
    let toml = "gradle/libs.versions.toml";
    let changes =
        edited(&GradleDependencies, dir.path(), dir.path(), set(toml, "com.squareup:okio", "3.10.0")).unwrap();
    assert_eq!(changes[0].1, CATALOG.replace("okio:3.9.0", "okio:3.10.0"));
}

#[test]
fn removing_the_last_user_of_a_catalog_entry_removes_the_entry() {
    let other = "dependencies {\n    implementation(libs.junit.api)\n}\n";
    let settings = "include(\"app\")\n";
    let dir = project(&[
        ("settings.gradle.kts", settings),
        ("build.gradle.kts", WITH_CATALOG),
        ("app/build.gradle.kts", other),
        ("gradle/libs.versions.toml", CATALOG),
    ]);
    let script = "build.gradle.kts";
    let changes =
        edited(&GradleDependencies, dir.path(), dir.path(), remove(script, "org.springframework:spring-web")).unwrap();
    assert_eq!(changes.len(), 2, "the entry goes");
    assert!(!changes[0].1.contains("spring-web ="));
    assert!(!changes[1].1.contains("libs.spring.web"));
    let changes =
        edited(&GradleDependencies, dir.path(), dir.path(), remove(script, "org.junit.jupiter:junit-jupiter-api"))
            .unwrap();
    assert_eq!(changes.len(), 1, "another module still uses it");
}

const PARENT: &str = r#"<project>
  <groupId>demo</groupId>
  <artifactId>parent</artifactId>
  <version>1.0</version>
  <packaging>pom</packaging>
  <modules><module>app</module><module>lib</module></modules>
  <properties><junit.version>5.10.0</junit.version></properties>
  <dependencyManagement>
    <dependencies>
      <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.13</version></dependency>
      <dependency>
        <groupId>org.springframework.boot</groupId><artifactId>spring-boot-dependencies</artifactId>
        <version>3.3.0</version><type>pom</type><scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>
"#;

const APP: &str = r#"<project>
  <parent><groupId>demo</groupId><artifactId>parent</artifactId><version>1.0</version></parent>
  <artifactId>app</artifactId>
  <dependencies>
    <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId></dependency>
    <dependency><groupId>demo</groupId><artifactId>lib</artifactId><version>${project.version}</version></dependency>
    <dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><version>${junit.version}</version><scope>test</scope></dependency>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-web</artifactId></dependency>
  </dependencies>
</project>
"#;

const LIB: &str = "<project>\n  <parent><groupId>demo</groupId><artifactId>parent</artifactId><version>1.0</version></parent>\n  <artifactId>lib</artifactId>\n</project>\n";

const BOM: &str = r#"<project>
  <groupId>org.springframework.boot</groupId><artifactId>spring-boot-dependencies</artifactId><version>3.3.0</version>
  <properties><boot.version>3.3.0</boot.version></properties>
  <dependencyManagement><dependencies>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-web</artifactId><version>${boot.version}</version></dependency>
  </dependencies></dependencyManagement>
</project>
"#;

fn maven_project() -> tempfile::TempDir {
    let home =
        ".m2/repository/org/springframework/boot/spring-boot-dependencies/3.3.0/spring-boot-dependencies-3.3.0.pom";
    project(&[("repo/pom.xml", PARENT), ("repo/app/pom.xml", APP), ("repo/lib/pom.xml", LIB), (home, BOM)])
}

#[test]
fn maven_modules_show_where_each_version_is_written() {
    let dir = maven_project();
    let modules = listed(&MavenDependencies, &dir.path().join("repo"), dir.path());
    let ids: Vec<&str> = modules.iter().map(|module| module.id.as_str()).collect();
    assert_eq!(ids, ["pom.xml", "app/pom.xml", "lib/pom.xml"]);
    let app = &modules[1];
    assert_eq!(app.name, "app");
    assert_eq!(app.repositories.last().unwrap().url, sources::CENTRAL);
    let shown: Vec<(&str, &str, &VersionPlace, bool)> =
        app.dependencies.iter().map(|d| (d.artifact.as_str(), d.version.as_str(), &d.version_place, d.local)).collect();
    assert_eq!(
        shown,
        [
            ("slf4j-api", "2.0.13", &VersionPlace::ManagedHere { by: "pom.xml".to_owned() }, false),
            ("lib", "1.0", &VersionPlace::Literal, true),
            ("junit-jupiter", "5.10.0", &VersionPlace::Property { name: "junit.version".to_owned() }, false),
            (
                "spring-boot-starter-web",
                "3.3.0",
                &VersionPlace::Managed { by: "org.springframework.boot:spring-boot-dependencies:3.3.0".to_owned() },
                false
            ),
        ]
    );
}

#[test]
fn a_version_a_parent_or_a_bom_manages_is_not_written_again() {
    let dir = maven_project();
    let root = dir.path().join("repo");
    let change = add("lib/pom.xml", "org.slf4j:slf4j-api", "9.9", "compile");
    let changes = edited(&MavenDependencies, &root, dir.path(), change).unwrap();
    assert!(changes[0].1.contains("<artifactId>slf4j-api</artifactId>\n"), "{}", changes[0].1);
    assert!(!changes[0].1.contains("9.9"));
    let change = add("lib/pom.xml", "org.springframework.boot:spring-boot-starter-web", "9.9", "");
    let changes = edited(&MavenDependencies, &root, dir.path(), change).unwrap();
    assert!(!changes[0].1.contains("9.9"), "the BOM manages it");
    let change = add("lib/pom.xml", "org.other:free", "9.9", "test");
    let changes = edited(&MavenDependencies, &root, dir.path(), change).unwrap();
    assert!(changes[0].1.contains("<version>9.9</version>") && changes[0].1.contains("<scope>test</scope>"));
    assert!(changes[0].1.contains("<dependencies>"), "a list is made");
}

#[test]
fn an_upgrade_edits_the_parent_when_the_version_is_written_there() {
    let dir = maven_project();
    let root = dir.path().join("repo");
    let managed = set("app/pom.xml", "org.slf4j:slf4j-api", "2.1.0");
    let changes = edited(&MavenDependencies, &root, dir.path(), managed).unwrap();
    assert_eq!(changes, [("pom.xml".to_owned(), PARENT.replace("2.0.13", "2.1.0"))]);
    let property = set("app/pom.xml", "org.junit.jupiter:junit-jupiter", "5.11.0");
    let changes = edited(&MavenDependencies, &root, dir.path(), property).unwrap();
    assert_eq!(changes, [("pom.xml".to_owned(), PARENT.replace("5.10.0", "5.11.0"))]);
}

#[test]
fn an_upgrade_of_what_a_bom_from_a_repository_manages_is_refused() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = maven_project();
    let change = set("app/pom.xml", "org.springframework.boot:spring-boot-starter-web", "4.0.0");
    let error = edited(&MavenDependencies, &dir.path().join("repo"), dir.path(), change).unwrap_err();
    assert!(error.contains("managed outside"), "{error}");
}

#[test]
fn the_repositories_a_project_names_are_used_only_once_it_is_trusted() {
    let pom = "<project>\n  <artifactId>x</artifactId>\n  <repositories>\n    <repository><id>corp</id><url>https://evil.test/m2</url></repository>\n  </repositories>\n</project>\n";
    let settings = "<settings><servers><server><id>corp</id><username>me</username><password>secret</password></server></servers></settings>";
    let dir = project(&[("repo/pom.xml", pom), (".m2/settings.xml", settings)]);
    let root = dir.path().join("repo");
    let shown = |env: &BuildEnv| {
        let open = HashMap::new();
        let files =
            BuildFiles { root: &root, env, open: &open, remote_pom: &|_, _, _, _| None, seen: Default::default() };
        MavenDependencies.modules(&files).remove(0).repositories
    };
    let env = BuildEnv::default();
    env.set_home(Some(dir.path().to_owned()));
    let untrusted = shown(&env);
    assert_eq!(untrusted.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), [sources::CENTRAL]);
    crate::trust::trust_in_tests(&env, &root);
    let trusted = shown(&env);
    assert_eq!(trusted[0].url, "https://evil.test/m2");
    assert!(trusted[0].authorization.is_some(), "the person trusted the project and its repository");

    let script = "repositories { maven { url 'https://evil.test/m2' } }\n";
    let gradle = project(&[("build.gradle", script)]);
    let modules = listed(&GradleDependencies, gradle.path(), gradle.path());
    assert_eq!(modules[0].repositories.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), [sources::CENTRAL]);
}

#[test]
fn a_plain_http_repository_is_not_asked() {
    let transport = Arc::new(Recorded::default());
    let service = service(&transport);
    let plain = vec![Repository::new("old", "http://old.test/m2")];
    let (status, failed) = service.status("a", "b", "1", &plain, ALL);
    service.wait_idle(Duration::from_secs(10));
    assert!(!status.pending && !failed && transport.urls().is_empty());
}

#[test]
fn the_modules_are_read_again_only_when_a_build_file_changed() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    let first = window.run("dependencies.project.list", json!({}));
    let service = window.session.state_mut::<Dependencies>().clone();
    let reads = || locked(&service.shared.state).read.as_ref().map(|read| read.files.len());
    assert!(reads().is_some_and(|files| files > 0));
    let memo = locked(&service.shared.state).read.as_ref().unwrap().modules.clone();
    locked(&service.shared.state).read.as_mut().unwrap().modules[0].name = "kept".to_owned();
    assert_eq!(window.run("dependencies.project.list", json!({}))["modules"][0]["name"], "kept", "not read again");
    let changed = SIMPLE.replace("<artifactId>shop</artifactId>", "<artifactId>shop</artifactId><name>shop two</name>");
    std::fs::write(window.root.join("pom.xml"), changed).unwrap();
    assert_eq!(window.run("dependencies.project.list", json!({}))["modules"][0]["name"], "shop two");
    assert_eq!(first["modules"][0]["name"], memo[0].name);
}

#[test]
fn settings_xml_gives_repositories_mirrors_and_credentials_for_their_own_host_only() {
    let settings = r#"<settings>
      <servers>
        <server><id>corp</id><username>me</username><password>secret</password></server>
        <server><id>locked</id><username>me</username><password>{encrypted}</password></server>
      </servers>
      <mirrors><mirror><id>corp</id><url>https://corp.test/all</url><mirrorOf>central,!locked</mirrorOf></mirror></mirrors>
      <profiles><profile><id>p</id><repositories>
        <repository><id>locked</id><url>https://locked.test/m2</url></repository>
        <repository><id>snapshots</id><url>https://snap.test</url><releases><enabled>false</enabled></releases></repository>
      </repositories></profile></profiles>
      <activeProfiles><activeProfile>p</activeProfile></activeProfiles>
    </settings>"#;
    let dir = project(&[("repo/pom.xml", LIB), (".m2/settings.xml", settings)]);
    let modules = listed(&MavenDependencies, &dir.path().join("repo"), dir.path());
    let repositories = &modules[0].repositories;
    let shown: Vec<(&str, bool)> = repositories.iter().map(|r| (r.url.as_str(), r.authorization.is_some())).collect();
    assert_eq!(shown, [("https://locked.test/m2", false), ("https://corp.test/all", true)]);
    assert!(!serde_json::to_string(repositories).unwrap().contains("Basic"), "credentials are never shown");
    assert!(!format!("{repositories:?}").contains("Basic"));
}

struct Window {
    registry: Registry,
    session: Session,
    root: PathBuf,
    transport: Arc<Recorded>,
    _dir: tempfile::TempDir,
}

fn fake_tool(bin: &Path, name: &str) {
    std::fs::create_dir_all(bin).unwrap();
    let path = bin.join(name);
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn window(files: &[(&str, &str)]) -> Window {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = project(files);
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let bin = root.join("bin");
    fake_tool(&bin, "mvn");
    fake_tool(&bin, "gradle");
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    let env = BuildEnv::default();
    env.set_folders(Some(vec![bin]));
    env.set_home(Some(root.join("home")));
    crate::trust::trust_in_tests(&env, &root);
    session.set_state(env.clone());
    let transport = Arc::new(Recorded::default());
    let service = service(&transport);
    service.set_env(env);
    session.set_state(service);
    for key in [SEARCH_SETTING, CHECK_UPDATES, CHECK_VULNERABILITIES] {
        session.settings_mut().set(key, json!(true)).unwrap();
    }
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    Window { registry, session, root, transport, _dir: dir }
}

impl Window {
    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap()
    }

    fn dispatch(&mut self, id: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
        self.registry.dispatch(&mut self.session, id, params)
    }

    fn inspector(&mut self) {
        let service = self.session.state_mut::<Dependencies>().clone();
        self.session.state_mut::<crate::inspections::Inspections>().set_inspectors(vec![service.inspector()], None);
    }
}

const SIMPLE: &str = "<project>\n  <groupId>demo</groupId>\n  <artifactId>shop</artifactId>\n  <version>1</version>\n  <dependencies>\n    <dependency>\n      <groupId>a.b</groupId>\n      <artifactId>c</artifactId>\n      <version>1.0</version>\n    </dependency>\n  </dependencies>\n</project>\n";

fn checks(transport: &Arc<Recorded>) {
    transport.get(&format!("{CENTRAL}/a/b/c/maven-metadata.xml"), metadata(&["1.0", "1.1", "2.0"]));
}

#[test]
fn the_commands_list_add_upgrade_and_remove_and_the_project_reloads() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    checks(&window.transport);
    window.transport.post(
        "a.b:c",
        json!({ "vulns": [vulnerability("7", "a.b:c", json!([{ "introduced": "0" }, { "fixed": "1.1" }]))] }),
    );
    let central = Repository::new("central", CENTRAL);
    window.session.state_mut::<Dependencies>().set_providers(vec![Arc::new(Central(central))]);
    let listed = window.run("dependencies.project.list", json!({ "waitMs": 10000 }));
    let module = &listed["modules"][0];
    assert_eq!(module["id"], "pom.xml");
    let dependency = &module["dependencies"][0];
    assert_eq!(dependency["status"]["latest"], "2.0");
    assert_eq!(dependency["status"]["newer"], json!(["2.0", "1.1"]));
    assert_eq!(dependency["status"]["safe"], "1.1");
    assert_eq!(dependency["status"]["advisories"][0]["id"], "CVE-7");
    assert_eq!(listed["pending"], false);

    let added = window.run(
        "dependencies.module.add",
        json!({ "module": "pom.xml", "group": "x", "artifact": "y", "version": "3", "scope": "test" }),
    );
    assert_eq!(added["reloaded"], true, "{added}");
    let text = std::fs::read_to_string(window.root.join("pom.xml")).unwrap();
    assert!(
        text.contains("<artifactId>y</artifactId>\n      <version>3</version>\n      <scope>test</scope>"),
        "{text}"
    );

    window.run(
        "dependencies.version.set",
        json!({ "module": "pom.xml", "group": "a.b", "artifact": "c", "version": "2.0" }),
    );
    let text = std::fs::read_to_string(window.root.join("pom.xml")).unwrap();
    assert!(text.contains("<version>2.0</version>"));

    window.run("dependencies.module.remove", json!({ "module": "pom.xml", "group": "x", "artifact": "y" }));
    let text = std::fs::read_to_string(window.root.join("pom.xml")).unwrap();
    assert_eq!(text, SIMPLE.replace("<version>1.0</version>", "<version>2.0</version>"));
}

struct Central(Repository);

impl DependencyProvider for Central {
    fn id(&self) -> &str {
        MavenDependencies.id()
    }

    fn applies(&self, root: &Path) -> bool {
        MavenDependencies.applies(root)
    }

    fn handles(&self, file_name: &str) -> bool {
        MavenDependencies.handles(file_name)
    }

    fn modules(&self, files: &BuildFiles<'_>) -> Vec<DependencyModule> {
        let mut modules = MavenDependencies.modules(files);
        for module in &mut modules {
            module.repositories = vec![self.0.clone()];
        }
        modules
    }

    fn change(&self, files: &BuildFiles<'_>, change: &DependencyChange) -> Result<Vec<FileChange>, String> {
        MavenDependencies.change(files, change)
    }

    fn sites(&self, files: &BuildFiles<'_>, path: &Path, text: &str) -> Vec<VersionSite> {
        MavenDependencies.sites(files, path, text)
    }

    fn completing(&self, path: &Path, text: &str, caret: usize) -> Option<Completing> {
        MavenDependencies.completing(path, text, caret)
    }

    fn repositories_for(&self, _files: &BuildFiles<'_>, _path: &Path) -> Vec<Repository> {
        vec![self.0.clone()]
    }

    fn reload(&self, session: &mut Session) -> Result<(), CommandError> {
        MavenDependencies.reload(session)
    }
}

#[test]
fn an_open_build_file_is_edited_in_its_editor_and_saved() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    let pom = window.root.join("pom.xml");
    window.run("file.document.open", json!({ "path": pom }));
    let document = window.session.state_mut::<Workbench>().active_document_mut().unwrap();
    assert!(document.replace_ranges(vec![(0..0, "<!-- mine -->\n".to_owned())]));
    window.run(
        "dependencies.module.add",
        json!({ "module": "pom.xml", "group": "x", "artifact": "y", "version": "3", "scope": "test" }),
    );
    let text = std::fs::read_to_string(&pom).unwrap();
    assert!(text.contains("<!-- mine -->") && text.contains("<artifactId>y</artifactId>"), "{text}");
    assert_eq!(window.run("editor.edit.undo", json!(null)), json!(true));
    let document = window.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert!(!document.text().contains("<artifactId>y"), "one undo takes the edit back");
}

#[test]
fn outdated_and_vulnerable_versions_are_marked_with_a_quick_fix_that_upgrades() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    window.inspector();
    checks(&window.transport);
    window.transport.post(
        "a.b:c",
        json!({ "vulns": [vulnerability("7", "a.b:c", json!([{ "introduced": "0" }, { "fixed": "1.1" }]))] }),
    );
    let service = window.session.state_mut::<Dependencies>().clone();
    service.set_providers(vec![Arc::new(Central(Repository::new("central", CENTRAL)))]);
    let pom = window.root.join("pom.xml");
    window.run("file.document.open", json!({ "path": pom }));
    window.run("problems.file.list", json!({}));
    for _ in 0..3 {
        service.wait_idle(Duration::from_secs(10));
        window.run("problems.file.list", json!({}));
    }
    let problems = window.run("problems.file.list", json!({}));
    let found = problems["problems"].as_array().or(problems.as_array()).unwrap();
    let problem = found.iter().find(|problem| problem["code"] == "vulnerable").expect("marked");
    assert_eq!(problem["rows"][0]["label"], "CVE-7");
    assert_eq!(problem["rows"][0]["badge"], "9.8");
    assert_eq!(problem["fixes"][0]["title"], "Upgrade to unaffected version 1.1");
    let id = problem["fixes"][0]["id"].clone();
    window.run("problems.fix.apply", json!({ "ids": [id] }));
    for _ in 0..20 {
        if window
            .run("problems.fix.status", json!({}))
            .as_array()
            .is_some_and(|jobs| jobs.iter().all(|job| job["state"] != "running"))
        {
            break;
        }
    }
    let text = std::fs::read_to_string(&pom).unwrap();
    assert_eq!(text, SIMPLE.replace("<version>1.0</version>", "<version>1.1</version>"));
    for _ in 0..3 {
        service.wait_idle(Duration::from_secs(10));
        window.run("problems.file.list", json!({}));
    }
    let problems = window.run("problems.file.list", json!({}));
    let found = problems["problems"].as_array().or(problems.as_array()).unwrap();
    let problem = found.iter().find(|problem| problem["code"] == "outdated").expect("outdated now");
    assert_eq!(problem["message"], "Newer version is available: 2.0");
}

#[test]
fn gradle_scripts_and_the_catalog_are_inspected_too() {
    let script = "dependencies {\n    implementation 'a.b:c:1.0'\n}\n";
    let toml = "[libraries]\nd = \"a.b:c:1.0\"\n";
    let mut window = window(&[("build.gradle", script), ("gradle/libs.versions.toml", toml)]);
    window.inspector();
    checks(&window.transport);
    window.transport.post("a.b:c", json!({ "vulns": [] }));
    let service = window.session.state_mut::<Dependencies>().clone();
    service.set_endpoints(Endpoints { search: SEARCH.to_owned(), central: CENTRAL.to_owned(), osv: OSV.to_owned() });
    service.set_providers(vec![Arc::new(Scripts(Repository::new("central", CENTRAL)))]);
    for (file, start) in
        [("build.gradle", script.find("1.0").unwrap()), ("gradle/libs.versions.toml", toml.find("1.0").unwrap())]
    {
        let path = window.root.join(file);
        window.run("file.document.open", json!({ "path": path }));
        for _ in 0..3 {
            window.run("problems.file.list", json!({}));
            service.wait_idle(Duration::from_secs(10));
        }
        let problems = window.run("problems.file.list", json!({}));
        let found = problems.as_array().unwrap();
        assert_eq!(found.len(), 1, "{file}: {problems}");
        assert_eq!(found[0]["code"], "outdated");
        assert_eq!(found[0]["fixes"][0]["title"], "Upgrade to 2.0");
        let document = window.session.state::<Workbench>().unwrap().active_document().unwrap();
        let chars = document.text()[..start].chars().count() as u64;
        assert!(found[0]["start"].as_u64().is_some_and(|at| at == chars || at == start as u64), "{problems}");
    }
}

struct Scripts(Repository);

impl DependencyProvider for Scripts {
    fn id(&self) -> &str {
        GradleDependencies.id()
    }

    fn applies(&self, root: &Path) -> bool {
        GradleDependencies.applies(root)
    }

    fn handles(&self, file_name: &str) -> bool {
        GradleDependencies.handles(file_name)
    }

    fn modules(&self, files: &BuildFiles<'_>) -> Vec<DependencyModule> {
        GradleDependencies.modules(files)
    }

    fn change(&self, files: &BuildFiles<'_>, change: &DependencyChange) -> Result<Vec<FileChange>, String> {
        GradleDependencies.change(files, change)
    }

    fn sites(&self, files: &BuildFiles<'_>, path: &Path, text: &str) -> Vec<VersionSite> {
        GradleDependencies.sites(files, path, text)
    }

    fn completing(&self, path: &Path, text: &str, caret: usize) -> Option<Completing> {
        GradleDependencies.completing(path, text, caret)
    }

    fn repositories_for(&self, _files: &BuildFiles<'_>, _path: &Path) -> Vec<Repository> {
        vec![self.0.clone()]
    }

    fn reload(&self, session: &mut Session) -> Result<(), CommandError> {
        GradleDependencies.reload(session)
    }
}

#[test]
fn offline_the_build_file_shows_nothing_and_stays_quiet() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    window.inspector();
    let pom = window.root.join("pom.xml");
    window.run("file.document.open", json!({ "path": pom }));
    let service = window.session.state_mut::<Dependencies>().clone();
    window.run("problems.file.list", json!({}));
    service.wait_idle(Duration::from_secs(10));
    let problems = window.run("problems.file.list", json!({}));
    assert_eq!(problems.as_array().map(Vec::len).unwrap_or_default(), 0, "{problems}");
    let listed = window.run("dependencies.project.list", json!({}));
    assert_eq!(listed["modules"][0]["dependencies"][0]["artifact"], "c", "listing needs no network");
    assert_eq!(listed["failed"], true);
}

#[test]
fn turned_off_in_the_settings_nothing_is_asked() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    for key in [SEARCH_SETTING, CHECK_UPDATES, CHECK_VULNERABILITIES] {
        window.session.settings_mut().set(key, json!(false)).unwrap();
    }
    let listed = window.run("dependencies.project.list", json!({ "waitMs": 100 }));
    assert_eq!(listed["network"], false);
    assert_eq!(listed["modules"][0]["dependencies"][0]["version"], "1.0");
    assert_eq!(window.run("dependencies.repository.search", json!({ "query": "x" }))["disabled"], true);
    assert!(window.transport.urls().is_empty());
}

#[test]
fn maven_work_offline_keeps_the_manager_off_the_network() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    window.run("maven.options.set", json!({ "offline": true }));
    let listed = window.run("dependencies.project.list", json!({ "waitMs": 100 }));
    assert_eq!(listed["network"], false);
    assert!(window.transport.urls().is_empty());
}

#[test]
fn a_provider_works_only_where_its_plug_in_supports_the_project() {
    let dir = project(&[("pom.xml", SIMPLE)]);
    let service = Dependencies::default();
    assert_eq!(service.providers_for(dir.path()).len(), 2);
    service.set_plugins(Some(Arc::new(Vec::new)));
    assert!(service.providers_for(dir.path()).is_empty());
}

fn completing(text: &str) -> Option<Completing> {
    let caret = text.find('|').unwrap();
    let text = text.replace('|', "");
    MavenDependencies.completing(Path::new("pom.xml"), &text, caret)
}

#[test]
fn a_pom_completes_coordinates_inside_a_dependency_only() {
    assert_eq!(
        completing("<dependency><groupId>org.sp|"),
        Some(Completing::Group { prefix: "org.sp".to_owned(), from: 21 })
    );
    assert_eq!(
        completing("<dependency>\n<groupId>a.b</groupId>\n<artifactId>sp|</artifactId>\n</dependency>"),
        Some(Completing::Artifact { group: "a.b".to_owned(), prefix: "sp".to_owned(), from: 48 })
    );
    assert_eq!(
        completing("<dependency><groupId>g</groupId><artifactId>a</artifactId><version>|</version></dependency>"),
        Some(Completing::Version { group: "g".to_owned(), artifact: "a".to_owned(), prefix: String::new(), from: 67 })
    );
    assert_eq!(completing("<parent><groupId>org|"), None);
    assert_eq!(completing("<dependency></dependency><groupId>org|"), None);
}

fn gradle_completing(name: &str, text: &str) -> Option<Completing> {
    let caret = text.find('|').unwrap();
    let text = text.replace('|', "");
    GradleDependencies.completing(Path::new(name), &text, caret)
}

#[test]
fn gradle_files_complete_coordinates_in_their_strings() {
    let kotlin = "dependencies {\n  implementation(\"org.sp|\n}\n";
    assert_eq!(
        gradle_completing("build.gradle.kts", kotlin),
        Some(Completing::Group { prefix: "org.sp".to_owned(), from: 33 })
    );
    let groovy = "dependencies {\n  implementation 'g:a:1.|'\n}\n";
    assert_eq!(
        gradle_completing("build.gradle", groovy),
        Some(Completing::Version {
            group: "g".to_owned(),
            artifact: "a".to_owned(),
            prefix: "1.".to_owned(),
            from: 37
        })
    );
    assert_eq!(gradle_completing("build.gradle", "plugins {\n id 'a|'\n}\n"), None);
    let toml = "[libraries]\nfoo = { module = \"org.x:co|\" }\n";
    assert_eq!(
        gradle_completing("libs.versions.toml", toml),
        Some(Completing::Artifact { group: "org.x".to_owned(), prefix: "co".to_owned(), from: 36 })
    );
    assert_eq!(gradle_completing("libs.versions.toml", "[plugins]\nfoo = { id = \"org.x|\" }\n"), None);
}

#[test]
fn completion_lists_what_the_index_and_the_metadata_give() {
    let mut window = window(&[(
        "pom.xml",
        "<project>\n<dependencies>\n<dependency>\n<groupId>a.b</groupId>\n<artifactId>c</artifactId>\n<version>1.</version>\n</dependency>\n</dependencies>\n</project>\n",
    )]);
    checks(&window.transport);
    window
        .session
        .state_mut::<Dependencies>()
        .set_providers(vec![Arc::new(Central(Repository::new("central", CENTRAL)))]);
    let pom = window.root.join("pom.xml");
    window.run("file.document.open", json!({ "path": pom }));
    let text = std::fs::read_to_string(&pom).unwrap();
    let caret = text.find("1.<").unwrap() + 2;
    window.run("editor.selection.set", json!({ "selections": [{ "head": caret }] }));
    window.run("editor.completion.start", json!({}));
    let service = window.session.state_mut::<Dependencies>().clone();
    service.wait_idle(Duration::from_secs(10));
    let list = window.run("editor.completion.list", json!({}));
    let labels: Vec<&str> =
        list["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
    assert!(labels.contains(&"1.0") && labels.contains(&"1.1"), "{list}");
    assert!(!labels.contains(&"2.0"), "the typed part filters: {list}");
}

const STARTERS_POM: &str = "<project>\n  <modelVersion>4.0.0</modelVersion>\n  <parent>\n    <groupId>org.springframework.boot</groupId>\n    <artifactId>spring-boot-starter-parent</artifactId>\n    <version>3.5.0</version>\n  </parent>\n  <artifactId>shop</artifactId>\n  <dependencies>\n    <dependency>\n      <groupId>org.springframework.boot</groupId>\n      <artifactId>spring-boot-starter-web</artifactId>\n    </dependency>\n  </dependencies>\n</project>\n";

#[test]
fn a_bom_goes_into_dependency_management_once_and_before_the_dependencies() {
    let bom = NewBom { group: "org.acme", artifact: "acme-bom", version: "1.0" };
    let text = pom_edit::add_bom(STARTERS_POM, &bom).unwrap();
    let management = text.find("<dependencyManagement>").unwrap();
    assert!(
        management < text.find("<dependencies>\n    <dependency>\n      <groupId>org.springframework.boot").unwrap()
    );
    assert!(
        text.contains("        <version>1.0</version>\n        <type>pom</type>\n        <scope>import</scope>"),
        "{text}"
    );
    assert_eq!(pom_edit::add_bom(&text, &bom).unwrap(), text, "the same BOM twice changes nothing");
    let other = NewBom { artifact: "other-bom", ..bom };
    let twice = pom_edit::add_bom(&text, &other).unwrap();
    assert_eq!(twice.matches("<scope>import</scope>").count(), 2);
    assert_eq!(twice.matches("<dependencyManagement>").count(), 1);
}

#[test]
fn a_repository_is_added_once_and_snapshots_stay_off_unless_the_service_says() {
    let new = NewRepository { id: "acme", name: "Acme", url: "https://repo.acme.org", snapshots: false };
    let text = pom_edit::add_repository(STARTERS_POM, &new).unwrap();
    assert!(text.contains("<repositories>\n    <repository>\n      <id>acme</id>"), "{text}");
    assert!(text.contains("      <snapshots>\n        <enabled>false</enabled>\n      </snapshots>"), "{text}");
    assert_eq!(pom_edit::add_repository(&text, &new).unwrap(), text);
    let other = NewRepository { id: "two", url: "https://repo.two.org", snapshots: true, ..new };
    let twice = pom_edit::add_repository(&text, &other).unwrap();
    assert_eq!(twice.matches("<repository>").count(), 2);
    assert_eq!(twice.matches("<snapshots>").count(), 1);
}

#[test]
fn an_annotation_processor_is_optional_in_a_pom_as_the_service_generates_it() {
    let new = NewDependency { group: "l", artifact: "lombok", version: None, scope: None, optional: true };
    let text = pom_edit::add(STARTERS_POM, &new).unwrap();
    assert!(text.contains("<artifactId>lombok</artifactId>\n      <optional>true</optional>"), "{text}");
}

#[test]
fn gradle_takes_a_bom_through_the_dependency_management_plugin_when_it_has_it() {
    let groovy = "plugins {\n    id 'io.spring.dependency-management' version '1.1.7'\n}\n\ndependencies {\n    implementation 'a:b'\n}\n";
    let text = script::add_bom(groovy, "org.acme", "acme-bom", "1.0", false);
    assert!(
        text.ends_with("dependencyManagement {\n    imports {\n        mavenBom 'org.acme:acme-bom:1.0'\n    }\n}\n"),
        "{text}"
    );
    assert_eq!(script::add_bom(&text, "org.acme", "acme-bom", "1.0", false), text);
    let second = script::add_bom(&text, "org.acme", "two-bom", "2.0", false);
    assert!(
        second.contains("mavenBom 'org.acme:acme-bom:1.0'\n        mavenBom 'org.acme:two-bom:2.0'\n    }"),
        "{second}"
    );

    let kotlin = "dependencies {\n    implementation(\"a:b\")\n}\n";
    let text = script::add_bom(kotlin, "org.acme", "acme-bom", "1.0", true);
    assert!(text.contains("implementation(platform(\"org.acme:acme-bom:1.0\"))"), "{text}");
}

#[test]
fn gradle_gets_a_repository_in_its_own_block_or_a_new_one_before_the_dependencies() {
    let groovy = "repositories {\n    mavenCentral()\n}\n\ndependencies {\n}\n";
    let text = script::add_repository(groovy, "https://repo.acme.org", false);
    assert!(text.contains("    mavenCentral()\n    maven { url = 'https://repo.acme.org' }\n}"), "{text}");
    assert_eq!(script::add_repository(&text, "https://repo.acme.org/", false), text);

    let kotlin = "dependencies {\n    implementation(\"a:b\")\n}\n";
    let text = script::add_repository(kotlin, "https://repo.acme.org", true);
    assert!(
        text.starts_with("repositories {\n    maven { url = uri(\"https://repo.acme.org\") }\n}\n\ndependencies"),
        "{text}"
    );
}

#[test]
fn a_failed_step_of_several_changes_writes_nothing() {
    let mut window = window(&[("pom.xml", STARTERS_POM)]);
    let before = std::fs::read_to_string(window.root.join("pom.xml")).unwrap();
    let wanted = [
        add("pom.xml", "x:y", "1", ""),
        DependencyChange::Remove {
            module: "pom.xml".to_owned(),
            group: "not".to_owned(),
            artifact: "declared".to_owned(),
            scope: None,
        },
    ];
    assert!(change_all(&mut window.session, "pom.xml", &wanted).is_err());
    assert_eq!(std::fs::read_to_string(window.root.join("pom.xml")).unwrap(), before);
}

struct Initializr(std::sync::Mutex<Result<Value, String>>);

impl zen_project::generators::ProjectGenerator for Initializr {
    fn info(&self) -> zen_project::generators::GeneratorInfo {
        zen_project::generators::GeneratorInfo {
            id: "spring-boot.initializr".to_owned(),
            ..zen_project::generators::GeneratorInfo::default()
        }
    }

    fn form(&self) -> Result<zen_project::generators::GeneratorForm, String> {
        Ok(zen_project::generators::GeneratorForm::default())
    }

    fn files(
        &self,
        _request: &zen_project::generators::GeneratorRequest,
    ) -> Result<Vec<zen_project::generators::GeneratedFile>, String> {
        Ok(Vec::new())
    }

    fn step(
        &self,
        _step: &str,
        values: zen_project::generators::Values,
    ) -> Result<zen_project::generators::StepAnswer, String> {
        assert_eq!(values["bootVersion"], "3.5.0", "the version comes from the build");
        let answer = self.0.lock().unwrap().clone()?;
        Ok(zen_project::generators::StepAnswer { answer, from_saved: false })
    }
}

fn starters_answer() -> Value {
    json!({ "starters": {
        "field": { "id": "dependencies", "label": "Dependencies", "type": "select", "groups": [{ "name": "Web", "options": [
            { "value": "web", "label": "Web" }, { "value": "ai", "label": "AI" }, { "value": "lombok", "label": "Lombok" }
        ] }] },
        "ticked": ["web"],
        "dependencies": {
            "web": { "groupId": "org.springframework.boot", "artifactId": "spring-boot-starter-web", "scope": "compile" },
            "ai": { "groupId": "org.springframework.ai", "artifactId": "spring-ai-starter", "scope": "compile", "bom": "spring-ai", "repository": "milestones" },
            "lombok": { "groupId": "org.projectlombok", "artifactId": "lombok", "scope": "annotationProcessor" }
        },
        "boms": { "spring-ai": { "groupId": "org.springframework.ai", "artifactId": "spring-ai-bom", "version": "1.1.0" } },
        "repositories": { "milestones": { "name": "Milestones", "url": "https://repo.spring.io/milestone", "snapshotEnabled": false } }
    } })
}

fn with_initializr(window: &mut Window, answer: Result<Value, String>) {
    let plugin = |id: &str, marker: &str| crate::plugin_projects::PluginProject {
        id: id.to_owned(),
        name: id.to_owned(),
        support: zen_project::support::ProjectSupport::parse(&format!(r#"{{"markers":{{"files":["{marker}"]}}}}"#))
            .unwrap(),
    };
    let plugins = vec![
        plugin(crate::spring_boot::PLUGIN, "pom.xml"),
        plugin("maven", "pom.xml"),
        plugin("gradle", "build.gradle"),
    ];
    window.session.state_mut::<crate::plugin_projects::PluginProjects>().set(plugins);
    window
        .session
        .state_mut::<Dependencies>()
        .set_providers(vec![Arc::new(MavenDependencies), Arc::new(GradleDependencies)]);
    let projects = crate::new_project::NewProjects::default();
    projects.declare(Arc::new(Initializr(std::sync::Mutex::new(answer))));
    window.session.set_state(projects);
}

const STARTERS_GRADLE: &str = "plugins {\n    id 'org.springframework.boot' version '3.5.0'\n    id 'io.spring.dependency-management' version '1.1.7'\n}\n\nrepositories {\n    mavenCentral()\n}\n\ndependencies {\n    implementation 'org.springframework.boot:spring-boot-starter-web'\n}\n";

#[test]
fn starters_of_a_maven_project_are_edited_in_its_pom_and_the_project_reloads() {
    let mut window = window(&[("pom.xml", STARTERS_POM)]);
    with_initializr(&mut window, Ok(starters_answer()));
    let got = window.run("springBoot.starters.get", json!({ "waitMs": 10000 }));
    assert_eq!((got["state"].clone(), got["bootVersion"].clone()), (json!("ready"), json!("3.5.0")), "{got}");
    assert_eq!(got["ticked"], json!(["web"]));
    assert_eq!(got["field"]["groups"][0]["options"][1]["label"], "AI");

    let done = window.run("springBoot.starters.apply", json!({ "add": ["ai", "lombok"], "remove": ["web"] }));
    assert_eq!(done["reloaded"], true, "{done}");
    let text = std::fs::read_to_string(window.root.join("pom.xml")).unwrap();
    assert!(!text.contains("spring-boot-starter-web"), "{text}");
    assert!(text.contains("<artifactId>spring-ai-starter</artifactId>"), "{text}");
    assert!(text.contains("<artifactId>spring-ai-bom</artifactId>"), "{text}");
    assert!(text.contains("<url>https://repo.spring.io/milestone</url>"), "{text}");
    assert!(text.contains("<artifactId>lombok</artifactId>\n      <optional>true</optional>"), "{text}");
}

#[test]
fn starters_of_a_gradle_project_are_edited_in_its_script() {
    let mut window = window(&[("build.gradle", STARTERS_GRADLE), ("settings.gradle", "rootProject.name = 'shop'\n")]);
    with_initializr(&mut window, Ok(starters_answer()));
    let got = window.run("springBoot.starters.get", json!({ "waitMs": 10000 }));
    assert_eq!(got["state"], "ready", "{got}");
    window.run("springBoot.starters.apply", json!({ "add": ["ai", "lombok"], "remove": ["web"] }));
    let text = std::fs::read_to_string(window.root.join("build.gradle")).unwrap();
    assert!(!text.contains("spring-boot-starter-web"), "{text}");
    assert!(text.contains("implementation 'org.springframework.ai:spring-ai-starter'"), "{text}");
    assert!(text.contains("annotationProcessor 'org.projectlombok:lombok'"), "{text}");
    assert!(text.contains("mavenBom 'org.springframework.ai:spring-ai-bom:1.1.0'"), "{text}");
    assert!(text.contains("maven { url = 'https://repo.spring.io/milestone' }"), "{text}");
}

#[test]
fn without_the_network_the_build_file_is_left_alone() {
    let mut window = window(&[("pom.xml", STARTERS_POM)]);
    with_initializr(&mut window, Err("Editing starters needs the network.".to_owned()));
    let got = window.run("springBoot.starters.get", json!({ "waitMs": 10000 }));
    assert_eq!(got["state"], "failed");
    assert_eq!(got["error"], "Editing starters needs the network.");
    let refused = window.dispatch("springBoot.starters.apply", json!({ "add": ["ai"] }));
    assert!(refused.is_err());
    assert_eq!(std::fs::read_to_string(window.root.join("pom.xml")).unwrap(), STARTERS_POM);
}

#[test]
fn a_starter_the_service_does_not_list_refuses_the_whole_edit() {
    let mut window = window(&[("pom.xml", STARTERS_POM)]);
    with_initializr(&mut window, Ok(starters_answer()));
    window.run("springBoot.starters.get", json!({ "waitMs": 10000 }));
    let refused = window.dispatch("springBoot.starters.apply", json!({ "add": ["ai", "nothing"] }));
    assert!(refused.is_err());
    assert_eq!(std::fs::read_to_string(window.root.join("pom.xml")).unwrap(), STARTERS_POM);
}

#[test]
fn a_project_without_a_spring_boot_version_is_told_so() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    with_initializr(&mut window, Ok(starters_answer()));
    let refused = window.dispatch("springBoot.starters.get", json!({}));
    assert!(refused.unwrap_err().to_string().contains("does not name a Spring Boot version"));
}

#[test]
fn coordinates_that_would_climb_a_path_are_never_asked_for() {
    let transport = Arc::new(Recorded::default());
    let service = service(&transport);
    for (group, artifact) in [("a", "../../secret"), ("a/b", "c"), ("a", "b?x=1"), ("..", "c"), ("a", "..")] {
        let on = Allowed { search: false, updates: true, advisories: true };
        assert!(!service.status(group, artifact, "1", &central(), on).0.pending, "{group}:{artifact}");
    }
    assert_eq!(service.pom(&central(), "a", "b", "../../../secret"), None);
    service.wait_idle(Duration::from_secs(10));
    assert_eq!(transport.urls(), Vec::<String>::new());
    assert!(sources::safe_coordinates(&["org.acme", "core_2.13", "1.0.0-rc+1"]));
}

#[test]
fn every_network_setting_is_declared_off_and_names_its_hosts() {
    let transport = Arc::new(Recorded::default());
    let service = service(&transport);
    let declared = zen_project::inspect::Inspector::settings(&*service.inspector());
    assert_eq!(declared.len(), 3);
    for setting in declared {
        assert!(!setting.default, "{}", setting.key);
        assert!(!setting.network.is_empty(), "{}", setting.key);
    }
}

#[test]
fn an_agent_neither_lists_nor_changes_a_pom_hidden_from_it() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    let hidden = window.root.join("pom.xml");
    let listed = window.run("dependencies.project.list", json!({}));
    let module = listed["modules"][0]["id"].clone();
    assert!(!module.is_null(), "{listed}");
    window.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&["pom.xml".to_owned()]);
    let answer = zen_core::macros::as_agent(&mut window.session, |session| {
        window.registry.dispatch(session, "dependencies.project.list", json!({}))
    })
    .unwrap();
    assert_eq!(answer["modules"], json!([]), "{answer}");
    let person = window.run("dependencies.project.list", json!({}));
    assert_eq!(person["modules"][0]["id"], module, "the person still sees it");
    let change = json!({ "module": module, "group": "a.b", "artifact": "c", "version": "1.1" });
    let refused = zen_core::macros::as_agent(&mut window.session, |session| {
        window.registry.dispatch(session, "dependencies.version.set", change)
    });
    match refused {
        Err(zen_core::DispatchError::Failed { .. }) => {}
        other => panic!("not refused: {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&hidden).unwrap(), SIMPLE);
}

#[test]
fn a_version_change_is_kept_in_local_history() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    let data = tempfile::tempdir().unwrap();
    let history = crate::local_history::LocalHistory::open(data.path().join("history"), Default::default());
    window.session.set_state(history);
    let pom = window.root.join("pom.xml");
    let listed = window.run("dependencies.project.list", json!({}));
    let module = listed["modules"][0]["id"].clone();
    let change = json!({ "module": module, "group": "a.b", "artifact": "c", "version": "1.1" });
    window.run("dependencies.version.set", change);
    assert!(std::fs::read_to_string(&pom).unwrap().contains("<version>1.1</version>"));
    let history = window.run("localHistory.versions.list", json!({ "path": pom }));
    assert!(!history["versions"].as_array().unwrap().is_empty(), "{history}");
}

#[test]
fn a_file_changed_outside_since_the_plan_read_it_is_not_overwritten() {
    let mut window = window(&[("pom.xml", SIMPLE)]);
    let pom = window.root.join("pom.xml");
    std::fs::write(&pom, "<project>changed outside</project>\n").unwrap();
    let change =
        FixWrite { path: pom.clone(), text: "<project>planned</project>\n".to_owned(), read: Some(SIMPLE.to_owned()) };
    assert!(write_changes(&mut window.session, std::slice::from_ref(&change)).is_err());
    assert_eq!(std::fs::read_to_string(&pom).unwrap(), "<project>changed outside</project>\n");
    let current = FixWrite { read: Some("<project>changed outside</project>\n".to_owned()), ..change };
    write_changes(&mut window.session, &[current]).unwrap();
    assert_eq!(std::fs::read_to_string(&pom).unwrap(), "<project>planned</project>\n");
}
