use std::path::{Path, PathBuf};

use pulldown_cmark::{Options, Parser, html};

struct Page {
    slug: String,
    title: String,
    summary: String,
    body: String,
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let out = args.first().map_or_else(|| PathBuf::from("deploy/releases/site/docs"), PathBuf::from);
    let mut pages = read_guide(Path::new("docs/guide"))?;
    pages.push(shortcuts_page()?);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|error| format!("cannot create {}: {error}", out.display()))?;
    for page in &pages {
        let html = layout(&pages, Some(page), &format!("<h1>{}</h1>\n{}", escape(&page.title), page.body));
        write(&out.join(format!("{}.html", page.slug)), &html)?;
    }
    let mut index = String::from(
        "<h1>Documentation</h1>\n<p class=\"lead\">How to use GoGarden and Zen Toolbox. New here? Start with \
         <a href=\"getting-started.html\">Getting started</a>.</p>\n<div class=\"cards\">\n",
    );
    for page in &pages {
        index.push_str(&format!(
            "<a class=\"card\" href=\"{}.html\"><strong>{}</strong><span>{}</span></a>\n",
            page.slug,
            escape(&page.title),
            escape(&page.summary)
        ));
    }
    index.push_str("</div>\n");
    write(&out.join("index.html"), &layout(&pages, None, &index))?;
    println!("Wrote {} pages to {}.", pages.len() + 1, out.display());
    Ok(())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn read_guide(folder: &Path) -> Result<Vec<Page>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|error| format!("cannot read {}: {error}", folder.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "md")
                && path.file_name().is_some_and(|name| name.to_string_lossy().as_bytes()[0].is_ascii_digit())
        })
        .collect();
    files.sort();
    files
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
            let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let slug = stem.split_once('-').map_or(stem.as_str(), |(_, rest)| rest).to_owned();
            page(slug, &text).ok_or_else(|| format!("{}: the first line must be `# Title`", path.display()))
        })
        .collect()
}

fn page(slug: String, text: &str) -> Option<Page> {
    let (first, rest) = text.split_once('\n')?;
    let title = first.strip_prefix("# ")?.trim().to_owned();
    let summary = rest.trim_start().split("\n\n").next().unwrap_or_default().replace('\n', " ");
    let summary = summary.replace(['*', '`'], "");
    let mut body = String::new();
    html::push_html(&mut body, Parser::new_ext(rest, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH));
    Some(Page { slug, title, summary, body })
}

fn shortcuts_page() -> Result<Page, String> {
    let messages =
        std::fs::read_to_string("crates/platform/ui/locales/en/ui.ftl").map_err(|error| error.to_string())?;
    let message = |id: &str| -> Option<String> {
        messages.lines().find_map(|line| line.strip_prefix(&format!("{id} = ")).map(str::to_owned))
    };
    let name = |action: &str| -> String {
        let (base, extend) = match action.strip_suffix("+extend") {
            Some(base) => (base, true),
            None => (action, false),
        };
        let title = message(&format!("keymap-{}", base.replace(['.', '/'], "-"))).unwrap_or_else(|| action.to_owned());
        if extend { format!("{title} with Selection") } else { title }
    };
    let keymap = zen_core::keymap::Keymap::default_macos();
    let mut markdown = String::from(
        "The default keymap of GoGarden on macOS. Change any shortcut in Settings → Keymap.\n\n\
         Search Everywhere also opens with **⇧ pressed twice**.\n\n",
    );
    for (index, group) in zen_core::keymap::GROUPS.iter().enumerate() {
        let rows: Vec<String> = keymap
            .bindings()
            .into_iter()
            .filter(|binding| zen_core::keymap::group_of(&binding.action) == index && !binding.keystrokes.is_empty())
            .map(|binding| {
                let keys: Vec<String> = binding.keystrokes.iter().map(|key| format!("`{key}`")).collect();
                format!("| {} | {} |", name(&binding.action), keys.join(" "))
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        markdown.push_str(&format!(
            "## {}\n\n| Action | Shortcut |\n|---|---|\n{}\n\n",
            message(group).unwrap_or_default(),
            rows.join("\n")
        ));
    }
    page("keyboard-shortcuts".to_owned(), &format!("# Keyboard shortcuts\n\n{markdown}"))
        .ok_or_else(|| "the shortcuts page".to_owned())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn layout(pages: &[Page], current: Option<&Page>, content: &str) -> String {
    let title = current.map_or_else(|| "Documentation".to_owned(), |page| page.title.clone());
    let description =
        current.map_or_else(|| "How to use GoGarden and Zen Toolbox.".to_owned(), |page| page.summary.clone());
    let mut menu = String::from("<a href=\"index.html\"");
    if current.is_none() {
        menu.push_str(" aria-current=\"page\"");
    }
    menu.push_str(">Overview</a>\n");
    for page in pages {
        let here = current.is_some_and(|current| current.slug == page.slug);
        menu.push_str(&format!(
            "<a href=\"{}.html\"{}>{}</a>\n",
            page.slug,
            if here { " aria-current=\"page\"" } else { "" },
            escape(&page.title)
        ));
    }
    TEMPLATE
        .replace("{title}", &escape(&title))
        .replace("{description}", &escape(&description))
        .replace("{menu}", &menu)
        .replace("{content}", content)
}

const TEMPLATE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · Zen IDE</title>
<meta name="description" content="{description}">
<meta name="color-scheme" content="dark">
<meta name="theme-color" content="#0E0F15">
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' y1='0' x2='1' y2='1'%3E%3Cstop offset='0' stop-color='%238B7CF8'/%3E%3Cstop offset='1' stop-color='%234338CA'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='64' height='64' rx='18' fill='url(%23g)'/%3E%3Cpath d='M35 16.3A16 16 0 1 1 23.4 18.5' fill='none' stroke='%23fff' stroke-width='5.4' stroke-linecap='round'/%3E%3C/svg%3E">
<style>
/* Inter (SIL Open Font License 1.1, see /fonts/OFL-Inter.txt), served from this host. */
@font-face { font-family: "Inter"; src: url("/fonts/Inter-Regular.ttf") format("truetype"); font-weight: 400; font-display: swap; }
@font-face { font-family: "Inter"; src: url("/fonts/Inter-SemiBold.ttf") format("truetype"); font-weight: 600; font-display: swap; }
@font-face { font-family: "Inter"; src: url("/fonts/Inter-Bold.ttf") format("truetype"); font-weight: 700; font-display: swap; }
:root {
  --ink-top: #24203A; --ink-bottom: #0E0F15; --text: #EEEFF4; --text-2: #9AA0B0; --link: #A9ACFF;
  --panel: rgba(255, 255, 255, 0.04); --edge: rgba(255, 255, 255, 0.09);
  --font: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  --mono: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
}
* { box-sizing: border-box; }
html { background: var(--ink-bottom); }
body {
  margin: 0; min-height: 100vh; font-family: var(--font); font-size: 16px; line-height: 1.65; color: var(--text);
  background: radial-gradient(60rem 30rem at 50% -10rem, rgba(124, 127, 247, 0.16), transparent 70%),
    linear-gradient(180deg, var(--ink-top) 0%, #171824 30%, #12131B 70%, var(--ink-bottom) 100%);
  background-attachment: fixed; -webkit-font-smoothing: antialiased;
}
a { color: var(--link); text-underline-offset: 3px; }
a:hover { color: #C6C8FF; }
:focus-visible { outline: 2px solid var(--link); outline-offset: 3px; border-radius: 6px; }
.wrap { max-width: 1080px; margin: 0 auto; padding: 0 16px; }
header.top { display: flex; flex-wrap: wrap; gap: 8px 20px; align-items: baseline; padding: 28px 0 8px; }
header.top .brand { font-weight: 700; color: var(--text); text-decoration: none; font-size: 1.1rem; }
header.top nav a { color: var(--text-2); text-decoration: none; margin-right: 16px; font-size: 0.95rem; }
header.top nav a:hover, header.top nav a.on { color: var(--text); }
.layout { display: grid; grid-template-columns: 220px minmax(0, 1fr); gap: 40px; padding: 24px 0 64px; }
aside nav { position: sticky; top: 24px; display: flex; flex-direction: column; gap: 2px; }
aside nav a { padding: 6px 12px; border-radius: 8px; text-decoration: none; color: var(--text-2); font-size: 0.95rem; }
aside nav a:hover { color: var(--text); background: var(--panel); }
aside nav a[aria-current="page"] { color: var(--text); background: rgba(124, 127, 247, 0.2); }
main { min-width: 0; }
main h1 { margin: 0 0 12px; font-size: clamp(1.9rem, 5vw, 2.5rem); letter-spacing: -0.02em; line-height: 1.15; }
main h2 { margin: 36px 0 8px; font-size: 1.3rem; }
main h3 { margin: 24px 0 6px; font-size: 1.05rem; }
main p, main li { color: #DADCE4; }
main ul { padding-left: 1.3rem; }
main li { margin: 4px 0; }
main code { font-family: var(--mono); font-size: 0.88em; padding: 1px 6px; border-radius: 6px; background: rgba(8, 9, 14, 0.6); border: 1px solid var(--edge); overflow-wrap: anywhere; }
main pre { padding: 14px 16px; border-radius: 12px; background: rgba(8, 9, 14, 0.7); border: 1px solid var(--edge); overflow-x: auto; }
main pre code { padding: 0; border: 0; background: none; }
main table { width: 100%; border-collapse: collapse; margin: 12px 0; font-size: 0.95rem; }
main th, main td { text-align: left; padding: 8px 10px; border-bottom: 1px solid var(--edge); vertical-align: top; }
main th { color: var(--text-2); font-weight: 600; }
.lead { color: var(--text-2); font-size: 1.1rem; }
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; margin-top: 24px; }
.card { display: flex; flex-direction: column; gap: 6px; padding: 18px; border-radius: 14px; background: var(--panel); border: 1px solid var(--edge); text-decoration: none; }
.card strong { color: var(--text); }
.card span { color: var(--text-2); font-size: 0.92rem; line-height: 1.5; }
.card:hover { border-color: rgba(169, 172, 255, 0.4); }
footer { padding: 24px 0 40px; border-top: 1px solid var(--edge); color: var(--text-2); font-size: 0.85rem; }
@media (max-width: 760px) {
  .layout { grid-template-columns: 1fr; gap: 16px; }
  aside nav { position: static; flex-direction: row; flex-wrap: wrap; }
}
</style>
</head>
<body>
<div class="wrap">
  <header class="top">
    <a class="brand" href="/">Zen IDE</a>
    <nav><a class="on" href="/docs/">Documentation</a><a href="/changelog.html">Changelog</a></nav>
  </header>
  <div class="layout">
    <aside><nav aria-label="Documentation">
{menu}    </nav></aside>
    <main>
{content}
    </main>
  </div>
  <footer>Zen IDE is made by <strong>ifzenelse</strong>. This documentation is written with the code: every release that changes what you see updates it.</footer>
</div>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_has_a_title_a_summary_and_html() {
        let page = page(
            "editor".to_owned(),
            "# The editor\n\nTyping, **selecting** and `code`.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n",
        )
        .unwrap();
        assert_eq!(page.title, "The editor");
        assert_eq!(page.summary, "Typing, selecting and code.");
        assert!(page.body.contains("<table>"));
        assert!(page.body.contains("<strong>selecting</strong>"));
        assert!(super::page("x".to_owned(), "No title\n").is_none());
    }

    #[test]
    fn the_guide_builds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let pages = read_guide(&root.join("docs/guide")).unwrap();
        assert!(pages.iter().any(|page| page.slug == "getting-started"));
        let slugs: Vec<String> = pages.iter().map(|page| format!("{}.html", page.slug)).collect();
        for page in &pages {
            for link in page.body.split("href=\"").skip(1).filter_map(|rest| rest.split('"').next()) {
                if !link.contains("://") && link.ends_with(".html") {
                    assert!(
                        slugs.contains(&link.to_owned()) || link == "keyboard-shortcuts.html",
                        "{}: {link}",
                        page.slug
                    );
                }
            }
        }
    }
}
