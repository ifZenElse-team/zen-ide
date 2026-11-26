use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use crate::driver::{DbError, ErrorKind};
use crate::value::Execution;

pub const HOT: f64 = 0.2;

/// An option of Explain Plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ExplainOption {
    /// Runs the statement and reports what really happened.
    Analyze,
    /// Shows output columns, schemas and more detail.
    Verbose,
    /// Shows the planner's estimated costs.
    Costs,
    /// Shows buffer usage.
    Buffers,
    /// Shows the settings that differ from their defaults and changed the plan.
    Settings,
}

/// The options of an explain, as PostgreSQL names them; drivers ignore those they do not have.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExplainOptions {
    /// Run the statement (off by default): it executes, inside a transaction that is rolled back.
    pub analyze: bool,
    /// More detail: output columns and qualified names.
    pub verbose: bool,
    /// The estimated costs (on by default).
    pub costs: bool,
    /// Buffer usage.
    pub buffers: bool,
    /// Settings that changed the plan.
    pub settings: bool,
}

impl Default for ExplainOptions {
    fn default() -> Self {
        Self { analyze: false, verbose: false, costs: true, buffers: false, settings: false }
    }
}

impl ExplainOptions {
    pub fn is_on(&self, option: ExplainOption) -> bool {
        match option {
            ExplainOption::Analyze => self.analyze,
            ExplainOption::Verbose => self.verbose,
            ExplainOption::Costs => self.costs,
            ExplainOption::Buffers => self.buffers,
            ExplainOption::Settings => self.settings,
        }
    }

    pub fn changed(&self) -> Vec<ExplainOption> {
        let default = Self::default();
        [
            ExplainOption::Analyze,
            ExplainOption::Verbose,
            ExplainOption::Costs,
            ExplainOption::Buffers,
            ExplainOption::Settings,
        ]
        .into_iter()
        .filter(|option| self.is_on(*option) != default.is_on(*option))
        .collect()
    }
}

/// How the server's answer to an explain reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PlanFormat {
    /// PostgreSQL's `EXPLAIN (FORMAT JSON)`.
    PostgresJson,
    /// MySQL's and MariaDB's `FORMAT=JSON`.
    MysqlJson,
    /// MySQL's `EXPLAIN ANALYZE`, an indented tree of text.
    MysqlTree,
    /// SQLite's `EXPLAIN QUERY PLAN` rows: id, parent and detail.
    SqliteRows,
}

/// The statement that explains a query, and how its answer reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExplainRequest {
    /// The statement sent.
    pub text: String,
    /// How its answer reads.
    pub format: PlanFormat,
    /// Whether it runs the query (ANALYZE).
    pub executes: bool,
    /// The options asked for that this driver does not have, so left out.
    pub ignored: Vec<ExplainOption>,
}

pub fn body(statement: &str) -> &str {
    statement.trim().trim_end_matches(|c: char| c == ';' || c.is_whitespace())
}

pub fn postgres_options() -> Vec<ExplainOption> {
    vec![
        ExplainOption::Analyze,
        ExplainOption::Verbose,
        ExplainOption::Costs,
        ExplainOption::Buffers,
        ExplainOption::Settings,
    ]
}

pub fn postgres(statement: &str, options: &ExplainOptions) -> ExplainRequest {
    let mut parts = vec!["FORMAT JSON"];
    if options.analyze {
        parts.push("ANALYZE");
    }
    if options.verbose {
        parts.push("VERBOSE");
    }
    if !options.costs {
        parts.push("COSTS FALSE");
    }
    if options.buffers {
        parts.push("BUFFERS");
    } else if options.analyze {
        parts.push("BUFFERS FALSE");
    }
    if options.settings {
        parts.push("SETTINGS");
    }
    ExplainRequest {
        text: format!("EXPLAIN ({}) {}", parts.join(", "), body(statement)),
        format: PlanFormat::PostgresJson,
        executes: options.analyze,
        ignored: Vec::new(),
    }
}

pub fn mysql(statement: &str, options: &ExplainOptions, mariadb: bool) -> ExplainRequest {
    let ignored = options.changed().into_iter().filter(|option| *option != ExplainOption::Analyze).collect();
    let body = body(statement);
    let (text, format) = match (options.analyze, mariadb) {
        (true, true) => (format!("ANALYZE FORMAT=JSON {body}"), PlanFormat::MysqlJson),
        (true, false) => (format!("EXPLAIN ANALYZE {body}"), PlanFormat::MysqlTree),
        (false, _) => (format!("EXPLAIN FORMAT=JSON {body}"), PlanFormat::MysqlJson),
    };
    ExplainRequest { text, format, executes: options.analyze, ignored }
}

pub fn sqlite(statement: &str, options: &ExplainOptions) -> ExplainRequest {
    ExplainRequest {
        text: format!("EXPLAIN QUERY PLAN {}", body(statement)),
        format: PlanFormat::SqliteRows,
        executes: false,
        ignored: options.changed(),
    }
}

/// A property of a plan or of a node, as text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    /// Its name, as the server wrote it.
    pub key: String,
    /// Its value as text.
    pub value: String,
}

/// What the shares of a plan's nodes are of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Measure {
    /// The time each node took itself (an analyzed plan).
    Time,
    /// The cost each node adds itself.
    Cost,
    /// Nothing to measure: the server reports neither.
    None,
}

/// One node of a plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanNode {
    /// Its index in the plan, in tree order.
    pub id: usize,
    /// Its parent's index; `None` for a root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// How deep it is; 0 for a root.
    pub depth: usize,
    /// The operation (`Seq Scan`, `Hash Join`, `Index lookup`).
    pub node: String,
    /// What it reads: a table, an index, a CTE or a function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    /// Its conditions (filters, index and join conditions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// The estimated cost before its first row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_cost: Option<f64>,
    /// The estimated cost, with what is below it (MySQL: the cost of the join so far).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// The estimated cost it adds itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_cost: Option<f64>,
    /// The estimated rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<f64>,
    /// The rows it really returned, per loop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_rows: Option<f64>,
    /// How many times it ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loops: Option<f64>,
    /// Milliseconds it took over all its loops, with what is below it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_ms: Option<f64>,
    /// Milliseconds it took itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_time_ms: Option<f64>,
    /// Its share of the plan's time (analyzed) or cost, from 0 to 1.
    pub share: f64,
    /// Whether its share is at least `HOT`.
    pub hot: bool,
    /// How many nodes are right below it.
    pub children: usize,
    /// Everything the server said about it.
    pub details: Vec<Detail>,
}

/// A plan read into a tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// How the server's answer read.
    pub format: PlanFormat,
    /// The nodes in tree order: each node before its children.
    pub nodes: Vec<PlanNode>,
    /// Whether the statement ran (ANALYZE), so times and actual rows are known.
    pub analyzed: bool,
    /// What the shares are of.
    pub measure: Measure,
    /// The estimated cost of the whole plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost: Option<f64>,
    /// Milliseconds the whole plan took.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_time_ms: Option<f64>,
    /// Milliseconds of planning, when the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planning_ms: Option<f64>,
    /// Milliseconds of execution, when the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_ms: Option<f64>,
    /// What the server said about the plan as a whole: settings, planning buffers, triggers.
    pub details: Vec<Detail>,
    /// The answer as the server sent it.
    pub raw: String,
}

#[derive(Default)]
struct Draft {
    node: String,
    entity: Option<String>,
    condition: Option<String>,
    startup_cost: Option<f64>,
    cost: Option<f64>,
    self_cost: Option<f64>,
    rows: Option<f64>,
    actual_rows: Option<f64>,
    loops: Option<f64>,
    time_ms: Option<f64>,
    self_time_ms: Option<f64>,
    details: Vec<Detail>,
    children: Vec<Draft>,
}

impl Draft {
    fn inclusive_time(&self) -> Option<f64> {
        self.time_ms.or_else(|| sum(self.children.iter().map(Draft::inclusive_time)))
    }

    fn inclusive_cost(&self) -> Option<f64> {
        self.cost.or(self.self_cost).or_else(|| sum(self.children.iter().map(Draft::inclusive_cost)))
    }

    fn settle(&mut self) {
        for child in &mut self.children {
            child.settle();
        }
        if self.self_time_ms.is_none()
            && let Some(time) = self.time_ms
        {
            let below = sum(self.children.iter().map(Draft::inclusive_time)).unwrap_or(0.0);
            self.self_time_ms = Some((time - below).max(0.0));
        }
        if self.self_cost.is_none()
            && let Some(cost) = self.cost
        {
            let below = sum(self.children.iter().map(Draft::inclusive_cost)).unwrap_or(0.0);
            self.self_cost = Some((cost - below).max(0.0));
        }
    }
}

fn sum(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    values.flatten().fold(None, |total, value| Some(total.unwrap_or(0.0) + value))
}

fn flatten(drafts: Vec<Draft>, parent: Option<usize>, depth: usize, nodes: &mut Vec<PlanNode>) {
    for draft in drafts {
        let id = nodes.len();
        nodes.push(PlanNode {
            id,
            parent,
            depth,
            node: draft.node,
            entity: draft.entity,
            condition: draft.condition,
            startup_cost: draft.startup_cost,
            cost: draft.cost,
            self_cost: draft.self_cost,
            rows: draft.rows,
            actual_rows: draft.actual_rows,
            loops: draft.loops,
            time_ms: draft.time_ms,
            self_time_ms: draft.self_time_ms,
            share: 0.0,
            hot: false,
            children: draft.children.len(),
            details: draft.details,
        });
        flatten(draft.children, Some(id), depth + 1, nodes);
    }
}

struct Totals {
    analyzed: bool,
    cost: Option<f64>,
    planning_ms: Option<f64>,
    execution_ms: Option<f64>,
    details: Vec<Detail>,
}

fn finish(format: PlanFormat, mut roots: Vec<Draft>, totals: Totals, raw: String) -> Plan {
    for root in &mut roots {
        root.settle();
    }
    let total_time = totals.execution_ms.or_else(|| sum(roots.iter().map(Draft::inclusive_time)));
    let total_cost = totals.cost.or_else(|| sum(roots.iter().map(Draft::inclusive_cost)));
    let mut nodes = Vec::new();
    flatten(roots, None, 0, &mut nodes);
    let timed = nodes.iter().any(|node| node.self_time_ms.is_some());
    let measure = if timed && total_time.is_some_and(|total| total > 0.0) {
        Measure::Time
    } else if total_cost.is_some_and(|total| total > 0.0) && nodes.iter().any(|node| node.self_cost.is_some()) {
        Measure::Cost
    } else {
        Measure::None
    };
    for node in &mut nodes {
        let (part, whole) = match measure {
            Measure::Time => (node.self_time_ms, total_time),
            Measure::Cost => (node.self_cost, total_cost),
            Measure::None => (None, None),
        };
        node.share = match (part, whole) {
            (Some(part), Some(whole)) if whole > 0.0 => (part / whole).clamp(0.0, 1.0),
            _ => 0.0,
        };
        node.hot = node.share >= HOT;
    }
    Plan {
        format,
        nodes,
        analyzed: totals.analyzed,
        measure,
        total_cost,
        total_time_ms: total_time.filter(|_| timed),
        planning_ms: totals.planning_ms,
        execution_ms: totals.execution_ms,
        details: totals.details,
        raw,
    }
}

fn invalid(what: &str) -> DbError {
    DbError::new(ErrorKind::Invalid, format!("the plan could not be read: {what}"))
}

fn text_of(execution: &Execution) -> String {
    execution
        .rows
        .iter()
        .filter_map(|row| row.first().and_then(crate::value::Value::display))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse(format: PlanFormat, execution: &Execution, analyzed: bool) -> Result<Plan, DbError> {
    match format {
        PlanFormat::PostgresJson => {
            let raw = text_of(execution);
            let json: Json = serde_json::from_str(&raw).map_err(|error| invalid(&error.to_string()))?;
            postgres_plan(&json, raw, analyzed)
        }
        PlanFormat::MysqlJson => {
            let raw = text_of(execution);
            let json: Json = serde_json::from_str(&raw).map_err(|error| invalid(&error.to_string()))?;
            mysql_plan(&json, raw, analyzed)
        }
        PlanFormat::MysqlTree => Ok(tree_plan(&text_of(execution))),
        PlanFormat::SqliteRows => Ok(sqlite_plan(execution)),
    }
}

fn number(value: Option<&Json>) -> Option<f64> {
    match value? {
        Json::Number(number) => number.as_f64(),
        Json::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn scalar_text(value: &Json) -> Option<String> {
    match value {
        Json::String(text) => Some(text.clone()),
        Json::Number(number) => Some(number.to_string()),
        Json::Bool(flag) => Some(flag.to_string()),
        Json::Null => Some("null".to_owned()),
        Json::Array(items) if items.iter().all(|item| !item.is_object() && !item.is_array()) => {
            Some(items.iter().filter_map(scalar_text).collect::<Vec<_>>().join(", "))
        }
        _ => None,
    }
}

fn details_of(object: &Map<String, Json>, skip: &[&str], prefix: &str, details: &mut Vec<Detail>) {
    for (key, value) in object {
        if skip.contains(&key.as_str()) {
            continue;
        }
        let key = if prefix.is_empty() { key.clone() } else { format!("{prefix}{key}") };
        match scalar_text(value) {
            Some(text) => details.push(Detail { key, value: text }),
            None => details.push(Detail { key, value: value.to_string() }),
        }
    }
}

const PG_CONDITIONS: &[&str] =
    &["Index Cond", "Recheck Cond", "Hash Cond", "Merge Cond", "Join Filter", "Filter", "TID Cond", "One-Time Filter"];

fn postgres_name(object: &Map<String, Json>) -> String {
    let text = |key: &str| object.get(key).and_then(Json::as_str);
    let kind = text("Node Type").unwrap_or("Node");
    let mut name = match (kind, text("Strategy")) {
        ("Aggregate", Some("Hashed")) => "HashAggregate".to_owned(),
        ("Aggregate", Some("Sorted")) => "GroupAggregate".to_owned(),
        ("Aggregate", Some("Mixed")) => "MixedAggregate".to_owned(),
        ("SetOp", Some("Hashed")) => "HashSetOp".to_owned(),
        ("ModifyTable", _) => text("Operation").unwrap_or(kind).to_owned(),
        _ => kind.to_owned(),
    };
    if text("Scan Direction") == Some("Backward") {
        name.push_str(" Backward");
    }
    if let Some(join) = text("Join Type").filter(|join| *join != "Inner") {
        name = match name.strip_suffix(" Join") {
            Some(base) => format!("{base} {join} Join"),
            None => format!("{name} ({join})"),
        };
    }
    if let Some(mode) = text("Partial Mode").filter(|mode| *mode != "Simple") {
        name = format!("{mode} {name}");
    }
    if object.get("Parallel Aware") == Some(&Json::Bool(true)) {
        name = format!("Parallel {name}");
    }
    name
}

fn postgres_entity(object: &Map<String, Json>) -> Option<String> {
    let text = |key: &str| object.get(key).and_then(Json::as_str);
    let relation = text("Relation Name").map(|relation| match text("Schema") {
        Some(schema) => format!("{schema}.{relation}"),
        None => relation.to_owned(),
    });
    let named = relation
        .or_else(|| text("CTE Name").map(|name| format!("CTE {name}")))
        .or_else(|| text("Function Name").map(str::to_owned))
        .or_else(|| text("Subplan Name").map(str::to_owned));
    let named = match (named, text("Alias"), text("Relation Name")) {
        (Some(named), Some(alias), Some(relation)) if alias != relation => Some(format!("{named} {alias}")),
        (named, _, _) => named,
    };
    match (named, text("Index Name")) {
        (Some(named), Some(index)) => Some(format!("{named} using {index}")),
        (None, Some(index)) => Some(index.to_owned()),
        (named, None) => named,
    }
}

fn postgres_node(object: &Map<String, Json>, depth: usize) -> Draft {
    let conditions: Vec<String> =
        PG_CONDITIONS.iter().filter_map(|key| object.get(*key).and_then(Json::as_str).map(str::to_owned)).collect();
    let loops = number(object.get("Actual Loops"));
    let time_ms = match (number(object.get("Actual Total Time")), loops) {
        (Some(time), Some(loops)) if loops > 0.0 => Some(time * loops),
        _ => None,
    };
    let mut details = Vec::new();
    details_of(object, &["Plans"], "", &mut details);
    let children = match object.get("Plans") {
        Some(Json::Array(plans)) if depth < MAX_DEPTH => {
            plans.iter().filter_map(Json::as_object).map(|plan| postgres_node(plan, depth + 1)).collect()
        }
        _ => Vec::new(),
    };
    Draft {
        node: postgres_name(object),
        entity: postgres_entity(object),
        condition: (!conditions.is_empty()).then(|| conditions.join("; ")),
        startup_cost: number(object.get("Startup Cost")),
        cost: number(object.get("Total Cost")),
        self_cost: None,
        rows: number(object.get("Plan Rows")),
        actual_rows: number(object.get("Actual Rows")),
        loops,
        time_ms,
        self_time_ms: None,
        details,
        children,
    }
}

const MAX_DEPTH: usize = 200;

fn postgres_plan(json: &Json, raw: String, analyzed: bool) -> Result<Plan, DbError> {
    let top = match json {
        Json::Array(items) => items.first().and_then(Json::as_object),
        Json::Object(object) => Some(object),
        _ => None,
    }
    .ok_or_else(|| invalid("no plan"))?;
    let plan = top.get("Plan").and_then(Json::as_object).ok_or_else(|| invalid("no Plan"))?;
    let root = postgres_node(plan, 0);
    let mut details = Vec::new();
    for (key, value) in top {
        match (key.as_str(), value) {
            ("Plan" | "Planning Time" | "Execution Time", _) => {}
            (_, Json::Object(object)) => details_of(object, &[], &format!("{key}: "), &mut details),
            ("Triggers", Json::Array(triggers)) => {
                for trigger in triggers.iter().filter_map(Json::as_object) {
                    let name = trigger.get("Trigger Name").and_then(Json::as_str).unwrap_or("trigger");
                    let time = number(trigger.get("Time")).map(|time| format!("{time} ms")).unwrap_or_default();
                    let calls =
                        number(trigger.get("Calls")).map(|calls| format!(", {calls} calls")).unwrap_or_default();
                    details.push(Detail { key: format!("Trigger {name}"), value: format!("{time}{calls}") });
                }
            }
            _ => details
                .push(Detail { key: key.clone(), value: scalar_text(value).unwrap_or_else(|| value.to_string()) }),
        }
    }
    let totals = Totals {
        analyzed,
        cost: root.cost,
        planning_ms: number(top.get("Planning Time")),
        execution_ms: number(top.get("Execution Time")),
        details,
    };
    Ok(finish(PlanFormat::PostgresJson, vec![root], totals, raw))
}

fn mysql_access(access: &str) -> String {
    match access {
        "ALL" => "Full Table Scan",
        "index" => "Full Index Scan",
        "range" => "Index Range Scan",
        "ref" => "Index Lookup",
        "eq_ref" => "Unique Index Lookup",
        "ref_or_null" => "Index Lookup or Null",
        "const" | "system" => "Constant Row",
        "index_merge" => "Index Merge",
        "fulltext" => "Full-text Index",
        "unique_subquery" | "index_subquery" => "Index Subquery",
        other => return format!("Table ({other})"),
    }
    .to_owned()
}

fn mysql_label(key: &str) -> String {
    match key {
        "query_block" => "Query Block".to_owned(),
        "nested_loop" => "Nested Loop".to_owned(),
        "ordering_operation" => "Order".to_owned(),
        "grouping_operation" => "Group".to_owned(),
        "duplicates_removal" => "Duplicates Removal".to_owned(),
        "filesort" => "Sort".to_owned(),
        "temporary_table" => "Temporary Table".to_owned(),
        "materialized_from_subquery" => "Materialized Subquery".to_owned(),
        "union_result" => "Union".to_owned(),
        "windowing" => "Window".to_owned(),
        other => {
            let words = other.replace('_', " ");
            let mut chars = words.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    }
}

const MYSQL_CONDITIONS: &[&str] = &["attached_condition", "index_condition", "having_condition"];

fn all_scalar(object: &Map<String, Json>) -> bool {
    object.values().all(|value| scalar_text(value).is_some())
}

fn mysql_node(key: &str, object: &Map<String, Json>, depth: usize) -> Draft {
    let mut draft = Draft { node: mysql_label(key), ..Draft::default() };
    let table = key == "table";
    let text = |name: &str| object.get(name).and_then(Json::as_str);
    if table {
        draft.node = text("access_type").map_or_else(|| "Table".to_owned(), mysql_access);
        draft.entity = text("table_name").map(|name| match text("key") {
            Some(key) => format!("{name} using {key}"),
            None => name.to_owned(),
        });
    }
    let conditions: Vec<String> = MYSQL_CONDITIONS.iter().filter_map(|name| text(name).map(str::to_owned)).collect();
    draft.condition = (!conditions.is_empty()).then(|| conditions.join("; "));
    let costs = object.get("cost_info").and_then(Json::as_object);
    let cost_of = |name: &str| costs.and_then(|costs| number(costs.get(name)));
    draft.cost = cost_of("query_cost").or_else(|| cost_of("prefix_cost")).or_else(|| number(object.get("cost")));
    if table {
        draft.self_cost = match (cost_of("read_cost"), cost_of("eval_cost")) {
            (None, None) => number(object.get("cost")),
            (read, eval) => Some(read.unwrap_or(0.0) + eval.unwrap_or(0.0)),
        };
        draft.rows = number(object.get("rows_produced_per_join")).or_else(|| number(object.get("rows")));
        let own = sum([number(object.get("r_table_time_ms")), number(object.get("r_other_time_ms"))].into_iter());
        draft.self_time_ms = own;
        draft.time_ms = number(object.get("r_total_time_ms")).or(own);
    } else {
        draft.time_ms = number(object.get("r_total_time_ms"));
    }
    draft.actual_rows = number(object.get("r_rows")).or_else(|| number(object.get("r_output_rows")));
    draft.loops = number(object.get("r_loops"));
    for (name, value) in object {
        match value {
            Json::Object(inner) if name == "cost_info" || all_scalar(inner) => {
                details_of(inner, &[], &format!("{name}."), &mut draft.details)
            }
            Json::Object(inner) if depth < MAX_DEPTH => draft.children.push(mysql_node(name, inner, depth + 1)),
            Json::Array(items) if items.iter().any(Json::is_object) && depth < MAX_DEPTH => {
                let mut group = Draft { node: mysql_label(name), ..Draft::default() };
                for item in items.iter().filter_map(Json::as_object) {
                    let single = (item.len() == 1).then(|| item.iter().next()).flatten();
                    match single {
                        Some((inner_key, Json::Object(inner))) => {
                            group.children.push(mysql_node(inner_key, inner, depth + 2))
                        }
                        _ => group.children.push(mysql_node(name, item, depth + 2)),
                    }
                }
                draft.children.push(group);
            }
            _ => draft
                .details
                .push(Detail { key: name.clone(), value: scalar_text(value).unwrap_or_else(|| value.to_string()) }),
        }
    }
    draft
}

fn mysql_plan(json: &Json, raw: String, analyzed: bool) -> Result<Plan, DbError> {
    let top = json.as_object().ok_or_else(|| invalid("no plan"))?;
    let mut roots = Vec::new();
    let mut details = Vec::new();
    for (key, value) in top {
        match value {
            Json::Object(inner) if key == "query_block" => roots.push(mysql_node(key, inner, 0)),
            Json::Object(inner) => details_of(inner, &[], &format!("{key}."), &mut details),
            _ => details
                .push(Detail { key: key.clone(), value: scalar_text(value).unwrap_or_else(|| value.to_string()) }),
        }
    }
    if roots.is_empty() {
        return Err(invalid("no query_block"));
    }
    let cost = roots.first().and_then(|root| root.cost);
    let totals = Totals { analyzed, cost, planning_ms: None, execution_ms: None, details };
    Ok(finish(PlanFormat::MysqlJson, roots, totals, raw))
}

fn tree_measures(label: &str) -> (String, Vec<(String, String)>) {
    let mut rest = label.trim_end().to_owned();
    let mut groups = Vec::new();
    while rest.ends_with(')') {
        let Some(open) = rest.rfind(" (") else { break };
        let group = rest[open + 2..rest.len() - 1].to_owned();
        if !(group.starts_with("cost=") || group.starts_with("actual ") || group == "never executed") {
            break;
        }
        groups.push(group);
        rest.truncate(open);
        rest = rest.trim_end().to_owned();
    }
    let mut pairs = Vec::new();
    for group in groups.iter().rev() {
        if group == "never executed" {
            pairs.push(("never executed".to_owned(), "true".to_owned()));
            continue;
        }
        let actual = group.starts_with("actual ");
        let group = group.strip_prefix("actual ").unwrap_or(group);
        for pair in group.split_whitespace() {
            if let Some((key, value)) = pair.split_once('=') {
                let key = if actual { format!("actual {key}") } else { key.to_owned() };
                pairs.push((key, value.to_owned()));
            }
        }
    }
    (rest, pairs)
}

fn tree_node(label: &str) -> Draft {
    let (label, pairs) = tree_measures(label);
    let pair = |name: &str| pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str());
    let parse = |name: &str| pair(name).and_then(|value| value.parse::<f64>().ok());
    let mut draft = Draft { node: label.clone(), ..Draft::default() };
    let (head, condition) = match label.split_once(", with index condition: ") {
        Some((head, condition)) => (head.to_owned(), Some(condition.to_owned())),
        None => (label.clone(), None),
    };
    draft.condition = condition;
    if let Some((name, after)) = head.split_once(" on ") {
        draft.node = name.to_owned();
        let mut words = after.split_whitespace();
        let table = words.next().unwrap_or_default().to_owned();
        draft.entity = Some(match (words.next(), words.next()) {
            (Some("using"), Some(index)) => format!("{table} using {index}"),
            _ => table,
        });
        draft.details.push(Detail { key: "detail".to_owned(), value: head.clone() });
    } else if let Some((name, value)) = head.split_once(": ") {
        draft.node = name.to_owned();
        if name == "Filter" {
            draft.condition = Some(value.to_owned());
        } else {
            draft.details.push(Detail { key: name.to_owned(), value: value.to_owned() });
        }
    }
    draft.cost = parse("cost");
    draft.rows = parse("rows");
    draft.actual_rows = parse("actual rows");
    draft.loops = parse("actual loops");
    let last =
        pair("actual time").and_then(|time| time.split_once("..")).and_then(|(_, last)| last.parse::<f64>().ok());
    draft.time_ms = match (last, draft.loops) {
        (Some(last), Some(loops)) if loops > 0.0 => Some(last * loops),
        _ => None,
    };
    for (key, value) in pairs {
        draft.details.push(Detail { key, value });
    }
    draft
}

fn tree_plan(raw: &str) -> Plan {
    let mut lines: Vec<(usize, String)> = Vec::new();
    for line in raw.lines() {
        let indent = line.len() - line.trim_start().len();
        match line.trim_start().strip_prefix("-> ") {
            Some(label) => lines.push((indent, label.to_owned())),
            None if !line.trim().is_empty() => match lines.last_mut() {
                Some((_, label)) => {
                    label.push(' ');
                    label.push_str(line.trim());
                }
                None => lines.push((indent, line.trim().to_owned())),
            },
            None => {}
        }
    }
    let mut stack: Vec<(usize, Draft)> = Vec::new();
    let mut roots = Vec::new();
    let close = |stack: &mut Vec<(usize, Draft)>, roots: &mut Vec<Draft>| {
        if let Some((_, done)) = stack.pop() {
            match stack.last_mut() {
                Some((_, parent)) => parent.children.push(done),
                None => roots.push(done),
            }
        }
    };
    for (indent, label) in lines {
        while stack.last().is_some_and(|(open, _)| *open >= indent) {
            close(&mut stack, &mut roots);
        }
        if stack.len() < MAX_DEPTH {
            stack.push((indent, tree_node(&label)));
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut roots);
    }
    let analyzed = roots.iter().any(|root| root.time_ms.is_some());
    let totals = Totals { analyzed, cost: None, planning_ms: None, execution_ms: None, details: Vec::new() };
    finish(PlanFormat::MysqlTree, roots, totals, raw.to_owned())
}

fn sqlite_node(detail: &str) -> Draft {
    let mut draft = Draft { node: detail.to_owned(), ..Draft::default() };
    let mut words = detail.split_whitespace();
    if let (Some(verb @ ("SCAN" | "SEARCH")), Some(table)) = (words.next(), words.next()) {
        draft.node = verb.to_owned();
        let rest: Vec<&str> = words.collect();
        let index = rest.iter().position(|word| *word == "INDEX").and_then(|at| rest.get(at + 1));
        draft.entity = Some(match index {
            Some(index) => format!("{table} using {index}"),
            None => table.to_owned(),
        });
        if let (Some(open), true) = (detail.rfind(" ("), detail.ends_with(')')) {
            draft.condition = Some(detail[open + 2..detail.len() - 1].to_owned());
        }
    }
    draft.details.push(Detail { key: "detail".to_owned(), value: detail.to_owned() });
    draft
}

fn sqlite_plan(execution: &Execution) -> Plan {
    let column = |name: &str| execution.columns.iter().position(|column| column.name.eq_ignore_ascii_case(name));
    let (id_at, parent_at) = (column("id").unwrap_or(0), column("parent").unwrap_or(1));
    let detail_at = column("detail").unwrap_or(3);
    let rows: Vec<(i64, i64, String)> = execution
        .rows
        .iter()
        .filter_map(|row| {
            let cell = |at: usize| row.get(at).and_then(crate::value::Value::display);
            let id = cell(id_at)?.parse().ok()?;
            let parent = cell(parent_at).and_then(|parent| parent.parse().ok()).unwrap_or(0);
            Some((id, parent, cell(detail_at).unwrap_or_default()))
        })
        .collect();
    fn build(rows: &[(i64, i64, String)], parent: i64, depth: usize) -> Vec<Draft> {
        rows.iter()
            .filter(|(id, of, _)| *of == parent && *id != parent)
            .map(|(id, _, detail)| {
                let mut draft = sqlite_node(detail);
                if depth < MAX_DEPTH {
                    draft.children = build(rows, *id, depth + 1);
                }
                draft
            })
            .collect()
    }
    let roots = build(&rows, 0, 0);
    let mut raw = String::from("QUERY PLAN\n");
    fn draw(drafts: &[Draft], prefix: &str, raw: &mut String) {
        for (at, draft) in drafts.iter().enumerate() {
            let last = at + 1 == drafts.len();
            let detail = draft.details.first().map_or(draft.node.as_str(), |detail| detail.value.as_str());
            raw.push_str(&format!("{prefix}{}{detail}\n", if last { "`--" } else { "|--" }));
            draw(&draft.children, &format!("{prefix}{}", if last { "   " } else { "|  " }), raw);
        }
    }
    draw(&roots, "", &mut raw);
    let totals = Totals { analyzed: false, cost: None, planning_ms: None, execution_ms: None, details: Vec::new() };
    finish(PlanFormat::SqliteRows, roots, totals, raw.trim_end().to_owned())
}

#[cfg(test)]
mod tests;
