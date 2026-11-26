use std::process::ExitCode;

fn main() -> ExitCode {
    zen_cli::set_language(std::env::var(zen_cli::LANGUAGE_ENV).ok().as_deref());
    let code = zen_cli::run(
        std::env::args_os(),
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
