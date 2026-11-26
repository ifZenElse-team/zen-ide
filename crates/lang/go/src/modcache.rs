use std::path::{Path, PathBuf};

pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_uppercase() {
            escaped.push('!');
            escaped.push(c.to_ascii_lowercase());
        } else {
            escaped.push(c);
        }
    }
    escaped
}

pub fn module_dir(cache: &Path, path: &str, version: &str) -> PathBuf {
    let escaped = escape(path);
    let mut dir = cache.to_owned();
    let mut parts = escaped.split('/').filter(|part| !part.is_empty()).peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_some() {
            dir.push(part);
        } else {
            dir.push(format!("{part}@{}", escape(version)));
        }
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitals_are_escaped() {
        assert_eq!(escape("github.com/BurntSushi/toml"), "github.com/!burnt!sushi/toml");
        assert_eq!(escape("v1.0.0-RC1"), "v1.0.0-!r!c1");
        assert_eq!(escape("golang.org/x/text"), "golang.org/x/text");
    }

    #[test]
    fn module_folders_are_in_the_cache() {
        let cache = Path::new("/cache");
        assert_eq!(
            module_dir(cache, "github.com/BurntSushi/toml", "v1.4.0"),
            Path::new("/cache/github.com/!burnt!sushi/toml@v1.4.0")
        );
        assert_eq!(module_dir(cache, "rsc.io/Q", "v1.0.0-Beta"), Path::new("/cache/rsc.io/!q@v1.0.0-!beta"));
    }
}
