use super::*;
use crate::edit::Staged;

fn origin() -> Origin {
    Origin {
        query: 1,
        statement: 0,
        connection: "c".to_owned(),
        context: ContextKind::Console("console".to_owned()),
        text: "SELECT * FROM people".to_owned(),
        params: Vec::new(),
    }
}

fn result(paging: Paging, budget: Arc<Budget>) -> Arc<ResultSet> {
    Arc::new(ResultSet::new(7, origin(), paging, budget, None))
}

fn columns(names: &[&str]) -> Vec<Column> {
    names.iter().map(|name| Column::new((*name).to_owned(), "t".to_owned())).collect()
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(future)
}

#[test]
fn columns_stay_typed_until_a_value_of_another_kind_comes() {
    let set = result(Paging::default(), Arc::default());
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["n", "t", "mixed", "nulls", "doc"]));
    let doc = Value::Document(vec![("a".to_owned(), Value::Integer(1))]);
    reader.push(vec![
        vec![Value::Integer(1), Value::Text("a".into()), Value::Integer(5), Value::Null, doc.clone()],
        vec![Value::Null, Value::Text("бэ".into()), Value::Text("x".into()), Value::Null, Value::Null],
        vec![Value::Integer(3), Value::Null, Value::Float(1.5), Value::Bool(true), doc.clone()],
    ]);
    assert_eq!(set.len(), 3);
    assert_eq!(set.get(0, 0), Value::Integer(1));
    assert_eq!(set.get(1, 0), Value::Null);
    assert_eq!(set.get(1, 1), Value::Text("бэ".into()));
    assert_eq!(set.get(2, 1), Value::Null);
    assert_eq!(set.get(0, 2), Value::Integer(5));
    assert_eq!(set.get(1, 2), Value::Text("x".into()));
    assert_eq!(set.get(2, 2), Value::Float(1.5));
    assert_eq!(set.get(1, 3), Value::Null);
    assert_eq!(set.get(2, 3), Value::Bool(true));
    assert_eq!(set.get(2, 4), doc);
    assert_eq!(set.page(1, 5).len(), 2);
    assert!(set.info().bytes > 0);
}

#[test]
fn typed_text_keeps_its_kind_and_temporals_their_type() {
    let set = result(Paging::default(), Arc::default());
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["when", "price"]));
    let at = |text: &str, kind: &str| Value::Temporal(Typed { text: text.into(), type_name: kind.into() });
    reader.push(vec![
        vec![at("2024-01-01 10:00:00+08", "timestamptz"), Value::Decimal("1.50".into())],
        vec![at("2024-01-02", "date"), Value::Decimal("2".into())],
    ]);
    assert_eq!(set.get(0, 0), at("2024-01-01 10:00:00+08", "timestamptz"));
    assert_eq!(set.get(1, 0), at("2024-01-02", "date"));
    let cells = set.cells(0..2, 0..2, 100);
    assert!(cells[0][1].number);
    assert_eq!(cells[1][1].text, "2");
}

#[test]
fn cells_are_cut_null_is_marked_and_booleans_are_known() {
    let set = result(Paging::default(), Arc::default());
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["long", "flag", "maybe", "multi"]));
    reader.push(vec![vec![
        Value::Text("x".repeat(500)),
        Value::Bool(true),
        Value::Null,
        Value::Text("first\nsecond".into()),
    ]]);
    let cells = set.cells(0..1, 0..4, 100);
    assert_eq!(cells[0][0].text.chars().count(), 100);
    assert!(cells[0][0].cut);
    assert_eq!(cells[0][1].boolean, Some(true));
    assert!(cells[0][2].null);
    assert_eq!(cells[0][3].text, "first");
    assert!(cells[0][3].cut);
}

#[test]
fn staged_edits_show_over_the_values_read() {
    let set = result(Paging::default(), Arc::default());
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["a"]));
    reader.push(vec![vec![Value::Integer(1)], vec![Value::Integer(2)]]);
    assert!(set.stage(1, 0, Staged::Value(Value::Text("two".into()))));
    assert_eq!(set.get(1, 0), Value::Text("two".into()));
    assert_eq!(set.original(1, 0), Value::Integer(2));
    assert_eq!(set.page(0, 2)[1][0], Value::Text("two".into()));
    assert!(set.cells(1..2, 0..1, 10)[0][0].changed);
    assert_eq!(set.pending().cells, 1);
    assert!(set.stage(1, 0, Staged::Value(Value::Integer(2))), "the value read again drops the change");
    assert!(!set.pending().any());
    set.stage(0, 0, Staged::Default);
    assert!(set.cells(0..1, 0..1, 10)[0][0].default);
    set.drop_edits();
    assert_eq!(set.get(1, 0), Value::Integer(2));
}

#[test]
fn new_and_deleted_rows_are_staged_and_saved_rows_replace_them() {
    let set = result(Paging::default(), Arc::default());
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["id", "name"]));
    reader.push((1..=3).map(|id| vec![Value::Integer(id), Value::Text(format!("n{id}"))]).collect());
    reader.finish(true, false);
    let added = set.add_row(vec![(1, Staged::Value(Value::Text("new".into())))]);
    assert_eq!((added, set.len(), set.stored()), (3, 4, 3));
    assert_eq!(set.marks(0..4), [RowMark::Clean, RowMark::Clean, RowMark::Clean, RowMark::Inserted]);
    assert!(set.cells(3..4, 0..1, 10)[0][0].default, "a new row's cell without a value takes its default");
    set.stage(1, 1, Staged::Value(Value::Text("changed".into())));
    assert_eq!(set.delete_rows(&[0]), 1);
    assert_eq!(set.marks(0..2), [RowMark::Deleted, RowMark::Changed]);
    let changes = set.changes();
    assert_eq!(changes.deleted[0].original, [Value::Integer(1), Value::Text("n1".into())]);
    assert_eq!(changes.updated[0].row, 1);
    assert_eq!(changes.inserted[0].row, 3);
    let saved = SavedRows {
        updated: vec![(1, vec![(1, Value::Text("CHANGED".into()))])],
        inserted: vec![vec![(0, Value::Integer(10)), (1, Value::Text("new".into()))]],
    };
    set.apply_saved(&saved);
    assert!(!set.pending().any());
    assert_eq!(
        set.page(0, 10),
        [
            vec![Value::Integer(2), Value::Text("CHANGED".into())],
            vec![Value::Integer(3), Value::Text("n3".into())],
            vec![Value::Integer(10), Value::Text("new".into())],
        ]
    );
    assert_eq!(set.delete_rows(&[3]), 0);
}

#[test]
fn loaded_rows_sort_on_the_client_and_go_back_to_their_order() {
    let set = result(Paging::default(), Arc::default());
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["n", "s"]));
    reader.push(vec![
        vec![Value::Integer(2), Value::Text("b".into())],
        vec![Value::Null, Value::Text("c".into())],
        vec![Value::Integer(10), Value::Text("a".into())],
    ]);
    reader.finish(true, false);
    set.stage(0, 1, Staged::Value(Value::Text("B".into())));
    assert!(set.sort_rows(&[(0, true)]));
    let firsts: Vec<Value> = set.page(0, 3).into_iter().map(|row| row[0].clone()).collect();
    assert_eq!(firsts, [Value::Null, Value::Integer(10), Value::Integer(2)], "as the server sorts: NULL is largest");
    assert_eq!(set.get(2, 1), Value::Text("B".into()), "staged changes move with their rows");
    assert!(set.sort_rows(&[]));
    assert_eq!(set.get(0, 1), Value::Text("B".into()));
    assert_eq!(set.get(2, 0), Value::Integer(10));
    let (found, more) = set.find("A", false, 10, 1000);
    assert_eq!((found, more), (vec![(2, 1)], false));
}

#[test]
fn outside_a_transaction_reading_goes_to_the_cap_and_stops() {
    let paging = Paging { page: 200, row_cap: 1000, keep_open: false };
    let set = result(paging, Arc::default());
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["n"]));
    reader.portal(false);
    block_on(async {
        assert_eq!(reader.want().await, 200);
        reader.push((0..200).map(|n| vec![Value::Integer(n)]).collect());
        assert_eq!(reader.want().await, 800);
        reader.push((0..800).map(|n| vec![Value::Integer(n)]).collect());
        assert_eq!(reader.want().await, 0);
    });
    reader.finish(false, false);
    let info = set.info();
    assert_eq!(info.stopped_because, Some(StopReason::Cap));
    assert!(info.more);
    assert!(!set.portal_open());
    assert!(set.want_more(Some(200), true).unwrap(), "a closed portal is read again");
    let again = set.reader(true, tokio_util::sync::CancellationToken::new());
    assert_eq!(again.skip(), 1000);
}

#[test]
fn inside_the_persons_transaction_the_portal_waits_for_more() {
    let set = result(Paging { page: 50, row_cap: 1000, keep_open: false }, Arc::default());
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["n"]));
    reader.portal(true);
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        assert_eq!(reader.want().await, 50);
        reader.push((0..50).map(|n| vec![Value::Integer(n)]).collect());
        let waiting = tokio::time::timeout(Duration::from_millis(50), reader.want()).await;
        assert!(waiting.is_err(), "nobody wants more yet");
        assert_eq!(set.info().state, ReadState::Open);
        assert!(!set.want_more(Some(50), false).unwrap(), "the open portal reads on");
        assert_eq!(reader.want().await, 50);
        set.close();
        assert_eq!(reader.want().await, 0);
    });
}

#[test]
fn the_budget_stops_reading_and_comes_back_when_a_result_goes() {
    let budget = Arc::new(Budget::new(10_000));
    let store = ResultStore { results: Mutex::new(Vec::new()), budget: Arc::clone(&budget), paging: Mutex::default() };
    let set = result(Paging { page: 100, row_cap: 1_000_000, keep_open: false }, Arc::clone(&budget));
    store.insert(Arc::clone(&set));
    let mut reader = set.reader(true, tokio_util::sync::CancellationToken::new());
    reader.columns(columns(&["t"]));
    block_on(async {
        let mut kept = true;
        while kept {
            let wanted = reader.want().await;
            if wanted == 0 {
                break;
            }
            kept = reader.push((0..wanted).map(|_| vec![Value::Text("y".repeat(100))]).collect());
        }
        assert_eq!(reader.want().await, 0);
    });
    reader.finish(false, false);
    assert_eq!(set.info().stopped_because, Some(StopReason::Budget));
    assert!(budget.used() >= 10_000);
    assert!(store.remove(7));
    assert_eq!(budget.used(), 0);
}

#[test]
fn tables_name_their_results() {
    assert_eq!(table_of("SELECT * FROM people WHERE a = 1").as_deref(), Some("people"));
    assert_eq!(table_of("select * from public.people;").as_deref(), Some("public.people"));
    assert_eq!(table_of("SELECT * FROM a JOIN b ON a.id = b.id").as_deref(), Some("a(+)"));
    assert_eq!(table_of("SELECT 1"), None);
    assert_eq!(table_of("SELECT * FROM (SELECT 1) q"), None);
    assert_eq!(table_of("SELECT 'from x' AS a, b FROM orders o").as_deref(), Some("orders"));
    assert_eq!(table_of("-- from notes\nSELECT * /* from x */ FROM tickets;").as_deref(), Some("tickets"));
    assert_eq!(table_of("SELECT 'it''s from y' FROM t").as_deref(), Some("t"));
}

fn mixed_value(seed: &mut u64) -> Value {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    let pick = *seed;
    let small = (pick >> 8) as i64 % 21 - 10;
    match pick % 12 {
        0 => Value::Null,
        1 => Value::Integer(small),
        2 => Value::Integer((1_i64 << 53) + small),
        3 => Value::Float(small as f64 / 2.0),
        4 => Value::Float(f64::NAN),
        5 => Value::Float((1_i64 << 53) as f64),
        6 => Value::Decimal(format!("{small}.5")),
        7 => Value::Decimal("abc".to_owned()),
        8 => Value::Text(small.to_string()),
        9 => Value::Text(format!("t{small}")),
        10 => Value::Bool(small > 0),
        _ => Value::Bytes(vec![small as u8]),
    }
}

#[test]
fn sorting_a_column_of_mixed_types_never_panics_and_keeps_a_total_order() {
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..200 {
        let mut values: Vec<Value> = (0..64).map(|_| mixed_value(&mut seed)).collect();
        values.sort_by(compare_values);
        for pair in values.windows(2) {
            assert_ne!(compare_values(&pair[0], &pair[1]), std::cmp::Ordering::Greater, "{pair:?}");
        }
    }
    let values: Vec<Value> = (0..300).map(|_| mixed_value(&mut seed)).collect();
    for a in &values {
        for b in &values {
            assert_eq!(compare_values(a, b), compare_values(b, a).reverse(), "{a:?} {b:?}");
        }
    }
    for a in values.iter().take(60) {
        for b in values.iter().take(60) {
            for c in values.iter().take(60) {
                if compare_values(a, b).is_le() && compare_values(b, c).is_le() {
                    assert!(compare_values(a, c).is_le(), "{a:?} <= {b:?} <= {c:?}");
                }
            }
        }
    }
}
