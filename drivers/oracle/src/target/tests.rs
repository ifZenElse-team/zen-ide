use serde_json::json;

use super::*;

fn program() -> String {
    std::env::current_exe().unwrap().to_string_lossy().into_owned()
}

fn target(params: Json) -> Result<Target, Failure> {
    let mut params = params;
    params["settings"]["program"] = json!(program());
    Target::of(&params)
}

#[test]
fn a_service_name_is_an_easy_connect_and_a_sid_the_host_port_sid_form() {
    let service =
        target(json!({ "host": "db", "port": 1522, "database": "ORCLPDB1", "user": "hr", "password": "p@s/s" }))
            .unwrap();
    assert_eq!(service.connect_lines(), "CONNECT hr/\"p@s/s\"@//db:1522/ORCLPDB1\n");
    let sid = target(json!({ "host": "::1", "database": "XE", "user": "hr", "settings": { "connectionType": "sid" } }))
        .unwrap();
    assert_eq!(sid.identifier, "[::1]:1521:XE");
    let defaults = target(json!({ "user": "system" })).unwrap();
    assert_eq!(defaults.identifier, "//localhost:1521/FREEPDB1");
}

#[test]
fn tls_connects_over_tcps_and_checks_the_host_only_when_asked() {
    let full = target(json!({
        "host": "db", "port": 2484, "database": "svc", "user": "hr", "tls": { "mode": "verify-full" },
        "settings": { "wallet": "/w" }
    }))
    .unwrap();
    assert_eq!(
        full.identifier,
        "(DESCRIPTION=(ADDRESS=(PROTOCOL=TCPS)(HOST=db)(PORT=2484))(CONNECT_DATA=(SERVICE_NAME=svc))(SECURITY=(SSL_SERVER_DN_MATCH=ON)(MY_WALLET_DIRECTORY=/w)))"
    );
    let sid = target(json!({ "database": "XE", "user": "hr", "tls": { "mode": "require" }, "settings": { "connectionType": "sid" } }))
        .unwrap();
    assert!(sid.identifier.contains("(SID=XE)") && sid.identifier.contains("DN_MATCH=OFF"), "{}", sid.identifier);
    let prefer = target(json!({ "database": "svc", "user": "hr", "tls": { "mode": "prefer" } })).unwrap();
    assert_eq!(prefer.identifier, "//localhost:1521/svc");
}

#[test]
fn aliases_connect_strings_roles_and_cloud_wallets() {
    let alias = target(json!({ "database": "shop_high", "user": "admin", "settings": {
        "connectionType": "tns", "tnsAdmin": "/etc/tns", "role": "SYSDBA", "wallet": "/w/Wallet_shop.zip" } }))
    .unwrap();
    assert_eq!(alias.tns_admin, Some(PathBuf::from("/etc/tns")));
    assert_eq!(
        alias.connect_lines(),
        "SET CLOUDCONFIG \"/w/Wallet_shop.zip\"\nCONNECT admin/\"\"@shop_high AS SYSDBA\n"
    );
    let custom = target(json!({ "user": "Mixed Case", "settings": {
        "connectionType": "custom", "connectString": "tcps://db:2484/svc?ssl_server_dn_match=true" } }))
    .unwrap();
    assert_eq!(custom.user, "\"Mixed Case\"");
    assert_eq!(custom.identifier, "tcps://db:2484/svc?ssl_server_dn_match=true");
}

#[test]
fn nothing_can_add_a_line_or_leave_the_identifier() {
    let refused = |params: Json| target(params).unwrap_err();
    assert_eq!(refused(json!({ "user": "hr", "password": "a\"b" })).kind(), Some("authentication"));
    assert!(refused(json!({ "user": "hr", "database": "svc\nHOST rm" })).message.contains("service name"));
    assert!(refused(json!({ "user": "hr", "host": "db (x)" })).message.contains("host"));
    assert!(
        refused(json!({ "user": "hr", "settings": { "connectionType": "custom", "connectString": "a\nb" } }))
            .message
            .contains("line break")
    );
    assert!(
        refused(json!({ "user": "hr", "settings": { "connectionType": "custom", "connectString": "db / x" } }))
            .message
            .contains("spaces")
    );
    assert!(refused(json!({ "user": "hr", "settings": { "role": "ROOT" } })).message.contains("role"));
    assert_eq!(refused(json!({ "database": "svc" })).kind(), Some("authentication"));
}

#[test]
fn the_password_is_taken_out_of_what_is_shown() {
    let found = target(json!({ "user": "hr", "password": "s3cret" })).unwrap();
    assert_eq!(found.redact("bad s3cret here"), "bad ******** here");
}
