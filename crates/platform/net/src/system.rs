#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemProxy {
    pub http: Option<(String, u16)>,
    pub https: Option<(String, u16)>,
    pub socks: Option<(String, u16)>,
    pub exceptions: Vec<String>,
    pub script: Option<String>,
}

const AUTO_DISCOVERY: &str = "WPAD";

pub fn parse_scutil(output: &str) -> SystemProxy {
    let mut values = std::collections::HashMap::new();
    let mut exceptions = Vec::new();
    let mut in_exceptions = false;
    for line in output.lines().map(str::trim) {
        if in_exceptions {
            if line == "}" {
                in_exceptions = false;
            } else if let Some((_, value)) = line.split_once(" : ") {
                exceptions.push(value.trim().to_owned());
            }
            continue;
        }
        if line.starts_with("ExceptionsList : ") {
            in_exceptions = true;
        } else if let Some((key, value)) = line.split_once(" : ") {
            values.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let on = |key: &str| values.get(key).is_some_and(|value| value == "1");
    let endpoint = |prefix: &str| {
        let host = values.get(&format!("{prefix}Proxy"))?;
        let port = values.get(&format!("{prefix}Port"))?.parse().ok()?;
        on(&format!("{prefix}Enable")).then(|| (host.clone(), port))
    };
    let script = if on("ProxyAutoConfigEnable") {
        Some(values.get("ProxyAutoConfigURLString").cloned().unwrap_or_default())
    } else {
        on("ProxyAutoDiscoveryEnable").then(|| AUTO_DISCOVERY.to_owned())
    };
    SystemProxy { http: endpoint("HTTP"), https: endpoint("HTTPS"), socks: endpoint("SOCKS"), exceptions, script }
}

#[cfg(target_os = "macos")]
pub(crate) fn read() -> SystemProxy {
    std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_scutil(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn read() -> SystemProxy {
    SystemProxy::default()
}
