use super::*;

fn effect(statement: &str, dialect: Dialect) -> Effect {
    assess(statement, dialect).effect
}

fn risks(statement: &str, dialect: Dialect) -> Vec<(Risk, Option<String>)> {
    assess(statement, dialect).dangers.into_iter().map(|danger| (danger.risk, danger.object)).collect()
}

fn risk(risk: Risk, object: &str) -> Vec<(Risk, Option<String>)> {
    vec![(risk, Some(object.to_owned()))]
}

#[test]
fn reads_are_reads_in_every_dialect() {
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::SqlServer, Dialect::Standard] {
        for statement in [
            "SELECT * FROM orders",
            "  select 1",
            "(SELECT 1) UNION (SELECT 2)",
            "VALUES (1), (2)",
            "SHOW search_path",
            "EXPLAIN SELECT * FROM orders",
            "WITH recent AS (SELECT * FROM orders) SELECT * FROM recent",
            "-- a comment\nSELECT 1",
            "/* DELETE FROM orders */ SELECT 1",
            "SELECT 'DELETE FROM orders' AS text",
            "SELECT * FROM orders FOR UPDATE",
        ] {
            let assessed = assess(statement, dialect);
            assert_eq!(assessed, Assessment::of(Effect::Read), "{statement} in {dialect:?}");
            assert!(assessed.allowed_read_only() && !assessed.changes());
        }
    }
}

#[test]
fn changes_are_found_wherever_they_hide() {
    let dialect = Dialect::Postgres;
    for statement in [
        "INSERT INTO orders VALUES (1)",
        "UPDATE orders SET paid = true WHERE id = 1",
        "WITH gone AS (DELETE FROM orders WHERE id = 1 RETURNING *) SELECT * FROM gone",
        "WITH ids AS (SELECT id FROM orders) DELETE FROM orders WHERE id IN (SELECT id FROM ids)",
        "EXPLAIN ANALYZE UPDATE orders SET paid = true WHERE id = 1",
        "SELECT * INTO archive FROM orders",
        "SELECT nextval('orders_id_seq')",
        "SELECT pg_terminate_backend(42)",
        "CALL refresh_totals()",
        "DO $$ BEGIN DELETE FROM orders; END $$",
        "COPY orders FROM '/tmp/orders.csv'",
        "MERGE INTO orders USING incoming ON orders.id = incoming.id WHEN MATCHED THEN DELETE",
    ] {
        assert_eq!(effect(statement, dialect), Effect::Data, "{statement}");
    }
    for statement in ["CREATE TABLE t (id int)", "ALTER TABLE t ADD COLUMN c int", "GRANT SELECT ON t TO bob", "VACUUM"]
    {
        assert_eq!(effect(statement, dialect), Effect::Structure, "{statement}");
    }
    assert_eq!(effect("FROBNICATE everything", dialect), Effect::Other);
    assert!(!assess("FROBNICATE everything", dialect).allowed_read_only());
}

#[test]
fn transactions_and_session_settings_are_neither_reads_nor_changes_unless_they_loosen_read_only() {
    let dialect = Dialect::Postgres;
    for statement in ["BEGIN", "COMMIT", "ROLLBACK TO SAVEPOINT a", "START TRANSACTION ISOLATION LEVEL SERIALIZABLE"] {
        assert_eq!(effect(statement, dialect), Effect::Transaction, "{statement}");
    }
    for statement in ["SET search_path TO shop", "RESET ALL", "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"] {
        assert_eq!(effect(statement, dialect), Effect::Session, "{statement}");
    }
    for statement in [
        "SET default_transaction_read_only = off",
        "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE",
        "BEGIN READ WRITE",
        "START TRANSACTION READ WRITE",
        "SET ROLE admin",
    ] {
        let assessed = assess(statement, dialect);
        assert_eq!(assessed.effect, Effect::Other, "{statement}");
        assert!(!assessed.allowed_read_only(), "{statement}");
    }
    assert_eq!(effect("SET readonly = 0", Dialect::Standard), Effect::Other);
    assert_eq!(effect("PRAGMA query_only = 0", Dialect::Standard), Effect::Other);
    assert_eq!(effect("PRAGMA journal_mode = WAL", Dialect::Standard), Effect::Other);
    assert_eq!(effect("PRAGMA table_info(orders)", Dialect::Standard), Effect::Session);
    assert_eq!(effect("SET SESSION TRANSACTION READ WRITE", Dialect::MySql), Effect::Other);
    assert_eq!(effect("USE shop", Dialect::MySql), Effect::Session);
}

#[test]
fn delete_and_update_without_a_where_are_risks() {
    let dialect = Dialect::Postgres;
    assert_eq!(risks("DELETE FROM orders", dialect), risk(Risk::DeleteWithoutWhere, "orders"));
    assert_eq!(risks("delete from public.orders;", dialect), risk(Risk::DeleteWithoutWhere, "public.orders"));
    assert_eq!(risks("DELETE FROM ONLY \"Orders\"", dialect), risk(Risk::DeleteWithoutWhere, "\"Orders\""));
    assert_eq!(risks("UPDATE orders SET paid = true", dialect), risk(Risk::UpdateWithoutWhere, "orders"));
    assert_eq!(
        risks("UPDATE ONLY orders SET paid = (SELECT true WHERE 1 = 2)", dialect),
        risk(Risk::UpdateWithoutWhere, "orders")
    );
    assert_eq!(risks("DELETE FROM orders USING customers", dialect), risk(Risk::DeleteWithoutWhere, "orders"));
    assert!(risks("DELETE FROM orders WHERE id = 1", dialect).is_empty());
    assert!(risks("UPDATE orders SET paid = true WHERE id IN (SELECT id FROM late)", dialect).is_empty());
    assert!(risks("DELETE FROM orders WHERE CURRENT OF c", dialect).is_empty());
    assert!(risks("DELETE FROM orders WHERE id = 1 RETURNING *", dialect).is_empty());
}

#[test]
fn a_where_that_is_always_true_is_no_where() {
    let dialect = Dialect::Postgres;
    for statement in [
        "DELETE FROM orders WHERE true",
        "DELETE FROM orders WHERE TRUE RETURNING id",
        "DELETE FROM orders WHERE 1 = 1",
        "DELETE FROM orders WHERE (1=1)",
        "DELETE FROM orders WHERE id = 5 OR 1 = 1",
        "DELETE FROM orders WHERE 'a' = 'a'",
        "UPDATE orders SET paid = true WHERE id = id",
        "UPDATE orders SET paid = true WHERE 1 <> 0",
        "DELETE FROM orders WHERE NOT false",
    ] {
        assert_eq!(risks(statement, dialect).len(), 1, "{statement}");
    }
    assert!(risks("DELETE FROM orders WHERE id = 1 AND true", dialect).is_empty());
    assert!(risks("DELETE FROM orders WHERE paid = true", dialect).is_empty());
}

#[test]
fn ctes_comments_and_strings_do_not_fool_the_risks() {
    let dialect = Dialect::Postgres;
    assert_eq!(
        risks("WITH gone AS (DELETE FROM orders RETURNING *) SELECT count(*) FROM gone", dialect),
        risk(Risk::DeleteWithoutWhere, "orders")
    );
    assert_eq!(
        risks("WITH gone AS (DELETE FROM orders WHERE id = 1 RETURNING *) UPDATE totals SET n = n - 1", dialect),
        risk(Risk::UpdateWithoutWhere, "totals")
    );
    assert_eq!(risks("DELETE FROM orders -- WHERE id = 1", dialect), risk(Risk::DeleteWithoutWhere, "orders"));
    assert_eq!(risks("DELETE FROM orders /* WHERE id = 1 */", dialect), risk(Risk::DeleteWithoutWhere, "orders"));
    assert!(risks("SELECT 'DELETE FROM orders'", dialect).is_empty());
    assert!(risks("-- DELETE FROM orders\nSELECT 1", dialect).is_empty());
    assert!(risks("CREATE FUNCTION f() RETURNS void AS $$ DELETE FROM orders $$ LANGUAGE sql", dialect).is_empty());
    assert!(risks("SELECT * FROM orders FOR UPDATE", dialect).is_empty());
    assert!(risks("EXPLAIN DELETE FROM orders", dialect).is_empty());
    assert_eq!(risks("EXPLAIN ANALYZE DELETE FROM orders", dialect), risk(Risk::DeleteWithoutWhere, "orders"));
    assert!(risks("INSERT INTO orders VALUES (1) ON CONFLICT (id) DO UPDATE SET paid = true", dialect).is_empty());
    assert!(
        risks("CREATE TRIGGER t BEFORE UPDATE OR DELETE ON orders FOR EACH ROW EXECUTE FUNCTION f()", dialect)
            .is_empty()
    );
    assert!(risks("GRANT SELECT, UPDATE, DELETE ON orders TO bob", dialect).is_empty());
    assert!(
        risks("ALTER TABLE lines ADD CONSTRAINT fk FOREIGN KEY (o) REFERENCES orders (id) ON DELETE CASCADE", dialect)
            .is_empty()
    );
}

#[test]
fn drop_and_truncate_are_risks() {
    let dialect = Dialect::Postgres;
    assert_eq!(risks("DROP TABLE orders", dialect), risk(Risk::Drop, "TABLE orders"));
    assert_eq!(risks("drop table if exists shop.orders cascade", dialect), risk(Risk::Drop, "TABLE shop.orders"));
    assert_eq!(risks("DROP MATERIALIZED VIEW totals", dialect), risk(Risk::Drop, "MATERIALIZED VIEW totals"));
    assert_eq!(risks("DROP DATABASE shop", dialect), risk(Risk::Drop, "DATABASE shop"));
    assert_eq!(risks("DROP INDEX CONCURRENTLY orders_paid", dialect), risk(Risk::Drop, "INDEX orders_paid"));
    assert_eq!(risks("ALTER TABLE orders DROP COLUMN note", dialect), risk(Risk::Drop, "COLUMN note"));
    assert!(risks("ALTER TABLE orders ALTER COLUMN note DROP DEFAULT", dialect).is_empty());
    assert!(risks("ALTER TABLE orders ALTER COLUMN note DROP NOT NULL", dialect).is_empty());
    assert_eq!(risks("TRUNCATE orders", dialect), risk(Risk::Truncate, "orders"));
    assert_eq!(risks("TRUNCATE TABLE ONLY shop.orders RESTART IDENTITY", dialect), risk(Risk::Truncate, "shop.orders"));
}

#[test]
fn mysql_has_its_own_comments_quotes_and_multi_table_statements() {
    let dialect = Dialect::MySql;
    assert_eq!(risks("DELETE FROM `orders` # WHERE id = 1", dialect), risk(Risk::DeleteWithoutWhere, "`orders`"));
    assert!(risks("DELETE FROM orders WHERE note = \"no where\"", dialect).is_empty());
    assert_eq!(risks("UPDATE LOW_PRIORITY orders SET paid = 1", dialect), risk(Risk::UpdateWithoutWhere, "orders"));
    assert_eq!(risks("DELETE FROM orders LIMIT 10", dialect), risk(Risk::DeleteWithoutWhere, "orders"));
    assert!(risks("DELETE o FROM orders o JOIN late l ON o.id = l.id", dialect).is_empty());
    assert!(risks("INSERT INTO orders VALUES (1) ON DUPLICATE KEY UPDATE paid = 1", dialect).is_empty());
    assert_eq!(effect("REPLACE INTO orders VALUES (1)", dialect), Effect::Data);
    assert_eq!(effect("SELECT * FROM orders INTO OUTFILE '/tmp/o'", dialect), Effect::Data);
    assert_eq!(effect("/*!40101 SET NAMES utf8 */", dialect), Effect::Other);
    assert_eq!(effect("SELECT 1 # DELETE FROM orders", dialect), Effect::Read);
}

#[test]
fn sql_server_and_clickhouse_statements() {
    assert_eq!(
        risks("DELETE FROM [dbo].[orders]", Dialect::SqlServer),
        risk(Risk::DeleteWithoutWhere, "[dbo].[orders]")
    );
    assert_eq!(risks("TRUNCATE TABLE [orders]", Dialect::SqlServer), risk(Risk::Truncate, "[orders]"));
    assert!(risks("ALTER TABLE orders DELETE WHERE id = 1", Dialect::Standard).is_empty());
    assert_eq!(risks("ALTER TABLE orders DELETE WHERE 1", Dialect::Standard).len(), 1);
    assert_eq!(risks("ALTER TABLE orders UPDATE paid = 1 WHERE true", Dialect::Standard).len(), 1);
    assert_eq!(effect("OPTIMIZE TABLE orders FINAL", Dialect::Standard), Effect::Structure);
    assert_eq!(effect("SYSTEM DROP DNS CACHE", Dialect::Standard), Effect::Structure);
}

#[test]
fn commands_of_one_line_languages() {
    assert_eq!(assess_command("GET a", true), Assessment::of(Effect::Read));
    assert_eq!(assess_command("SET a 1", false), Assessment::of(Effect::Data));
    for flush in ["FLUSHALL", "flushdb ASYNC"] {
        let assessed = assess_command(flush, false);
        assert_eq!(assessed.dangers, vec![Danger { risk: Risk::Flush, object: None }]);
        assert!(!assessed.allowed_read_only());
    }
    assert_eq!(assess("FLUSHALL", Dialect::Lines).dangers.len(), 1);
}
