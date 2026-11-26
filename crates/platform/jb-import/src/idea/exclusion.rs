use std::path::{Component, Path, PathBuf};

use roxmltree::Node;
use zen_project::settings::{ExclusionChange, ReadFiles};

use super::modules::{RESOURCE_TYPE, ROOT_MANAGER, source_kind};
use super::xml::{self, Macros};
use super::{IDEA, MODULES};

const MAX_FILE: u64 = 4 * 1024 * 1024;

struct Disk<'a>(&'a Path);

impl Disk<'_> {
    fn resolve(&self, relative: &Path) -> Option<PathBuf> {
        let mut path = self.0.to_owned();
        for component in relative.components() {
            let Component::Normal(name) = component else { return None };
            path.push(name);
            if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                return None;
            }
        }
        Some(path)
    }
}

impl ReadFiles for Disk<'_> {
    fn read(&self, relative: &Path) -> Option<String> {
        let path = self.resolve(relative)?;
        if std::fs::metadata(&path).ok()?.len() > MAX_FILE {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }

    fn list(&self, relative_dir: &Path) -> Vec<PathBuf> {
        let Some(dir) = self.resolve(relative_dir) else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut listed: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_symlink()))
            .map(|entry| relative_dir.join(entry.file_name()))
            .collect();
        listed.sort();
        listed
    }
}

struct Module {
    path: PathBuf,
    original: String,
    text: String,
    macros: Macros,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Excluded,
    Resource,
}

impl Mark {
    fn element(self, url: &str) -> String {
        match self {
            Mark::Excluded => format!("<excludeFolder url=\"{}\" />", escape(url)),
            Mark::Resource => format!("<sourceFolder url=\"{}\" type=\"{RESOURCE_TYPE}\" />", escape(url)),
        }
    }

    fn tag(self) -> Tag {
        match self {
            Mark::Excluded => Tag::ExcludeFolder,
            Mark::Resource => Tag::SourceFolder,
        }
    }

    fn is(self, node: &Node<'_, '_>) -> bool {
        match self {
            Mark::Excluded => node.has_tag_name("excludeFolder"),
            Mark::Resource => node.has_tag_name("sourceFolder") && source_kind(node).is_resource(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Tag {
    SourceFolder,
    ExcludeFolder,
    ExcludePattern,
}

impl Tag {
    fn of(node: &Node<'_, '_>) -> Option<Tag> {
        match node.tag_name().name() {
            "sourceFolder" => Some(Tag::SourceFolder),
            "excludeFolder" => Some(Tag::ExcludeFolder),
            "excludePattern" => Some(Tag::ExcludePattern),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Tag::SourceFolder => "sourceFolder",
            Tag::ExcludeFolder => "excludeFolder",
            Tag::ExcludePattern => "excludePattern",
        }
    }
}

fn modules_of(root: &Path, user_home: Option<&Path>) -> Vec<Module> {
    let disk = Disk(root);
    let macros = Macros::new(root, user_home);
    let mut modules = Vec::new();
    for relative in super::module_sources(root, &disk, &macros, &mut Vec::new()) {
        let Some(text) = disk.read(&relative) else { continue };
        let module_dir = super::modules::module_dir(root, &relative);
        modules.push(Module {
            path: root.join(&relative),
            original: text.clone(),
            text,
            macros: macros.for_module(&module_dir),
        });
    }
    modules
}

fn check_folder(root: &Path, folder: &Path) -> Result<(), String> {
    if folder == root || !folder.starts_with(root) {
        return Err(format!("{} is not a folder inside the project", folder.display()));
    }
    Ok(())
}

pub(super) fn set_excluded(
    root: &Path,
    user_home: Option<&Path>,
    folder: &Path,
    excluded: bool,
) -> Result<ExclusionChange, String> {
    set_mark(root, user_home, folder, Mark::Excluded, excluded)
}

pub(super) fn set_resource_root(
    root: &Path,
    user_home: Option<&Path>,
    folder: &Path,
    marked: bool,
) -> Result<ExclusionChange, String> {
    set_mark(root, user_home, folder, Mark::Resource, marked)
}

fn set_mark(
    root: &Path,
    user_home: Option<&Path>,
    folder: &Path,
    mark: Mark,
    on: bool,
) -> Result<ExclusionChange, String> {
    check_folder(root, folder)?;
    let mut modules = modules_of(root, user_home);
    if !on {
        for module in &mut modules {
            remove_all(module, folder, |node| mark.is(node))?;
        }
        return write_changed(&modules);
    }
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let marked = modules.iter().any(|module| find(module, folder, |node| mark.is(node)).is_some());
    let other = |node: &Node<'_, '_>| match mark {
        Mark::Excluded => Mark::Resource.is(node),
        Mark::Resource => (node.has_tag_name("sourceFolder") && !mark.is(node)) || Mark::Excluded.is(node),
    };
    for module in &mut modules {
        remove_all(module, folder, other)?;
    }
    if marked {
        return write_changed(&modules);
    }
    let Some((index, at)) = content_for(&modules, folder) else {
        if !modules.is_empty() || root.join(IDEA).join(MODULES).exists() {
            return Err(format!("no module of the project contains {}", folder.display()));
        }
        let url = url_for(folder, &Macros::new(root, None).for_module(root));
        return create_module(root, &[mark.element(&url)]);
    };
    let module = &mut modules[index];
    let url = url_for(folder, &module.macros);
    module.text = insert_child(&module.text, at, &mark.element(&url), mark.tag(), Some(&url))?;
    write_changed(&modules)
}

pub(super) fn set_exclude_patterns(
    root: &Path,
    user_home: Option<&Path>,
    patterns: &[String],
) -> Result<ExclusionChange, String> {
    let mut wanted: Vec<String> = Vec::new();
    for pattern in patterns.iter().map(|pattern| pattern.trim()).filter(|pattern| !pattern.is_empty()) {
        if pattern.contains(['/', '\\', ';']) {
            return Err(format!("{pattern} is not a name pattern"));
        }
        if !wanted.iter().any(|kept| kept == pattern) {
            wanted.push(pattern.to_owned());
        }
    }
    let mut modules = modules_of(root, user_home);
    let mut kept: Vec<String> = Vec::new();
    for module in &mut modules {
        while let Ok(document) = xml::parse(&module.text) {
            let mut seen = kept.clone();
            let mut stale = None;
            for node in patterns_of(&document) {
                let written = split_patterns(node.attribute("pattern").unwrap_or_default());
                let keep = !written.is_empty()
                    && written.iter().all(|pattern| wanted.contains(pattern) && !seen.contains(pattern));
                if !keep {
                    stale = Some(node.range().start);
                    break;
                }
                seen.extend(written);
            }
            match stale {
                Some(at) => module.text = remove_child(&module.text, at, Tag::ExcludePattern)?,
                None => {
                    kept = seen;
                    break;
                }
            }
        }
    }
    let missing: Vec<&String> = wanted.iter().filter(|pattern| !kept.contains(pattern)).collect();
    if missing.is_empty() {
        return write_changed(&modules);
    }
    let elements: Vec<String> =
        missing.iter().map(|pattern| format!("<excludePattern pattern=\"{}\" />", escape(pattern))).collect();
    let Some((index, at)) = main_content(&modules, root) else {
        if !modules.is_empty() || root.join(IDEA).join(MODULES).exists() {
            return Err("no module of the project has a content root".to_owned());
        }
        return create_module(root, &elements);
    };
    let module = &mut modules[index];
    for element in &elements {
        module.text = insert_child(&module.text, at, element, Tag::ExcludePattern, None)?;
    }
    write_changed(&modules)
}

fn split_patterns(written: &str) -> Vec<String> {
    written.split(';').map(str::trim).filter(|pattern| !pattern.is_empty()).map(str::to_owned).collect()
}

fn patterns_of<'a, 'input>(document: &'a roxmltree::Document<'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    xml::component(document, ROOT_MANAGER)
        .into_iter()
        .flat_map(|manager| manager.children().filter(|node| node.has_tag_name("content")))
        .flat_map(|content| content.children().filter(|node| node.has_tag_name("excludePattern")))
}

fn contents<'a, 'input>(document: &'a roxmltree::Document<'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    xml::component(document, ROOT_MANAGER)
        .into_iter()
        .flat_map(|manager| manager.children().filter(|node| node.has_tag_name("content")))
}

fn content_for(modules: &[Module], folder: &Path) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize, PathBuf)> = None;
    for (index, module) in modules.iter().enumerate() {
        let Ok(document) = xml::parse(&module.text) else { continue };
        for content in contents(&document) {
            let Some(content_root) = content.attribute("url").and_then(|url| module.macros.url(url)) else { continue };
            let deeper =
                best.as_ref().is_none_or(|(_, _, best)| content_root.starts_with(best) && content_root != *best);
            if folder.starts_with(&content_root) && folder != content_root && deeper {
                best = Some((index, content.range().start, content_root));
            }
        }
    }
    best.map(|(index, at, _)| (index, at))
}

fn main_content(modules: &[Module], root: &Path) -> Option<(usize, usize)> {
    let mut first = None;
    for (index, module) in modules.iter().enumerate() {
        let Ok(document) = xml::parse(&module.text) else { continue };
        for content in contents(&document) {
            first = first.or(Some((index, content.range().start)));
            if content.attribute("url").and_then(|url| module.macros.url(url)).as_deref() == Some(root) {
                return Some((index, content.range().start));
            }
        }
    }
    first
}

fn find(module: &Module, folder: &Path, is: impl Fn(&Node<'_, '_>) -> bool) -> Option<(usize, Tag)> {
    let document = xml::parse(&module.text).ok()?;
    contents(&document).flat_map(|content| content.children()).find_map(|node| {
        let tag = Tag::of(&node)?;
        let here = node.attribute("url").and_then(|url| module.macros.url(url)).as_deref() == Some(folder);
        (here && is(&node)).then(|| (node.range().start, tag))
    })
}

fn remove_all(module: &mut Module, folder: &Path, is: impl Fn(&Node<'_, '_>) -> bool) -> Result<(), String> {
    while let Some((at, tag)) = find(module, folder, &is) {
        module.text = remove_child(&module.text, at, tag)?;
    }
    Ok(())
}

fn write_changed(modules: &[Module]) -> Result<ExclusionChange, String> {
    let mut change = ExclusionChange::default();
    for module in modules.iter().filter(|module| module.text != module.original) {
        write(&module.path, &module.text)?;
        change.changed = true;
        change.files.push(module.path.clone());
    }
    Ok(change)
}

fn url_for(folder: &Path, macros: &Macros) -> String {
    let under = |base: &Path, name: &str| {
        let relative = folder.strip_prefix(base).ok()?;
        let parts: Vec<String> =
            relative.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
        Some(format!("file://{name}/{}", parts.join("/")))
    };
    macros
        .module_dir
        .as_deref()
        .and_then(|dir| under(dir, xml::MODULE_DIR))
        .or_else(|| under(&macros.project_dir, xml::PROJECT_DIR))
        .unwrap_or_else(|| format!("file://{}", folder.to_string_lossy().replace('\\', "/")))
}

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn indent_before(text: &str, at: usize) -> &str {
    let line_start = text[..at].rfind('\n').map_or(0, |found| found + 1);
    let indent = &text[line_start..at];
    if indent.chars().all(|c| c == ' ' || c == '\t') { indent } else { "" }
}

fn indent_step(text: &str, content: &Node<'_, '_>) -> String {
    let outer = indent_before(text, content.range().start);
    let parent = content.parent_element().map_or("", |parent| indent_before(text, parent.range().start));
    match outer.strip_prefix(parent) {
        Some(step) if !step.is_empty() => step.to_owned(),
        _ if outer.contains('\t') => "\t".to_owned(),
        _ => "  ".to_owned(),
    }
}

fn open_tag_end(text: &str, start: usize) -> usize {
    let mut quote = None;
    for (offset, byte) in text.as_bytes()[start..].iter().enumerate() {
        match (quote, *byte) {
            (None, b'"' | b'\'') => quote = Some(*byte),
            (Some(open), byte) if byte == open => quote = None,
            (None, b'>') => return start + offset + 1,
            _ => {}
        }
    }
    text.len()
}

fn content_at<'a, 'input>(document: &'a roxmltree::Document<'input>, at: usize) -> Option<Node<'a, 'input>> {
    document.descendants().find(|node| node.has_tag_name("content") && node.range().start == at)
}

fn insert_child(
    text: &str,
    content_start: usize,
    element: &str,
    tag: Tag,
    url: Option<&str>,
) -> Result<String, String> {
    let document = xml::parse(text)?;
    let content = content_at(&document, content_start).ok_or("the module file changed while it was read")?;
    let line = newline(text);
    let range = content.range();
    let outer = indent_before(text, range.start);
    let children: Vec<Node<'_, '_>> = content.children().filter(Node::is_element).collect();
    let indent = children.first().map_or_else(
        || format!("{outer}{}", indent_step(text, &content)),
        |child| indent_before(text, child.range().start).to_owned(),
    );
    let same: Vec<&Node<'_, '_>> = children.iter().filter(|child| child.has_tag_name(tag.name())).collect();
    let after_url =
        url.and_then(|url| same.iter().find(|child| child.attribute("url").is_some_and(|other| other > url)));
    let mut edited = text.to_owned();
    if let Some(next) = after_url {
        edited.insert_str(next.range().start, &format!("{element}{line}{indent}"));
        return Ok(edited);
    }
    if let Some(last) = same.last() {
        edited.insert_str(last.range().end, &format!("{line}{indent}{element}"));
        return Ok(edited);
    }
    if let Some(later) = children.iter().find(|child| Tag::of(child).is_some_and(|other| other > tag)) {
        edited.insert_str(later.range().start, &format!("{element}{line}{indent}"));
        return Ok(edited);
    }
    if let Some(last) = children.last() {
        edited.insert_str(last.range().end, &format!("{line}{indent}{element}"));
        return Ok(edited);
    }
    let open_end = open_tag_end(text, range.start);
    if open_end >= range.end {
        let tag = &text[range.start..range.end];
        let head = tag.trim_end_matches("/>").trim_end();
        let replaced = format!("{head}>{line}{indent}{element}{line}{outer}</content>");
        edited.replace_range(range, &replaced);
        return Ok(edited);
    }
    let close = text[..range.end].rfind("</").unwrap_or(range.end);
    if !text[open_end..close].trim().is_empty() {
        edited.insert_str(close, element);
        return Ok(edited);
    }
    edited.replace_range(open_end..close, &format!("{line}{indent}{element}{line}{outer}"));
    Ok(edited)
}

fn remove_child(text: &str, at: usize, tag: Tag) -> Result<String, String> {
    let document = xml::parse(text)?;
    let node = document
        .descendants()
        .find(|node| node.has_tag_name(tag.name()) && node.range().start == at)
        .ok_or("the module file changed while it was read")?;
    let content = node.parent().filter(|parent| parent.has_tag_name("content"));
    let range = node.range();
    let mut start = range.start;
    let bytes = text.as_bytes();
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start > 0 && bytes[start - 1] == b'\n' {
        start -= 1;
        if start > 0 && bytes[start - 1] == b'\r' {
            start -= 1;
        }
    }
    let mut edited = text.to_owned();
    edited.replace_range(start..range.end, "");
    let Some(content) = content else { return Ok(edited) };
    let others = content.children().filter(Node::is_element).count() > 1;
    if others {
        return Ok(edited);
    }
    let content_start = content.range().start;
    let removed = range.end - start;
    let content_end = content.range().end - removed;
    let open_end = open_tag_end(&edited, content_start);
    let close = edited[..content_end].rfind("</").unwrap_or(content_end);
    if open_end <= close && edited[open_end..close].trim().is_empty() {
        let head = edited[content_start..open_end - 1].trim_end().to_owned();
        edited.replace_range(content_start..content_end, &format!("{head} />"));
    }
    Ok(edited)
}

fn create_module(root: &Path, children: &[String]) -> Result<ExclusionChange, String> {
    let name = root.file_name().map_or_else(|| "project".to_owned(), |name| name.to_string_lossy().into_owned());
    let idea = root.join(IDEA);
    let module = idea.join(format!("{name}.iml"));
    if module.exists() {
        return Err(format!("{} exists but .idea/{MODULES} does not list it", module.display()));
    }
    std::fs::create_dir_all(&idea).map_err(|error| format!("cannot create {}: {error}", idea.display()))?;
    let lines: String = children.iter().map(|child| format!("      {child}\n")).collect();
    let module_text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<module type=\"WEB_MODULE\" version=\"4\">\n  <component name=\"{ROOT_MANAGER}\">\n    <content url=\"file://$MODULE_DIR$\">\n{lines}    </content>\n    <orderEntry type=\"inheritedJdk\" />\n    <orderEntry type=\"sourceFolder\" forTests=\"false\" />\n  </component>\n</module>\n"
    );
    let file = escape(&format!("{}/{IDEA}/{name}.iml", xml::PROJECT_DIR));
    let modules_text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n  <component name=\"ProjectModuleManager\">\n    <modules>\n      <module fileurl=\"file://{file}\" filepath=\"{file}\" />\n    </modules>\n  </component>\n</project>\n"
    );
    let modules = idea.join(MODULES);
    write(&module, &module_text)?;
    write(&modules, &modules_text)?;
    Ok(ExclusionChange { changed: true, files: vec![module, modules] })
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().readonly()) {
        return Err(format!("{} is read-only", path.display()));
    }
    zen_vfs::write_atomic(path, text.as_bytes()).map_err(|error| error.to_string())
}
