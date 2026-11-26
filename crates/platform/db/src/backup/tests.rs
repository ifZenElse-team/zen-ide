use std::path::{Path, PathBuf};

use super::find::{container_for, folders, in_container, is_local, local, parse_version, published};
use super::*;
use crate::registry::{TlsMode, TlsSettings};

fn pg_target() -> Target {
    Target {
        family: Some(Family::Postgres),
        host: "db.example.com".to_owned(),
        port: 5433,
        database: "shop".to_owned(),
        user: Some("app".to_owned()),
        tls: TlsSettings { mode: TlsMode::Require, ..TlsSettings::default() },
    }
}

fn found(tool: Tool, program: &str) -> Found {
    Found {
        tool,
        origin: Origin::Homebrew,
        program: PathBuf::from(program),
        container: None,
        docker: None,
        version: Some(format!("{} (PostgreSQL) 17.2", tool.name())),
        major: Some(17),
    }
}

fn in_docker(tool: Tool, name: &str) -> Found {
    Found {
        tool,
        origin: Origin::Docker,
        program: PathBuf::from(name),
        container: Some(Container {
            id: "abc123".to_owned(),
            name: "shop-db".to_owned(),
            image: "postgres:18".to_owned(),
            port: 5432,
        }),
        docker: Some(PathBuf::from("/usr/local/bin/docker")),
        version: Some("pg_dump (PostgreSQL) 18.0".to_owned()),
        major: Some(18),
    }
}

#[test]
fn pg_dump_writes_its_file_with_the_options_and_the_password_only_in_a_file() {
    let options = PgDumpOptions {
        format: PgFormat::Custom,
        compression: Some(6),
        schemas: vec!["public".to_owned()],
        tables: vec!["public.orders".to_owned()],
        clean: true,
        if_exists: true,
        no_owner: true,
        ..PgDumpOptions::default()
    };
    let file = Path::new("/backups/.shop.dump.part");
    let plan = backup(
        &found(Tool::PgDump, "/opt/homebrew/opt/libpq/bin/pg_dump"),
        &pg_target(),
        &options,
        &MysqldumpOptions::default(),
        file,
    )
    .unwrap();
    assert_eq!(plan.program, PathBuf::from("/opt/homebrew/opt/libpq/bin/pg_dump"));
    assert_eq!(
        plan.args,
        [
            "--host=db.example.com",
            "--port=5433",
            "--username=app",
            "--no-password",
            "--dbname=dbname='shop'",
            "--format=c",
            "--compress=6",
            "--schema=public",
            "--table=public.orders",
            "--clean",
            "--if-exists",
            "--no-owner",
            "--verbose",
            "--file=/backups/.shop.dump.part",
        ]
    );
    assert_eq!(plan.env, [("PGSSLMODE".to_owned(), "require".to_owned())]);
    assert_eq!(plan.password_way, crate::client::PasswordWay::FileInEnv("PGPASSFILE"));
    assert!(plan.stdout.is_none() && plan.stdin.is_none());
    let mut with_file = plan.clone();
    with_file.with_password_file("/tmp/zen-secret");
    assert_eq!(with_file.env.last(), Some(&("PGPASSFILE".to_owned(), "/tmp/zen-secret".to_owned())));
    assert_eq!(with_file.args, plan.args, "the password is never in the arguments");
    assert!(!with_file.env.iter().any(|(name, _)| name == "PGPASSWORD"), "nor in a password variable");
    assert_eq!(plan.password_file("a:b").as_deref(), Some("*:*:*:*:a\\:b\n"));
    let (program, args, _) = plan.shell();
    assert_eq!(program, plan.program, "no shell without a redirection");
    assert!(!args.iter().any(|arg| arg.contains("PGPASSWORD")));
}

#[test]
fn a_container_runs_the_tool_with_docker_exec_and_its_output_goes_to_the_file() {
    let file = Path::new("/backups/shop dump.part");
    let plan = backup(
        &in_docker(Tool::PgDump, "pg_dump"),
        &pg_target(),
        &PgDumpOptions::default(),
        &MysqldumpOptions::default(),
        file,
    )
    .unwrap();
    assert_eq!(plan.program, PathBuf::from("/usr/local/bin/docker"));
    assert_eq!(plan.args[..4], ["exec", "abc123", "pg_dump", "--host=127.0.0.1"].map(str::to_owned));
    assert!(plan.args.contains(&"--port=5432".to_owned()), "the container's own port: {:?}", plan.args);
    assert!(plan.env.is_empty(), "no TLS inside the container");
    assert_eq!(plan.stdout.as_deref(), Some(file));
    let (program, args, env) = plan.shell();
    assert_eq!(program, PathBuf::from("/bin/sh"));
    assert_eq!(
        args[..4],
        ["-c", "exec \"$@\" > \"$ZEN_BACKUP_OUT\"", "zen-backup", "/usr/local/bin/docker"].map(str::to_owned)
    );
    assert_eq!(env, [("ZEN_BACKUP_OUT".to_owned(), "/backups/shop dump.part".to_owned())]);
    assert!(plan.display().ends_with("> '/backups/shop dump.part'"), "{}", plan.display());
    let directory = PgDumpOptions { format: PgFormat::Directory, ..PgDumpOptions::default() };
    let refused =
        backup(&in_docker(Tool::PgDump, "pg_dump"), &pg_target(), &directory, &MysqldumpOptions::default(), file)
            .unwrap_err();
    assert_eq!(refused.code, NOT_IN_CONTAINER);
}

#[test]
fn mysqldump_takes_its_switches_tls_and_tables_after_the_database() {
    let target = Target {
        family: Some(Family::Mysql),
        host: "127.0.0.1".to_owned(),
        port: 3307,
        database: "shop".to_owned(),
        user: Some("root".to_owned()),
        tls: TlsSettings::default(),
    };
    let tool = Found {
        tool: Tool::Mysqldump,
        origin: Origin::Homebrew,
        program: PathBuf::from("/opt/homebrew/opt/mysql-client/bin/mysqldump"),
        container: None,
        docker: None,
        version: Some("mysqldump  Ver 8.4.3 for macos15.0 on arm64 (Homebrew)".to_owned()),
        major: Some(8),
    };
    let options = MysqldumpOptions {
        content: Content::SchemaOnly,
        tables: vec!["orders".to_owned(), "customers".to_owned()],
        events: true,
        verbose: false,
        ..MysqldumpOptions::default()
    };
    let plan = backup(&tool, &target, &PgDumpOptions::default(), &options, Path::new("/b/shop.sql.part")).unwrap();
    assert_eq!(
        plan.args,
        [
            "--host=127.0.0.1",
            "--port=3307",
            "--protocol=TCP",
            "--user=root",
            "--ssl-mode=PREFERRED",
            "--no-data",
            "--single-transaction",
            "--add-drop-table",
            "--triggers",
            "--extended-insert",
            "--comments",
            "--routines",
            "--events",
            "--result-file=/b/shop.sql.part",
            "shop",
            "orders",
            "customers",
        ]
    );
    assert_eq!(plan.password_way, crate::client::PasswordWay::FileArg("--defaults-extra-file="));
    let mut with_file = plan.clone();
    with_file.with_password_file("/tmp/zen-secret");
    assert_eq!(with_file.args[0], "--defaults-extra-file=/tmp/zen-secret", "the first option");
    let tricky = MysqldumpOptions { tables: vec!["--result-file=/etc/x".to_owned()], ..options.clone() };
    let refused = backup(&tool, &target, &PgDumpOptions::default(), &tricky, Path::new("/b/shop.sql.part"));
    assert_eq!(refused.unwrap_err().code, BAD_NAME, "a table name is never an option");
    let mariadb = Target { family: Some(Family::Mariadb), ..target };
    let plan = backup(&tool, &mariadb, &PgDumpOptions::default(), &options, Path::new("/b/shop.sql.part")).unwrap();
    assert!(plan.args.contains(&"--column-statistics=0".to_owned()), "MySQL 8's mysqldump on MariaDB: {:?}", plan.args);
}

#[test]
fn restores_pick_pg_restore_psql_or_mysql_and_read_the_file() {
    let target = pg_target();
    let options = PgRestoreOptions { clean: true, if_exists: true, jobs: Some(4), ..PgRestoreOptions::default() };
    let file = Path::new("/b/shop.dump");
    let plan = restore(
        &found(Tool::PgRestore, "/x/pg_restore"),
        &target,
        PgFormat::Custom,
        &options,
        &MysqlRestoreOptions::default(),
        file,
    )
    .unwrap();
    assert_eq!(plan.args.last().map(String::as_str), Some("/b/shop.dump"));
    assert!(plan.args.contains(&"--jobs=4".to_owned()) && plan.args.contains(&"--exit-on-error".to_owned()));
    let created = PgRestoreOptions { create: true, ..PgRestoreOptions::default() };
    let plan = restore(
        &found(Tool::PgRestore, "/x/pg_restore"),
        &target,
        PgFormat::Custom,
        &created,
        &MysqlRestoreOptions::default(),
        file,
    )
    .unwrap();
    assert!(plan.args.contains(&"--dbname=dbname='postgres'".to_owned()) && plan.args.contains(&"--create".to_owned()));
    let plan = restore(
        &in_docker(Tool::PgRestore, "pg_restore"),
        &target,
        PgFormat::Custom,
        &options,
        &MysqlRestoreOptions::default(),
        file,
    )
    .unwrap();
    assert_eq!(plan.args[..2], ["exec", "-i"].map(str::to_owned));
    let mut with_file = plan.clone();
    with_file.with_password_file("/tmp/zen-secret");
    assert_eq!(with_file.args[2..4], ["-e", "PGPASSFILE=/tmp/zen-secret"].map(str::to_owned), "before the container");
    assert_eq!(with_file.args[with_file.tool_at - 1], plan.args[plan.tool_at - 1], "the tool follows");
    assert_eq!(plan.stdin.as_deref(), Some(file));
    assert!(!plan.args.iter().any(|arg| arg.starts_with("--jobs")), "no parallel restore from standard input");
    let plan = restore(
        &found(Tool::Psql, "/x/psql"),
        &target,
        PgFormat::Plain,
        &PgRestoreOptions::default(),
        &MysqlRestoreOptions::default(),
        Path::new("/b/shop.sql"),
    )
    .unwrap();
    assert!(
        plan.args.contains(&"--set=ON_ERROR_STOP=1".to_owned()) && plan.args.contains(&"--file=/b/shop.sql".to_owned())
    );
    let refused = restore(
        &found(Tool::PgRestore, "/x/pg_restore"),
        &target,
        PgFormat::Plain,
        &PgRestoreOptions::default(),
        &MysqlRestoreOptions::default(),
        file,
    )
    .unwrap_err();
    assert_eq!(refused.code, NO_TOOL);
    let my = Target { family: Some(Family::Mariadb), port: 3306, ..pg_target() };
    let client = Found {
        tool: Tool::Mysql,
        program: PathBuf::from("/x/mariadb"),
        version: None,
        major: None,
        ..found(Tool::Mysql, "")
    };
    let plan = restore(
        &client,
        &my,
        PgFormat::Plain,
        &PgRestoreOptions::default(),
        &MysqlRestoreOptions { force: true, extra_args: Vec::new() },
        Path::new("/b/shop.sql"),
    )
    .unwrap();
    assert!(plan.args.contains(&"--ssl".to_owned()), "MariaDB's client names TLS its own way: {:?}", plan.args);
    assert_eq!(plan.args.last().map(String::as_str), Some("shop"));
    assert_eq!(plan.stdin.as_deref(), Some(Path::new("/b/shop.sql")));
    let (program, args, env) = plan.shell();
    assert_eq!(program, PathBuf::from("/bin/sh"));
    assert_eq!(args[1], "exec \"$@\" < \"$ZEN_BACKUP_IN\"");
    assert_eq!(env.last().map(|(name, _)| name.as_str()), Some("ZEN_BACKUP_IN"));
}

#[test]
fn the_format_of_a_backup_comes_from_what_it_starts_with() {
    let dir = tempfile::tempdir().unwrap();
    let custom = dir.path().join("a.dump");
    std::fs::write(&custom, b"PGDMP\x01\x0e\x00rest").unwrap();
    let plain = dir.path().join("a.sql");
    std::fs::write(&plain, "--\n-- PostgreSQL database dump\n").unwrap();
    let mut tar = vec![0_u8; 600];
    tar[257..262].copy_from_slice(b"ustar");
    let tarred = dir.path().join("a.tar");
    std::fs::write(&tarred, tar).unwrap();
    assert_eq!(pg_format(&custom).unwrap(), PgFormat::Custom);
    assert_eq!(pg_format(&plain).unwrap(), PgFormat::Plain);
    assert_eq!(pg_format(&tarred).unwrap(), PgFormat::Tar);
    assert_eq!(pg_format(dir.path()).unwrap(), PgFormat::Directory);
    assert!(pg_format(&dir.path().join("missing")).is_err());
    assert_eq!(restore_tool(Family::Postgres, PgFormat::Plain), Tool::Psql);
    assert_eq!(restore_tool(Family::Postgres, PgFormat::Tar), Tool::PgRestore);
    assert_eq!(restore_tool(Family::Mysql, PgFormat::Custom), Tool::Mysql);
    assert_eq!(
        file_name("shop/main", "20261027-1200", Family::Postgres, PgFormat::Custom),
        "shop_main_20261027-1200.dump"
    );
    assert_eq!(file_name("", "t", Family::Postgres, PgFormat::Directory), "backup_t");
    assert_eq!(file_name("shop", "t", Family::Mariadb, PgFormat::Custom), "shop_t.sql");
}

#[test]
fn versions_are_read_from_each_tools_own_words() {
    assert_eq!(parse_version("pg_dump (PostgreSQL) 16.4 (Homebrew)"), Some(("16.4".to_owned(), 16)));
    assert_eq!(parse_version("pg_dump (PostgreSQL) 18.0"), Some(("18.0".to_owned(), 18)));
    assert_eq!(parse_version("mysqldump  Ver 8.4.3 for macos15.0 on arm64 (Homebrew)"), Some(("8.4.3".to_owned(), 8)));
    assert_eq!(
        parse_version("mysqldump  Ver 10.19 Distrib 10.11.6-MariaDB, for Linux"),
        Some(("10.11.6".to_owned(), 10))
    );
    assert_eq!(
        parse_version("mariadb-dump from 11.4.2-MariaDB, client 10.19 for Linux"),
        Some(("11.4.2".to_owned(), 11))
    );
    assert_eq!(parse_version("nothing here"), None);
}

#[test]
fn docker_ports_find_the_container_that_publishes_the_connections_port() {
    assert_eq!(published("0.0.0.0:55432->5432/tcp, [::]:55432->5432/tcp"), [(55432, 5432)]);
    assert_eq!(published("127.0.0.1:14333->1433/tcp"), [(14333, 1433)]);
    assert_eq!(published("33060/tcp, 0.0.0.0:8000-8001->9000-9001/tcp"), [(8000, 9000), (8001, 9001)]);
    let listing = "aaa\tweb\tnginx\t0.0.0.0:8080->80/tcp\nbbb\tshop-db\tpostgres:18\t0.0.0.0:5433->5432/tcp\nccc\tidle\tbusybox\t\n";
    let container = container_for(listing, 5433).unwrap();
    assert_eq!((container.id.as_str(), container.name.as_str(), container.port), ("bbb", "shop-db", 5432));
    assert!(container_for(listing, 5432).is_none());
    assert!(is_local("localhost") && is_local("127.0.0.1") && is_local("[::1]") && is_local(""));
    assert!(!is_local("db.example.com"));
    let printed = "pg_dump\tpg_dump (PostgreSQL) 18.0\npsql\tpsql (PostgreSQL) 18.0\n";
    let tools = in_container(Family::Postgres, printed, &container, Path::new("/usr/bin/docker"));
    assert_eq!(tools.iter().map(|found| found.tool).collect::<Vec<_>>(), [Tool::PgDump, Tool::Psql]);
    assert!(tools.iter().all(|found| found.origin == Origin::Docker && found.major == Some(18)));
}

#[test]
fn tools_are_found_in_path_and_keg_only_homebrew_folders_and_the_newest_is_chosen() {
    let dir = tempfile::tempdir().unwrap();
    let brew = dir.path().join("brew");
    let tool = |folder: &Path, name: &str| {
        std::fs::create_dir_all(folder).unwrap();
        std::fs::write(folder.join(name), "#!/bin/sh\n").unwrap();
    };
    tool(&brew.join("opt/libpq/bin"), "pg_dump");
    tool(&brew.join("opt/postgresql@14/bin"), "pg_dump");
    tool(&brew.join("opt/postgresql@17/bin"), "pg_restore");
    tool(&brew.join("opt/mysql-client/bin"), "mysqldump");
    tool(&brew.join("opt/unrelated/bin"), "pg_dump");
    let on_path = dir.path().join("path");
    tool(&on_path, "psql");
    let searched = folders(vec![on_path.clone()], std::slice::from_ref(&brew), &dir.path().join("Postgres.app"));
    assert_eq!(searched[0], (on_path.clone(), Origin::Path));
    let kegs: Vec<&PathBuf> =
        searched.iter().filter(|(_, origin)| *origin == Origin::Homebrew).map(|(folder, _)| folder).collect();
    assert_eq!(
        kegs,
        [
            &brew.join("bin"),
            &brew.join("opt/postgresql@17/bin"),
            &brew.join("opt/postgresql@14/bin"),
            &brew.join("opt/libpq/bin"),
            &brew.join("opt/mysql-client/bin"),
        ],
        "keg-only formulae, the newest version first"
    );
    let pg = local(Family::Postgres, &searched);
    let programs: Vec<(Tool, &Path)> = pg.iter().map(|found| (found.tool, found.program.as_path())).collect();
    assert!(programs.contains(&(Tool::PgDump, brew.join("opt/libpq/bin/pg_dump").as_path())));
    assert!(programs.contains(&(Tool::PgRestore, brew.join("opt/postgresql@17/bin/pg_restore").as_path())));
    assert!(programs.contains(&(Tool::Psql, on_path.join("psql").as_path())));
    assert!(!programs.iter().any(|(_, program)| program.starts_with(brew.join("opt/unrelated"))));
    let my = local(Family::Mysql, &searched);
    assert_eq!(my.len(), 1);

    let older = Found { major: Some(16), ..found(Tool::PgDump, "/a/pg_dump") };
    let newer = Found { major: Some(17), ..found(Tool::PgDump, "/b/pg_dump") };
    let detection = Detection {
        tools: vec![older.clone(), newer.clone(), in_docker(Tool::PgDump, "pg_dump")],
        docker: DockerState::NotLocal,
    };
    assert_eq!(
        choose(&detection, Tool::PgDump).map(|found| found.origin),
        Some(Origin::Docker),
        "the server is newer than 17"
    );
    let detection = Detection { tools: vec![older, newer.clone()], docker: DockerState::NotLocal };
    assert_eq!(choose(&detection, Tool::PgDump), Some(&newer));
    assert!(choose(&detection, Tool::Psql).is_none());
}

#[test]
fn a_postgresql_tool_of_the_servers_version_is_taken_and_a_mismatch_is_refused() {
    let at = |tool: Tool, major: u32, origin: Origin| Found {
        major: Some(major),
        origin,
        ..found(tool, &format!("/opt/homebrew/opt/postgresql@{major}/bin/{}", tool.name()))
    };
    let (r18, r16) = (at(Tool::PgRestore, 18, Origin::Homebrew), at(Tool::PgRestore, 16, Origin::Homebrew));
    assert_eq!(for_server(&[&r18, &r16], Some(&r18), Some(16)), Ok(Some(&r16)), "the server's own version");
    assert_eq!(
        for_server(&[&r18], Some(&r18), Some(16)),
        Err(VersionProblem::TooNew { tool: 18, server: 16 }),
        "a newer pg_restore only: refused, never half a restore"
    );
    let r15 = at(Tool::PgRestore, 15, Origin::Path);
    assert_eq!(for_server(&[&r18, &r15], Some(&r18), Some(16)), Ok(Some(&r15)), "else the newest not newer");
    let (d18, d14) = (at(Tool::PgDump, 18, Origin::Homebrew), at(Tool::PgDump, 14, Origin::Path));
    assert_eq!(for_server(&[&d18, &d14], Some(&d18), Some(16)), Ok(Some(&d18)), "a newer pg_dump backs up");
    assert_eq!(
        for_server(&[&d14], Some(&d14), Some(16)),
        Err(VersionProblem::TooOld { tool: 14, server: 16 }),
        "an older pg_dump cannot"
    );
    assert_eq!(for_server(&[&r18], Some(&r18), None), Ok(Some(&r18)), "an unknown server keeps the choice");
    let mysql = at(Tool::Mysql, 9, Origin::Path);
    assert_eq!(for_server(&[&mysql], Some(&mysql), Some(8)), Ok(Some(&mysql)), "MySQL tools are not judged here");
    assert_eq!(server_major("160004\n"), Some(16));
    assert_eq!(server_major("90624"), Some(9));
    assert_eq!(server_major("not a number"), None);
}

#[test]
fn a_plain_script_runs_with_any_psql() {
    let psql = Found { major: Some(18), ..found(Tool::Psql, "/opt/homebrew/bin/psql") };
    assert_eq!(for_server(&[&psql], Some(&psql), Some(16)), Ok(Some(&psql)));
}

#[test]
fn a_name_starting_with_a_dash_never_reaches_a_backup_tool_as_an_option() {
    let target = |database: &str| Target {
        family: Some(Family::Mysql),
        host: "127.0.0.1".to_owned(),
        port: 3306,
        database: database.to_owned(),
        user: Some("root".to_owned()),
        tls: TlsSettings::default(),
    };
    let file = Path::new("/b/shop.sql");
    let dump = |target: &Target, options: &MysqldumpOptions| {
        backup(&found(Tool::Mysqldump, "/x/mysqldump"), target, &PgDumpOptions::default(), options, file)
    };
    let refused = |error: PlanError| error.code == BAD_NAME;
    assert!(dump(&target("--all-databases"), &MysqldumpOptions::default()).is_err_and(refused));
    let tables = MysqldumpOptions { tables: vec!["--all-databases".to_owned()], ..MysqldumpOptions::default() };
    assert!(dump(&target("shop"), &tables).is_err_and(refused));
    assert!(dump(&target("shop"), &MysqldumpOptions::default()).is_ok());
    let restored = restore(
        &found(Tool::Mysql, "/x/mysql"),
        &target("--init-command=x"),
        PgFormat::Plain,
        &PgRestoreOptions::default(),
        &MysqlRestoreOptions::default(),
        file,
    );
    assert!(restored.is_err_and(refused));
}

#[test]
fn a_program_the_project_brings_is_never_found_as_a_backup_tool() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("shop");
    let (brought, installed) = (project.join("bin"), dir.path().join("tools"));
    std::fs::create_dir_all(&brought).unwrap();
    std::fs::create_dir_all(&installed).unwrap();
    let probe = Probe::on_path(vec![brought.clone(), installed.clone()], Some(&project));
    let searched: Vec<&PathBuf> = probe.folders.iter().map(|(folder, _)| folder).collect();
    assert!(searched.contains(&&installed) && !searched.contains(&&brought), "{searched:?}");
}
