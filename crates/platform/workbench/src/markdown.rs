use std::collections::HashMap;
use std::fmt::Write as _;
use std::ops::Range;

pub mod commands;
pub mod format;
pub mod math;
pub mod mermaid;
pub mod selection;

pub use commands::{RemoteImages, host_of, register};
use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use schemars::JsonSchema;
use serde::Serialize;

/// A Markdown text read into blocks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Rendered {
    /// The blocks in reading order; the footnotes' last.
    pub blocks: Vec<Block>,
    /// The headings, for the outline.
    pub headings: Vec<Heading>,
}

/// A heading of the outline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Heading {
    /// From 1 to 6.
    pub level: u8,
    /// Its text.
    pub text: String,
    /// Its anchor, as GitHub makes them: `#getting-started`.
    pub anchor: String,
    /// Its line in the source, from 1.
    pub line: u32,
}

/// A block of the preview.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    /// What it is.
    #[serde(flatten)]
    pub kind: BlockKind,
    /// The first source line it comes from, from 1.
    pub line: u32,
    /// The last source line it comes from, from 1.
    pub end_line: u32,
    /// The quotes, list items and footnotes it is in, the outermost first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nesting: Vec<Container>,
}

/// What a block is.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum BlockKind {
    /// A heading.
    Heading {
        /// From 1 to 6.
        level: u8,
        /// GitHub's anchor of it, without `#`.
        anchor: String,
        /// Its text.
        inlines: Vec<Inline>,
    },
    /// A paragraph.
    Paragraph {
        /// Its text.
        inlines: Vec<Inline>,
    },
    /// A fenced or indented code block.
    Code {
        /// The language its fence names (`rust`, `mermaid`), empty when none.
        language: String,
        /// The code.
        text: String,
    },
    /// Display math (`$$…$$` or a fenced block labelled `math`), as TeX.
    Math {
        /// The TeX.
        text: String,
    },
    /// A table.
    Table {
        /// Each column's alignment.
        align: Vec<Align>,
        /// The header cells.
        head: Vec<Vec<Inline>>,
        /// The rows of cells.
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// A horizontal rule.
    Rule,
    /// The line above the footnotes.
    Footnotes,
}

/// A column's alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    /// As the text goes.
    None,
    /// Left.
    Left,
    /// Centred.
    Center,
    /// Right.
    Right,
}

/// A kind of GitHub alert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Alert {
    /// `[!NOTE]`
    Note,
    /// `[!TIP]`
    Tip,
    /// `[!IMPORTANT]`
    Important,
    /// `[!WARNING]`
    Warning,
    /// `[!CAUTION]`
    Caution,
}

/// What a block is in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Container {
    /// A block quote, or an alert.
    Quote {
        /// The alert it is, if it is one.
        #[serde(skip_serializing_if = "Option::is_none")]
        alert: Option<Alert>,
        /// Whether this block is the first of the quote (where an alert shows its title).
        first: bool,
    },
    /// A list item.
    Item {
        /// The bullet or number shown before the item's first block (`•`, `3.`); absent on its
        /// further blocks.
        #[serde(skip_serializing_if = "Option::is_none")]
        marker: Option<String>,
        /// The item's checkbox, on its first block, when it is a task.
        #[serde(skip_serializing_if = "Option::is_none")]
        task: Option<Task>,
    },
    /// A footnote's definition.
    Footnote {
        /// Its number, as its references show it.
        number: u32,
        /// Whether this block is the first of the footnote (where its number shows).
        first: bool,
    },
}

/// A task list item's checkbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Whether it is ticked.
    pub checked: bool,
    /// The source line of its `[ ]`, from 1.
    pub line: u32,
}

/// A run of text in one style.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Inline {
    /// The text; `\n` for a line break.
    pub text: String,
    /// Strong emphasis.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strong: bool,
    /// Emphasis.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub emphasis: bool,
    /// Struck through.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    /// Inline code (also `<kbd>`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub code: bool,
    /// Inline math, as TeX.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub math: bool,
    /// The address it links to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// An image: its address; `text` is its description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// A footnote reference: the footnote's number; `text` is the number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footnote: Option<u32>,
}

impl Inline {
    fn plain(text: &str) -> Self {
        Inline { text: text.to_owned(), ..Inline::default() }
    }
}

pub fn plain_text(inlines: &[Inline]) -> String {
    inlines.iter().map(|inline| inline.text.as_str()).collect()
}

pub fn slug(text: &str) -> String {
    text.trim()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

const DROPPED: &[&str] = &["script", "style", "iframe", "object", "embed", "noscript", "template", "frame", "frameset"];

#[derive(Clone, Default)]
struct Styles {
    strong: usize,
    emphasis: usize,
    strike: usize,
    code: usize,
    link: Vec<Option<String>>,
}

#[derive(Clone)]
struct Open {
    container: Container,
    fresh: bool,
}

struct Reader {
    line_starts: Vec<usize>,
    blocks: Vec<Block>,
    footnotes: Vec<Block>,
    headings: Vec<Heading>,
    anchors: HashMap<String, usize>,
    footnote_numbers: HashMap<String, u32>,
    open: Vec<Open>,
    lists: Vec<Option<u64>>,
    inlines: Vec<Inline>,
    leaf: Option<Leaf>,
    leaf_start: Option<usize>,
    leaf_end: usize,
    styles: Styles,
    image: Option<(String, String)>,
    code: Option<(String, String)>,
    table: Option<TableReader>,
    html: Option<String>,
    dropping: usize,
    in_footnote: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaf {
    Paragraph,
    Heading(u8),
}

struct TableReader {
    align: Vec<Align>,
    head: Vec<Vec<Inline>>,
    rows: Vec<Vec<Vec<Inline>>>,
    in_head: bool,
    start: usize,
}

impl Reader {
    fn new(text: &str) -> Self {
        let line_starts = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect();
        Reader {
            line_starts,
            blocks: Vec::new(),
            footnotes: Vec::new(),
            headings: Vec::new(),
            anchors: HashMap::new(),
            footnote_numbers: HashMap::new(),
            open: Vec::new(),
            lists: Vec::new(),
            inlines: Vec::new(),
            leaf: None,
            leaf_start: None,
            leaf_end: 0,
            styles: Styles::default(),
            image: None,
            code: None,
            table: None,
            html: None,
            dropping: 0,
            in_footnote: false,
        }
    }

    fn line(&self, at: usize) -> u32 {
        self.line_starts.partition_point(|&start| start <= at) as u32
    }

    fn lines(&self, range: &Range<usize>) -> (u32, u32) {
        let last = range.end.saturating_sub(1).max(range.start);
        (self.line(range.start), self.line(last))
    }

    fn footnote_number(&mut self, label: &str) -> u32 {
        let next = self.footnote_numbers.len() as u32 + 1;
        *self.footnote_numbers.entry(label.to_owned()).or_insert(next)
    }

    fn nesting(&mut self) -> Vec<Container> {
        let nesting = self
            .open
            .iter()
            .map(|open| match &open.container {
                Container::Quote { alert, .. } => Container::Quote { alert: *alert, first: open.fresh },
                Container::Footnote { number, .. } => Container::Footnote { number: *number, first: open.fresh },
                item => item.clone(),
            })
            .collect();
        for open in &mut self.open {
            open.fresh = false;
            if let Container::Item { marker, task } = &mut open.container {
                *marker = None;
                *task = None;
            }
        }
        nesting
    }

    fn emit(&mut self, kind: BlockKind, range: Range<usize>) {
        let (line, end_line) = self.lines(&range);
        let block = Block { kind, line, end_line, nesting: self.nesting() };
        if self.in_footnote { self.footnotes.push(block) } else { self.blocks.push(block) }
    }

    fn start_leaf(&mut self, leaf: Leaf, at: usize) {
        self.finish_leaf();
        self.leaf = Some(leaf);
        self.leaf_start = Some(at);
    }

    fn finish_leaf(&mut self) {
        let leaf = self.leaf.take();
        let inlines = std::mem::take(&mut self.inlines);
        let Some(start) = self.leaf_start.take() else { return };
        if inlines.is_empty() {
            return;
        }
        let range = start..self.leaf_end.max(start + 1);
        let kind = match leaf {
            Some(Leaf::Heading(level)) => {
                let text = plain_text(&inlines);
                let base = slug(&text);
                let seen = self.anchors.entry(base.clone()).or_insert(0);
                let anchor = if *seen == 0 { base.clone() } else { format!("{base}-{seen}") };
                *seen += 1;
                let line = self.line(start);
                self.headings.push(Heading { level, text, anchor: anchor.clone(), line });
                BlockKind::Heading { level, anchor, inlines }
            }
            _ => BlockKind::Paragraph { inlines },
        };
        self.emit(kind, range);
    }

    fn push_text(&mut self, text: &str, range: &Range<usize>, code: bool) {
        if self.dropping > 0 {
            return;
        }
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(text);
            return;
        }
        if self.table.is_some() {
            self.push_inline(Inline { text: text.to_owned(), code: code || self.styles.code > 0, ..self.styled() });
            return;
        }
        if self.leaf.is_none() {
            self.start_leaf(Leaf::Paragraph, range.start);
        }
        self.leaf_end = self.leaf_end.max(range.end);
        if code || self.styles.link.last().is_some_and(Option::is_some) {
            self.push_inline(Inline { text: text.to_owned(), code: code || self.styles.code > 0, ..self.styled() });
            return;
        }
        for (piece, link) in autolinks(text) {
            let mut inline = Inline { text: piece.to_owned(), ..self.styled() };
            if link.is_some() {
                inline.link = link;
            }
            self.push_inline(inline);
        }
    }

    fn styled(&self) -> Inline {
        Inline {
            strong: self.styles.strong > 0,
            emphasis: self.styles.emphasis > 0,
            strike: self.styles.strike > 0,
            code: self.styles.code > 0,
            link: self.styles.link.iter().rev().find_map(Clone::clone),
            ..Inline::default()
        }
    }

    fn push_inline(&mut self, inline: Inline) {
        if inline.text.is_empty() && inline.image.is_none() && inline.footnote.is_none() {
            return;
        }
        if let Some(table) = &mut self.table {
            let row = if table.in_head { None } else { table.rows.last_mut() };
            let cell = match row {
                Some(row) => row.last_mut(),
                None => table.head.last_mut(),
            };
            if let Some(cell) = cell {
                cell.push(inline);
            }
            return;
        }
        match self.inlines.last_mut() {
            Some(last)
                if last.text != "\n"
                    && inline.text != "\n"
                    && last.image.is_none()
                    && inline.image.is_none()
                    && last.footnote.is_none()
                    && inline.footnote.is_none()
                    && Inline { text: String::new(), ..last.clone() }
                        == Inline { text: String::new(), ..inline.clone() } =>
            {
                last.text.push_str(&inline.text);
            }
            _ => self.inlines.push(inline),
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(tag) => self.end(tag, range),
            Event::Text(text) => match &mut self.code {
                Some((_, code)) => code.push_str(&text),
                None => match &mut self.html {
                    Some(html) => html.push_str(&text),
                    None => self.push_text(&text, &range, false),
                },
            },
            Event::Code(text) => self.push_text(&text, &range, true),
            Event::InlineMath(text) => {
                self.push_text("", &range, false);
                self.push_inline(Inline { text: text.to_string(), math: true, ..Inline::default() });
            }
            Event::DisplayMath(text) => {
                if self.leaf.is_some() {
                    self.finish_leaf();
                }
                self.emit(BlockKind::Math { text: text.trim().to_owned() }, range);
            }
            Event::Html(html) => match &mut self.html {
                Some(block) => block.push_str(&html),
                None => self.inline_html(&html, &range),
            },
            Event::InlineHtml(html) => self.inline_html(&html, &range),
            Event::FootnoteReference(label) => {
                let number = self.footnote_number(&label);
                self.push_text("", &range, false);
                self.push_inline(Inline { text: number.to_string(), footnote: Some(number), ..Inline::default() });
            }
            Event::SoftBreak => self.push_text(" ", &range, false),
            Event::HardBreak => {
                self.push_text("", &range, false);
                self.push_inline(Inline::plain("\n"));
            }
            Event::Rule => {
                self.finish_leaf();
                self.emit(BlockKind::Rule, range);
            }
            Event::TaskListMarker(checked) => {
                let line = self.line(range.start);
                if let Some(Open { container: Container::Item { task, .. }, .. }) =
                    self.open.iter_mut().rev().find(|open| matches!(open.container, Container::Item { .. }))
                {
                    *task = Some(Task { checked, line });
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::Paragraph => self.start_leaf(Leaf::Paragraph, range.start),
            Tag::Heading { level, .. } => self.start_leaf(Leaf::Heading(level as u8), range.start),
            Tag::BlockQuote(kind) => {
                self.finish_leaf();
                let alert = kind.map(|kind| match kind {
                    BlockQuoteKind::Note => Alert::Note,
                    BlockQuoteKind::Tip => Alert::Tip,
                    BlockQuoteKind::Important => Alert::Important,
                    BlockQuoteKind::Warning => Alert::Warning,
                    BlockQuoteKind::Caution => Alert::Caution,
                });
                self.open.push(Open { container: Container::Quote { alert, first: true }, fresh: true });
            }
            Tag::CodeBlock(kind) => {
                self.finish_leaf();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or_default().to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
            }
            Tag::HtmlBlock => {
                self.finish_leaf();
                self.html = Some(String::new());
                self.leaf_start = Some(range.start);
                self.leaf_end = range.end;
            }
            Tag::List(first) => {
                self.finish_leaf();
                self.lists.push(first);
            }
            Tag::Item => {
                self.finish_leaf();
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.");
                        *number += 1;
                        marker
                    }
                    _ => ["•", "◦", "▪"][(self.lists.len().saturating_sub(1)) % 3].to_owned(),
                };
                self.open.push(Open { container: Container::Item { marker: Some(marker), task: None }, fresh: true });
            }
            Tag::FootnoteDefinition(label) => {
                self.finish_leaf();
                let number = self.footnote_number(&label);
                self.in_footnote = true;
                self.open.push(Open { container: Container::Footnote { number, first: true }, fresh: true });
            }
            Tag::Table(align) => {
                self.finish_leaf();
                let align = align
                    .into_iter()
                    .map(|align| match align {
                        Alignment::None => Align::None,
                        Alignment::Left => Align::Left,
                        Alignment::Center => Align::Center,
                        Alignment::Right => Align::Right,
                    })
                    .collect();
                self.table =
                    Some(TableReader { align, head: Vec::new(), rows: Vec::new(), in_head: false, start: range.start });
            }
            Tag::TableHead => {
                if let Some(table) = &mut self.table {
                    table.in_head = true;
                }
            }
            Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.rows.push(Vec::new());
                }
            }
            Tag::TableCell => {
                if let Some(table) = &mut self.table {
                    match (table.in_head, table.rows.last_mut()) {
                        (true, _) | (false, None) => table.head.push(Vec::new()),
                        (false, Some(row)) => row.push(Vec::new()),
                    }
                }
            }
            Tag::Emphasis => self.styles.emphasis += 1,
            Tag::Strong => self.styles.strong += 1,
            Tag::Strikethrough => self.styles.strike += 1,
            Tag::Link { dest_url, .. } => self.styles.link.push(Some(dest_url.to_string())),
            Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd, range: Range<usize>) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => {
                self.leaf_end = self.leaf_end.max(range.end);
                self.finish_leaf();
            }
            TagEnd::BlockQuote(_) | TagEnd::Item | TagEnd::FootnoteDefinition => {
                self.leaf_end = self.leaf_end.max(range.end);
                self.finish_leaf();
                if let Some(open) = self.open.last()
                    && open.fresh
                    && matches!(open.container, Container::Item { .. })
                {
                    self.emit(BlockKind::Paragraph { inlines: Vec::new() }, range.clone());
                }
                self.open.pop();
                if tag == TagEnd::FootnoteDefinition {
                    self.in_footnote = false;
                }
            }
            TagEnd::List(_) => {
                self.finish_leaf();
                self.lists.pop();
            }
            TagEnd::CodeBlock => {
                if let Some((language, text)) = self.code.take() {
                    let kind = if language.eq_ignore_ascii_case("math") {
                        BlockKind::Math { text: text.trim().to_owned() }
                    } else {
                        BlockKind::Code { language, text }
                    };
                    self.emit(kind, range);
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(html) = self.html.take() {
                    let start = self.leaf_start.take().unwrap_or(range.start);
                    let inlines = html_inlines(&html);
                    if !inlines.iter().all(|inline| inline.text.trim().is_empty() && inline.image.is_none()) {
                        self.emit(BlockKind::Paragraph { inlines }, start..range.end);
                    }
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.emit(
                        BlockKind::Table { align: table.align, head: table.head, rows: table.rows },
                        table.start..range.end,
                    );
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    table.in_head = false;
                }
            }
            TagEnd::Emphasis => self.styles.emphasis = self.styles.emphasis.saturating_sub(1),
            TagEnd::Strong => self.styles.strong = self.styles.strong.saturating_sub(1),
            TagEnd::Strikethrough => self.styles.strike = self.styles.strike.saturating_sub(1),
            TagEnd::Link => {
                self.styles.link.pop();
            }
            TagEnd::Image => {
                if let Some((src, alt)) = self.image.take() {
                    self.push_text("", &range, false);
                    self.push_inline(Inline { text: alt, image: Some(src), ..self.styled() });
                }
            }
            _ => {}
        }
    }

    fn inline_html(&mut self, html: &str, range: &Range<usize>) {
        for tag in tags(html) {
            if DROPPED.contains(&tag.name.as_str()) {
                if tag.closing {
                    self.dropping = self.dropping.saturating_sub(1);
                } else if !tag.self_closing {
                    self.dropping += 1;
                }
                continue;
            }
            if self.dropping > 0 {
                continue;
            }
            let change = |count: &mut usize| {
                if tag.closing { *count = count.saturating_sub(1) } else { *count += 1 }
            };
            match tag.name.as_str() {
                "b" | "strong" => change(&mut self.styles.strong),
                "i" | "em" => change(&mut self.styles.emphasis),
                "s" | "del" | "strike" => change(&mut self.styles.strike),
                "code" | "kbd" | "samp" => change(&mut self.styles.code),
                "a" if tag.closing => {
                    self.styles.link.pop();
                }
                "a" => self.styles.link.push(tag.attribute("href")),
                "br" => {
                    self.push_text("", range, false);
                    self.push_inline(Inline::plain("\n"));
                }
                "img" if !tag.closing => {
                    if let Some(src) = tag.attribute("src") {
                        self.push_text("", range, false);
                        let alt = tag.attribute("alt").unwrap_or_default();
                        self.push_inline(Inline { text: alt, image: Some(src), ..Inline::default() });
                    }
                }
                _ => {}
            }
        }
    }

    fn finish(mut self) -> Rendered {
        self.finish_leaf();
        let mut blocks = self.blocks;
        if !self.footnotes.is_empty() {
            let line = self.footnotes.first().map_or(1, |block| block.line);
            blocks.push(Block { kind: BlockKind::Footnotes, line, end_line: line, nesting: Vec::new() });
            let mut footnotes = self.footnotes;
            footnotes.sort_by_key(|block| match block.nesting.first() {
                Some(Container::Footnote { number, .. }) => *number,
                _ => u32::MAX,
            });
            blocks.extend(footnotes);
        }
        Rendered { blocks, headings: self.headings }
    }
}

pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_MATH
}

pub fn render(text: &str) -> Rendered {
    let mut reader = Reader::new(text);
    for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
        reader.event(event, range);
    }
    reader.finish()
}

fn autolinks(text: &str) -> Vec<(&str, Option<String>)> {
    let mut pieces = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let starts = rest.starts_with("https://") || rest.starts_with("http://") || rest.starts_with("www.");
        let boundary = at == 0 || text[..at].ends_with(|c: char| c.is_whitespace() || "(<*_~".contains(c));
        if starts && boundary {
            let mut end = at + rest.find(|c: char| c.is_whitespace() || c == '<').unwrap_or(rest.len());
            while end > at && text[..end].ends_with(['.', ',', ':', ';', '!', '?', '"', '\'', '*', '_', '~']) {
                end -= 1;
            }
            if text[..end].ends_with(')') && text[at..end].matches('(').count() < text[at..end].matches(')').count() {
                end -= 1;
            }
            let address = &text[at..end];
            if address.len() > "https://".len().min(address.len()) && address.contains('.') {
                if at > from {
                    pieces.push((&text[from..at], None));
                }
                let link = if address.starts_with("www.") { format!("https://{address}") } else { address.to_owned() };
                pieces.push((address, Some(link)));
                from = end;
                at = end;
                continue;
            }
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    if from < text.len() || pieces.is_empty() {
        pieces.push((&text[from..], None));
    }
    pieces
}

struct HtmlTag {
    name: String,
    closing: bool,
    self_closing: bool,
    attributes: Vec<(String, String)>,
}

impl HtmlTag {
    fn attribute(&self, name: &str) -> Option<String> {
        self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
    }
}

fn tags(html: &str) -> Vec<HtmlTag> {
    pieces(html)
        .into_iter()
        .filter_map(|piece| match piece {
            Piece::Tag(tag) => Some(tag),
            Piece::Text(_) => None,
        })
        .collect()
}

enum Piece {
    Tag(HtmlTag),
    Text(String),
}

fn pieces(html: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(comment) = rest.strip_prefix("<!--") {
            rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        if rest.starts_with('<')
            && let Some(end) = rest.find('>')
        {
            let inside = &rest[1..end];
            let closing = inside.starts_with('/');
            let inside = inside.trim_start_matches('/');
            let self_closing = inside.ends_with('/');
            let inside = inside.trim_end_matches('/');
            let name_end = inside.find(|c: char| c.is_whitespace()).unwrap_or(inside.len());
            let name = inside[..name_end].to_ascii_lowercase();
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                pieces.push(Piece::Tag(HtmlTag {
                    name,
                    closing,
                    self_closing,
                    attributes: attributes(&inside[name_end..]),
                }));
                rest = &rest[end + 1..];
                continue;
            }
        }
        let next = rest[1..].find('<').map_or(rest.len(), |at| at + 1);
        pieces.push(Piece::Text(decode_entities(&rest[..next])));
        rest = &rest[next..];
    }
    pieces
}

fn attributes(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let key_end = rest.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(rest.len());
        let key = rest[..key_end].to_ascii_lowercase();
        rest = rest[key_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let inner = &after[1..];
                    let end = inner.find(quote).unwrap_or(inner.len());
                    value = decode_entities(&inner[..end]);
                    rest = inner.get(end + 1..).unwrap_or("").trim_start();
                }
                _ => {
                    let end = after.find(char::is_whitespace).unwrap_or(after.len());
                    value = decode_entities(&after[..end]);
                    rest = after[end..].trim_start();
                }
            }
        }
        if !key.is_empty() {
            found.push((key, value));
        }
    }
    found
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", "\u{a0}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&copy;", "©")
        .replace("&amp;", "&")
}

pub fn html_inlines(html: &str) -> Vec<Inline> {
    let mut reader = Reader::new("");
    reader.leaf = Some(Leaf::Paragraph);
    reader.leaf_start = Some(0);
    for piece in pieces(html) {
        match piece {
            Piece::Tag(tag) => {
                let block_end = matches!(
                    tag.name.as_str(),
                    "p" | "div"
                        | "summary"
                        | "details"
                        | "li"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "tr"
                        | "table"
                );
                if block_end && tag.closing && reader.dropping == 0 && !reader.inlines.is_empty() {
                    reader.push_inline(Inline::plain("\n"));
                }
                if tag.name == "summary" && reader.dropping == 0 {
                    if tag.closing {
                        reader.styles.strong = reader.styles.strong.saturating_sub(1);
                    } else {
                        reader.styles.strong += 1;
                    }
                    continue;
                }
                let mut fragment = String::from("<");
                if tag.closing {
                    fragment.push('/');
                }
                fragment.push_str(&tag.name);
                for (key, value) in &tag.attributes {
                    let _ = write!(fragment, " {key}=\"{}\"", value.replace('"', "&quot;"));
                }
                fragment.push('>');
                if tag.self_closing && !tag.closing && DROPPED.contains(&tag.name.as_str()) {
                    continue;
                }
                reader.inline_html(&fragment, &(0..0));
            }
            Piece::Text(text) => {
                if reader.dropping == 0 {
                    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    let padded = if text.is_empty() { text } else { format!("{text} ") };
                    if !padded.is_empty() {
                        reader.push_inline(Inline { text: padded, ..reader.styled() });
                    }
                }
            }
        }
    }
    while reader.inlines.last().is_some_and(|last| last.text == "\n") {
        reader.inlines.pop();
    }
    if let Some(last) = reader.inlines.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    reader.inlines
}

pub fn to_html(rendered: &Rendered) -> String {
    let mut out = String::new();
    for block in &rendered.blocks {
        let quotes = block.nesting.iter().filter(|container| matches!(container, Container::Quote { .. })).count();
        let depth = block.nesting.iter().filter(|container| matches!(container, Container::Item { .. })).count();
        let mut prefix = String::new();
        for container in &block.nesting {
            match container {
                Container::Quote { alert: Some(alert), first: true } => {
                    let _ = write!(prefix, "<p class=\"alert-title\">{alert:?}</p>");
                }
                Container::Item { marker: Some(marker), task } => match task {
                    Some(task) => {
                        let _ = write!(
                            prefix,
                            "<input type=\"checkbox\" disabled{}> ",
                            if task.checked { " checked" } else { "" }
                        );
                    }
                    None => {
                        let _ = write!(prefix, "{} ", escape(marker));
                    }
                },
                Container::Footnote { number, first: true } => {
                    let _ = write!(prefix, "<sup id=\"fn-{number}\">{number}</sup> ");
                }
                _ => {}
            }
        }
        let attributes = format!(
            " data-line=\"{}\"{}{}",
            block.line,
            if quotes > 0 { format!(" data-quote=\"{quotes}\"") } else { String::new() },
            if depth > 0 { format!(" data-list=\"{depth}\"") } else { String::new() }
        );
        match &block.kind {
            BlockKind::Heading { level, anchor, inlines } => {
                let _ = writeln!(
                    out,
                    "<h{level} id=\"{}\"{attributes}>{prefix}{}</h{level}>",
                    escape(anchor),
                    inline_html(inlines)
                );
            }
            BlockKind::Paragraph { inlines } => {
                let _ = writeln!(out, "<p{attributes}>{prefix}{}</p>", inline_html(inlines));
            }
            BlockKind::Code { language, text } => {
                let _ = writeln!(
                    out,
                    "<pre{attributes}><code class=\"language-{}\">{}</code></pre>",
                    escape(language),
                    escape(text)
                );
            }
            BlockKind::Math { text } => {
                let _ = writeln!(out, "<div class=\"math\"{attributes}>{}</div>", escape(text));
            }
            BlockKind::Table { align, head, rows } => {
                let style = |column: usize| match align.get(column) {
                    Some(Align::Left) => " style=\"text-align:left\"",
                    Some(Align::Center) => " style=\"text-align:center\"",
                    Some(Align::Right) => " style=\"text-align:right\"",
                    _ => "",
                };
                let _ = write!(out, "<table{attributes}><thead><tr>");
                for (column, cell) in head.iter().enumerate() {
                    let _ = write!(out, "<th{}>{}</th>", style(column), inline_html(cell));
                }
                out.push_str("</tr></thead><tbody>");
                for row in rows {
                    out.push_str("<tr>");
                    for (column, cell) in row.iter().enumerate() {
                        let _ = write!(out, "<td{}>{}</td>", style(column), inline_html(cell));
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</tbody></table>\n");
            }
            BlockKind::Rule => out.push_str("<hr>\n"),
            BlockKind::Footnotes => out.push_str("<hr class=\"footnotes\">\n"),
        }
    }
    out
}

fn inline_html(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        if inline.text == "\n" {
            out.push_str("<br>");
            continue;
        }
        if let Some(src) = &inline.image {
            let _ = write!(out, "<img src=\"{}\" alt=\"{}\">", escape(src), escape(&inline.text));
            continue;
        }
        if let Some(number) = inline.footnote {
            let _ = write!(out, "<sup><a href=\"#fn-{number}\">{number}</a></sup>");
            continue;
        }
        let mut text = escape(&inline.text);
        for (on, open, close) in [
            (inline.code, "<code>", "</code>"),
            (inline.math, "<span class=\"math\">", "</span>"),
            (inline.strike, "<del>", "</del>"),
            (inline.emphasis, "<em>", "</em>"),
            (inline.strong, "<strong>", "</strong>"),
        ] {
            if on {
                text = format!("{open}{text}{close}");
            }
        }
        if let Some(link) = &inline.link {
            text = format!("<a href=\"{}\">{text}</a>", escape(link));
        }
        out.push_str(&text);
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn to_text(rendered: &Rendered) -> String {
    let mut out = String::new();
    for block in &rendered.blocks {
        let mut prefix = String::new();
        for container in &block.nesting {
            match container {
                Container::Quote { alert, first } => {
                    prefix.push_str("> ");
                    if let (Some(alert), true) = (alert, first) {
                        let _ = write!(prefix, "[{}] ", format!("{alert:?}").to_uppercase());
                    }
                }
                Container::Item { marker: Some(marker), task } => match task {
                    Some(task) => prefix.push_str(if task.checked { "[x] " } else { "[ ] " }),
                    None => {
                        let _ = write!(prefix, "{marker} ");
                    }
                },
                Container::Item { .. } => prefix.push_str("  "),
                Container::Footnote { number, first: true } => {
                    let _ = write!(prefix, "[{number}] ");
                }
                Container::Footnote { .. } => {}
            }
        }
        let body = match &block.kind {
            BlockKind::Heading { level, inlines, .. } => {
                format!("{} {}", "#".repeat(*level as usize), plain_text(inlines))
            }
            BlockKind::Paragraph { inlines } => plain_text(inlines),
            BlockKind::Code { language, text } => format!("```{language}\n{}```", text),
            BlockKind::Math { text } => format!("$$ {text} $$"),
            BlockKind::Table { head, rows, .. } => {
                let row = |cells: &Vec<Vec<Inline>>| {
                    format!("| {} |", cells.iter().map(|cell| plain_text(cell)).collect::<Vec<_>>().join(" | "))
                };
                std::iter::once(row(head)).chain(rows.iter().map(row)).collect::<Vec<_>>().join("\n")
            }
            BlockKind::Rule => "---".to_owned(),
            BlockKind::Footnotes => "---".to_owned(),
        };
        let _ = writeln!(out, "{prefix}{body}\n");
    }
    out.trim_end().to_owned() + "\n"
}

pub fn block_at_line(rendered: &Rendered, line: u32) -> Option<usize> {
    let mut found = None;
    for (index, block) in rendered.blocks.iter().enumerate() {
        if matches!(block.kind, BlockKind::Footnotes)
            || block.nesting.iter().any(|c| matches!(c, Container::Footnote { .. }))
        {
            break;
        }
        if block.line <= line {
            found = Some(index);
        }
        if block.line > line {
            break;
        }
    }
    found.or_else(|| (!rendered.blocks.is_empty()).then_some(0))
}

pub fn block_of_anchor(rendered: &Rendered, anchor: &str) -> Option<usize> {
    let anchor = anchor.trim_start_matches('#');
    rendered
        .blocks
        .iter()
        .position(|block| matches!(&block.kind, BlockKind::Heading { anchor: own, .. } if own == anchor))
}

pub fn toggled_task(line: &str) -> Option<String> {
    let marker = line.find("[ ]").or_else(|| line.find("[x]")).or_else(|| line.find("[X]"))?;
    let before = line[..marker].trim_start_matches(|c: char| c == '>' || c.is_whitespace()).trim_end();
    let listed = matches!(before, "-" | "*" | "+")
        || (before.len() > 1
            && before.ends_with(['.', ')'])
            && before[..before.len() - 1].chars().all(|c| c.is_ascii_digit()));
    if !listed {
        return None;
    }
    let ticked = &line[marker + 1..marker + 2] != " ";
    Some(format!("{}[{}]{}", &line[..marker], if ticked { " " } else { "x" }, &line[marker + 3..]))
}

#[cfg(test)]
mod tests;
