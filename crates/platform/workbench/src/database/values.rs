use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::geometry::{self as spatial, Bounds, Summary};
use zen_db::present::{self, Calc, ValueFormat};
use zen_db::value::Value;
use zen_i18n::t;

use super::consoles::{SettingsParams, UpdateSettings, settings};
use super::sources::Database;

const SHOWN_CHARS: usize = 4000;

const MOST_CHARS: usize = 100_000;

const MOST_CELLS: usize = 1_000_000;

fn result_of(session: &Session, id: u64) -> Result<std::sync::Arc<zen_db::results::ResultSet>, CommandError> {
    session
        .state::<Database>()
        .and_then(Database::started)
        .and_then(|engine| engine.results().get(id))
        .ok_or_else(|| CommandError::with_code("unknownResult", t!("database-error-unknown-result", id = id)))
        .and_then(|set| super::safety::agent_sees(session, set.connection()).map(|()| set))
}

pub fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Integer(_) => "integer",
        Value::Decimal(_) => "decimal",
        Value::Float(_) => "float",
        Value::Text(_) => "text",
        Value::Bytes(_) => "bytes",
        Value::Temporal(_) => "temporal",
        Value::Interval(_) => "interval",
        Value::Uuid(_) => "uuid",
        Value::Json(_) => "json",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Document(_) => "document",
        Value::Other(_) => "other",
    }
}

/// Which cell.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ValueParams {
    /// The result's id.
    pub id: u64,
    /// The row, from 0.
    pub row: usize,
    /// The column, from 0.
    pub column: usize,
    /// The format to render it in; the panel's setting, else the value's preferred one, when absent or not offered.
    pub format: Option<ValueFormat>,
    /// The most characters to answer; 4000 when absent, at most 100 000.
    pub limit: Option<usize>,
}

/// A picture held in bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    /// Its format: `PNG`, `JPEG`, `GIF`, `WebP`, `BMP` or `ICO`.
    pub format: String,
    /// Its size in bytes.
    pub bytes: usize,
}

/// One cell's value as the value panel shows it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValueAnswer {
    /// The result's id.
    pub id: u64,
    /// The row.
    pub row: usize,
    /// The column.
    pub column: usize,
    /// The column's name.
    pub name: String,
    /// The column's type as the database names it.
    #[serde(rename = "type")]
    pub type_name: String,
    /// The value's kind in the value model.
    pub kind: String,
    /// Whether it is `NULL`.
    pub null: bool,
    /// The formats it offers, the preferred one first.
    pub formats: Vec<ValueFormat>,
    /// The format answered.
    pub format: ValueFormat,
    /// The value rendered in it, at most `limit` characters; a picture's format and size for `image`.
    pub text: String,
    /// The characters the whole rendering has.
    pub length: usize,
    /// Whether `text` was cut.
    pub truncated: bool,
    /// The bytes of a binary value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<usize>,
    /// The picture a binary value holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<Picture>,
    /// What a spatial value holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry: Option<Summary>,
}

pub fn cell(
    session: &Session,
    id: u64,
    row: usize,
    column: usize,
) -> Result<(Value, zen_db::value::Column), CommandError> {
    let result = result_of(session, id)?;
    let columns = result.columns();
    let known = columns.get(column).cloned();
    match known {
        Some(known) if row < result.len() => Ok((result.get(row, column), known)),
        _ => Err(CommandError::with_code(
            "noCell",
            t!("database-error-no-cell", row = row, column = column, rows = result.len(), columns = columns.len()),
        )),
    }
}

pub struct ResultValue;

impl Command for ResultValue {
    type Params = ValueParams;
    type Output = ValueAnswer;
    const ID: &'static str = "db.result.value";
    const TITLE: &'static str = "Cell Value";
    const DESCRIPTION: &'static str = "One cell of a result (`id`, `row`, `column`, from 0) as the value panel shows it: the formats the value offers (`text`; `json` pretty-printed for JSON, documents, maps, arrays and text holding JSON; `xml` indented; `hex` for bytes and text; `image` for bytes holding a PNG, JPEG, GIF, WebP, BMP or ICO picture; `geometry` for spatial values, as WKT with `SRID=…;`, with `geometry` telling its kind, SRID, axes, encoding, points, parts and bounds) and the value rendered in `format` (else the panel's preferred format when offered, else the value's own), at most `limit` characters (4000, at most 100 000) with `length` and `truncated`. Arrays read as their SQL literal; timestamps as the server sent them, with their zone.";

    fn run(&self, session: &mut Session, params: ValueParams) -> Result<ValueAnswer, CommandError> {
        let (value, column) = cell(session, params.id, params.row, params.column)?;
        let limit = params.limit.unwrap_or(SHOWN_CHARS).clamp(1, MOST_CHARS);
        let wanted = params.format.or(settings(session).value_format);
        let format = present::chosen(&value, wanted);
        let rendered = present::render(&value, format, limit).unwrap_or_default();
        let (size, picture) = match &value {
            Value::Bytes(bytes) => (
                Some(bytes.len()),
                present::image_kind(bytes).map(|format| Picture { format: format.to_owned(), bytes: bytes.len() }),
            ),
            _ => (None, None),
        };
        let geometry = spatial::from_value(&value).map(|found| found.summary());
        Ok(ValueAnswer {
            id: params.id,
            row: params.row,
            column: params.column,
            name: column.name,
            type_name: column.type_name,
            kind: kind_of(&value).to_owned(),
            null: value == Value::Null,
            formats: present::formats(&value),
            format,
            text: rendered.text,
            length: rendered.length,
            truncated: rendered.truncated,
            size,
            picture,
            geometry,
        })
    }
}

/// Where to save a cell's value.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveParams {
    /// The result's id.
    pub id: u64,
    /// The row, from 0.
    pub row: usize,
    /// The column, from 0.
    pub column: usize,
    /// The file to write.
    pub path: PathBuf,
    /// The format to write; the value as it is (bytes as bytes, text as UTF-8) when absent.
    pub format: Option<ValueFormat>,
    /// Replace the file when it exists; refused with code `exists` otherwise.
    pub overwrite: bool,
}

/// A value saved.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// The file written.
    pub path: PathBuf,
    /// The bytes written.
    pub bytes: usize,
}

pub fn file_bytes(value: &Value, format: Option<ValueFormat>) -> Vec<u8> {
    match (value, format) {
        (Value::Bytes(bytes), None | Some(ValueFormat::Image)) => bytes.clone(),
        (_, Some(format)) if format != ValueFormat::Image => {
            present::render(value, format, usize::MAX).map(|rendered| rendered.text).unwrap_or_default().into_bytes()
        }
        (Value::Map(_) | Value::Document(_), _) => present::pretty_json(&present::json_text(value)).0.into_bytes(),
        _ => present::plain_text(value).into_bytes(),
    }
}

pub struct SaveValue;

impl Command for SaveValue {
    type Params = SaveParams;
    type Output = Saved;
    const ID: &'static str = "db.result.saveValue";
    const TITLE: &'static str = "Save Value to File";
    const DESCRIPTION: &'static str = "Writes one cell of a result (`id`, `row`, `column`) to the file `path`: bytes as they are and text as UTF-8, or rendered in `format` (`json` pretty-printed, `hex` as a dump). Never replaces a file unless `overwrite`; refused with code `exists` otherwise, and a folder is refused.";

    fn run(&self, session: &mut Session, params: SaveParams) -> Result<Saved, CommandError> {
        let (value, _) = cell(session, params.id, params.row, params.column)?;
        let path = session.state_mut::<crate::Workbench>().resolve(&params.path);
        super::guard_agent_file(session, &path)?;
        if path.is_dir() {
            return Err(CommandError::with_code(
                "isFolder",
                t!("database-error-save-folder", path = crate::display(&path)),
            ));
        }
        if path.exists() && !params.overwrite {
            return Err(CommandError::with_code(
                "exists",
                t!("database-error-save-exists", path = crate::display(&path)),
            ));
        }
        let bytes = file_bytes(&value, params.format);
        std::fs::write(&path, &bytes).map_err(|error| {
            CommandError::new(t!("workbench-error-create", path = crate::display(&path), error = error.to_string()))
        })?;
        Ok(Saved { path, bytes: bytes.len() })
    }
}

/// Which cells to sum up.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CalcParams {
    /// The result's id.
    pub id: u64,
    /// The first and last row, both included; every row kept when absent.
    pub rows: Option<(usize, usize)>,
    /// The first and last column, both included; every column when absent.
    pub columns: Option<(usize, usize)>,
}

/// The figures of cells of a result.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalcAnswer {
    /// The result's id.
    pub id: u64,
    /// The figures.
    #[serde(flatten)]
    pub calc: Calc,
}

pub struct CalcCells;

impl Command for CalcCells {
    type Params = CalcParams;
    type Output = CalcAnswer;
    const ID: &'static str = "db.result.calc";
    const TITLE: &'static str = "Calculate Selection";
    const DESCRIPTION: &'static str = "The figures of a rectangle of a result's cells (`rows` and `columns` as `[first, last]`, all when absent), as the calc panel and the status bar show them for the grid's selection: `cells`, `count` (not NULL), `numeric`, `sum` and `avg` of the numbers (integers and exact decimals summed exactly), `min` and `max` (numerically when there are numbers, else as text), `distinct` values; at most a million cells (`truncated`).";

    fn run(&self, session: &mut Session, params: CalcParams) -> Result<CalcAnswer, CommandError> {
        let result = result_of(session, params.id)?;
        let (rows, columns) = (result.len(), result.columns().len());
        let (first, last) = params.rows.unwrap_or((0, rows.saturating_sub(1)));
        let (left, right) = params.columns.unwrap_or((0, columns.saturating_sub(1)));
        let rows = if rows == 0 || first > last { 0..0 } else { first..(last + 1).min(rows) };
        let columns = if columns == 0 || left > right { 0..0 } else { left..(right + 1).min(columns) };
        Ok(CalcAnswer { id: params.id, calc: result.calc(rows, columns, MOST_CELLS) })
    }
}

const SHAPES: usize = 100;
const MOST_SHAPES: usize = 1000;
const SHAPE_CHARS: usize = 1000;
const MOST_SHAPE_CELLS: usize = 100_000;

/// Which cells to read shapes from.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GeometryParams {
    /// The result's id.
    pub id: u64,
    /// The first and last row, both included; every row kept when absent.
    pub rows: Option<(usize, usize)>,
    /// The first and last column, both included; every column when absent.
    pub columns: Option<(usize, usize)>,
    /// The most shapes to answer; 100 when absent, at most 1000.
    pub limit: Option<usize>,
    /// The most WKT characters of each shape; 1000 when absent, at most 100 000; 0 leaves the WKT out.
    pub chars: Option<usize>,
}

/// One spatial value of a result.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    /// The row.
    pub row: usize,
    /// The column.
    pub column: usize,
    /// What it holds.
    #[serde(flatten)]
    pub summary: Summary,
    /// It as WKT with `SRID=…;`, at most `chars` characters; absent when `chars` is 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wkt: Option<String>,
    /// Whether `wkt` was cut.
    pub truncated: bool,
}

/// The spatial values of cells of a result.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeometryAnswer {
    /// The result's id.
    pub id: u64,
    /// The shapes, row by row, at most `limit`.
    pub shapes: Vec<Shape>,
    /// The rectangle around every shape answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    /// The cells looked at.
    pub cells: u64,
    /// Whether shapes or cells were left out: more than `limit` shapes, or more than 100 000 cells.
    pub truncated: bool,
}

pub struct ResultGeometry;

impl Command for ResultGeometry {
    type Params = GeometryParams;
    type Output = GeometryAnswer;
    const ID: &'static str = "db.result.geometry";
    const TITLE: &'static str = "Geometry Viewer";
    const DESCRIPTION: &'static str = "The spatial values in a rectangle of a result's cells (`rows` and `columns` as `[first, last]`, all when absent), as the value panel's geometry viewer draws them: each cell holding PostGIS `geometry` or `geography` (EWKB), MySQL geometry, a GeoPackage blob, WKB, WKT or EWKT text, or GeoJSON, with its `kind`, `srid`, `dimensions`, `encoding`, `points`, `parts`, `bounds` and its WKT cut at `chars` (1000, at most 100 000; 0 leaves it out); at most `limit` shapes (100, at most 1000) and 100 000 cells (`truncated`), with the `bounds` of all of them. Nothing is fetched from the network: no map tiles.";

    fn run(&self, session: &mut Session, params: GeometryParams) -> Result<GeometryAnswer, CommandError> {
        let result = result_of(session, params.id)?;
        let (rows, columns) = (result.len(), result.columns().len());
        let (first, last) = params.rows.unwrap_or((0, rows.saturating_sub(1)));
        let (left, right) = params.columns.unwrap_or((0, columns.saturating_sub(1)));
        let rows = if rows == 0 || first > last { 0..0 } else { first..(last + 1).min(rows) };
        let columns = if columns == 0 || left > right { 0..0 } else { left..(right + 1).min(columns) };
        let limit = params.limit.unwrap_or(SHAPES).clamp(1, MOST_SHAPES);
        let chars = params.chars.unwrap_or(SHAPE_CHARS).min(MOST_CHARS);
        let mut answer = GeometryAnswer { id: params.id, shapes: Vec::new(), bounds: None, cells: 0, truncated: false };
        'rows: for row in rows {
            for column in columns.clone() {
                if answer.cells as usize >= MOST_SHAPE_CELLS {
                    answer.truncated = true;
                    break 'rows;
                }
                answer.cells += 1;
                let Some(found) = spatial::from_value(&result.get(row, column)) else { continue };
                if answer.shapes.len() >= limit {
                    answer.truncated = true;
                    break 'rows;
                }
                let summary = found.summary();
                if let Some(bounds) = summary.bounds {
                    answer.bounds = Some(answer.bounds.map_or(bounds, |known| known.union(bounds)));
                }
                let (wkt, truncated) = match chars {
                    0 => (None, false),
                    chars => {
                        let (text, _, truncated) = found.ewkt(chars);
                        (Some(text), truncated)
                    }
                };
                answer.shapes.push(Shape { row, column, summary, wkt, truncated });
            }
        }
        Ok(answer)
    }
}

/// Show or hide the value panel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PanelParams {
    /// Show (`true`) or hide it; the other way round from now when absent.
    pub shown: Option<bool>,
}

/// The value panel.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PanelAnswer {
    /// Whether it shows.
    pub shown: bool,
    /// The format it prefers; each value's own when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ValueFormat>,
}

pub struct ToggleValuePanel;

impl Command for ToggleValuePanel {
    type Params = PanelParams;
    type Output = PanelAnswer;
    const ID: &'static str = "db.result.toggleValuePanel";
    const TITLE: &'static str = "Show/Hide Value Panel";
    const DESCRIPTION: &'static str = "Shows or hides the value panel beside every result grid (`shown`, else the other way round), remembered in the Database settings. The panel shows the selected cell's value as text, pretty JSON with folding, indented XML, a hex dump, a picture or the shapes of a spatial value, and the figures of the selection.";

    fn run(&self, session: &mut Session, params: PanelParams) -> Result<PanelAnswer, CommandError> {
        let shown = params.shown.unwrap_or(!settings(session).value_panel);
        let changed =
            UpdateSettings.run(session, SettingsParams { value_panel: Some(shown), ..SettingsParams::default() })?;
        Ok(PanelAnswer { shown: changed.value_panel, format: changed.value_format })
    }
}

#[derive(Default)]
pub struct Selections {
    wanted: std::collections::HashMap<u64, (usize, usize)>,
}

pub fn take_selection(session: &mut Session, id: u64) -> Option<(usize, usize)> {
    session.state_mut::<Selections>().wanted.remove(&id)
}

/// A cell to go to.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectParams {
    /// The result's id; the result the Database workspace shows in front when absent.
    pub id: Option<u64>,
    /// The row, from 1 as the row numbers show it.
    pub row: usize,
    /// The column, from 0; the column selected now when absent.
    pub column: Option<usize>,
}

/// The cell selected.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Selected {
    /// The result.
    pub id: u64,
    /// The row, from 1.
    pub row: usize,
    /// The rows read so far.
    pub rows: usize,
}

pub struct SelectRow;

impl Command for SelectRow {
    type Params = SelectParams;
    type Output = Selected;
    const ID: &'static str = "db.result.select";
    const TITLE: &'static str = "Go to Row";
    const DESCRIPTION: &'static str = "Selects row `row` (from 1, as the row numbers show it) of a result (`id`, else the one shown in front) in its grid and scrolls to it, in column `column` (from 0) when given. Refused with code `noRow` past the rows read so far (read more with db.result.fetchNext or db.result.fetchAll). Answers the row and the rows read.";

    fn run(&self, session: &mut Session, params: SelectParams) -> Result<Selected, CommandError> {
        let id = super::filters::result_or_active(session, params.id)?.id();
        let set = result_of(session, id)?;
        let rows = set.len();
        if params.row == 0 || params.row > rows {
            return Err(CommandError::with_code(
                "noRow",
                t!("database-error-go-to-row", row = params.row, rows = rows),
            ));
        }
        let column = params.column.unwrap_or(usize::MAX);
        session.state_mut::<Selections>().wanted.insert(id, (params.row - 1, column));
        Ok(Selected { id, row: params.row, rows })
    }
}
