use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::index::{self, Snapshot};
use super::source::{Bean, BeanKind, Injection};

const MAX_WAIT: Duration = Duration::from_secs(120);

/// A place a gutter mark leads to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MarkerTarget {
    /// The file.
    pub path: PathBuf,
    /// The line, one-based.
    pub line: u32,
    /// What is there, e.g. `OrderService` or `OrderController.orders`.
    pub name: String,
    /// More about it, e.g. its type.
    pub detail: String,
}

/// A gutter mark of a line: a bean with the places it is injected into, or an injection point with
/// the beans that can fill it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineMarker {
    /// The line, one-based.
    pub line: u32,
    /// `bean` (a bean's definition) or `injection` (a place a bean is injected into).
    pub kind: String,
    /// What the mark says when pointed at.
    pub tooltip: String,
    /// Where it leads.
    pub targets: Vec<MarkerTarget>,
}

fn matches(bean: &Bean, injection: &Injection) -> bool {
    bean.type_name == injection.type_name || bean.supertypes.contains(&injection.type_name)
}

pub(crate) fn markers(snapshot: &Snapshot, path: &Path) -> Vec<LineMarker> {
    let Some((_, facts)) = snapshot.files.iter().find(|(file, _)| file == path) else { return Vec::new() };
    let mut found: Vec<LineMarker> = Vec::new();
    for bean in &facts.beans {
        let targets: Vec<MarkerTarget> = snapshot
            .files
            .iter()
            .flat_map(|(file, facts)| {
                facts.injections.iter().filter(|injection| matches(bean, injection)).map(move |injection| {
                    MarkerTarget {
                        path: file.clone(),
                        line: injection.line + 1,
                        name: format!("{}.{}", injection.owner, injection.name),
                        detail: injection.type_name.clone(),
                    }
                })
            })
            .collect();
        let tooltip = match bean.kind {
            BeanKind::Component => t!("workbench-spring-boot-bean", name = bean.name.clone()),
            BeanKind::Method => t!("workbench-spring-boot-bean-method", name = bean.name.clone()),
        };
        found.push(LineMarker { line: bean.line + 1, kind: "bean".to_owned(), tooltip, targets });
    }
    for injection in &facts.injections {
        let targets: Vec<MarkerTarget> = snapshot
            .files
            .iter()
            .flat_map(|(file, facts)| {
                facts.beans.iter().filter(|bean| matches(bean, injection)).map(move |bean| MarkerTarget {
                    path: file.clone(),
                    line: bean.line + 1,
                    name: bean.name.clone(),
                    detail: bean.type_name.clone(),
                })
            })
            .collect();
        if targets.is_empty() {
            continue;
        }
        let tooltip = t!("workbench-spring-boot-injection", name = injection.type_name.clone());
        found.push(LineMarker { line: injection.line + 1, kind: "injection".to_owned(), tooltip, targets });
    }
    found.sort_by_key(|marker| (marker.line, marker.kind != "bean"));
    let mut merged: Vec<LineMarker> = Vec::new();
    for marker in found {
        match merged.last_mut() {
            Some(last) if last.line == marker.line => {
                for target in marker.targets {
                    if !last.targets.contains(&target) {
                        last.targets.push(target);
                    }
                }
            }
            _ => merged.push(marker),
        }
    }
    merged
}

/// The parameters of `editor.lineMarkers.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MarkersParams {
    /// The file; the active one when absent.
    pub path: Option<PathBuf>,
    /// Waits up to this long for the project's sources to be read (at most two minutes).
    pub wait_ms: Option<u64>,
}

/// What `editor.lineMarkers.list` answers.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MarkersAnswer {
    /// A number that changes whenever the marks may have.
    pub generation: u64,
    /// The marks, by line.
    pub markers: Vec<LineMarker>,
}

fn path_of(session: &Session, path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    match path {
        Some(path) => Ok(path),
        None => session
            .state::<crate::Workbench>()
            .and_then(crate::Workbench::active_document)
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document"))),
    }
}

fn wait(ms: Option<u64>) -> Duration {
    Duration::from_millis(ms.unwrap_or(0)).min(MAX_WAIT)
}

pub(crate) fn list(session: &mut Session, path: &Path, wait: Duration) -> MarkersAnswer {
    if !super::source::is_source(path) {
        return MarkersAnswer::default();
    }
    let Some(snapshot) = index::snapshot(session, wait) else { return MarkersAnswer::default() };
    MarkersAnswer { generation: snapshot.generation, markers: markers(&snapshot, path) }
}

pub struct ListMarkers;

impl Command for ListMarkers {
    type Params = MarkersParams;
    type Output = MarkersAnswer;
    const ID: &'static str = "editor.lineMarkers.list";
    const TITLE: &'static str = "Gutter Navigation Marks";
    const DESCRIPTION: &'static str = "The navigation marks of a file's gutter, each with the places it leads to: with \
         the Spring Boot plug-in, a bean (a @Component, @Service, @Repository, @Controller, @RestController or \
         @Configuration class, or a @Bean method) with the places it is injected into, and an injection point \
         (a constructor parameter of a bean, an @Autowired field or setter) with the beans of its type. \
         editor.lineMarkers.navigate goes to one.";

    fn run(&self, session: &mut Session, params: MarkersParams) -> Result<MarkersAnswer, CommandError> {
        let path = path_of(session, params.path)?;
        Ok(list(session, &path, wait(params.wait_ms)))
    }
}

/// The parameters of `editor.lineMarkers.navigate`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NavigateParams {
    /// The file; the active one when absent.
    pub path: Option<PathBuf>,
    /// The mark's line, one-based.
    pub line: u32,
    /// Which of its targets, from 0; may be left out when it has one.
    pub index: Option<usize>,
}

/// What `editor.lineMarkers.navigate` did.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Navigated {
    /// Whether a target was opened.
    pub navigated: bool,
    /// The targets to choose from when the mark has several and no index was given.
    pub targets: Vec<MarkerTarget>,
}

fn line_offset(session: &Session, path: &Path, line: u32) -> usize {
    let text = session
        .state::<crate::Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| document.text().to_owned())
        .unwrap_or_else(|| std::fs::read_to_string(path).unwrap_or_default());
    let mut offset = 0;
    for (index, text_line) in text.split_inclusive('\n').enumerate() {
        if index + 1 >= line as usize {
            return offset + text_line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        }
        offset += text_line.chars().count();
    }
    offset
}

pub(crate) fn open(session: &mut Session, path: &Path, line: u32) -> Result<(), CommandError> {
    let offset = line_offset(session, path, line);
    let params = crate::history::NavigateToParams { path: path.to_owned(), offset, end: None };
    crate::history::NavigateTo.run(session, params).map(|_| ())
}

pub struct NavigateMarker;

impl Command for NavigateMarker {
    type Params = NavigateParams;
    type Output = Navigated;
    const ID: &'static str = "editor.lineMarkers.navigate";
    const TITLE: &'static str = "Go to Gutter Mark Target";
    const DESCRIPTION: &'static str = "Opens a target of the gutter mark on a line (editor.lineMarkers.list): the only \
         one, or the one `index` names; with several and no index, answers them to choose from.";

    fn run(&self, session: &mut Session, params: NavigateParams) -> Result<Navigated, CommandError> {
        let path = path_of(session, params.path)?;
        let answer = list(session, &path, Duration::ZERO);
        let marker = answer
            .markers
            .into_iter()
            .find(|marker| marker.line == params.line)
            .ok_or_else(|| CommandError::new(t!("workbench-spring-boot-no-marker", line = params.line)))?;
        let chosen = match params.index {
            Some(index) => Some(
                marker
                    .targets
                    .get(index)
                    .cloned()
                    .ok_or_else(|| CommandError::new(t!("workbench-spring-boot-no-target", index = index)))?,
            ),
            None if marker.targets.len() == 1 => marker.targets.first().cloned(),
            None if marker.targets.is_empty() => {
                return Err(CommandError::new(t!("workbench-spring-boot-not-injected")));
            }
            None => None,
        };
        match chosen {
            Some(target) => {
                open(session, &target.path, target.line)?;
                Ok(Navigated { navigated: true, targets: Vec::new() })
            }
            None => Ok(Navigated { navigated: false, targets: marker.targets }),
        }
    }
}

/// A request mapping of the project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndpointInfo {
    /// The HTTP method, e.g. `GET`; empty when the mapping takes every method.
    pub method: String,
    /// The full path: the class's @RequestMapping path and the method's.
    pub path: String,
    /// The handler, `Class.method`.
    pub handler: String,
    /// Its file.
    pub file: PathBuf,
    /// Its line, one-based.
    pub line: u32,
}

/// The parameters of `springBoot.endpoints.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EndpointsParams {
    /// Waits up to this long for the project's sources to be read (at most two minutes).
    pub wait_ms: Option<u64>,
}

/// What `springBoot.endpoints.list` answers.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndpointList {
    /// Whether the project's sources are still being read for the first time.
    pub loading: bool,
    /// A number that changes whenever the list may have.
    pub generation: u64,
    /// The mappings, by path, then method.
    pub endpoints: Vec<EndpointInfo>,
}

pub(crate) fn endpoints(session: &mut Session, wait: Duration) -> Option<EndpointList> {
    let snapshot = index::snapshot(session, wait)?;
    let mut endpoints: Vec<EndpointInfo> = snapshot
        .files
        .iter()
        .flat_map(|(file, facts)| {
            facts.endpoints.iter().map(move |endpoint| EndpointInfo {
                method: endpoint.method.clone(),
                path: endpoint.path.clone(),
                handler: endpoint.handler.clone(),
                file: file.clone(),
                line: endpoint.line + 1,
            })
        })
        .collect();
    endpoints.sort_by(|a, b| (&a.path, &a.method, &a.handler).cmp(&(&b.path, &b.method, &b.handler)));
    Some(EndpointList { loading: !snapshot.scanned, generation: snapshot.generation, endpoints })
}

pub struct ListEndpoints;

impl Command for ListEndpoints {
    type Params = EndpointsParams;
    type Output = EndpointList;
    const ID: &'static str = "springBoot.endpoints.list";
    const TITLE: &'static str = "Spring Boot Endpoints";
    const DESCRIPTION: &'static str = "The project's request mappings, as the Endpoints tool window lists them: \
         @GetMapping, @PostMapping, @PutMapping, @DeleteMapping, @PatchMapping and @RequestMapping methods of Java \
         and Kotlin classes, each with its HTTP method, its full path (the class's @RequestMapping path first) and \
         its handler; navigate.place.goTo or springBoot.endpoints.open goes to it. Needs the Spring Boot plug-in.";

    fn run(&self, session: &mut Session, params: EndpointsParams) -> Result<EndpointList, CommandError> {
        endpoints(session, wait(params.wait_ms)).ok_or_else(|| CommandError::new(t!("workbench-spring-boot-off")))
    }
}

/// The parameters of `springBoot.endpoints.open`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenEndpointParams {
    /// The handler, `Class.method`, as `springBoot.endpoints.list` gives it.
    pub handler: String,
    /// The path, when the handler has several mappings.
    pub path: Option<String>,
}

pub struct OpenEndpoint;

impl Command for OpenEndpoint {
    type Params = OpenEndpointParams;
    type Output = Navigated;
    const ID: &'static str = "springBoot.endpoints.open";
    const TITLE: &'static str = "Open Spring Boot Endpoint";
    const DESCRIPTION: &'static str = "Opens the handler method of a request mapping of springBoot.endpoints.list.";

    fn run(&self, session: &mut Session, params: OpenEndpointParams) -> Result<Navigated, CommandError> {
        let list =
            endpoints(session, Duration::ZERO).ok_or_else(|| CommandError::new(t!("workbench-spring-boot-off")))?;
        let endpoint = list
            .endpoints
            .into_iter()
            .find(|endpoint| {
                endpoint.handler == params.handler && params.path.as_ref().is_none_or(|path| *path == endpoint.path)
            })
            .ok_or_else(|| {
                CommandError::new(t!("workbench-spring-boot-no-endpoint", handler = params.handler.clone()))
            })?;
        open(session, &endpoint.file, endpoint.line)?;
        Ok(Navigated { navigated: true, targets: Vec::new() })
    }
}

pub struct RefreshEndpoints;

impl Command for RefreshEndpoints {
    type Params = zen_core::NoParams;
    type Output = EndpointList;
    const ID: &'static str = "springBoot.endpoints.refresh";
    const TITLE: &'static str = "Refresh Spring Boot Endpoints";
    const DESCRIPTION: &'static str = "Reads the project's Java and Kotlin sources again for the Endpoints tool window \
         and the bean marks, then answers springBoot.endpoints.list.";

    fn run(&self, session: &mut Session, _params: zen_core::NoParams) -> Result<EndpointList, CommandError> {
        index::force_rescan(session);
        endpoints(session, MAX_WAIT).ok_or_else(|| CommandError::new(t!("workbench-spring-boot-off")))
    }
}
