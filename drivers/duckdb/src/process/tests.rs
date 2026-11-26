use super::*;

fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|line| (*line).to_owned()).collect()
}

#[test]
fn errors_keep_their_message_and_point_at_the_place() {
    let failure = error(
        &lines(&[
            "Catalog Error: Table with name nope does not exist!",
            "Did you mean \"pg_enum\"?",
            "",
            "LINE 2: SELECT * FROM nope",
            "                      ^",
        ]),
        "-- first\nSELECT * FROM nope",
    );
    assert_eq!(failure.message, "Catalog Error: Table with name nope does not exist! Did you mean \"pg_enum\"?");
    assert_eq!(failure.data.unwrap()["position"], 24);
    let plain = error(&lines(&["Parser Error: syntax error at end of input"]), "SELECT 1 +");
    assert!(plain.data.unwrap().get("position").is_none());
}

#[test]
fn the_markers_noise_on_standard_error_is_left_out() {
    assert!(noise("Did you mean: \".open\""));
    assert!(noise("Run '.help' for more information."));
    assert!(noise(""));
    assert!(!noise("Did you mean \"pg_enum\"?"));
}

#[test]
fn a_description_names_the_columns_and_their_types() {
    let columns = described(&lines(&[
        r#"{"column_name":"id","column_type":"INTEGER","null":"NO"}"#,
        r#"{"column_name":"tags","column_type":"VARCHAR[]","null":"YES"}"#,
    ]));
    assert_eq!(columns[1], Column { name: "tags".into(), type_name: Some("VARCHAR[]".into()) });
}

#[test]
fn a_file_is_never_read_as_an_option_of_the_program() {
    assert_eq!(file_argument(""), ":memory:");
    assert_eq!(file_argument(":memory:"), ":memory:");
    assert_eq!(file_argument("/data/lake.duckdb"), "/data/lake.duckdb");
    assert_eq!(file_argument("lake.duckdb"), "lake.duckdb");
    assert_eq!(file_argument("-unsigned"), "./-unsigned");
    assert_eq!(file_argument("-cmd"), "./-cmd");
}

#[test]
fn the_version_is_read_from_what_duckdb_prints() {
    assert_eq!(parse_version("v2.0.0 (Cyanoptera) d54cbbe3df"), Some((2, 0)));
    assert_eq!(parse_version("v1.1.3 19864453f7\n"), Some((1, 1)));
    assert_eq!(parse_version("v0.10.0 20b1486d11"), Some((0, 10)));
    assert_eq!(parse_version("3.45.1 2024-01-30"), None);
    assert_eq!(parse_version(""), None);
}

#[cfg(unix)]
#[test]
fn only_absolute_folders_of_the_path_are_searched() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!("zen-duckdb-find-{}", std::process::id()));
    let relative = base.join("rel");
    let absolute = base.join("abs");
    for folder in [&relative, &absolute] {
        std::fs::create_dir_all(folder).unwrap();
        let program = folder.join("duckdb");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let up = std::env::current_dir().unwrap().components().count().saturating_sub(1);
    let mut from_here = PathBuf::new();
    for _ in 0..up {
        from_here.push("..");
    }
    from_here.push(relative.strip_prefix("/").unwrap());
    assert!(from_here.join("duckdb").is_file());
    let only_relative = find_in(None, Some(from_here.clone().into_os_string()), Some("home".into()));
    assert!(
        only_relative.as_ref().is_ok_and(|found| found.is_absolute()) || only_relative.is_err(),
        "{only_relative:?}"
    );
    let paths = std::env::join_paths([from_here, absolute.clone()]).unwrap();
    assert_eq!(find_in(None, Some(paths), None).unwrap(), absolute.join("duckdb"));
    let _ = std::fs::remove_dir_all(base);
}
