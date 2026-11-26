use std::collections::VecDeque;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

pub const LIMIT: usize = 500;

pub const TITLE_LIMIT: usize = 200;

pub const BODY_LIMIT: usize = 4000;

pub const RUN_ACTION: &str = "ui.notifications.action";

/// Where a notification comes from, as Settings → Notifications lists the groups.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    /// The IDE itself: files, themes, keymaps and other messages.
    #[default]
    General,
    /// New versions of the IDE.
    Updates,
    /// Plug-ins: offers, installs and their problems.
    Plugins,
    /// Language servers: starts, restarts, installs.
    LanguageServers,
    /// The debugger and its installation.
    Debugger,
    /// Git operations.
    Git,
    /// Commands that failed.
    Errors,
    /// Power Save Mode: the notice that it is on.
    PowerSave,
}

impl Group {
    pub const ALL: [Group; 8] = [
        Group::General,
        Group::Updates,
        Group::Plugins,
        Group::LanguageServers,
        Group::Debugger,
        Group::Git,
        Group::Errors,
        Group::PowerSave,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Group::General => "general",
            Group::Updates => "updates",
            Group::Plugins => "plugins",
            Group::LanguageServers => "languageServers",
            Group::Debugger => "debugger",
            Group::Git => "git",
            Group::Errors => "errors",
            Group::PowerSave => "powerSave",
        }
    }

    pub fn of_command(id: &str) -> Option<Group> {
        let declared = DECLARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, group)) = declared.iter().find(|(prefix, _)| id.starts_with(prefix)) {
            return Some(*group);
        }
        let area = id.split('.').next().unwrap_or_default();
        match area {
            "vcs" => Some(Group::Git),
            "debug" => Some(Group::Debugger),
            "lsp" | "language" => Some(Group::LanguageServers),
            "view" if id.starts_with("view.vcs.") => Some(Group::Git),
            _ => None,
        }
    }

    pub fn declare(prefix: &'static str, group: &str) {
        let Some(group) = Group::ALL.into_iter().find(|known| known.id() == group) else { return };
        let mut declared = DECLARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !declared.contains(&(prefix, group)) {
            declared.push((prefix, group));
        }
    }
}

static DECLARED: std::sync::Mutex<Vec<(&'static str, Group)>> = std::sync::Mutex::new(Vec::new());

/// How serious a notification is: an error's balloon stays until it is closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Information.
    #[default]
    Info,
    /// A warning.
    Warning,
    /// An error.
    Error,
}

/// A link of a notification: a command (or a `view.*` action of the window) with its parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    /// The action's id within its notification, given to `ui.notifications.action`.
    pub id: String,
    /// The link's text.
    pub label: String,
    /// The command or `view.*` action it runs.
    pub command: String,
    /// The command's parameters.
    #[serde(default)]
    pub params: Value,
    /// Whether the link goes away once it is used, as what it asked is then done or under way.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub once: bool,
}

impl Action {
    pub fn new(id: &str, label: String, command: &str, params: Value) -> Self {
        Self { id: id.to_owned(), label, command: command.to_owned(), params, once: false }
    }

    #[must_use]
    pub fn once(mut self) -> Self {
        self.once = true;
        self
    }
}

/// One notification.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// The id, unique in the window, larger for newer notifications.
    pub id: u64,
    /// When it came, in milliseconds since 1970.
    pub time: u64,
    /// Where it comes from.
    pub group: Group,
    /// How serious it is.
    #[serde(rename = "type")]
    pub kind: Kind,
    /// The title, empty for a one-line notification.
    pub title: String,
    /// The text.
    pub body: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) worded: (Option<zen_i18n::Message>, Option<zen_i18n::Message>),
    /// Its links.
    pub actions: Vec<Action>,
    /// Whether it was seen in the Notifications tool window or acted on.
    pub read: bool,
    /// Whether the window shows it as a balloon when it comes; false for what an editor banner
    /// already shows.
    pub balloon: bool,
    /// Whether its balloon stays until it is closed or acted on, as an error's does.
    pub sticky: bool,
}

impl Notification {
    fn worded_now(&self) -> Self {
        let (title, body) = &self.worded;
        Self {
            title: title.as_ref().map_or_else(|| self.title.clone(), |title| cut(title.text(), TITLE_LIMIT)),
            body: body.as_ref().map_or_else(|| self.body.clone(), |body| cut(body.text(), BODY_LIMIT)),
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewNotification {
    pub group: Group,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub title_message: Option<zen_i18n::Message>,
    pub body_message: Option<zen_i18n::Message>,
    pub actions: Vec<Action>,
    pub balloon: bool,
    pub sticky: bool,
}

impl NewNotification {
    pub fn message(group: Group, kind: Kind, body: impl Into<String>) -> Self {
        Self { group, kind, body: body.into(), balloon: true, ..Self::default() }
    }

    pub fn worded(group: Group, kind: Kind, body: zen_i18n::Message) -> Self {
        let text = body.text();
        Self { body_message: Some(body), ..Self::message(group, kind, text) }
    }

    pub fn titled_by(mut self, title: zen_i18n::Message) -> Self {
        self.title = title.text();
        self.title_message = Some(title);
        self
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    pub fn silent(mut self) -> Self {
        self.balloon = false;
        self
    }

    pub fn sticky(mut self) -> Self {
        self.sticky = true;
        self
    }
}

#[derive(Debug, Default)]
pub struct Notifications {
    items: VecDeque<Notification>,
    last_id: u64,
    queued: Vec<(String, Value)>,
}

impl Notifications {
    pub fn add(&mut self, new: NewNotification, time: u64) -> u64 {
        self.last_id += 1;
        self.items.push_back(Notification {
            id: self.last_id,
            time,
            group: new.group,
            kind: new.kind,
            title: cut(new.title, TITLE_LIMIT),
            body: cut(new.body, BODY_LIMIT),
            worded: (new.title_message, new.body_message),
            actions: new.actions,
            read: false,
            balloon: new.balloon,
            sticky: new.sticky,
        });
        while self.items.len() > LIMIT {
            self.items.pop_front();
        }
        self.last_id
    }

    pub fn newest_first(&self) -> impl Iterator<Item = &Notification> {
        self.items.iter().rev()
    }

    pub fn get(&self, id: u64) -> Option<&Notification> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn last_id(&self) -> u64 {
        self.last_id
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn unread(&self) -> usize {
        self.items.iter().filter(|item| !item.read).count()
    }

    pub fn unread_error(&self) -> bool {
        self.items.iter().any(|item| !item.read && item.kind == Kind::Error)
    }

    pub fn mark_read(&mut self, ids: Option<&[u64]>) -> usize {
        let mut count = 0;
        for item in self.items.iter_mut().filter(|item| ids.is_none_or(|ids| ids.contains(&item.id))) {
            count += usize::from(!item.read);
            item.read = true;
        }
        count
    }

    pub fn remove(&mut self, ids: Option<&[u64]>) -> usize {
        let before = self.items.len();
        self.items.retain(|item| ids.is_some_and(|ids| !ids.contains(&item.id)));
        before - self.items.len()
    }

    pub fn request(&mut self, id: u64, action: &str) -> Result<Action, CommandError> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| CommandError::new(t!("workbench-notification-unknown", id = id)))?;
        let found = item
            .actions
            .iter()
            .find(|candidate| candidate.id == action)
            .cloned()
            .ok_or_else(|| CommandError::new(t!("workbench-notification-no-action", id = id, action = action)))?;
        item.read = true;
        if found.once {
            item.actions.retain(|candidate| !candidate.once);
        }
        self.queued.push((found.command.clone(), found.params.clone()));
        Ok(found)
    }

    pub fn take_queued(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.queued)
    }
}

fn cut(mut text: String, limit: usize) -> String {
    if let Some((end, _)) = text.char_indices().nth(limit.saturating_sub(1))
        && text[end..].chars().nth(1).is_some()
    {
        text.truncate(end);
        text.push('…');
        text.shrink_to_fit();
    }
    text
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

pub fn post(session: &mut Session, new: NewNotification) -> u64 {
    session.state_mut::<Notifications>().add(new, now_ms())
}

/// The parameters of `ui.notifications.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// List only the unread ones.
    pub unread_only: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOutput {
    pub notifications: Vec<Notification>,
    pub unread: usize,
}

pub struct ListNotifications;

impl Command for ListNotifications {
    type Params = ListParams;
    type Output = ListOutput;
    const ID: &'static str = "ui.notifications.list";
    const TITLE: &'static str = "Notifications";
    const DESCRIPTION: &'static str = "Lists the window's notifications, the newest first, with their id, time \
         (milliseconds since 1970), group, type (info, warning, error), title, body, actions and whether they are \
         read; `unreadOnly` lists only the unread ones. Listing does not mark them read.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ListOutput, CommandError> {
        let store = session.state_mut::<Notifications>();
        let notifications = store
            .newest_first()
            .filter(|item| !params.unread_only || !item.read)
            .map(Notification::worded_now)
            .collect();
        Ok(ListOutput { notifications, unread: store.unread() })
    }
}

/// The parameters of `ui.notifications.markRead` and `ui.notifications.clear`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IdsParams {
    /// The notifications; every one when not given.
    pub ids: Option<Vec<u64>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    pub count: usize,
    pub unread: usize,
}

pub struct MarkRead;

impl Command for MarkRead {
    type Params = IdsParams;
    type Output = Changed;
    const ID: &'static str = "ui.notifications.markRead";
    const TITLE: &'static str = "Mark Notifications as Read";
    const DESCRIPTION: &'static str =
        "Marks the notifications `ids` read, every one when `ids` is not given; answers how many were unread.";

    fn run(&self, session: &mut Session, params: IdsParams) -> Result<Changed, CommandError> {
        let store = session.state_mut::<Notifications>();
        let count = store.mark_read(params.ids.as_deref());
        Ok(Changed { count, unread: store.unread() })
    }
}

pub struct Clear;

impl Command for Clear {
    type Params = IdsParams;
    type Output = Changed;
    const ID: &'static str = "ui.notifications.clear";
    const TITLE: &'static str = "Clear All Notifications";
    const DESCRIPTION: &'static str =
        "Removes the notifications `ids` from the Notifications tool window, every one when `ids` is not given.";

    fn run(&self, session: &mut Session, params: IdsParams) -> Result<Changed, CommandError> {
        let store = session.state_mut::<Notifications>();
        let count = store.remove(params.ids.as_deref());
        Ok(Changed { count, unread: store.unread() })
    }
}

/// The parameters of `ui.notifications.action`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionParams {
    /// The notification.
    pub id: u64,
    /// The action's id, as `ui.notifications.list` gives it.
    pub action: String,
}

pub struct RunAction;

impl Command for RunAction {
    type Params = ActionParams;
    type Output = Action;
    const ID: &'static str = RUN_ACTION;
    const TITLE: &'static str = "Run Notification Action";
    const DESCRIPTION: &'static str = "Runs the action `action` of the notification `id`, as a click on its link does, \
         and marks the notification read; the window runs the action's command, and the answer has its result.";

    fn run(&self, session: &mut Session, params: ActionParams) -> Result<Action, CommandError> {
        let person_only = session.state::<Notifications>().and_then(|store| store.get(params.id)).is_some_and(|item| {
            item.actions.iter().any(|action| {
                action.id == params.action && crate::trust::PERSON_ONLY.contains(&action.command.as_str())
            })
        });
        if person_only && zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("workbench-trust-agent")));
        }
        session.state_mut::<Notifications>().request(params.id, &params.action)
    }
}

/// The parameters of `ui.notifications.post`: an agent's notification has no actions.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostParams {
    /// Where it comes from (`general` when not given).
    #[serde(default)]
    pub group: Group,
    /// How serious it is (`info` when not given).
    #[serde(default, rename = "type")]
    pub kind: Kind,
    /// The title, empty for a one-line notification.
    #[serde(default)]
    pub title: String,
    /// The text.
    #[serde(default)]
    pub body: String,
    /// Whether the window shows it as a balloon; true when not given.
    #[serde(default = "yes")]
    pub balloon: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Posted {
    pub id: u64,
}

pub struct Post;

impl Command for Post {
    type Params = PostParams;
    type Output = Posted;
    const ID: &'static str = "ui.notifications.post";
    const TITLE: &'static str = "Post Notification";
    const DESCRIPTION: &'static str = "Adds a notification to the window, shown as a balloon unless `balloon` is false: \
         a group, a type (info, warning, error), a title and a body. A posted notification has no actions: \
         only the IDE's own notifications have links that run commands.";

    fn run(&self, session: &mut Session, params: PostParams) -> Result<Posted, CommandError> {
        if params.title.trim().is_empty() && params.body.trim().is_empty() {
            return Err(CommandError::new(t!("workbench-notification-empty")));
        }
        let PostParams { group, kind, title, body, balloon } = params;
        let new = NewNotification { group, kind, title, body, balloon, ..NewNotification::default() };
        Ok(Posted { id: post(session, new) })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListNotifications)?;
    registry.register(MarkRead)?;
    registry.register(Clear)?;
    registry.register(RunAction)?;
    registry.register(Post)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn note(body: &str) -> NewNotification {
        NewNotification::message(Group::General, Kind::Info, body)
    }

    #[test]
    fn a_notification_and_a_run_summary_are_worded_for_whoever_reads_them() {
        let (registry, mut session) = setup();
        let title = zen_i18n::Message::new("workbench-run-process-interrupted");
        zen_i18n::with_language("ru", || {
            post(
                &mut session,
                NewNotification::worded(
                    Group::General,
                    Kind::Warning,
                    zen_i18n::Message::new("workbench-error-no-document"),
                )
                .titled_by(title.clone()),
            )
        });
        let listed = |session: &mut Session, language: &str| {
            zen_i18n::with_language(language, || {
                registry.dispatch(session, "ui.notifications.list", json!({})).unwrap()
            })
        };
        let english = listed(&mut session, "en");
        assert_eq!(english["notifications"][0]["body"], "No file is open.", "{english}");
        assert_eq!(english["notifications"][0]["title"], "Process finished: ended by a signal");
        let russian = listed(&mut session, "ru");
        assert!(!russian["notifications"][0]["body"].as_str().unwrap().is_ascii(), "{russian}");

        let mut output = zen_run::RunOutput::new(Box::new(zen_run::PlainParser));
        let failed =
            zen_i18n::Message::new("workbench-before-launch-failed").with("task", "build").with_message("error", title);
        zen_i18n::with_language("ru", || output.fail_before_launch_worded(failed, None));
        let summary = zen_i18n::with_language("en", || crate::run::RunSummary::of(&output));
        assert!(
            summary.error.as_deref().is_some_and(str::is_ascii),
            "an agent's summary is English: {:?}",
            summary.error
        );
    }

    fn setup() -> (Registry, Session) {
        zen_i18n::register(crate::LOCALES);
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        (registry, Session::new())
    }

    #[test]
    fn adds_newest_first_with_unread_counts() {
        let mut store = Notifications::default();
        let first = store.add(note("one"), 10);
        let second = store.add(note("two").titled("Title"), 20);
        assert!(second > first);
        let bodies: Vec<&str> = store.newest_first().map(|item| item.body.as_str()).collect();
        assert_eq!(bodies, ["two", "one"]);
        assert_eq!(store.get(second).map(|item| (item.time, item.title.as_str())), Some((20, "Title")));
        assert_eq!(store.unread(), 2);
        assert_eq!(store.mark_read(Some(&[first])), 1);
        assert_eq!(store.mark_read(Some(&[first])), 0, "already read");
        assert_eq!(store.unread(), 1);
        assert_eq!(store.mark_read(None), 1);
        assert_eq!(store.unread(), 0);
    }

    #[test]
    fn an_unread_error_is_told_apart() {
        let mut store = Notifications::default();
        store.add(note("fine"), 0);
        assert!(!store.unread_error());
        let error = store.add(NewNotification::message(Group::Errors, Kind::Error, "broken"), 0);
        assert!(store.unread_error());
        store.mark_read(Some(&[error]));
        assert!(!store.unread_error());
    }

    #[test]
    fn clears_some_or_all() {
        let mut store = Notifications::default();
        let ids: Vec<u64> = (0..4).map(|n| store.add(note(&n.to_string()), n)).collect();
        assert_eq!(store.remove(Some(&ids[1..3])), 2);
        assert_eq!(store.newest_first().map(|item| item.id).collect::<Vec<_>>(), [ids[3], ids[0]]);
        assert_eq!(store.remove(None), 2);
        assert!(store.is_empty());
        let next = store.add(note("after"), 9);
        assert!(next > ids[3], "ids are never reused");
    }

    #[test]
    fn keeps_at_most_the_limit_dropping_the_oldest() {
        let mut store = Notifications::default();
        for n in 0..LIMIT as u64 + 20 {
            store.add(note(&n.to_string()), n);
        }
        assert_eq!(store.len(), LIMIT);
        assert_eq!(store.newest_first().last().map(|item| item.body.clone()), Some("20".to_owned()));
        assert_eq!(store.last_id(), LIMIT as u64 + 20);
    }

    #[test]
    fn an_action_marks_read_and_is_queued_for_the_window() {
        let mut store = Notifications::default();
        let id = store.add(
            note("Delve is missing").action(Action::new(
                "install",
                "Install".to_owned(),
                "toolchain.dlv.install",
                json!({}),
            )),
            0,
        );
        assert!(store.request(id, "nothing").is_err());
        assert!(store.request(id + 1, "install").is_err());
        assert!(store.take_queued().is_empty(), "nothing runs for a wrong id");
        let action = store.request(id, "install").unwrap();
        assert_eq!(action.command, "toolchain.dlv.install");
        assert!(store.get(id).unwrap().read);
        assert_eq!(store.take_queued(), [("toolchain.dlv.install".to_owned(), json!({}))]);
        assert!(store.take_queued().is_empty());
    }

    #[test]
    fn commands_list_mark_clear_and_post() {
        let (registry, mut session) = setup();
        let id =
            post(
                &mut session,
                NewNotification::message(Group::Git, Kind::Error, "rejected")
                    .titled("Push failed")
                    .action(Action::new("retry", "Retry".to_owned(), "vcs.remote.push", Value::Null)),
            );
        registry.dispatch(&mut session, "ui.notifications.post", json!({ "body": "plain", "balloon": false })).unwrap();
        let list = registry.dispatch(&mut session, "ui.notifications.list", json!({})).unwrap();
        assert_eq!(list["unread"], 2);
        let first = &list["notifications"][1];
        assert_eq!(
            (&first["group"], &first["type"], &first["title"]),
            (&json!("git"), &json!("error"), &json!("Push failed"))
        );
        assert_eq!(first["actions"][0]["params"], json!(null));
        assert_eq!(list["notifications"][0]["actions"], json!([]));
        assert_eq!(list["notifications"][0]["group"], "general");
        assert_eq!(list["notifications"][0]["balloon"], false);
        let marked = registry.dispatch(&mut session, "ui.notifications.markRead", json!({ "ids": [id] })).unwrap();
        assert_eq!((marked["count"].as_u64(), marked["unread"].as_u64()), (Some(1), Some(1)));
        let unread = registry.dispatch(&mut session, "ui.notifications.list", json!({ "unreadOnly": true })).unwrap();
        assert_eq!(unread["notifications"].as_array().map(Vec::len), Some(1));
        let ran = registry.dispatch(&mut session, RUN_ACTION, json!({ "id": id, "action": "retry" })).unwrap();
        assert_eq!(ran["command"], "vcs.remote.push");
        assert_eq!(session.state_mut::<Notifications>().take_queued().len(), 1);
        let cleared = registry.dispatch(&mut session, "ui.notifications.clear", json!({})).unwrap();
        assert_eq!((cleared["count"].as_u64(), cleared["unread"].as_u64()), (Some(2), Some(0)));
        let refused = registry.dispatch(&mut session, "ui.notifications.post", json!({ "body": " " }));
        assert!(refused.is_err());
    }

    #[test]
    fn a_link_used_once_goes_away_and_the_others_stay() {
        let mut session = Session::new();
        let id = post(
            &mut session,
            NewNotification::message(Group::LanguageServers, Kind::Warning, "ESLint is not installed")
                .action(Action::new("install", "Install".to_owned(), "lsp.server.install", json!({})).once())
                .action(Action::new("dismiss", "Don't ask".to_owned(), "lsp.server.dismiss", json!({})).once())
                .action(Action::new("log", "Show Log".to_owned(), "lsp.server.log", json!({}))),
        );
        let store = session.state_mut::<Notifications>();
        store.request(id, "log").unwrap();
        assert_eq!(store.get(id).unwrap().actions.len(), 3, "a link for looking stays");
        store.request(id, "install").unwrap();
        let left: Vec<&str> = store.get(id).unwrap().actions.iter().map(|action| action.id.as_str()).collect();
        assert_eq!(left, ["log"]);
        assert_eq!(store.take_queued().len(), 2);
    }

    #[test]
    fn a_posted_notification_cannot_carry_actions() {
        let (registry, mut session) = setup();
        let lure = json!({ "type": "error", "title": "Update failed", "body": "Click Retry",
                           "actions": [{ "id": "retry", "label": "Retry", "command": "terminal.session.run" }] });
        let refused = registry.dispatch(&mut session, "ui.notifications.post", lure);
        assert!(refused.is_err(), "{refused:?}");
        assert!(session.state_mut::<Notifications>().is_empty(), "nothing was posted");
        let posted = registry
            .dispatch(&mut session, "ui.notifications.post", json!({ "group": "git", "type": "warning", "body": "b" }))
            .unwrap();
        let id = posted["id"].as_u64().unwrap();
        let item = session.state_mut::<Notifications>().get(id).cloned().unwrap();
        assert_eq!((item.group, item.kind, item.balloon, item.actions.len()), (Group::Git, Kind::Warning, true, 0));
    }

    #[test]
    fn long_titles_and_bodies_are_cut() {
        let mut store = Notifications::default();
        let long = "я".repeat(BODY_LIMIT * 3);
        let id = store.add(note(&long).titled("t".repeat(TITLE_LIMIT + 1)), 0);
        let item = store.get(id).unwrap();
        assert_eq!(item.body.chars().count(), BODY_LIMIT);
        assert!(item.body.ends_with('…'));
        assert_eq!(item.title.chars().count(), TITLE_LIMIT);
        assert!(item.title.ends_with('…'));
        assert!(item.body.capacity() < BODY_LIMIT * 3, "the cut text frees the rest");
        let exact = "x".repeat(BODY_LIMIT);
        let id = store.add(note(&exact).titled("short"), 0);
        assert_eq!(
            store.get(id).map(|item| (item.body.as_str(), item.title.as_str())),
            Some((exact.as_str(), "short"))
        );
    }

    #[test]
    fn commands_are_grouped_by_their_area() {
        assert_eq!(Group::of_command("vcs.remote.push"), Some(Group::Git));
        assert_eq!(Group::of_command("view.vcs.push"), Some(Group::Git));
        assert_eq!(Group::of_command("debug.session.start"), Some(Group::Debugger));
        Group::declare("demo.debugger.", "debugger");
        Group::declare("demo.server.", "languageServers");
        Group::declare("demo.nowhere.", "no such group");
        assert_eq!(Group::of_command("demo.debugger.install"), Some(Group::Debugger));
        assert_eq!(Group::of_command("demo.server.install"), Some(Group::LanguageServers));
        assert_eq!(Group::of_command("demo.nowhere.x"), None);
        assert_eq!(Group::of_command("lsp.server.install"), Some(Group::LanguageServers));
        assert_eq!(Group::of_command("file.document.save"), None);
        let ids: Vec<&str> = Group::ALL.iter().map(|group| group.id()).collect();
        for group in Group::ALL {
            assert_eq!(serde_json::to_value(group).unwrap(), json!(group.id()));
        }
        assert_eq!(ids.len(), 8);
    }
}
