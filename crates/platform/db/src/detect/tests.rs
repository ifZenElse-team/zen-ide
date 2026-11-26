use std::collections::HashMap;

use super::yaml::{Yaml, parse};
use super::*;

#[test]
fn env_files_are_read_as_dotenv_reads_them() {
    let pairs = parse_env(
        "# comment\nexport A=1\nB = \"two words\" \nC='${A} kept'\nD=${A}-x # note\nE=\"multi\nline\"\nbad key=1\nF=\n",
    );
    let get = |key: &str| pairs.iter().find(|(known, _)| known == key).map(|(_, value)| value.as_str());
    assert_eq!(get("A"), Some("1"));
    assert_eq!(get("B"), Some("two words"));
    assert_eq!(get("C"), Some("${A} kept"));
    assert_eq!(get("D"), Some("1-x"));
    assert_eq!(get("E"), Some("multi\nline"));
    assert_eq!(get("F"), Some(""));
    assert!(get("bad key").is_none());
}

#[test]
fn urls_and_groups_of_env_files_are_found_and_passwords_kept_apart() {
    let text = "DATABASE_URL=postgres://app:s3cr%40t@db.local:5433/shop?sslmode=require\n\
                ANALYTICS_DATABASE_URL=mysql://root@127.0.0.1/stats\n\
                REDIS_URL=rediss://:pw@cache:6380/1\n\
                MONGO_URL=mongodb+srv://ann@cluster0.example.net/app\n\
                CACHE_URL=memcached://x\n\
                POSTGRES_USER=pg\nPOSTGRES_PASSWORD=pgpw\nPOSTGRES_DB=main\n\
                MYSQL_ROOT_PASSWORD=rootpw\nMYSQL_DATABASE=legacy\nMYSQL_PORT=3307\n";
    let found = from_env(".env", text);
    let keys: Vec<&str> = found.iter().map(|finding| finding.key.as_str()).collect();
    assert_eq!(keys, ["DATABASE_URL", "ANALYTICS_DATABASE_URL", "REDIS_URL", "MONGO_URL", "POSTGRES_*", "MYSQL_*"]);
    let url = &found[0];
    assert_eq!((url.driver.as_str(), url.host.as_deref(), url.port), ("postgresql", Some("db.local"), Some(5433)));
    assert_eq!((url.database.as_deref(), url.user.as_deref()), (Some("shop"), Some("app")));
    assert_eq!(url.password.as_ref().map(|secret| secret.expose().to_owned()).as_deref(), Some("s3cr@t"));
    assert!(!url.url.as_deref().unwrap().contains("s3cr"), "{:?}", url.url);
    assert_eq!(found[1].product, "MySQL");
    assert!(found[1].password.is_none());
    assert_eq!((found[2].driver.as_str(), found[2].product.as_str()), ("redis", "Redis"));
    assert_eq!(found[3].driver, "mongodb");
    let group = &found[4];
    assert_eq!(
        (group.host.as_deref(), group.database.as_deref(), group.user.as_deref()),
        (Some("localhost"), Some("main"), Some("pg"))
    );
    assert!(group.password.is_some());
    let mysql = &found[5];
    assert_eq!((mysql.user.as_deref(), mysql.port), (Some("root"), Some(3307)));
    assert_eq!(found[0].id, from_env(".env", text)[0].id, "ids stay the same");
    assert_ne!(found[0].id, from_env("api/.env", text)[0].id);
    let changed = from_env(".env", "DATABASE_URL=postgres://app:other@db.local:5433/shop2");
    assert_eq!(changed[0].id, found[0].id);
    assert_ne!(changed[0].fingerprint(), found[0].fingerprint(), "another database is offered again");
    let new_password = from_env(".env", "DATABASE_URL=postgres://app:changed@db.local:5433/shop?sslmode=require");
    assert_eq!(new_password[0].fingerprint(), found[0].fingerprint(), "the password is not in the fingerprint");
}

#[test]
fn the_yaml_of_compose_files_is_read() {
    let text = r#"
version: "3.9"
x-common: &common
  restart: always
services:
  db:
    <<: *common
    image: "postgres:16-alpine"   # the database
    ports:
      - "127.0.0.1:5433:5432"
      - 8080
    environment:
      POSTGRES_PASSWORD: example
      POSTGRES_DB: shop
    command: >
      postgres -c log_statement=all
  cache:
    image: redis:7
    ports: ["6380:6379"]
    environment: [REDIS_PASSWORD=pw, OTHER=1]
  web:
    image: nginx
    ports:
    - target: 80
      published: 8081
"#;
    let root = parse(text);
    let services = root.get("services").unwrap();
    let db = services.get("db").unwrap();
    assert_eq!(db.get("image").and_then(Yaml::scalar), Some("postgres:16-alpine"));
    assert_eq!(db.get("ports").unwrap().items().len(), 2);
    assert_eq!(db.get("environment").unwrap().get("POSTGRES_DB").and_then(Yaml::scalar), Some("shop"));
    assert_eq!(db.get("command").and_then(Yaml::scalar), Some("postgres -c log_statement=all"));
    let cache = services.get("cache").unwrap();
    assert_eq!(cache.get("ports").unwrap().items(), [Yaml::Scalar("6380:6379".to_owned())]);
    assert_eq!(cache.get("environment").unwrap().items().len(), 2);
    let web = services.get("web").unwrap();
    assert_eq!(web.get("ports").unwrap().items()[0].get("published").and_then(Yaml::scalar), Some("8081"));
    assert_eq!(parse(""), Yaml::Null);
    assert_eq!(parse("{a: 1, b: [x, 'y']}").get("b").unwrap().items().len(), 2);
}

#[test]
fn database_services_of_compose_files_are_found_with_their_ports_and_credentials() {
    let text = r#"
services:
  db:
    image: postgres:16
    ports: ["${PG_PORT:-5433}:5432"]
    environment:
      POSTGRES_USER: ${DB_USER}
      POSTGRES_PASSWORD: ${DB_PASSWORD}
  mysql:
    image: docker.io/library/mariadb:11
    ports:
      - target: 3306
        published: "3307"
        host_ip: 127.0.0.1
    environment:
      - MARIADB_ROOT_PASSWORD=root
      - MARIADB_DATABASE=app
  cache:
    image: valkey/valkey:8
    expose: ["6379"]
  search:
    image: docker.elastic.co/elasticsearch/elasticsearch:8.15.0
    ports: ["9200:9200"]
    environment:
      ELASTIC_PASSWORD: elastic-pw
  web:
    image: nginx:latest
    ports: ["80:80"]
"#;
    let vars: HashMap<String, String> =
        [("DB_USER", "app"), ("DB_PASSWORD", "from-env")].map(|(k, v)| (k.to_owned(), v.to_owned())).into();
    let found = from_compose("docker-compose.yml", text, &vars, Some(".env"));
    let services: Vec<&str> = found.iter().map(|finding| finding.key.as_str()).collect();
    assert_eq!(services, ["db", "mysql", "cache", "search"]);
    let db = &found[0];
    assert_eq!((db.host.as_deref(), db.port, db.published), (Some("localhost"), Some(5433), true));
    assert_eq!((db.user.as_deref(), db.database.as_deref()), (Some("app"), Some("app")));
    assert_eq!(db.password.as_ref().unwrap().expose(), "from-env");
    assert_eq!(db.files, ["docker-compose.yml", ".env"]);
    let mysql = &found[1];
    assert_eq!((mysql.driver.as_str(), mysql.product.as_str()), ("mariadb", "MariaDB"));
    assert_eq!((mysql.host.as_deref(), mysql.port), (Some("127.0.0.1"), Some(3307)));
    assert_eq!((mysql.user.as_deref(), mysql.database.as_deref()), (Some("root"), Some("app")));
    let cache = &found[2];
    assert_eq!((cache.product.as_str(), cache.port, cache.published), ("Valkey", None, false));
    assert_eq!(found[3].user.as_deref(), Some("elastic"));
    assert!(from_compose("compose.yaml", "services: {}", &HashMap::new(), None).is_empty());
    assert!(from_compose("compose.yaml", ":: not yaml [", &HashMap::new(), None).is_empty());
}

#[test]
fn candidate_files_are_named_as_compose_and_dotenv_name_them() {
    for name in [".env", ".env.local", ".env.example", "docker-compose.yml", "compose.yaml", "docker-compose.dev.yaml"]
    {
        assert!(is_candidate(name), "{name}");
    }
    for name in [".envrc", "env", ".env.", "compose.json", "my-compose.yml", "docker-compose"] {
        assert!(!is_candidate(name), "{name}");
    }
    assert!(env_rank(".env") < env_rank(".env.local") && env_rank(".env.local") < env_rank(".env.example"));
    let vars: HashMap<String, String> = [("A".to_owned(), "1".to_owned()), ("E".to_owned(), String::new())].into();
    assert_eq!(interpolate("$A ${A} ${B:-2} ${E:-3} ${E-4} $$ ${A:+on} ${B:+on}", &vars), "1 1 2 3  $ on ");
}
