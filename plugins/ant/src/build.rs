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

const TASKS: &[&str] = &[
    "ant",
    "antcall",
    "antversion",
    "apply",
    "attrib",
    "available",
    "basename",
    "buildnumber",
    "bunzip2",
    "bzip2",
    "checksum",
    "chmod",
    "chown",
    "componentdef",
    "concat",
    "condition",
    "copy",
    "delete",
    "depend",
    "dependset",
    "diagnostics",
    "dirname",
    "ear",
    "echo",
    "echoproperties",
    "echoxml",
    "exec",
    "fail",
    "filter",
    "fixcrlf",
    "genkey",
    "get",
    "gunzip",
    "gzip",
    "hostinfo",
    "import",
    "include",
    "input",
    "jar",
    "java",
    "javac",
    "javadoc",
    "jlink",
    "jmod",
    "junit",
    "junitlauncher",
    "junitreport",
    "length",
    "loadfile",
    "loadproperties",
    "loadresource",
    "local",
    "macrodef",
    "mail",
    "makeurl",
    "manifest",
    "manifestclasspath",
    "mkdir",
    "move",
    "parallel",
    "patch",
    "pathconvert",
    "presetdef",
    "property",
    "propertyfile",
    "record",
    "replace",
    "replaceregexp",
    "resourcecount",
    "retry",
    "rmic",
    "script",
    "scriptdef",
    "sequential",
    "setpermissions",
    "signjar",
    "sleep",
    "sql",
    "subant",
    "symlink",
    "sync",
    "tar",
    "taskdef",
    "tempfile",
    "touch",
    "truncate",
    "tstamp",
    "typedef",
    "unjar",
    "untar",
    "unwar",
    "unzip",
    "uptodate",
    "waitfor",
    "war",
    "whichresource",
    "xmlproperty",
    "xslt",
    "zip",
];

const TYPES: &[&str] = &["path", "fileset", "dirset", "filelist", "patternset", "filterset", "mapper", "resources"];

const TOP_LEVEL: &[&str] = &["target", "extension-point", "description"];

const CHILDREN: &[(&str, &[&str])] = &[
    ("javac", &["src", "classpath", "sourcepath", "include", "exclude", "compilerarg", "modulepath"]),
    ("jar", &["fileset", "zipfileset", "zipgroupfileset", "manifest", "metainf", "service"]),
    ("war", &["fileset", "lib", "classes", "webinf", "metainf", "zipfileset", "manifest"]),
    ("zip", &["fileset", "zipfileset", "zipgroupfileset"]),
    ("tar", &["fileset", "tarfileset"]),
    ("java", &["arg", "jvmarg", "classpath", "sysproperty", "env", "modulepath"]),
    ("exec", &["arg", "env", "redirector"]),
    ("apply", &["arg", "fileset", "srcfile", "targetfile", "mapper"]),
    ("junit", &["classpath", "test", "batchtest", "formatter", "jvmarg", "sysproperty", "env"]),
    ("batchtest", &["fileset"]),
    ("junitreport", &["fileset", "report"]),
    ("javadoc", &["fileset", "packageset", "sourcepath", "classpath", "link", "group", "doctitle"]),
    ("copy", &["fileset", "filterset", "mapper", "filterchain"]),
    ("move", &["fileset", "filterset", "mapper"]),
    ("delete", &["fileset", "dirset", "include", "exclude"]),
    ("sync", &["fileset", "preserveintarget"]),
    ("path", &["pathelement", "fileset", "dirset", "filelist", "path"]),
    ("classpath", &["pathelement", "fileset", "dirset", "filelist", "path"]),
    ("modulepath", &["pathelement", "fileset", "dirset", "path"]),
    ("src", &["pathelement", "dirset"]),
    ("fileset", &["include", "exclude", "patternset", "filename", "contains", "date"]),
    ("dirset", &["include", "exclude", "patternset"]),
    ("zipfileset", &["include", "exclude"]),
    ("patternset", &["include", "exclude", "includesfile", "excludesfile"]),
    ("manifest", &["attribute", "section"]),
    ("section", &["attribute"]),
    ("macrodef", &["attribute", "element", "text", "sequential"]),
    ("presetdef", &["javac", "java", "jar", "copy", "exec"]),
    (
        "condition",
        &["and", "or", "not", "equals", "isset", "istrue", "isfalse", "available", "os", "contains", "matches"],
    ),
    ("and", &["equals", "isset", "istrue", "isfalse", "available", "os", "not", "or"]),
    ("or", &["equals", "isset", "istrue", "isfalse", "available", "os", "not", "and"]),
    ("not", &["equals", "isset", "istrue", "isfalse", "available", "os", "and", "or"]),
    ("antcall", &["param", "reference", "propertyset"]),
    ("ant", &["property", "reference", "propertyset", "target"]),
    ("subant", &["fileset", "filelist", "property", "propertyset", "target"]),
    ("filterset", &["filter", "filtersfile"]),
];

const ATTRIBUTES: &[(&str, &[&str])] = &[
    ("project", &["name", "default", "basedir"]),
    ("target", &["name", "depends", "if", "unless", "description", "extensionOf", "onMissingExtensionPoint"]),
    ("extension-point", &["name", "depends", "if", "unless", "description", "extensionOf"]),
    ("property", &["name", "value", "location", "file", "resource", "url", "environment", "prefix", "refid"]),
    (
        "javac",
        &[
            "srcdir",
            "destdir",
            "classpath",
            "classpathref",
            "includeantruntime",
            "release",
            "source",
            "target",
            "debug",
            "encoding",
            "fork",
            "includes",
            "excludes",
            "nowarn",
            "deprecation",
            "failonerror",
        ],
    ),
    ("jar", &["destfile", "basedir", "manifest", "includes", "excludes", "compress", "update"]),
    ("war", &["destfile", "webxml", "basedir", "needxmlfile"]),
    ("zip", &["destfile", "basedir", "includes", "excludes", "update"]),
    ("java", &["classname", "jar", "module", "fork", "failonerror", "classpath", "classpathref", "dir", "maxmemory"]),
    ("exec", &["executable", "dir", "failonerror", "osfamily", "output", "resultproperty", "spawn"]),
    ("mkdir", &["dir"]),
    ("delete", &["file", "dir", "quiet", "failonerror", "includeemptydirs"]),
    ("copy", &["file", "tofile", "todir", "overwrite", "flatten", "preservelastmodified", "failonerror"]),
    ("move", &["file", "tofile", "todir", "overwrite", "flatten"]),
    ("echo", &["message", "file", "append", "level", "encoding"]),
    ("fail", &["message", "if", "unless", "status"]),
    ("antcall", &["target", "inheritAll", "inheritRefs"]),
    ("ant", &["antfile", "dir", "target", "output", "inheritAll", "inheritRefs", "useNativeBasedir"]),
    ("subant", &["target", "antfile", "genericantfile", "inheritall", "failonerror"]),
    ("import", &["file", "optional", "as", "prefixSeparator"]),
    ("include", &["file", "optional", "as", "prefixSeparator", "name"]),
    ("junit", &["printsummary", "fork", "forkmode", "haltonfailure", "haltonerror", "failureproperty", "dir"]),
    ("test", &["name", "todir", "outfile", "if", "unless"]),
    ("batchtest", &["todir", "fork", "if", "unless"]),
    ("formatter", &["type", "usefile", "classname", "extension"]),
    ("fileset", &["dir", "file", "includes", "excludes", "defaultexcludes", "casesensitive", "id", "refid"]),
    ("dirset", &["dir", "includes", "excludes", "id", "refid"]),
    ("path", &["id", "refid", "path", "location"]),
    ("classpath", &["refid", "path", "location"]),
    ("pathelement", &["location", "path"]),
    ("include", &["name", "if", "unless"]),
    ("exclude", &["name", "if", "unless"]),
    ("arg", &["value", "line", "file", "path", "prefix", "suffix"]),
    ("jvmarg", &["value", "line"]),
    ("sysproperty", &["key", "value", "file", "path"]),
    ("taskdef", &["name", "classname", "classpath", "classpathref", "resource", "file", "uri"]),
    ("typedef", &["name", "classname", "classpath", "classpathref", "resource", "file", "uri"]),
    ("macrodef", &["name", "uri", "description"]),
    ("available", &["property", "file", "classname", "resource", "classpathref", "type", "value"]),
    ("condition", &["property", "value", "else"]),
    ("equals", &["arg1", "arg2", "casesensitive", "trim"]),
    ("isset", &["property"]),
    ("istrue", &["value"]),
    ("isfalse", &["value"]),
    ("os", &["family", "name", "arch", "version"]),
    ("loadfile", &["srcfile", "property", "encoding", "failonerror"]),
    ("loadproperties", &["srcfile", "resource", "prefix", "encoding"]),
    ("tstamp", &["prefix"]),
    ("param", &["name", "value", "location"]),
    ("attribute", &["name", "value", "default", "description"]),
    ("manifest", &["file", "mode", "encoding"]),
];

const TARGET_ATTRIBUTES: &[(&str, &str)] = &[
    ("target", "depends"),
    ("target", "extensionOf"),
    ("extension-point", "depends"),
    ("extension-point", "extensionOf"),
    ("project", "default"),
    ("antcall", "target"),
    ("ant", "target"),
    ("subant", "target"),
];

const LIST_ATTRIBUTES: &[&str] = &["depends", "extensionOf"];

const PROPERTY_ATTRIBUTES: &[&str] = &["if", "unless", "property"];

const REFERENCE_ATTRIBUTES: &[&str] = &["refid", "classpathref", "pathref", "sourcepathref"];

const BUILT_IN_PROPERTIES: &[&str] = &[
    "basedir",
    "ant.file",
    "ant.home",
    "ant.version",
    "ant.project.name",
    "ant.project.default-target",
    "ant.java.version",
    "ant.core.lib",
    "java.home",
    "java.version",
    "user.home",
    "user.dir",
    "user.name",
    "os.name",
    "file.separator",
    "path.separator",
    "line.separator",
];

const VALUES: &[(&str, &[&str])] = &[
    ("includeantruntime", &["false", "true"]),
    ("fork", &["true", "false"]),
    ("failonerror", &["true", "false"]),
    ("debug", &["true", "false"]),
    ("overwrite", &["true", "false"]),
    ("haltonfailure", &["true", "false"]),
    ("quiet", &["true", "false"]),
    ("append", &["true", "false"]),
    ("optional", &["true", "false"]),
    ("inheritAll", &["true", "false"]),
    ("printsummary", &["yes", "no", "withOutAndErr", "on", "off"]),
    ("level", &["error", "warning", "info", "verbose", "debug"]),
    ("onMissingExtensionPoint", &["fail", "warn", "ignore"]),
    ("type", &["plain", "brief", "xml", "failure"]),
    ("family", &["windows", "unix", "mac", "dos", "os/2"]),
];

#[derive(Clone, Debug)]
struct Open {
    name: String,
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
        let Some(close) = tag_end(text, at) else { return (stack, Some(at)) };
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
            stack.push(Open { name });
        }
        at = close + 1;
    }
    (stack, None)
}

fn tag_end(text: &str, lt: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (index, c) in text[lt..].char_indices() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(lt + index),
            _ => {}
        }
    }
    None
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')
}

fn attribute_values(text: &str, element: Option<&str>, attribute: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(lt) = text[at..].find('<').map(|found| at + found) {
        let Some(end) = tag_end(text, lt) else { break };
        let tag = &text[lt + 1..end];
        at = end + 1;
        let name: String = tag.chars().take_while(|c| is_name(*c)).collect();
        if name.is_empty() || element.is_some_and(|element| element != name) {
            continue;
        }
        if let Some(value) = attribute_in(tag, attribute)
            && !value.is_empty()
            && !found.contains(&value)
        {
            found.push(value);
        }
    }
    found
}

fn attribute_in(tag: &str, attribute: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(at) = rest.find(attribute) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + attribute.len()..].trim_start();
        rest = &rest[at + attribute.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(value) = after.strip_prefix('=') else { continue };
        let value = value.trim_start();
        let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let inner = &value[1..];
        return Some(inner[..inner.find(quote).unwrap_or(inner.len())].to_owned());
    }
    None
}

fn targets(text: &str) -> Vec<String> {
    let mut names = attribute_values(text, Some("target"), "name");
    for name in attribute_values(text, Some("extension-point"), "name") {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn properties(text: &str) -> Vec<String> {
    let mut names = attribute_values(text, Some("property"), "name");
    for name in attribute_values(text, None, "property") {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn item(label: &str, kind: &'static str, insert: String, detail: &str) -> Item {
    Item { label: label.to_owned(), kind, insert, detail: detail.to_owned() }
}

fn children_of(parent: &str) -> Vec<&'static str> {
    if let Some((_, children)) = CHILDREN.iter().find(|(name, _)| *name == parent) {
        return children.to_vec();
    }
    let mut all: Vec<&'static str> = Vec::new();
    match parent {
        "" => all.push("project"),
        "project" => {
            all.extend(TOP_LEVEL);
            all.extend(TYPES);
            all.extend(TASKS);
        }
        _ => {
            all.extend(TASKS);
            all.extend(TYPES);
        }
    }
    all
}

fn has_children(name: &str) -> bool {
    matches!(name, "project" | "target" | "extension-point" | "sequential" | "parallel" | "description")
        || CHILDREN.iter().any(|(element, _)| *element == name)
}

fn element_items(parent: &str) -> Vec<Item> {
    children_of(parent)
        .into_iter()
        .map(|child| {
            let insert = if has_children(child) { format!("{child}>$0</{child}>") } else { format!("{child} $0/>") };
            let detail = if parent.is_empty() { "ant" } else { parent };
            item(child, "tag", insert, detail)
        })
        .collect()
}

fn value_items(text: &str, element: &str, attribute: &str) -> Option<Vec<Item>> {
    if TARGET_ATTRIBUTES.contains(&(element, attribute)) {
        let names = targets(text);
        return Some(names.iter().map(|name| item(name, "value", name.clone(), "target")).collect());
    }
    if PROPERTY_ATTRIBUTES.contains(&attribute) && !(element == "condition" || element == "available") {
        return Some(property_items(text, ""));
    }
    if REFERENCE_ATTRIBUTES.contains(&attribute) {
        let ids = attribute_values(text, None, "id");
        return Some(ids.iter().map(|id| item(id, "value", id.clone(), "id")).collect());
    }
    let (_, values) = VALUES.iter().find(|(name, _)| *name == attribute)?;
    Some(values.iter().map(|value| item(value, "value", (*value).to_owned(), attribute)).collect())
}

fn property_items(text: &str, close: &str) -> Vec<Item> {
    let mut items: Vec<Item> =
        properties(text).iter().map(|name| item(name, "property", format!("{name}{close}"), "property")).collect();
    items.extend(BUILT_IN_PROPERTIES.iter().map(|name| item(name, "property", format!("{name}{close}"), "ant")));
    items
}

pub fn complete(text: &str, caret: usize) -> Option<Completions> {
    if caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let before = &text[..caret];
    if let Some(open) = before.rfind("${").filter(|open| !before[*open..].contains(['}', '"', '\'', '<', '>'])) {
        let from = open + 2;
        if before[from..].chars().all(is_name) {
            return Some(Completions { from, items: property_items(text, "}$0") });
        }
    }
    let (stack, in_tag) = open_at(text, caret);
    let lt = in_tag?;
    let tag = &text[lt + 1..caret];
    if tag.starts_with(['/', '!', '?']) {
        return None;
    }
    let name_end = tag.find(|c: char| !is_name(c)).unwrap_or(tag.len());
    let element = &tag[..name_end];
    if name_end == tag.len() {
        let parent = stack.last().map_or("", |open| open.name.as_str());
        return Some(Completions { from: lt + 1, items: element_items(parent) });
    }
    let mut quote: Option<(char, usize)> = None;
    for (index, c) in tag.char_indices().skip(name_end) {
        match quote {
            Some((open, _)) if c == open => quote = None,
            Some(_) => {}
            None if matches!(c, '"' | '\'') => quote = Some((c, index)),
            None => {}
        }
    }
    match quote {
        Some((_, at)) => {
            let head = tag[..at].trim_end().strip_suffix('=')?.trim_end();
            let attribute_start = head.rfind(|c: char| !is_name(c)).map_or(0, |found| found + 1);
            let attribute = &head[attribute_start..];
            let value_start = lt + 1 + at + 1;
            let typed = &text[value_start..caret];
            let from = if LIST_ATTRIBUTES.contains(&attribute) {
                let comma = typed.rfind(',').map_or(0, |found| found + 1);
                value_start + comma + (typed[comma..].len() - typed[comma..].trim_start().len())
            } else {
                value_start
            };
            let items = value_items(text, element, attribute)?;
            let written: Vec<&str> = typed.split(',').map(str::trim).collect();
            let items = if LIST_ATTRIBUTES.contains(&attribute) {
                items.into_iter().filter(|item| !written[..written.len() - 1].contains(&item.label.as_str())).collect()
            } else {
                items
            };
            Some(Completions { from, items })
        }
        None => {
            let typed_start = tag.rfind(|c: char| !is_name(c)).map_or(0, |found| found + 1);
            if !tag[..typed_start].ends_with(char::is_whitespace) {
                return None;
            }
            let (_, attributes) = ATTRIBUTES.iter().find(|(name, _)| *name == element)?;
            let items = attributes
                .iter()
                .filter(|attribute| attribute_in(tag, attribute).is_none())
                .map(|attribute| item(attribute, "attribute", format!("{attribute}=\"$0\""), element))
                .collect();
            Some(Completions { from: lt + 1 + typed_start, items })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: &str = r#"<?xml version="1.0"?>
<project name="shop" default="jar" basedir=".">
  <property name="src.dir" value="src"/>
  <path id="compile.classpath"><fileset dir="lib"/></path>
  <target name="init"><mkdir dir="build"/></target>
  <target name="compile" depends="init" description="Compiles the sources">
    <javac srcdir="${src.dir}" destdir="build" classpathref="" includeantruntime="false"/>
    <ja
  </target>
  <target name="jar" depends="compile, " if="">
    <antcall target=""/>
    <echo message="${s"/>
  </target>
  <extension-point name="ready"/>
  <!-- <target depends=""/> -->
</project>
"#;

    fn labels(after: &str) -> (usize, Vec<String>) {
        let caret = BUILD.find(after).unwrap() + after.len();
        let found = complete(BUILD, caret).unwrap_or_else(|| panic!("nothing after {after:?}"));
        (found.from, found.items.into_iter().map(|item| item.label).collect())
    }

    #[test]
    fn target_names_complete_where_a_target_is_named() {
        let (from, names) = labels("depends=\"compile, ");
        assert_eq!(from, BUILD.find("depends=\"compile, ").unwrap() + "depends=\"compile, ".len());
        assert_eq!(names, ["init", "jar", "ready"], "a target already listed is not offered again");
        assert_eq!(labels("<antcall target=\"").1, ["init", "compile", "jar", "ready"]);
        let text = "<project default=\"\"><target name=\"build\"/></project>";
        assert_eq!(complete(text, text.find("default=\"").unwrap() + 9).unwrap().items[0].label, "build");
        assert_eq!(complete(text, text.find("name=\"").unwrap() + 6), None, "a new target's name is free text");
    }

    #[test]
    fn tasks_complete_inside_a_target_and_targets_at_the_top() {
        let caret = BUILD.find("<ja\n").unwrap() + 3;
        let found = complete(BUILD, caret).unwrap();
        assert_eq!(found.from, caret - 2);
        let jar = found.items.iter().find(|item| item.label == "jar").unwrap();
        assert_eq!(jar.insert, "jar>$0</jar>");
        let mkdir = found.items.iter().find(|item| item.label == "mkdir").unwrap();
        assert_eq!(mkdir.insert, "mkdir $0/>");
        assert!(!found.items.iter().any(|item| item.label == "target"), "no target inside a target");
        let text = "<project>\n  <";
        let top = complete(text, text.len()).unwrap();
        assert!(top.items.iter().any(|item| item.label == "target"));
        let text = "<project><target><javac><";
        let nested: Vec<String> = complete(text, text.len()).unwrap().items.into_iter().map(|i| i.label).collect();
        assert!(nested.contains(&"classpath".to_owned()) && nested.contains(&"src".to_owned()));
        assert_eq!(complete("<", 1).unwrap().items[0].label, "project");
    }

    #[test]
    fn attributes_values_properties_and_references_complete() {
        let text = "<project><target name=\"a\"><javac srcdir=\"src\" ";
        let found = complete(text, text.len()).unwrap();
        assert_eq!(found.from, text.len());
        assert!(found.items.iter().any(|item| item.label == "destdir" && item.insert == "destdir=\"$0\""));
        assert!(!found.items.iter().any(|item| item.label == "srcdir"), "written already");
        let (from, names) = labels("message=\"${s");
        assert_eq!((from, names[0].as_str()), (BUILD.find("${s\"").unwrap() + 2, "src.dir"));
        assert!(labels("classpathref=\"").1.contains(&"compile.classpath".to_owned()));
        assert!(labels("if=\"").1.contains(&"src.dir".to_owned()));
        assert_eq!(labels("includeantruntime=\"").1, ["false", "true"]);
    }

    #[test]
    fn nothing_in_comments_text_or_closing_tags() {
        let caret = BUILD.find("<!-- <target depends=\"").unwrap() + "<!-- <target depends=\"".len();
        assert_eq!(complete(BUILD, caret), None);
        assert_eq!(complete("<project>text", 13), None);
        assert_eq!(complete("<project></pro", 14), None);
        assert_eq!(complete("abc", 9), None);
    }
}
