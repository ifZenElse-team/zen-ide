use serde_json::{Value, json};

const INTERNAL: &str = "internal";

const ARCHETYPE_GROUP: &str = "org.apache.maven.archetypes";

const INTERNAL_ARCHETYPES: [&str; 9] = [
    "maven-archetype-archetype",
    "maven-archetype-j2ee-simple",
    "maven-archetype-plugin",
    "maven-archetype-plugin-site",
    "maven-archetype-quickstart",
    "maven-archetype-simple",
    "maven-archetype-site",
    "maven-archetype-site-simple",
    "maven-archetype-webapp",
];

const INTERNAL_VERSIONS: [&str; 2] = ["1.5", "1.4"];

const MAX_ARCHETYPES: usize = 1000;

const MAX_VERSIONS: usize = 8;

const PROJECT_VERSION: &str = "1.0-SNAPSHOT";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Archetype {
    group: String,
    artifact: String,
    versions: Vec<String>,
    repository: String,
}

impl Archetype {
    fn id(&self) -> String {
        format!("{}:{}", self.group, self.artifact)
    }
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

fn rows<'a>(input: &'a Value, list: &str) -> &'a [Value] {
    input["values"]["lists"][list].as_array().map_or(&[], Vec::as_slice)
}

fn in_language(english: &str, russian: &str) -> Value {
    json!({ "en": english, "ru": russian })
}

fn says<'a>(input: &Value, english: &'a str, russian: &'a str) -> &'a str {
    if str_of(input, "language") == "ru" { russian } else { english }
}

fn internal() -> Vec<Archetype> {
    INTERNAL_ARCHETYPES
        .iter()
        .map(|artifact| Archetype {
            group: ARCHETYPE_GROUP.to_owned(),
            artifact: (*artifact).to_owned(),
            versions: INTERNAL_VERSIONS.iter().map(|version| (*version).to_owned()).collect(),
            repository: String::new(),
        })
        .collect()
}

fn tag_value<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(block[start..end].trim())
}

fn add_archetype(list: &mut Vec<Archetype>, added: Archetype) {
    if let Some(known) = list.iter_mut().find(|known| known.group == added.group && known.artifact == added.artifact) {
        for version in added.versions {
            if !known.versions.contains(&version) && known.versions.len() < MAX_VERSIONS {
                known.versions.insert(0, version);
            }
        }
    } else if list.len() < MAX_ARCHETYPES {
        list.push(added);
    }
}

fn parse_catalog(text: &str) -> Vec<Archetype> {
    let mut list = Vec::new();
    for block in text.split("<archetype>").skip(1) {
        let block = block.split("</archetype>").next().unwrap_or_default();
        let (Some(group), Some(artifact), Some(version)) =
            (tag_value(block, "groupId"), tag_value(block, "artifactId"), tag_value(block, "version"))
        else {
            continue;
        };
        add_archetype(
            &mut list,
            Archetype {
                group: group.to_owned(),
                artifact: artifact.to_owned(),
                versions: vec![version.to_owned()],
                repository: tag_value(block, "repository").unwrap_or_default().to_owned(),
            },
        );
    }
    list
}

fn added_archetypes(input: &Value) -> Vec<Archetype> {
    rows(input, "archetypes")
        .iter()
        .filter(|row| !str_of(row, "groupId").is_empty() && !str_of(row, "artifactId").is_empty())
        .map(|row| Archetype {
            group: str_of(row, "groupId").to_owned(),
            artifact: str_of(row, "artifactId").to_owned(),
            versions: vec![str_of(row, "version").to_owned()],
            repository: str_of(row, "repository").to_owned(),
        })
        .collect()
}

fn catalog_url(input: &Value, catalog: &str) -> Option<String> {
    rows(input, "catalogs").iter().map(|row| str_of(row, "url")).find(|url| *url == catalog).map(str::to_owned)
}

enum Catalog {
    Read(Vec<Archetype>),
    Fetch(String),
    Failed(String),
}

fn read_catalog(input: &Value) -> Catalog {
    let catalog = str_of(&input["values"], "catalog");
    let Some(url) = catalog_url(input, catalog) else { return Catalog::Read(internal()) };
    let fetched = &input["fetched"]["catalog"];
    if fetched.is_null() {
        return Catalog::Fetch(url);
    }
    if let Some(error) = fetched["error"].as_str() {
        return Catalog::Failed(format!("{url}: {error}"));
    }
    let status = fetched["status"].as_u64().unwrap_or_default();
    if status != 200 {
        return Catalog::Failed(format!("{url}: HTTP {status}"));
    }
    Catalog::Read(parse_catalog(str_of(fetched, "text")))
}

fn option(value: &str, label: &str) -> Value {
    json!({ "value": value, "label": label })
}

fn form_fields(input: &Value, archetypes: &[Archetype]) -> Value {
    let catalogs: Vec<Value> = std::iter::once(option(INTERNAL, says(input, "Internal", "Встроенный")))
        .chain(rows(input, "catalogs").iter().map(|row| option(str_of(row, "url"), str_of(row, "name"))))
        .collect();
    let chosen: Vec<Value> = std::iter::once(option("", ""))
        .chain(archetypes.iter().map(|archetype| option(&archetype.id(), &archetype.id())))
        .collect();
    let versions: Vec<Value> = archetypes
        .iter()
        .flat_map(|archetype| {
            archetype.versions.iter().map(|version| {
                json!({
                    "value": version,
                    "label": version,
                    "enabledWhen": { "field": "archetype", "anyOf": [archetype.id()] },
                })
            })
        })
        .collect();
    json!([
        { "id": "about", "label": "", "type": "note",
          "before": in_language("To create a general Maven project, go to the ", "Чтобы создать обычный проект Maven, перейдите на страницу "),
          "link": { "label": "Java", "generator": "java.java" },
          "after": in_language(" page.", ".") },
        { "id": "jdk", "label": "JDK:", "type": "jdk", "required": true },
        { "id": "catalog", "label": in_language("Catalog:", "Каталог:"), "type": "choice", "default": INTERNAL, "options": catalogs,
          "manage": { "id": "catalogs",
                      "label": in_language("Manage catalogs…", "Управление каталогами…"),
                      "title": in_language("Maven Archetype Catalogs", "Каталоги Maven Archetype"),
                      "columns": [
                          { "id": "name", "label": in_language("Name", "Название") },
                          { "id": "url", "label": "URL" }] } },
        { "id": "archetype", "label": in_language("Archetype:", "Архетип:"), "type": "choice", "required": true, "options": chosen,
          "manage": { "id": "archetypes", "addOne": true,
                      "label": in_language("Add…", "Добавить…"),
                      "title": in_language("Add Archetype", "Добавить архетип"),
                      "value": "${groupId}:${artifactId}",
                      "columns": [
                          { "id": "groupId", "label": "GroupId" },
                          { "id": "artifactId", "label": "ArtifactId" },
                          { "id": "version", "label": in_language("Version", "Версия") },
                          { "id": "repository", "label": in_language("Repository", "Репозиторий") }] } },
        { "id": "version", "label": in_language("Version:", "Версия:"), "type": "choice", "required": true, "options": versions },
        { "id": "properties", "label": in_language("Additional Properties", "Дополнительные свойства"), "type": "table",
          "empty": in_language("No properties", "Нет свойств"),
          "columns": [
              { "id": "name", "label": in_language("Name", "Имя") },
              { "id": "value", "label": in_language("Value", "Значение") }] },
        { "id": "group", "label": "GroupId:", "type": "text", "default": "org.example", "required": true, "advanced": true },
        { "id": "artifact", "label": "ArtifactId:", "type": "text", "default": "${name}", "required": true, "advanced": true },
    ])
}

fn form(input: &Value) -> Value {
    match read_catalog(input) {
        Catalog::Fetch(url) => json!({ "fetch": [{ "id": "catalog", "url": url }] }),
        Catalog::Failed(error) => json!({ "error": error }),
        Catalog::Read(mut archetypes) => {
            for added in added_archetypes(input) {
                add_archetype(&mut archetypes, added);
            }
            json!({ "form": { "fields": form_fields(input, &archetypes) } })
        }
    }
}

fn is_coordinate(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with(['-', '.'])
        && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn known_repository(input: &Value, id: &str) -> String {
    let matches = |archetype: &&Archetype| archetype.id() == id;
    added_archetypes(input).iter().find(matches).map(|archetype| archetype.repository.clone()).unwrap_or_default()
}

fn generate(input: &Value) -> Value {
    let values = &input["values"];
    let archetype = str_of(values, "archetype");
    let Some((archetype_group, archetype_artifact)) = archetype.split_once(':') else {
        return json!({ "error": says(input, "Choose an archetype.", "Выберите архетип.") });
    };
    let (group, artifact, version) = (str_of(values, "group"), str_of(values, "artifact"), str_of(values, "version"));
    if ![group, artifact, archetype_group, archetype_artifact, version].into_iter().all(is_coordinate) {
        return json!({ "error": says(
            input,
            "GroupId, ArtifactId and the archetype's coordinates can hold only letters, digits, dots, hyphens and underscores.",
            "GroupId, ArtifactId и координаты архетипа могут содержать только буквы, цифры, точки, дефисы и подчёркивания.",
        ) });
    }
    let mut args: Vec<String> = vec![
        "-B".to_owned(),
        "archetype:generate".to_owned(),
        "-DinteractiveMode=false".to_owned(),
        format!("-DgroupId={group}"),
        format!("-DartifactId={artifact}"),
        format!("-Dversion={PROJECT_VERSION}"),
        format!("-DarchetypeGroupId={archetype_group}"),
        format!("-DarchetypeArtifactId={archetype_artifact}"),
        format!("-DarchetypeVersion={version}"),
    ];
    let repository = known_repository(input, archetype);
    if !repository.is_empty() {
        args.push(format!("-DarchetypeRepository={repository}"));
    }
    for property in values["properties"].as_array().map_or(&[][..], Vec::as_slice) {
        let name = str_of(property, "name");
        if name.is_empty() {
            continue;
        }
        if !is_coordinate(name) {
            return json!({ "error": says(input, "A property's name is not valid.", "Недопустимое имя свойства.") });
        }
        args.push(format!("-D{name}={}", str_of(property, "value")));
    }
    let java = values["jdk"].as_str();
    json!({ "run": { "tool": "maven", "args": args, "javaHome": java, "subfolder": artifact } })
}

#[derive(Debug)]
struct Element {
    name: String,
    open_end: usize,
    close_start: usize,
    end: usize,
    line_start: usize,
}

fn skip_to(text: &str, from: usize, ending: &str) -> Option<usize> {
    text[from..].find(ending).map(|index| from + index + ending.len())
}

fn project_children(text: &str) -> Option<(usize, Vec<Element>)> {
    let mut depth = 0_usize;
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    let mut children = Vec::new();
    let mut at = 0;
    let mut project_end = None;
    while let Some(found) = text[at..].find('<') {
        let start = at + found;
        let rest = &text[start..];
        if rest.starts_with("<!--") {
            at = skip_to(text, start, "-->")?;
        } else if rest.starts_with("<![CDATA[") {
            at = skip_to(text, start, "]]>")?;
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            at = skip_to(text, start, ">")?;
        } else if rest.starts_with("</") {
            let end = skip_to(text, start, ">")?;
            let (name, open_end, open_start) = stack.pop()?;
            depth -= 1;
            if depth == 1 {
                let line_start = text[..open_start].rfind('\n').map_or(0, |index| index + 1);
                children.push(Element { name, open_end, close_start: start, end, line_start });
            } else if depth == 0 {
                project_end = Some(start);
            }
            at = end;
        } else {
            let end = skip_to(text, start, ">")?;
            let inner = &text[start + 1..end - 1];
            let name: String = inner.chars().take_while(|c| !c.is_whitespace() && *c != '/').collect();
            if inner.ends_with('/') {
                if depth == 1 {
                    let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
                    children.push(Element { name, open_end: end, close_start: end, end, line_start });
                }
            } else {
                stack.push((name, end, start));
                depth += 1;
            }
            at = end;
        }
    }
    Some((project_end?, children))
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn indent_of(text: &str, element: &Element) -> String {
    text[element.line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

fn lists_module(text: &str, modules: &Element, path: &str) -> bool {
    text[modules.open_end..modules.close_start]
        .split("<module>")
        .skip(1)
        .filter_map(|part| part.split("</module>").next())
        .map(|listed| listed.trim().trim_end_matches('/').trim_end_matches("/pom.xml"))
        .any(|listed| listed == path || listed == escaped(path))
}

fn add_module_entry(text: &str, path: &str) -> Result<Option<String>, ()> {
    let (project_end, children) = project_children(text).ok_or(())?;
    let find = |name: &str| children.iter().find(|child| child.name == name);
    let indent = children.first().map_or_else(|| "    ".to_owned(), |child| indent_of(text, child));
    let entry = format!("<module>{}</module>", escaped(path));
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut inserted = String::new();
    if find("packaging").is_none() {
        inserted.push_str(&format!("\n{indent}<packaging>pom</packaging>"));
    }
    if let Some(modules) = find("modules") {
        if lists_module(text, modules, path) {
            return Ok(None);
        }
        let closing = text[..modules.close_start].rfind('\n').map_or(modules.close_start, |index| index + 1);
        if closing <= modules.open_end {
            let line = format!("\n{indent}{indent}{entry}\n{indent}");
            edits.push((modules.close_start, modules.close_start, line));
        } else {
            edits.push((closing, closing, format!("{indent}{indent}{entry}\n")));
        }
    } else {
        inserted.push_str(&format!("\n\n{indent}<modules>\n{indent}{indent}{entry}\n{indent}</modules>"));
    }
    if let Some(packaging) = find("packaging")
        && text[packaging.open_end..packaging.close_start].trim() != "pom"
    {
        edits.push((packaging.open_end, packaging.close_start, "pom".to_owned()));
    }
    if !inserted.is_empty() {
        let after = ["packaging", "version", "artifactId", "groupId", "parent"].into_iter().find_map(find);
        let at = after.map_or(project_end, |element| element.end);
        edits.push((at, at, inserted));
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut edited = text.to_owned();
    for (start, end, replacement) in edits {
        edited.replace_range(start..end, &replacement);
    }
    Ok(Some(edited))
}

fn add_module(input: &Value) -> Value {
    if !input["hasBuild"].as_bool().unwrap_or(false) {
        if input["mustBuild"].as_bool().unwrap_or(false) {
            return json!({ "error": says(input, "The module has no pom.xml.", "В модуле нет pom.xml.") });
        }
        return json!({ "files": [] });
    }
    let (file, path) = (str_of(input, "file"), str_of(input, "path"));
    match add_module_entry(str_of(input, "text"), path) {
        Ok(Some(edited)) => json!({ "files": [{ "path": file, "text": edited }] }),
        Ok(None) => json!({ "files": [] }),
        Err(()) => {
            json!({ "error": says(input, "The parent's pom.xml cannot be read.", "Не удаётся прочитать pom.xml родителя.") })
        }
    }
}

pub fn answer(input: &[u8]) -> Vec<u8> {
    let Ok(input) = serde_json::from_slice::<Value>(input) else { return Vec::new() };
    let answer = match str_of(&input, "step") {
        "form" => form(&input),
        "generate" => generate(&input),
        "addModule" => add_module(&input),
        "files" => json!({ "files": [] }),
        _ => json!({ "error": "unknown step" }),
    };
    answer.to_string().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(input: &Value) -> Value {
        serde_json::from_slice(&answer(input.to_string().as_bytes())).unwrap()
    }

    fn field<'a>(form: &'a Value, id: &str) -> &'a Value {
        form["form"]["fields"].as_array().unwrap().iter().find(|field| field["id"] == id).unwrap()
    }

    #[test]
    fn the_form_has_the_specified_fields_in_their_order() {
        let form = ask(&json!({ "step": "form", "values": {} }));
        let ids: Vec<&str> =
            form["form"]["fields"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["about", "jdk", "catalog", "archetype", "version", "properties", "group", "artifact"]);
        assert_eq!(field(&form, "group")["advanced"], true);
        assert_eq!(field(&form, "catalog")["manage"]["id"], "catalogs");
        assert_eq!(field(&form, "archetype")["manage"]["addOne"], true);
    }

    #[test]
    fn the_internal_catalog_needs_no_network_and_starts_on_no_archetype() {
        let form = ask(&json!({ "step": "form", "values": { "catalog": "internal" } }));
        assert!(form["fetch"].is_null());
        let options = field(&form, "archetype")["options"].as_array().unwrap();
        assert_eq!(options[0]["value"], "");
        assert!(options.iter().any(|o| o["value"] == "org.apache.maven.archetypes:maven-archetype-quickstart"));
    }

    #[test]
    fn a_version_belongs_to_its_archetype() {
        let form = ask(&json!({ "step": "form", "values": {} }));
        let versions = field(&form, "version")["options"].as_array().unwrap();
        assert!(versions.iter().all(|version| version["enabledWhen"]["field"] == "archetype"));
    }

    #[test]
    fn a_catalog_from_the_list_is_fetched_only_when_chosen_and_read() {
        let catalogs = json!({ "catalogs": [{ "name": "Ours", "url": "https://repo.example/archetype-catalog.xml" }] });
        let idle = ask(&json!({ "step": "form", "values": { "lists": catalogs } }));
        assert!(idle["fetch"].is_null());
        assert_eq!(field(&idle, "catalog")["options"][1]["label"], "Ours");
        let chosen = json!({ "lists": catalogs, "catalog": "https://repo.example/archetype-catalog.xml" });
        let fetch = ask(&json!({ "step": "form", "values": chosen }));
        assert_eq!(fetch["fetch"][0]["url"], "https://repo.example/archetype-catalog.xml");
        let text = "<archetype-catalog><archetypes><archetype><groupId>g</groupId><artifactId>a</artifactId><version>1</version></archetype><archetype><groupId>g</groupId><artifactId>a</artifactId><version>2</version></archetype></archetypes></archetype-catalog>";
        let read = ask(&json!({ "step": "form", "values": chosen,
                                "fetched": { "catalog": { "status": 200, "text": text } } }));
        let archetypes = field(&read, "archetype")["options"].as_array().unwrap();
        assert_eq!(archetypes.len(), 2);
        assert_eq!(archetypes[1]["value"], "g:a");
        assert_eq!(field(&read, "version")["options"].as_array().unwrap().len(), 2);
        let failed = ask(&json!({ "step": "form", "values": chosen, "fetched": { "catalog": { "status": 404 } } }));
        assert!(failed["error"].as_str().unwrap().contains("404"));
    }

    #[test]
    fn an_added_archetype_is_offered_and_generated_from_its_repository() {
        let lists = json!({ "archetypes": [{ "groupId": "com.acme", "artifactId": "base", "version": "3",
                                              "repository": "https://repo.acme/maven" }] });
        let form = ask(&json!({ "step": "form", "values": { "lists": lists } }));
        let archetypes = field(&form, "archetype")["options"].as_array().unwrap();
        assert!(archetypes.iter().any(|option| option["value"] == "com.acme:base"));
        let run = ask(&json!({ "step": "generate", "values": { "lists": lists, "archetype": "com.acme:base",
            "version": "3", "group": "org.example", "artifact": "shop" } }));
        let args: Vec<&str> = run["run"]["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert!(args.contains(&"-DarchetypeRepository=https://repo.acme/maven"), "{args:?}");
    }

    #[test]
    fn generating_runs_maven_without_questions_in_the_jdk_chosen() {
        let run = ask(&json!({ "step": "generate", "values": {
            "archetype": "org.apache.maven.archetypes:maven-archetype-quickstart", "version": "1.5",
            "group": "com.acme", "artifact": "shop", "jdk": "/jdk/21",
            "properties": [{ "name": "package", "value": "com.acme.shop" }, { "name": "", "value": "" }] } }));
        let run = &run["run"];
        assert_eq!(run["tool"], "maven");
        assert_eq!(run["javaHome"], "/jdk/21");
        assert_eq!(run["subfolder"], "shop");
        let args: Vec<&str> = run["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert_eq!(&args[..3], ["-B", "archetype:generate", "-DinteractiveMode=false"]);
        for arg in ["-DgroupId=com.acme", "-DartifactId=shop", "-DarchetypeVersion=1.5", "-Dpackage=com.acme.shop"] {
            assert!(args.contains(&arg), "{arg} in {args:?}");
        }
    }

    #[test]
    fn a_value_that_could_leave_the_folder_or_pass_for_an_option_is_refused() {
        for (group, artifact, name) in [
            ("com.acme", "../up", "a"),
            ("-Dx=y", "shop", "a"),
            ("com.acme", "my shop", "a"),
            ("com.acme", "shop", "a=b"),
        ] {
            let answer = ask(&json!({ "step": "generate", "values": { "archetype": "g:a", "version": "1",
                "group": group, "artifact": artifact, "properties": [{ "name": name, "value": "v" }] } }));
            assert!(answer["error"].is_string(), "{group} {artifact} {name}");
        }
        assert!(ask(&json!({ "step": "generate", "values": {} }))["error"].is_string());
    }

    const PARENT: &str = "<?xml version=\"1.0\"?>\n<project>\n    <modelVersion>4.0.0</modelVersion>\n    <groupId>a</groupId>\n    <artifactId>b</artifactId>\n    <version>1</version>\n</project>\n";

    fn added(text: &str, path: &str) -> Value {
        ask(&json!({ "step": "addModule", "file": "pom.xml", "text": text, "path": path, "hasBuild": true }))
    }

    fn edited(text: &str, path: &str) -> String {
        added(text, path)["files"][0]["text"].as_str().unwrap().to_owned()
    }

    #[test]
    fn a_parent_without_modules_gets_them_and_pom_packaging() {
        let edited = edited(PARENT, "api");
        assert_eq!(
            edited,
            "<?xml version=\"1.0\"?>\n<project>\n    <modelVersion>4.0.0</modelVersion>\n    <groupId>a</groupId>\n    <artifactId>b</artifactId>\n    <version>1</version>\n    <packaging>pom</packaging>\n\n    <modules>\n        <module>api</module>\n    </modules>\n</project>\n"
        );
    }

    #[test]
    fn a_jar_parent_becomes_pom() {
        let text =
            PARENT.replace("    <version>1</version>\n", "    <version>1</version>\n    <packaging>jar</packaging>\n");
        let edited = edited(&text, "api");
        assert!(edited.contains("<packaging>pom</packaging>"), "{edited}");
        assert!(!edited.contains("jar"), "{edited}");
        assert!(edited.contains("<module>api</module>"), "{edited}");
    }

    #[test]
    fn a_new_module_goes_last_in_the_project_s_own_modules_not_a_profile_s() {
        let text = "<project>\n  <artifactId>b</artifactId>\n  <packaging>pom</packaging>\n  <profiles>\n    <profile>\n      <modules>\n        <module>x</module>\n      </modules>\n    </profile>\n  </profiles>\n  <modules>\n    <module>core</module>\n  </modules>\n</project>\n";
        let edited = edited(text, "api");
        assert!(
            edited.contains("    <module>core</module>\n    <module>api</module>\n  </modules>\n</project>"),
            "{edited}"
        );
        assert!(edited.contains("<module>x</module>\n      </modules>"), "{edited}");
    }

    #[test]
    fn a_listed_module_changes_nothing_also_by_its_pom() {
        for listed in ["api", "api/", "api/pom.xml"] {
            let text = format!(
                "<project>\n  <packaging>pom</packaging>\n  <modules>\n    <module>{listed}</module>\n  </modules>\n</project>\n"
            );
            assert_eq!(added(&text, "api")["files"], json!([]), "{listed}");
        }
    }

    #[test]
    fn a_commented_out_modules_block_does_not_count() {
        let text =
            "<project>\n  <version>1</version>\n  <!-- <modules><module>api</module></modules> -->\n</project>\n";
        let edited = edited(text, "api");
        assert!(edited.contains("<modules>\n    <module>api</module>"), "{edited}");
    }

    #[test]
    fn a_new_module_of_another_build_system_is_left_out_of_the_parent() {
        let without = json!({ "step": "addModule", "file": "pom.xml", "text": PARENT, "path": "a", "hasBuild": false });
        assert_eq!(ask(&without)["files"], json!([]));
    }

    #[test]
    fn an_empty_modules_element_takes_the_module() {
        let text = "<project>\n  <packaging>pom</packaging>\n  <modules></modules>\n</project>\n";
        assert!(edited(text, "api").contains("<modules>\n    <module>api</module>\n  </modules>"));
    }

    #[test]
    fn a_folder_without_a_pom_cannot_be_a_module_and_a_broken_parent_is_refused() {
        let without = json!({ "step": "addModule", "file": "pom.xml", "text": PARENT, "path": "a", "hasBuild": false, "mustBuild": true });
        assert!(ask(&without)["error"].is_string());
        assert!(added("<project><a>", "api")["error"].is_string());
    }
}
