use std::path::PathBuf;

use zen_theme::{EditorKey, Key, ResolvedTheme, SyntaxKey, TerminalKey, UiKey, format, zen_dark, zen_light};

const BEGIN: &str = "<!-- keys:begin -->\n";
const END: &str = "<!-- keys:end -->\n";

fn docs(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs").join(path)
}

fn blessing() -> bool {
    std::env::var_os("ZEN_THEME_BLESS").is_some_and(|value| value == "1")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(docs(path)).unwrap_or_else(|error| panic!("docs/{path}: {error}"))
}

fn check(path: &str, current: &str, expected: &str) {
    if blessing() {
        std::fs::write(docs(path), current).unwrap_or_else(|error| panic!("docs/{path}: {error}"));
    } else {
        assert!(
            current == expected,
            "docs/{path} is out of date; run `ZEN_THEME_BLESS=1 cargo test -p zen-theme --test docs`"
        );
    }
}

#[test]
fn the_schema_is_current() {
    let committed = if blessing() { String::new() } else { read("themes/theme.schema.json") };
    check("themes/theme.schema.json", &format::schema_json(), &committed);
}

#[test]
fn the_zen_dark_example_is_the_built_in_theme() {
    let written = zen_theme::to_json(&zen_dark());
    let committed = if blessing() { written.clone() } else { read("themes/zen-dark.json") };
    check("themes/zen-dark.json", &written, &committed);
    assert_eq!(zen_theme::parse(&committed), Ok(zen_dark()));
}

#[test]
fn the_zen_light_example_is_the_built_in_theme() {
    let written = zen_theme::to_json(&zen_light());
    let committed = if blessing() { written.clone() } else { read("themes/zen-light.json") };
    check("themes/zen-light.json", &written, &committed);
    assert_eq!(zen_theme::parse(&committed), Ok(zen_light()));
}

fn table<K: Key>(title: &str, value: impl Fn(K) -> (String, String)) -> String {
    let mut text =
        format!("### {title} (`{}`)\n\n| Key | Meaning | Zen Dark | Zen Light |\n|---|---|---|---|\n", K::SECTION);
    for key in K::ALL {
        let (dark, light) = value(*key);
        text.push_str(&format!("| `{}` | {} | `{dark}` | `{light}` |\n", key.id(), key.doc()));
    }
    text
}

#[test]
fn the_key_tables_list_every_key() {
    let (zen, light) = (ResolvedTheme::zen_dark(), ResolvedTheme::zen_light());
    let tables = [
        table::<UiKey>("Window colours", |key| (zen.ui(key).to_string(), light.ui(key).to_string())),
        table::<EditorKey>("Editor colours", |key| (zen.editor(key).to_string(), light.editor(key).to_string())),
        table::<SyntaxKey>("Code styles", |key| (zen.syntax(key).to_string(), light.syntax(key).to_string())),
        table::<TerminalKey>("Terminal colours", |key| {
            (zen.terminal(key).to_string(), light.terminal(key).to_string())
        }),
    ]
    .join("\n");
    let doc = read("themes.md");
    let start = doc.find(BEGIN).expect("docs/themes.md has the keys:begin marker") + BEGIN.len();
    let end = doc.find(END).expect("docs/themes.md has the keys:end marker");
    let generated = format!("\n{tables}\n");
    let updated = format!("{}{generated}{}", &doc[..start], &doc[end..]);
    check("themes.md", &updated, &doc);
}

#[test]
fn the_documented_example_is_a_theme() {
    let doc = read("themes.md");
    let start = doc.find("```jsonc\n").expect("docs/themes.md has a jsonc example") + "```jsonc\n".len();
    let example = &doc[start..start + doc[start..].find("```").expect("the example ends")];
    let theme = zen_theme::parse(example).unwrap_or_else(|error| panic!("the example: {error}"));
    assert_eq!(theme.id, "dusk");
    assert!(theme.unknown_keys.is_empty(), "{:?}", theme.unknown_keys);
    let resolved = theme.resolve(|_| None);
    assert!(resolved.warnings.is_empty(), "{:?}", resolved.warnings);
    assert_eq!(resolved.theme.terminal(TerminalKey::Ansi1).to_string(), "#F0524F");
}
