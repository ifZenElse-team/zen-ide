const PROGRAM: &str = env!("CARGO_BIN_EXE_rustronin");

#[cfg(unix)]
#[test]
fn a_second_start_hands_a_file_at_a_line_and_a_folder_to_the_running_ide() {
    zen_testkit::handoff::a_second_start_hands_a_file_at_a_line_and_a_folder_to_the_running_ide(
        &zen_testkit::handoff::Case {
            program: PROGRAM,
            id: "rustronin",
            file: ("main.rs", "fn main() {\n    println!(\"hi\");\n}\n"),
        },
    );
}
