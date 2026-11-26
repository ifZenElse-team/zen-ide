use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value as Json, json};

use crate::yaml::{self, Document, Node};

mod helm;

pub struct File {
    pub path: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexNode {
    pub id: String,
    pub kind: String,
    pub group: &'static str,
    pub name: String,
    pub namespace: String,
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub detail: String,
    pub in_graph: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub files: Vec<String>,
    pub after: &'static str,
    pub items: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub error: bool,
    pub message: String,
}

#[derive(Default)]
pub struct Index {
    pub kinds: BTreeSet<&'static str>,
    pub nodes: Vec<IndexNode>,
    pub edges: BTreeSet<Edge>,
    pub references: Vec<Reference>,
    pub problems: Vec<Problem>,
    pub completions: Vec<Completion>,
}

const FLUX_SOURCES: [&str; 5] = ["GitRepository", "OCIRepository", "HelmRepository", "Bucket", "HelmChart"];

fn group_of(api_version: &str, kind: &str) -> Option<&'static str> {
    let group = api_version.split('/').next().unwrap_or("");
    match (group, kind) {
        ("source.toolkit.fluxcd.io", kind) if FLUX_SOURCES.contains(&kind) => Some("flux"),
        ("kustomize.toolkit.fluxcd.io", "Kustomization") | ("helm.toolkit.fluxcd.io", "HelmRelease") => Some("flux"),
        ("argoproj.io", "Application" | "ApplicationSet" | "AppProject") => Some("argocd"),
        ("kustomize.config.k8s.io", "Kustomization" | "Component") => Some("kustomize"),
        _ => None,
    }
}

fn is_kustomization_file(path: &str) -> bool {
    matches!(file_name(path), "kustomization.yaml" | "kustomization.yml" | "Kustomization")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

pub fn normalise(base: &str, path: &str) -> String {
    let joined = if path.starts_with('/') || base.is_empty() { path.to_owned() } else { format!("{base}/{path}") };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

fn folder_id(folder: &str) -> String {
    format!("path:{folder}")
}

struct Object<'a> {
    node: usize,
    doc: &'a Document,
    path: &'a str,
}

struct Builder<'a> {
    index: Index,
    paths: BTreeSet<&'a str>,
    folders: BTreeSet<String>,
    namespaces: BTreeMap<String, String>,
    remotes: Vec<String>,
}

impl<'a> Builder<'a> {
    fn exists_folder(&self, folder: &str) -> bool {
        folder.is_empty()
            || self.folders.contains(folder)
            || self.paths.iter().any(|path| path.starts_with(folder) && path[folder.len()..].starts_with('/'))
    }

    fn exists_file(&self, path: &str) -> bool {
        self.paths.contains(path) && !self.folders.contains(path)
    }

    fn folder_node(&mut self, folder: &str) -> String {
        let id = folder_id(folder);
        if !self.index.nodes.iter().any(|node| node.id == id) {
            let name = if folder.is_empty() { ".".to_owned() } else { folder.to_owned() };
            self.index.nodes.push(IndexNode {
                id: id.clone(),
                kind: "Folder".to_owned(),
                group: "files",
                name,
                namespace: String::new(),
                path: folder.to_owned(),
                start: 0,
                end: 0,
                detail: String::new(),
                in_graph: true,
            });
        }
        id
    }

    fn problem(&mut self, path: &str, node: &Node, error: bool, message: String) {
        self.index.problems.push(Problem { path: path.to_owned(), start: node.start, end: node.end, error, message });
    }

    fn reference(&mut self, path: &str, node: &Node, target: &str) {
        if node.end > node.start {
            self.index.references.push(Reference {
                path: path.to_owned(),
                start: node.start,
                end: node.end,
                target: target.to_owned(),
            });
        }
    }

    fn find(&self, kinds: &[&str], name: &str, namespace: &str) -> Option<String> {
        let candidates: Vec<&IndexNode> = self
            .index
            .nodes
            .iter()
            .filter(|node| node.group != "files" && kinds.contains(&node.kind.as_str()) && node.name == name)
            .collect();
        candidates
            .iter()
            .find(|node| node.namespace == namespace)
            .or_else(|| candidates.iter().find(|node| node.namespace.is_empty() || namespace.is_empty()))
            .map(|node| node.id.clone())
    }
}

fn namespace_of_folder(folder: &str, namespaces: &BTreeMap<String, String>) -> String {
    namespaces.get(folder).cloned().unwrap_or_default()
}

pub fn same_repository(a: &str, b: &str) -> bool {
    fn plain(url: &str) -> String {
        let url = url.trim().to_ascii_lowercase();
        let url = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest).to_owned();
        let url = url.split_once('@').map_or(url.as_str(), |(_, rest)| rest).to_owned();
        let url = match url.split_once(':') {
            Some((host, rest))
                if !rest.starts_with('/') && !rest.chars().next().is_some_and(|c| c.is_ascii_digit()) =>
            {
                format!("{host}/{rest}")
            }
            _ => url,
        };
        let url = url.trim_end_matches('/');
        url.strip_suffix(".git").unwrap_or(url).trim_end_matches('/').to_owned()
    }
    !a.trim().is_empty() && plain(a) == plain(b)
}

pub fn build(files: &[File], paths: &[String]) -> Index {
    build_with(files, paths, &[])
}

pub fn build_with(files: &[File], paths: &[String], remotes: &[String]) -> Index {
    let parsed: Vec<(&File, Vec<Document>)> = files.iter().map(|file| (file, yaml::documents(&file.text))).collect();
    let mut builder = Builder {
        index: Index::default(),
        paths: paths.iter().map(String::as_str).collect(),
        folders: BTreeSet::new(),
        namespaces: BTreeMap::new(),
        remotes: remotes.to_vec(),
    };
    for path in paths {
        let mut folder = folder_of(path);
        while !folder.is_empty() {
            builder.folders.insert(folder.to_owned());
            folder = folder_of(folder);
        }
    }
    for (file, docs) in &parsed {
        if is_kustomization_file(&file.path)
            && let Some(namespace) = docs.first().and_then(|doc| doc.root.get("namespace")).and_then(Node::str)
        {
            builder.namespaces.insert(folder_of(&file.path).to_owned(), namespace.to_owned());
        }
    }
    let charts = helm::chart_folders(files);
    let mut objects: Vec<Object> = Vec::new();
    for (file, docs) in &parsed {
        if helm::chart_of_template(&charts, &file.path).is_some() {
            continue;
        }
        for doc in docs {
            let api_version = doc.root.get("apiVersion").and_then(Node::str).unwrap_or("");
            let kind = doc.root.get("kind").and_then(Node::str).unwrap_or("");
            let kustomize_file = is_kustomization_file(&file.path) && (api_version.is_empty() || kind.is_empty());
            let group = match group_of(api_version, kind) {
                Some(group) => group,
                None if kustomize_file => "kustomize",
                None if file_name(&file.path) == "Chart.yaml" => "helm",
                None => continue,
            };
            builder.index.kinds.insert(group);
            let folder = folder_of(&file.path).to_owned();
            let (kind, name, id) = match group {
                "kustomize" => ("Kustomize".to_owned(), folder.clone(), format!("kustomize:{folder}")),
                "helm" => {
                    let name = doc.root.get("name").and_then(Node::str).unwrap_or(&folder).to_owned();
                    ("Chart".to_owned(), name, format!("chart:{folder}"))
                }
                _ => {
                    let name = doc.root.at(&["metadata", "name"]).and_then(Node::str).unwrap_or("").to_owned();
                    let namespace = doc
                        .root
                        .at(&["metadata", "namespace"])
                        .and_then(Node::str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| namespace_of_folder(&folder, &builder.namespaces));
                    let id = format!("{group}:{kind}/{}/{name}", if namespace.is_empty() { "-" } else { &namespace });
                    (kind.to_owned(), name, id)
                }
            };
            let namespace = if matches!(group, "flux" | "argocd") {
                doc.root
                    .at(&["metadata", "namespace"])
                    .and_then(Node::str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| namespace_of_folder(&folder, &builder.namespaces))
            } else {
                String::new()
            };
            let detail = detail_of(group, &kind, &doc.root);
            let at = builder.index.nodes.len();
            builder.index.nodes.push(IndexNode {
                id,
                kind,
                group,
                name,
                namespace,
                path: file.path.clone(),
                start: doc.root.start,
                end: doc.root.end,
                detail,
                in_graph: true,
            });
            if matches!(group, "flux" | "argocd")
                && let Some(name) = doc.root.at(&["metadata", "name"])
            {
                let id = builder.index.nodes[at].id.clone();
                builder.reference(&file.path, name, &id);
            }
            objects.push(Object { node: at, doc, path: &file.path });
        }
    }
    duplicates(&mut builder, &objects);
    for object in &objects {
        let node = builder.index.nodes[object.node].clone();
        match (node.group, node.kind.as_str()) {
            ("flux", "Kustomization") => flux_kustomization(&mut builder, object, &node),
            ("flux", "HelmRelease") => helm_release(&mut builder, object, &node),
            ("argocd", "Application") => application(&mut builder, object, &node, &object.doc.root.get("spec")),
            ("argocd", "ApplicationSet") => application_set(&mut builder, object, &node),
            ("kustomize", _) => kustomize(&mut builder, object, &node),
            _ => {}
        }
    }
    let folders: Vec<IndexNode> = builder.index.nodes.iter().filter(|node| node.group == "files").cloned().collect();
    for folder in folders {
        for object in &objects {
            let node = &builder.index.nodes[object.node];
            if node.group != "files" && folder_of(object.path) == folder.path {
                let edge = Edge { from: folder.id.clone(), to: node.id.clone(), kind: "resource" };
                builder.index.edges.insert(edge);
            }
        }
    }
    app_of_apps(&mut builder);
    cycles(&mut builder);
    helm::values(&mut builder, &parsed, &charts);
    builder.index
}

fn detail_of(group: &str, kind: &str, root: &Node) -> String {
    let spec = |keys: &[&str]| root.at(keys).and_then(Node::str).unwrap_or("").to_owned();
    match (group, kind) {
        ("flux", "GitRepository" | "OCIRepository" | "HelmRepository" | "Bucket") => spec(&["spec", "url"]),
        ("flux", "Kustomization") => spec(&["spec", "path"]),
        ("flux", "HelmRelease") => spec(&["spec", "chart", "spec", "chart"]),
        ("argocd", "Application") => {
            let path = spec(&["spec", "source", "path"]);
            if path.is_empty() { spec(&["spec", "source", "chart"]) } else { path }
        }
        ("helm", _) => spec(&["version"]),
        _ => String::new(),
    }
}

fn duplicates(builder: &mut Builder, objects: &[Object]) {
    let mut seen: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for object in objects {
        let node = &builder.index.nodes[object.node];
        if !matches!(node.group, "flux" | "argocd") || node.name.is_empty() {
            continue;
        }
        let key = (node.kind.clone(), node.namespace.clone(), node.name.clone());
        *seen.entry(key).or_default() += 1;
    }
    for object in objects {
        let node = builder.index.nodes[object.node].clone();
        let key = (node.kind.clone(), node.namespace.clone(), node.name.clone());
        if seen.get(&key).is_some_and(|count| *count > 1)
            && let Some(name) = object.doc.root.at(&["metadata", "name"])
        {
            let message = format!("{} {} is declared more than once in this namespace", node.kind, node.name);
            builder.problem(object.path, name, true, message);
        }
    }
}

fn source_ref(
    builder: &mut Builder,
    object: &Object,
    node: &IndexNode,
    reference: &Node,
    kinds: &[&str],
    default: &str,
) {
    let kind = reference.get("kind").and_then(Node::str).unwrap_or(default);
    let Some(name_node) = reference.get("name") else { return };
    let Some(name) = name_node.str() else { return };
    let namespace = reference.get("namespace").and_then(Node::str).unwrap_or(&node.namespace).to_owned();
    let allowed: Vec<&str> = if kinds.contains(&kind) { vec![kind] } else { kinds.to_vec() };
    match builder.find(&allowed, name, &namespace) {
        Some(target) => {
            builder.index.edges.insert(Edge { from: node.id.clone(), to: target.clone(), kind: "source" });
            builder.reference(object.path, name_node, &target);
        }
        None => {
            let message = format!("No {kind} named {name} in this repository");
            builder.problem(object.path, name_node, true, message);
        }
    }
}

fn depends_on(builder: &mut Builder, object: &Object, node: &IndexNode, spec: Option<&Node>) {
    let Some(spec) = spec else { return };
    for item in spec.get("dependsOn").map(Node::items).unwrap_or_default() {
        let Some(name_node) = item.get("name") else { continue };
        let Some(name) = name_node.str() else { continue };
        let namespace = item.get("namespace").and_then(Node::str).unwrap_or(&node.namespace).to_owned();
        match builder.find(&[node.kind.as_str()], name, &namespace) {
            Some(target) => {
                builder.index.edges.insert(Edge { from: node.id.clone(), to: target.clone(), kind: "dependsOn" });
                builder.reference(object.path, name_node, &target);
            }
            None => {
                let message = format!("No {} named {name} to depend on in this repository", node.kind);
                builder.problem(object.path, name_node, true, message);
            }
        }
    }
}

fn path_edge(builder: &mut Builder, object: &Object, node: &IndexNode, path_node: &Node, base: &str, error: bool) {
    let Some(path) = path_node.str() else { return };
    if path.contains("{{") {
        return;
    }
    let folder = normalise(base, path);
    if builder.exists_folder(&folder) {
        let target = builder.folder_node(&folder);
        builder.index.edges.insert(Edge { from: node.id.clone(), to: target.clone(), kind: "path" });
        builder.reference(object.path, path_node, &target);
    } else {
        let message = format!("No folder {path} in this repository");
        builder.problem(object.path, path_node, error, message);
    }
}

fn flux_kustomization(builder: &mut Builder, object: &Object, node: &IndexNode) {
    let spec = object.doc.root.get("spec");
    let source = spec.and_then(|spec| spec.get("sourceRef"));
    let mut this_repository = true;
    if let Some(source) = source {
        let name = source.get("name").and_then(Node::str).unwrap_or("");
        let kind = source.get("kind").and_then(Node::str).unwrap_or("GitRepository");
        this_repository =
            kind == "GitRepository" && (name == "flux-system" || builder.find(&[kind], name, "").is_none());
        source_ref(builder, object, node, source, &["GitRepository", "OCIRepository", "Bucket"], "GitRepository");
    }
    if let Some(path) = spec.and_then(|spec| spec.get("path")) {
        path_edge(builder, object, node, path, "", this_repository);
    }
    depends_on(builder, object, node, spec);
}

fn helm_release(builder: &mut Builder, object: &Object, node: &IndexNode) {
    let spec = object.doc.root.get("spec");
    if let Some(chart) = spec.and_then(|spec| spec.at(&["chart", "spec"])) {
        match chart.get("sourceRef") {
            Some(source) => {
                source_ref(
                    builder,
                    object,
                    node,
                    source,
                    &["HelmRepository", "GitRepository", "Bucket"],
                    "HelmRepository",
                );
                let kind = source.get("kind").and_then(Node::str).unwrap_or("HelmRepository");
                if matches!(kind, "GitRepository" | "Bucket")
                    && let Some(chart_node) = chart.get("chart")
                    && let Some(path) = chart_node.str()
                {
                    let folder = normalise("", path);
                    let id = format!("chart:{folder}");
                    if builder.index.nodes.iter().any(|known| known.id == id) {
                        builder.index.edges.insert(Edge { from: node.id.clone(), to: id.clone(), kind: "chart" });
                        builder.reference(object.path, chart_node, &id);
                    }
                }
            }
            None => {
                let message =
                    "This HelmRelease names no repository for its chart (spec.chart.spec.sourceRef)".to_owned();
                builder.problem(object.path, chart, true, message);
            }
        }
    }
    if let Some(chart_ref) = spec.and_then(|spec| spec.get("chartRef")) {
        source_ref(builder, object, node, chart_ref, &["OCIRepository", "HelmChart"], "OCIRepository");
    }
    depends_on(builder, object, node, spec);
}

fn application(builder: &mut Builder, object: &Object, node: &IndexNode, spec: &Option<&Node>) {
    let Some(spec) = spec else { return };
    let sources: Vec<&Node> =
        spec.get("source").into_iter().chain(spec.get("sources").map(Node::items).unwrap_or_default()).collect();
    for source in sources {
        if let Some(path) = source.get("path") {
            let url = source.get("repoURL").and_then(Node::str).unwrap_or("");
            let here = builder.remotes.iter().any(|remote| same_repository(remote, url));
            path_edge(builder, object, node, path, "", here);
        }
        if let Some(chart) = source.get("chart").and_then(Node::str) {
            let id = format!("chart:{}", normalise("", chart));
            if builder.index.nodes.iter().any(|known| known.id == id) {
                builder.index.edges.insert(Edge { from: node.id.clone(), to: id, kind: "chart" });
            }
        }
    }
    if let Some(project_node) = spec.get("project")
        && let Some(project) = project_node.str()
        && !project.contains("{{")
    {
        match builder.find(&["AppProject"], project, "") {
            Some(target) => {
                builder.index.edges.insert(Edge { from: node.id.clone(), to: target.clone(), kind: "project" });
                builder.reference(object.path, project_node, &target);
            }
            None if project != "default" => {
                let message = format!("No AppProject named {project} in this repository");
                builder.problem(object.path, project_node, false, message);
            }
            None => {}
        }
    }
}

fn glob(pattern: &str, path: &str) -> bool {
    let (pattern, path) = (pattern.as_bytes(), path.as_bytes());
    fn go(pattern: &[u8], path: &[u8]) -> bool {
        match pattern.split_first() {
            None => path.is_empty(),
            Some((b'*', rest)) if rest.first() == Some(&b'*') => (0..=path.len()).any(|at| go(&rest[1..], &path[at..])),
            Some((b'*', rest)) => {
                (0..=path.len()).take_while(|at| *at == 0 || path[at - 1] != b'/').any(|at| go(rest, &path[at..]))
            }
            Some((byte, rest)) => path.first() == Some(byte) && go(rest, &path[1..]),
        }
    }
    go(pattern, path)
}

fn application_set(builder: &mut Builder, object: &Object, node: &IndexNode) {
    let spec = object.doc.root.get("spec");
    if let Some(template) = spec.and_then(|spec| spec.at(&["template", "spec"])) {
        application(builder, object, node, &Some(template));
    }
    let generators = spec.and_then(|spec| spec.get("generators")).map(Node::items).unwrap_or_default();
    for generator in generators {
        for directory in generator.at(&["git", "directories"]).map(Node::items).unwrap_or_default() {
            let Some(pattern_node) = directory.get("path") else { continue };
            let Some(pattern) = pattern_node.str() else { continue };
            if directory.get("exclude").and_then(Node::str) == Some("true") {
                continue;
            }
            let pattern = normalise("", pattern);
            let matched: Vec<String> =
                builder.folders.iter().filter(|folder| glob(&pattern, folder)).cloned().collect();
            for folder in matched {
                let target = builder.folder_node(&folder);
                builder.index.edges.insert(Edge { from: node.id.clone(), to: target, kind: "path" });
            }
        }
    }
}

fn kustomize(builder: &mut Builder, object: &Object, node: &IndexNode) {
    let folder = folder_of(object.path).to_owned();
    let own = builder.folder_node(&folder);
    builder.index.edges.insert(Edge { from: own, to: node.id.clone(), kind: "resource" });
    for key in ["resources", "components", "bases"] {
        for entry in object.doc.root.get(key).map(Node::items).unwrap_or_default() {
            let Some(name) = entry.str() else { continue };
            if crate::targets::is_remote(name) {
                continue;
            }
            let path = normalise(&folder, name);
            if builder.exists_file(&path) {
                let targets: Vec<String> = builder
                    .index
                    .nodes
                    .iter()
                    .filter(|known| known.path == path && known.group != "files")
                    .map(|known| known.id.clone())
                    .collect();
                for target in &targets {
                    builder.index.edges.insert(Edge { from: node.id.clone(), to: target.clone(), kind: "resource" });
                }
                if let Some(first) = targets.first() {
                    builder.reference(object.path, entry, first);
                }
            } else if builder.exists_folder(&path) {
                let target = builder.folder_node(&path);
                builder.index.edges.insert(Edge { from: node.id.clone(), to: target.clone(), kind: "resource" });
                builder.reference(object.path, entry, &target);
            } else {
                let message = format!("No file or folder {name} next to this kustomization");
                builder.problem(object.path, entry, true, message);
            }
        }
    }
    let root = &object.doc.root;
    let named =
        |key: &str| root.get(key).map(Node::items).unwrap_or_default().iter().filter_map(|item| item.get("path"));
    let patches: Vec<&Node> = named("patches")
        .chain(named("patchesJson6902"))
        .chain(root.get("patchesStrategicMerge").map(Node::items).unwrap_or_default().iter())
        .collect();
    for entry in patches {
        let Some(name) = entry.str().filter(|name| !name.contains('\n') && !name.trim().is_empty()) else { continue };
        if !builder.exists_file(&normalise(&folder, name)) {
            builder.problem(object.path, entry, true, format!("No patch file {name} next to this kustomization"));
        }
    }
}

fn app_of_apps(builder: &mut Builder) {
    let parents: Vec<(String, String)> = builder
        .index
        .edges
        .iter()
        .filter(|edge| edge.kind == "path" && edge.from.starts_with("argocd:Application/"))
        .map(|edge| (edge.from.clone(), edge.to.trim_start_matches("path:").to_owned()))
        .collect();
    for (parent, folder) in parents {
        let children: Vec<String> = builder
            .index
            .nodes
            .iter()
            .filter(|node| {
                node.group == "argocd"
                    && matches!(node.kind.as_str(), "Application" | "ApplicationSet")
                    && node.id != parent
                    && (folder.is_empty() || node.path.starts_with(&format!("{folder}/")))
            })
            .map(|node| node.id.clone())
            .collect();
        for child in children {
            builder.index.edges.insert(Edge { from: parent.clone(), to: child, kind: "child" });
        }
    }
}

fn cycles(builder: &mut Builder) {
    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in builder.index.edges.iter().filter(|edge| edge.kind == "dependsOn") {
        graph.entry(edge.from.as_str()).or_default().push(edge.to.as_str());
    }
    let mut in_cycle: BTreeSet<String> = BTreeSet::new();
    for start in graph.keys() {
        let mut stack = vec![*start];
        let mut seen = BTreeSet::new();
        while let Some(at) = stack.pop() {
            for next in graph.get(at).into_iter().flatten() {
                if next == start {
                    in_cycle.insert((*start).to_owned());
                } else if seen.insert(*next) {
                    stack.push(next);
                }
            }
        }
    }
    let nodes: Vec<IndexNode> =
        builder.index.nodes.iter().filter(|node| in_cycle.contains(&node.id)).cloned().collect();
    for node in nodes {
        builder.index.problems.push(Problem {
            path: node.path.clone(),
            start: node.start,
            end: node.start.min(node.end),
            error: false,
            message: format!("{} {} depends on itself through dependsOn", node.kind, node.name),
        });
    }
}

pub fn to_json(index: &Index) -> Json {
    json!({
        "kinds": index.kinds.iter().collect::<Vec<_>>(),
        "nodes": index.nodes.iter().map(|node| json!({
            "id": node.id, "kind": node.kind, "group": node.group, "name": node.name,
            "namespace": node.namespace, "path": node.path, "start": node.start, "end": node.end,
            "detail": node.detail, "inGraph": node.in_graph,
        })).collect::<Vec<_>>(),
        "edges": index.edges.iter().map(|edge| json!({ "from": edge.from, "to": edge.to, "kind": edge.kind })).collect::<Vec<_>>(),
        "references": index.references.iter().map(|reference| json!({
            "path": reference.path, "start": reference.start, "end": reference.end, "target": reference.target,
        })).collect::<Vec<_>>(),
        "problems": index.problems.iter().map(|problem| json!({
            "path": problem.path, "start": problem.start, "end": problem.end,
            "severity": if problem.error { "error" } else { "warning" }, "message": problem.message,
        })).collect::<Vec<_>>(),
        "completions": index.completions.iter().map(|completion| json!({
            "files": completion.files, "after": completion.after,
            "items": completion.items.iter().map(|(label, detail)| json!({ "label": label, "detail": detail })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}
