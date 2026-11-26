use serde_json::json;

use super::*;

#[test]
fn an_address_with_hosts_login_database_and_options_is_read() {
    let target = parse_url(
        "mongodb://ann%40corp:p%3Ass@a.local:27018,b.local,[::1]:27019/shop?replicaSet=rs0&authSource=users&authMechanism=SCRAM-SHA-1&tls=true&appName=Me",
    )
    .unwrap();
    assert_eq!(
        target.hosts,
        [("a.local".to_owned(), 27018), ("b.local".to_owned(), DEFAULT_PORT), ("::1".to_owned(), 27019)]
    );
    assert_eq!((target.user.as_deref(), target.password.as_str()), (Some("ann@corp"), "p:ss"));
    assert_eq!(target.database, "shop");
    assert_eq!(target.replica_set.as_deref(), Some("rs0"));
    assert_eq!(target.source(), "users");
    assert_eq!(target.mechanism, Some(Mechanism::Sha1));
    assert_eq!(target.tls["mode"], "verify-full");
    assert_eq!(target.application, "Me");
}

#[test]
fn an_srv_address_names_one_host_and_turns_tls_on() {
    let target = parse_url("mongodb+srv://cluster0.example.net/app?tlsAllowInvalidCertificates=true").unwrap();
    assert_eq!(target.srv.as_deref(), Some("cluster0.example.net"));
    assert!(target.hosts.is_empty());
    assert_eq!(target.tls["mode"], "require");
    assert!(parse_url("mongodb+srv://a.example.net:27017").is_err());
    assert!(parse_url("mongodb+srv://a.example.net,b.example.net").is_err());
    assert!(parse_url("postgres://x").is_err());
}

#[test]
fn connect_parameters_win_over_the_address() {
    let target = Target::of(&json!({
        "url": "mongodb://old:secret@db.local/app?tls=false", "user": "ann", "password": "new",
        "database": "shop", "tls": { "mode": "require" }, "settings": { "authSource": "admin", "directConnection": true }
    }))
    .unwrap();
    assert_eq!(target.hosts, [("db.local".to_owned(), DEFAULT_PORT)]);
    assert_eq!(
        (target.user.as_deref(), target.password.as_str(), target.database.as_str()),
        (Some("ann"), "new", "shop")
    );
    assert_eq!(target.tls["mode"], "require", "a mode chosen in the dialog wins");
    assert!(target.direct);
    let plain = Target::of(&json!({ "host": "h", "port": 1, "tls": { "mode": "prefer" } })).unwrap();
    assert_eq!((plain.hosts[0].1, plain.database.as_str(), plain.user.clone()), (1, "test", None));
    let from_url = Target::of(&json!({ "url": "mongodb://h/?tls=true", "tls": { "mode": "prefer" } })).unwrap();
    assert_eq!(from_url.tls["mode"], "verify-full", "the address's TLS applies when the dialog left the default");
}

#[test]
fn an_x509_login_needs_tls_with_a_client_certificate() {
    let params = |tls: serde_json::Value| json!({ "host": "db.local", "settings": { "authMechanism": "MONGODB-X509" }, "tls": tls });
    let target =
        Target::of(&params(json!({ "mode": "verify-full", "clientCert": "/c.pem", "clientKey": "/k.pem" }))).unwrap();
    assert!(target.x509 && target.mechanism.is_none());
    assert!(Target::of(&params(json!({ "mode": "disable", "clientCert": "/c.pem" }))).is_err());
    assert!(Target::of(&params(json!({ "mode": "require" }))).is_err());
    let address = parse_url("mongodb://h/?authMechanism=MONGODB-X509&tls=true&tlsCertificateKeyFile=/me.pem").unwrap();
    assert!(address.x509);
    assert_eq!(address.tls["clientCert"], "/me.pem");
}
