use std::cmp::Ordering;

use serde_json::{Map, Value, json};

const GENERATOR: &str = "initializr";
const METADATA: &str = "metadata";
const METADATA_TYPE: &str = "application/vnd.initializr.v2.3+json";
const COORDINATES: &str = "coordinates";
const COORDINATES_TYPE: &str = "application/vnd.initializr.v2.1+json";
const EDIT_STEP: &str = "starters";
const SERVICE_URL: &str = "serviceUrl";
const VERSIONS: &str = "bootVersion";
const STARTERS: &str = "dependencies";
const PROJECT_NAME: &str = "name";

const JDK: &str = "jdk";

const FIELD_ORDER: [&str; 9] = [
    "language",
    "type",
    "groupId",
    "artifactId",
    "packageName",
    JDK,
    "javaVersion",
    "packaging",
    "configurationFileFormat",
];

const LEFT_TO_THE_SERVICE: [&str; 2] = ["version", "description"];

const SEGMENTED: [&str; 4] = ["language", "type", "packaging", "configurationFileFormat"];

fn label(id: &str) -> Value {
    let (en, ru) = match id {
        "type" => ("Type:", "Тип:"),
        "language" => ("Language:", "Язык:"),
        VERSIONS => ("Spring Boot:", "Spring Boot:"),
        "groupId" => ("Group:", "Группа:"),
        "artifactId" => ("Artifact:", "Артефакт:"),
        "packageName" => ("Package name:", "Имя пакета:"),
        "packaging" => ("Packaging:", "Упаковка:"),
        "javaVersion" => ("Java:", "Java:"),
        "configurationFileFormat" => ("Configuration:", "Конфигурация:"),
        STARTERS => ("Dependencies:", "Зависимости:"),
        other => return json!(words(other)),
    };
    json!({ "en": en, "ru": ru })
}

fn words(id: &str) -> String {
    let mut text = String::new();
    for (index, letter) in id.chars().enumerate() {
        match index {
            0 => text.extend(letter.to_uppercase()),
            _ if letter.is_uppercase() => {
                text.push(' ');
                text.extend(letter.to_lowercase());
            }
            _ => text.push(letter),
        }
    }
    text
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Other,
    Milestone,
    Candidate,
    Snapshot,
    Release,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    numbers: [u64; 3],
    stage: Stage,
    build: u64,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let (major, rest) = text.trim().split_once('.')?;
        let (minor, rest) = rest.split_once('.')?;
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        let (patch, tail) = rest.split_at(digits);
        let tail = tail.trim_start_matches(['.', '-']);
        let digits = tail.find(|c: char| c.is_ascii_digit()).unwrap_or(tail.len());
        let (name, build) = tail.split_at(digits);
        let stage = match name.to_ascii_uppercase().as_str() {
            "" | "RELEASE" => Stage::Release,
            "SNAPSHOT" | "BUILD-SNAPSHOT" => Stage::Snapshot,
            "RC" | "CR" => Stage::Candidate,
            "M" => Stage::Milestone,
            _ => Stage::Other,
        };
        Some(Self {
            numbers: [major.parse().ok()?, minor.parse().ok()?, patch.parse().ok()?],
            stage,
            build: build.parse().unwrap_or(0),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Range {
    low: Version,
    low_inclusive: bool,
    high: Option<(Version, bool)>,
}

impl Range {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let open = text.chars().next()?;
        if open != '[' && open != '(' {
            return Some(Self { low: Version::parse(text)?, low_inclusive: true, high: None });
        }
        let close = text.chars().last()?;
        let (low, high) = text[1..text.len() - 1].split_once(',')?;
        Some(Self {
            low: Version::parse(low)?,
            low_inclusive: open == '[',
            high: Some((Version::parse(high)?, close == ']')),
        })
    }

    fn contains(&self, version: &Version) -> bool {
        let above = match version.cmp(&self.low) {
            Ordering::Greater => true,
            Ordering::Equal => self.low_inclusive,
            Ordering::Less => false,
        };
        above
            && self.high.as_ref().is_none_or(|(high, inclusive)| match version.cmp(high) {
                Ordering::Less => true,
                Ordering::Equal => *inclusive,
                Ordering::Greater => false,
            })
    }
}

fn range_note(text: &str) -> Value {
    let text = text.trim();
    let bounds = text.strip_prefix(['[', '(']).and_then(|inner| inner.strip_suffix([']', ')']));
    let Some((low, high)) = bounds.and_then(|bounds| bounds.split_once(',')) else {
        return json!({ "en": format!("Requires Spring Boot >={text}"), "ru": format!("Нужен Spring Boot >={text}") });
    };
    let above = if text.starts_with('[') { ">=" } else { ">" };
    let below = if text.ends_with(']') { "<=" } else { "<" };
    let (low, high) = (low.trim(), high.trim());
    json!({
        "en": format!("Requires Spring Boot {above}{low} and {below}{high}"),
        "ru": format!("Нужен Spring Boot {above}{low} и {below}{high}"),
    })
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

fn options(field: &Value) -> impl Iterator<Item = &Value> {
    field.get("values").and_then(Value::as_array).into_iter().flatten()
}

fn plain_option(value: &Value) -> Option<Value> {
    let mut option = Map::new();
    option.insert("value".to_owned(), json!(text_of(value, "id")?));
    option.insert("label".to_owned(), json!(text_of(value, "name").or(text_of(value, "id"))?));
    if let Some(description) = text_of(value, "description") {
        option.insert("description".to_owned(), json!(description));
    }
    Some(Value::Object(option))
}

fn first_link(link: Option<&Value>) -> Option<&str> {
    let link = link?;
    match link {
        Value::Array(links) => first_link(links.first()),
        _ => text_of(link, "href"),
    }
}

fn starter_url(starter: &Value, boot: &str) -> Option<String> {
    let links = starter.get("_links")?;
    if let Some(guide) = first_link(links.get("guide")) {
        return Some(guide.to_owned());
    }
    first_link(links.get("reference")).map(|href| href.replace("{bootVersion}", boot))
}

fn starter(value: &Value, versions: &[(String, Option<Version>)], boot: &str) -> Option<Value> {
    let mut option = plain_option(value)?;
    let fields = option.as_object_mut()?;
    if let Some(url) = starter_url(value, boot) {
        fields.insert("url".to_owned(), json!(url));
    }
    let wanted = text_of(value, "versionRange");
    if let Some((text, range)) = wanted.and_then(|text| Some((text, Range::parse(text)?))) {
        let fits = |version: &Option<Version>| version.as_ref().is_none_or(|version| range.contains(version));
        let fitting: Vec<&str> = versions.iter().filter(|(_, v)| fits(v)).map(|(id, _)| id.as_str()).collect();
        if fitting.len() < versions.len() {
            fields.insert("enabledWhen".to_owned(), json!({ "field": VERSIONS, "anyOf": fitting }));
            fields.insert("disabledNote".to_owned(), range_note(text));
        }
    }
    Some(option)
}

fn starters(id: &str, field: &Value, metadata: &Value) -> Value {
    let versions: Vec<(String, Option<Version>)> = metadata
        .get(VERSIONS)
        .map(|versions| options(versions).filter_map(|v| text_of(v, "id")))
        .into_iter()
        .flatten()
        .map(|id| (id.to_owned(), Version::parse(id)))
        .collect();
    let boot = metadata.get(VERSIONS).and_then(|field| text_of(field, "default")).unwrap_or_default();
    select_field(id, field, &versions, boot)
}

fn select_field(id: &str, field: &Value, versions: &[(String, Option<Version>)], boot: &str) -> Value {
    let groups: Vec<Value> = options(field)
        .map(|group| {
            let items = options(group).filter_map(|item| starter(item, versions, boot)).collect::<Vec<_>>();
            json!({ "name": text_of(group, "name").unwrap_or_default(), "options": items })
        })
        .collect();
    json!({ "id": id, "label": label(id), "type": "select", "groups": groups, "page": 1 })
}

fn choice(id: &str, field: &Value) -> Value {
    let project = |option: &&Value| {
        option.get("tags").and_then(|tags| text_of(tags, "format")).is_none_or(|format| format == "project")
    };
    let items: Vec<Value> = options(field).filter(project).filter_map(plain_option).collect();
    let mut answer = json!({ "id": id, "label": label(id), "type": "choice", "options": items });
    if let Some(default) = text_of(field, "default") {
        answer["default"] = json!(default);
    }
    if SEGMENTED.contains(&id) {
        answer["segmented"] = json!(true);
    }
    if id == VERSIONS {
        answer["page"] = json!(1);
    }
    answer
}

fn text_default(id: &str, metadata: &Value) -> String {
    let given = |field: &str| metadata.get(field).and_then(|field| text_of(field, "default")).unwrap_or_default();
    let (default, group, artifact) = (given(id), given("groupId"), given("artifactId"));
    match id {
        "artifactId" if !default.is_empty() => "${name}".to_owned(),
        "packageName" if !group.is_empty() && default == format!("{group}.{artifact}") => {
            "${groupId}.${artifactId}".to_owned()
        }
        _ => default.to_owned(),
    }
}

fn field(id: &str, metadata: &Value) -> Option<Value> {
    if id == JDK {
        return Some(json!({ "id": JDK, "label": "JDK:", "type": "jdk" }));
    }
    let given = metadata.get(id)?;
    match text_of(given, "type")? {
        "text" => {
            let default = text_default(id, metadata);
            let required = !default.is_empty();
            Some(json!({ "id": id, "label": label(id), "type": "text", "default": default, "required": required }))
        }
        "single-select" | "action" => Some(choice(id, given)),
        "hierarchical-multi-select" => Some(starters(id, given, metadata)),
        _ => None,
    }
}

fn form(metadata: &Value, service: &str) -> Value {
    let known = |id: &&str| *id == JDK || metadata.get(*id).is_some();
    let mut ids: Vec<&str> = FIELD_ORDER.iter().copied().filter(known).collect();
    let others = metadata.as_object().into_iter().flat_map(Map::keys).map(String::as_str);
    let mut rest: Vec<&str> = others
        .filter(|id| {
            !id.starts_with('_')
                && *id != PROJECT_NAME
                && *id != STARTERS
                && *id != VERSIONS
                && !FIELD_ORDER.contains(id)
                && !LEFT_TO_THE_SERVICE.contains(id)
        })
        .collect();
    rest.sort_unstable();
    ids.extend(rest);
    ids.push(VERSIONS);
    ids.push(STARTERS);
    let fields: Vec<Value> = ids.into_iter().filter_map(|id| field(id, metadata)).collect();
    let server = json!({ "label": { "en": "Server URL:", "ru": "Адрес сервера:" }, "url": service });
    json!({ "form": { "fields": fields, "server": server } })
}

fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn query_value(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(text) => encoded(text),
        Value::Bool(on) => on.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(items) => items.iter().filter_map(Value::as_str).map(encoded).collect::<Vec<_>>().join(","),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

fn expanded(template: &str, values: &Map<String, Value>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(length) = rest[start..].find('}') else { break };
        let expression = &rest[start + 1..start + length];
        rest = &rest[start + length + 1..];
        let (first, names) = match expression.chars().next() {
            Some('?') => ('?', &expression[1..]),
            Some('&') => ('&', &expression[1..]),
            _ => continue,
        };
        let pairs = names
            .split(',')
            .filter_map(|name| Some(format!("{name}={}", query_value(values.get(name.trim())?)?)))
            .collect::<Vec<_>>();
        if !pairs.is_empty() {
            out.push(first);
            out.push_str(&pairs.join("&"));
        }
    }
    out.push_str(rest);
    out
}

fn say(language: &str, en: String, ru: String) -> Value {
    json!({ "error": if language == "ru" { ru } else { en } })
}

fn authority(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn on_service(url: &str, service: &str) -> bool {
    !url.contains('\\')
        && authority(url).zip(authority(service)).is_some_and(|(url, service)| url.eq_ignore_ascii_case(service))
}

fn elsewhere(url: &str, service: &str, language: &str) -> Value {
    say(
        language,
        format!("{service} sends this project to another address, which is refused: {url}"),
        format!("{service} отправляет за проектом на другой адрес, это запрещено: {url}"),
    )
}

fn fetch_metadata(service: &str) -> Value {
    json!({ "fetch": [{
        "id": METADATA,
        "url": service,
        "headers": { "Accept": METADATA_TYPE },
        "cache": true,
    }] })
}

fn archive(metadata: &Value, input: &Value, service: &str, language: &str) -> Value {
    let chosen = input.get("values").and_then(Value::as_object).cloned().unwrap_or_default();
    let kind = chosen.get("type").and_then(Value::as_str).unwrap_or_default();
    let Some(template) =
        metadata.get("_links").and_then(|links| links.get(kind)).and_then(|link| text_of(link, "href"))
    else {
        return say(
            language,
            format!("The service gives no address for the type {kind}."),
            format!("Сервис не даёт адрес для типа {kind}."),
        );
    };
    let mut values = chosen;
    if let Some(name) = input.get(PROJECT_NAME) {
        values.insert(PROJECT_NAME.to_owned(), name.clone());
    }
    let url = expanded(template, &values);
    if !on_service(&url, service) {
        return elsewhere(&url, service, language);
    }
    json!({ "archive": { "url": url } })
}

fn fetch_coordinates(metadata: &Value, service: &str, boot: &str, language: &str) -> Value {
    let chosen = Map::from_iter([(VERSIONS.to_owned(), json!(boot))]);
    let link = metadata.get("_links").and_then(|links| links.get(STARTERS)).and_then(|link| text_of(link, "href"));
    let url = link.map_or_else(
        || format!("{service}/dependencies?{VERSIONS}={}", encoded(boot)),
        |template| expanded(template, &chosen),
    );
    if !on_service(&url, service) {
        return elsewhere(&url, service, language);
    }
    json!({ "fetch": [{
        "id": COORDINATES,
        "url": url,
        "headers": { "Accept": COORDINATES_TYPE },
        "cache": true,
    }] })
}

fn refused_version(fetched: &Value, boot: &str, service: &str, language: &str) -> Value {
    let message = serde_json::from_str::<Value>(text_of(fetched, "text").unwrap_or_default())
        .ok()
        .and_then(|answer| text_of(&answer, "message").map(str::to_owned))
        .unwrap_or_default();
    say(
        language,
        format!("{service} does not know the starters of Spring Boot {boot}. {message}"),
        format!("{service} не знает стартеры Spring Boot {boot}. {message}"),
    )
}

fn ticked(coordinates: &Map<String, Value>, declared: &[&str]) -> Vec<String> {
    let name = |entry: &Value| {
        format!(
            "{}:{}",
            text_of(entry, "groupId").unwrap_or_default(),
            text_of(entry, "artifactId").unwrap_or_default()
        )
    };
    coordinates.iter().filter(|(_, entry)| declared.contains(&name(entry).as_str())).map(|(id, _)| id.clone()).collect()
}

fn starters_of_project(metadata: &Value, input: &Value, service: &str, language: &str) -> Value {
    let values = input.get("values");
    let boot = values.and_then(|values| text_of(values, VERSIONS)).unwrap_or_default();
    let Some(fetched) = input.get("fetched").and_then(|fetched| fetched.get(COORDINATES)) else {
        return fetch_coordinates(metadata, service, boot, language);
    };
    if let Some(error) = text_of(fetched, "error") {
        return needs_network(service, error, language);
    }
    if fetched.get("status").and_then(Value::as_u64) != Some(200) {
        return refused_version(fetched, boot, service, language);
    }
    let parsed = serde_json::from_str::<Value>(text_of(fetched, "text").unwrap_or_default());
    let Ok(answer) = parsed else { return refused_version(fetched, boot, service, language) };
    let empty = Map::new();
    let coordinates = answer.get(STARTERS).and_then(Value::as_object).unwrap_or(&empty);
    let declared: Vec<&str> = values
        .and_then(|values| values.get("declared"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let versions = [(boot.to_owned(), Version::parse(boot))];
    let field = metadata.get(STARTERS).map_or(Value::Null, |field| select_field(STARTERS, field, &versions, boot));
    json!({ "starters": {
        "field": field,
        "ticked": ticked(coordinates, &declared),
        STARTERS: coordinates,
        "boms": answer.get("boms"),
        "repositories": answer.get("repositories"),
    } })
}

fn needs_network(service: &str, error: &str, language: &str) -> Value {
    say(
        language,
        format!("Editing starters needs the network, and {service} cannot be used: {error}"),
        format!("Для изменения стартеров нужна сеть, а {service} недоступен: {error}"),
    )
}

pub(crate) fn answer(input: &str) -> String {
    let answer = match serde_json::from_str::<Value>(input) {
        Ok(input) => answer_to(&input),
        Err(error) => json!({ "error": format!("the input is invalid: {error}") }),
    };
    answer.to_string()
}

fn answer_to(input: &Value) -> Value {
    let language = text_of(input, "language").unwrap_or("en");
    if text_of(input, "generator") != Some(GENERATOR) {
        return say(language, "Unknown generator.".to_owned(), "Неизвестный генератор.".to_owned());
    }
    let service = input.get("settings").and_then(|settings| text_of(settings, SERVICE_URL)).unwrap_or_default();
    let service = service.trim().trim_end_matches('/');
    let Some(fetched) = input.get("fetched").and_then(|fetched| fetched.get(METADATA)) else {
        return fetch_metadata(service);
    };
    let editing = text_of(input, "step") == Some(EDIT_STEP);
    if let Some(error) = text_of(fetched, "error").filter(|_| editing) {
        return needs_network(service, error, language);
    }
    if let Some(error) = text_of(fetched, "error") {
        return say(
            language,
            format!("Cannot reach {service}: {error}"),
            format!("Не удаётся связаться с {service}: {error}"),
        );
    }
    let status = fetched.get("status").and_then(Value::as_u64).unwrap_or(0);
    let parsed = serde_json::from_str::<Value>(text_of(fetched, "text").unwrap_or_default());
    let Some(metadata) = parsed.ok().filter(|metadata| status == 200 && metadata.is_object()) else {
        return say(
            language,
            format!("{service} gave no project metadata (status {status})."),
            format!("{service} не дал описания проектов (статус {status})."),
        );
    };
    match text_of(input, "step") {
        Some("form") => form(&metadata, service),
        Some(EDIT_STEP) => starters_of_project(&metadata, input, service, language),
        _ => archive(&metadata, input, service, language),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const STUB: &str = include_str!("../testdata/metadataWithV23AcceptHeader.json");

    fn stub() -> Value {
        let mapping: Value = serde_json::from_str(STUB).unwrap();
        let body = mapping["response"]["body"].as_str().unwrap();
        serde_json::from_str(&body.replace("http://start.example.com", "https://start.spring.io")).unwrap()
    }

    fn input(step: &str, values: Value) -> Value {
        json!({
            "step": step, "generator": "initializr", "language": "en", "name": "shop", "values": values,
            "settings": { "serviceUrl": "https://start.spring.io/" },
            "fetched": { "metadata": { "status": 200, "text": stub().to_string() } },
        })
    }

    fn the_form() -> Value {
        answer_to(&input("form", json!({})))["form"].clone()
    }

    fn field_of<'a>(form: &'a Value, id: &str) -> &'a Value {
        form["fields"].as_array().unwrap().iter().find(|field| field["id"] == id).unwrap()
    }

    fn option_of<'a>(field: &'a Value, id: &str) -> &'a Value {
        let groups = field["groups"].as_array().unwrap();
        groups.iter().flat_map(|group| group["options"].as_array().unwrap()).find(|o| o["value"] == id).unwrap()
    }

    #[test]
    fn the_metadata_is_asked_of_the_set_service_with_its_version_and_the_cache() {
        let mut asked = input("form", json!({}));
        asked["fetched"] = json!({});
        let wanted = answer_to(&asked);
        assert_eq!(wanted["fetch"][0]["url"], "https://start.spring.io");
        assert_eq!(wanted["fetch"][0]["headers"]["Accept"], "application/vnd.initializr.v2.3+json");
        assert_eq!(wanted["fetch"][0]["cache"], true);
    }

    #[test]
    fn every_field_and_default_of_the_stub_is_in_the_form() {
        let metadata = stub();
        let form = the_form();
        let shown = |id: &&String| !matches!(id.as_str(), "_links" | "name" | "version" | "description");
        for id in metadata.as_object().unwrap().keys().filter(shown) {
            let field = field_of(&form, id);
            if let Some(default) = metadata[id]["default"].as_str()
                && !matches!(id.as_str(), "artifactId" | "packageName")
            {
                assert_eq!(field["default"], default, "{id}");
            }
        }
        assert_eq!(field_of(&form, "javaVersion")["options"].as_array().unwrap().len(), 3);
        assert_eq!(field_of(&form, "bootVersion")["default"], "2.4.4");
    }

    #[test]
    fn a_field_the_form_does_not_know_still_comes_through() {
        let mut metadata = stub();
        metadata["jvmVendor"] = json!({ "type": "text", "default": "any" });
        let form = form(&metadata, "https://start.spring.io");
        let field = field_of(&form["form"], "jvmVendor");
        assert_eq!(field["label"], "Jvm vendor");
        assert_eq!(field["default"], "any");
    }

    #[test]
    fn the_first_page_has_the_specified_order_and_the_second_the_version_and_the_starters() {
        let mut metadata = stub();
        metadata["configurationFileFormat"] = json!({ "type": "single-select", "default": "properties", "values": [
            { "id": "properties", "name": "Properties" }, { "id": "yaml", "name": "YAML" }] });
        let form = form(&metadata, "https://start.spring.io");
        let fields = form["form"]["fields"].as_array().unwrap();
        let ids: Vec<&str> = fields.iter().map(|field| field["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            [
                "language",
                "type",
                "groupId",
                "artifactId",
                "packageName",
                "jdk",
                "javaVersion",
                "packaging",
                "configurationFileFormat",
                "bootVersion",
                "dependencies",
            ]
        );
        let on_page = |page: u64| fields.iter().filter(|field| field["page"].as_u64().unwrap_or(0) == page).count();
        assert_eq!((on_page(0), on_page(1)), (9, 2));
        for id in ["language", "type", "packaging", "configurationFileFormat"] {
            assert_eq!(field_of(&form["form"], id)["segmented"], true, "{id}");
        }
        assert_eq!(field_of(&form["form"], "jdk")["type"], "jdk");
        assert_eq!(form["form"]["server"]["url"], "https://start.spring.io");
    }

    #[test]
    fn only_types_that_make_a_project_are_offered() {
        let form = the_form();
        let types: Vec<&str> = field_of(&form, "type")["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["value"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["maven-project", "gradle-project"]);
    }

    #[test]
    fn the_names_follow_the_project_and_the_group() {
        let form = the_form();
        assert_eq!(field_of(&form, "artifactId")["default"], "${name}");
        assert_eq!(field_of(&form, "packageName")["default"], "${groupId}.${artifactId}");
        assert!(form["fields"].as_array().unwrap().iter().all(|field| field["id"] != "name"));
    }

    #[test]
    fn starters_keep_their_groups_descriptions_and_guides() {
        let form = the_form();
        let starters = field_of(&form, "dependencies");
        assert_eq!(starters["type"], "select");
        assert_eq!(starters["groups"][0]["name"], "Core");
        let web = option_of(starters, "web");
        assert_eq!(web["description"], "Web dependency description");
        assert_eq!(web["url"], "https://example.com/guide");
        assert_eq!(option_of(starters, "org.acme:foo")["url"], "https://example.com/guide1");
    }

    #[test]
    fn a_starter_outside_a_boot_version_is_disabled_with_the_range() {
        let form = the_form();
        let starter = option_of(field_of(&form, "dependencies"), "org.acme:bur");
        assert_eq!(starter["enabledWhen"], json!({ "field": "bootVersion", "anyOf": ["2.4.4"] }));
        assert!(starter["disabledNote"]["en"].as_str().unwrap().contains(">=2.4.4"));
        assert!(starter["disabledNote"]["en"].as_str().unwrap().contains("<2.5.0-SNAPSHOT"));
        assert!(starter["disabledNote"]["ru"].as_str().unwrap().contains(">=2.4.4"));
        assert!(option_of(field_of(&form, "dependencies"), "web").get("enabledWhen").is_none());
    }

    #[test]
    fn versions_order_as_the_service_orders_them() {
        let v = |text: &str| Version::parse(text).unwrap();
        assert!(v("4.1.0-M3") < v("4.1.0-RC1"));
        assert!(v("4.1.0-RC1") < v("4.1.0-SNAPSHOT"));
        assert!(v("4.1.0-SNAPSHOT") < v("4.1.0"));
        assert!(v("4.1.0-M1") < v("4.1.0-M3"));
        assert_eq!(v("2.3.10.RELEASE").cmp(&v("2.3.10")), Ordering::Equal);
        assert!(v("2.3.10.RELEASE") < v("2.4.4"));
        assert!(v("4.0.10-SNAPSHOT") < v("4.1.0-M1"));
    }

    #[test]
    fn ranges_read_as_the_service_writes_them() {
        let range = Range::parse("[4.0.0,4.2.0-M1)").unwrap();
        assert!(range.contains(&Version::parse("4.0.0").unwrap()));
        assert!(range.contains(&Version::parse("4.1.3-SNAPSHOT").unwrap()));
        assert!(!range.contains(&Version::parse("4.2.0-M1").unwrap()));
        assert!(!range.contains(&Version::parse("4.2.0-RC1").unwrap()));
        assert!(!range.contains(&Version::parse("4.0.0-SNAPSHOT").unwrap()));
        let open = Range::parse("4.1.0-M3").unwrap();
        assert!(open.contains(&Version::parse("4.2.0-SNAPSHOT").unwrap()));
        assert!(!open.contains(&Version::parse("4.0.9").unwrap()));
        assert_eq!(range_note("[4.0.0,4.2.0-M1)")["en"], "Requires Spring Boot >=4.0.0 and <4.2.0-M1");
        assert_eq!(range_note("4.1.0-M3")["ru"], "Нужен Spring Boot >=4.1.0-M3");
    }

    #[test]
    fn the_archive_and_the_starters_come_only_from_the_host_the_form_shows() {
        for (url, on) in [
            ("https://start.spring.io/starter.zip", true),
            ("https://START.spring.io/x?a=b", true),
            ("https://start.spring.io:8443/x", false),
            ("https://start.spring.io@evil.example/x", false),
            ("https://start.spring.io.evil.example/x", false),
            ("https://evil.example/?https://start.spring.io/", false),
            ("https://start.spring.io\\@evil.example/", false),
            ("http://start.spring.io/x", false),
        ] {
            assert_eq!(on_service(url, "https://start.spring.io"), on, "{url}");
        }
        let elsewhere = |answer: &Value| answer["error"].as_str().is_some_and(|text| text.contains("evil.example"));
        let mut metadata = stub();
        metadata["_links"]["maven-project"]["href"] = json!("https://evil.example/starter.zip{?type}");
        let chosen = json!({ "values": { "type": "maven-project" } });
        let refused = archive(&metadata, &chosen, "https://start.spring.io", "en");
        assert!(elsewhere(&refused) && refused.get("archive").is_none(), "{refused}");
        metadata["_links"][STARTERS]["href"] = json!("https://evil.example/dependencies{?bootVersion}");
        let refused = fetch_coordinates(&metadata, "https://start.spring.io", "3.0.0", "en");
        assert!(elsewhere(&refused) && refused.get("fetch").is_none(), "{refused}");
    }

    #[test]
    fn the_archive_address_comes_from_the_type_s_link_with_every_value() {
        let values = json!({
            "type": "maven-project", "language": "java", "bootVersion": "2.4.4", "groupId": "org.acme",
            "artifactId": "shop", "description": "A shop & more", "dependencies": ["web", "org.acme:foo"],
            "packaging": "", "unasked": "x",
        });
        let answer = answer_to(&input("files", values));
        let url = answer["archive"]["url"].as_str().unwrap();
        assert!(
            url.starts_with("https://start.spring.io/starter.zip?type=maven-project&dependencies=web,org.acme%3Afoo")
        );
        assert!(url.contains("&name=shop"));
        assert!(url.contains("&description=A%20shop%20%26%20more"));
        assert!(!url.contains("packaging"), "an empty value stays out");
        assert!(!url.contains("unasked"));
    }

    #[test]
    fn a_failure_of_the_service_is_said_in_the_person_s_language() {
        let mut asked = input("form", json!({}));
        asked["language"] = json!("ru");
        asked["fetched"]["metadata"] = json!({ "error": "timed out" });
        assert!(answer_to(&asked)["error"].as_str().unwrap().starts_with("Не удаётся"));
        asked["language"] = json!("en");
        asked["fetched"]["metadata"] = json!({ "status": 404, "text": "not here" });
        assert!(answer_to(&asked)["error"].as_str().unwrap().contains("404"));
    }

    #[test]
    fn answers_are_json_with_an_error_for_bad_input() {
        assert!(serde_json::from_str::<Value>(&answer("{")).unwrap()["error"].is_string());
        let unknown = json!({ "generator": "other", "language": "en" });
        assert!(serde_json::from_str::<Value>(&answer(&unknown.to_string())).unwrap()["error"].is_string());
    }

    const COORDINATES_SAMPLE: &str = r#"{
      "bootVersion": "2.4.4",
      "dependencies": {
        "web": { "groupId": "org.springframework.boot", "artifactId": "spring-boot-starter-web", "scope": "compile" },
        "org.acme:foo": { "groupId": "org.acme", "artifactId": "foo", "scope": "compile", "bom": "acme" }
      },
      "boms": { "acme": { "groupId": "org.acme", "artifactId": "acme-bom", "version": "1.0" } },
      "repositories": {}
    }"#;

    fn editing(extra: Value) -> Value {
        let mut input = input("starters", json!({ "bootVersion": "2.3.10.RELEASE", "declared": ["org.acme:foo"] }));
        for (id, answer) in extra.as_object().unwrap() {
            input["fetched"][id] = answer.clone();
        }
        input
    }

    #[test]
    fn editing_asks_the_service_for_the_coordinates_of_the_project_s_version() {
        let asked = answer_to(&editing(json!({})));
        let fetch = &asked["fetch"][0];
        assert_eq!(fetch["id"], COORDINATES);
        assert_eq!(fetch["url"], "https://start.spring.io/dependencies?bootVersion=2.3.10.RELEASE");
        assert_eq!(fetch["cache"], true);
        assert_eq!(fetch["headers"]["Accept"], COORDINATES_TYPE);
    }

    #[test]
    fn the_chooser_ticks_what_the_build_declares_and_disables_what_the_version_cannot_take() {
        let done = answer_to(&editing(json!({ "coordinates": { "status": 200, "text": COORDINATES_SAMPLE } })));
        let starters = &done["starters"];
        assert_eq!(starters["ticked"], json!(["org.acme:foo"]));
        assert_eq!(starters["boms"]["acme"]["artifactId"], "acme-bom");
        assert_eq!(starters["dependencies"]["web"]["artifactId"], "spring-boot-starter-web");
        let biz = option_of(&starters["field"], "org.acme:biz");
        assert_eq!(biz["enabledWhen"], json!({ "field": VERSIONS, "anyOf": [] }));
        assert_eq!(biz["disabledNote"]["en"], "Requires Spring Boot >=2.6.0-SNAPSHOT");
        assert!(option_of(&starters["field"], "web").get("enabledWhen").is_none());
    }

    #[test]
    fn a_version_the_service_refuses_says_so_with_the_service_s_words() {
        let mapping: Value = serde_json::from_str(include_str!(
            "../testdata/getDependenciesMetadataWithUnsupportedPlatformVersion.json"
        ))
        .unwrap();
        let refused = json!({ "status": mapping["response"]["status"], "text": mapping["response"]["body"] });
        let done = answer_to(&editing(json!({ "coordinates": refused })));
        let error = done["error"].as_str().unwrap();
        assert!(error.contains("Invalid Spring Boot version '1.5.12.RELEASE'"), "{error}");
    }

    #[test]
    fn without_the_network_the_chooser_says_it_needs_it() {
        let done = answer_to(&editing(json!({ "coordinates": { "error": "no route" } })));
        assert!(done["error"].as_str().unwrap().contains("needs the network"), "{done}");
        let mut asked = editing(json!({}));
        asked["fetched"]["metadata"] = json!({ "error": "no route" });
        assert!(answer_to(&asked)["error"].as_str().unwrap().contains("needs the network"));
    }
}
