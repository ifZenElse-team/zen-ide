use std::path::{Path, PathBuf};

const WEB2: &str = "/usr/share/dict/web2";

const OUTPUT: &str = "crates/platform/spell/data/web2.z";

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let source = args.first().map_or_else(|| PathBuf::from(WEB2), PathBuf::from);
    let text =
        std::fs::read_to_string(&source).map_err(|error| format!("cannot read {}: {error}", source.display()))?;
    let encoded = zen_spell::encode(text.lines());
    let words = zen_spell::decode(&encoded)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("no workspace root")?;
    let output = root.join(OUTPUT);
    std::fs::write(&output, &encoded).map_err(|error| format!("cannot write {}: {error}", output.display()))?;
    println!("{} words from {} into {} ({} bytes)", words.len(), source.display(), OUTPUT, encoded.len());
    Ok(())
}
