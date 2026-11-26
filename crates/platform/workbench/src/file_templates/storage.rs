use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{CHILD_INFIX, ChildTemplate, FileTemplate, TemplateKind};

const SUFFIX: &str = ".ft";

const COMPONENT: &str = "ExportableFileTemplateSettings";

const MAX_TEMPLATE: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub templates: PathBuf,
    pub settings: PathBuf,
    pub root: &'static str,
}

impl Folder {
    pub fn ide(data: &Path) -> Self {
        Self {
            templates: data.join("fileTemplates"),
            settings: data.join("options").join("file.template.settings.xml"),
            root: "application",
        }
    }

    pub fn project(project: &Path) -> Self {
        let idea = project.join(".idea");
        Self {
            templates: idea.join("fileTemplates"),
            settings: idea.join("file.template.settings.xml"),
            root: "project",
        }
    }

    fn linked(&self, kind: TemplateKind) -> bool {
        let link = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
        link(&self.templates) || link(&self.kind_folder(kind))
    }

    fn kind_folder(&self, kind: TemplateKind) -> PathBuf {
        match kind {
            TemplateKind::File => self.templates.clone(),
            TemplateKind::Include => self.templates.join("includes"),
            TemplateKind::Code => self.templates.join("code"),
        }
    }
}

fn section(kind: TemplateKind) -> &'static str {
    match kind {
        TemplateKind::File => "default_templates",
        TemplateKind::Include => "includes_templates",
        TemplateKind::Code => "code_templates",
    }
}

pub fn full_name(name: &str, extension: &str) -> String {
    if extension.is_empty() { name.to_owned() } else { format!("{name}.{extension}") }
}

fn split(full: &str) -> (String, String) {
    match full.rsplit_once('.') {
        Some((name, extension)) if !name.is_empty() => (name.to_owned(), extension.to_owned()),
        _ => (full.to_owned(), String::new()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Options {
    file_name: String,
    reformat: bool,
    live_templates: bool,
}

type Settings = HashMap<(TemplateKind, String), Options>;

fn options_of(template: roxmltree::Node<'_, '_>) -> Options {
    Options {
        file_name: template.attribute("file-name").unwrap_or_default().to_owned(),
        reformat: template.attribute("reformat") != Some("false"),
        live_templates: template.attribute("live-template-enabled") == Some("true"),
    }
}

fn read_settings(file: &Path) -> Settings {
    let mut settings = Settings::new();
    let Ok(text) = std::fs::read_to_string(file) else { return settings };
    let Ok(document) = roxmltree::Document::parse(&text) else { return settings };
    for kind in [TemplateKind::File, TemplateKind::Include, TemplateKind::Code] {
        let sections = document.descendants().filter(|node| node.has_tag_name(section(kind)));
        for template in sections.flat_map(|node| node.children()).filter(|node| node.has_tag_name("template")) {
            let Some(name) = template.attribute("name") else { continue };
            settings.insert((kind, name.to_owned()), options_of(template));
        }
    }
    settings
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\n' => escaped.push_str("&#10;"),
            c => escaped.push(c),
        }
    }
    escaped
}

fn template_entry(template: &FileTemplate) -> String {
    format!(
        "<template name=\"{}\" file-name=\"{}\" reformat=\"{}\" live-template-enabled=\"{}\" />",
        escape(&full_name(&template.name, &template.extension)),
        escape(&template.file_name),
        template.reformat,
        template.live_templates
    )
}

fn component(
    templates: &[FileTemplate],
    kept: &[(TemplateKind, String, String)],
    written: &dyn Fn(&FileTemplate) -> Option<String>,
) -> Option<String> {
    if templates.is_empty() && kept.is_empty() {
        return None;
    }
    let mut xml = format!("<component name=\"{COMPONENT}\">\n");
    for kind in [TemplateKind::File, TemplateKind::Include, TemplateKind::Code] {
        let mut entries: Vec<(String, String)> = templates
            .iter()
            .filter(|template| template.kind == kind)
            .map(|template| {
                let entry = written(template).unwrap_or_else(|| template_entry(template));
                (full_name(&template.name, &template.extension), entry)
            })
            .chain(kept.iter().filter(|(of, _, _)| *of == kind).map(|(_, name, entry)| (name.clone(), entry.clone())))
            .collect();
        if entries.is_empty() {
            continue;
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        xml.push_str(&format!("    <{}>\n", section(kind)));
        for (_, entry) in entries {
            xml.push_str(&format!("      {entry}\n"));
        }
        xml.push_str(&format!("    </{}>\n", section(kind)));
    }
    xml.push_str("  </component>");
    Some(xml)
}

fn line_start(text: &str, at: usize) -> usize {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    if text[start..at].chars().all(|c| c == ' ' || c == '\t') { start } else { at }
}

fn merged_settings(
    old: &str,
    folder: &Folder,
    templates: &[FileTemplate],
    known: &[FileTemplate],
) -> Option<Option<String>> {
    let document = roxmltree::Document::parse(old).ok()?;
    let root = document.root_element();
    let existing =
        root.children().find(|node| node.has_tag_name("component") && node.attribute("name") == Some(COMPONENT));
    let mut kept = Vec::new();
    let mut original: Vec<(TemplateKind, String, String, Options)> = Vec::new();
    if let Some(existing) = existing {
        for kind in [TemplateKind::File, TemplateKind::Include, TemplateKind::Code] {
            let entries = existing
                .children()
                .filter(|node| node.has_tag_name(section(kind)))
                .flat_map(|node| node.children())
                .filter(|node| node.has_tag_name("template"));
            for entry in entries {
                let Some(name) = entry.attribute("name") else { continue };
                original.push((kind, name.to_owned(), old[entry.range()].to_owned(), options_of(entry)));
                let ours = |template: &FileTemplate| {
                    template.kind == kind && full_name(&template.name, &template.extension) == name
                };
                if !templates.iter().any(ours) && !known.iter().any(ours) {
                    kept.push((kind, name.to_owned(), old[entry.range()].to_owned()));
                }
            }
        }
    }
    let written = |template: &FileTemplate| {
        original.iter().find_map(|(kind, name, entry, options)| {
            let same = *kind == template.kind
                && *name == full_name(&template.name, &template.extension)
                && *options
                    == Options {
                        file_name: template.file_name.clone(),
                        reformat: template.reformat,
                        live_templates: template.live_templates,
                    };
            same.then(|| entry.clone())
        })
    };
    let entry_of = |template: &FileTemplate| {
        original
            .iter()
            .any(|(kind, name, _, _)| *kind == template.kind && *name == full_name(&template.name, &template.extension))
    };
    let plain = |template: &FileTemplate| {
        !entry_of(template) && template.file_name.is_empty() && template.reformat && !template.live_templates
    };
    let dropped = original.iter().any(|(kind, name, _, _)| {
        !kept.iter().any(|(of, known, _)| of == kind && known == name)
            && !templates
                .iter()
                .any(|template| template.kind == *kind && full_name(&template.name, &template.extension) == *name)
    });
    if existing.is_some() && !dropped && templates.iter().all(|template| written(template).is_some() || plain(template))
    {
        return Some(Some(old.to_owned()));
    }
    let component = component(templates, &kept, &written);
    let merged = match (existing, component) {
        (Some(existing), Some(component)) => {
            format!("{}{component}{}", &old[..existing.range().start], &old[existing.range().end..])
        }
        (Some(existing), None) => {
            let range = existing.range();
            let from = line_start(old, range.start);
            let to = if from < range.start {
                range.end + usize::from(old[range.end..].starts_with('\n'))
            } else {
                range.end
            };
            let others = root.children().any(|node| node.is_element() && node != existing);
            if !others && root.attributes().all(|attribute| attribute.name() == "version") {
                return Some(None);
            }
            format!("{}{}", &old[..from], &old[to..])
        }
        (None, None) => return Some(Some(old.to_owned())),
        (None, Some(component)) => {
            let range = root.range();
            let element = &old[range.clone()];
            if let Some(open) = element.strip_suffix("/>") {
                let open = open.trim_end();
                format!("{}{open}>\n  {component}\n</{}>{}", &old[..range.start], folder.root, &old[range.end..])
            } else {
                let close = range.start + element.rfind("</")?;
                let before = old[..close].trim_end_matches([' ', '\t']);
                let newline = if before.ends_with('\n') { "" } else { "\n" };
                format!("{before}{newline}  {component}\n{}", &old[close..])
            }
        }
    };
    Some(Some(merged))
}

fn write_settings(folder: &Folder, templates: &[FileTemplate], known: &[FileTemplate]) -> std::io::Result<()> {
    let old = std::fs::read_to_string(&folder.settings).ok();
    let merged = old.as_deref().and_then(|old| merged_settings(old, folder, templates, known));
    let xml = match merged {
        Some(xml) => xml,
        None => component(templates, &[], &|_| None).map(|component| {
            let version = if folder.root == "project" { " version=\"4\"" } else { "" };
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{}{version}>\n  {component}\n</{}>\n",
                folder.root, folder.root
            )
        }),
    };
    let Some(xml) = xml else {
        return match std::fs::remove_file(&folder.settings) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    };
    if old.as_deref() == Some(xml.as_str()) {
        return Ok(());
    }
    if let Some(parent) = folder.settings.parent() {
        std::fs::create_dir_all(parent)?;
    }
    zen_vfs::write_atomic(&folder.settings, xml.as_bytes()).map_err(|error| error.source)
}

pub fn read(folder: &Folder) -> Vec<FileTemplate> {
    unflatten(read_flat(folder))
}

fn child_index(full: &str, parent: &str) -> Option<usize> {
    let rest = full.strip_prefix(parent)?.strip_prefix(CHILD_INFIX)?;
    let digits = rest.split('.').next()?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn unflatten(flat: Vec<FileTemplate>) -> Vec<FileTemplate> {
    let parents: Vec<String> = flat
        .iter()
        .filter(|template| template.kind == TemplateKind::File && !template.full_name().contains(CHILD_INFIX))
        .map(FileTemplate::full_name)
        .collect();
    let mut children: Vec<(String, usize, ChildTemplate)> = Vec::new();
    let mut templates = Vec::new();
    for template in flat {
        let full = template.full_name();
        let parent = (template.kind == TemplateKind::File)
            .then(|| parents.iter().find_map(|parent| Some((parent.clone(), child_index(&full, parent)?))))
            .flatten();
        match parent {
            Some((parent, index)) => {
                let extension = full[parent.len() + CHILD_INFIX.len()..]
                    .split_once('.')
                    .map(|(_, extension)| extension.to_owned())
                    .unwrap_or_default();
                let child = ChildTemplate {
                    extension,
                    file_name: template.file_name,
                    text: template.text,
                    reformat: template.reformat,
                    live_templates: template.live_templates,
                };
                children.push((parent, index, child));
            }
            None => templates.push(template),
        }
    }
    children.sort_by_key(|(_, index, _)| *index);
    for (parent, _, child) in children {
        if let Some(template) =
            templates.iter_mut().find(|template| template.kind == TemplateKind::File && template.full_name() == parent)
        {
            template.children.push(child);
        }
    }
    templates
}

fn flatten(templates: &[FileTemplate]) -> Vec<FileTemplate> {
    let mut flat = Vec::with_capacity(templates.len());
    for template in templates {
        flat.push(FileTemplate { children: Vec::new(), ..template.clone() });
        for (index, child) in template.children.iter().enumerate() {
            let full = full_name(&format!("{}{CHILD_INFIX}{index}", template.full_name()), &child.extension);
            let (name, extension) = split(&full);
            flat.push(FileTemplate {
                name,
                extension,
                kind: TemplateKind::File,
                file_name: child.file_name.clone(),
                text: child.text.clone(),
                reformat: child.reformat,
                live_templates: child.live_templates,
                children: Vec::new(),
            });
        }
    }
    flat
}

fn read_flat(folder: &Folder) -> Vec<FileTemplate> {
    let settings = read_settings(&folder.settings);
    let mut templates = Vec::new();
    for kind in [TemplateKind::File, TemplateKind::Include, TemplateKind::Code] {
        if folder.linked(kind) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(folder.kind_folder(kind)) else { continue };
        let mut found: Vec<FileTemplate> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let plain = entry.file_type().is_ok_and(|kind| kind.is_file());
            if !plain || entry.metadata().map_or(true, |meta| meta.len() > MAX_TEMPLATE) {
                continue;
            }
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.starts_with('.') {
                continue;
            }
            let full = file.strip_suffix(SUFFIX).unwrap_or(&file);
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let (name, extension) = split(full);
            let options = settings.get(&(kind, full.to_owned())).cloned().unwrap_or(Options {
                file_name: String::new(),
                reformat: true,
                live_templates: false,
            });
            let template = FileTemplate {
                name,
                extension,
                kind,
                file_name: options.file_name,
                text: String::from_utf8_lossy(&bytes).replace("\r\n", "\n"),
                reformat: options.reformat,
                live_templates: options.live_templates,
                children: Vec::new(),
            };
            match found.iter_mut().find(|known| known.name == template.name) {
                Some(known) if file.ends_with(SUFFIX) => *known = template,
                Some(_) => {}
                None => found.push(template),
            }
        }
        found.sort_by(|a, b| a.name.cmp(&b.name));
        templates.extend(found);
    }
    templates
}

fn file_of(folder: &Folder, template: &FileTemplate) -> PathBuf {
    let full = full_name(&template.name, &template.extension);
    let suffix = if template.kind == TemplateKind::File && full.contains(CHILD_INFIX) { "" } else { SUFFIX };
    folder.kind_folder(template.kind).join(format!("{full}{suffix}"))
}

pub fn write(folder: &Folder, templates: &[FileTemplate]) -> std::io::Result<()> {
    let templates = &flatten(templates);
    for kind in [TemplateKind::File, TemplateKind::Include, TemplateKind::Code] {
        if folder.linked(kind) {
            let path = folder.kind_folder(kind).display().to_string();
            return Err(std::io::Error::other(zen_i18n::t!("workbench-file-templates-linked", path = path)));
        }
    }
    let before = read_flat(folder);
    for gone in before.iter().filter(|old| !templates.iter().any(|new| new.kind == old.kind && new.name == old.name)) {
        for path in [file_of(folder, gone), folder.kind_folder(gone.kind).join(full_name(&gone.name, &gone.extension))]
        {
            match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
    }
    for template in templates {
        let bare = |template: &FileTemplate| {
            folder.kind_folder(template.kind).join(full_name(&template.name, &template.extension))
        };
        let old = before.iter().find(|old| old.kind == template.kind && old.name == template.name);
        if let Some(old) = old {
            if old.extension == template.extension && old.text == template.text {
                continue;
            }
            if old.extension != template.extension {
                let _ = std::fs::remove_file(file_of(folder, old));
                let _ = std::fs::remove_file(bare(old));
            }
        }
        let suffixed = file_of(folder, template);
        let unsuffixed = bare(template);
        let keeps_its_name = old.is_some_and(|old| old.extension == template.extension)
            && !suffixed.exists()
            && std::fs::symlink_metadata(&unsuffixed).is_ok_and(|meta| meta.is_file());
        let path = if keeps_its_name { unsuffixed } else { suffixed };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        zen_vfs::write_atomic(&path, template.text.as_bytes()).map_err(|error| error.source)?;
    }
    write_settings(folder, templates, &before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(name: &str, extension: &str, kind: TemplateKind) -> FileTemplate {
        FileTemplate {
            name: name.to_owned(),
            extension: extension.to_owned(),
            kind,
            file_name: String::new(),
            text: format!("text of {name}\n"),
            reformat: true,
            live_templates: false,
            children: Vec::new(),
        }
    }

    #[test]
    fn templates_are_written_in_the_shared_layout_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let folder = Folder::project(dir.path());
        let mut service = template("Service", "go", TemplateKind::File);
        service.file_name = "${NAME}_service".to_owned();
        service.live_templates = true;
        service.reformat = false;
        let header = template("File Header", "go", TemplateKind::Include);
        let plain = template("Notes", "", TemplateKind::File);
        write(&folder, &[service.clone(), header.clone(), plain.clone()]).unwrap();
        let idea = dir.path().join(".idea");
        assert_eq!(std::fs::read_to_string(idea.join("fileTemplates/Service.go.ft")).unwrap(), "text of Service\n");
        assert!(idea.join("fileTemplates/includes/File Header.go.ft").is_file());
        assert!(idea.join("fileTemplates/Notes.ft").is_file());
        let xml = std::fs::read_to_string(idea.join("file.template.settings.xml")).unwrap();
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">"), "{xml}");
        assert!(xml.contains(r#"<component name="ExportableFileTemplateSettings">"#), "{xml}");
        assert!(
            xml.contains(r#"<template name="Service.go" file-name="${NAME}_service" reformat="false" live-template-enabled="true" />"#),
            "{xml}"
        );
        assert!(xml.contains("<includes_templates>"), "{xml}");
        assert_eq!(read(&folder), vec![plain, service.clone(), header]);

        write(&folder, &[service.clone()]).unwrap();
        assert!(!idea.join("fileTemplates/includes/File Header.go.ft").exists(), "a removed template's file goes");
        assert_eq!(read(&folder), vec![service]);
        write(&folder, &[]).unwrap();
        assert!(!idea.join("file.template.settings.xml").exists());
        assert!(read(&folder).is_empty());
    }

    #[test]
    fn files_without_the_ft_suffix_and_settings_written_elsewhere_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let folder = Folder::ide(dir.path());
        std::fs::create_dir_all(dir.path().join("fileTemplates/includes")).unwrap();
        std::fs::create_dir_all(dir.path().join("options")).unwrap();
        std::fs::write(dir.path().join("fileTemplates/Handler.go"), "package x\r\n").unwrap();
        std::fs::write(dir.path().join("fileTemplates/includes/Header.txt"), "// h").unwrap();
        std::fs::write(dir.path().join("fileTemplates/.DS_Store"), "x").unwrap();
        std::fs::write(
            dir.path().join("options/file.template.settings.xml"),
            r#"<application><component name="ExportableFileTemplateSettings"><default_templates>
            <template name="Handler.go" file-name="h_${NAME}" reformat="false" live-template-enabled="true" />
            </default_templates></component></application>"#,
        )
        .unwrap();
        let read = read(&folder);
        assert_eq!(read.len(), 2);
        assert_eq!(
            (read[0].name.as_str(), read[0].extension.as_str(), read[0].text.as_str()),
            ("Handler", "go", "package x\n")
        );
        assert_eq!((read[0].file_name.as_str(), read[0].reformat, read[0].live_templates), ("h_${NAME}", false, true));
        assert_eq!((read[1].kind, read[1].reformat, read[1].live_templates), (TemplateKind::Include, true, false));
    }

    #[test]
    fn writing_keeps_the_rest_of_the_settings_file_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let folder = Folder::project(dir.path());
        std::fs::create_dir_all(dir.path().join(".idea")).unwrap();
        let original = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n  <!-- kept -->\n  <component name=\"Other\" a='1'>\t<x/></component>\n  <component name=\"ExportableFileTemplateSettings\">\n    <default_templates>\n      <template name=\"Gone.go\" file-name=\"\" reformat=\"true\" live-template-enabled=\"false\" />\n      <template name=\"Builtin.go\"   file-name=\"b_${NAME}\" reformat='false' live-template-enabled=\"false\" />\n    </default_templates>\n  </component>\n  <component name=\"Last\"/>\n</project>";
        std::fs::write(&folder.settings, original).unwrap();
        std::fs::create_dir_all(&folder.templates).unwrap();
        std::fs::write(folder.templates.join("Gone.go.ft"), "gone").unwrap();
        let service = template("Service", "go", TemplateKind::File);
        write(&folder, std::slice::from_ref(&service)).unwrap();
        let xml = std::fs::read_to_string(&folder.settings).unwrap();
        let (head, rest) = original.split_once("<component name=\"ExportableFileTemplateSettings\">").unwrap();
        let tail = &rest[rest.find("</component>").unwrap() + "</component>".len()..];
        assert!(xml.starts_with(head) && xml.ends_with(tail), "{xml}");
        assert!(!xml.contains("Gone.go"), "a removed template's entry goes: {xml}");
        assert!(
            xml.contains("<template name=\"Builtin.go\"   file-name=\"b_${NAME}\" reformat='false' live-template-enabled=\"false\" />"),
            "an entry without a file is kept as written: {xml}"
        );
        assert!(xml.contains("<template name=\"Service.go\""), "{xml}");
        assert!(roxmltree::Document::parse(&xml).is_ok(), "{xml}");

        write(&folder, &[]).unwrap();
        let xml = std::fs::read_to_string(&folder.settings).unwrap();
        assert!(xml.contains("Builtin.go") && !xml.contains("Service.go") && xml.ends_with(tail), "{xml}");

        let plain = "<project version=\"4\">\n  <component name=\"Other\" />\n</project>\n";
        std::fs::write(&folder.settings, plain).unwrap();
        write(&folder, std::slice::from_ref(&service)).unwrap();
        let xml = std::fs::read_to_string(&folder.settings).unwrap();
        assert!(xml.starts_with("<project version=\"4\">\n  <component name=\"Other\" />\n  <component name=\"ExportableFileTemplateSettings\">"), "{xml}");
        assert!(xml.ends_with("  </component>\n</project>\n"), "{xml}");
        write(&folder, &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&folder.settings).unwrap(),
            "<project version=\"4\">\n  <component name=\"Other\" />\n</project>\n"
        );
        let empty = "<project version=\"4\" />";
        std::fs::write(&folder.settings, empty).unwrap();
        write(&folder, std::slice::from_ref(&service)).unwrap();
        let xml = std::fs::read_to_string(&folder.settings).unwrap();
        assert!(roxmltree::Document::parse(&xml).is_ok() && xml.contains("Service.go"), "{xml}");
    }

    #[cfg(unix)]
    #[test]
    fn links_and_huge_files_are_not_read_and_linked_folders_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, "secret").unwrap();
        let project = dir.path().join("p");
        let folder = Folder::project(&project);
        std::fs::create_dir_all(folder.templates.join("includes")).unwrap();
        std::os::unix::fs::symlink(&secret, folder.templates.join("Leak.txt.ft")).unwrap();
        std::os::unix::fs::symlink(&secret, folder.templates.join("includes/Header.txt")).unwrap();
        std::fs::write(folder.templates.join("Huge.txt.ft"), vec![b'x'; (MAX_TEMPLATE + 1) as usize]).unwrap();
        std::fs::write(folder.templates.join("Fine.txt.ft"), "fine").unwrap();
        let names: Vec<String> = read(&folder).into_iter().map(|template| template.name).collect();
        assert_eq!(names, ["Fine"]);

        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("Outside.txt.ft"), "outside").unwrap();
        std::fs::remove_dir_all(folder.templates.join("includes")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, folder.templates.join("includes")).unwrap();
        assert_eq!(read(&folder).len(), 1, "a linked folder is not read");
        let header = template("Header", "txt", TemplateKind::Include);
        assert!(write(&folder, &[header]).is_err());
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 1, "nothing is written through the link");
    }

    #[test]
    fn templates_and_entries_not_changed_are_left_as_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let folder = Folder::project(dir.path());
        std::fs::create_dir_all(&folder.templates).unwrap();
        std::fs::write(folder.templates.join("Crlf.go.ft"), "a\r\nb\r\n").unwrap();
        std::fs::write(folder.templates.join("Bare.txt"), "bare ${NAME}\n").unwrap();
        let xml = "<project version=\"4\">\n  <component name=\"ExportableFileTemplateSettings\">\n    <default_templates>\n      <template name='Bare.txt' file-name='b_${NAME}' reformat='false' live-template-enabled='false'/>\n    </default_templates>\n  </component>\n</project>\n";
        std::fs::write(&folder.settings, xml).unwrap();
        let mut all = read(&folder);
        write(&folder, &all).unwrap();
        assert_eq!(std::fs::read_to_string(&folder.settings).unwrap(), xml, "nothing changed, nothing is written");
        all.push(FileTemplate { reformat: false, ..template("Service", "go", TemplateKind::File) });
        write(&folder, &all).unwrap();
        assert_eq!(std::fs::read(folder.templates.join("Crlf.go.ft")).unwrap(), b"a\r\nb\r\n");
        assert!(!folder.templates.join("Bare.txt.ft").exists(), "a file without .ft is not copied");
        let written = std::fs::read_to_string(&folder.settings).unwrap();
        assert!(
            written.contains(
                "<template name='Bare.txt' file-name='b_${NAME}' reformat='false' live-template-enabled='false'/>"
            ),
            "{written}"
        );
        assert!(written.contains("Service.go"), "{written}");
        all[0].text = "changed\n".to_owned();
        write(&folder, &all).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.templates.join("Bare.txt")).unwrap(),
            "changed\n",
            "it keeps its file"
        );
        assert!(!folder.templates.join("Bare.txt.ft").exists());
    }
}
