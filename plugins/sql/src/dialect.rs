#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dialect {
    #[default]
    Generic,
    Postgres,
    MySql,
    Sqlite,
    ClickHouse,
    SqlServer,
}

impl Dialect {
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" => Dialect::Postgres,
            "mysql" | "mariadb" => Dialect::MySql,
            "sqlite" => Dialect::Sqlite,
            "clickhouse" => Dialect::ClickHouse,
            "sqlserver" | "mssql" => Dialect::SqlServer,
            _ => Dialect::Generic,
        }
    }

    pub fn bit(self) -> u8 {
        match self {
            Dialect::Generic => ANY,
            Dialect::Postgres => PG,
            Dialect::MySql => MY,
            Dialect::Sqlite => LITE,
            Dialect::ClickHouse => CH,
            Dialect::SqlServer => MS,
        }
    }

    pub fn hash_comments(self) -> bool {
        matches!(self, Dialect::Generic | Dialect::MySql | Dialect::ClickHouse)
    }

    pub fn backslashes(self) -> bool {
        matches!(self, Dialect::MySql | Dialect::ClickHouse)
    }

    pub fn postgres_strings(self) -> bool {
        matches!(self, Dialect::Generic | Dialect::Postgres)
    }

    pub fn backticks(self) -> bool {
        !matches!(self, Dialect::Postgres | Dialect::SqlServer)
    }

    pub fn brackets(self) -> bool {
        matches!(self, Dialect::Sqlite | Dialect::SqlServer)
    }

    pub fn nested_comments(self) -> bool {
        matches!(self, Dialect::Postgres | Dialect::SqlServer)
    }

    pub fn delimiter_lines(self) -> bool {
        matches!(self, Dialect::Generic | Dialect::MySql)
    }

    pub fn go_lines(self) -> bool {
        matches!(self, Dialect::Generic | Dialect::SqlServer)
    }

    pub fn backslash_lines(self) -> bool {
        matches!(self, Dialect::Generic | Dialect::Postgres)
    }

    pub fn refuses_trailing_commas(self) -> bool {
        !matches!(self, Dialect::ClickHouse)
    }
}

pub const PG: u8 = 1;
pub const MY: u8 = 2;
pub const LITE: u8 = 4;
pub const CH: u8 = 8;
pub const MS: u8 = 16;
pub const ANY: u8 = PG | MY | LITE | CH | MS;
