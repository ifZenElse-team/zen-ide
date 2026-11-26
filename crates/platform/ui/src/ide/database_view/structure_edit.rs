use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::super::{IdePalette, theme, vcs};
use super::DatabaseView;
use crate::accessibility;
use zen_workbench::database::alter::{CreateAsked, CreateKind, CreateRequest};

pub(super) const BAR: f32 = 34.0;
const STAGED_ROW: f32 = 20.0;
const STAGED_SHOWN: usize = 4;

#[derive(Clone, Debug, PartialEq)]
enum Action {
    AddColumn,
    EditColumn(String),
    DropColumn(String),
    AddConstraint,
    AddForeignKey,
    DropConstraint(String),
    AddIndex,
    DropIndex(String),
    Rename,
    Comment,
    Drop,
    CreateTable,
    CreateView,
    CreateSequence,
    CreateRoutine,
    CreateSchema,
    RenameSchema,
    CommentSchema,
    DropSchema,
    FindUsages(Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
enum Input {
    Text(String),
    Code(String),
    Check(bool),
    Choice(Vec<(&'static str, String)>, usize),
}

#[derive(Clone, Debug, PartialEq)]
struct Field {
    key: &'static str,
    label: String,
    input: Input,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct NewColumn {
    name: String,
    type_name: String,
    not_null: bool,
    key: bool,
    identity: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Form {
    tab: String,
    action: Action,
    title: String,
    note: Option<String>,
    fields: Vec<Field>,
    original: Vec<Field>,
    columns: Vec<NewColumn>,
    context: Context,
    error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Context {
    connection: String,
    path: Vec<String>,
    schema: String,
    kind: String,
    name: String,
    driver: String,
    cascade: bool,
}

#[derive(Clone, Debug)]
struct PreviewDialog {
    tab: String,
    answer: Result<Value, String>,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct StructureEdit {
    form: Option<Form>,
    preview: Option<PreviewDialog>,
    message: Option<(String, String, bool)>,
    state: Option<(u64, String, Value)>,
}

impl StructureEdit {
    pub(super) fn popups(&self) -> Vec<String> {
        let mut open = Vec::new();
        if self.form.is_some() {
            open.push("databaseStructureChange".to_owned());
        }
        if self.preview.is_some() {
            open.push("databaseStructurePreview".to_owned());
        }
        open
    }
}

fn text(value: &str) -> Input {
    Input::Text(value.to_owned())
}

fn field(key: &'static str, label: String, input: Input) -> Field {
    Field { key, label, input }
}

fn actions_choice(current: &str) -> Input {
    let choices = vec![
        ("", t!("ide-dbalter-action-default")),
        ("no action", "NO ACTION".to_owned()),
        ("restrict", "RESTRICT".to_owned()),
        ("cascade", "CASCADE".to_owned()),
        ("set null", "SET NULL".to_owned()),
        ("set default", "SET DEFAULT".to_owned()),
    ];
    let at = choices.iter().position(|(key, _)| *key == current).unwrap_or(0);
    Input::Choice(choices, at)
}

fn template(driver: &str, schema: &str, name: &str) -> String {
    let dialect = match driver {
        "mysql" => zen_db::dialect::SqlDialect::MySql,
        "mariadb" => zen_db::dialect::SqlDialect::MariaDb,
        _ => zen_db::dialect::SqlDialect::Postgres,
    };
    zen_db::alter::routine_template(dialect, schema, name, false)
}

fn row_text(row: &Value, at: usize) -> String {
    row.get(at).and_then(Value::as_str).unwrap_or_default().to_owned()
}

impl Form {
    fn value(&self, key: &str) -> Option<&Input> {
        self.fields.iter().find(|field| field.key == key).map(|field| &field.input)
    }

    fn string(&self, key: &str) -> String {
        match self.value(key) {
            Some(Input::Text(text) | Input::Code(text)) => text.trim().to_owned(),
            Some(Input::Choice(choices, at)) => choices.get(*at).map(|(key, _)| (*key).to_owned()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn optional(&self, key: &str) -> Option<String> {
        Some(self.string(key)).filter(|text| !text.is_empty())
    }

    fn flag(&self, key: &str) -> bool {
        matches!(self.value(key), Some(Input::Check(true)))
    }

    fn changed(&self, key: &str) -> bool {
        let before = self.original.iter().find(|field| field.key == key).map(|field| &field.input);
        before != self.value(key)
    }

    fn number(&self, key: &str) -> Option<i64> {
        self.optional(key).and_then(|text| text.parse().ok())
    }

    fn list(&self, key: &str) -> Vec<String> {
        self.string(key).split(',').map(str::trim).filter(|part| !part.is_empty()).map(str::to_owned).collect()
    }

    fn change(&self) -> Value {
        let context = &self.context;
        let schema = context.schema.as_str();
        let table = context.name.as_str();
        match &self.action {
            Action::AddColumn => json!({ "action": "addColumn", "schema": schema, "table": table, "column": {
                "name": self.string("name"), "type": self.string("type"), "notNull": self.flag("notNull"),
                "default": self.optional("default"), "identity": self.flag("identity"), "comment": self.optional("comment") } }),
            Action::EditColumn(column) => {
                let mut patch = json!({});
                for key in ["name", "type", "default", "comment"] {
                    if self.changed(key) {
                        patch[key] = json!(self.string(key));
                    }
                }
                if self.changed("notNull") {
                    patch["notNull"] = json!(self.flag("notNull"));
                }
                json!({ "action": "alterColumn", "schema": schema, "table": table, "column": column, "patch": patch })
            }
            Action::DropColumn(column) => json!({ "action": "dropColumn", "schema": schema, "table": table,
                "column": column, "cascade": self.flag("cascade") }),
            Action::AddConstraint => {
                json!({ "action": "addConstraint", "schema": schema, "table": table, "constraint": {
                "name": self.optional("name"), "kind": self.string("kind"), "columns": self.list("columns"),
                "check": self.optional("check") } })
            }
            Action::AddForeignKey => {
                json!({ "action": "addConstraint", "schema": schema, "table": table, "constraint": {
                "name": self.optional("name"), "kind": "foreignKey", "columns": self.list("columns"),
                "references": { "schema": self.optional("refSchema"), "table": self.string("refTable"),
                    "columns": self.list("refColumns"), "onDelete": self.optional("onDelete"),
                    "onUpdate": self.optional("onUpdate") } } })
            }
            Action::DropConstraint(name) => json!({ "action": "dropConstraint", "schema": schema, "table": table,
                "name": name, "cascade": self.flag("cascade") }),
            Action::AddIndex => json!({ "action": "createIndex", "schema": schema, "table": table, "index": {
                "name": self.optional("name"), "columns": self.list("columns"), "unique": self.flag("unique") } }),
            Action::DropIndex(name) => json!({ "action": "dropIndex", "schema": schema, "table": table, "name": name }),
            Action::Rename => json!({ "action": "rename", "schema": schema, "kind": context.kind, "name": context.name,
                "to": self.string("to") }),
            Action::Comment => {
                json!({ "action": "comment", "schema": schema, "kind": context.kind, "name": context.name,
                "comment": self.string("comment") })
            }
            Action::Drop => json!({ "action": "drop", "schema": schema, "kind": context.kind, "name": context.name,
                "cascade": self.flag("cascade") }),
            Action::CreateTable => {
                let columns: Vec<Value> = self
                    .columns
                    .iter()
                    .filter(|column| !column.name.trim().is_empty())
                    .map(|column| {
                        json!({ "name": column.name.trim(), "type": column.type_name.trim(),
                        "notNull": column.not_null, "identity": column.identity })
                    })
                    .collect();
                let key: Vec<&str> =
                    self.columns.iter().filter(|column| column.key).map(|column| column.name.trim()).collect();
                json!({ "action": "createTable", "schema": schema, "name": self.string("name"), "columns": columns,
                    "primaryKey": key, "comment": self.optional("comment") })
            }
            Action::CreateView => json!({ "action": "createView", "schema": schema, "name": self.string("name"),
                "query": self.string("query") }),
            Action::CreateSequence => {
                json!({ "action": "createSequence", "schema": schema, "name": self.string("name"),
                "settings": { "start": self.number("start"), "increment": self.number("increment"),
                    "min": self.number("min"), "max": self.number("max"), "cycle": self.flag("cycle") } })
            }
            Action::CreateRoutine => json!({ "action": "createRoutine", "schema": schema, "name": self.string("name"),
                "definition": self.string("definition") }),
            Action::CreateSchema => json!({ "action": "createSchema", "name": self.string("name") }),
            Action::RenameSchema => json!({ "action": "renameSchema", "name": schema, "to": self.string("to") }),
            Action::CommentSchema => {
                json!({ "action": "commentSchema", "name": schema, "comment": self.string("comment") })
            }
            Action::DropSchema => json!({ "action": "dropSchema", "name": schema, "cascade": self.flag("cascade") }),
            Action::FindUsages(_) => Value::Null,
        }
    }

    fn usages(&self) -> Option<Value> {
        if !self.flag("usages") {
            return None;
        }
        let context = &self.context;
        let plain = context.name.split('(').next().unwrap_or_default();
        let (column, new_name) = match &self.action {
            Action::Rename => (None, self.string("to")),
            Action::EditColumn(column) if self.changed("name") => (Some(column.clone()), self.string("name")),
            _ => return None,
        };
        let old = column.as_deref().unwrap_or(plain);
        if new_name.is_empty() || new_name == old {
            return None;
        }
        Some(json!({ "connection": context.connection, "path": context.path, "column": column, "newName": new_name }))
    }
}

fn has_usages(kind: &str) -> bool {
    matches!(kind, "table" | "foreignTable" | "view" | "materializedView" | "function" | "procedure")
}

fn usages_field() -> Field {
    field("usages", t!("ide-dbalter-rename-usages"), Input::Check(true))
}

fn header_value(properties: &Value, label: &str) -> String {
    properties["header"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|field| field["label"] == label)
        .and_then(|field| field["value"].as_str())
        .unwrap_or_default()
        .to_owned()
}

fn form(
    tab: &str,
    action: Action,
    context: Context,
    properties: &Value,
    selected: Option<&Value>,
    column: Option<&Value>,
) -> Form {
    let cascade = context.cascade;
    let mut note = None;
    let cascade_field = || field("cascade", t!("ide-dbalter-cascade"), Input::Check(false));
    let name = context.name.clone();
    let (title, mut fields) = match &action {
        Action::AddColumn | Action::EditColumn(_) => {
            let column = column.cloned().unwrap_or(Value::Null);
            let mut fields = vec![
                field("name", t!("ide-dbalter-name"), text(column["name"].as_str().unwrap_or_default())),
                field("type", t!("ide-dbalter-type"), text(column["typeName"].as_str().unwrap_or_default())),
                field("notNull", t!("ide-dbalter-not-null"), Input::Check(column["nullable"] == false)),
                field("default", t!("ide-dbalter-default"), text(column["default"].as_str().unwrap_or_default())),
                field("comment", t!("ide-dbalter-comment"), text(column["comment"].as_str().unwrap_or_default())),
            ];
            if action != Action::AddColumn {
                fields.push(usages_field());
            }
            if action == Action::AddColumn {
                fields.push(field("identity", t!("ide-dbalter-identity"), Input::Check(false)));
                (t!("ide-dbalter-add-column-title", table = name.as_str()), fields)
            } else {
                (t!("ide-dbalter-edit-column-title", column = column["name"].as_str().unwrap_or_default()), fields)
            }
        }
        Action::DropColumn(column) => {
            note = Some(t!("ide-dbalter-drop-column-note", column = column.as_str(), table = name.as_str()));
            (t!("ide-dbalter-drop-column-title"), if cascade { vec![cascade_field()] } else { Vec::new() })
        }
        Action::AddConstraint => (
            t!("ide-dbalter-add-constraint-title", table = name.as_str()),
            vec![
                field(
                    "kind",
                    t!("ide-dbalter-constraint-kind"),
                    Input::Choice(
                        vec![
                            ("unique", t!("ide-dbalter-kind-unique")),
                            ("primaryKey", t!("ide-dbalter-kind-primary-key")),
                            ("check", t!("ide-dbalter-kind-check")),
                        ],
                        0,
                    ),
                ),
                field("name", t!("ide-dbalter-name-optional"), text("")),
                field(
                    "columns",
                    t!("ide-dbalter-columns"),
                    text(&selected.map(|row| row_text(row, 0)).unwrap_or_default()),
                ),
                field("check", t!("ide-dbalter-check"), text("")),
            ],
        ),
        Action::AddForeignKey => (
            t!("ide-dbalter-add-foreign-key-title", table = name.as_str()),
            vec![
                field("name", t!("ide-dbalter-name-optional"), text("")),
                field("columns", t!("ide-dbalter-columns"), text("")),
                field("refSchema", t!("ide-dbalter-ref-schema"), text(&context.schema)),
                field("refTable", t!("ide-dbalter-ref-table"), text("")),
                field("refColumns", t!("ide-dbalter-ref-columns"), text("")),
                field("onDelete", t!("ide-dbalter-on-delete"), actions_choice("")),
                field("onUpdate", t!("ide-dbalter-on-update"), actions_choice("")),
            ],
        ),
        Action::DropConstraint(constraint) => {
            note = Some(t!("ide-dbalter-drop-constraint-note", name = constraint.as_str(), table = name.as_str()));
            (t!("ide-dbalter-drop-constraint-title"), if cascade { vec![cascade_field()] } else { Vec::new() })
        }
        Action::AddIndex => (
            t!("ide-dbalter-add-index-title", table = name.as_str()),
            vec![
                field("name", t!("ide-dbalter-name-optional"), text("")),
                field(
                    "columns",
                    t!("ide-dbalter-columns"),
                    text(&selected.map(|row| row_text(row, 0)).unwrap_or_default()),
                ),
                field("unique", t!("ide-dbalter-unique"), Input::Check(false)),
            ],
        ),
        Action::DropIndex(index) => {
            note = Some(t!("ide-dbalter-drop-index-note", name = index.as_str(), table = name.as_str()));
            (t!("ide-dbalter-drop-index-title"), Vec::new())
        }
        Action::Rename => {
            let plain = name.split('(').next().unwrap_or_default().to_owned();
            let mut fields = vec![field("to", t!("ide-dbalter-new-name"), text(&plain))];
            if has_usages(&context.kind) {
                fields.push(usages_field());
            }
            (t!("ide-dbalter-rename-title", name = plain.as_str()), fields)
        }
        Action::Comment | Action::CommentSchema => {
            let current = header_value(properties, &t!("database-object-column-comment"));
            (
                t!("ide-dbalter-comment-title", name = name.as_str()),
                vec![field("comment", t!("ide-dbalter-comment"), Input::Code(current))],
            )
        }
        Action::Drop => {
            note = Some(t!("ide-dbalter-drop-note", name = name.as_str()));
            (t!("ide-dbalter-drop-title"), if cascade { vec![cascade_field()] } else { Vec::new() })
        }
        Action::CreateTable => (
            t!("ide-dbalter-create-table-title", schema = context.schema.as_str()),
            vec![
                field("name", t!("ide-dbalter-name"), text("")),
                field("comment", t!("ide-dbalter-comment"), text("")),
            ],
        ),
        Action::CreateView => (
            t!("ide-dbalter-create-view-title", schema = context.schema.as_str()),
            vec![
                field("name", t!("ide-dbalter-name"), text("")),
                field("query", t!("ide-dbalter-query"), Input::Code("SELECT ".to_owned())),
            ],
        ),
        Action::CreateSequence => (
            t!("ide-dbalter-create-sequence-title", schema = context.schema.as_str()),
            vec![
                field("name", t!("ide-dbalter-name"), text("")),
                field("start", t!("ide-dbalter-start"), text("")),
                field("increment", t!("ide-dbalter-increment"), text("")),
                field("min", t!("ide-dbalter-minimum"), text("")),
                field("max", t!("ide-dbalter-maximum"), text("")),
                field("cycle", t!("ide-dbalter-cycle"), Input::Check(false)),
            ],
        ),
        Action::CreateRoutine => (
            t!("ide-dbalter-create-routine-title", schema = context.schema.as_str()),
            vec![
                field("name", t!("ide-dbalter-name"), text("new_function")),
                field(
                    "definition",
                    t!("ide-dbalter-definition"),
                    Input::Code(template(&context.driver, &context.schema, "new_function")),
                ),
            ],
        ),
        Action::CreateSchema => {
            (t!("ide-dbalter-create-schema-title"), vec![field("name", t!("ide-dbalter-name"), text(""))])
        }
        Action::RenameSchema => (
            t!("ide-dbalter-rename-title", name = context.schema.as_str()),
            vec![field("to", t!("ide-dbalter-new-name"), text(&context.schema))],
        ),
        Action::DropSchema => {
            note = Some(t!("ide-dbalter-drop-schema-note", name = context.schema.as_str()));
            (t!("ide-dbalter-drop-title"), vec![cascade_field()])
        }
        Action::FindUsages(_) => (String::new(), Vec::new()),
    };
    if context.driver == "sqlite" && matches!(action, Action::AddColumn | Action::EditColumn(_) | Action::CreateTable) {
        fields.retain(|field| field.key != "comment");
    }
    let columns = if action == Action::CreateTable {
        vec![NewColumn {
            name: "id".to_owned(),
            type_name: "bigint".to_owned(),
            not_null: true,
            key: true,
            identity: true,
        }]
    } else {
        Vec::new()
    };
    Form { tab: tab.to_owned(), action, title, note, original: fields.clone(), fields, columns, context, error: None }
}

fn context_of(object: &Value, alter: &Value) -> Context {
    let kind = object["kind"].as_str().unwrap_or_default().to_owned();
    let path: Vec<String> =
        object["path"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    Context {
        connection: object["connection"].as_str().unwrap_or_default().to_owned(),
        path: path.clone(),
        schema: path.get(1).cloned().unwrap_or_default(),
        name: if kind == "schema" {
            path.get(1).cloned().unwrap_or_default()
        } else {
            path.get(3).cloned().unwrap_or_default()
        },
        kind,
        driver: alter["dialect"].as_str().unwrap_or_default().to_owned(),
        cascade: alter["supports"]["cascade"] == true,
    }
}

fn small_button(
    ui: &mut Ui,
    p: &IdePalette,
    x: &mut f32,
    bar: Rect,
    label: &str,
    enabled: bool,
    salt: &str,
) -> Option<bool> {
    let font = theme::regular(12.0);
    let width = ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.to_owned(), font.clone(), p.text).size().x) + 18.0;
    if *x + width > bar.right() {
        return None;
    }
    let rect = Rect::from_min_size(pos2(*x, bar.top() + 6.0), vec2(width, bar.height() - 12.0));
    *x = rect.right() + 4.0;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, ui.scope_id().with(("database-structure-action", salt)), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    ui.painter().rect_stroke(rect, 4, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
    let color = if enabled { p.text } else { p.text_secondary };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, color);
    accessibility::button_with(&response, || label.to_owned());
    Some(enabled && response.clicked())
}

impl DatabaseView {
    pub(super) fn structure_state(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        tab: &str,
    ) -> Value {
        let pass = ctx.cumulative_pass_nr();
        if let Some((at, known, state)) = &self.structure.state
            && *at == pass
            && known == tab
        {
            return state.clone();
        }
        let state =
            registry.dispatch(session, "db.structure.state", json!({ "tab": tab, "limit": 50 })).unwrap_or_default();
        if state["running"].is_u64() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.structure.state = Some((pass, tab.to_owned(), state.clone()));
        state
    }

    fn structure_message(&self, tab: &str, alter: &Value) -> Option<(String, bool)> {
        if alter["running"].is_u64() {
            return None;
        }
        if let Some((known, message, error)) = &self.structure.message
            && known == tab
            && !message.is_empty()
        {
            return Some((message.clone(), *error));
        }
        alter["last"]["message"].as_str().map(|message| (message.to_owned(), true))
    }

    pub(super) fn structure_height(&self, tab: &str, alter: &Value) -> f32 {
        if alter["supports"]["structure"] != true {
            return 0.0;
        }
        let staged = alter["changes"].as_array().map_or(0, Vec::len).min(STAGED_SHOWN);
        let rows = staged + usize::from(self.structure_message(tab, alter).is_some());
        BAR + if rows > 0 { rows as f32 * STAGED_ROW + 8.0 } else { 0.0 }
    }

    pub(super) fn structure_bar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (object, properties, alter, selected): (&Value, &Value, &Value, Option<Value>),
        area: Rect,
    ) {
        let tab = object["tab"].as_str().unwrap_or_default().to_owned();
        let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR));
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let supports = &alter["supports"];
        let read_only = alter["readOnly"] == true;
        let running = alter["running"].is_u64();
        let kind = object["kind"].as_str().unwrap_or_default().to_owned();
        let context = context_of(object, alter);
        let section = properties["section"].as_str().unwrap_or_default().to_owned();
        let right_width = if alter["dirty"] == true { 300.0 } else { 120.0 };
        let left = Rect::from_min_max(bar.min, pos2(bar.right() - right_width, bar.bottom()));
        let mut x = left.left() + 8.0;
        let can = !read_only && !running;
        let name_of =
            |row: &Option<Value>, at: usize| row.as_ref().map(|row| row_text(row, at)).filter(|name| !name.is_empty());
        let mut buttons: Vec<(String, Option<Action>)> = Vec::new();
        if kind == "table" {
            match section.as_str() {
                "columns" => {
                    buttons.push((t!("ide-dbalter-add-column"), Some(Action::AddColumn)));
                    buttons.push((t!("ide-dbalter-edit-column"), name_of(&selected, 0).map(Action::EditColumn)));
                    buttons.push((t!("ide-dbalter-drop-column"), name_of(&selected, 0).map(Action::DropColumn)));
                }
                "constraints" => {
                    buttons.push((t!("ide-dbalter-add-constraint"), Some(Action::AddConstraint)));
                    buttons
                        .push((t!("ide-dbalter-drop-constraint"), name_of(&selected, 0).map(Action::DropConstraint)));
                }
                "foreignKeys" => {
                    buttons.push((t!("ide-dbalter-add-foreign-key"), Some(Action::AddForeignKey)));
                    buttons
                        .push((t!("ide-dbalter-drop-constraint"), name_of(&selected, 0).map(Action::DropConstraint)));
                }
                "indexes" => {
                    buttons.push((t!("ide-dbalter-add-index"), Some(Action::AddIndex)));
                    buttons.push((t!("ide-dbalter-drop-index"), name_of(&selected, 0).map(Action::DropIndex)));
                }
                _ => {}
            }
        }
        if kind == "schema" {
            buttons.push((t!("ide-dbalter-new-table"), Some(Action::CreateTable)));
            buttons.push((t!("ide-dbalter-new-view"), Some(Action::CreateView)));
            if supports["sequences"] == true {
                buttons.push((t!("ide-dbalter-new-sequence"), Some(Action::CreateSequence)));
            }
            if supports["routines"] == true {
                buttons.push((t!("ide-dbalter-new-function"), Some(Action::CreateRoutine)));
            }
            if supports["schemas"] == true {
                buttons.push((t!("ide-dbalter-new-schema"), Some(Action::CreateSchema)));
            }
            if supports["renameSchemas"] == true {
                buttons.push((t!("ide-dbalter-rename"), Some(Action::RenameSchema)));
            }
            if supports["comments"] == true && context.driver != "mysql" {
                buttons.push((t!("ide-dbalter-comment-action"), Some(Action::CommentSchema)));
            }
            if supports["schemas"] == true {
                buttons.push((t!("ide-dbalter-drop-schema"), Some(Action::DropSchema)));
            }
        } else {
            buttons.push((t!("ide-dbalter-rename"), Some(Action::Rename)));
            if supports["comments"] == true {
                buttons.push((t!("ide-dbalter-comment-action"), Some(Action::Comment)));
            }
            buttons.push((t!("ide-dbalter-drop-object"), Some(Action::Drop)));
        }
        if has_usages(&kind) && context.path.len() == 4 {
            let column = if section == "columns" { name_of(&selected, 1) } else { None };
            buttons.push((t!("ide-database-find-usages"), Some(Action::FindUsages(column))));
        }
        let mut chosen = None;
        let font = theme::regular(12.0);
        let needed: f32 = buttons
            .iter()
            .map(|(label, _)| {
                ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.clone(), font.clone(), p.text).size().x) + 22.0
            })
            .sum();
        let room = if x + needed > left.right() {
            Rect::from_min_max(left.min, pos2(left.right() - 32.0, left.bottom()))
        } else {
            left
        };
        let mut rest = Vec::new();
        for (index, (label, action)) in buttons.into_iter().enumerate() {
            let salt = format!("{tab}/{index}");
            if !rest.is_empty() {
                rest.push((label, action));
                continue;
            }
            let enabled = action.as_ref().is_some_and(|action| can || matches!(action, Action::FindUsages(_)));
            match small_button(ui, p, &mut x, room, &label, enabled, &salt) {
                None => rest.push((label, action)),
                Some(true) => chosen = action,
                Some(false) => {}
            }
        }
        if !rest.is_empty() {
            let at = Rect::from_min_size(pos2(x, bar.top() + 6.0), vec2(26.0, bar.height() - 12.0));
            let more = super::more_button(ui, p, at, ("structure", tab.as_str()));
            egui::Popup::menu(&more).show(|ui| {
                ui.set_min_width(180.0);
                for (label, action) in rest {
                    let enabled = action.as_ref().is_some_and(|action| can || matches!(action, Action::FindUsages(_)));
                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                        chosen = action;
                    }
                }
            });
        }
        if let Some(Action::FindUsages(column)) = &chosen {
            let params = json!({ "connection": context.connection, "path": context.path, "column": column });
            if let Err(error) = registry.dispatch(session, "db.usages.find", params) {
                self.structure_said(&tab, Err(error));
            }
        } else if let Some(action) = chosen {
            let column = match &action {
                Action::EditColumn(name) => {
                    let answer = registry
                        .dispatch(
                            session,
                            "db.object.get",
                            json!({ "connection": object["connection"], "path": object["path"], "limit": 2000 }),
                        )
                        .unwrap_or_default();
                    answer["object"]["columns"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|column| column["name"] == name.as_str())
                        .cloned()
                }
                _ => None,
            };
            self.structure.form = Some(form(&tab, action, context, properties, selected.as_ref(), column.as_ref()));
        }
        let mut right = bar.right() - 8.0;
        if alter["dirty"] == true {
            let font = theme::regular(12.0);
            for (label, primary, salt) in
                [(t!("ide-dbalter-discard"), false, "discard"), (t!("ide-dbalter-save"), true, "save")]
            {
                let width =
                    ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.clone(), font.clone(), p.text).size().x) + 22.0;
                let rect = Rect::from_min_size(pos2(right - width, bar.top() + 6.0), vec2(width, bar.height() - 12.0));
                right = rect.left() - 6.0;
                let response = ui.interact(
                    rect,
                    ui.scope_id().with(("database-structure-commit", salt)),
                    if running { Sense::hover() } else { Sense::click() },
                );
                if primary {
                    ui.painter().rect_filled(rect, 4, p.accent);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, &label, font.clone(), p.accent_text);
                } else {
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.hover);
                    }
                    ui.painter().rect_stroke(rect, 4, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, &label, font.clone(), p.text);
                }
                accessibility::button_with(&response, || label.clone());
                if response.clicked() && !running {
                    if primary {
                        self.open_structure_preview(registry, session, &tab);
                    } else {
                        let answer = registry.dispatch(session, "db.structure.discard", json!({ "tab": tab }));
                        self.structure_said(&tab, answer.map(|_| String::new()));
                    }
                }
            }
            let count = t!("ide-dbalter-staged", count = alter["total"].as_u64().unwrap_or(0));
            ui.painter().text(
                pos2(right, bar.center().y),
                Align2::RIGHT_CENTER,
                count,
                theme::regular(12.0),
                p.warning_text,
            );
        } else if running {
            ui.painter().text(
                pos2(right, bar.center().y),
                Align2::RIGHT_CENTER,
                t!("ide-dbalter-running"),
                theme::regular(12.0),
                p.text_secondary,
            );
        } else if read_only {
            ui.painter().text(
                pos2(right, bar.center().y),
                Align2::RIGHT_CENTER,
                t!("ide-dbalter-read-only"),
                theme::regular(12.0),
                p.text_secondary,
            );
        }
        let changes = alter["changes"].as_array().cloned().unwrap_or_default();
        let shown = changes.len().min(STAGED_SHOWN);
        if let Some((message, error)) = self.structure_message(&tab, alter) {
            let color = if error { p.error_text } else { p.text_secondary };
            let row = Rect::from_min_size(
                pos2(bar.left(), bar.bottom() + 4.0 + shown as f32 * STAGED_ROW),
                vec2(bar.width(), STAGED_ROW),
            );
            let job = vcs::truncated(&message, theme::regular(12.0), color, row.width() - 28.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(row.left() + 14.0, row.center().y - galley.size().y / 2.0), galley, color);
            accessibility::label(ui, ui.scope_id().with("database-structure-message"), row, || message.clone());
        }
        let mut unstage = None;
        for (index, change) in changes.iter().take(STAGED_SHOWN).enumerate() {
            let row = Rect::from_min_size(
                pos2(bar.left(), bar.bottom() + 4.0 + index as f32 * STAGED_ROW),
                vec2(bar.width(), STAGED_ROW),
            );
            let summary = change["summary"].as_str().unwrap_or_default();
            let mut shown = format!("{}. {summary}", index + 1);
            if index + 1 == STAGED_SHOWN && changes.len() > STAGED_SHOWN {
                shown.push_str(&t!(
                    "ide-dbalter-more",
                    count = alter["total"].as_u64().unwrap_or(0) as usize - index - 1
                ));
            }
            ui.painter().text(
                pos2(row.left() + 14.0, row.center().y),
                Align2::LEFT_CENTER,
                &shown,
                theme::regular(12.0),
                p.text,
            );
            let cross = Rect::from_center_size(pos2(row.right() - 16.0, row.center().y), vec2(16.0, 16.0));
            let response =
                ui.interact(cross, ui.scope_id().with(("database-structure-unstage", index)), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(cross, 3, p.hover);
            }
            ui.painter().text(cross.center(), Align2::CENTER_CENTER, "×", theme::regular(13.0), p.text_secondary);
            let label = t!("ide-dbalter-unstage", change = summary);
            accessibility::button_with(&response, || label.clone());
            if response.clicked() && !running {
                unstage = change["index"].as_u64();
            }
        }
        let rows = shown + usize::from(self.structure_message(&tab, alter).is_some());
        if rows > 0 {
            let bottom = bar.bottom() + 8.0 + rows as f32 * STAGED_ROW;
            ui.painter().hline(bar.x_range(), bottom - 0.5, Stroke::new(1.0, p.frame));
        }
        if let Some(index) = unstage {
            let answer = registry.dispatch(session, "db.structure.unstage", json!({ "tab": tab, "index": index }));
            self.structure_said(&tab, answer.map(|_| String::new()));
        }
    }

    fn structure_said(&mut self, tab: &str, answer: Result<String, zen_core::DispatchError>) {
        self.structure.state = None;
        self.structure.message = Some(match answer {
            Ok(text) => (tab.to_owned(), text, false),
            Err(error) => (tab.to_owned(), super::message(error), true),
        });
    }

    fn open_structure_preview(&mut self, registry: &Registry, session: &mut Session, tab: &str) {
        let answer = registry
            .dispatch(session, "db.structure.preview", json!({ "tab": tab, "limit": 2000 }))
            .map_err(super::message);
        self.structure.preview = Some(PreviewDialog { tab: tab.to_owned(), answer, error: None });
    }

    fn open_create_form(&mut self, registry: &Registry, session: &mut Session, asked: &CreateAsked) {
        let tab = asked.tab.as_str();
        let object = registry.dispatch(session, "db.object.state", json!({ "tab": tab })).unwrap_or_default();
        let alter = registry.dispatch(session, "db.structure.state", json!({ "tab": tab })).unwrap_or_default();
        let action = match asked.kind {
            CreateKind::Table => Action::CreateTable,
            CreateKind::View => Action::CreateView,
            CreateKind::Sequence => Action::CreateSequence,
            CreateKind::Function => Action::CreateRoutine,
            CreateKind::Schema => Action::CreateSchema,
            CreateKind::Column => Action::AddColumn,
            CreateKind::Index => Action::AddIndex,
        };
        let context = context_of(&object, &alter);
        self.structure.form = Some(form(tab, action, context, &Value::Null, None, None));
    }

    pub(super) fn structure_dialogs(&mut self, ui: &Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        if let Some(asked) = session.state_mut::<CreateRequest>().0.take() {
            self.open_create_form(registry, session, &asked);
        }
        if let Some(form) = self.structure.form.clone() {
            self.structure.form = self.form_dialog(ui, p, registry, session, form);
        }
        if let Some(dialog) = self.structure.preview.clone() {
            self.structure.preview = self.preview_dialog(ui, p, registry, session, dialog);
        }
    }

    fn form_dialog(
        &mut self,
        ui: &Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        mut form: Form,
    ) -> Option<Form> {
        let mut close = false;
        let mut stage = false;
        let modal = egui::Modal::new(ui.scope_id().with("database-structure-form"))
            .frame(super::super::dialog_frame(p))
            .show(ui.ctx(), |ui| {
                ui.set_width(520.0);
                ui.label(egui::RichText::new(&form.title).font(theme::semibold(15.0)).color(p.text));
                if let Some(note) = &form.note {
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(note).font(theme::regular(12.5)).color(p.warning_text))
                            .wrap(),
                    );
                }
                ui.add_space(10.0);
                egui::Grid::new(ui.scope_id().with("fields")).num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                    for (index, field) in form.fields.iter_mut().enumerate() {
                        ui.label(egui::RichText::new(&field.label).font(theme::regular(12.5)).color(p.text_secondary));
                        match &mut field.input {
                            Input::Text(value) => {
                                let response = ui.add(egui::TextEdit::singleline(value).desired_width(360.0));
                                accessibility::labelled(&response, &field.label);
                                if index == 0 && ui.memory(|memory| memory.focused().is_none()) {
                                    response.request_focus();
                                }
                            }
                            Input::Code(value) => {
                                let response = ui.add(
                                    egui::TextEdit::multiline(value).code_editor().desired_rows(6).desired_width(360.0),
                                );
                                accessibility::labelled(&response, &field.label);
                            }
                            Input::Check(on) => {
                                ui.checkbox(on, "");
                            }
                            Input::Choice(choices, at) => {
                                let shown = choices.get(*at).map(|(_, label)| label.clone()).unwrap_or_default();
                                egui::ComboBox::from_id_salt(("database-structure-choice", field.key))
                                    .selected_text(shown)
                                    .show_ui(ui, |ui| {
                                        for (index, (_, label)) in choices.iter().enumerate() {
                                            ui.selectable_value(at, index, label.as_str());
                                        }
                                    });
                            }
                        }
                        ui.end_row();
                    }
                });
                if form.action == Action::CreateTable {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(t!("ide-dbalter-columns")).font(theme::semibold(12.5)).color(p.text));
                    ui.add_space(4.0);
                    let mut remove = None;
                    egui::Grid::new(ui.scope_id().with("columns")).num_columns(6).spacing([8.0, 6.0]).show(ui, |ui| {
                        for label in [
                            t!("ide-dbalter-name"),
                            t!("ide-dbalter-type"),
                            t!("ide-dbalter-not-null"),
                            t!("ide-dbalter-primary-key"),
                            t!("ide-dbalter-identity"),
                            String::new(),
                        ] {
                            ui.label(egui::RichText::new(label).font(theme::regular(11.5)).color(p.text_secondary));
                        }
                        ui.end_row();
                        for (index, column) in form.columns.iter_mut().enumerate() {
                            let name = ui.add(egui::TextEdit::singleline(&mut column.name).desired_width(140.0));
                            accessibility::labelled(&name, &t!("ide-dbalter-name"));
                            let kind = ui.add(egui::TextEdit::singleline(&mut column.type_name).desired_width(140.0));
                            accessibility::labelled(&kind, &t!("ide-dbalter-type"));
                            ui.checkbox(&mut column.not_null, "");
                            ui.checkbox(&mut column.key, "");
                            ui.checkbox(&mut column.identity, "");
                            if ui.small_button("×").clicked() {
                                remove = Some(index);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(index) = remove {
                        form.columns.remove(index);
                    }
                    if ui.button(t!("ide-dbalter-add-row")).clicked() {
                        form.columns.push(NewColumn::default());
                    }
                }
                if let Some(error) = &form.error {
                    ui.add_space(8.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(error).font(theme::regular(12.5)).color(p.error_text))
                            .wrap(),
                    );
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if vcs::primary_button(ui, p, &t!("ide-dbalter-stage")).clicked() {
                        stage = true;
                    }
                    if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if stage {
            let params = json!({ "tab": form.tab, "change": form.change() });
            match registry.dispatch(session, "db.structure.stage", params) {
                Ok(_) => {
                    let found = form.usages().map(|usages| registry.dispatch(session, "db.usages.rename", usages));
                    self.structure.state = None;
                    self.structure.message = None;
                    if let Some(Err(error)) = found {
                        self.structure_said(&form.tab, Err(error));
                    }
                    return None;
                }
                Err(error) => form.error = Some(super::message(error)),
            }
        }
        if close || modal.should_close() { None } else { Some(form) }
    }

    fn preview_dialog(
        &mut self,
        ui: &Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        mut dialog: PreviewDialog,
    ) -> Option<PreviewDialog> {
        let mut close = false;
        let tab = dialog.tab.clone();
        let modal = egui::Modal::new(ui.scope_id().with("database-structure-preview"))
            .frame(super::super::dialog_frame(p))
            .show(ui.ctx(), |ui| {
                ui.set_width(760.0);
                ui.label(
                    egui::RichText::new(t!("ide-dbalter-preview-title")).font(theme::semibold(15.0)).color(p.text),
                );
                ui.add_space(4.0);
                let preview = match &dialog.answer {
                    Ok(preview) => preview.clone(),
                    Err(error) => {
                        ui.add(
                            egui::Label::new(egui::RichText::new(error).font(theme::regular(12.5)).color(p.error_text))
                                .wrap(),
                        );
                        ui.add_space(12.0);
                        if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                            close = true;
                        }
                        return;
                    }
                };
                let total = preview["total"].as_u64().unwrap_or(0);
                let headline = if preview["confirm"] == true {
                    t!("ide-dbalter-preview-confirm", count = total)
                } else {
                    t!("ide-dbalter-preview-note", count = total)
                };
                ui.label(
                    egui::RichText::new(headline).font(theme::regular(12.5)).color(if preview["confirm"] == true {
                        p.warning_text
                    } else {
                        p.text_secondary
                    }),
                );
                for note in preview["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("• {note}")).font(theme::regular(12.5)).color(p.warning_text),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(10.0);
                let sql = preview["sql"].as_str().unwrap_or_default().to_owned();
                let (area, _) = ui.allocate_exact_size(vec2(760.0, 340.0), Sense::hover());
                ui.painter().rect_filled(area, 6, p.editor_background);
                super::object::code_view(
                    ui,
                    p,
                    area.shrink(6.0),
                    &sql,
                    super::code::Lang::Sql,
                    "database-structure-sql",
                    &t!("ide-dbalter-preview-title"),
                );
                if let Some(error) = &dialog.error {
                    ui.add_space(8.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(error).font(theme::regular(12.5)).color(p.error_text))
                            .wrap(),
                    );
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if vcs::primary_button(ui, p, &t!("ide-dbalter-execute")).clicked() {
                        match registry.dispatch(session, "db.structure.execute", json!({ "tab": tab })) {
                            Ok(_) => close = true,
                            Err(error) if super::super::transaction_dialog::ask(session, &error) => close = true,
                            Err(error) => dialog.error = Some(super::message(error)),
                        }
                        self.structure.state = None;
                    }
                    if vcs::secondary_button(ui, p, &t!("ide-dbalter-copy")).clicked() {
                        ui.ctx().copy_text(sql.clone());
                    }
                    if vcs::secondary_button(ui, p, &t!("ide-dbalter-to-console")).clicked() {
                        match registry.dispatch(session, "db.structure.toConsole", json!({ "tab": tab })) {
                            Ok(_) => close = true,
                            Err(error) => dialog.error = Some(super::message(error)),
                        }
                    }
                    if vcs::secondary_button(ui, p, &t!("ide-dbalter-to-migration")).clicked() {
                        match registry.dispatch(session, "db.structure.saveMigration", json!({ "tab": tab })) {
                            Ok(saved) => {
                                let path = saved["path"].as_str().unwrap_or_default();
                                self.structure.message =
                                    Some((tab.clone(), t!("ide-dbalter-migration-saved", path = path), false));
                                close = true;
                            }
                            Err(error) => dialog.error = Some(super::message(error)),
                        }
                    }
                    if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if close || modal.should_close() { None } else { Some(dialog) }
    }
}
