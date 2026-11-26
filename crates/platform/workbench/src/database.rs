use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_modules::{ModuleDescriptor, ModuleHost};
use zen_secrets::SecretStore;

#[cfg(not(target_arch = "wasm32"))]
pub mod agent_access;
#[cfg(not(target_arch = "wasm32"))]
pub mod alter;
#[cfg(not(target_arch = "wasm32"))]
pub mod assist;
#[cfg(not(target_arch = "wasm32"))]
pub mod backups;
#[cfg(not(target_arch = "wasm32"))]
pub mod clients;
#[cfg(not(target_arch = "wasm32"))]
pub mod clipboard;
#[cfg(not(target_arch = "wasm32"))]
pub mod compare;
#[cfg(not(target_arch = "wasm32"))]
pub mod connections;
#[cfg(not(target_arch = "wasm32"))]
pub mod consoles;
#[cfg(not(target_arch = "wasm32"))]
pub mod csv_imports;
#[cfg(not(target_arch = "wasm32"))]
pub mod detect;
#[cfg(not(target_arch = "wasm32"))]
pub mod diagrams;
#[cfg(not(target_arch = "wasm32"))]
pub mod edits;
#[cfg(not(target_arch = "wasm32"))]
pub mod exports;
#[cfg(not(target_arch = "wasm32"))]
pub mod files;
#[cfg(not(target_arch = "wasm32"))]
pub mod filters;
#[cfg(not(target_arch = "wasm32"))]
pub mod find;
#[cfg(not(target_arch = "wasm32"))]
pub mod history;
#[cfg(not(target_arch = "wasm32"))]
pub mod imports;
#[cfg(not(target_arch = "wasm32"))]
pub mod injected;
#[cfg(not(target_arch = "wasm32"))]
pub mod keys;
#[cfg(not(target_arch = "wasm32"))]
pub mod migrations;
#[cfg(not(target_arch = "wasm32"))]
pub mod navigator;
#[cfg(not(target_arch = "wasm32"))]
pub mod objects;
#[cfg(not(target_arch = "wasm32"))]
pub mod plans;
#[cfg(not(target_arch = "wasm32"))]
pub mod queries;
#[cfg(not(target_arch = "wasm32"))]
pub mod recovery;
#[cfg(not(target_arch = "wasm32"))]
pub mod relations;
#[cfg(not(target_arch = "wasm32"))]
pub mod removal;
#[cfg(not(target_arch = "wasm32"))]
pub mod safety;
#[cfg(not(target_arch = "wasm32"))]
pub mod scripts;
#[cfg(not(target_arch = "wasm32"))]
pub mod search;
#[cfg(not(target_arch = "wasm32"))]
pub mod sessions;
#[cfg(not(target_arch = "wasm32"))]
pub mod several;
#[cfg(not(target_arch = "wasm32"))]
pub mod sources;
#[cfg(not(target_arch = "wasm32"))]
pub mod structure;
#[cfg(not(target_arch = "wasm32"))]
pub mod transactions;
#[cfg(not(target_arch = "wasm32"))]
pub mod transfers;
#[cfg(not(target_arch = "wasm32"))]
pub mod usages;
#[cfg(not(target_arch = "wasm32"))]
pub mod values;
#[cfg(not(target_arch = "wasm32"))]
pub mod variables;

#[cfg(not(target_arch = "wasm32"))]
pub use sources::Database;

use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, WorkspaceInfo, Workspaces};
use crate::{Workbench, display};

pub const WORKSPACE: &str = "database";

pub const OVERVIEW: &str = "overview";

pub const LOCAL_CONNECTION: &str = "local";

const CONSOLES_LISTED: usize = 50;

pub const NO_CONNECTION: &str = "noConnection";

pub static MODULE: ModuleDescriptor = ModuleDescriptor {
    id: "database",
    name: "module-database-name",
    description: "module-database-description",
    locales: &[],
    register: register_module,
};

fn register_module(host: &mut ModuleHost<'_>) -> Result<(), RegistryError> {
    register(host.commands())?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let data = host.data_dir().map(Path::to_path_buf);
        host.run_provider(Arc::new(migrations::MigrationRun::new(data)));
    }
    host.window_state(Arc::new(|session: &mut Session| {
        install(session);
        session.set_state(Secrets(zen_secrets::system()));
    }));
    Ok(())
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        agent_access::register(registry, NewConsole)?;
        agent_access::register(registry, ListConsoles)?;
        agent_access::register(registry, crate::sql_format::FormatSql)?;
        register_connections(registry)?;
    }
    #[cfg(target_arch = "wasm32")]
    {
        registry.register(NewConsole)?;
        registry.register(ListConsoles)?;
        registry.register(crate::sql_format::FormatSql)?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn register_connections(registry: &mut Registry) -> Result<(), RegistryError> {
    agent_access::register(registry, sources::ListSources)?;
    agent_access::register(registry, usages::FindDbUsages)?;
    agent_access::register(registry, usages::RenameDbUsages)?;
    agent_access::register(registry, sources::GetSource)?;
    agent_access::register(registry, sources::AddSource)?;
    agent_access::register(registry, sources::UpdateSource)?;
    agent_access::register(registry, sources::RemoveSource)?;
    agent_access::register(registry, removal::RemovalPreview)?;
    agent_access::register(registry, sources::DuplicateSource)?;
    agent_access::register(registry, connections::TestSource)?;
    agent_access::register(registry, sources::ListTypes)?;
    agent_access::register(registry, sources::CreateType)?;
    agent_access::register(registry, sources::UpdateType)?;
    agent_access::register(registry, sources::DeleteType)?;
    agent_access::register(registry, connections::Connect)?;
    agent_access::register(registry, connections::Disconnect)?;
    agent_access::register(registry, connections::DisconnectAll)?;
    agent_access::register(registry, connections::Invalidate)?;
    agent_access::register(registry, connections::Status)?;
    agent_access::register(registry, connections::Cancel)?;
    agent_access::register(registry, connections::OperationStateCommand)?;
    agent_access::register(registry, sources::SetSecret)?;
    agent_access::register(registry, sources::ListDrivers)?;
    agent_access::register(registry, sources::DriverLog)?;
    agent_access::register(registry, files::CreateFile)?;
    agent_access::register(registry, files::OpenFile)?;
    agent_access::register(registry, assist::Complete)?;
    agent_access::register(registry, assist::Resolve)?;
    agent_access::register(registry, assist::Navigate)?;
    agent_access::register(registry, injected::ListInjections)?;
    agent_access::register(registry, injected::BindInjection)?;
    agent_access::register(registry, injected::SetDefault)?;
    agent_access::register(registry, injected::RunInjection)?;
    agent_access::register(registry, consoles::SplitScript)?;
    agent_access::register(registry, consoles::StatementUnderCaret)?;
    agent_access::register(registry, consoles::NextStatement)?;
    agent_access::register(registry, consoles::PreviousStatement)?;
    agent_access::register(registry, consoles::ExecuteStatement)?;
    agent_access::register(registry, consoles::ExecuteInNewTab)?;
    agent_access::register(registry, consoles::ExecuteScript)?;
    agent_access::register(registry, consoles::ExecuteFromCaret)?;
    agent_access::register(registry, variables::ListParameters)?;
    agent_access::register(registry, variables::ListVariables)?;
    agent_access::register(registry, variables::SetVariable)?;
    agent_access::register(registry, variables::SetParameter)?;
    agent_access::register(registry, consoles::ChooseConnection)?;
    agent_access::register(registry, consoles::ChooseSchema)?;
    agent_access::register(registry, consoles::ListSchemas)?;
    agent_access::register(registry, consoles::ToggleResults)?;
    agent_access::register(registry, consoles::MaximizeResults)?;
    agent_access::register(registry, consoles::ShowPanelTab)?;
    agent_access::register(registry, consoles::CancelConsole)?;
    agent_access::register(registry, consoles::ConsoleStateCommand)?;
    agent_access::register(registry, consoles::ConsoleOutput)?;
    agent_access::register(registry, consoles::GetSettings)?;
    agent_access::register(registry, consoles::UpdateSettings)?;
    agent_access::register(registry, queries::RunQuery)?;
    agent_access::register(registry, queries::QueryStateCommand)?;
    agent_access::register(registry, queries::CancelQuery)?;
    agent_access::register(registry, queries::ListResults)?;
    agent_access::register(registry, queries::ResultPage)?;
    agent_access::register(registry, queries::FetchNext)?;
    agent_access::register(registry, queries::FetchAll)?;
    agent_access::register(registry, queries::CountRows)?;
    agent_access::register(registry, queries::PinResult)?;
    agent_access::register(registry, queries::RenameResult)?;
    agent_access::register(registry, queries::CloseResult)?;
    agent_access::register(registry, queries::SetView)?;
    agent_access::register(registry, clipboard::CopyCells)?;
    agent_access::register(registry, clipboard::CopyAs)?;
    agent_access::register(registry, clipboard::CopyState)?;
    agent_access::register(registry, clipboard::CopySettingsCommand)?;
    agent_access::register(registry, clipboard::Paste)?;
    agent_access::register(registry, queries::RefreshResult)?;
    agent_access::register(registry, values::ResultValue)?;
    agent_access::register(registry, values::SaveValue)?;
    agent_access::register(registry, values::CalcCells)?;
    agent_access::register(registry, values::ResultGeometry)?;
    agent_access::register(registry, values::ToggleValuePanel)?;
    agent_access::register(registry, values::SelectRow)?;
    agent_access::register(registry, structure::SchemaChildren)?;
    agent_access::register(registry, structure::SchemaRefresh)?;
    agent_access::register(registry, structure::ObjectGet)?;
    agent_access::register(registry, structure::ObjectDdl)?;
    agent_access::register(registry, objects::ObjectOpen)?;
    agent_access::register(registry, objects::ObjectStateCommand)?;
    agent_access::register(registry, objects::ShowPage)?;
    agent_access::register(registry, objects::ShowSection)?;
    agent_access::register(registry, objects::SetDdlOptions)?;
    agent_access::register(registry, objects::ReadData)?;
    agent_access::register(registry, objects::ObjectProperties)?;
    agent_access::register(registry, alter::Stage)?;
    agent_access::register(registry, alter::Unstage)?;
    agent_access::register(registry, alter::Discard)?;
    agent_access::register(registry, alter::StateCommand)?;
    agent_access::register(registry, alter::PreviewCommand)?;
    agent_access::register(registry, alter::Execute)?;
    agent_access::register(registry, alter::ToConsole)?;
    agent_access::register(registry, alter::SaveMigration)?;
    agent_access::register(registry, alter::ObjectDrop)?;
    agent_access::register(registry, alter::ObjectRename)?;
    agent_access::register(registry, alter::ObjectTruncate)?;
    agent_access::register(registry, alter::NavigatorCreate)?;
    agent_access::register(registry, structure::ObjectCopyName)?;
    agent_access::register(registry, keys::KeyGet)?;
    agent_access::register(registry, keys::KeyExpire)?;
    agent_access::register(registry, keys::KeyState)?;
    agent_access::register(registry, keys::KeyOpen)?;
    agent_access::register(registry, navigator::NavigatorState)?;
    agent_access::register(registry, navigator::NavigatorExpand)?;
    agent_access::register(registry, navigator::NavigatorCollapse)?;
    agent_access::register(registry, navigator::NavigatorSelect)?;
    agent_access::register(registry, navigator::NavigatorRefresh)?;
    agent_access::register(registry, navigator::NavigatorFilter)?;
    agent_access::register(registry, navigator::NavigatorOptions)?;
    agent_access::register(registry, filters::FilterStateCommand)?;
    agent_access::register(registry, filters::ApplyFilter)?;
    agent_access::register(registry, filters::SortColumn)?;
    agent_access::register(registry, filters::FilterByValue)?;
    agent_access::register(registry, filters::ResetFilter)?;
    agent_access::register(registry, filters::FilterBack)?;
    agent_access::register(registry, filters::FilterForward)?;
    agent_access::register(registry, filters::CompleteFilter)?;
    agent_access::register(registry, filters::DistinctValues)?;
    agent_access::register(registry, filters::FindInResult)?;
    agent_access::register(registry, filters::SaveFilter)?;
    agent_access::register(registry, filters::DeleteFilter)?;
    agent_access::register(registry, edits::EditStateCommand)?;
    agent_access::register(registry, edits::SetCell)?;
    agent_access::register(registry, edits::RevertCell)?;
    agent_access::register(registry, edits::AddRow)?;
    agent_access::register(registry, edits::DuplicateRow)?;
    agent_access::register(registry, edits::DeleteRows)?;
    agent_access::register(registry, edits::CopyFromAbove)?;
    agent_access::register(registry, edits::PreviewChanges)?;
    agent_access::register(registry, edits::SaveChanges)?;
    agent_access::register(registry, edits::RevertChanges)?;
    agent_access::register(registry, edits::SetRowKey)?;
    agent_access::register(registry, edits::GenerateSql)?;
    agent_access::register(registry, relations::ResultRelations)?;
    agent_access::register(registry, relations::OpenReferenced)?;
    agent_access::register(registry, relations::OpenReferencing)?;
    agent_access::register(registry, relations::DataBack)?;
    agent_access::register(registry, relations::DataForward)?;
    agent_access::register(registry, relations::DataNavigation)?;
    agent_access::register(registry, relations::LookupStart)?;
    agent_access::register(registry, relations::LookupStateCommand)?;
    agent_access::register(registry, relations::LookupChoose)?;
    agent_access::register(registry, relations::LookupClose)?;
    agent_access::register(registry, transactions::Commit)?;
    agent_access::register(registry, safety::SetReadOnly)?;
    agent_access::register(registry, transactions::Rollback)?;
    agent_access::register(registry, transactions::Mode)?;
    agent_access::register(registry, transactions::Status)?;
    agent_access::register(registry, transactions::Pending)?;
    agent_access::register(registry, transactions::Postpone)?;
    agent_access::register(registry, transactions::CloseConsole)?;
    agent_access::register(registry, connections::ListSshPrompts)?;
    agent_access::register(registry, connections::AnswerSsh)?;
    agent_access::register(registry, detect::ListDetections)?;
    agent_access::register(registry, detect::AcceptDetection)?;
    agent_access::register(registry, detect::DismissDetection)?;
    agent_access::register(registry, imports::ImportDbeaver)?;
    agent_access::register(registry, imports::ImportJetbrains)?;
    agent_access::register(registry, exports::ExportSettings)?;
    agent_access::register(registry, exports::StartExport)?;
    agent_access::register(registry, exports::ExportStateCommand)?;
    agent_access::register(registry, exports::CancelExport)?;
    agent_access::register(registry, csv_imports::ImportSettings)?;
    agent_access::register(registry, csv_imports::PreviewImport)?;
    agent_access::register(registry, csv_imports::StartImport)?;
    agent_access::register(registry, csv_imports::ImportStateCommand)?;
    agent_access::register(registry, csv_imports::CancelImport)?;
    agent_access::register(registry, transfers::PreviewTransfer)?;
    agent_access::register(registry, transfers::StartTransfer)?;
    agent_access::register(registry, transfers::TransferStateCommand)?;
    agent_access::register(registry, transfers::CancelTransfer)?;
    agent_access::register(registry, history::ListHistory)?;
    agent_access::register(registry, history::GetHistory)?;
    agent_access::register(registry, history::ClearHistory)?;
    agent_access::register(registry, history::HistorySettings)?;
    agent_access::register(registry, history::OpenHistory)?;
    agent_access::register(registry, history::OpenInConsole)?;
    agent_access::register(registry, recovery::ListRecovered)?;
    agent_access::register(registry, recovery::GetRecovered)?;
    agent_access::register(registry, recovery::RestoreRecovered)?;
    agent_access::register(registry, recovery::DiscardRecovered)?;
    agent_access::register(registry, recovery::OpenRecovery)?;
    agent_access::register(registry, scripts::BindScript)?;
    agent_access::register(registry, scripts::ScriptStateCommand)?;
    agent_access::register(registry, scripts::RunScript)?;
    agent_access::register(registry, scripts::ScriptTargets)?;
    agent_access::register(registry, scripts::ListScripts)?;
    agent_access::register(registry, scripts::NewScriptCommand)?;
    agent_access::register(registry, plans::Explain)?;
    agent_access::register(registry, plans::PlanStateCommand)?;
    agent_access::register(registry, plans::SelectNode)?;
    agent_access::register(registry, plans::SetPlanView)?;
    agent_access::register(registry, search::GoToObject)?;
    agent_access::register(registry, sessions::ListSessions)?;
    agent_access::register(registry, sessions::SessionsState)?;
    agent_access::register(registry, sessions::CancelSessionQuery)?;
    agent_access::register(registry, sessions::TerminateSession)?;
    agent_access::register(registry, sessions::OpenSessions)?;
    agent_access::register(registry, several::RunOnConnections)?;
    agent_access::register(registry, several::ConnectionsState)?;
    agent_access::register(registry, find::SearchTargets)?;
    agent_access::register(registry, find::OpenSearch)?;
    agent_access::register(registry, find::StartSearch)?;
    agent_access::register(registry, find::SearchStateCommand)?;
    agent_access::register(registry, find::CancelSearch)?;
    agent_access::register(registry, find::OpenSearchResult)?;
    agent_access::register(registry, compare::CompareSchemas)?;
    agent_access::register(registry, compare::CompareData)?;
    agent_access::register(registry, compare::CompareStateCommand)?;
    agent_access::register(registry, compare::CancelCompare)?;
    agent_access::register(registry, compare::CompareDdl)?;
    agent_access::register(registry, compare::CompareScript)?;
    agent_access::register(registry, compare::OpenCompare)?;
    agent_access::register(registry, compare::SaveSnapshot)?;
    agent_access::register(registry, compare::ListSnapshots)?;
    agent_access::register(registry, clients::OpenTerminal)?;
    agent_access::register(registry, clients::ClientCommandLine)?;
    agent_access::register(registry, structure::GenerateCode)?;
    agent_access::register(registry, diagrams::OpenDiagram)?;
    agent_access::register(registry, diagrams::DiagramStateCommand)?;
    agent_access::register(registry, diagrams::ZoomDiagram)?;
    agent_access::register(registry, diagrams::SetDiagramView)?;
    agent_access::register(registry, diagrams::SelectEntity)?;
    agent_access::register(registry, diagrams::ExportDiagram)?;
    agent_access::register(registry, diagrams::CancelDiagram)?;
    agent_access::register(registry, backups::BackupSettings)?;
    agent_access::register(registry, backups::ListTools)?;
    agent_access::register(registry, backups::StartBackup)?;
    agent_access::register(registry, backups::StartRestore)?;
    agent_access::register(registry, backups::BackupState)?;
    agent_access::register(registry, backups::CancelBackup)?;
    agent_access::register(registry, migrations::ListMigrations)?;
    agent_access::register(registry, migrations::MigrationStatus)?;
    agent_access::register(registry, migrations::RunMigrations)?;
    agent_access::register(registry, migrations::OpenMigrations)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn tab_connection(session: &Session) -> Option<String> {
    let tab = session.state::<Workspaces>()?.active_tab(WORKSPACE)?.clone();
    match &tab.path {
        Some(path) => consoles::connection_of(session, path),
        None => tab.data["connection"]
            .as_str()
            .map(str::to_owned)
            .filter(|id| consoles::known_connections(session).contains(id)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn selected_connection(session: &mut Session) -> Option<String> {
    let known = consoles::known_connections(session);
    navigator::navigator(session)
        .selected()
        .and_then(|row| row.connection.clone())
        .filter(|id| known.contains(id))
        .or_else(|| tab_connection(session))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn toolbar_state(session: &mut Session, id: &str) -> Option<serde_json::Value> {
    let entry = sources::find(session, id).ok()?;
    let (types, _) = sources::connection_types(session);
    let color = Some(types.resolve(&entry.source.connection_type).color.clone()).filter(|color| color != "none");
    let safety = safety::target(session, &entry);
    let transaction = transactions::state_of(session, &entry, &zen_db::driver::ContextKind::Main, 10);
    Some(serde_json::json!({
        "connection": id,
        "connectionName": entry.source.name,
        "color": color,
        "safety": safety,
        "transaction": transaction,
    }))
}

pub fn install(session: &mut Session) {
    #[cfg(not(target_arch = "wasm32"))]
    session.set_state(sources::DiskCache::default());
    #[cfg(not(target_arch = "wasm32"))]
    session.set_state(crate::hidden_files::AgentHiddenPaths(Some(Arc::new(agent_access::hidden_paths))));
    #[cfg(not(target_arch = "wasm32"))]
    session.set_state(crate::workspaces::AgentHiddenTabs(Some(Arc::new(agent_access::hidden_tab))));
    let workspaces = session.state_mut::<Workspaces>();
    workspaces.register(WorkspaceInfo {
        id: WORKSPACE.to_owned(),
        title: t!("database-workspace-title"),
        icon: "database".to_owned(),
    });
    let overview = InnerTab {
        id: OVERVIEW.to_owned(),
        kind: OVERVIEW.to_owned(),
        title: t!("database-overview-title"),
        path: None,
        closable: false,
        data: serde_json::Value::Null,
        group: 0,
    };
    workspaces.pin(WORKSPACE, overview);
    workspaces.host_files(WORKSPACE, &["sql"]);
    workspaces.accept_files(WORKSPACE, takes_script);
    #[cfg(not(target_arch = "wasm32"))]
    workspaces.guard_close(WORKSPACE, transactions::guard_close);
    #[cfg(not(target_arch = "wasm32"))]
    workspaces.label_tabs(WORKSPACE, consoles::tab_label);
    #[cfg(not(target_arch = "wasm32"))]
    session.set_state(assist::Checks::default());
    #[cfg(not(target_arch = "wasm32"))]
    session.set_state(scripts::Bindings::default());
    #[cfg(not(target_arch = "wasm32"))]
    session
        .state_mut::<zen_modules::search::SearchContributors>()
        .add(Arc::new(search::DatabaseContributor::default()));
}

pub fn takes_script(session: &Session, path: &Path) -> bool {
    let sql = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("sql"));
    let console = session.state::<Workspaces>().is_some_and(|workspaces| workspaces.is_console(path));
    let project =
        session.state::<Workbench>().and_then(Workbench::project).is_some_and(|project| path.starts_with(project));
    sql && !console && project
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn private_file(prefix: &str, text: &str) -> std::io::Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{stamp:x}-{next}", std::process::id()));
    write_private(&path, text)?;
    Ok(path)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    if let Err(error) = file.write_all(text.as_bytes()).and_then(|()| file.flush()) {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn guard_agent_file(session: &Session, path: &Path) -> Result<(), CommandError> {
    let root = session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_path_buf)
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    crate::hidden_files::guard_change(session, &root, &[path])
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn guard_agent_new_path(session: &Session, path: &Path) -> Result<(), CommandError> {
    let Some(patterns) = crate::hidden_files::for_caller(session) else { return Ok(()) };
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf);
    if crate::hidden_files::is_hidden(&patterns, root.as_deref(), path) {
        return Err(crate::hidden_files::refused(&path.to_string_lossy()));
    }
    Ok(())
}

pub fn project_key(project: &Path) -> String {
    use sha2::Digest;
    let name: String = project
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    let name = name.trim_start_matches('.');
    let digest = sha2::Sha256::digest(project.as_os_str().as_encoded_bytes());
    let hash = hex::encode(&digest[..4]);
    if name.is_empty() { hash } else { format!("{name}-{hash}") }
}

fn consoles_of_project(session: &Session) -> Result<PathBuf, CommandError> {
    let folder = session
        .state::<Workspaces>()
        .and_then(Workspaces::consoles_folder)
        .ok_or_else(|| CommandError::new(t!("database-error-no-data-folder")))?;
    let project = session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    Ok(folder.join(project_key(project)))
}

fn connection_folder(connection: Option<&str>) -> Result<String, CommandError> {
    let name = connection.map(str::trim).filter(|name| !name.is_empty()).unwrap_or(LOCAL_CONNECTION);
    let plain = !name.starts_with('.') && !name.contains(['/', '\\', ':']) && !name.chars().any(char::is_control);
    if !plain {
        return Err(CommandError::new(t!("database-error-connection-name", name = name)));
    }
    Ok(name.to_owned())
}

pub fn for_caller(session: &Session, people: String, agents: String) -> String {
    if zen_core::macros::by_agent(session) { agents } else { people }
}

pub fn no_console_connection(session: &Session) -> String {
    for_caller(session, t!("database-error-no-console-connection"), t!("database-error-no-console-connection-agent"))
}

/// What `db.console.new` creates.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NewConsoleParams {
    /// The connection the console belongs to; when absent, the connection selected in the Database Navigator, else that of the workspace's active tab, else the folder `local`.
    pub connection: Option<String>,
    /// The text the console starts with; empty when absent.
    pub text: Option<String>,
}

/// A console created and opened.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleOpened {
    /// The console's file, under `<data folder>/consoles/<project key>/<connection>/`.
    pub path: PathBuf,
    /// The connection folder it belongs to.
    pub connection: String,
    /// Its inner tab in the Database workspace.
    pub tab: InnerTab,
}

pub struct NewConsole;

impl Command for NewConsole {
    type Params = NewConsoleParams;
    type Output = ConsoleOpened;
    const ID: &'static str = "db.console.new";
    const TITLE: &'static str = "New Console";
    const DESCRIPTION: &'static str = "Creates an SQL console (`console.sql`, then `console_1.sql`, ...) in the IDE's data folder under `consoles/<project key>/<connection>/` (never in the project); without `connection` it takes the connection selected in the Database Navigator, else the one of the workspace's active tab (a console's, an object's), else the folder `local`, opens it as a tab of the Database workspace and expands the workspace. Connects nowhere. Refused with `noConnection` while there is no connection at all: add one with `db.dataSource.add` first. Closing it while it is empty deletes it. Answers with its path and tab.";

    fn run(&self, session: &mut Session, params: NewConsoleParams) -> Result<ConsoleOpened, CommandError> {
        #[cfg(not(target_arch = "wasm32"))]
        if consoles::known_connections(session).is_empty() {
            let message =
                for_caller(session, t!("database-error-no-connection"), t!("database-error-no-connection-agent"));
            return Err(CommandError::with_code(NO_CONNECTION, message));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let connection = params.connection.or_else(|| selected_connection(session));
        #[cfg(target_arch = "wasm32")]
        let connection = params.connection;
        let connection = connection_folder(connection.as_deref())?;
        let folder = consoles_of_project(session)?.join(&connection);
        let failed = |error: std::io::Error| {
            CommandError::new(t!("workbench-error-create", path = display(&folder), error = error.to_string()))
        };
        std::fs::create_dir_all(&folder).map_err(failed)?;
        let text = params.text.unwrap_or_default();
        let path = crate::scratch::create_new(&folder, "console", "sql", &text).map_err(failed)?;
        let open =
            OpenTabParams { workspace: WORKSPACE.to_owned(), path: Some(path.clone()), ..OpenTabParams::default() };
        let tab = OpenTab.run(session, open)?;
        session.state_mut::<Workbench>().revision += 1;
        Ok(ConsoleOpened { path, connection, tab })
    }
}

/// Which consoles to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListConsolesParams {
    /// Only the consoles of this connection folder.
    pub connection: Option<String>,
    /// The most consoles to list; 50 when absent.
    pub limit: Option<usize>,
}

/// A console of the project.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleEntry {
    /// The console's file.
    pub path: PathBuf,
    /// The connection folder it belongs to.
    pub connection: String,
    /// Its file name.
    pub name: String,
    /// Its size on disk in bytes (unsaved changes not counted).
    pub size: u64,
    /// Whether it is open as a tab of the Database workspace.
    pub open: bool,
}

/// The project's consoles.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleList {
    /// The folder of the project's consoles.
    pub folder: PathBuf,
    /// The consoles, the most recently changed first, at most `limit` of them.
    pub consoles: Vec<ConsoleEntry>,
    /// How many consoles the project has.
    pub total: usize,
    /// Whether consoles were left out because of `limit`.
    pub truncated: bool,
}

pub struct ListConsoles;

impl Command for ListConsoles {
    type Params = ListConsolesParams;
    type Output = ConsoleList;
    const ID: &'static str = "db.console.list";
    const TITLE: &'static str = "List Consoles";
    const DESCRIPTION: &'static str = "Lists the SQL consoles of the open project, kept in the IDE's data folder: each one's path, connection folder, name, size and whether it is open in the Database workspace, the most recently changed first (at most `limit`, 50 by default, with `total` and `truncated`). Never reads their text; use get_document_text for that.";

    fn run(&self, session: &mut Session, params: ListConsolesParams) -> Result<ConsoleList, CommandError> {
        let folder = consoles_of_project(session)?;
        let wanted = params.connection.as_deref().map(|name| connection_folder(Some(name))).transpose()?;
        let open: Vec<PathBuf> = session
            .state::<Workspaces>()
            .map(|workspaces| workspaces.tabs(WORKSPACE).iter().filter_map(|tab| tab.path.clone()).collect())
            .unwrap_or_default();
        let mut found = Vec::new();
        for connection in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let name = connection.file_name().to_string_lossy().into_owned();
            if wanted.as_ref().is_some_and(|wanted| *wanted != name) || !connection.path().is_dir() {
                continue;
            }
            for file in std::fs::read_dir(connection.path()).into_iter().flatten().flatten() {
                let path = file.path();
                let Ok(meta) = file.metadata() else { continue };
                if !meta.is_file() || path.extension().is_none_or(|extension| extension != "sql") {
                    continue;
                }
                let modified = meta.modified().ok();
                let entry = ConsoleEntry {
                    name: file.file_name().to_string_lossy().into_owned(),
                    connection: name.clone(),
                    size: meta.len(),
                    open: open.contains(&path),
                    path,
                };
                found.push((modified, entry));
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
        let total = found.len();
        let limit = params.limit.unwrap_or(CONSOLES_LISTED);
        let consoles = found.into_iter().take(limit).map(|(_, entry)| entry).collect();
        Ok(ConsoleList { folder, consoles, total, truncated: total > limit })
    }
}

#[derive(Clone)]
pub struct Secrets(pub Arc<dyn SecretStore>);

#[cfg(test)]
mod tests;
