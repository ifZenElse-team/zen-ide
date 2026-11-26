use serde_json::{Value, json};

const KOTLIN_VERSION: &str = "2.4.21";

const KOTLIN_NEWEST_TARGET: u64 = 26;

const FALLBACK_RELEASE: u64 = 21;

const FIRST_RELEASE_FLAG: u64 = 9;

const PROJECT_VERSION: &str = "1.0-SNAPSHOT";

const DEFAULT_GROUP: &str = "org.example";

const RESERVED: [&str; 67] = [
    "abstract",
    "as",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "fun",
    "goto",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "int",
    "interface",
    "is",
    "long",
    "native",
    "new",
    "null",
    "object",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "void",
    "volatile",
    "when",
    "while",
    "_",
    "exports",
    "module",
    "requires",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Java,
    Kotlin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Build {
    Ide,
    Maven,
    GradleGroovy,
    GradleKotlin,
}

struct Project {
    language: Language,
    build: Build,
    group: String,
    artifact: String,
    release: u64,
    sample: bool,
    compact: bool,
    module: bool,
}

fn text<'a>(values: &'a Value, key: &str) -> &'a str {
    values[key].as_str().unwrap_or_default()
}

fn safe_id(text: &str) -> String {
    text.trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' })
        .collect()
}

fn package_part(part: &str) -> String {
    let mut name: String = part.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert(0, '_');
    }
    if RESERVED.contains(&name.as_str()) {
        name.push('_');
    }
    name
}

fn package_of(group: &str) -> String {
    let parts: Vec<String> = group.split('.').filter(|part| !part.is_empty()).map(package_part).collect();
    if parts.is_empty() { DEFAULT_GROUP.to_owned() } else { parts.join(".") }
}

impl Project {
    fn read(language: Language, input: &Value) -> Self {
        let values = &input["values"];
        let build = match (text(values, "build"), text(values, "dsl")) {
            ("maven", _) => Build::Maven,
            ("gradle", "groovy") => Build::GradleGroovy,
            ("gradle", _) => Build::GradleKotlin,
            _ => Build::Ide,
        };
        let artifact = Some(text(values, "artifact")).filter(|artifact| !artifact.trim().is_empty());
        Self {
            language,
            build,
            group: safe_id(
                Some(text(values, "group")).filter(|group| !group.trim().is_empty()).unwrap_or(DEFAULT_GROUP),
            ),
            artifact: safe_id(artifact.unwrap_or(input["name"].as_str().unwrap_or_default())),
            release: values["jdkRelease"].as_u64().unwrap_or(FALLBACK_RELEASE),
            sample: values["sample"].as_bool().unwrap_or(false),
            compact: language == Language::Kotlin && values["compact"].as_bool().unwrap_or(false),
            module: values["module"].as_bool().unwrap_or(false),
        }
    }

    fn kotlin_target(&self) -> String {
        match self.release.min(KOTLIN_NEWEST_TARGET) {
            8 => "1.8".to_owned(),
            release => release.to_string(),
        }
    }

    fn source_root(&self) -> &'static str {
        match (self.build, self.language) {
            (Build::Ide, _) => "src",
            (_, Language::Kotlin) if self.compact => "src",
            (_, Language::Java) => "src/main/java",
            (_, Language::Kotlin) => "src/main/kotlin",
        }
    }

    fn package(&self) -> Option<String> {
        (self.build != Build::Ide).then(|| package_of(&self.group))
    }

    fn sample_file(&self) -> (String, String) {
        let package = self.package();
        let folder = package.as_ref().map_or(String::new(), |package| format!("{}/", package.replace('.', "/")));
        let first = package.map_or(String::new(), |package| match self.language {
            Language::Java => format!("package {package};\n\n"),
            Language::Kotlin => format!("package {package}\n\n"),
        });
        let (name, body) = match self.language {
            Language::Java => (
                "Main.java",
                "public class Main {\n    public static void main(String[] args) {\n        System.out.println(\"Hello, World!\");\n    }\n}\n",
            ),
            Language::Kotlin => ("Main.kt", "fun main() {\n    println(\"Hello, World!\")\n}\n"),
        };
        (format!("{}/{folder}{name}", self.source_root()), format!("{first}{body}"))
    }

    fn pom_properties(&self) -> String {
        let release = self.release;
        let level = match self.language {
            Language::Java if release >= FIRST_RELEASE_FLAG => {
                format!("        <maven.compiler.release>{release}</maven.compiler.release>\n")
            }
            Language::Java => format!(
                "        <maven.compiler.source>{release}</maven.compiler.source>\n        <maven.compiler.target>{release}</maven.compiler.target>\n"
            ),
            Language::Kotlin => format!(
                "        <kotlin.version>{KOTLIN_VERSION}</kotlin.version>\n        <kotlin.compiler.jvmTarget>{}</kotlin.compiler.jvmTarget>\n",
                self.kotlin_target()
            ),
        };
        format!(
            "    <properties>\n{level}        <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>\n    </properties>\n"
        )
    }

    fn pom(&self) -> String {
        let kotlin = match self.language {
            Language::Kotlin => KOTLIN_POM.replace(KOTLIN_SOURCES, self.source_root()),
            Language::Java => String::new(),
        };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project xmlns=\"http://maven.apache.org/POM/4.0.0\"\n         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n         xsi:schemaLocation=\"http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd\">\n    <modelVersion>4.0.0</modelVersion>\n\n    <groupId>{}</groupId>\n    <artifactId>{}</artifactId>\n    <version>{PROJECT_VERSION}</version>\n\n{}{kotlin}</project>\n",
            self.group,
            self.artifact,
            self.pom_properties(),
        )
    }

    fn gradle_java_level(&self) -> String {
        let release = self.release;
        match (self.build, release >= FIRST_RELEASE_FLAG) {
            (Build::GradleGroovy, true) => {
                format!("tasks.withType(JavaCompile).configureEach {{\n    options.release = {release}\n}}\n")
            }
            (_, true) => {
                format!("tasks.withType<JavaCompile>().configureEach {{\n    options.release = {release}\n}}\n")
            }
            (_, false) => format!(
                "java {{\n    sourceCompatibility = JavaVersion.toVersion({release})\n    targetCompatibility = JavaVersion.toVersion({release})\n}}\n"
            ),
        }
    }

    fn gradle_kotlin_level(&self) -> String {
        let target = self.kotlin_target();
        let (java, kotlin) = (
            format!("java {{\n    targetCompatibility = JavaVersion.toVersion(\"{target}\")\n}}\n"),
            match self.build {
                Build::GradleGroovy => format!(
                    "kotlin {{\n    compilerOptions {{\n        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.fromTarget('{target}')\n    }}\n}}\n"
                ),
                _ => format!(
                    "kotlin {{\n    compilerOptions {{\n        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.fromTarget(\"{target}\")\n    }}\n}}\n"
                ),
            },
        );
        let sources = match (self.compact, self.build) {
            (false, _) => "",
            (true, Build::GradleGroovy) => "\nsourceSets {\n    main.kotlin.srcDirs = ['src']\n}\n",
            (true, _) => "\nsourceSets.main {\n    kotlin.setSrcDirs(listOf(\"src\"))\n}\n",
        };
        format!("{java}\n{kotlin}{sources}")
    }

    fn gradle_build(&self) -> String {
        let (group, version) = (&self.group, PROJECT_VERSION);
        match (self.build, self.language) {
            (Build::GradleGroovy, Language::Java) => format!(
                "plugins {{\n    id 'java'\n}}\n\ngroup = '{group}'\nversion = '{version}'\n\nrepositories {{\n    mavenCentral()\n}}\n\n{}",
                self.gradle_java_level()
            ),
            (_, Language::Java) => format!(
                "plugins {{\n    java\n}}\n\ngroup = \"{group}\"\nversion = \"{version}\"\n\nrepositories {{\n    mavenCentral()\n}}\n\n{}",
                self.gradle_java_level()
            ),
            (Build::GradleGroovy, Language::Kotlin) => format!(
                "plugins {{\n    id 'org.jetbrains.kotlin.jvm' version '{KOTLIN_VERSION}'\n}}\n\ngroup = '{group}'\nversion = '{version}'\n\nrepositories {{\n    mavenCentral()\n}}\n\n{}",
                self.gradle_kotlin_level()
            ),
            (_, Language::Kotlin) => format!(
                "plugins {{\n    kotlin(\"jvm\") version \"{KOTLIN_VERSION}\"\n}}\n\ngroup = \"{group}\"\nversion = \"{version}\"\n\nrepositories {{\n    mavenCentral()\n}}\n\n{}",
                self.gradle_kotlin_level()
            ),
        }
    }

    fn build_files(&self) -> Vec<(String, String)> {
        let artifact = &self.artifact;
        match self.build {
            Build::Ide => vec![(".gitignore".to_owned(), "out/\n".to_owned())],
            Build::Maven => {
                vec![("pom.xml".to_owned(), self.pom()), (".gitignore".to_owned(), "target/\n".to_owned())]
            }
            Build::GradleGroovy => vec![
                ("build.gradle".to_owned(), self.gradle_build()),
                ("settings.gradle".to_owned(), format!("rootProject.name = '{artifact}'\n")),
                (".gitignore".to_owned(), GRADLE_IGNORE.to_owned()),
            ],
            Build::GradleKotlin => vec![
                ("build.gradle.kts".to_owned(), self.gradle_build()),
                ("settings.gradle.kts".to_owned(), format!("rootProject.name = \"{artifact}\"\n")),
                (".gitignore".to_owned(), GRADLE_IGNORE.to_owned()),
            ],
        }
    }

    fn files(&self) -> Vec<(String, String)> {
        let mut files = self.build_files();
        if self.module {
            files.retain(|(path, _)| !path.starts_with("settings.gradle"));
        }
        files.push(if self.sample {
            self.sample_file()
        } else {
            (format!("{}/.gitkeep", self.source_root()), String::new())
        });
        files
    }
}

const GRADLE_IGNORE: &str = ".gradle/\nbuild/\n";

const KOTLIN_SOURCES: &str = "src/main/kotlin";

const KOTLIN_POM: &str = "    <dependencies>
        <dependency>
            <groupId>org.jetbrains.kotlin</groupId>
            <artifactId>kotlin-stdlib</artifactId>
            <version>${kotlin.version}</version>
        </dependency>
    </dependencies>

    <build>
        <sourceDirectory>src/main/kotlin</sourceDirectory>
        <plugins>
            <plugin>
                <groupId>org.jetbrains.kotlin</groupId>
                <artifactId>kotlin-maven-plugin</artifactId>
                <version>${kotlin.version}</version>
                <executions>
                    <execution>
                        <id>compile</id>
                        <phase>compile</phase>
                        <goals>
                            <goal>compile</goal>
                        </goals>
                    </execution>
                </executions>
            </plugin>
        </plugins>
    </build>
";

pub fn answer(language: Language, input: &[u8]) -> Vec<u8> {
    let Ok(input) = serde_json::from_slice::<Value>(input) else { return Vec::new() };
    let files: Vec<Value> = Project::read(language, &input)
        .files()
        .into_iter()
        .map(|(path, text)| json!({ "path": path, "text": text }))
        .collect();
    json!({ "files": files }).to_string().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(language: Language, name: &str, values: Value) -> Vec<(String, String)> {
        let input = json!({ "step": "files", "name": name, "values": values }).to_string();
        let answer: Value = serde_json::from_slice(&answer(language, input.as_bytes())).unwrap();
        answer["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| (file["path"].as_str().unwrap().to_owned(), file["text"].as_str().unwrap().to_owned()))
            .collect()
    }

    fn file<'a>(files: &'a [(String, String)], path: &str) -> &'a str {
        let found = files.iter().find(|(name, _)| name == path);
        &found.unwrap_or_else(|| panic!("{path} in {files:?}")).1
    }

    fn paths(files: &[(String, String)]) -> Vec<&str> {
        files.iter().map(|(path, _)| path.as_str()).collect()
    }

    fn build(build: &str, dsl: &str, release: u64) -> Value {
        json!({ "build": build, "dsl": dsl, "group": "com.acme", "artifact": "shop", "jdkRelease": release, "sample": true })
    }

    #[test]
    fn a_gradle_module_has_no_settings_of_its_own_as_the_project_s_lists_it() {
        let mut values = build("gradle", "kotlin", 21);
        values["module"] = json!(true);
        let files = files(Language::Java, "shop", values);
        assert!(paths(&files).contains(&"build.gradle.kts"));
        assert!(!paths(&files).contains(&"settings.gradle.kts"));
    }

    #[test]
    fn an_ide_project_has_only_sources_in_src_and_no_package() {
        let files = files(Language::Java, "shop", build("ide", "", 21));
        assert_eq!(paths(&files), [".gitignore", "src/Main.java"]);
        assert!(file(&files, "src/Main.java").starts_with("public class Main"));
    }

    #[test]
    fn a_maven_java_project_compiles_for_the_jdk_s_release_in_the_group_s_package() {
        let files = files(Language::Java, "shop", build("maven", "", 21));
        assert_eq!(paths(&files), ["pom.xml", ".gitignore", "src/main/java/com/acme/Main.java"]);
        let pom = file(&files, "pom.xml");
        assert!(pom.contains("<groupId>com.acme</groupId>\n    <artifactId>shop</artifactId>"), "{pom}");
        assert!(pom.contains("<maven.compiler.release>21</maven.compiler.release>"), "{pom}");
        assert!(file(&files, "src/main/java/com/acme/Main.java").starts_with("package com.acme;\n"));
    }

    #[test]
    fn java_8_has_no_release_flag_in_maven_or_gradle() {
        let maven = files(Language::Java, "shop", build("maven", "", 8));
        assert!(file(&maven, "pom.xml").contains("<maven.compiler.source>8</maven.compiler.source>"));
        assert!(!file(&maven, "pom.xml").contains("maven.compiler.release"));
        let gradle = files(Language::Java, "shop", build("gradle", "groovy", 8));
        assert!(file(&gradle, "build.gradle").contains("sourceCompatibility = JavaVersion.toVersion(8)"));
        assert!(!file(&gradle, "build.gradle").contains("options.release"));
    }

    #[test]
    fn gradle_writes_the_build_in_the_chosen_dsl() {
        let groovy = files(Language::Java, "shop", build("gradle", "groovy", 25));
        assert_eq!(
            paths(&groovy),
            ["build.gradle", "settings.gradle", ".gitignore", "src/main/java/com/acme/Main.java"]
        );
        assert!(file(&groovy, "build.gradle").contains("tasks.withType(JavaCompile)"));
        assert_eq!(file(&groovy, "settings.gradle"), "rootProject.name = 'shop'\n");
        let kotlin = files(Language::Java, "shop", build("gradle", "kotlin", 25));
        assert_eq!(
            paths(&kotlin),
            ["build.gradle.kts", "settings.gradle.kts", ".gitignore", "src/main/java/com/acme/Main.java"]
        );
        assert!(file(&kotlin, "build.gradle.kts").contains("tasks.withType<JavaCompile>()"));
        assert_eq!(file(&kotlin, "settings.gradle.kts"), "rootProject.name = \"shop\"\n");
    }

    #[test]
    fn kotlin_uses_its_gradle_plugin_and_its_source_folder() {
        let files = files(Language::Kotlin, "shop", build("gradle", "kotlin", 21));
        let script = file(&files, "build.gradle.kts");
        assert!(script.contains(&format!("kotlin(\"jvm\") version \"{KOTLIN_VERSION}\"")), "{script}");
        assert!(script.contains("fromTarget(\"21\")"), "{script}");
        assert_eq!(file(&files, "src/main/kotlin/com/acme/Main.kt").lines().next(), Some("package com.acme"));
    }

    #[test]
    fn kotlin_with_maven_compiles_with_its_plugin() {
        let files = files(Language::Kotlin, "shop", build("maven", "", 17));
        let pom = file(&files, "pom.xml");
        assert!(pom.contains("<kotlin.compiler.jvmTarget>17</kotlin.compiler.jvmTarget>"), "{pom}");
        assert!(pom.contains("<artifactId>kotlin-maven-plugin</artifactId>"), "{pom}");
        assert!(pom.contains("<sourceDirectory>src/main/kotlin</sourceDirectory>"), "{pom}");
    }

    #[test]
    fn compact_kotlin_keeps_its_sources_in_src_for_every_build() {
        let compact = |build_tool: &str, dsl: &str| {
            let mut values = build(build_tool, dsl, 21);
            values["compact"] = json!(true);
            files(Language::Kotlin, "shop", values)
        };
        let maven = compact("maven", "");
        assert!(file(&maven, "pom.xml").contains("<sourceDirectory>src</sourceDirectory>"));
        assert!(paths(&maven).contains(&"src/com/acme/Main.kt"));
        let groovy = compact("gradle", "groovy");
        assert!(file(&groovy, "build.gradle").contains("main.kotlin.srcDirs = ['src']"));
        let kotlin = compact("gradle", "kotlin");
        assert!(file(&kotlin, "build.gradle.kts").contains("kotlin.setSrcDirs(listOf(\"src\"))"));
        assert!(paths(&kotlin).contains(&"src/com/acme/Main.kt"));
    }

    #[test]
    fn compact_means_nothing_to_java() {
        let mut values = build("maven", "", 21);
        values["compact"] = json!(true);
        assert!(paths(&files(Language::Java, "shop", values)).contains(&"src/main/java/com/acme/Main.java"));
    }

    #[test]
    fn kotlin_never_targets_a_release_newer_than_its_compiler_knows() {
        let files = files(Language::Kotlin, "shop", build("gradle", "kotlin", KOTLIN_NEWEST_TARGET + 1));
        let target = format!("fromTarget(\"{KOTLIN_NEWEST_TARGET}\")");
        assert!(file(&files, "build.gradle.kts").contains(&target));
    }

    #[test]
    fn kotlin_writes_java_8_as_its_compiler_names_it() {
        let files = files(Language::Kotlin, "shop", build("maven", "", 8));
        assert!(file(&files, "pom.xml").contains("<kotlin.compiler.jvmTarget>1.8</kotlin.compiler.jvmTarget>"));
    }

    #[test]
    fn no_sample_code_keeps_only_the_source_folder() {
        let mut values = build("maven", "", 21);
        values["sample"] = json!(false);
        let files = files(Language::Java, "shop", values);
        assert_eq!(paths(&files), ["pom.xml", ".gitignore", "src/main/java/.gitkeep"]);
    }

    #[test]
    fn a_project_name_a_build_refuses_becomes_a_valid_artifact() {
        let values = json!({ "build": "maven", "group": "org.example", "jdkRelease": 21 });
        let files = files(Language::Java, "my shop", values);
        assert!(file(&files, "pom.xml").contains("<artifactId>my-shop</artifactId>"));
    }

    #[test]
    fn text_in_a_group_or_artifact_cannot_break_out_of_the_build_file() {
        let values = json!({ "build": "maven", "group": "a</groupId><x>", "artifact": "b'\"", "jdkRelease": 21 });
        let files = files(Language::Java, "shop", values);
        let pom = file(&files, "pom.xml");
        assert!(pom.contains("<groupId>a--groupId--x-</groupId>"), "{pom}");
        assert!(pom.contains("<artifactId>b--</artifactId>"), "{pom}");
    }

    #[test]
    fn a_group_that_is_no_package_name_gives_a_valid_package() {
        assert_eq!(package_of("com.acme-shop.1st.class"), "com.acme_shop._1st.class_");
        assert_eq!(package_of("..."), DEFAULT_GROUP);
    }

    #[test]
    fn input_that_is_not_json_is_no_answer() {
        assert!(answer(Language::Java, b"nonsense").is_empty());
    }
}
