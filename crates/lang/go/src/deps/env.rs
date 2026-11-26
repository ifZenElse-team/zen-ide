use std::path::Path;
use std::process::Command;

use serde_json::Value;

pub const DEFAULT_PROXY: &str = "https://proxy.golang.org,direct";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoEnv {
    pub proxy: String,
    pub private: String,
    pub no_proxy: String,
    pub no_sumdb: String,
    pub flags: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proxy {
    Url(String),
    Direct,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyStep {
    pub proxy: Proxy,
    pub on_any_error: bool,
}

impl GoEnv {
    pub fn read(tool: &Path, env: &[(String, String)], dir: &Path) -> GoEnv {
        if !tool.as_os_str().is_empty() {
            let ask = |dir: Option<&Path>| {
                let mut command = Command::new(tool);
                command.args([
                    "env",
                    "-json",
                    "GOPROXY",
                    "GOPRIVATE",
                    "GONOPROXY",
                    "GONOSUMDB",
                    "GOFLAGS",
                    "GOVERSION",
                ]);
                command.envs(env.iter().map(|(key, value)| (key, value)));
                command.env("GOTOOLCHAIN", "local");
                if let Some(dir) = dir.filter(|dir| dir.is_dir()) {
                    command.current_dir(dir);
                }
                let output = zen_run::capture(&mut command, std::time::Duration::from_secs(60))
                    .ok()
                    .filter(|output| output.status.success())?;
                serde_json::from_slice::<Value>(&output.stdout).ok()
            };
            if let Some(json) = ask(Some(dir)).or_else(|| ask(Some(&std::env::temp_dir()))) {
                return GoEnv::from_json(&json);
            }
        }
        GoEnv::from_vars(|key| std::env::var(key).ok())
    }

    pub fn from_json(json: &Value) -> GoEnv {
        GoEnv::from_vars(|key| json[key].as_str().map(str::to_owned))
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> GoEnv {
        let private = var("GOPRIVATE").unwrap_or_default();
        GoEnv {
            proxy: var("GOPROXY").filter(|proxy| !proxy.is_empty()).unwrap_or_else(|| DEFAULT_PROXY.to_owned()),
            no_proxy: var("GONOPROXY").filter(|value| !value.is_empty()).unwrap_or_else(|| private.clone()),
            no_sumdb: var("GONOSUMDB").filter(|value| !value.is_empty()).unwrap_or_else(|| private.clone()),
            private,
            flags: var("GOFLAGS").unwrap_or_default(),
            version: var("GOVERSION").unwrap_or_default(),
        }
    }

    pub fn is_private(&self, module: &str) -> bool {
        [&self.private, &self.no_proxy, &self.no_sumdb].iter().any(|patterns| matches_prefix_patterns(patterns, module))
    }

    pub fn proxies(&self) -> Vec<ProxyStep> {
        let mut steps = Vec::new();
        let mut rest = self.proxy.trim();
        while !rest.is_empty() {
            let end = rest.find([',', '|']).unwrap_or(rest.len());
            let entry = rest[..end].trim();
            let on_any_error = rest[end..].starts_with('|');
            rest = rest.get(end + 1..).unwrap_or("");
            if entry.is_empty() {
                continue;
            }
            let proxy = match entry {
                "direct" => Proxy::Direct,
                "off" => Proxy::Off,
                url => Proxy::Url(url.trim_end_matches('/').to_owned()),
            };
            steps.push(ProxyStep { proxy, on_any_error });
        }
        steps
    }
}

pub fn matches_prefix_patterns(patterns: &str, target: &str) -> bool {
    patterns.split(',').map(str::trim).filter(|pattern| !pattern.is_empty()).any(|pattern| {
        let pattern = pattern.trim_end_matches('/');
        let wanted: Vec<&str> = pattern.split('/').collect();
        let names: Vec<&str> = target.split('/').collect();
        names.len() >= wanted.len() && wanted.iter().zip(&names).all(|(glob, name)| glob_matches(glob, name))
    })
}

fn glob_matches(glob: &str, name: &str) -> bool {
    let glob: Vec<char> = glob.chars().collect();
    let name: Vec<char> = name.chars().collect();
    matches_from(&glob, &name)
}

fn matches_from(glob: &[char], name: &[char]) -> bool {
    match glob.first() {
        None => name.is_empty(),
        Some('*') => (0..=name.len()).any(|skip| matches_from(&glob[1..], &name[skip..])),
        Some('?') => !name.is_empty() && matches_from(&glob[1..], &name[1..]),
        Some('[') => {
            let Some(close) = glob.iter().position(|&c| c == ']') else {
                return name.first() == Some(&'[') && matches_from(&glob[1..], &name[1..]);
            };
            let Some(&c) = name.first() else { return false };
            let class = &glob[1..close];
            let (negated, class) = match class.first() {
                Some('^') => (true, &class[1..]),
                _ => (false, class),
            };
            let mut found = false;
            let mut index = 0;
            while index < class.len() {
                if index + 2 < class.len() && class[index + 1] == '-' {
                    found |= class[index] <= c && c <= class[index + 2];
                    index += 3;
                } else {
                    found |= class[index] == c;
                    index += 1;
                }
            }
            found != negated && matches_from(&glob[close + 1..], &name[1..])
        }
        Some(&literal) => name.first() == Some(&literal) && matches_from(&glob[1..], &name[1..]),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn private_modules_are_matched_by_prefix_patterns() {
        let env = GoEnv::from_json(
            &json!({ "GOPRIVATE": "*.corp.example.com,rsc.io/private", "GONOSUMDB": "gitlab.infra.*" }),
        );
        assert!(env.is_private("git.corp.example.com/team/lib"));
        assert!(env.is_private("rsc.io/private/quux"));
        assert!(env.is_private("gitlab.infra.example.com/backend/go-kit"));
        assert!(!env.is_private("rsc.io/privateer"));
        assert!(!env.is_private("corp.example.com/lib"));
        assert!(!env.is_private("github.com/google/uuid"));
        assert_eq!(env.no_proxy, "*.corp.example.com,rsc.io/private", "GONOPROXY defaults to GOPRIVATE");
        assert!(matches_prefix_patterns("github.com/[a-c]*", "github.com/bar/x"));
        assert!(!matches_prefix_patterns("github.com/[^a-c]*", "github.com/bar/x"));
        assert!(matches_prefix_patterns("example.com/?ib", "example.com/lib/v2"));
    }

    #[test]
    fn goproxy_lists_proxies_with_their_fallbacks() {
        let env = GoEnv::from_vars(|key| {
            (key == "GOPROXY").then(|| "https://a.example/,https://b.example|direct,off".to_owned())
        });
        assert_eq!(
            env.proxies(),
            [
                ProxyStep { proxy: Proxy::Url("https://a.example".to_owned()), on_any_error: false },
                ProxyStep { proxy: Proxy::Url("https://b.example".to_owned()), on_any_error: true },
                ProxyStep { proxy: Proxy::Direct, on_any_error: false },
                ProxyStep { proxy: Proxy::Off, on_any_error: false },
            ]
        );
        let default = GoEnv::from_vars(|_| None);
        assert_eq!(default.proxy, DEFAULT_PROXY);
        assert_eq!(default.proxies()[0].proxy, Proxy::Url("https://proxy.golang.org".to_owned()));
    }
}
