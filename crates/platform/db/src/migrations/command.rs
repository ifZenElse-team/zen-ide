use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{MigrationSet, Tool};

/// Which way a run goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    /// Applies every pending migration.
    #[default]
    Up,
    /// Reverts the last applied migration.
    Down,
}

impl Direction {
    pub fn id(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }

    pub fn from_id(id: &str) -> Option<Direction> {
        match id {
            "up" => Some(Direction::Up),
            "down" => Some(Direction::Down),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub driver: String,
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub tls: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub secret: Option<SecretFile>,
    pub display: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretFile {
    pub text: String,
    pub via: Via,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Env(&'static str),
    FirstArg(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    NoDown,
    Driver(String),
    NeedsTarget,
    PasswordInUrl,
    BadName(String),
}

pub const DRIVERS: [&str; 4] = ["postgresql", "mysql", "mariadb", "sqlite"];

const MASK: &str = "******";

fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn default_port(driver: &str) -> u16 {
    match driver {
        "postgresql" => 5432,
        _ => 3306,
    }
}

fn local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

#[derive(Clone, Copy)]
enum Form {
    Url,
    PqUrl,
    GoMysql,
    MigrateMysql,
    Jdbc,
    SqlAlchemy,
    Prisma,
    Rails,
    Path,
}

fn userinfo(target: &Target, password: Option<&str>) -> String {
    let user = target.user.as_deref().filter(|user| !user.is_empty()).map(encode);
    match (user, password) {
        (Some(user), Some(password)) => {
            format!("{user}:{}@", if password == MASK { password.to_owned() } else { encode(password) })
        }
        (Some(user), None) => format!("{user}@"),
        (None, Some(password)) => {
            format!(":{}@", if password == MASK { password.to_owned() } else { encode(password) })
        }
        (None, None) => String::new(),
    }
}

fn url(target: &Target, form: Form, password: Option<&str>) -> String {
    let sqlite = target.driver == "sqlite";
    if sqlite {
        let path = &target.database;
        return match form {
            Form::Url => format!("sqlite://{path}"),
            Form::PqUrl => format!("sqlite3://{path}"),
            Form::Jdbc => format!("jdbc:sqlite:{path}"),
            Form::SqlAlchemy => format!("sqlite:///{path}"),
            Form::Prisma => format!("file:{path}"),
            Form::Rails => format!("sqlite3:{path}"),
            Form::GoMysql | Form::MigrateMysql | Form::Path => path.clone(),
        };
    }
    let postgres = target.driver == "postgresql";
    let port = target.port.unwrap_or_else(|| default_port(&target.driver));
    let host = &target.host;
    let database = encode(&target.database);
    let tls = target.tls.as_deref().filter(|mode| !mode.is_empty());
    let credentials = userinfo(target, password);
    match form {
        Form::Jdbc => {
            let scheme = match target.driver.as_str() {
                "postgresql" => "postgresql",
                "mariadb" => "mariadb",
                _ => "mysql",
            };
            let query = match (postgres, tls) {
                (true, Some(mode)) => format!("?sslmode={mode}"),
                _ => String::new(),
            };
            format!("jdbc:{scheme}://{host}:{port}/{database}{query}")
        }
        Form::GoMysql => format!("{credentials}tcp({host}:{port})/{database}?parseTime=true&multiStatements=true"),
        Form::MigrateMysql => format!("mysql://{credentials}tcp({host}:{port})/{database}?multiStatements=true"),
        _ => {
            let scheme = match (postgres, form) {
                (true, Form::PqUrl) => "postgres",
                (true, _) => "postgresql",
                (false, Form::SqlAlchemy) => "mysql+pymysql",
                (false, Form::Rails) => "mysql2",
                (false, _) => "mysql",
            };
            let mode = match (postgres, form, tls) {
                (true, Form::PqUrl, Some("prefer") | None) => {
                    Some(if local_host(host) { "disable" } else { "require" }.to_owned())
                }
                (true, _, Some(mode)) => Some(mode.to_owned()),
                _ => None,
            };
            let query = mode.map(|mode| format!("?sslmode={mode}")).unwrap_or_default();
            format!("{scheme}://{credentials}{host}:{port}/{database}{query}")
        }
    }
}

struct Built {
    program: &'static str,
    args: Vec<String>,
    shown: Vec<String>,
    env: Vec<(String, String)>,
    secret: Option<SecretFile>,
}

fn pgpass(target: &Target) -> Result<Option<SecretFile>, Refusal> {
    match &target.password {
        None => Ok(None),
        Some(password) if password.contains(['\n', '\r']) => Err(Refusal::PasswordInUrl),
        Some(password) => {
            let text = format!("*:*:*:*:{}\n", password.replace('\\', "\\\\").replace(':', "\\:"));
            Ok(Some(SecretFile { text, via: Via::Env("PGPASSFILE") }))
        }
    }
}

fn property(key: &str, value: &str) -> String {
    let escaped =
        value.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('=', "\\=").replace(':', "\\:");
    format!("{key}={escaped}\n")
}

fn no_password_in_url(target: &Target) -> Result<(), Refusal> {
    if target.password.is_some() { Err(Refusal::PasswordInUrl) } else { Ok(()) }
}

fn relative_to(base: &str, path: &str) -> String {
    if base == "." {
        return path.to_owned();
    }
    match path.strip_prefix(base).and_then(|rest| rest.strip_prefix('/')) {
        Some(rest) => rest.to_owned(),
        None if path == base => ".".to_owned(),
        None => {
            let depth = base.split('/').count();
            format!("{}{path}", "../".repeat(depth))
        }
    }
}

fn plain(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

pub fn invocation(
    set: &MigrationSet,
    direction: Direction,
    target: &Target,
    down_to: Option<&str>,
) -> Result<Invocation, Refusal> {
    if !DRIVERS.contains(&target.driver.as_str()) {
        return Err(Refusal::Driver(target.driver.clone()));
    }
    if direction == Direction::Down && !set.tool.reverts() {
        return Err(Refusal::NoDown);
    }
    let folder = relative_to(&set.base, &set.folder);
    let down = direction == Direction::Down;
    let postgres = target.driver == "postgresql";
    let pg_or_none = |target: &Target| -> Result<Option<SecretFile>, Refusal> {
        if postgres { pgpass(target) } else { no_password_in_url(target).map(|()| None) }
    };
    let built = match set.tool {
        Tool::Flyway => {
            let mut env = vec![("FLYWAY_URL".to_owned(), url(target, Form::Jdbc, None))];
            env.extend(target.user.clone().map(|user| ("FLYWAY_USER".to_owned(), user)));
            let secret = target.password.as_deref().map(|password| SecretFile {
                text: property("flyway.password", password),
                via: Via::FirstArg("-configFiles="),
            });
            let args =
                vec![if down { "undo" } else { "migrate" }.to_owned(), format!("-locations=filesystem:{folder}")];
            Built { program: "flyway", shown: args.clone(), args, env, secret }
        }
        Tool::Liquibase => {
            let changelog =
                set.migrations.first().map_or_else(String::new, |first| relative_to(&set.base, &first.file));
            let mut env = vec![("LIQUIBASE_COMMAND_URL".to_owned(), url(target, Form::Jdbc, None))];
            env.extend(target.user.clone().map(|user| ("LIQUIBASE_COMMAND_USERNAME".to_owned(), user)));
            let secret = target.password.as_deref().map(|password| SecretFile {
                text: property("liquibase.command.password", password),
                via: Via::FirstArg("--defaults-file="),
            });
            let mut args = vec![if down { "rollback-count" } else { "update" }.to_owned()];
            args.push(format!("--changelog-file={changelog}"));
            if down {
                args.push("--count=1".to_owned());
            }
            Built { program: "liquibase", shown: args.clone(), args, env, secret }
        }
        Tool::GolangMigrate => {
            let form =
                if matches!(target.driver.as_str(), "mysql" | "mariadb") { Form::MigrateMysql } else { Form::PqUrl };
            let secret = pg_or_none(target)?;
            let mut args = plain(&["-path", &folder, "-database"]);
            args.push(url(target, form, None));
            args.extend(if down { plain(&["down", "1"]) } else { plain(&["up"]) });
            Built { program: "migrate", shown: args.clone(), args, env: Vec::new(), secret }
        }
        Tool::Goose => {
            let (driver, form) = match target.driver.as_str() {
                "postgresql" => ("postgres", Form::Url),
                "sqlite" => ("sqlite3", Form::Path),
                _ => ("mysql", Form::GoMysql),
            };
            let secret = pg_or_none(target)?;
            let env = vec![
                ("GOOSE_DRIVER".to_owned(), driver.to_owned()),
                ("GOOSE_DBSTRING".to_owned(), url(target, form, None)),
                ("GOOSE_MIGRATION_DIR".to_owned(), folder.clone()),
            ];
            let args = plain(&["-dir", &folder, if down { "down" } else { "up" }]);
            Built { program: "goose", shown: args.clone(), args, env, secret }
        }
        Tool::Atlas => {
            let secret = pg_or_none(target)?;
            let form = if postgres { Form::PqUrl } else { Form::Url };
            let dir = format!("file://{folder}");
            let mut args = plain(&["migrate", if down { "down" } else { "apply" }, "--dir", &dir, "--url"]);
            args.push(url(target, form, None));
            Built { program: "atlas", shown: args.clone(), args, env: Vec::new(), secret }
        }
        Tool::Sqlx => {
            let secret = pg_or_none(target)?;
            let args = plain(&["migrate", if down { "revert" } else { "run" }, "--source", &folder]);
            let env = vec![("DATABASE_URL".to_owned(), url(target, Form::Url, None))];
            Built { program: "sqlx", shown: args.clone(), args, env, secret }
        }
        Tool::Diesel => {
            let args = plain(&["migration", if down { "revert" } else { "run" }, "--migration-dir", &folder]);
            let form = if target.driver == "sqlite" { Form::Path } else { Form::Url };
            let secret = pg_or_none(target)?;
            let env = vec![("DATABASE_URL".to_owned(), url(target, form, None))];
            Built { program: "diesel", shown: args.clone(), args, env, secret }
        }
        Tool::Alembic => {
            no_password_in_url(target)?;
            let args = if down { plain(&["downgrade", "-1"]) } else { plain(&["upgrade", "head"]) };
            let env = vec![("DATABASE_URL".to_owned(), url(target, Form::SqlAlchemy, None))];
            Built { program: "alembic", shown: args.clone(), args, env, secret: None }
        }
        Tool::Django => {
            no_password_in_url(target)?;
            let app = set.app.clone().unwrap_or_default();
            let identifier = app.chars().next().is_none_or(|first| first.is_alphabetic() || first == '_')
                && app.chars().all(|c| c.is_alphanumeric() || c == '_');
            if !identifier {
                return Err(Refusal::BadName(app));
            }
            let mut args = plain(&["manage.py", "migrate"]);
            if !app.is_empty() {
                args.push(app);
                if down {
                    args.push(down_to.ok_or(Refusal::NeedsTarget)?.to_owned());
                }
            } else if down {
                return Err(Refusal::NeedsTarget);
            }
            let env = vec![("DATABASE_URL".to_owned(), url(target, Form::Url, None))];
            Built { program: "python3", shown: args.clone(), args, env, secret: None }
        }
        Tool::Prisma => {
            no_password_in_url(target)?;
            let args = plain(&["migrate", "deploy"]);
            let env = vec![("DATABASE_URL".to_owned(), url(target, Form::Prisma, None))];
            Built { program: "prisma", shown: args.clone(), args, env, secret: None }
        }
        Tool::Rails => {
            no_password_in_url(target)?;
            let args = plain(&[if down { "db:rollback" } else { "db:migrate" }]);
            let env = vec![("DATABASE_URL".to_owned(), url(target, Form::Rails, None))];
            Built { program: "rails", shown: args.clone(), args, env, secret: None }
        }
        Tool::Knex => {
            no_password_in_url(target)?;
            let args = plain(&[if down { "migrate:down" } else { "migrate:latest" }]);
            let env = vec![("DATABASE_URL".to_owned(), url(target, Form::Url, None))];
            Built { program: "knex", shown: args.clone(), args, env, secret: None }
        }
    };
    let display = std::iter::once(built.program.to_owned()).chain(built.shown).collect::<Vec<_>>().join(" ");
    Ok(Invocation {
        program: built.program.to_owned(),
        args: built.args,
        env: built.env,
        secret: built.secret,
        display,
    })
}
