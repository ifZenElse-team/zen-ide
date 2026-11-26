use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;
use zen_core::{Registry, Session};

use super::*;
use crate::ide_settings::zip::ZipWriter;
use crate::java::class_file::tests::minimal;

const POM: &str = r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>demo</groupId><artifactId>shop</artifactId><version>1.0</version>
  <dependencies>
    <dependency><groupId>org.demo</groupId><artifactId>lib</artifactId><version>1.0</version></dependency>
    <dependency><groupId>org.demo</groupId><artifactId>managed</artifactId></dependency>
    <dependency><groupId>org.gone</groupId><artifactId>gone</artifactId><version>9</version></dependency>
  </dependencies>
</project>"#;

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(0);
    for (name, data) in files {
        writer.add(name, data).unwrap();
    }
    writer.finish()
}

struct Setup {
    dir: tempfile::TempDir,
    root: PathBuf,
    env: BuildEnv,
    places: Places,
    jdk: PathBuf,
}

impl Setup {
    fn new(build_file: (&str, &str)) -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("shop");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(build_file.0), build_file.1).unwrap();
        let home = dir.path().join("home");
        let jdk = dir.path().join("jdk");
        std::fs::create_dir_all(jdk.join("lib")).unwrap();
        std::fs::write(jdk.join("release"), "JAVA_VERSION=\"21.0.5\"\n").unwrap();
        let source = b"package java.lang;\npublic final class String {}\n";
        std::fs::write(jdk.join("lib/src.zip"), zip(&[("java.base/java/lang/String.java", source)])).unwrap();
        let env = BuildEnv::default();
        crate::trust::trust_in_tests(&env, &root);
        env.set_home(Some(home.clone()));
        let found = jdk.clone();
        env.set_jdk(Some(Arc::new(move |_: &Path| Some(found.clone()))));
        let places =
            Places { maven: maven_repository(&home), gradle: gradle_files(&home), cache: dir.path().join("cache") };
        Self { dir, root, env, places, jdk }
    }

    fn maven_jar(&self, artifact: &str, version: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let folder = self.places.maven.join("org/demo").join(artifact).join(version);
        std::fs::create_dir_all(&folder).unwrap();
        let jar = folder.join(format!("{artifact}-{version}.jar"));
        std::fs::write(&jar, zip(files)).unwrap();
        jar
    }

    fn provider(&self) -> JvmLibraries {
        JvmLibraries::with_places(self.env.clone(), self.places.clone(), vec![Box::new(MavenTree), Box::new(Declared)])
    }

    fn unpacked(&self) -> Vec<Library> {
        let (_, work) = self.provider().libraries(&self.root);
        for (archive, root, origin, placeholders) in work {
            std::fs::create_dir_all(root.parent().unwrap()).unwrap();
            unpack(&archive, &root, &origin, placeholders).unwrap();
        }
        self.provider().libraries(&self.root).0
    }
}

fn class_jar() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("org/demo/Box.class", minimal("org/demo/Box", "Box.java")),
        ("org/demo/Box$Item.class", minimal("org/demo/Box$Item", "Box.java")),
        ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n".to_vec()),
    ]
}

fn refs<'a>(files: &'a [(&'static str, Vec<u8>)]) -> Vec<(&'static str, &'a [u8])> {
    files.iter().map(|(name, data)| (*name, data.as_slice())).collect()
}

#[test]
fn external_libraries_list_the_jdk_and_every_maven_library_with_its_jar_contents() {
    let setup = Setup::new(("pom.xml", POM));
    let files = class_jar();
    let jar = setup.maven_jar("lib", "1.0", &refs(&files));
    setup.maven_jar("managed", "2.1", &[]);
    setup.maven_jar("managed", "2.10", &[]);
    let (libraries, work) = setup.provider().libraries(&setup.root);
    let labels: Vec<&str> = libraries.iter().filter_map(|library| library.label.as_deref()).collect();
    assert_eq!(
        labels,
        ["< 21.0.5 >", "Maven: org.demo:lib:1.0", "Maven: org.demo:managed:2.10", "Maven: org.gone:gone:9"]
    );
    assert_eq!(libraries[0].kind, LibraryKind::Sdk);
    assert!(libraries[1..].iter().all(|library| library.group.as_deref() == Some("")), "right under the node");
    assert!(libraries.iter().all(|library| !library.exists), "nothing is unpacked yet");
    assert!(libraries[3].root.as_os_str().is_empty(), "a library not downloaded has no folder");
    assert_eq!(work.len(), 3, "the JDK's sources and the two jars there");

    let libraries = setup.unpacked();
    assert!(libraries[..3].iter().all(|library| library.exists));
    let jdk = &libraries[0].root;
    assert_eq!(std::fs::read_to_string(jdk.join("java.base/java/lang/String.java")).unwrap().lines().count(), 2);
    let lib = &libraries[1].root;
    assert!(lib.starts_with(&setup.places.cache));
    assert_eq!(std::fs::read(lib.join("org/demo/Box.class")).unwrap(), b"", "a class is a placeholder");
    assert_eq!(std::fs::read_to_string(lib.join("META-INF/MANIFEST.MF")).unwrap(), "Manifest-Version: 1.0\n");
    assert_eq!(coordinates_of(&jar, &setup.places), Some(Coordinates::new("org.demo", "lib", "1.0")));
    let found = library_of(&lib.join("org/demo/Box.class")).unwrap();
    assert_eq!((found.0.as_path(), found.1), (jar.as_path(), Some(Coordinates::new("org.demo", "lib", "1.0"))));
    assert_eq!(library_of(lib).map(|(jar, _)| jar).as_deref(), Some(jar.as_path()), "the library's own row");
    let managed = &libraries[2].root;
    assert_ne!(origin_file(lib), origin_file(managed));
    assert!(origin_file(managed).ends_with(format!("{}.origin.json", managed.file_name().unwrap().to_string_lossy())));
    let _ = &setup.jdk;
}

#[test]
fn a_library_class_opens_as_its_sources_when_there_are_some_else_decompiled_and_read_only() {
    let setup = Setup::new(("pom.xml", POM));
    let files = class_jar();
    let jar = setup.maven_jar("lib", "1.0", &refs(&files));
    let libraries = setup.unpacked();
    let class = libraries[1].root.join("org/demo/Box$Item.class");
    let cache = setup.dir.path().join("documents");

    let decompiled = opened_as(&class, &cache).unwrap();
    assert!(decompiled.ends_with("org/demo/Box$Item.java"), "{}", decompiled.display());
    let text = std::fs::read_to_string(&decompiled).unwrap();
    assert!(text.contains("package org.demo;") && text.contains("public class Item {"), "{text}");
    assert!(text.contains("public void run() { /* compiled code */ }"), "{text}");
    let document = virtual_documents::get(&decompiled).unwrap();
    assert_eq!(document.kind, VirtualKind::Decompiled);
    assert_eq!(document.library.as_deref(), Some(jar.as_path()));
    assert_eq!(document.bytecode, Some(65));
    assert!(virtual_documents::reason(&decompiled).is_some_and(|reason| reason.contains("decompiled")));

    let sources = jar.with_file_name("lib-1.0-sources.jar");
    let source = b"package org.demo;\n\npublic class Box {\n    public static class Item {}\n}\n";
    std::fs::write(&sources, zip(&[("org/demo/Box.java", source)])).unwrap();
    let opened = opened_as(&class, &cache).unwrap();
    assert!(opened.ends_with("org/demo/Box.java"), "{}", opened.display());
    assert_eq!(std::fs::read(&opened).unwrap(), source);
    assert_eq!(virtual_documents::get(&opened).unwrap().kind, VirtualKind::Sources);

    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": setup.root })).unwrap();
    std::fs::remove_file(&sources).unwrap();
    let plain = setup.dir.path().join("Plain.class");
    std::fs::write(&plain, minimal("Plain", "Plain.java")).unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": plain })).unwrap();
    let workbench = session.state::<crate::Workbench>().unwrap();
    let active = workbench.active_document().unwrap();
    assert!(active.path().extension().is_some_and(|extension| extension == "java"));
    assert!(active.text().contains("public class Plain {"), "any class file opens decompiled");
    assert!(active.read_only().is_some() && !active.is_locked());
    let info = registry.dispatch(&mut session, "java.libraries.document", json!({})).unwrap();
    assert_eq!(info["kind"], "decompiled");
    assert_eq!(info["java"], 21);
    assert!(opened_as(&setup.root.join("pom.xml"), &cache).is_none());
    let text_class = setup.dir.path().join("Fake.class");
    std::fs::write(&text_class, "not a class").unwrap();
    assert!(opened_as(&text_class, &cache).is_none(), "a file that is not a class opens as it is");
}

#[test]
fn gradle_builds_name_their_libraries_in_the_gradle_cache_and_find_sources_beside_them() {
    let build = "dependencies {\n    implementation 'com.google.guava:guava:33.0.0-jre'\n    testImplementation(\"org.junit.jupiter:junit-jupiter:5.10.2\")\n    id 'java'\n    implementation \"x:y:${version}\"\n}\n";
    let found = gradle_coordinates(build);
    let texts: Vec<String> = found.iter().map(Coordinates::text).collect();
    assert_eq!(texts, ["com.google.guava:guava:33.0.0-jre", "org.junit.jupiter:junit-jupiter:5.10.2"]);
    let setup = Setup::new(("build.gradle", build));
    let version = setup.places.gradle.join("com.google.guava/guava/33.0.0-jre");
    let (classes, sources) = (version.join("1111"), version.join("2222"));
    std::fs::create_dir_all(&classes).unwrap();
    std::fs::create_dir_all(&sources).unwrap();
    let jar = classes.join("guava-33.0.0-jre.jar");
    std::fs::write(&jar, zip(&[])).unwrap();
    std::fs::write(sources.join("guava-33.0.0-jre-sources.jar"), zip(&[])).unwrap();
    let guava = Coordinates::new("com.google.guava", "guava", "33.0.0-jre");
    assert_eq!(jar_of(&setup.places, &guava, ""), Some(jar.clone()));
    assert_eq!(sources_of(&jar), Some(sources.join("guava-33.0.0-jre-sources.jar")));
    assert_eq!(coordinates_of(&jar, &setup.places), Some(guava));
    let libraries = setup.unpacked();
    let labels: Vec<&str> = libraries.iter().filter_map(|library| library.label.as_deref()).collect();
    assert_eq!(
        labels,
        ["< 21.0.5 >", "Gradle: com.google.guava:guava:33.0.0-jre", "Gradle: org.junit.jupiter:junit-jupiter:5.10.2"]
    );
    assert!(libraries[1].exists && !libraries[2].exists);
}

fn wait_loaded(env: &BuildEnv, key: &str) -> crate::build_tools::Load {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let load = env.load(key).unwrap();
        if load.state != LoadState::Loading || Instant::now() > deadline {
            return load;
        }
        std::thread::yield_now();
    }
}

#[cfg(unix)]
fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_loaded_maven_tree_lists_every_library_and_sources_download_through_maven_on_request() {
    let setup = Setup::new(("pom.xml", POM));
    let tree = "demo:shop:jar:1.0\n+- org.demo:lib:jar:1.0:compile\n|  \\- org.demo:deep:jar:0.1:compile\n\\- org.demo:bom:pom:1:import\n";
    let line = zen_run::CommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), format!("printf '{}'", tree.replace('\\', "\\\\"))],
        cwd: setup.root.clone(),
        env: Vec::new(),
        display: String::new(),
    };
    let key = crate::maven::tree_key(&setup.root);
    let before = setup.provider().generation(&setup.root);
    setup.env.spawn(&key, line, None, |output| Ok(output.to_owned()));
    assert_eq!(wait_loaded(&setup.env, &key).state, LoadState::Loaded);
    assert_ne!(setup.provider().generation(&setup.root), before, "a loaded tree reads the list again");
    let (libraries, _) = setup.provider().libraries(&setup.root);
    let labels: Vec<&str> = libraries.iter().filter_map(|library| library.label.as_deref()).collect();
    assert_eq!(labels, ["< 21.0.5 >", "Maven: org.demo:lib:1.0", "Maven: org.demo:deep:0.1"], "the tree, not the POM");

    let bin = setup.dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin.join("mvn"), "echo \"mvn $*\"");
    setup.env.set_folders(Some(vec![bin]));
    let files = class_jar();
    setup.maven_jar("lib", "1.0", &refs(&files));
    let libraries = setup.unpacked();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(setup.env.clone());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": setup.root })).unwrap();
    let class = libraries[1].root.join("org/demo/Box.class");
    let started = registry.dispatch(&mut session, "java.libraries.downloadSources", json!({ "path": class })).unwrap();
    assert_eq!(started["tool"], "maven");
    assert_eq!(started["coordinates"], "org.demo:lib:1.0");
    let load = wait_loaded(&setup.env, &sources_key(&setup.root));
    assert_eq!(load.state, LoadState::Loaded, "{load:?}");
    assert!(
        load.output.contains("dependency:sources -B -DincludeGroupIds=org.demo -DincludeArtifactIds=lib"),
        "{}",
        load.output
    );
    let refused = registry.dispatch(
        &mut session,
        "java.libraries.downloadSources",
        json!({ "path": setup.root.join("pom.xml") }),
    );
    assert!(refused.is_err());
}

#[cfg(unix)]
#[test]
fn a_reloaded_gradle_build_lists_every_resolved_library_with_its_jar() {
    let setup = Setup::new(("build.gradle", "dependencies { implementation 'com.google.guava:guava:33.0.0-jre' }\n"));
    let jar = setup.dir.path().join("cache-of-gradle/guava-33.0.0-jre.jar");
    std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
    std::fs::write(&jar, zip(&[])).unwrap();
    let output = format!(
        "zen-library com.google.guava:guava:33.0.0-jre\t{}\nzen-library com.google.guava:failureaccess:1.0.2\t/nowhere/failureaccess-1.0.2.jar\nnoise\n",
        jar.display()
    );
    let found = gradle_libraries(&output);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].jar.as_deref(), Some(jar.as_path()));
    let bin = setup.dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let printed = output.replace('\t', "\\t").replace('\n', "\\n");
    script(&bin.join("gradle"), &format!("case \"$*\" in *zenLibraries*) printf '{printed}' ;; esac"));
    setup.env.set_folders(Some(vec![bin]));
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(setup.env.clone());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": setup.root })).unwrap();
    let before = setup.provider().generation(&setup.root);
    load_gradle_libraries(&mut session, &setup.env, &setup.root).unwrap();
    let load = wait_loaded(&setup.env, &gradle_key(&setup.root));
    assert_eq!(load.state, LoadState::Loaded, "{load:?}");
    assert_ne!(setup.provider().generation(&setup.root), before);
    let provider = JvmLibraries::with_places(
        setup.env.clone(),
        setup.places.clone(),
        vec![Box::new(MavenTree), Box::new(GradleResolved), Box::new(Declared)],
    );
    let (libraries, work) = provider.libraries(&setup.root);
    let labels: Vec<&str> = libraries.iter().filter_map(|library| library.label.as_deref()).collect();
    assert_eq!(
        labels,
        ["< 21.0.5 >", "Gradle: com.google.guava:guava:33.0.0-jre", "Gradle: com.google.guava:failureaccess:1.0.2"]
    );
    assert!(work.iter().any(|(archive, ..)| archive == &jar), "the jar Gradle resolved is unpacked");
}

#[test]
fn nobody_downloads_sources_through_a_build_not_trusted() {
    let setup = Setup::new(("pom.xml", POM));
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(setup.env.clone());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": setup.root })).unwrap();
    registry.dispatch(&mut session, "project.trust.revoke", json!({})).unwrap();
    match registry.dispatch(&mut session, "java.libraries.downloadSources", json!({})) {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{error:?}")
        }
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn a_cache_whose_owner_has_gone_is_not_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("gone");
    let cache = owner.join("cache");
    let root = cache.join("jdk").join("jdk-21");
    let archive = dir.path().join("src.zip");
    std::fs::write(&archive, zip(&[("a.txt", b"a")])).unwrap();
    unpack_later(vec![(archive, root.clone(), String::new(), false)], &cache, &Unpacks::default());
    let deadline = Instant::now() + Duration::from_secs(30);
    while is_unpacking(&root) {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!owner.exists());
}

#[test]
fn the_last_handle_stops_unpacking_and_waits_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let root = cache.join("jdk").join("jdk-21");
    let names: Vec<String> = (0..3000).map(|at| format!("java.base/{at}.java")).collect();
    let files: Vec<(&str, &[u8])> = names.iter().map(|name| (name.as_str(), b"class A {}".as_slice())).collect();
    let archive = dir.path().join("src.zip");
    std::fs::write(&archive, zip(&files)).unwrap();
    let unpacks = Unpacks::default();
    unpack_later(vec![(archive, root.clone(), String::new(), false)], &cache, &unpacks);
    drop(unpacks);
    assert!(!is_unpacking(&root));
    let left: Vec<String> = std::fs::read_dir(root.parent().unwrap())
        .map(|found| found.filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned())).collect())
        .unwrap_or_default();
    assert!(!left.iter().any(|name| name.contains(".part-")), "{left:?}");
}
