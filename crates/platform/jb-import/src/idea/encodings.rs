use std::path::Path;

use zen_project::settings::{Encodings, FileEncoding, NewFileBom, ReadFiles, SettingsProblem};

use super::xml::{self, Macros};

const ENCODINGS: &str = ".idea/encodings.xml";
const PROJECT_URL: &str = "PROJECT";
const WITH_BOM: &str = "with BOM";
const WITH_NO_BOM: &str = "with NO BOM";
const BOM_ON_WINDOWS: &str = "with BOM under Windows, with no BOM otherwise";
const EMPTY_PROJECT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n</project>\n";

pub(super) fn read(
    root: &Path,
    files: &dyn ReadFiles,
    macros: &Macros,
    problems: &mut Vec<SettingsProblem>,
) -> Encodings {
    let Some(text) = files.read(Path::new(ENCODINGS)) else { return Encodings::default() };
    let (encodings, messages) = parse(&text, macros);
    problems.extend(messages.into_iter().map(|message| SettingsProblem { file: root.join(ENCODINGS), message }));
    encodings
}

fn parse(text: &str, macros: &Macros) -> (Encodings, Vec<String>) {
    let mut encodings = Encodings::default();
    let mut messages = Vec::new();
    let document = match xml::parse(text) {
        Ok(document) => document,
        Err(error) => return (encodings, vec![format!("cannot parse encodings.xml: {error}")]),
    };
    let Some(component) = xml::component(&document, "Encoding") else { return (encodings, messages) };
    encodings.bom_for_new_files = match component.attribute("addBOMForNewFiles") {
        None => None,
        Some(WITH_BOM) => Some(NewFileBom::With),
        Some(WITH_NO_BOM) => Some(NewFileBom::Without),
        Some(BOM_ON_WINDOWS) => Some(NewFileBom::OnWindows),
        Some(other) => {
            messages.push(format!("addBOMForNewFiles has the unknown value \"{other}\"; it is ignored"));
            None
        }
    };
    for file in component.children().filter(|child| child.has_tag_name("file")) {
        let (Some(url), Some(charset)) = (file.attribute("url"), file.attribute("charset")) else { continue };
        let charset = charset.trim();
        if charset.is_empty() {
            continue;
        }
        if url == PROJECT_URL {
            encodings.default = Some(charset.to_owned());
        } else if let Some(path) = macros.url(url) {
            encodings.files.push(FileEncoding { path, charset: charset.to_owned() });
        } else {
            messages.push(format!("the encoding of \"{url}\" is ignored: only file:// URLs are read"));
        }
    }
    (encodings, messages)
}

pub(super) fn write(root: &Path, user_home: Option<&Path>, encodings: &Encodings) -> Result<(), String> {
    let path = root.join(ENCODINGS);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => EMPTY_PROJECT.to_owned(),
        Err(error) => return Err(error.to_string()),
    };
    let updated = with_component(&text, &Macros::new(root, user_home), root, encodings)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    zen_vfs::write_atomic(&path, updated.as_bytes()).map_err(|error| error.to_string())
}

fn with_component(text: &str, macros: &Macros, root: &Path, encodings: &Encodings) -> Result<String, String> {
    let document = xml::parse(text)?;
    let project = document.root_element();
    let found = xml::component(&document, "Encoding");
    let mut block = String::from("  <component name=\"Encoding\"");
    let mut kept_entries = Vec::new();
    if let Some(component) = found {
        for attribute in component.attributes() {
            if !matches!(attribute.name(), "name" | "addBOMForNewFiles") {
                block.push_str(&format!(" {}=\"{}\"", attribute.name(), escaped(attribute.value())));
            }
        }
        for file in component.children().filter(|child| child.has_tag_name("file")) {
            let foreign = file.attribute("url").is_some_and(|url| url != PROJECT_URL && macros.url(url).is_none());
            if foreign {
                kept_entries.push(text[file.range()].to_owned());
            }
        }
    }
    if let Some(bom) = encodings.bom_for_new_files {
        let value = match bom {
            NewFileBom::With => WITH_BOM,
            NewFileBom::Without => WITH_NO_BOM,
            NewFileBom::OnWindows => BOM_ON_WINDOWS,
        };
        block.push_str(&format!(" addBOMForNewFiles=\"{value}\""));
    }
    let mut entries: Vec<String> = encodings
        .files
        .iter()
        .map(|file| {
            format!("<file url=\"{}\" charset=\"{}\" />", escaped(&url_of(root, &file.path)), escaped(&file.charset))
        })
        .collect();
    entries.append(&mut kept_entries);
    if let Some(default) = &encodings.default {
        entries.push(format!("<file url=\"{PROJECT_URL}\" charset=\"{}\" />", escaped(default)));
    }
    if entries.is_empty() {
        block.push_str(" />");
    } else {
        block.push_str(">\n");
        for entry in entries {
            block.push_str(&format!("    {entry}\n"));
        }
        block.push_str("  </component>");
    }
    Ok(match found {
        Some(component) => {
            let range = component.range();
            let start = text[..range.start].rfind('\n').map_or(range.start, |newline| newline + 1);
            let start = if text[start..range.start].trim().is_empty() { start } else { range.start };
            format!("{}{block}{}", &text[..start], &text[range.end..])
        }
        None => match project.last_child().map(|_| text[..project.range().end].rfind("</")) {
            Some(Some(end)) => {
                let start = text[..end].rfind('\n').map_or(end, |newline| newline + 1);
                let start = if text[start..end].trim().is_empty() { start } else { end };
                format!("{}{block}\n{}", &text[..start], &text[end..])
            }
            _ => return Err("encodings.xml has no <project> element to write into".to_owned()),
        },
    })
}

fn url_of(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if relative.as_os_str().is_empty() => format!("file://{}", xml::PROJECT_DIR),
        Ok(relative) => format!("file://{}/{}", xml::PROJECT_DIR, relative.display()),
        Err(_) => format!("file://{}", path.display()),
    }
}

fn escaped(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn macros() -> Macros {
        Macros::new(Path::new("/work/shop"), Some(Path::new("/home/ann")))
    }

    #[test]
    fn files_the_project_default_and_the_bom_are_read_with_macros() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="Encoding" defaultCharsetForPropertiesFiles="ISO-8859-1" addBOMForNewFiles="with BOM">
    <file url="file://$PROJECT_DIR$/legacy/a.go" charset="windows-1251" />
    <file url="file://$USER_HOME$/notes" charset="KOI8-R" />
    <file url="file:///abs/x.txt" charset=" UTF-16 " />
    <file url="file://$PROJECT_DIR$/no-charset.txt" />
    <file url="PROJECT" charset="UTF-8" />
  </component>
</project>"#;
        let (encodings, messages) = parse(text, &macros());
        assert!(messages.is_empty(), "{messages:?}");
        let file = |path: &str, charset: &str| FileEncoding { path: PathBuf::from(path), charset: charset.to_owned() };
        assert_eq!(
            encodings,
            Encodings {
                default: Some("UTF-8".to_owned()),
                files: vec![
                    file("/work/shop/legacy/a.go", "windows-1251"),
                    file("/home/ann/notes", "KOI8-R"),
                    file("/abs/x.txt", "UTF-16"),
                ],
                bom_for_new_files: Some(NewFileBom::With),
            }
        );
    }

    #[test]
    fn missing_unknown_and_broken_parts_are_left_out() {
        let none = parse(r#"<project version="4"><component name="Encoding" /></project>"#, &macros());
        assert_eq!(none, (Encodings::default(), Vec::new()));
        assert_eq!(parse("<project/>", &macros()), (Encodings::default(), Vec::new()));
        let odd = r#"<component name="Encoding" addBOMForNewFiles="sometimes">
            <file url="jar://$PROJECT_DIR$/lib.jar!/" charset="UTF-8" /></component>"#;
        let (encodings, messages) = parse(odd, &macros());
        assert_eq!(encodings, Encodings::default());
        assert_eq!(messages.len(), 2, "{messages:?}");
        let no_bom = parse(r#"<component name="Encoding" addBOMForNewFiles="with NO BOM" />"#, &macros());
        assert_eq!(no_bom.0.bom_for_new_files, Some(NewFileBom::Without));
        let windows = parse(
            r#"<component name="Encoding" addBOMForNewFiles="with BOM under Windows, with no BOM otherwise" />"#,
            &macros(),
        );
        assert_eq!(windows.0.bom_for_new_files, Some(NewFileBom::OnWindows));
        let broken = parse("<project", &macros());
        assert_eq!(broken.0, Encodings::default());
        assert!(broken.1[0].starts_with("cannot parse encodings.xml"));
    }

    fn saved() -> Encodings {
        Encodings {
            default: Some("windows-1251".to_owned()),
            files: vec![FileEncoding { path: PathBuf::from("/work/shop/a&b/x.txt"), charset: "KOI8-R".to_owned() }],
            bom_for_new_files: Some(NewFileBom::OnWindows),
        }
    }

    #[test]
    fn a_new_file_is_written_in_the_shared_format_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut encodings = saved();
        encodings.files[0].path = root.join("a&b/x.txt");
        write(root, None, &encodings).unwrap();
        let text = std::fs::read_to_string(root.join(ENCODINGS)).unwrap();
        assert_eq!(
            text,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project version=\"4\">\n  <component name=\"Encoding\" \
             addBOMForNewFiles=\"with BOM under Windows, with no BOM otherwise\">\n    <file \
             url=\"file://$PROJECT_DIR$/a&amp;b/x.txt\" charset=\"KOI8-R\" />\n    <file url=\"PROJECT\" \
             charset=\"windows-1251\" />\n  </component>\n</project>\n"
        );
        let (read, messages) = parse(&text, &Macros::new(root, None));
        assert!(messages.is_empty(), "{messages:?}");
        assert_eq!(read, encodings);
    }

    #[test]
    fn only_the_encoding_component_changes_and_foreign_entries_stay() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="Before"><x /></component>
  <component name="Encoding" defaultCharsetForPropertiesFiles="ISO-8859-1" addBOMForNewFiles="with BOM">
    <file url="jar://$PROJECT_DIR$/lib.jar!/" charset="UTF-8" />
    <file url="file://$PROJECT_DIR$/old.txt" charset="UTF-8" />
  </component>
  <component name="After" />
</project>
"#;
        let encodings = Encodings { default: None, files: Vec::new(), bom_for_new_files: None };
        let written = with_component(text, &macros(), Path::new("/work/shop"), &encodings).unwrap();
        assert_eq!(
            written,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="Before"><x /></component>
  <component name="Encoding" defaultCharsetForPropertiesFiles="ISO-8859-1">
    <file url="jar://$PROJECT_DIR$/lib.jar!/" charset="UTF-8" />
  </component>
  <component name="After" />
</project>
"#
        );
    }

    #[test]
    fn a_file_without_the_component_gets_it_before_the_end_and_a_broken_one_is_left_alone() {
        let text = "<project version=\"4\">\n  <component name=\"Other\" />\n</project>\n";
        let written = with_component(text, &macros(), Path::new("/work/shop"), &Encodings::default()).unwrap();
        assert_eq!(
            written,
            "<project version=\"4\">\n  <component name=\"Other\" />\n  <component name=\"Encoding\" />\n</project>\n"
        );
        assert!(with_component("<project", &macros(), Path::new("/work/shop"), &Encodings::default()).is_err());
    }
}
