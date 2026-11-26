pub mod alter;
pub mod assist;
pub mod clip;
pub mod codegen;
pub mod compare;
pub mod detect;
pub mod diagram;
pub mod dialect;
pub mod driver;
pub mod edit;
pub mod explain;
pub mod export;
pub mod filter;
pub mod format;
pub mod geometry;
pub mod import;
pub mod inject;
pub mod migrations;
pub mod model;
pub mod params;
pub mod present;
pub mod profile;
pub mod registry;
pub mod relations;
pub mod safety;
pub mod schema;
pub mod search;
pub mod sessions;
pub mod sql;
pub mod types;
pub mod value;

#[cfg(not(target_arch = "wasm32"))]
pub mod backup;
#[cfg(not(target_arch = "wasm32"))]
pub mod clickhouse;
#[cfg(not(target_arch = "wasm32"))]
pub mod client;
#[cfg(not(target_arch = "wasm32"))]
pub mod csv_import;
#[cfg(not(target_arch = "wasm32"))]
pub mod engine;
#[cfg(not(target_arch = "wasm32"))]
pub mod external;
#[cfg(not(target_arch = "wasm32"))]
pub mod history;
#[cfg(not(target_arch = "wasm32"))]
pub mod journal;
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "fake")))]
pub mod memory;
#[cfg(not(target_arch = "wasm32"))]
pub mod mysql;
#[cfg(not(target_arch = "wasm32"))]
pub mod parquet_writer;
#[cfg(not(target_arch = "wasm32"))]
pub mod postgres;
#[cfg(not(target_arch = "wasm32"))]
pub mod recovery;
#[cfg(not(target_arch = "wasm32"))]
pub mod redis;
#[cfg(not(target_arch = "wasm32"))]
pub mod results;
#[cfg(not(target_arch = "wasm32"))]
pub mod sqlite;
#[cfg(not(target_arch = "wasm32"))]
pub mod ssh;
#[cfg(not(target_arch = "wasm32"))]
pub mod transfer;

pub type Waker = std::sync::Arc<dyn Fn() + Send + Sync>;

#[cfg(not(target_arch = "wasm32"))]
pub fn drivers() -> Vec<std::sync::Arc<dyn driver::Driver>> {
    vec![
        std::sync::Arc::new(postgres::Postgres),
        std::sync::Arc::new(mysql::MySql::for_mysql()),
        std::sync::Arc::new(mysql::MySql::for_mariadb()),
        std::sync::Arc::new(sqlite::Sqlite),
        std::sync::Arc::new(redis::Redis),
        std::sync::Arc::new(clickhouse::ClickHouse),
    ]
}
