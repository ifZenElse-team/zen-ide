use std::time::{Duration, Instant};

use super::*;

fn check(group: &str, cases: &[(&str, &str, bool)]) {
    let failed: Vec<String> = cases
        .iter()
        .filter(|(glob, path, expected)| section_matches(glob, path) != *expected)
        .map(|(glob, path, expected)| format!("[{glob}] {path}: expected {expected}"))
        .collect();
    assert!(failed.is_empty(), "{group}:\n{}", failed.join("\n"));
}

fn properties_of(text: &str, relative: &str) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    for (name, value) in File::parse(text).properties_for(relative) {
        if value == UNSET {
            properties.remove(name);
        } else {
            properties.insert(name.clone(), value.clone());
        }
    }
    properties
}

fn pairs(list: &[(&str, &str)]) -> BTreeMap<String, String> {
    list.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

#[test]
fn star_and_question_mark() {
    check(
        "star",
        &[
            ("a*e.c", "ace.c", true),
            ("a*e.c", "abcde.c", true),
            ("a*e.c", "ae.c", true),
            ("a*e.c", "a/e.c", false),
            ("Bar/*", "Bar/foo.txt", true),
            ("Bar/*", "Bar/foo/bar.txt", false),
            ("*", "deep/er/x", true),
            ("som?.c", "some.c", true),
            ("som?.c", "som.c", false),
            ("som?.c", "som/.c", false),
        ],
    );
}

#[test]
fn double_star_crosses_folders() {
    check(
        "star star",
        &[
            ("a**z.c", "a/z.c", true),
            ("a**z.c", "amnz.c", true),
            ("a**z.c", "am/nz.c", true),
            ("a**z.c", "a/mnz.c", true),
            ("a**z.c", "amn/z.c", true),
            ("a**z.c", "a/mn/z.c", true),
            ("b/**z.c", "b/z.c", true),
            ("b/**z.c", "b/mnz.c", true),
            ("b/**z.c", "b/mn/z.c", true),
            ("b/**z.c", "bmnz.c", false),
            ("c**/z.c", "c/z.c", true),
            ("c**/z.c", "cmn/z.c", true),
            ("c**/z.c", "c/mn/z.c", true),
            ("c**/z.c", "cmnz.c", false),
            ("c**/z.c", "cz.c", false),
            ("d/**/z.c", "d/z.c", true),
            ("d/**/z.c", "d/mn/z.c", true),
            ("d/**/z.c", "d/m/n/z.c", true),
            ("d/**/z.c", "dmn/z.c", false),
            ("d/**/z.c", "d/mnz.c", false),
            ("**/z.c", "z.c", true),
            ("**/z.c", "a/b/z.c", true),
            ("/**/z.c", "z.c", true),
            ("e/**/**/z.c", "e/z.c", true),
            ("e/**/**/z.c", "e/a/b/z.c", true),
            ("**.go", "a/b/x.go", true),
            ("src/**", "src/a/b.go", true),
            ("src/**", "other/a.go", false),
        ],
    );
}

#[test]
fn brackets() {
    check(
        "brackets",
        &[
            ("[ab].a", "a.a", true),
            ("[ab].a", "c.a", false),
            ("[!ab].b", "c.b", true),
            ("[!ab].b", "a.b", false),
            ("[^ab].b", "a.b", false),
            ("[d-g].c", "f.c", true),
            ("[d-g].c", "h.c", false),
            ("[!d-g].d", "h.d", true),
            ("[!d-g].d", "f.d", false),
            ("[abd-g].e", "e.e", true),
            ("[abd-g].e", "c.e", false),
            ("[-ab].f", "-.f", true),
            ("[ab-].f", "-.f", true),
            ("[\\]ab].g", "].g", true),
            ("[\\]ab].g", "\\.g", false),
            ("[ab\\]].g", "].g", true),
            ("[!\\]ab].g", "c.g", true),
            ("[!\\]ab].g", "].g", false),
            ("[ab]].g", "b].g", true),
            ("[!ab]].g", "c].g", true),
            ("[!ab]].g", "a].g", false),
            ("[a\\-c].h", "-.h", true),
            ("[a\\-c].h", "b.h", false),
            ("ab[e/]cd.i", "ab[e/]cd.i", true),
            ("ab[e/]cd.i", "ab/cd.i", false),
            ("ab[e/]cd.i", "abecd.i", false),
            ("ab[/c", "ab[/c", true),
            ("[*.j", "[x.j", true),
            ("[].k", "[].k", true),
            ("x.[!/]", "x./", false),
        ],
    );
}

#[test]
fn braces() {
    let nested = "{some,a{*c,b}[ef]}.j";
    check(
        "braces",
        &[
            ("*.{py,js,html}", "test.py", true),
            ("*.{py,js,html}", "test.js", true),
            ("*.{py,js,html}", "test.html", true),
            ("*.{py,js,html}", "test.pyc", false),
            ("{single}.b", "{single}.b", true),
            ("{single}.b", "single.b", false),
            ("{}.c", "{}.c", true),
            ("a{b,c,}.d", "a.d", true),
            ("a{b,c,}.d", "ab.d", true),
            ("{.f", "{.f", true),
            ("{word,{also},this}.g", "word.g", true),
            ("{word,{also},this}.g", "{also}.g", true),
            ("{word,{also},this}.g", "this.g", true),
            ("{word,{also},this}.g", "also.g", false),
            ("{},b}.h", "{},b}.h", true),
            ("{{,b,c{d}.i", "{{,b,c{d}.i", true),
            ("{{,b,c{d}.i", "{.i", false),
            ("{a\\,b,cd}", "a,b", true),
            ("{a\\,b,cd}", "cd", true),
            ("{a\\,b,cd}", "a", false),
            ("{e,\\},f}", "}", true),
            ("{e,\\},f}", "e", true),
            ("{g,\\\\,i}", "\\", true),
            ("{a,{b,c}}.go", "c.go", true),
            ("{*.json,.prettierrc}", ".prettierrc", true),
            ("{*.json,.prettierrc}", "a/b.json", true),
            ("{src,lib}/**/*.rs", "lib/a/b.rs", true),
            ("{src,lib}/**/*.rs", "lib/b.rs", true),
            ("{src,lib}/**/*.rs", "other/lib/b.rs", false),
            (nested, "some.j", true),
            (nested, "abe.j", true),
            (nested, "abf.j", true),
            (nested, "abg.j", false),
            (nested, "ace.j", true),
            (nested, "acf.j", true),
            (nested, "acg.j", false),
            (nested, "abce.j", true),
            (nested, "abcf.j", true),
            (nested, "abcg.j", false),
            (nested, "ae.j", false),
            (nested, ".j", false),
        ],
    );
}

#[test]
fn numeric_ranges() {
    check(
        "numbers",
        &[
            ("{3..120}", "1", false),
            ("{3..120}", "3", true),
            ("{3..120}", "15", true),
            ("{3..120}", "60", true),
            ("{3..120}", "5a", false),
            ("{3..120}", "120", true),
            ("{3..120}", "121", false),
            ("{3..120}", "060", false),
            ("{120..3}", "60", true),
            ("{0..3}", "0", true),
            ("file{1..3}.txt", "file2.txt", true),
            ("file{1..3}.txt", "file4.txt", false),
            ("file{1..3}.txt", "file.txt", false),
            ("v{-2..12}", "v-1", true),
            ("v{-2..12}", "v-3", false),
            ("{-10..-3}", "-10", true),
            ("{-10..-3}", "-5", true),
            ("{-10..-3}", "-2", false),
            ("{-10..-3}", "5", false),
            ("{1..3}{5..7}", "26", true),
            ("{1..3}{5..7}", "28", false),
            ("x{1..3}", "pkg/x2", true),
            ("{aardvark..antelope}", "{aardvark..antelope}", true),
            ("{aardvark..antelope}", "aardvark", false),
            ("{1..3,7}", "7", true),
        ],
    );
}

#[test]
fn escapes_and_backslashes() {
    check(
        "escapes",
        &[
            ("\\*.go", "*.go", true),
            ("\\*.go", "a.go", false),
            ("\\?.go", "?.go", true),
            ("\\?.go", "a.go", false),
            ("\\[ab].go", "[ab].go", true),
            ("\\[ab].go", "a.go", false),
            ("\\{a,b}.go", "{a,b}.go", true),
            ("\\{a,b}.go", "a.go", false),
            ("a\\*.go", "a*.go", true),
            ("a\\*.go", "ab.go", false),
            ("src\\*.cs", "src*.cs", true),
            ("src\\*.cs", "src/a.cs", false),
            ("src\\sub\\*.cs", "src/sub/a.cs", false),
            ("src\\sub\\*.cs", "srcsub*.cs", true),
            ("abc\\", "abc\\", true),
            ("\\\\x", "\\x", true),
        ],
    );
}

#[test]
fn anchoring_dotfiles_spaces_unicode_and_case() {
    check(
        "paths",
        &[
            ("/a.txt", "a.txt", true),
            ("/a.txt", "b/a.txt", false),
            ("a/b.txt", "a/b.txt", true),
            ("a/b.txt", "x/a/b.txt", false),
            ("b.txt", "x/y/b.txt", true),
            ("*", ".gitignore", true),
            ("*.md", ".hidden.md", true),
            (".*", "sub/.env", true),
            ("*", "sub/.git/config", true),
            ("my file.txt", "dir with space/my file.txt", true),
            ("{a b,c}.txt", "a b.txt", true),
            ("dir with space/*.txt", "dir with space/x.txt", true),
            ("файл*.go", "пакет/файл_тест.go", true),
            ("[а-я]*.txt", "ж.txt", true),
            ("[а-я]*.txt", "z.txt", false),
            ("*.😀", "x.😀", true),
            ("?.txt", "é.txt", true),
            ("*.GO", "a.go", false),
            ("*.go", "a.GO", false),
            ("Makefile", "build/Makefile", true),
            ("makefile", "build/Makefile", false),
        ],
    );
}

#[test]
fn the_parser_reads_comments_case_crlf_and_a_bom() {
    let text = "\u{feff}root = TRUE\r\n# a comment\r\n; another\r\n  # indented\r\n\t; tabbed\r\n\r\n[*.GO]\r\n\
                INDENT_STYLE = Tab\r\nIndent_Size=Tab\r\nKey = Value = More\r\n#[*.go]\r\n";
    let file = File::parse(text);
    assert!(file.root, "root after a BOM and in upper case");
    assert!(file.problems.is_empty(), "{:?}", file.problems);
    assert_eq!(file.sections.len(), 1, "a commented-out section is a comment");
    assert_eq!(file.sections[0].glob, "*.GO", "the glob keeps its case");
    assert_eq!(
        file.sections[0].properties,
        [("indent_style", "tab"), ("indent_size", "tab"), ("key", "value = more")]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
    );
    assert!(properties_of(text, "a.go").is_empty(), "globs are case-sensitive");
    assert_eq!(properties_of(text, "a.GO").get("indent_style").map(String::as_str), Some("tab"));

    let bom_section = File::parse("\u{feff}[*]\nindent_size = 2\n");
    assert!(bom_section.problems.is_empty(), "{:?}", bom_section.problems);
    assert_eq!(properties_of("\u{feff}[*]\nindent_size = 2\n", "a.txt"), pairs(&[("indent_size", "2")]));

    let inline = File::parse("[*]\nindent_size = 2 # two\nindent_style = space ; spaces\n[*.go] ; go\n");
    let lines: Vec<usize> = inline.problems.iter().map(|problem| problem.line).collect();
    assert_eq!(lines, vec![2, 3, 4], "a ; or # after other text is not a comment: {:?}", inline.problems);
}

#[test]
fn section_precedence_within_a_file() {
    let text = "[*]\nindent_size = 2\ntab_width = 8\n[*.go]\nindent_size = 4\n[*]\ntab_width = 3\n[main.go]\n\
                indent_size = unset\n";
    assert_eq!(properties_of(text, "a.go"), pairs(&[("indent_size", "4"), ("tab_width", "3")]));
    assert_eq!(properties_of(text, "main.go"), pairs(&[("tab_width", "3")]), "unset by a later section");
    assert_eq!(properties_of(text, "a.txt"), pairs(&[("indent_size", "2"), ("tab_width", "3")]));
}

#[test]
fn nested_files_and_root_true() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("a/b/c")).unwrap();
    let write = |name: &str, text: &str| std::fs::write(root.join(name), text).unwrap();
    write(".editorconfig", "root = true\n[*]\nindent_style = space\nindent_size = 2\nend_of_line = lf\n");
    write("a/.editorconfig", "[*.go]\nindent_style = tab\n[b/**]\nindent_size = 6\n");
    write("a/b/.editorconfig", "ROOT = True\n[*]\ncharset = utf-8\n");
    write("a/b/c/.editorconfig", "[*.go]\nindent_size = 8\n");
    assert_eq!(
        properties(root, &root.join("a/x.go")),
        pairs(&[("end_of_line", "lf"), ("indent_size", "2"), ("indent_style", "tab")])
    );
    assert_eq!(
        properties(root, &root.join("a/b/c/x.go")),
        pairs(&[("charset", "utf-8"), ("indent_size", "8")]),
        "root = true in a/b: nothing above it"
    );
    write("a/b/.editorconfig", "[*]\ncharset = utf-8\nroot = true\n");
    assert_eq!(
        properties(root, &root.join("a/b/c/x.go")),
        pairs(&[("charset", "utf-8"), ("end_of_line", "lf"), ("indent_size", "8"), ("indent_style", "tab")]),
        "root inside a section does not count; a/.editorconfig's [b/**] is overridden by the nearer file"
    );
    write("a/b/c/.editorconfig", "root = true\n");
    assert!(properties(root, &root.join("a/b/c/x.go")).is_empty(), "a root file without sections");
}

#[test]
fn long_lines_and_hostile_globs() {
    let glob = "a".repeat(5000);
    let value = "v".repeat(20_000);
    let text = format!("[{glob}]\n{} = {value}\n", "k".repeat(2000));
    let file = File::parse(&text);
    assert!(file.problems.is_empty());
    assert_eq!(properties_of(&text, &glob).get(&"k".repeat(2000)).map(String::len), Some(20_000));
    let deep = vec!["d"; 2000].join("/") + "/x.go";
    assert!(section_matches("**/x.go", &deep));
    assert!(section_matches("*.go", &deep));

    let started = Instant::now();
    let name = "a".repeat(200);
    assert!(!section_matches("*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*b", &name));
    assert!(!section_matches("**a**a**a**a**a**a**a**a**a**a**a**b", &name));
    let braces = "{a,aa,aaa}".repeat(30) + "b";
    assert!(!section_matches(&braces, &name));
    assert!(started.elapsed() < Duration::from_secs(60), "took {:?}", started.elapsed());
}
