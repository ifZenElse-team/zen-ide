#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use serde_json::Value;

use super::env::{Proxy, ProxyStep};
use crate::compare_versions;

pub trait Fetch: Send + Sync {
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Http;

impl Fetch for Http {
    #[cfg(not(target_arch = "wasm32"))]
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>, String> {
        zen_update::fetch::document(url, 32 << 20, Duration::from_secs(20)).map_err(|error| error.to_string())
    }

    #[cfg(target_arch = "wasm32")]
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>, String> {
        Err(format!("no network for {url}"))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Versions {
    pub list: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
}

impl Versions {
    pub fn newer_than(&self, current: &str) -> Option<String> {
        self.newer_than_skipping(current, &|_| false)
    }

    pub fn newer_than_skipping(&self, current: &str, skip: &dyn Fn(&str) -> bool) -> Option<String> {
        let incompatible = current.ends_with("+incompatible");
        let newest = self
            .list
            .iter()
            .filter(|version| crate::provider::is_release(version))
            .filter(|version| incompatible || !version.ends_with("+incompatible"))
            .filter(|version| !skip(version))
            .max_by(|a, b| compare_versions(a, b))
            .or(self.latest.as_ref().filter(|latest| !skip(latest)))?;
        (compare_versions(newest, current).is_gt()).then(|| newest.clone())
    }

    pub fn latest(&self) -> Option<String> {
        let newest = |releases: bool, compatible: bool| {
            self.list
                .iter()
                .filter(|version| !releases || crate::provider::is_release(version))
                .filter(|version| !compatible || !version.ends_with("+incompatible"))
                .max_by(|a, b| compare_versions(a, b))
                .cloned()
        };
        newest(true, true)
            .or_else(|| newest(false, true))
            .or_else(|| newest(true, false))
            .or_else(|| self.latest.clone())
    }

    pub fn release_at_or_after(&self, version: &str) -> Option<String> {
        self.release_at_or_after_skipping(version, &|_| false)
    }

    pub fn release_at_or_after_skipping(&self, version: &str, skip: &dyn Fn(&str) -> bool) -> Option<String> {
        self.list
            .iter()
            .filter(|listed| crate::provider::is_release(listed) && compare_versions(listed, version).is_ge())
            .filter(|listed| !skip(listed))
            .min_by(|a, b| compare_versions(a, b))
            .cloned()
    }
}

pub fn escape(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_ascii_uppercase() {
            escaped.push('!');
            escaped.push(c.to_ascii_lowercase());
        } else {
            escaped.push(c);
        }
    }
    escaped
}

pub fn versions(fetch: &dyn Fetch, steps: &[ProxyStep], module: &str) -> Result<Option<Versions>, String> {
    for step in steps {
        let base = match &step.proxy {
            Proxy::Url(base) => base,
            Proxy::Direct | Proxy::Off => return Ok(None),
        };
        let module_url = format!("{base}/{}", escape(module));
        match ask(fetch, &module_url) {
            Ok(Some(versions)) => return Ok(Some(versions)),
            Ok(None) => continue,
            Err(_) if step.on_any_error => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub fn go_mod(fetch: &dyn Fetch, steps: &[ProxyStep], module: &str, version: &str) -> Result<Option<Vec<u8>>, String> {
    for step in steps {
        let base = match &step.proxy {
            Proxy::Url(base) => base,
            Proxy::Direct | Proxy::Off => return Ok(None),
        };
        match fetch.get(&format!("{base}/{}/@v/{}.mod", escape(module), escape(version))) {
            Ok(Some(text)) => return Ok(Some(text)),
            Ok(None) => continue,
            Err(_) if step.on_any_error => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn ask(fetch: &dyn Fetch, module_url: &str) -> Result<Option<Versions>, String> {
    let Some(list) = fetch.get(&format!("{module_url}/@v/list"))? else { return Ok(None) };
    let list: Vec<String> = String::from_utf8_lossy(&list)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    if !list.is_empty() {
        return Ok(Some(Versions { list, latest: None }));
    }
    let Some(latest) = fetch.get(&format!("{module_url}/@latest"))? else { return Ok(None) };
    let latest =
        serde_json::from_slice::<Value>(&latest).ok().and_then(|info| info["Version"].as_str().map(str::to_owned));
    Ok(Some(Versions { list, latest }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capital_letters_are_escaped() {
        assert_eq!(escape("github.com/Khan/genqlient"), "github.com/!khan/genqlient");
        assert_eq!(escape("github.com/BurntSushi/toml"), "github.com/!burnt!sushi/toml");
    }

    #[test]
    fn the_newest_release_is_offered() {
        let versions = Versions {
            list: ["v1.0.0", "v1.2.6", "v1.10.0-rc.1", "v1.2.10", "v2.0.0+incompatible"].map(str::to_owned).to_vec(),
            latest: None,
        };
        assert_eq!(versions.newer_than("v1.2.3").as_deref(), Some("v1.2.10"));
        assert_eq!(versions.newer_than("v1.2.10"), None);
        assert_eq!(versions.newer_than("v1.0.0+incompatible").as_deref(), Some("v2.0.0+incompatible"));
        assert_eq!(versions.release_at_or_after("v1.2.4").as_deref(), Some("v1.2.6"));
        assert_eq!(
            versions.release_at_or_after_skipping("v1.2.4", &|version| version == "v1.2.6").as_deref(),
            Some("v1.2.10")
        );
        assert_eq!(versions.newer_than_skipping("v1.2.3", &|version| version == "v1.2.10").as_deref(), Some("v1.2.6"));
        assert_eq!(versions.latest().as_deref(), Some("v1.2.10"));
        let pseudo = Versions { list: Vec::new(), latest: Some("v0.0.0-20240101000000-abcdef123456".to_owned()) };
        assert_eq!(
            pseudo.newer_than("v0.0.0-20180720192143-9eef69e6c4e7").as_deref(),
            Some("v0.0.0-20240101000000-abcdef123456")
        );
        assert_eq!(pseudo.newer_than("v0.0.0-20250101000000-abcdef123456"), None);
    }
}
