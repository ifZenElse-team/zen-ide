use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_modules::form::{Action, Choice, Form, Item, MenuEntry, Tone};

use super::{Jdk, JdkSource, Jdks, ProjectJdk, adoptium};
use crate::Workbench;
use crate::notifications::{self, Group, Kind, NewNotification};
use crate::tasks::Tasks;

pub const PAGE: &str = "jdk";

pub const FORM: &str = "toolchain.jdk.settings";

pub const OFFERED: [u32; 5] = [25, 21, 17, 11, 8];

/// The JDKs and the project's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JdkList {
    /// Every JDK found on this computer, added by hand or downloaded.
    pub jdks: Vec<Jdk>,
    /// The project's JDK and language level, when there is a project and a JDK.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectJdk>,
    /// Where downloaded JDKs go.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_folder: Option<PathBuf>,
    /// The feature release being downloaded now, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloading: Option<u32>,
}

/// Which project a command is about.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectParams {
    /// The project's root folder; the window's project when absent.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

fn root(session: &Session, given: Option<PathBuf>) -> Option<PathBuf> {
    given.or_else(|| session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned))
}

fn list(session: &mut Session, project: Option<&Path>) -> JdkList {
    let jdks = session.state_mut::<Jdks>().clone();
    let mut store = jdks.store();
    JdkList {
        jdks: store.jdks(),
        project: project.and_then(|root| store.project(root)),
        download_folder: store.download_folder(),
        downloading: store.downloading,
    }
}

pub struct ListJdks;

impl Command for ListJdks {
    type Params = ProjectParams;
    type Output = JdkList;
    const ID: &'static str = "toolchain.jdk.list";
    const TITLE: &'static str = "List JDKs";
    const DESCRIPTION: &'static str = "The JDKs on this computer (JAVA_HOME, /usr/libexec/java_home, \
         /Library/Java/JavaVirtualMachines, Homebrew, SDKMAN, asdf, mise, Gradle's and Maven's toolchains, added by \
         hand, downloaded by the IDE: home, version, feature release, vendor, source) and the project's JDK and \
         language level (`chosen` false: the installed JDK of the level the build asks for, buildLevel, else the \
         default, JAVA_HOME's else the newest).";

    fn run(&self, session: &mut Session, params: ProjectParams) -> Result<JdkList, CommandError> {
        let root = root(session, params.project);
        Ok(list(session, root.as_deref()))
    }
}

pub struct RefreshJdks;

impl Command for RefreshJdks {
    type Params = NoParams;
    type Output = JdkList;
    const ID: &'static str = "toolchain.jdk.refresh";
    const TITLE: &'static str = "Refresh JDKs";
    const DESCRIPTION: &'static str = "Looks for JDKs on this computer again (after installing one outside the IDE).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<JdkList, CommandError> {
        session.state_mut::<Jdks>().clone().store().refresh();
        let root = root(session, None);
        Ok(list(session, root.as_deref()))
    }
}

/// The parameters of `toolchain.jdk.add` and `toolchain.jdk.remove`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HomeParams {
    /// The JDK's home folder (the one with `bin/java`), or its macOS bundle (`….jdk`).
    pub home: PathBuf,
}

fn person_only(session: &Session) -> Result<(), CommandError> {
    if zen_core::macros::by_agent(session) {
        return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("jdk-agent-refused")));
    }
    Ok(())
}

pub struct AddJdk;

impl Command for AddJdk {
    type Params = HomeParams;
    type Output = JdkList;
    const ID: &'static str = "toolchain.jdk.add";
    const TITLE: &'static str = "Add JDK from Disk";
    const DESCRIPTION: &'static str = "Adds a JDK by its home folder (with bin/java and a release file), kept for every \
         project.";

    fn run(&self, session: &mut Session, params: HomeParams) -> Result<JdkList, CommandError> {
        person_only(session)?;
        session
            .state_mut::<Jdks>()
            .clone()
            .store()
            .add(&params.home)
            .map_err(|home| CommandError::new(t!("jdk-not-a-jdk", path = home)))?;
        let root = root(session, None);
        Ok(list(session, root.as_deref()))
    }
}

pub struct RemoveJdk;

impl Command for RemoveJdk {
    type Params = HomeParams;
    type Output = JdkList;
    const ID: &'static str = "toolchain.jdk.remove";
    const TITLE: &'static str = "Remove JDK";
    const DESCRIPTION: &'static str = "Forgets a JDK added by hand, or deletes one the IDE downloaded (from its own \
         folder); JDKs found elsewhere stay where they are.";

    fn run(&self, session: &mut Session, params: HomeParams) -> Result<JdkList, CommandError> {
        person_only(session)?;
        if !session.state_mut::<Jdks>().clone().store().remove(&params.home) {
            return Err(CommandError::new(t!("jdk-not-removable", path = params.home.display().to_string())));
        }
        let root = root(session, None);
        Ok(list(session, root.as_deref()))
    }
}

/// The parameters of `toolchain.jdk.choose`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChooseParams {
    /// The JDK's home, from `toolchain.jdk.list`; empty or absent for the default.
    #[serde(default)]
    pub home: Option<String>,
    /// The project's root folder; the window's project when absent.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

fn project_root(session: &Session, given: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    root(session, given).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

pub struct ChooseJdk;

impl Command for ChooseJdk {
    type Params = ChooseParams;
    type Output = JdkList;
    const ID: &'static str = "toolchain.jdk.choose";
    const TITLE: &'static str = "Choose Project JDK";
    const DESCRIPTION: &'static str = "Sets the project's JDK (Project SDK) by its home from toolchain.jdk.list, or \
         back to the default with no home. Language servers and run configurations of the project use it \
         (JAVA_HOME and PATH).";

    fn run(&self, session: &mut Session, params: ChooseParams) -> Result<JdkList, CommandError> {
        person_only(session)?;
        let root = project_root(session, params.project)?;
        let home = params.home.filter(|home| !home.trim().is_empty()).map(PathBuf::from);
        session
            .state_mut::<Jdks>()
            .clone()
            .store()
            .choose(&root, home.as_deref())
            .map_err(|home| CommandError::new(t!("jdk-unknown", path = home)))?;
        Ok(list(session, Some(&root)))
    }
}

/// The parameters of `toolchain.jdk.languageLevel`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LevelParams {
    /// A feature release such as `17`, or `default` for the JDK's own.
    pub level: String,
    /// The project's root folder; the window's project when absent.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

pub struct SetLanguageLevel;

impl Command for SetLanguageLevel {
    type Params = LevelParams;
    type Output = JdkList;
    const ID: &'static str = "toolchain.jdk.languageLevel";
    const TITLE: &'static str = "Set Language Level";
    const DESCRIPTION: &'static str = "Sets the project's Java language level: a feature release from 8 up (`17`), or \
         `default` for the project JDK's own (SDK default).";

    fn run(&self, session: &mut Session, params: LevelParams) -> Result<JdkList, CommandError> {
        let root = project_root(session, params.project)?;
        let level = match params.level.trim() {
            "default" | "" => None,
            text => match text.strip_prefix("1.").unwrap_or(text).parse::<u32>() {
                Ok(level) if (MIN_LEVEL..=MAX_LEVEL).contains(&level) => Some(level),
                _ => return Err(CommandError::new(t!("jdk-bad-level", level = text))),
            },
        };
        session.state_mut::<Jdks>().clone().store().set_language_level(&root, level);
        Ok(list(session, Some(&root)))
    }
}

const MIN_LEVEL: u32 = 8;
const MAX_LEVEL: u32 = 40;

pub struct ProjectJdkCommand;

impl Command for ProjectJdkCommand {
    type Params = ProjectParams;
    type Output = Option<ProjectJdk>;
    const ID: &'static str = "toolchain.jdk.project";
    const TITLE: &'static str = "Project JDK";
    const DESCRIPTION: &'static str = "The project's JDK (home, version, feature release, vendor, whether chosen or \
         found: the installed JDK of the level the build asks for, buildLevel, else the default) and language \
         level, the one its language servers, builds and runs use; null without a JDK. A chosen JDK is never \
         replaced.";

    fn run(&self, session: &mut Session, params: ProjectParams) -> Result<Option<ProjectJdk>, CommandError> {
        let root = project_root(session, params.project)?;
        Ok(session.state_mut::<Jdks>().clone().store().project(&root))
    }
}

pub struct AvailableJdks;

impl Command for AvailableJdks {
    type Params = NoParams;
    type Output = adoptium::Releases;
    const ID: &'static str = "toolchain.jdk.available";
    const TITLE: &'static str = "List Downloadable JDKs";
    const DESCRIPTION: &'static str = "Asks the Adoptium API (network) which Eclipse Temurin feature releases can be \
         downloaded, and which of them are long-term support; download one with toolchain.jdk.download.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<adoptium::Releases, CommandError> {
        adoptium::fetch_releases().map_err(|error| CommandError::new(t!("jdk-network-failed", error = error)))
    }
}

/// The parameters of `toolchain.jdk.download`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadParams {
    /// The feature release, e.g. `21`.
    pub version: u32,
    /// The vendor; `temurin` (Eclipse Temurin), the only one for now.
    #[serde(default)]
    pub vendor: Option<String>,
    /// Make it the project's JDK once it is downloaded.
    #[serde(default, rename = "use")]
    pub use_for_project: bool,
}

/// What `toolchain.jdk.download` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadStarted {
    /// The background task that downloads it; follow it with tasks.background.list.
    pub task: u64,
    /// Where it goes.
    pub folder: PathBuf,
}

pub struct DownloadJdk;

impl Command for DownloadJdk {
    type Params = DownloadParams;
    type Output = DownloadStarted;
    const ID: &'static str = "toolchain.jdk.download";
    const TITLE: &'static str = "Download JDK";
    const DESCRIPTION: &'static str = "Downloads the newest Eclipse Temurin JDK of a feature release (network, from the \
         Adoptium API and GitHub), checks its SHA-256 and unpacks it into the IDE's own folder, in the background \
         (follow the task with tasks.background.list); `use` makes it the project's JDK when done.";

    fn run(&self, session: &mut Session, params: DownloadParams) -> Result<DownloadStarted, CommandError> {
        if let Some(vendor) = params.vendor.as_deref().filter(|vendor| *vendor != "temurin") {
            return Err(CommandError::new(t!("jdk-unknown-vendor", vendor = vendor)));
        }
        let jdks = session.state_mut::<Jdks>().clone();
        let folder = {
            let mut store = jdks.store();
            if let Some(running) = store.downloading {
                return Err(CommandError::new(t!("jdk-downloading-already", version = running)));
            }
            let folder = store.download_folder().ok_or_else(|| CommandError::new(t!("jdk-no-data-folder")))?;
            store.downloading = Some(params.version);
            folder
        };
        let project = params.use_for_project.then(|| root(session, None)).flatten();
        let task = session.state_mut::<Tasks>().start(&t!("jdk-download-task", version = params.version), "jdk", true);
        let id = task.id();
        let target = folder.clone();
        let version = params.version;
        std::thread::spawn(move || {
            task.log(&t!("jdk-download-asking", version = version));
            let result = adoptium::fetch_package(version).and_then(|package| {
                task.log(&format!("{} ({})", package.link, package.sha256));
                let total = package.size.max(1);
                adoptium::install(&package, &target, |done, _| {
                    task.progress(
                        Some(done as f32 / total as f32),
                        &t!("jdk-download-progress", release = package.release.as_str()),
                    );
                })
            });
            let mut store = jdks.store();
            store.downloading = None;
            store.refresh();
            match result {
                Ok(folder) => {
                    let home = super::detect::read(&folder, JdkSource::Downloaded).map(|jdk| jdk.home);
                    if let (Some(root), Some(home)) = (&project, &home) {
                        let _ = store.choose(root, Some(home));
                    }
                    task.finish(Ok(t!("jdk-download-done", path = folder.display().to_string())));
                }
                Err(error) => task.finish(Err(t!("jdk-network-failed", error = error))),
            }
        });
        Ok(DownloadStarted { task: id, folder })
    }
}

pub struct JdkSettings;

impl Command for JdkSettings {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = FORM;
    const TITLE: &'static str = "JDK Settings";
    const DESCRIPTION: &'static str = "Settings → Build, Execution, Deployment → JDKs as a form: the project's JDK and \
         language level, adding a JDK from disk, downloading one, and the JDKs found.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        let root = root(session, None);
        Ok(form(&list(session, root.as_deref()), root.is_some()))
    }
}

fn source_name(source: JdkSource) -> String {
    match source {
        JdkSource::JavaHome => "JAVA_HOME".to_owned(),
        JdkSource::System => t!("jdk-source-system"),
        JdkSource::Homebrew => "Homebrew".to_owned(),
        JdkSource::Sdkman => "SDKMAN!".to_owned(),
        JdkSource::Asdf => "asdf".to_owned(),
        JdkSource::Mise => "mise".to_owned(),
        JdkSource::Gradle => "Gradle".to_owned(),
        JdkSource::Maven => "Maven".to_owned(),
        JdkSource::Downloaded => t!("jdk-source-downloaded"),
        JdkSource::Added => t!("jdk-source-added"),
    }
}

fn form(list: &JdkList, has_project: bool) -> Form {
    let mut items = Vec::new();
    if has_project {
        items.push(Item::Group { title: t!("jdk-project") });
        match &list.project {
            Some(project) => {
                let mut options =
                    vec![Choice { value: String::new(), label: t!("jdk-default", jdk = default_label(list)) }];
                options.extend(list.jdks.iter().map(|jdk| Choice {
                    value: jdk.home.display().to_string(),
                    label: format!("{} — {}", jdk.label(), source_name(jdk.source)),
                }));
                items.push(Item::Choice {
                    label: t!("jdk-project-sdk"),
                    options,
                    selected: Some(if project.chosen { project.jdk.home.display().to_string() } else { String::new() }),
                    action: Action::with_value(ChooseJdk::ID, "home"),
                });
                let newest = list.jdks.iter().map(|jdk| jdk.major).max().unwrap_or(MIN_LEVEL).max(project.jdk.major);
                let mut levels = vec![Choice {
                    value: "default".to_owned(),
                    label: t!("jdk-level-default", level = project.jdk.major),
                }];
                levels.extend(
                    (MIN_LEVEL..=newest)
                        .rev()
                        .map(|level| Choice { value: level.to_string(), label: level.to_string() }),
                );
                items.push(Item::Choice {
                    label: t!("jdk-language-level"),
                    options: levels,
                    selected: Some(if project.language_level_default {
                        "default".to_owned()
                    } else {
                        project.language_level.to_string()
                    }),
                    action: Action::with_value(SetLanguageLevel::ID, "level"),
                });
                if project.language_level > project.jdk.major {
                    items.push(Item::Text {
                        text: t!("jdk-level-too-high", level = project.language_level, jdk = project.jdk.major),
                        tone: Tone::Error,
                        spinner: false,
                    });
                }
            }
            None => items.push(Item::Text { text: t!("jdk-none"), tone: Tone::Error, spinner: false }),
        }
    }
    let mut row = vec![Item::Menu {
        label: t!("jdk-add"),
        entries: std::iter::once(MenuEntry {
            label: t!("jdk-add-from-disk"),
            action: Action::with_value(AddJdk::ID, "home").picking_folder(&t!("jdk-pick-home")),
        })
        .chain(OFFERED.iter().map(|&version| MenuEntry {
            label: t!("jdk-download-version", version = version),
            action: Action::command(DownloadJdk::ID).param("version", version).param("use", has_project),
        }))
        .collect(),
    }];
    row.push(Item::Button { label: t!("jdk-refresh"), enabled: true, action: Action::command(RefreshJdks::ID) });
    items.push(Item::Row { items: row });
    if let Some(version) = list.downloading {
        items.push(Item::Text {
            text: t!("jdk-download-task", version = version),
            tone: Tone::Secondary,
            spinner: true,
        });
    }
    items.push(Item::Group { title: t!("jdk-found") });
    if list.jdks.is_empty() {
        items.push(Item::Text { text: t!("jdk-none"), tone: Tone::Secondary, spinner: false });
    }
    for jdk in &list.jdks {
        let text = Item::Text {
            text: format!("{} — {} — {}", jdk.label(), source_name(jdk.source), jdk.home.display()),
            tone: Tone::Normal,
            spinner: false,
        };
        if matches!(jdk.source, JdkSource::Added | JdkSource::Downloaded) {
            items.push(Item::Row {
                items: vec![
                    text,
                    Item::Button {
                        label: t!("jdk-remove"),
                        enabled: true,
                        action: Action::command(RemoveJdk::ID).param("home", jdk.home.display().to_string()),
                    },
                ],
            });
        } else {
            items.push(text);
        }
    }
    items.push(Item::Text { text: t!("jdk-note"), tone: Tone::Secondary, spinner: false });
    Form { items, busy: list.downloading.is_some() }
}

fn default_label(list: &JdkList) -> String {
    super::default_jdk(&list.jdks).map(Jdk::label).unwrap_or_else(|| t!("jdk-none"))
}

#[derive(Default)]
struct OfferedJdks(BTreeSet<(PathBuf, u32)>);

pub fn offer_jdk(session: &mut Session, root: &Path) {
    if !super::is_jvm_project(root) {
        return;
    }
    let jdks = session.state_mut::<Jdks>().clone();
    let (level, current) = {
        let mut store = jdks.store();
        let Some(level) = store.build_level(root).map(|found| found.level) else { return };
        let project = store.project(root);
        if store.jdk_of(level).is_some() || project.as_ref().is_some_and(|project| project.chosen) {
            return;
        }
        (level, project.map(|project| project.jdk.major))
    };
    if !session.state_mut::<OfferedJdks>().0.insert((root.to_owned(), level)) {
        return;
    }
    let body = match current {
        Some(current) => t!("jdk-no-match-body", level = level, current = current),
        None => t!("jdk-no-match-none", level = level),
    };
    let notice = NewNotification::message(Group::General, Kind::Warning, body)
        .titled_by(zen_i18n::Message::new("jdk-no-match-title").with_number("level", f64::from(level)))
        .action(notifications::Action::new(
            "download",
            t!("jdk-no-match-download", level = level),
            DownloadJdk::ID,
            serde_json::json!({ "version": level }),
        ))
        .action(notifications::Action::new(
            "choose",
            t!("jdk-no-match-choose"),
            "view.settings.open",
            serde_json::json!({ "page": "projectStructure" }),
        ));
    notifications::post(session, notice);
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListJdks)?;
    registry.register(RefreshJdks)?;
    registry.register(AddJdk)?;
    registry.register(RemoveJdk)?;
    registry.register(ChooseJdk)?;
    registry.register(SetLanguageLevel)?;
    registry.register(ProjectJdkCommand)?;
    registry.register(AvailableJdks)?;
    registry.register(DownloadJdk)?;
    registry.register(JdkSettings)?;
    Ok(())
}
