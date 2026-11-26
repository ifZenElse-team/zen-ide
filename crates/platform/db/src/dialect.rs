use crate::schema::quote_for;
use crate::value::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SqlDialect {
    #[default]
    Postgres,
    MySql,
    MariaDb,
    Sqlite,
    ClickHouse,
    SqlServer,
    Oracle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placeholders {
    Numbered,
    Question,
    Named,
    Inline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lexing {
    pub backticks: bool,
    pub brackets: bool,
    pub double_quoted_strings: bool,
    pub backslashes: bool,
    pub dollar_quotes: bool,
    pub hash_comments: bool,
}

pub fn table_id(schema: &str, name: &str) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(schema.as_bytes());
    hasher.update(&[0]);
    hasher.update(name.as_bytes());
    hasher.finalize().max(1)
}

fn doubled(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn float_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity".to_owned() } else { "-Infinity".to_owned() }
    } else {
        value.to_string()
    }
}

impl SqlDialect {
    pub fn of_language(language: &str) -> Option<Self> {
        match language {
            "sql:postgres" => Some(SqlDialect::Postgres),
            "sql:mysql" => Some(SqlDialect::MySql),
            "sql:mariadb" => Some(SqlDialect::MariaDb),
            "sql:sqlite" => Some(SqlDialect::Sqlite),
            "sql:clickhouse" => Some(SqlDialect::ClickHouse),
            "sql:sqlserver" => Some(SqlDialect::SqlServer),
            "sql:oracle" => Some(SqlDialect::Oracle),
            _ => None,
        }
    }

    pub fn driver(self) -> &'static str {
        match self {
            SqlDialect::Postgres => "postgresql",
            SqlDialect::MySql => "mysql",
            SqlDialect::MariaDb => "mariadb",
            SqlDialect::Sqlite => "sqlite",
            SqlDialect::ClickHouse => "clickhouse",
            SqlDialect::SqlServer => "sqlserver",
            SqlDialect::Oracle => "oracle",
        }
    }

    fn mysql(self) -> bool {
        matches!(self, SqlDialect::MySql | SqlDialect::MariaDb)
    }

    pub fn quote(self, name: &str) -> String {
        quote_for(self.driver(), name)
    }

    pub fn qualified(self, schema: &str, name: &str) -> String {
        format!("{}.{}", self.quote(schema), self.quote(name))
    }

    pub fn string(self, text: &str) -> String {
        match self {
            SqlDialect::Postgres | SqlDialect::Sqlite | SqlDialect::Oracle => doubled(text),
            SqlDialect::MySql | SqlDialect::MariaDb if text.contains(['\\', '\0']) => {
                format!("_utf8mb4 X'{}'", crate::value::hex(text.as_bytes()))
            }
            SqlDialect::MySql | SqlDialect::MariaDb => doubled(text),
            SqlDialect::ClickHouse => format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'")),
            SqlDialect::SqlServer => format!("N{}", doubled(text)),
        }
    }

    pub fn param(self, value: &Value) -> Option<String> {
        match value {
            Value::Bool(value) if self != SqlDialect::Postgres => Some(if *value { "1" } else { "0" }.to_owned()),
            Value::Bytes(bytes) if self != SqlDialect::Postgres => Some(crate::value::hex(bytes)),
            value => crate::edit::param_text(value),
        }
    }

    pub fn literal(self, value: &Value) -> String {
        match value {
            Value::Null => "NULL".to_owned(),
            Value::Bool(value) if matches!(self, SqlDialect::SqlServer | SqlDialect::Oracle) => {
                if *value { "1" } else { "0" }.to_owned()
            }
            Value::Bool(value) => if *value { "TRUE" } else { "FALSE" }.to_owned(),
            Value::Integer(value) => value.to_string(),
            Value::Float(value) if value.is_finite() => float_text(*value),
            Value::Decimal(text) if text.parse::<f64>().is_ok_and(f64::is_finite) => text.clone(),
            Value::Bytes(bytes) => match self {
                SqlDialect::Postgres => doubled(&format!("\\x{}", crate::value::hex(bytes))),
                SqlDialect::MySql | SqlDialect::MariaDb | SqlDialect::Sqlite => {
                    format!("X'{}'", crate::value::hex(bytes))
                }
                SqlDialect::ClickHouse => format!("unhex('{}')", crate::value::hex(bytes)),
                SqlDialect::SqlServer => format!("0x{}", crate::value::hex(bytes)),
                SqlDialect::Oracle => format!("HEXTORAW('{}')", crate::value::hex(bytes)),
            },
            other => self.string(&crate::edit::param_text(other).unwrap_or_default()),
        }
    }

    pub fn placeholder(self, number: usize, value: &Value) -> String {
        match self.placeholders() {
            Placeholders::Numbered => format!("${number}"),
            Placeholders::Named if matches!(value, Value::Bytes(_)) => {
                format!("CONVERT(VARBINARY(MAX), @P{number}, 2)")
            }
            Placeholders::Named => format!("@P{number}"),
            Placeholders::Question if matches!(value, Value::Bytes(_)) => "UNHEX(?)".to_owned(),
            Placeholders::Question => "?".to_owned(),
            Placeholders::Inline => self.literal(value),
        }
    }

    pub fn inlined(self, value: &Value) -> bool {
        self.mysql() && matches!(value, Value::Bool(_))
    }

    pub fn text(self, expression: &str) -> String {
        match self {
            SqlDialect::Postgres => format!("{expression}::text"),
            SqlDialect::MySql | SqlDialect::MariaDb => format!("CAST({expression} AS CHAR)"),
            SqlDialect::Sqlite => format!("CAST({expression} AS TEXT)"),
            SqlDialect::ClickHouse => format!("toString({expression})"),
            SqlDialect::SqlServer => format!("CAST({expression} AS NVARCHAR(MAX))"),
            SqlDialect::Oracle => format!("TO_CHAR({expression})"),
        }
    }

    pub fn contains(self, expression: &str, text: &str) -> String {
        self.like(&self.text(expression), text)
    }

    pub fn like(self, column: &str, text: &str) -> String {
        let (pattern, escape) = self.like_pattern(text);
        format!("{column} LIKE {pattern}{escape}")
    }

    pub fn like_pattern(self, text: &str) -> (String, &'static str) {
        if self == SqlDialect::ClickHouse {
            let escaped = text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            return (self.string(&format!("%{escaped}%")), "");
        }
        let mut escaped = text.replace('!', "!!").replace('%', "!%").replace('_', "!_");
        if self == SqlDialect::SqlServer {
            escaped = escaped.replace('[', "![");
        }
        (self.string(&format!("%{escaped}%")), " ESCAPE '!'")
    }

    pub fn order(self, expression: &str, descending: bool) -> String {
        let position = expression.chars().all(|c| c.is_ascii_digit());
        match self {
            SqlDialect::Postgres => {
                if descending {
                    format!("{expression} DESC")
                } else {
                    expression.to_owned()
                }
            }
            SqlDialect::MySql | SqlDialect::MariaDb | SqlDialect::SqlServer if position => {
                if descending {
                    format!("{expression} DESC")
                } else {
                    expression.to_owned()
                }
            }
            SqlDialect::MySql | SqlDialect::MariaDb => {
                if descending {
                    format!("{expression} IS NULL DESC, {expression} DESC")
                } else {
                    format!("{expression} IS NULL, {expression}")
                }
            }
            SqlDialect::SqlServer => {
                if descending {
                    format!("CASE WHEN {expression} IS NULL THEN 0 ELSE 1 END, {expression} DESC")
                } else {
                    format!("CASE WHEN {expression} IS NULL THEN 1 ELSE 0 END, {expression}")
                }
            }
            SqlDialect::Sqlite | SqlDialect::ClickHouse | SqlDialect::Oracle => {
                if descending {
                    format!("{expression} DESC NULLS FIRST")
                } else {
                    format!("{expression} NULLS LAST")
                }
            }
        }
    }

    pub fn not_distinct(self, left: &str, right: &str) -> String {
        match self {
            SqlDialect::Postgres => format!("{left} IS NOT DISTINCT FROM {right}"),
            SqlDialect::MySql | SqlDialect::MariaDb => format!("{left} <=> {right}"),
            SqlDialect::Sqlite => format!("{left} IS {right}"),
            SqlDialect::ClickHouse => format!("isNotDistinctFrom({left}, {right})"),
            SqlDialect::SqlServer => format!("({left} = {right} OR ({left} IS NULL AND {right} IS NULL))"),
            SqlDialect::Oracle => format!("DECODE({left}, {right}, 1, 0) = 1"),
        }
    }

    pub fn limit(self, rows: usize) -> String {
        if self == SqlDialect::Oracle { format!(" FETCH FIRST {rows} ROWS ONLY") } else { format!(" LIMIT {rows}") }
    }

    pub fn alias(self, name: &str) -> String {
        if self == SqlDialect::Oracle { format!(" {name}") } else { format!(" AS {name}") }
    }

    pub fn edits(self) -> bool {
        self != SqlDialect::ClickHouse
    }

    pub fn placeholders(self) -> Placeholders {
        match self {
            SqlDialect::Postgres => Placeholders::Numbered,
            SqlDialect::MySql | SqlDialect::MariaDb => Placeholders::Question,
            SqlDialect::Sqlite | SqlDialect::ClickHouse | SqlDialect::Oracle => Placeholders::Inline,
            SqlDialect::SqlServer => Placeholders::Named,
        }
    }

    pub fn returning(self) -> bool {
        matches!(self, SqlDialect::Postgres | SqlDialect::Sqlite)
    }

    pub fn default_keyword(self) -> bool {
        self != SqlDialect::Sqlite
    }

    pub fn insert_defaults(self, table: &str) -> String {
        if self.mysql() {
            format!("INSERT INTO {table} () VALUES ()")
        } else {
            format!("INSERT INTO {table} DEFAULT VALUES")
        }
    }

    pub fn row_id(self) -> bool {
        self == SqlDialect::Postgres
    }

    pub fn last_insert_id(self) -> Option<&'static str> {
        self.mysql().then_some("LAST_INSERT_ID()")
    }

    pub fn floats_compare(self) -> bool {
        !self.mysql()
    }

    pub fn lexing(self) -> Lexing {
        Lexing {
            backticks: !matches!(self, SqlDialect::Postgres | SqlDialect::SqlServer | SqlDialect::Oracle),
            brackets: matches!(self, SqlDialect::Sqlite | SqlDialect::SqlServer),
            double_quoted_strings: self.mysql(),
            backslashes: matches!(self, SqlDialect::MySql | SqlDialect::MariaDb | SqlDialect::ClickHouse),
            dollar_quotes: self == SqlDialect::Postgres,
            hash_comments: self.mysql(),
        }
    }
}

#[cfg(test)]
mod tests;
