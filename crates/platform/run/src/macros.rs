use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A built-in macro, written between dollar signs: `$FilePath$`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Macro {
    /// The absolute path of the current file.
    FilePath,
    /// The folder of the current file.
    FileDir,
    /// The name of the current file with its extension.
    FileName,
    /// The name of the current file without its extension.
    FileNameWithoutExtension,
    /// The project's root folder.
    ProjectFileDir,
    /// The project's name: its root folder's name.
    ProjectName,
    /// The text selected in the editor.
    SelectedText,
    /// The caret's line, from 1.
    LineNumber,
    /// The caret's column, from 1.
    ColumnNumber,
    /// A text asked for when the run starts.
    Prompt,
    /// A file asked for when the run starts.
    FilePrompt,
    /// The text on the clipboard.
    ClipboardContent,
    /// The Go SDK's folder.
    GoRoot,
    /// The Go workspace folder.
    GoPath,
    /// The `go` program of the Go SDK.
    GoExecPath,
    /// The import path of the current file's Go package.
    GoFileImportPath,
    /// The operating system's name.
    OsName,
    /// The system's folder for temporary files.
    TempDir,
}

impl Macro {
    pub const ALL: [Macro; 18] = [
        Macro::ClipboardContent,
        Macro::ColumnNumber,
        Macro::FileDir,
        Macro::FileName,
        Macro::FileNameWithoutExtension,
        Macro::FilePath,
        Macro::FilePrompt,
        Macro::GoExecPath,
        Macro::GoFileImportPath,
        Macro::GoPath,
        Macro::GoRoot,
        Macro::LineNumber,
        Macro::OsName,
        Macro::ProjectFileDir,
        Macro::ProjectName,
        Macro::Prompt,
        Macro::SelectedText,
        Macro::TempDir,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Macro::FilePath => "FilePath",
            Macro::FileDir => "FileDir",
            Macro::FileName => "FileName",
            Macro::FileNameWithoutExtension => "FileNameWithoutExtension",
            Macro::ProjectFileDir => "ProjectFileDir",
            Macro::ProjectName => "ProjectName",
            Macro::SelectedText => "SelectedText",
            Macro::LineNumber => "LineNumber",
            Macro::ColumnNumber => "ColumnNumber",
            Macro::Prompt => "Prompt",
            Macro::FilePrompt => "FilePrompt",
            Macro::ClipboardContent => "ClipboardContent",
            Macro::GoRoot => "GOROOT",
            Macro::GoPath => "GOPATH",
            Macro::GoExecPath => "GoExecPath",
            Macro::GoFileImportPath => "GoFileImportPath",
            Macro::OsName => "OSName",
            Macro::TempDir => "TempDir",
        }
    }

    pub fn token(self) -> String {
        format!("${}$", self.name())
    }

    pub fn from_name(name: &str) -> Option<Macro> {
        Macro::ALL.into_iter().find(|known| known.name() == name)
    }

    pub const fn needs_file(self) -> bool {
        matches!(
            self,
            Macro::FilePath
                | Macro::FileDir
                | Macro::FileName
                | Macro::FileNameWithoutExtension
                | Macro::SelectedText
                | Macro::LineNumber
                | Macro::ColumnNumber
                | Macro::GoFileImportPath
        )
    }

    pub const fn is_prompt(self) -> bool {
        matches!(self, Macro::Prompt | Macro::FilePrompt)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MacroContext {
    pub file: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub selection: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub clipboard: Option<String>,
    pub goroot: Option<PathBuf>,
    pub gopath: Option<PathBuf>,
    pub prompt: Option<String>,
    pub file_prompt: Option<PathBuf>,
    pub go_import_path: Option<String>,
    pub temp_dir: Option<PathBuf>,
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}

impl MacroContext {
    pub fn value(&self, item: Macro) -> Option<String> {
        let file = self.file.as_deref();
        match item {
            Macro::FilePath => file.map(shown),
            Macro::FileDir => file.and_then(Path::parent).map(shown),
            Macro::FileName => file.and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()),
            Macro::FileNameWithoutExtension => {
                file.and_then(Path::file_stem).map(|name| name.to_string_lossy().into_owned())
            }
            Macro::ProjectFileDir => self.project.as_deref().map(shown),
            Macro::ProjectName => {
                self.project.as_deref().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned())
            }
            Macro::SelectedText => self.selection.clone().filter(|text| !text.is_empty()),
            Macro::LineNumber => self.line.filter(|_| file.is_some()).map(|line| line.to_string()),
            Macro::ColumnNumber => self.column.filter(|_| file.is_some()).map(|column| column.to_string()),
            Macro::Prompt => self.prompt.clone(),
            Macro::FilePrompt => self.file_prompt.as_deref().map(shown),
            Macro::ClipboardContent => self.clipboard.clone(),
            Macro::GoRoot => self.goroot.as_deref().map(shown),
            Macro::GoPath => self.gopath.as_deref().map(shown),
            Macro::GoExecPath => self
                .goroot
                .as_deref()
                .map(|root| shown(&root.join("bin").join(format!("go{}", std::env::consts::EXE_SUFFIX)))),
            Macro::GoFileImportPath => self.go_import_path.clone().filter(|_| file.is_some()),
            Macro::OsName => Some(std::env::consts::OS.to_owned()),
            Macro::TempDir => self.temp_dir.as_deref().map(shown),
        }
    }
}

pub fn go_import_path(dir: &Path) -> Option<String> {
    let mut parts = Vec::new();
    let mut at = dir;
    loop {
        if let Ok(text) = std::fs::read_to_string(at.join("go.mod")) {
            let module = text.lines().find_map(|line| {
                let rest = line.trim().strip_prefix("module").filter(|rest| rest.starts_with(char::is_whitespace))?;
                let module = rest.split("//").next().unwrap_or_default().trim().trim_matches('"');
                (!module.is_empty()).then(|| module.to_owned())
            })?;
            parts.push(module);
            parts.reverse();
            return Some(parts.join("/"));
        }
        parts.push(at.file_name()?.to_string_lossy().into_owned());
        at = at.parent()?;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece<'a> {
    Text(&'a str),
    Macro(Macro),
}

pub fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    let mut plain = 0;
    let mut at = 0;
    while let Some(start) = text[at..].find('$').map(|found| at + found) {
        let Some(end) = text[start + 1..].find('$').map(|found| start + 1 + found) else { break };
        match Macro::from_name(&text[start + 1..end]) {
            Some(item) => {
                if plain < start {
                    pieces.push(Piece::Text(&text[plain..start]));
                }
                pieces.push(Piece::Macro(item));
                plain = end + 1;
                at = end + 1;
            }
            None => at = end,
        }
    }
    if plain < text.len() {
        pieces.push(Piece::Text(&text[plain..]));
    }
    pieces
}

pub fn used(text: &str) -> Vec<Macro> {
    let mut found = Vec::new();
    for piece in pieces(text) {
        if let Piece::Macro(item) = piece
            && !found.contains(&item)
        {
            found.push(item);
        }
    }
    found
}

pub fn expand(text: &str, context: &MacroContext) -> Result<String, Macro> {
    let mut out = String::with_capacity(text.len());
    for piece in pieces(text) {
        match piece {
            Piece::Text(plain) => out.push_str(plain),
            Piece::Macro(item) => out.push_str(&context.value(item).ok_or(item)?),
        }
    }
    Ok(out)
}

pub fn preview(text: &str, context: &MacroContext) -> String {
    let mut out = String::with_capacity(text.len());
    for piece in pieces(text) {
        match piece {
            Piece::Text(plain) => out.push_str(plain),
            Piece::Macro(item) => match context.value(item) {
                Some(value) => out.push_str(&value),
                None => {
                    out.push('<');
                    out.push_str(item.name());
                    out.push('>');
                }
            },
        }
    }
    out
}

pub fn expand_arguments(text: &str, context: &MacroContext) -> Result<Vec<String>, Macro> {
    crate::args::split(text).iter().map(|word| expand(word, context)).collect()
}

pub fn expand_line(text: &str, context: &MacroContext) -> Result<String, Macro> {
    if used(text).is_empty() {
        return Ok(text.to_owned());
    }
    let words = expand_arguments(text, context)?;
    Ok(words.iter().map(|word| crate::external_tool::quoted(word)).collect::<Vec<_>>().join(" "))
}

pub fn preview_arguments(text: &str, context: &MacroContext) -> Vec<String> {
    crate::args::split(text).iter().map(|word| preview(word, context)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> MacroContext {
        MacroContext {
            file: Some(PathBuf::from("/work/my app/src/main file.go")),
            project: Some(PathBuf::from("/work/my app")),
            selection: Some("say \"hi\" 'there'".to_owned()),
            line: Some(12),
            column: Some(5),
            clipboard: Some("from clipboard".to_owned()),
            goroot: Some(PathBuf::from("/usr/local/go")),
            gopath: Some(PathBuf::from("/home/me/go")),
            prompt: Some("typed in".to_owned()),
            file_prompt: Some(PathBuf::from("/tmp/chosen.txt")),
            go_import_path: Some("example.com/app/src".to_owned()),
            temp_dir: Some(PathBuf::from("/var/tmp")),
        }
    }

    #[test]
    fn every_macro_has_its_value() {
        let context = context();
        let expected = [
            ("FilePath", "/work/my app/src/main file.go"),
            ("FileDir", "/work/my app/src"),
            ("FileName", "main file.go"),
            ("FileNameWithoutExtension", "main file"),
            ("ProjectFileDir", "/work/my app"),
            ("ProjectName", "my app"),
            ("SelectedText", "say \"hi\" 'there'"),
            ("LineNumber", "12"),
            ("ColumnNumber", "5"),
            ("Prompt", "typed in"),
            ("FilePrompt", "/tmp/chosen.txt"),
            ("ClipboardContent", "from clipboard"),
            ("GOROOT", "/usr/local/go"),
            ("GOPATH", "/home/me/go"),
            ("GoExecPath", &format!("/usr/local/go/bin/go{}", std::env::consts::EXE_SUFFIX)),
            ("GoFileImportPath", "example.com/app/src"),
            ("OSName", std::env::consts::OS),
            ("TempDir", "/var/tmp"),
        ];
        assert_eq!(expected.len(), Macro::ALL.len());
        let names: Vec<String> = Macro::ALL.iter().map(|item| item.name().to_lowercase()).collect();
        assert!(names.is_sorted(), "{names:?}");
        for (name, value) in expected {
            let item = Macro::from_name(name).unwrap();
            assert_eq!(expand(&item.token(), &context).unwrap(), value, "{name}");
        }
    }

    #[test]
    fn the_import_path_comes_from_the_nearest_go_mod() {
        let root = std::env::temp_dir().join(format!("zen-run-import-path-{}", std::process::id()));
        let nested = root.join("internal").join("parse");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("go.mod"), "// a module\nmodule example.com/tool // the tool\n\ngo 1.22\n").unwrap();
        assert_eq!(go_import_path(&root).as_deref(), Some("example.com/tool"));
        assert_eq!(go_import_path(&nested).as_deref(), Some("example.com/tool/internal/parse"));
        std::fs::write(root.join("go.mod"), "go 1.22\n").unwrap();
        assert_eq!(go_import_path(&nested), None, "a go.mod without a module");
        std::fs::remove_dir_all(&root).unwrap();
        let empty = MacroContext { go_import_path: Some("x".to_owned()), ..MacroContext::default() };
        assert_eq!(empty.value(Macro::GoFileImportPath), None, "no import path without a file");
        assert_eq!(empty.value(Macro::GoExecPath), None, "no go without GOROOT");
    }

    #[test]
    fn unknown_names_and_lone_dollars_stay() {
        let context = context();
        assert_eq!(expand("$Unknown$ and $5 $FileName$", &context).unwrap(), "$Unknown$ and $5 main file.go");
        assert_eq!(expand("cost $$ $ProjectName$$", &context).unwrap(), "cost $$ my app$");
        assert_eq!(expand("plain", &context).unwrap(), "plain");
        assert_eq!(used("$FileDir$ $Prompt$ $FileDir$"), [Macro::FileDir, Macro::Prompt]);
    }

    #[test]
    fn a_macro_without_a_value_is_the_error() {
        let empty = MacroContext { project: Some(PathBuf::from("/p")), ..MacroContext::default() };
        assert_eq!(expand("$ProjectName$ $FilePath$ $LineNumber$", &empty), Err(Macro::FilePath));
        assert_eq!(
            expand("$SelectedText$", &MacroContext { selection: Some(String::new()), ..empty.clone() }),
            Err(Macro::SelectedText)
        );
        assert_eq!(expand("$Prompt$", &empty), Err(Macro::Prompt));
        let caret_only = MacroContext { line: Some(3), ..empty.clone() };
        assert_eq!(expand("$LineNumber$", &caret_only), Err(Macro::LineNumber), "no line without a file");
        assert_eq!(preview("$ProjectName$:$FilePath$", &empty), "p:<FilePath>");
    }

    #[test]
    fn values_with_spaces_and_quotes_stay_one_argument() {
        let context = context();
        let args = expand_arguments("-n $LineNumber$ \"$FilePath$\" $SelectedText$ --dir=$FileDir$", &context).unwrap();
        assert_eq!(args, ["-n", "12", "/work/my app/src/main file.go", "say \"hi\" 'there'", "--dir=/work/my app/src"]);
        assert_eq!(expand_arguments("'$FileName$' x", &context).unwrap(), ["main file.go", "x"]);
        assert_eq!(preview_arguments("$FilePath$ a", &MacroContext::default()), ["<FilePath>", "a"]);
    }

    #[test]
    fn a_line_with_macros_splits_back_into_the_expanded_words() {
        let context = context();
        let line = expand_line("-n $LineNumber$ $FilePath$ --sel=$SelectedText$", &context).unwrap();
        assert_eq!(
            crate::args::split(&line),
            ["-n", "12", "/work/my app/src/main file.go", "--sel=say \"hi\" 'there'"]
        );
        assert_eq!(expand_line("  'kept  as' typed ", &context).unwrap(), "  'kept  as' typed ");
        assert_eq!(expand_line("$Prompt$", &MacroContext::default()), Err(Macro::Prompt));
    }
}
