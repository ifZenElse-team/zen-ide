use std::path::Path;

use super::command::{Direction, Refusal, Target, Via, invocation};
use super::*;

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn one(root: &Path, folder: &str) -> MigrationSet {
    let mut sets = recognise(root, &root.join(folder));
    assert_eq!(sets.len(), 1, "{folder}: {sets:?}");
    sets.remove(0)
}

fn versions(set: &MigrationSet) -> Vec<&str> {
    set.migrations.iter().map(|migration| migration.version.as_str()).collect()
}

fn rows(list: &[&[Option<&str>]]) -> Vec<Vec<Option<String>>> {
    list.iter().map(|row| row.iter().map(|cell| cell.map(str::to_owned)).collect()).collect()
}

fn states(list: &[Status]) -> Vec<(&str, State, bool)> {
    list.iter().map(|status| (status.version.as_str(), status.state, status.file.is_some())).collect()
}

#[test]
fn each_tool_is_recognised_from_its_own_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "flyway.conf", "flyway.locations=filesystem:db/sql\n");
    write(root, "db/sql/V1__init.sql", "CREATE TABLE a (id int);");
    write(root, "db/sql/V1_10__more.sql", "");
    write(root, "db/sql/V1_9__less.sql", "");
    write(root, "db/sql/U1_9__less.sql", "");
    write(root, "db/sql/R__views.sql", "");
    let flyway = one(root, "db/sql");
    assert_eq!(flyway.tool, Tool::Flyway);
    assert_eq!(versions(&flyway), ["1", "1.9", "1.10", "R:views"]);
    assert_eq!(flyway.config.as_deref(), Some("flyway.conf"));
    assert_eq!(flyway.base, ".");
    assert!(flyway.migrations[1].reversible && !flyway.migrations[2].reversible);

    write(root, "go/go.mod", "module x\nrequire github.com/golang-migrate/migrate/v4 v4.17.0\n");
    write(root, "go/migrations/000001_users.up.sql", "");
    write(root, "go/migrations/000001_users.down.sql", "");
    write(root, "go/migrations/000002_orders.up.sql", "");
    let migrate = one(root, "go/migrations");
    assert_eq!((migrate.tool, migrate.base.as_str()), (Tool::GolangMigrate, "go"));
    assert_eq!(versions(&migrate), ["000001", "000002"]);
    assert!(migrate.migrations[0].reversible && !migrate.migrations[1].reversible);

    write(
        root,
        "goose/db/20240101000000_users.sql",
        "-- +goose Up\nCREATE TABLE u ();\n-- +goose Down\nDROP TABLE u;\n",
    );
    write(root, "goose/db/20240102000000_seed.go", "package db // +goose\n");
    let goose = one(root, "goose/db");
    assert_eq!(goose.tool, Tool::Goose);
    assert_eq!(versions(&goose), ["20240101000000", "20240102000000"]);

    write(root, "atlas.hcl", "env \"local\" {}\n");
    write(root, "atlas/migrations/20240101_init.sql", "");
    write(root, "atlas/migrations/atlas.sum", "h1:x\n");
    let atlas = one(root, "atlas/migrations");
    assert_eq!((atlas.tool, atlas.config.as_deref()), (Tool::Atlas, Some("atlas.hcl")));

    write(root, "rs/Cargo.toml", "[dependencies]\nsqlx = \"0.8\"\n");
    write(root, "rs/migrations/20240101_users.sql", "");
    write(root, "rs/seeds/001_data.sql", "");
    let sqlx = one(root, "rs/migrations");
    assert_eq!((sqlx.tool, sqlx.base.as_str()), (Tool::Sqlx, "rs"));
    assert!(recognise(root, &root.join("rs/seeds")).is_empty(), "numbered files outside a migrations folder");

    write(root, "diesel/diesel.toml", "[migrations_directory]\ndir = \"migrations\"\n");
    write(root, "diesel/migrations/00000000000000_diesel_initial_setup/up.sql", "");
    write(root, "diesel/migrations/00000000000000_diesel_initial_setup/down.sql", "");
    write(root, "diesel/migrations/2024-01-01-120000_posts/up.sql", "");
    let diesel = one(root, "diesel/migrations");
    assert_eq!(diesel.tool, Tool::Diesel);
    assert_eq!(versions(&diesel), ["00000000000000", "20240101120000"]);
    assert_eq!(diesel.config.as_deref(), Some("diesel/diesel.toml"));

    write(root, "py/alembic.ini", "[alembic]\nscript_location = alembic\n");
    write(root, "py/alembic/env.py", "");
    write(root, "py/alembic/versions/b2_orders.py", "revision = 'b2'\ndown_revision = 'a1'\ndef downgrade(): pass\n");
    write(root, "py/alembic/versions/a1_init.py", "revision: str = \"a1\"\ndown_revision: Union[str, None] = None\n");
    let alembic = one(root, "py/alembic/versions");
    assert_eq!((alembic.tool, alembic.base.as_str()), (Tool::Alembic, "py"));
    assert_eq!(versions(&alembic), ["a1", "b2"]);
    assert_eq!(alembic.migrations[1].parents, ["a1"]);

    write(root, "site/manage.py", "");
    write(root, "site/shop/migrations/__init__.py", "");
    write(root, "site/shop/migrations/0001_initial.py", "");
    write(root, "site/shop/migrations/0002_orders.py", "");
    let django = one(root, "site/shop/migrations");
    assert_eq!((django.tool, django.app.as_deref(), django.base.as_str()), (Tool::Django, Some("shop"), "site"));
    assert_eq!(versions(&django), ["0001_initial", "0002_orders"]);

    write(root, "web/package.json", "{}");
    write(root, "web/prisma/schema.prisma", "datasource db { url = env(\"DATABASE_URL\") }");
    write(root, "web/prisma/migrations/20240101000000_init/migration.sql", "");
    write(root, "web/prisma/migrations/migration_lock.toml", "");
    let prisma = one(root, "web/prisma/migrations");
    assert_eq!(prisma.tool, Tool::Prisma);
    assert_eq!((prisma.base.as_str(), prisma.config.as_deref()), ("web", Some("web/prisma/schema.prisma")));

    write(root, "app/Gemfile", "gem 'rails'\n");
    write(root, "app/db/migrate/20240101000000_create_users.rb", "");
    let rails = one(root, "app/db/migrate");
    assert_eq!((rails.tool, rails.base.as_str()), (Tool::Rails, "app"));

    write(root, "node/knexfile.js", "module.exports = {}");
    write(root, "node/migrations/20240101000000_users.js", "");
    let knex = one(root, "node/migrations");
    assert_eq!((knex.tool, knex.migrations[0].version.as_str()), (Tool::Knex, "20240101000000_users.js"));

    write(
        root,
        "java/db/changelog/db.changelog-master.xml",
        "<databaseChangeLog><changeSet id=\"1\" author=\"ann\"/><changeSet author=\"bob\" id=\"2\"></changeSet></databaseChangeLog>",
    );
    write(
        root,
        "java/db/changelog/changelog-2.sql",
        "--liquibase formatted sql\n--changeset eve:3 labels:x\nSELECT 1;\n",
    );
    let liquibase = one(root, "java/db/changelog");
    assert_eq!(liquibase.tool, Tool::Liquibase);
    let mut keys = versions(&liquibase);
    keys.sort_unstable();
    assert_eq!(keys, ["1::ann", "2::bob", "3::eve"]);

    write(root, "yaml/changelog.yaml", "databaseChangeLog:\n  - changeSet:\n      id: 7\n      author: kim\n");
    assert_eq!(versions(&one(root, "yaml")), ["7::kim"]);

    write(root, "notes/001_readme.sql", "");
    assert!(recognise(root, &root.join("notes")).is_empty(), "plain numbered SQL of no known tool");
    assert_ne!(set_id(Tool::Flyway, "db/sql", None), set_id(Tool::Flyway, "db/sql2", None));
}

#[test]
fn the_history_table_says_what_is_applied() {
    let set = |tool: Tool, list: &[&str]| MigrationSet {
        id: String::new(),
        tool,
        folder: "m".to_owned(),
        base: ".".to_owned(),
        config: None,
        app: Some("shop".to_owned()),
        migrations: list
            .iter()
            .map(|version| Migration {
                version: (*version).to_owned(),
                name: String::new(),
                file: format!("m/{version}"),
                reversible: true,
                parents: Vec::new(),
            })
            .collect(),
        truncated: false,
    };
    let flyway = set(Tool::Flyway, &["1", "2", "3"]);
    let history = read_history(
        Tool::Flyway,
        None,
        &rows(&[&[Some("1"), Some("init"), Some("t")], &[Some("2"), Some("x"), Some("f")]]),
    );
    assert_eq!(
        states(&statuses(&flyway, Some(&history))),
        [("1", State::Applied, true), ("2", State::Failed, true), ("3", State::Pending, true)]
    );
    assert!(statuses(&flyway, None).iter().all(|status| status.state == State::Unknown));
    let none = History { table: false, entries: Vec::new() };
    assert!(statuses(&flyway, Some(&none)).iter().all(|status| status.state == State::Pending));

    let migrate = set(Tool::GolangMigrate, &["000001", "000002", "000003"]);
    let dirty = read_history(Tool::GolangMigrate, None, &rows(&[&[Some("2"), Some("true")]]));
    assert_eq!(
        states(&statuses(&migrate, Some(&dirty))),
        [("000001", State::Applied, true), ("000002", State::Failed, true), ("000003", State::Pending, true)]
    );
    let ahead = read_history(Tool::GolangMigrate, None, &rows(&[&[Some("9"), Some("false")]]));
    assert_eq!(
        statuses(&migrate, Some(&ahead)).last().map(|status| (status.state, status.file.is_none())),
        Some((State::Applied, true))
    );

    let goose = read_history(
        Tool::Goose,
        None,
        &rows(&[&[Some("0"), Some("t")], &[Some("1"), Some("t")], &[Some("2"), Some("t")], &[Some("2"), Some("f")]]),
    );
    assert_eq!(goose.entries.iter().map(|entry| entry.version.as_str()).collect::<Vec<_>>(), ["1"]);

    let mut alembic = set(Tool::Alembic, &["a1", "b2", "c3"]);
    alembic.migrations[1].parents = vec!["a1".to_owned()];
    alembic.migrations[2].parents = vec!["b2".to_owned()];
    let head = read_history(Tool::Alembic, None, &rows(&[&[Some("b2")]]));
    assert_eq!(
        states(&statuses(&alembic, Some(&head))),
        [("a1", State::Applied, true), ("b2", State::Applied, true), ("c3", State::Pending, true)]
    );

    let django = set(Tool::Django, &["0001_initial", "0002_orders", "0003_more"]);
    let applied = read_history(
        Tool::Django,
        Some("shop"),
        &rows(&[
            &[Some("auth"), Some("0001_initial")],
            &[Some("shop"), Some("0001_initial")],
            &[Some("shop"), Some("0002_orders")],
        ]),
    );
    let list = statuses(&django, Some(&applied));
    assert_eq!(django_previous(&list).as_deref(), Some("0001_initial"));
    let first = read_history(Tool::Django, Some("shop"), &rows(&[&[Some("shop"), Some("0001_initial")]]));
    assert_eq!(django_previous(&statuses(&django, Some(&first))).as_deref(), Some("zero"));
    assert_eq!(django_previous(&statuses(&django, Some(&none))), None);

    let prisma = read_history(
        Tool::Prisma,
        None,
        &rows(&[&[Some("a"), Some("2024"), None], &[Some("b"), None, None], &[Some("c"), Some("2024"), Some("2024")]]),
    );
    assert_eq!(
        prisma.entries,
        [HistoryEntry { version: "a".into(), ok: true }, HistoryEntry { version: "b".into(), ok: false }]
    );

    let gone = read_history(Tool::Rails, None, &rows(&[&[Some("1")], &[Some("5")]]));
    let rails = set(Tool::Rails, &["1", "2"]);
    assert_eq!(
        states(&statuses(&rails, Some(&gone))),
        [("1", State::Applied, true), ("2", State::Pending, true), ("5", State::Applied, false)]
    );
    assert!(missing_table("ERROR: relation \"flyway_schema_history\" does not exist (SQLSTATE 42P01)"));
    assert!(missing_table("no such table: knex_migrations"));
    assert!(!missing_table("permission denied for table flyway_schema_history"));
    assert!(history_query(Tool::Liquibase, SqlDialect::MySql).contains("DATABASECHANGELOG"));
    assert!(history_query(Tool::Atlas, SqlDialect::Postgres).contains("atlas_schema_revisions.atlas_schema_revisions"));
}

#[test]
fn versions_compare_as_numbers() {
    assert_eq!(natural("1.9", "1.10"), Ordering::Less);
    assert_eq!(natural("0002_a", "0010_a"), Ordering::Less);
    assert_eq!(natural("20240101", "20240101"), Ordering::Equal);
    assert_eq!(natural("a", "b"), Ordering::Less);
}

fn target(driver: &str) -> Target {
    Target {
        driver: driver.to_owned(),
        host: "localhost".to_owned(),
        port: Some(5433),
        database: "shop db".to_owned(),
        user: Some("app".to_owned()),
        password: Some("s3cr@t".to_owned()),
        tls: Some("prefer".to_owned()),
    }
}

fn of(tool: Tool, folder: &str, base: &str) -> MigrationSet {
    MigrationSet {
        id: String::new(),
        tool,
        folder: folder.to_owned(),
        base: base.to_owned(),
        config: None,
        app: Some("shop".to_owned()),
        migrations: vec![Migration {
            version: "1".to_owned(),
            name: String::new(),
            file: format!("{folder}/db.changelog-master.xml"),
            reversible: true,
            parents: Vec::new(),
        }],
        truncated: false,
    }
}

#[test]
fn the_password_is_never_in_a_tools_environment_or_arguments() {
    let pg = target("postgresql");
    for tool in Tool::ALL {
        for direction in [Direction::Up, Direction::Down] {
            let set = of(tool, "db/migrations", ".");
            let built = invocation(&set, direction, &pg, Some("0001_initial"));
            if tool == Tool::Prisma && direction == Direction::Down {
                assert_eq!(built, Err(Refusal::NoDown));
                continue;
            }
            let url_only = matches!(tool, Tool::Alembic | Tool::Django | Tool::Prisma | Tool::Rails | Tool::Knex);
            if url_only {
                assert_eq!(built, Err(Refusal::PasswordInUrl), "{tool:?} takes a password only in a URL");
                continue;
            }
            let built = built.unwrap();
            assert!(!built.display.contains("s3cr"), "{tool:?}: {}", built.display);
            assert!(!built.args.iter().any(|arg| arg.contains("s3cr")), "{tool:?} {:?}", built.args);
            assert!(!built.env.iter().any(|(_, value)| value.contains("s3cr")), "{tool:?} {:?}", built.env);
            let secret = built.secret.unwrap_or_else(|| panic!("{tool:?} gets the password in a file"));
            assert!(secret.text.contains("s3cr@t"), "{tool:?}: {}", secret.text);
        }
    }
    let migrate = invocation(&of(Tool::GolangMigrate, "go/migrations", "go"), Direction::Down, &pg, None).unwrap();
    assert_eq!(
        migrate.args,
        ["-path", "migrations", "-database", "postgres://app@localhost:5433/shop%20db?sslmode=disable", "down", "1"]
    );
    assert!(migrate.env.is_empty());
    let secret = migrate.secret.unwrap();
    assert_eq!((secret.text.as_str(), secret.via), ("*:*:*:*:s3cr@t\n", Via::Env("PGPASSFILE")));

    let mysql = target("mysql");
    let refused = invocation(&of(Tool::GolangMigrate, "m", "."), Direction::Up, &mysql, None);
    assert_eq!(refused, Err(Refusal::PasswordInUrl), "golang-migrate takes MySQL's password only in its URL");
    let without = Target { password: None, ..mysql };
    let shown = invocation(&of(Tool::GolangMigrate, "m", "."), Direction::Up, &without, None).unwrap();
    assert!(shown.display.contains("mysql://app@tcp(localhost:5433)/shop%20db"), "{}", shown.display);

    let sqlx = invocation(&of(Tool::Sqlx, "rs/migrations", "rs"), Direction::Up, &pg, None).unwrap();
    assert_eq!(sqlx.args, ["migrate", "run", "--source", "migrations"]);
    assert_eq!(sqlx.env[0].1, "postgresql://app@localhost:5433/shop%20db?sslmode=prefer");

    let flyway = invocation(&of(Tool::Flyway, "db/sql", "."), Direction::Up, &pg, None).unwrap();
    assert!(
        flyway.env.contains(&(
            "FLYWAY_URL".to_owned(),
            "jdbc:postgresql://localhost:5433/shop%20db?sslmode=prefer".to_owned()
        ))
    );
    let secret = flyway.secret.unwrap();
    assert_eq!((secret.text.as_str(), secret.via), ("flyway.password=s3cr@t\n", Via::FirstArg("-configFiles=")));

    let mut bad = of(Tool::Django, "site/shop/migrations", "site");
    bad.app = Some("--settings=evil".to_owned());
    let no_password = Target { password: None, ..pg.clone() };
    assert_eq!(
        invocation(&bad, Direction::Up, &no_password, None),
        Err(Refusal::BadName("--settings=evil".to_owned())),
        "an app name is never an option"
    );

    let liquibase = invocation(&of(Tool::Liquibase, "src/db", "src"), Direction::Down, &pg, None).unwrap();
    assert_eq!(liquibase.args, ["rollback-count", "--changelog-file=db/db.changelog-master.xml", "--count=1"]);

    let mut django = of(Tool::Django, "site/shop/migrations", "site");
    assert_eq!(invocation(&django, Direction::Down, &no_password, None), Err(Refusal::NeedsTarget));
    let back = invocation(&django, Direction::Down, &no_password, Some("zero")).unwrap();
    assert_eq!(back.args, ["manage.py", "migrate", "shop", "zero"]);
    django.app = None;
    assert_eq!(invocation(&django, Direction::Down, &no_password, Some("zero")), Err(Refusal::NeedsTarget));

    let sqlite = Target { driver: "sqlite".to_owned(), database: "/p/app.db".to_owned(), ..Target::default() };
    let diesel = invocation(&of(Tool::Diesel, "migrations", "."), Direction::Up, &sqlite, None).unwrap();
    assert_eq!(diesel.env, [("DATABASE_URL".to_owned(), "/p/app.db".to_owned())]);
    let redis = Target { driver: "redis".to_owned(), ..Target::default() };
    assert_eq!(
        invocation(&of(Tool::Sqlx, "m", "."), Direction::Up, &redis, None),
        Err(Refusal::Driver("redis".into()))
    );
}
