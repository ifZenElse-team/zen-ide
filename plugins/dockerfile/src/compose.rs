use crate::assist::{Completion, Problem, Severity, Suggestion};

#[derive(Clone, Copy)]
pub enum Shape {
    Any,
    Enum(&'static [&'static str]),
    Keys(&'static Object),
    Named(&'static Object),
    List(&'static Shape),
}

pub struct Object {
    pub name: &'static str,
    pub keys: &'static [(&'static str, Shape)],
}

const ANY: Shape = Shape::Any;

const IPAM_CONFIG: Object = Object {
    name: "an IPAM configuration",
    keys: &[("subnet", ANY), ("ip_range", ANY), ("gateway", ANY), ("aux_addresses", ANY)],
};

const IPAM: Object = Object {
    name: "IPAM",
    keys: &[("driver", ANY), ("config", Shape::List(&Shape::Keys(&IPAM_CONFIG))), ("options", ANY)],
};

const NETWORK: Object = Object {
    name: "a network",
    keys: &[
        ("driver", ANY),
        ("driver_opts", ANY),
        ("attachable", ANY),
        ("enable_ipv4", ANY),
        ("enable_ipv6", ANY),
        ("external", ANY),
        ("internal", ANY),
        ("ipam", Shape::Keys(&IPAM)),
        ("labels", ANY),
        ("name", ANY),
    ],
};

const VOLUME: Object = Object {
    name: "a volume",
    keys: &[("driver", ANY), ("driver_opts", ANY), ("external", ANY), ("labels", ANY), ("name", ANY)],
};

const CONFIG: Object = Object {
    name: "a config",
    keys: &[
        ("file", ANY),
        ("environment", ANY),
        ("content", ANY),
        ("external", ANY),
        ("name", ANY),
        ("labels", ANY),
        ("template_driver", ANY),
    ],
};

const SECRET: Object = Object {
    name: "a secret",
    keys: &[
        ("file", ANY),
        ("environment", ANY),
        ("external", ANY),
        ("name", ANY),
        ("labels", ANY),
        ("driver", ANY),
        ("driver_opts", ANY),
        ("template_driver", ANY),
    ],
};

const INCLUDE: Object =
    Object { name: "an include", keys: &[("path", ANY), ("project_directory", ANY), ("env_file", ANY)] };

const BUILD: Object = Object {
    name: "build",
    keys: &[
        ("context", ANY),
        ("dockerfile", ANY),
        ("dockerfile_inline", ANY),
        ("args", ANY),
        ("ssh", ANY),
        ("cache_from", ANY),
        ("cache_to", ANY),
        ("additional_contexts", ANY),
        ("entitlements", ANY),
        ("extra_hosts", ANY),
        ("isolation", ANY),
        ("labels", ANY),
        ("network", ANY),
        ("no_cache", ANY),
        ("platforms", ANY),
        ("privileged", ANY),
        ("provenance", ANY),
        ("pull", ANY),
        ("sbom", ANY),
        ("secrets", ANY),
        ("shm_size", ANY),
        ("tags", ANY),
        ("target", ANY),
        ("ulimits", ANY),
    ],
};

const DEVICE: Object = Object {
    name: "a device reservation",
    keys: &[("capabilities", ANY), ("count", ANY), ("device_ids", ANY), ("driver", ANY), ("options", ANY)],
};

const LIMITS: Object = Object { name: "limits", keys: &[("cpus", ANY), ("memory", ANY), ("pids", ANY)] };

const RESERVATIONS: Object = Object {
    name: "reservations",
    keys: &[
        ("cpus", ANY),
        ("memory", ANY),
        ("devices", Shape::List(&Shape::Keys(&DEVICE))),
        ("generic_resources", ANY),
    ],
};

const RESOURCES: Object = Object {
    name: "resources",
    keys: &[("limits", Shape::Keys(&LIMITS)), ("reservations", Shape::Keys(&RESERVATIONS))],
};

const RESTART_POLICY: Object = Object {
    name: "restart_policy",
    keys: &[
        ("condition", Shape::Enum(&["none", "on-failure", "any"])),
        ("delay", ANY),
        ("max_attempts", ANY),
        ("window", ANY),
    ],
};

const UPDATE: Object = Object {
    name: "an update or rollback configuration",
    keys: &[
        ("parallelism", ANY),
        ("delay", ANY),
        ("failure_action", ANY),
        ("monitor", ANY),
        ("max_failure_ratio", ANY),
        ("order", Shape::Enum(&["stop-first", "start-first"])),
    ],
};

const PLACEMENT: Object =
    Object { name: "placement", keys: &[("constraints", ANY), ("preferences", ANY), ("max_replicas_per_node", ANY)] };

const DEPLOY: Object = Object {
    name: "deploy",
    keys: &[
        ("mode", Shape::Enum(&["global", "replicated", "replicated-job", "global-job"])),
        ("replicas", ANY),
        ("labels", ANY),
        ("endpoint_mode", Shape::Enum(&["vip", "dnsrr"])),
        ("placement", Shape::Keys(&PLACEMENT)),
        ("resources", Shape::Keys(&RESOURCES)),
        ("restart_policy", Shape::Keys(&RESTART_POLICY)),
        ("rollback_config", Shape::Keys(&UPDATE)),
        ("update_config", Shape::Keys(&UPDATE)),
    ],
};

const EXEC: Object = Object {
    name: "exec",
    keys: &[("command", ANY), ("user", ANY), ("privileged", ANY), ("working_dir", ANY), ("environment", ANY)],
};

const WATCH: Object = Object {
    name: "a watch rule",
    keys: &[
        ("path", ANY),
        ("action", Shape::Enum(&["rebuild", "sync", "sync+restart", "restart", "sync+exec"])),
        ("target", ANY),
        ("ignore", ANY),
        ("include", ANY),
        ("initial_sync", ANY),
        ("exec", Shape::Keys(&EXEC)),
    ],
};

const DEVELOP: Object = Object { name: "develop", keys: &[("watch", Shape::List(&Shape::Keys(&WATCH)))] };

const HEALTHCHECK: Object = Object {
    name: "healthcheck",
    keys: &[
        ("test", ANY),
        ("interval", ANY),
        ("timeout", ANY),
        ("retries", ANY),
        ("start_period", ANY),
        ("start_interval", ANY),
        ("disable", ANY),
    ],
};

const LOGGING: Object = Object { name: "logging", keys: &[("driver", ANY), ("options", ANY)] };

const EXTENDS: Object = Object { name: "extends", keys: &[("service", ANY), ("file", ANY)] };

const DEPENDENCY: Object = Object {
    name: "a dependency",
    keys: &[
        ("condition", Shape::Enum(&["service_started", "service_healthy", "service_completed_successfully"])),
        ("restart", ANY),
        ("required", ANY),
    ],
};

const SERVICE_NETWORK: Object = Object {
    name: "a service's network",
    keys: &[
        ("aliases", ANY),
        ("ipv4_address", ANY),
        ("ipv6_address", ANY),
        ("link_local_ips", ANY),
        ("mac_address", ANY),
        ("priority", ANY),
        ("gw_priority", ANY),
        ("driver_opts", ANY),
        ("interface_name", ANY),
    ],
};

const PORT: Object = Object {
    name: "a port",
    keys: &[
        ("target", ANY),
        ("published", ANY),
        ("host_ip", ANY),
        ("protocol", Shape::Enum(&["tcp", "udp"])),
        ("mode", Shape::Enum(&["host", "ingress"])),
        ("app_protocol", ANY),
        ("name", ANY),
    ],
};

const BIND: Object =
    Object { name: "bind", keys: &[("propagation", ANY), ("create_host_path", ANY), ("selinux", ANY)] };

const VOLUME_OPTIONS: Object = Object { name: "volume", keys: &[("nocopy", ANY), ("subpath", ANY)] };

const TMPFS: Object = Object { name: "tmpfs", keys: &[("size", ANY), ("mode", ANY)] };

const IMAGE_MOUNT: Object = Object { name: "image", keys: &[("subpath", ANY)] };

const MOUNT: Object = Object {
    name: "a mount",
    keys: &[
        ("type", Shape::Enum(&["bind", "volume", "tmpfs", "npipe", "cluster", "image"])),
        ("source", ANY),
        ("target", ANY),
        ("read_only", ANY),
        ("consistency", ANY),
        ("bind", Shape::Keys(&BIND)),
        ("volume", Shape::Keys(&VOLUME_OPTIONS)),
        ("tmpfs", Shape::Keys(&TMPFS)),
        ("image", Shape::Keys(&IMAGE_MOUNT)),
    ],
};

const GRANT: Object = Object {
    name: "a config or secret grant",
    keys: &[("source", ANY), ("target", ANY), ("uid", ANY), ("gid", ANY), ("mode", ANY)],
};

pub const RESTART: &[&str] = &["no", "always", "on-failure", "unless-stopped"];

const SERVICE: Object = Object {
    name: "a service",
    keys: &[
        ("annotations", ANY),
        ("attach", ANY),
        ("blkio_config", ANY),
        ("build", Shape::Keys(&BUILD)),
        ("cap_add", ANY),
        ("cap_drop", ANY),
        ("cgroup", ANY),
        ("cgroup_parent", ANY),
        ("command", ANY),
        ("configs", Shape::List(&Shape::Keys(&GRANT))),
        ("container_name", ANY),
        ("cpu_count", ANY),
        ("cpu_percent", ANY),
        ("cpu_period", ANY),
        ("cpu_quota", ANY),
        ("cpu_rt_period", ANY),
        ("cpu_rt_runtime", ANY),
        ("cpu_shares", ANY),
        ("cpus", ANY),
        ("cpuset", ANY),
        ("credential_spec", ANY),
        ("depends_on", Shape::Named(&DEPENDENCY)),
        ("deploy", Shape::Keys(&DEPLOY)),
        ("develop", Shape::Keys(&DEVELOP)),
        ("device_cgroup_rules", ANY),
        ("devices", ANY),
        ("dns", ANY),
        ("dns_opt", ANY),
        ("dns_search", ANY),
        ("domainname", ANY),
        ("driver_opts", ANY),
        ("entrypoint", ANY),
        ("env_file", ANY),
        ("environment", ANY),
        ("expose", ANY),
        ("extends", Shape::Keys(&EXTENDS)),
        ("external_links", ANY),
        ("extra_hosts", ANY),
        ("gpus", ANY),
        ("group_add", ANY),
        ("healthcheck", Shape::Keys(&HEALTHCHECK)),
        ("hostname", ANY),
        ("image", ANY),
        ("init", ANY),
        ("ipc", ANY),
        ("isolation", ANY),
        ("labels", ANY),
        ("label_file", ANY),
        ("links", ANY),
        ("logging", Shape::Keys(&LOGGING)),
        ("mac_address", ANY),
        ("mem_limit", ANY),
        ("mem_reservation", ANY),
        ("mem_swappiness", ANY),
        ("memswap_limit", ANY),
        ("models", ANY),
        ("network_mode", ANY),
        ("networks", Shape::Named(&SERVICE_NETWORK)),
        ("oom_kill_disable", ANY),
        ("oom_score_adj", ANY),
        ("pid", ANY),
        ("pids_limit", ANY),
        ("platform", ANY),
        ("ports", Shape::List(&Shape::Keys(&PORT))),
        ("post_start", ANY),
        ("pre_stop", ANY),
        ("privileged", ANY),
        ("profiles", ANY),
        ("provider", ANY),
        ("pull_policy", ANY),
        ("read_only", ANY),
        ("restart", Shape::Enum(RESTART)),
        ("runtime", ANY),
        ("scale", ANY),
        ("secrets", Shape::List(&Shape::Keys(&GRANT))),
        ("security_opt", ANY),
        ("shm_size", ANY),
        ("stdin_open", ANY),
        ("stop_grace_period", ANY),
        ("stop_signal", ANY),
        ("storage_opt", ANY),
        ("sysctls", ANY),
        ("tmpfs", ANY),
        ("tty", ANY),
        ("ulimits", ANY),
        ("use_api_socket", ANY),
        ("user", ANY),
        ("userns_mode", ANY),
        ("uts", ANY),
        ("volumes", Shape::List(&Shape::Keys(&MOUNT))),
        ("volumes_from", ANY),
        ("working_dir", ANY),
    ],
};

const FILE: Object = Object {
    name: "a Compose file",
    keys: &[
        ("version", ANY),
        ("name", ANY),
        ("include", Shape::List(&Shape::Keys(&INCLUDE))),
        ("services", Shape::Named(&SERVICE)),
        ("networks", Shape::Named(&NETWORK)),
        ("volumes", Shape::Named(&VOLUME)),
        ("configs", Shape::Named(&CONFIG)),
        ("secrets", Shape::Named(&SECRET)),
        ("models", ANY),
    ],
};

const ROOT: Shape = Shape::Keys(&FILE);

pub struct Entry<'a> {
    pub line: usize,
    pub column: usize,
    pub key: &'a str,
    pub key_start: usize,
    pub value: &'a str,
    pub value_start: usize,
    pub depth: Vec<Step<'a>>,
}

#[derive(Clone)]
pub enum Step<'a> {
    Key(&'a str),
    Item,
}

struct Frame<'a> {
    column: usize,
    step: Step<'a>,
}

fn key_of(content: &str) -> Option<(&str, usize, &str)> {
    if content.starts_with(['"', '\'']) {
        let quote = content.as_bytes()[0] as char;
        let close = content[1..].find(quote)? + 1;
        let after = content[close + 1..].trim_start_matches([' ', '\t']);
        let value = after.strip_prefix(':')?;
        if !(value.is_empty() || value.starts_with([' ', '\t'])) {
            return None;
        }
        let skipped = content.len() - value.len();
        return Some((&content[1..close], skipped, value));
    }
    let mut at = 0;
    while let Some(found) = content[at..].find(':') {
        let colon = at + found;
        let after = &content[colon + 1..];
        if after.is_empty() || after.starts_with([' ', '\t']) {
            let key = content[..colon].trim_end();
            if key.is_empty() || key.starts_with(['{', '[', '#', '&', '*', '!', '|', '>']) {
                return None;
            }
            return Some((key, colon + 1, after));
        }
        at = colon + 1;
    }
    None
}

fn strip_comment(value: &str) -> &str {
    let mut quote = None;
    for (index, c) in value.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), c) if c == open => quote = None,
            (None, '#') if index == 0 || value[..index].ends_with([' ', '\t']) => return value[..index].trim_end(),
            _ => {}
        }
    }
    value.trim_end()
}

pub fn entries(text: &str) -> Vec<Entry<'_>> {
    let mut out = Vec::new();
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut block: Option<usize> = None;
    let mut offset = 0;
    for (number, raw) in text.split('\n').enumerate() {
        let start = offset;
        offset += raw.len() + 1;
        let line = raw.trim_end_matches('\r');
        let indent = line.len() - line.trim_start_matches(' ').len();
        let body = &line[indent..];
        if let Some(scalar) = block {
            if body.is_empty() || indent > scalar {
                continue;
            }
            block = None;
        }
        if body.is_empty() || body.starts_with('#') || body.starts_with("---") || body.starts_with("...") {
            continue;
        }
        let mut column = indent;
        let mut rest = body;
        let mut items = Vec::new();
        while rest == "-" || rest.starts_with("- ") {
            items.push(column);
            let after = rest[1..].trim_start_matches(' ');
            column += rest.len() - after.len();
            rest = after;
        }
        for item in items {
            while stack
                .last()
                .is_some_and(|frame| frame.column > item || (frame.column == item && matches!(frame.step, Step::Item)))
            {
                stack.pop();
            }
            stack.push(Frame { column: item, step: Step::Item });
        }
        let Some((key, value_at, value)) = key_of(rest) else { continue };
        while stack.last().is_some_and(|frame| frame.column >= column) {
            stack.pop();
        }
        let value = strip_comment(value);
        let trimmed = value.trim_start_matches([' ', '\t']);
        let value_start = start + column + value_at + (value.len() - trimmed.len());
        let key_start = start + column + usize::from(rest.starts_with(['"', '\'']));
        let depth = stack.iter().map(|frame| frame.step.clone()).collect();
        out.push(Entry { line: number, column, key, key_start, value: trimmed, value_start, depth });
        if trimmed.starts_with('|') || trimmed.starts_with('>') {
            block = Some(column);
        }
        stack.push(Frame { column, step: Step::Key(key) });
    }
    out
}

fn child(shape: Shape, step: &Step<'_>) -> Shape {
    match (shape, step) {
        (Shape::Keys(object), Step::Key(key)) => {
            object.keys.iter().find(|(name, _)| name == key).map_or(Shape::Any, |(_, shape)| *shape)
        }
        (Shape::Named(object), Step::Key(_)) => Shape::Keys(object),
        (Shape::Named(object), Step::Item) => Shape::Keys(object),
        (Shape::List(inner), Step::Item) => *inner,
        _ => Shape::Any,
    }
}

fn shape_at(steps: &[Step<'_>]) -> Shape {
    let mut shape = ROOT;
    for step in steps {
        if let (Shape::Named(_), Step::Item) = (shape, step) {
            return Shape::Any;
        }
        shape = child(shape, step);
    }
    shape
}

fn plain(value: &str) -> Option<&str> {
    let value = value.trim_matches(['"', '\'']);
    (!value.is_empty() && !value.contains("${") && !value.starts_with(['{', '[', '&', '*', '!'])).then_some(value)
}

fn known_value(key: &str, allowed: &[&str], value: &str) -> bool {
    allowed.contains(&value)
        || (key == "restart"
            && value
                .strip_prefix("on-failure:")
                .is_some_and(|count| !count.is_empty() && count.bytes().all(|b| b.is_ascii_digit())))
}

pub fn problems(text: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    for entry in entries(text) {
        let parent = shape_at(&entry.depth);
        let key_end = entry.key_start + entry.key.len();
        if entry.key.starts_with("x-") || entry.key == "<<" {
            continue;
        }
        if entry.depth.is_empty() && entry.key == "version" {
            let message = "`version` is obsolete: the Compose Specification ignores it".to_owned();
            out.push(Problem { start: entry.key_start, end: key_end, severity: Severity::Warning, message });
            continue;
        }
        let Shape::Keys(object) = parent else { continue };
        let Some((_, shape)) = object.keys.iter().find(|(name, _)| *name == entry.key) else {
            let message = format!("`{}` is not a key of {} in the Compose Specification", entry.key, object.name);
            out.push(Problem { start: entry.key_start, end: key_end, severity: Severity::Warning, message });
            continue;
        };
        if let Shape::Enum(allowed) = shape
            && let Some(value) = plain(entry.value)
            && !known_value(entry.key, allowed, value)
        {
            let message = format!("`{value}` is not a value of `{}`: {}", entry.key, allowed.join(", "));
            let start = entry.value_start + usize::from(entry.value.starts_with(['"', '\'']));
            out.push(Problem { start, end: start + value.len(), severity: Severity::Error, message });
        }
    }
    out
}

pub fn complete(text: &str, caret: usize) -> Option<Completion> {
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let line_start = text[..caret].rfind('\n').map_or(0, |at| at + 1);
    let line_end = text[caret..].find('\n').map_or(text.len(), |at| caret + at);
    let before = &text[line_start..caret];
    let mut probe = String::with_capacity(text.len() + 8);
    probe.push_str(&text[..line_start]);
    if let Some((key, _, value)) = key_of(before.trim_start_matches([' ', '-'])) {
        let value = value.trim_start();
        probe.push_str(&before[..before.len() - value.len()]);
        probe.push('\n');
        let entries = entries(&probe);
        let entry = entries.last()?;
        let mut path = entry.depth.clone();
        path.push(Step::Key(key));
        let Shape::Enum(allowed) = shape_at(&path) else { return None };
        let items = allowed
            .iter()
            .filter(|candidate| candidate.starts_with(value.trim_matches(['"', '\''])))
            .map(|candidate| Suggestion {
                label: (*candidate).to_owned(),
                kind: "value",
                insert: (*candidate).to_owned(),
                detail: None,
            })
            .collect();
        return Some(Completion { from: caret - value.len(), items });
    }
    let typed = before.trim_start_matches([' ', '-']);
    if typed.contains([' ', ':', '"', '\'', '#']) {
        return None;
    }
    probe.push_str(&before[..before.len() - typed.len()]);
    probe.push_str("zen-probe:\n");
    let after = &text[line_end.min(text.len())..];
    probe.push_str(after.strip_prefix('\n').unwrap_or(after));
    let entries = entries(&probe);
    let index = entries.iter().position(|entry| entry.key == "zen-probe")?;
    let here = &entries[index];
    let Shape::Keys(object) = shape_at(&here.depth) else { return None };
    let present: Vec<&str> = entries
        .iter()
        .filter(|other| other.column == here.column && same_steps(&other.depth, &here.depth))
        .map(|other| other.key)
        .collect();
    let items = object
        .keys
        .iter()
        .filter(|(name, _)| {
            name.starts_with(typed) && !present.contains(name) && !(here.depth.is_empty() && *name == "version")
        })
        .map(|(name, shape)| Suggestion {
            label: (*name).to_owned(),
            kind: "property",
            insert: match shape {
                Shape::Keys(_) | Shape::Named(_) | Shape::List(_) => {
                    format!("{name}:\n{}  $0", " ".repeat(here.column))
                }
                _ => format!("{name}: "),
            },
            detail: None,
        })
        .collect();
    Some(Completion { from: caret - typed.len(), items })
}

fn same_steps(a: &[Step<'_>], b: &[Step<'_>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Step::Key(a), Step::Key(b)) => a == b,
            (Step::Item, Step::Item) => true,
            _ => false,
        })
}

pub struct Target {
    pub line: usize,
    pub name: String,
    pub id: String,
    pub args: Vec<String>,
}

pub fn run_targets(text: &str) -> Vec<Target> {
    let entries = entries(text);
    let Some(services) = entries.iter().find(|entry| entry.depth.is_empty() && entry.key == "services") else {
        return Vec::new();
    };
    let up = |extra: Option<&str>| {
        ["compose", "-f", "${file}", "up"].iter().map(|arg| (*arg).to_owned()).chain(extra.map(str::to_owned)).collect()
    };
    let mut targets =
        vec![Target { line: services.line, name: "All services".to_owned(), id: "up".to_owned(), args: up(None) }];
    let service_column = entries
        .iter()
        .filter(|entry| matches!(entry.depth.as_slice(), [Step::Key("services")]))
        .map(|entry| entry.column)
        .min();
    for entry in entries.iter().filter(|entry| {
        matches!(entry.depth.as_slice(), [Step::Key("services")])
            && Some(entry.column) == service_column
            && !entry.key.starts_with("x-")
    }) {
        targets.push(Target {
            line: entry.line,
            name: entry.key.to_owned(),
            id: format!("up:{}", entry.key),
            args: up(Some(entry.key)),
        });
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(&str, Severity, String)> {
        problems(text).into_iter().map(|p| (&text[p.start..p.end], p.severity, p.message)).collect()
    }

    fn labels(text: &str) -> Vec<String> {
        let caret = text.find('|').unwrap();
        let text = text.replacen('|', "", 1);
        complete(&text, caret).map(|found| found.items.into_iter().map(|item| item.label).collect()).unwrap_or_default()
    }

    const GOOD: &str = "name: shop\nservices:\n  web:\n    image: nginx:1.27\n    restart: on-failure:3\n    ports:\n      - \"8080:80\"\n      - target: 443\n        published: 8443\n        protocol: tcp\n    depends_on:\n      db:\n        condition: service_healthy\n    environment:\n      ANY_NAME: value\n    x-note: free\n    deploy:\n      resources:\n        limits:\n          memory: 512M\n    command: |\n      sh -c 'unknown: keys in a block'\n    healthcheck:\n      test: [\"CMD\", \"true\"]\n  db:\n    image: postgres\n    volumes:\n    - type: volume\n      source: data\n      target: /var/lib/postgresql/data\nvolumes:\n  data: {}\nx-common: &common\n  anything: goes\n";

    #[test]
    fn every_service_and_the_whole_file_are_run_targets() {
        let targets = run_targets(GOOD);
        let found: Vec<(usize, &str)> = targets.iter().map(|target| (target.line, target.name.as_str())).collect();
        assert_eq!(found, [(1, "All services"), (2, "web"), (24, "db")]);
        assert_eq!(targets[1].args, ["compose", "-f", "${file}", "up", "web"]);
        assert_eq!(targets[0].args, ["compose", "-f", "${file}", "up"]);
        assert!(run_targets("name: x\n").is_empty());
    }

    #[test]
    fn a_good_compose_file_has_no_problems() {
        assert_eq!(found(GOOD), []);
    }

    #[test]
    fn unknown_keys_bad_values_and_the_obsolete_version_are_told() {
        let text = "version: \"3.9\"\nservices:\n  web:\n    imagee: nginx\n    restart: sometimes\n    ports:\n      - target: 80\n        protocl: tcp\n    depends_on:\n      db:\n        condition: started\nnetworks:\n  front:\n    drivr: bridge\n";
        let problems = found(text);
        assert_eq!(problems[0].0, "version");
        assert_eq!(problems[0].1, Severity::Warning);
        assert_eq!(
            problems[1],
            ("imagee", Severity::Warning, "`imagee` is not a key of a service in the Compose Specification".to_owned())
        );
        assert_eq!(problems[2].0, "sometimes");
        assert_eq!(problems[2].1, Severity::Error);
        assert_eq!(problems[3].0, "protocl");
        assert_eq!(problems[4].0, "started");
        assert_eq!(
            problems[5],
            ("drivr", Severity::Warning, "`drivr` is not a key of a network in the Compose Specification".to_owned())
        );
        assert_eq!(problems.len(), 6);
    }

    #[test]
    fn keys_are_completed_where_the_caret_is_without_the_ones_there() {
        assert_eq!(labels("services:\n  web:\n    image: x\n    res|\n"), ["restart"]);
        let top = labels("services:\n  web:\n    image: x\nvol|");
        assert_eq!(top, ["volumes"]);
        let service = labels("services:\n  web:\n    image: x\n    |\n");
        assert!(service.contains(&"ports".to_owned()) && !service.contains(&"image".to_owned()), "{service:?}");
        assert_eq!(labels("services:\n  web:\n    ports:\n      - target: 80\n        pro|\n"), ["protocol"]);
        assert_eq!(labels("services:\n  web:\n    restart: un|\n"), ["unless-stopped"]);
        assert_eq!(
            labels("services:\n  web:\n    deploy:\n      mode: |\n"),
            ["global", "replicated", "replicated-job", "global-job"]
        );
        assert_eq!(
            labels("services:\n  web:\n    environment:\n      AN|\n"),
            Vec::<String>::new(),
            "free keys are not completed"
        );
    }
}
