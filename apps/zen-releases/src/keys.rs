use std::fs;
use std::path::Path;

pub fn generate(path: &Path) -> Result<String, String> {
    if path.exists() {
        return Err(format!("{} already exists; refusing to overwrite a key", path.display()));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| format!("no random source: {error}"))?;
    let (secret, public) = zen_update::key_pair_from_seed(seed);
    write_private(path, &format!("{secret}\n"))?;
    Ok(public)
}

pub fn read_secret(path: &Path) -> Result<String, String> {
    let secret = fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    secret_from_text(&secret, &path.display().to_string())
}

pub fn secret_from_text(text: &str, source: &str) -> Result<String, String> {
    let secret = text.trim().to_owned();
    zen_update::public_key_of(&secret).map_err(|_| format!("{source}: not a secret key (one line of hex)"))?;
    Ok(secret)
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(contents.as_bytes()).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_from_a_variable_is_checked_and_never_shown() {
        let (secret, _) = zen_update::key_pair_from_seed([12; 32]);
        assert_eq!(secret_from_text(&format!("{secret}\n"), "ZEN_PUBLISHER_KEY").unwrap(), secret);
        let wrong = "z".repeat(64);
        let error = secret_from_text(&wrong, "ZEN_PUBLISHER_KEY").unwrap_err();
        assert!(error.starts_with("ZEN_PUBLISHER_KEY") && !error.contains(&wrong), "{error}");
    }
}
