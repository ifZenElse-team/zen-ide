use super::*;

const FOUR: &str = "    ";

fn fmt(text: &str, dialect: Dialect, options: &FormatOptions) -> String {
    format_text(text, Flavor::of_dialect(dialect), options, FOUR)
}

fn upper() -> FormatOptions {
    FormatOptions { keyword_case: KeywordCase::Upper, ..FormatOptions::default() }
}

#[test]
fn a_select_gets_a_line_per_clause_and_per_item() {
    let text = "select a, b, count(*) as n from t left join u on u.id = t.id and u.on where x = 1 and y in (1, 2) group by a, b order by n desc limit 10";
    assert_eq!(
        fmt(text, Dialect::Postgres, &upper()),
        "SELECT a,\n    b,\n    count(*) AS n\nFROM t\nLEFT JOIN u ON u.id = t.id\n    AND u.on\nWHERE x = 1\n    AND y IN (1, 2)\nGROUP BY a,\n    b\nORDER BY n DESC\nLIMIT 10"
    );
}

#[test]
fn keyword_case_follows_the_option_and_leaves_names_alone() {
    let text = "SELECT t.select, \"From\", count(x), left(name, 2) FROM t";
    let lower = FormatOptions { keyword_case: KeywordCase::Lower, ..FormatOptions::default() };
    assert_eq!(
        fmt(text, Dialect::Postgres, &lower),
        "select t.select,\n    \"From\",\n    count(x),\n    left(name, 2)\nfrom t"
    );
    let as_is = FormatOptions::default();
    assert_eq!(fmt("Select a From t", Dialect::Postgres, &as_is), "Select a\nFrom t");
}

#[test]
fn commas_lead_when_asked_and_clauses_can_stand_alone() {
    let options = FormatOptions { commas: Commas::Leading, clause_on_own_line: true, ..upper() };
    assert_eq!(
        fmt("select a, b from t where x = 1 and y = 2", Dialect::Standard, &options),
        "SELECT\n    a\n    , b\nFROM\n    t\nWHERE\n    x = 1\n    AND y = 2"
    );
    let inline = FormatOptions { item_per_line: false, ..upper() };
    assert_eq!(fmt("select a,b from t", Dialect::Standard, &inline), "SELECT a, b\nFROM t");
}

#[test]
fn subqueries_and_ctes_are_indented() {
    let text = "with x as (select a from t), y as (select b from u) select * from x where a in (select b from y) and exists (select 1)";
    assert_eq!(
        fmt(text, Dialect::Postgres, &upper()),
        "WITH x AS (\n    SELECT a\n    FROM t\n),\ny AS (\n    SELECT b\n    FROM u\n)\nSELECT *\nFROM x\nWHERE a IN (\n    SELECT b\n    FROM y\n)\n    AND EXISTS (\n        SELECT 1\n    )"
    );
}

#[test]
fn inserts_updates_deletes_and_tables() {
    assert_eq!(
        fmt("insert into t (a, b) values (1, 'x'), (2, 'y')", Dialect::Postgres, &upper()),
        "INSERT INTO t (a, b)\nVALUES (1, 'x'),\n    (2, 'y')"
    );
    assert_eq!(
        fmt("update t set a = 1, b = 2 where id = 3 returning *", Dialect::Postgres, &upper()),
        "UPDATE t\nSET a = 1,\n    b = 2\nWHERE id = 3\nRETURNING *"
    );
    assert_eq!(fmt("delete from t where id = 3", Dialect::Postgres, &upper()), "DELETE FROM t\nWHERE id = 3");
    assert_eq!(
        fmt(
            "create table t (id int primary key, price numeric(10, 2) not null, check (price > 0))",
            Dialect::Postgres,
            &upper()
        ),
        "CREATE TABLE t (\n    id int PRIMARY KEY,\n    price numeric(10, 2) NOT NULL,\n    CHECK (price > 0)\n)"
    );
}

#[test]
fn comments_stay_where_they_are() {
    let text = "-- head\nselect a, -- first\n  /* own */\n  b\nfrom t -- tail";
    assert_eq!(
        fmt(text, Dialect::Standard, &upper()),
        "-- head\nSELECT a, -- first\n    /* own */\n    b\nFROM t -- tail"
    );
    let comment_in_call = "select f(a -- x\n, b) from t";
    let done = fmt(comment_in_call, Dialect::Standard, &upper());
    assert!(done.contains("-- x\n"), "{done}");
}

#[test]
fn between_case_and_is_distinct_from_do_not_break() {
    assert_eq!(
        fmt(
            "select case when a between 1 and 2 then 'x' else 'y' end from t where a between 1 and 2 and b is distinct from c",
            Dialect::Postgres,
            &upper()
        ),
        "SELECT CASE WHEN a BETWEEN 1 AND 2 THEN 'x' ELSE 'y' END\nFROM t\nWHERE a BETWEEN 1 AND 2\n    AND b IS DISTINCT FROM c"
    );
}

#[test]
fn dialect_quotes_stay_byte_for_byte() {
    let postgres = "select $body$ select  a ,b  from t $body$, E'a\\'  b', \"Mixed  Case\", x::text, $1 from t";
    let done = fmt(postgres, Dialect::Postgres, &upper());
    assert!(done.contains("$body$ select  a ,b  from t $body$"), "{done}");
    assert!(
        done.contains("E'a\\'  b'")
            && done.contains("\"Mixed  Case\"")
            && done.contains("x::text")
            && done.contains("$1")
    );
    let array = fmt("select array[1, 2][1] from t", Dialect::Postgres, &upper());
    assert_eq!(array, "SELECT array[1, 2][1]\nFROM t");
    let upsert = "insert into t (a) values (1) on duplicate key update a = values(a), b = 2";
    let done = fmt(upsert, Dialect::MySql, &upper());
    assert_eq!(done, "INSERT INTO t (a)\nVALUES (1)\nON duplicate key UPDATE a = VALUES(a), b = 2");
    let rollup = fmt("select a from t group by a with rollup", Dialect::MySql, &upper());
    assert_eq!(rollup, "SELECT a\nFROM t\nGROUP BY a WITH rollup");
    let mysql = "select `my  col`, 'it\\'s  here', \"a  string\" from t # tail comment";
    let done = fmt(mysql, Dialect::MySql, &upper());
    assert_eq!(done, "SELECT `my  col`,\n    'it\\'s  here',\n    \"a  string\"\nFROM t # tail comment");
}

#[test]
fn unsafe_statements_are_kept_and_others_only_recased() {
    let flavor = Flavor::of_dialect(Dialect::Postgres);
    let unclosed = "select 'open from t";
    let done = format(unclosed, flavor, &upper(), FOUR, None, &|| false).unwrap();
    assert_eq!((done.edits.len(), done.kept), (0, 1));
    assert_eq!(fmt("select (a from t", Dialect::Postgres, &upper()), "select (a from t");
    assert_eq!(fmt("grant select on t to bob", Dialect::Postgres, &upper()), "GRANT SELECT ON t TO bob");
    let function = "create function f() returns int as $$ begin  return 1; end $$ language plpgsql";
    assert_eq!(
        fmt(function, Dialect::Postgres, &upper()),
        "CREATE function f() returns int AS $$ begin  return 1; end $$ language plpgsql"
    );
    assert_eq!(fmt("\\d  t\nselect 1", Dialect::Postgres, &upper()), "\\d  t\nSELECT 1");
}

#[test]
fn only_the_statements_a_selection_touches_change() {
    let text = "select a from t;\nselect b from u;\nselect c from v;";
    let flavor = Flavor::of_dialect(Dialect::Standard);
    let second = text.find("select b").unwrap();
    let done = format(text, flavor, &upper(), FOUR, Some(second + 2..second + 4), &|| false).unwrap();
    assert_eq!(done.apply(text), "select a from t;\nSELECT b\nFROM u;\nselect c from v;");
    assert!(format(text, flavor, &upper(), FOUR, None, &|| true).is_none(), "cancelled");
}

#[test]
fn nested_lines_keep_the_statement_indentation() {
    assert_eq!(fmt("  select a, b from t", Dialect::Standard, &upper()), "  SELECT a,\n      b\n  FROM t");
    assert_eq!(
        format_text("select a, b from t", Flavor::of_dialect(Dialect::Standard), &upper(), "\t"),
        "SELECT a,\n\tb\nFROM t"
    );
}

const CORPUS: &[(Dialect, &str)] = &[
    (
        Dialect::Postgres,
        "select a, b, count(*) as n from t left join u on u.id = t.id where x = 1 and y in (1, 2) group by a, b order by n desc limit 10 offset 5;",
    ),
    (
        Dialect::Postgres,
        "WITH RECURSIVE r(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM r WHERE n < 10) SELECT * FROM r;",
    ),
    (
        Dialect::Postgres,
        "insert into t (a, b) values (1, 'x'), (2, 'y') on conflict (a) do update set b = excluded.b returning *;",
    ),
    (Dialect::Postgres, "select distinct on (a) a, b from t order by a, b desc nulls last for update skip locked;"),
    (
        Dialect::Postgres,
        "select $$ a ; b $$, $tag$ x $tag$, E'\\n', U&\"d\\0061t\", array[1, 2][1], j->>'k', j #> '{a}' from t;",
    ),
    (Dialect::Postgres, "create function f() returns int language sql as $$ select 1 $$;"),
    (Dialect::Postgres, "select a /* inline */ , b -- trailing\n -- own line\n from t where a = 1 -- end"),
    (
        Dialect::Postgres,
        "update t set a = (select max(b) from u where u.id = t.id), c = case when d then 1 else 2 end where e is not null;",
    ),
    (
        Dialect::Postgres,
        "select * from a cross join b natural join c full outer join d using (id) where x between 1 and 2 or y like 'z%';",
    ),
    (
        Dialect::Postgres,
        "select count(*) filter (where a > 0) over (partition by b order by c rows between unbounded preceding and current row) from t;",
    ),
    (Dialect::Postgres, "\\set x 1\nselect :x, :'x', :\"x\";\nbegin; commit;"),
    (
        Dialect::Postgres,
        "create table if not exists t (id bigserial primary key, name text not null default 'a, b', ref int references u (id) on delete set null, unique (name, ref));",
    ),
    (Dialect::Postgres, "select a from t union select b from u intersect select c from v except select d from w;"),
    (
        Dialect::Postgres,
        "select (select 1), coalesce((select 2), 3), exists(select 1) from t where (a, b) in ((1, 2), (3, 4));",
    ),
    (Dialect::MySql, "select `a`, b from `t` where c = 'it\\'s' and d = \"q\" # hash comment\n limit 5;"),
    (Dialect::MySql, "insert into t (a) values (1) on duplicate key update a = values(a), b = 2;"),
    (Dialect::MySql, "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; END //\nDELIMITER ;\nselect 1;"),
    (Dialect::MySql, "select count( * ) , sum(x)from t group by a with rollup;"),
    (
        Dialect::MySql,
        "create table t (id int(11) not null auto_increment, primary key (`id`)) engine=InnoDB default charset=utf8mb4;",
    ),
    (Dialect::MySql, "select a--b, c -- d\n from t;"),
    (Dialect::Standard, "SELECT [a b], \"c d\", `e f` FROM t WHERE x = 'y''z';"),
    (Dialect::Standard, "select\n\n  a\n\n,\n\n  b\n\nfrom\n\n  t\n\n;"),
    (Dialect::Standard, "select a from t where b = 1 and (c = 2 or d = 3) and not e;"),
    (Dialect::Standard, "create view v as select a, b from t;"),
    (Dialect::Standard, "explain select 1; vacuum; pragma table_info(t);"),
    (Dialect::Standard, "select a from t;; ;select b from u"),
    (Dialect::Standard, "select 'unterminated from t;"),
    (Dialect::Standard, "select 1.5e-3, -1, - -2, a-b, a - -b, x<=y, x<>y, x!=y, x||y from t;"),
    (Dialect::SqlServer, "select [order], top from t\nGO\nselect 2"),
    (Dialect::Postgres, "select 'é', \"ünïcode\", ñame from tâble where ä = 'ö';"),
];

fn options_matrix() -> Vec<FormatOptions> {
    let mut all = Vec::new();
    for keyword_case in [KeywordCase::AsIs, KeywordCase::Upper, KeywordCase::Lower] {
        for commas in [Commas::Trailing, Commas::Leading] {
            for clause_on_own_line in [false, true] {
                for item_per_line in [false, true] {
                    all.push(FormatOptions { keyword_case, commas, clause_on_own_line, item_per_line });
                }
            }
        }
    }
    all
}

fn words_and_tokens(text: &str, lexing: Lexing) -> Vec<(Kind, String)> {
    scan(text, lexing)
        .tokens
        .iter()
        .map(|token| {
            let raw = &text[token.start..token.end];
            (token.kind, if token.kind == Kind::Word { raw.to_ascii_uppercase() } else { raw.to_owned() })
        })
        .collect()
}

#[test]
fn the_corpus_round_trips_with_every_option() {
    for (dialect, text) in CORPUS {
        let flavor = Flavor::of_dialect(*dialect);
        for options in options_matrix() {
            for indent in [FOUR, "\t", "  "] {
                let once = format_text(text, flavor, &options, indent);
                let twice = format_text(&once, flavor, &options, indent);
                assert_eq!(once, twice, "idempotent: {dialect:?} {options:?}\n{text}\n---\n{once}");
                assert_eq!(
                    words_and_tokens(text, flavor.lexing),
                    words_and_tokens(&once, flavor.lexing),
                    "tokens kept: {dialect:?} {options:?}\n{once}"
                );
                if options.keyword_case == KeywordCase::AsIs {
                    let raw = |text: &str| -> Vec<String> {
                        scan(text, flavor.lexing)
                            .tokens
                            .iter()
                            .map(|token| text[token.start..token.end].to_owned())
                            .collect()
                    };
                    assert_eq!(raw(text), raw(&once), "as is keeps every token's case: {once}");
                }
                assert_eq!(split(text, flavor.dialect).len(), split(&once, flavor.dialect).len(), "{once}");
                let compact =
                    |text: &str| text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
                assert_eq!(compact(text), compact(&once), "only white space and case change: {once}");
            }
        }
    }
}

#[test]
fn an_unknown_dialect_keeps_what_only_some_dialects_read() {
    for kept in [
        "select $$a from b$$, 1",
        "insert into t values ($$select me, from here$$)",
        "select a, b # from old\nfrom t",
        "select 'a\\' from t",
        "select `from` from t",
    ] {
        assert_eq!(fmt(kept, Dialect::Standard, &upper()), kept, "{kept}");
    }
    assert_eq!(fmt("select a from t", Dialect::Standard, &upper()), "SELECT a\nFROM t", "plain SQL is formatted");
    assert!(fmt("select $$a from b$$, 1", Dialect::Postgres, &upper()).contains("$$a from b$$"));
}

#[test]
fn many_statements_on_one_line_take_linear_time() {
    let script = "select 1 from t; ".repeat(20_000);
    let started = std::time::Instant::now();
    let done = fmt(&script, Dialect::Postgres, &upper());
    assert!(done.contains("SELECT 1"));
    if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    }
}
