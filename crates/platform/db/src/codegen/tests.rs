use super::*;

fn column(name: &str, type_name: &str, nullable: bool) -> Column {
    Column { name: name.to_owned(), type_name: type_name.to_owned(), nullable, ..Column::default() }
}

fn users() -> Vec<Column> {
    vec![
        column("id", "bigint", false),
        column("user_name", "character varying(40)", false),
        Column {
            comment: Some("When the\nuser joined".to_owned()),
            ..column("created_at", "timestamp with time zone", true)
        },
        column("score", "numeric(10,2)", true),
        column("tags", "text[]", true),
        column("avatar", "bytea", true),
        column("settings", "jsonb", false),
        column("type", "integer", false),
    ]
}

#[test]
fn a_go_struct_has_tags_initialisms_and_pointers_for_nulls() {
    let generated = generate(CodeLanguage::Go, "postgresql", "users", Some("People who sign in"), &users());
    assert_eq!(generated.type_name, "User");
    assert_eq!(
        generated.code,
        "// User People who sign in\n\
         type User struct {\n\
         \tID        int64           `db:\"id\" json:\"id\"`\n\
         \tUserName  string          `db:\"user_name\" json:\"user_name\"`\n\
         \t// When the user joined\n\
         \tCreatedAt *time.Time      `db:\"created_at\" json:\"created_at\"`\n\
         \tScore     *string         `db:\"score\" json:\"score\"`\n\
         \tTags      []string        `db:\"tags\" json:\"tags\"`\n\
         \tAvatar    []byte          `db:\"avatar\" json:\"avatar\"`\n\
         \tSettings  json.RawMessage `db:\"settings\" json:\"settings\"`\n\
         \tType      int32           `db:\"type\" json:\"type\"`\n\
         }\n"
    );
}

#[test]
fn a_rust_struct_has_options_and_raw_identifiers() {
    let generated = generate(CodeLanguage::Rust, "postgresql", "users", None, &users());
    assert_eq!(
        generated.code,
        "#[derive(Debug, Clone, PartialEq)]\n\
         pub struct User {\n    \
             pub id: i64,\n    \
             pub user_name: String,\n    \
             /// When the user joined\n    \
             pub created_at: Option<String>,\n    \
             pub score: Option<String>,\n    \
             pub tags: Option<Vec<String>>,\n    \
             pub avatar: Option<Vec<u8>>,\n    \
             pub settings: String,\n    \
             pub r#type: i32,\n\
         }\n"
    );
}

#[test]
fn a_typescript_interface_keeps_column_names() {
    let mut columns = users();
    columns.push(column("first name", "text", false));
    let generated = generate(CodeLanguage::TypeScript, "postgresql", "user_accounts", None, &columns);
    assert_eq!(generated.type_name, "UserAccount");
    assert_eq!(
        generated.code,
        "export interface UserAccount {\n  \
             id: string;\n  \
             user_name: string;\n  \
             /** When the user joined */\n  \
             created_at: Date | null;\n  \
             score: string | null;\n  \
             tags: string[] | null;\n  \
             avatar: Uint8Array | null;\n  \
             settings: unknown;\n  \
             type: number;\n  \
             \"first name\": string;\n\
         }\n"
    );
}

#[test]
fn mysql_unsigned_and_tinyint_one_and_clickhouse_wrappers_are_mapped() {
    let columns = vec![
        column("id", "int unsigned", false),
        column("active", "tinyint(1)", false),
        column("big", "bigint(20) unsigned", true),
    ];
    let code = generate(CodeLanguage::Rust, "mysql", "flags", None, &columns).code;
    assert!(code.contains("pub id: u32,"), "{code}");
    assert!(code.contains("pub active: bool,"), "{code}");
    assert!(code.contains("pub big: Option<u64>,"), "{code}");
    let columns = vec![
        column("n", "Nullable(Int8)", false),
        column("names", "Array(LowCardinality(String))", false),
        column("at", "DateTime64(3)", false),
        column("huge", "UInt256", false),
    ];
    let code = generate(CodeLanguage::Go, "clickhouse", "events", None, &columns).code;
    assert!(code.contains("N     *int8"), "{code}");
    assert!(code.contains("Names []string"), "{code}");
    assert!(code.contains("At    time.Time"), "{code}");
    assert!(code.contains("Huge  string"), "{code}");
}

#[test]
fn sqlite_types_follow_affinity() {
    let columns = vec![
        column("id", "INTEGER", false),
        column("price", "REAL", true),
        column("data", "BLOB", true),
        column("name", "VARCHAR(10)", false),
        column("amount", "NUMERIC", false),
    ];
    let code = generate(CodeLanguage::TypeScript, "sqlite", "items", None, &columns).code;
    assert!(code.contains("id: string;"), "{code}");
    assert!(code.contains("price: number | null;"), "{code}");
    assert!(code.contains("data: Uint8Array | null;"), "{code}");
    assert!(code.contains("name: string;"), "{code}");
    assert!(code.contains("amount: string;"), "{code}");
}

#[test]
fn names_are_made_singular_and_unique() {
    assert_eq!(singular("categories"), "category");
    assert_eq!(singular("addresses"), "address");
    assert_eq!(singular("status"), "status");
    assert_eq!(singular("orders"), "order");
    assert_eq!(pascal("2fa_codes", false), "T2faCodes");
    assert_eq!(pascal("userURL", true), "UserURL");
    let columns = vec![column("user_id", "int", false), column("UserID", "int", false)];
    let code = generate(CodeLanguage::Go, "mysql", "x", None, &columns).code;
    assert!(code.contains("UserID  int32"), "{code}");
    assert!(code.contains("UserID2 int32"), "{code}");
}

#[test]
fn self_super_and_crate_columns_get_a_trailing_underscore_in_rust() {
    let columns = [column("self", "int4", false), column("super", "int4", false), column("crate", "int4", false)];
    let code = generate(CodeLanguage::Rust, "postgresql", "things", None, &columns).code;
    assert!(code.contains("pub self_: i32,") && code.contains("pub super_: i32,") && code.contains("pub crate_: i32,"));
    assert!(!code.contains("r#self"), "{code}");
}
