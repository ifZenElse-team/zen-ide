use super::*;
use crate::value::{Column, Execution, Value};

fn column(name: &str) -> Column {
    Column { name: name.to_owned(), type_name: "text".to_owned(), source: None, origin: None }
}

fn one_cell(text: &str) -> Execution {
    Execution {
        columns: vec![column("QUERY PLAN")],
        rows: vec![vec![Value::Json(text.to_owned())]],
        ..Execution::default()
    }
}

const PG_ANALYZED: &str = r#"[
  {
    "Plan": {
      "Node Type": "Sort",
      "Parallel Aware": false,
      "Startup Cost": 23.74,
      "Total Cost": 23.77,
      "Plan Rows": 10,
      "Actual Total Time": 0.112,
      "Actual Rows": 10.00,
      "Actual Loops": 1,
      "Sort Key": ["(sum(o.total)) DESC"],
      "Plans": [
        {
          "Node Type": "Aggregate",
          "Strategy": "Hashed",
          "Partial Mode": "Simple",
          "Startup Cost": 23.45,
          "Total Cost": 23.58,
          "Plan Rows": 10,
          "Actual Total Time": 0.108,
          "Actual Rows": 10.00,
          "Actual Loops": 1,
          "Group Key": ["o.customer"],
          "Plans": [
            {
              "Node Type": "Seq Scan",
              "Relation Name": "orders",
              "Schema": "public",
              "Alias": "o",
              "Startup Cost": 0.00,
              "Total Cost": 18.50,
              "Plan Rows": 990,
              "Actual Total Time": 0.039,
              "Actual Rows": 990.00,
              "Actual Loops": 1,
              "Filter": "(o.id > 10)",
              "Rows Removed by Filter": 10
            }
          ]
        }
      ]
    },
    "Settings": {"work_mem": "64MB"},
    "Planning Time": 0.120,
    "Triggers": [],
    "Execution Time": 0.126
  }
]"#;

#[test]
fn postgres_options_build_one_explain() {
    let plain = postgres("SELECT 1;  ", &ExplainOptions::default());
    assert_eq!(plain.text, "EXPLAIN (FORMAT JSON) SELECT 1");
    assert!(!plain.executes);
    let all = ExplainOptions { analyze: true, verbose: true, costs: false, buffers: true, settings: true };
    let request = postgres("DELETE FROM orders", &all);
    assert_eq!(
        request.text,
        "EXPLAIN (FORMAT JSON, ANALYZE, VERBOSE, COSTS FALSE, BUFFERS, SETTINGS) DELETE FROM orders"
    );
    assert!(request.executes);
    assert!(request.ignored.is_empty());
    let analyze = postgres("SELECT 1", &ExplainOptions { analyze: true, ..ExplainOptions::default() });
    assert_eq!(
        analyze.text, "EXPLAIN (FORMAT JSON, ANALYZE, BUFFERS FALSE) SELECT 1",
        "PostgreSQL 18 turns BUFFERS on with ANALYZE"
    );
}

#[test]
fn mysql_and_mariadb_explain_in_json_and_say_what_they_ignore() {
    let options = ExplainOptions { verbose: true, ..ExplainOptions::default() };
    let request = mysql("SELECT * FROM t;", &options, false);
    assert_eq!(request.text, "EXPLAIN FORMAT=JSON SELECT * FROM t");
    assert_eq!(request.format, PlanFormat::MysqlJson);
    assert_eq!(request.ignored, [ExplainOption::Verbose]);
    let analyze = ExplainOptions { analyze: true, ..ExplainOptions::default() };
    assert_eq!(mysql("SELECT 1", &analyze, false).text, "EXPLAIN ANALYZE SELECT 1");
    assert_eq!(mysql("SELECT 1", &analyze, false).format, PlanFormat::MysqlTree);
    assert_eq!(mysql("SELECT 1", &analyze, true).text, "ANALYZE FORMAT=JSON SELECT 1");
    let lite = sqlite("SELECT 1", &analyze);
    assert_eq!(lite.text, "EXPLAIN QUERY PLAN SELECT 1");
    assert!(!lite.executes, "SQLite has no ANALYZE: nothing runs");
    assert_eq!(lite.ignored, [ExplainOption::Analyze]);
}

#[test]
fn a_postgres_plan_reads_into_a_tree_with_self_times_and_hot_nodes() {
    let plan = parse(PlanFormat::PostgresJson, &one_cell(PG_ANALYZED), true).unwrap();
    let names: Vec<(&str, usize)> = plan.nodes.iter().map(|node| (node.node.as_str(), node.depth)).collect();
    assert_eq!(names, [("Sort", 0), ("HashAggregate", 1), ("Seq Scan", 2)]);
    let scan = &plan.nodes[2];
    assert_eq!(scan.entity.as_deref(), Some("public.orders o"));
    assert_eq!(scan.condition.as_deref(), Some("(o.id > 10)"));
    assert_eq!((scan.cost, scan.rows, scan.actual_rows), (Some(18.5), Some(990.0), Some(990.0)));
    assert_eq!(scan.parent, Some(1));
    assert_eq!(plan.measure, Measure::Time);
    assert_eq!(plan.execution_ms, Some(0.126));
    assert_eq!(plan.planning_ms, Some(0.12));
    let aggregate = &plan.nodes[1];
    assert!((aggregate.self_time_ms.unwrap() - 0.069).abs() < 1e-9, "{aggregate:?}");
    assert!(aggregate.hot && scan.hot, "{:?}", plan.nodes.iter().map(|node| node.share).collect::<Vec<_>>());
    assert!(!plan.nodes[0].hot, "the sort took 0.004 of 0.126 ms");
    assert!(plan.details.iter().any(|detail| detail.key == "Settings: work_mem" && detail.value == "64MB"));
    assert!(scan.details.iter().any(|detail| detail.key == "Rows Removed by Filter" && detail.value == "10"));
}

#[test]
fn an_estimated_postgres_plan_measures_cost_and_names_joins() {
    let text = r#"[{"Plan": {"Node Type": "Hash Join", "Join Type": "Left", "Total Cost": 100.0, "Startup Cost": 5.0,
        "Plan Rows": 50, "Hash Cond": "(o.customer = c.id)", "Plans": [
          {"Node Type": "Index Scan", "Scan Direction": "Backward", "Relation Name": "orders", "Alias": "orders", "Index Name": "orders_pkey", "Total Cost": 80.0, "Plan Rows": 1000, "Index Cond": "(id > 5)"},
          {"Node Type": "Hash", "Total Cost": 10.0, "Plan Rows": 10, "Plans": [
            {"Node Type": "Seq Scan", "Relation Name": "customers", "Alias": "c", "Total Cost": 10.0, "Plan Rows": 10}]}]}}]"#;
    let plan = parse(PlanFormat::PostgresJson, &one_cell(text), false).unwrap();
    assert_eq!(plan.nodes[0].node, "Hash Left Join");
    assert_eq!(plan.nodes[1].node, "Index Scan Backward");
    assert_eq!(plan.nodes[1].entity.as_deref(), Some("orders using orders_pkey"));
    assert_eq!(plan.measure, Measure::Cost);
    assert_eq!(plan.total_cost, Some(100.0));
    assert_eq!(plan.nodes[0].self_cost, Some(10.0));
    assert_eq!(plan.nodes[3].self_cost, Some(10.0), "a hash adds nothing over its scan: {:?}", plan.nodes[3]);
    assert!(plan.nodes[1].hot && (plan.nodes[1].share - 0.8).abs() < 1e-9);
    assert!(!plan.nodes[2].hot);
    assert_eq!(plan.total_time_ms, None);
}

#[test]
fn a_broken_answer_is_an_error_not_a_panic() {
    let error = parse(PlanFormat::PostgresJson, &one_cell("not json"), false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Invalid);
    assert!(parse(PlanFormat::MysqlJson, &one_cell("{}"), false).is_err());
    assert!(parse(PlanFormat::PostgresJson, &Execution::default(), false).is_err());
}

#[test]
fn a_mysql_json_plan_reads_tables_with_their_costs() {
    let text = r#"{"query_block": {"select_id": 1, "cost_info": {"query_cost": "131.25"},
      "ordering_operation": {"using_filesort": true, "grouping_operation": {"using_temporary_table": true,
        "nested_loop": [
          {"table": {"table_name": "c", "access_type": "ALL", "possible_keys": ["PRIMARY"], "rows_produced_per_join": 10,
            "cost_info": {"read_cost": "0.25", "eval_cost": "1.00", "prefix_cost": "1.25", "data_read_per_join": "160"}}},
          {"table": {"table_name": "o", "access_type": "ref", "key": "customer", "rows_produced_per_join": 990,
            "index_condition": "(`shop`.`o`.`id` > 10)",
            "cost_info": {"read_cost": "30.00", "eval_cost": "99.00", "prefix_cost": "131.25", "data_read_per_join": "15K"}}}]}}}}"#;
    let plan = parse(PlanFormat::MysqlJson, &one_cell(text), false).unwrap();
    let names: Vec<&str> = plan.nodes.iter().map(|node| node.node.as_str()).collect();
    assert_eq!(names, ["Query Block", "Order", "Group", "Nested Loop", "Full Table Scan", "Index Lookup"]);
    let lookup = &plan.nodes[5];
    assert_eq!(lookup.entity.as_deref(), Some("o using customer"));
    assert_eq!(lookup.condition.as_deref(), Some("(`shop`.`o`.`id` > 10)"));
    assert_eq!((lookup.self_cost, lookup.rows, lookup.depth), (Some(129.0), Some(990.0), 4));
    assert_eq!(plan.total_cost, Some(131.25));
    assert!(lookup.hot && !plan.nodes[4].hot);
    assert!(lookup.details.iter().any(|detail| detail.key == "cost_info.data_read_per_join" && detail.value == "15K"));
}

#[test]
fn a_mariadb_analyze_reads_its_times() {
    let text = r#"{"query_optimization": {"r_total_time_ms": 0.03},
      "query_block": {"select_id": 1, "cost": 0.0387, "r_loops": 1, "r_total_time_ms": 1.5,
        "filesort": {"sort_key": "c.`name`", "r_total_time_ms": 0.02, "r_output_rows": 10,
          "temporary_table": {"nested_loop": [
            {"table": {"table_name": "c", "access_type": "ALL", "rows": 10, "r_rows": 10, "cost": 0.012, "r_loops": 1,
              "r_table_time_ms": 0.02, "r_other_time_ms": 0.01, "r_engine_stats": {"pages_accessed": 1}}},
            {"table": {"table_name": "o", "access_type": "ref", "key": "customer", "rows": 1, "r_rows": 99, "cost": 0.0198,
              "r_loops": 10, "r_table_time_ms": 0.34, "r_other_time_ms": 1.1, "index_condition": "o.`id` > 10"}}]}}}}"#;
    let plan = parse(PlanFormat::MysqlJson, &one_cell(text), true).unwrap();
    assert!(plan.analyzed);
    assert_eq!(plan.measure, Measure::Time);
    let lookup = plan.nodes.iter().find(|node| node.entity.as_deref() == Some("o using customer")).unwrap();
    assert!((lookup.self_time_ms.unwrap() - 1.44).abs() < 1e-9);
    assert_eq!((lookup.actual_rows, lookup.loops), (Some(99.0), Some(10.0)));
    assert!(lookup.hot);
    assert!(plan.details.iter().any(|detail| detail.key == "query_optimization.r_total_time_ms"));
}

#[test]
fn a_mysql_analyze_tree_reads_into_nodes() {
    let text = "-> Sort: `sum(o.total)` DESC  (actual time=0.816..0.816 rows=10 loops=1)
    -> Table scan on <temporary>  (actual time=0.74..0.742 rows=10 loops=1)
        -> Aggregate using temporary table  (actual time=0.739..0.739 rows=10 loops=1)
            -> Nested loop inner join  (cost=132 rows=1000) (actual time=0.0681..0.418 rows=990 loops=1)
                -> Table scan on c  (cost=1.25 rows=10) (actual time=0.0163..0.0192 rows=10 loops=1)
                -> Index lookup on o using customer (customer=c.id), with index condition: (o.id > 10)  (cost=3.99 rows=100) (actual time=0.0343..0.037 rows=99 loops=10)
";
    let plan = parse(PlanFormat::MysqlTree, &one_cell(text), true).unwrap();
    let names: Vec<(&str, usize)> = plan.nodes.iter().map(|node| (node.node.as_str(), node.depth)).collect();
    assert_eq!(
        names,
        [
            ("Sort", 0),
            ("Table scan", 1),
            ("Aggregate using temporary table", 2),
            ("Nested loop inner join", 3),
            ("Table scan", 4),
            ("Index lookup", 4)
        ]
    );
    let lookup = &plan.nodes[5];
    assert_eq!(lookup.entity.as_deref(), Some("o using customer"));
    assert_eq!(lookup.condition.as_deref(), Some("(o.id > 10)"));
    assert_eq!(
        (lookup.cost, lookup.rows, lookup.actual_rows, lookup.loops),
        (Some(3.99), Some(100.0), Some(99.0), Some(10.0))
    );
    assert!((lookup.time_ms.unwrap() - 0.37).abs() < 1e-9);
    assert_eq!(plan.measure, Measure::Time);
    assert!(plan.nodes[2].hot, "the aggregate itself took 0.321 of 0.816 ms");
    assert_eq!(plan.nodes[0].details.iter().find(|detail| detail.key == "Sort").unwrap().value, "`sum(o.total)` DESC");
}

#[test]
fn sqlite_rows_read_into_a_tree_and_its_text() {
    let execution = Execution {
        columns: ["id", "parent", "notused", "detail"].map(column).to_vec(),
        rows: vec![
            vec![Value::Integer(3), Value::Integer(0), Value::Integer(0), Value::Text("SCAN o".into())],
            vec![
                Value::Integer(5),
                Value::Integer(0),
                Value::Integer(0),
                Value::Text("SEARCH c USING INTEGER PRIMARY KEY (rowid=?)".into()),
            ],
            vec![
                Value::Integer(9),
                Value::Integer(0),
                Value::Integer(0),
                Value::Text("USE TEMP B-TREE FOR ORDER BY".into()),
            ],
            vec![Value::Integer(12), Value::Integer(9), Value::Integer(0), Value::Text("SCAN x USING INDEX ix".into())],
        ],
        ..Execution::default()
    };
    let plan = parse(PlanFormat::SqliteRows, &execution, false).unwrap();
    let names: Vec<(&str, usize)> = plan.nodes.iter().map(|node| (node.node.as_str(), node.depth)).collect();
    assert_eq!(names, [("SCAN", 0), ("SEARCH", 0), ("USE TEMP B-TREE FOR ORDER BY", 0), ("SCAN", 1)]);
    assert_eq!(plan.nodes[1].entity.as_deref(), Some("c"));
    assert_eq!(plan.nodes[1].condition.as_deref(), Some("rowid=?"));
    assert_eq!(plan.nodes[3].entity.as_deref(), Some("x using ix"));
    assert_eq!(plan.measure, Measure::None);
    assert_eq!(
        plan.raw,
        "QUERY PLAN\n|--SCAN o\n|--SEARCH c USING INTEGER PRIMARY KEY (rowid=?)\n`--USE TEMP B-TREE FOR ORDER BY\n   `--SCAN x USING INDEX ix"
    );
}
