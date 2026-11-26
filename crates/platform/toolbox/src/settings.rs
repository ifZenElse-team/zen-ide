use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{CommandError, Settings};
use zen_i18n::t;
use zen_update::install::Installation;

use crate::ShellScripts;

pub const TOOLS_LOCATION: &str = "toolbox.tools.location";
pub const PREVIOUS_LOCATIONS: &str = "toolbox.tools.previousLocations";
pub const CHANNEL: &str = "toolbox.channel";
pub const UPDATES_CHECK: &str = "toolbox.updates.check";
pub const UPDATES_TOOLS: &str = "toolbox.updates.tools";
pub const UPDATES_SELF: &str = "toolbox.updates.self";
pub const UPDATES_AUTOMATIC: &str = "toolbox.updates.automatic";
pub const UPDATES_HELD: &str = "toolbox.updates.held";
pub const SHELL_SCRIPTS_ENABLED: &str = "toolbox.shellScripts.enabled";
pub const SHELL_SCRIPTS_LOCATION: &str = "toolbox.shellScripts.location";
pub const LAUNCH_AT_LOGIN: &str = "toolbox.launchAtLogin";
pub const NOTIFICATIONS: &str = "toolbox.notifications.enabled";
pub const NOTIFIED: &str = "toolbox.notifications.told";
pub const THEME: &str = "ui.theme";
pub const LANGUAGE: &str = "ui.language";

/// A release channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    /// Stable releases.
    #[default]
    Stable,
    /// Nightly builds.
    Nightly,
}

/// How Zen Toolbox checks for releases.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UpdateCheck {
    /// At start, when the window opens and every half hour.
    #[default]
    Automatic,
    /// Only when the user asks.
    Manual,
}

/// What Zen Toolbox does when an update is found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UpdateMode {
    /// Shows the update and waits for the user.
    #[default]
    Notify,
    /// Installs the update in the background.
    Automatic,
}

/// The colour theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follows the operating system.
    #[default]
    System,
    /// The dark theme.
    Dark,
    /// The light theme.
    Light,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Nightly => "nightly",
        }
    }

    pub fn parse(value: &str) -> Result<Self, CommandError> {
        parse_choice(CHANNEL, value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Defaults {
    pub tools_location: Option<PathBuf>,
    pub shell_scripts_location: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl Defaults {
    pub fn for_this_user() -> Self {
        Self {
            tools_location: Installation::default_root(),
            shell_scripts_location: crate::launchers::default_location(),
            home: zen_update::scripts::home(),
        }
    }
}

/// Every Zen Toolbox setting, with defaults filled in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxSettings {
    /// Where new tools are installed (`toolbox.tools.location`); installed tools are not moved.
    pub tools_location: Option<PathBuf>,
    /// The release channel (`toolbox.channel`).
    pub channel: Channel,
    /// How releases are checked (`toolbox.updates.check`).
    pub updates_check: UpdateCheck,
    /// What happens when a tool has an update (`toolbox.updates.tools`).
    pub updates_tools: UpdateMode,
    /// What happens when Zen Toolbox has an update (`toolbox.updates.self`).
    pub updates_self: UpdateMode,
    /// Whether shell scripts that launch the tools are generated (`toolbox.shellScripts.enabled`).
    pub shell_scripts_enabled: bool,
    /// Where shell scripts are generated (`toolbox.shellScripts.location`).
    pub shell_scripts_location: Option<PathBuf>,
    /// Whether Zen Toolbox starts in the tray when the user logs in (`toolbox.launchAtLogin`).
    pub launch_at_login: bool,
    /// Whether a new version shows a system notification (`toolbox.notifications.enabled`).
    pub notifications: bool,
    /// The colour theme (`ui.theme`).
    pub theme: Theme,
    /// The interface language, a language code or `system` (`ui.language`); `en` until the person
    /// chooses, whatever the system speaks.
    pub language: String,
}

impl ToolboxSettings {
    pub fn read(settings: &Settings, defaults: &Defaults) -> Self {
        let language: String = settings.get_or(LANGUAGE, zen_i18n::SOURCE.to_owned());
        Self {
            tools_location: absolute(settings, TOOLS_LOCATION).or_else(|| defaults.tools_location.clone()),
            channel: settings.get_or(CHANNEL, Channel::default()),
            updates_check: settings.get_or(UPDATES_CHECK, UpdateCheck::default()),
            updates_tools: settings.get_or(UPDATES_TOOLS, UpdateMode::default()),
            updates_self: settings.get_or(UPDATES_SELF, UpdateMode::default()),
            shell_scripts_enabled: settings.get_or(SHELL_SCRIPTS_ENABLED, true),
            shell_scripts_location: absolute(settings, SHELL_SCRIPTS_LOCATION)
                .or_else(|| defaults.shell_scripts_location.clone()),
            launch_at_login: settings.get_or(LAUNCH_AT_LOGIN, true),
            notifications: settings.get_or(NOTIFICATIONS, true),
            theme: settings.get_or(THEME, Theme::default()),
            language: if valid_language(&language) { language } else { zen_i18n::SOURCE.to_owned() },
        }
    }
}

pub fn previous_locations(settings: &Settings) -> Vec<PathBuf> {
    settings
        .get::<Vec<PathBuf>>(PREVIOUS_LOCATIONS)
        .unwrap_or_default()
        .into_iter()
        .filter(|path| path.is_absolute())
        .collect()
}

/// Parameters of `toolbox.settings.set`: only the settings to change.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    /// A new absolute tools directory, for new installations only.
    #[serde(default)]
    pub tools_location: Option<String>,
    /// A new release channel.
    #[serde(default)]
    #[schemars(with = "Option<Channel>")]
    pub channel: Option<String>,
    /// How releases are checked.
    #[serde(default)]
    #[schemars(with = "Option<UpdateCheck>")]
    pub updates_check: Option<String>,
    /// What happens when a tool has an update.
    #[serde(default)]
    #[schemars(with = "Option<UpdateMode>")]
    pub updates_tools: Option<String>,
    /// What happens when Zen Toolbox has an update.
    #[serde(default)]
    #[schemars(with = "Option<UpdateMode>")]
    pub updates_self: Option<String>,
    /// Whether shell scripts are generated.
    #[serde(default)]
    pub shell_scripts_enabled: Option<bool>,
    /// A new absolute shell scripts directory.
    #[serde(default)]
    pub shell_scripts_location: Option<String>,
    /// Whether Zen Toolbox starts in the tray at login; turning it off removes the login item.
    #[serde(default)]
    pub launch_at_login: Option<bool>,
    /// Whether a new version shows a system notification.
    #[serde(default)]
    pub notifications: Option<bool>,
    /// The colour theme.
    #[serde(default)]
    #[schemars(with = "Option<Theme>")]
    pub theme: Option<String>,
    /// The interface language: a language code such as `en` (the default) or `ru`, or `system` to
    /// follow the system's language; remembered across restarts and updates.
    #[serde(default)]
    pub language: Option<String>,
}

impl SettingsPatch {
    pub fn apply(&self, settings: &mut Settings, defaults: &Defaults) -> Result<(), CommandError> {
        let tools_location =
            self.tools_location.as_deref().map(|path| absolute_path(TOOLS_LOCATION, path)).transpose()?;
        let shell_scripts_location = self
            .shell_scripts_location
            .as_deref()
            .map(|path| absolute_path(SHELL_SCRIPTS_LOCATION, path))
            .transpose()?;
        let channel = self.channel.as_deref().map(Channel::parse).transpose()?;
        let updates_check =
            self.updates_check.as_deref().map(|value| parse_choice::<UpdateCheck>(UPDATES_CHECK, value)).transpose()?;
        let updates_tools =
            self.updates_tools.as_deref().map(|value| parse_choice::<UpdateMode>(UPDATES_TOOLS, value)).transpose()?;
        let updates_self =
            self.updates_self.as_deref().map(|value| parse_choice::<UpdateMode>(UPDATES_SELF, value)).transpose()?;
        let theme = self.theme.as_deref().map(|value| parse_choice::<Theme>(THEME, value)).transpose()?;
        if let Some(language) = self.language.as_deref().filter(|language| !valid_language(language)) {
            return Err(invalid(LANGUAGE, language, &language_choices()));
        }

        if let Some(location) = tools_location {
            let current = ToolboxSettings::read(settings, defaults).tools_location;
            let mut previous = previous_locations(settings);
            if let Some(current) = current.filter(|current| *current != location) {
                previous.retain(|path| *path != current);
                previous.insert(0, current);
            }
            previous.retain(|path| *path != location);
            write(settings, PREVIOUS_LOCATIONS, &previous)?;
            write(settings, TOOLS_LOCATION, &location)?;
        }
        if let Some(location) = shell_scripts_location {
            write(settings, SHELL_SCRIPTS_LOCATION, &location)?;
        }
        if let Some(channel) = channel {
            write(settings, CHANNEL, channel)?;
        }
        if let Some(check) = updates_check {
            write(settings, UPDATES_CHECK, check)?;
        }
        if let Some(mode) = updates_tools {
            write(settings, UPDATES_TOOLS, mode)?;
        }
        if let Some(mode) = updates_self {
            write(settings, UPDATES_SELF, mode)?;
        }
        if let Some(enabled) = self.shell_scripts_enabled {
            write(settings, SHELL_SCRIPTS_ENABLED, enabled)?;
        }
        if let Some(enabled) = self.launch_at_login {
            write(settings, LAUNCH_AT_LOGIN, enabled)?;
        }
        if let Some(enabled) = self.notifications {
            write(settings, NOTIFICATIONS, enabled)?;
        }
        if let Some(theme) = theme {
            write(settings, THEME, theme)?;
        }
        if let Some(language) = &self.language {
            write(settings, LANGUAGE, language)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandLine {
    pub channel: Option<Channel>,
    pub tools_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunSettings {
    pub channel: Channel,
    pub tools_dir: Option<PathBuf>,
    pub other_tools_dirs: Vec<PathBuf>,
    pub updates_check: UpdateCheck,
    pub updates_self: UpdateMode,
    pub shell_scripts: ShellScripts,
    pub launch_at_login: bool,
}

pub fn for_this_run(settings: &Settings, defaults: &Defaults, command_line: &CommandLine) -> RunSettings {
    let read = ToolboxSettings::read(settings, defaults);
    let (tools_dir, other_tools_dirs) = match &command_line.tools_dir {
        Some(dir) => (Some(dir.clone()), Vec::new()),
        None => (read.tools_location, previous_locations(settings)),
    };
    RunSettings {
        channel: command_line.channel.unwrap_or(read.channel),
        tools_dir,
        other_tools_dirs,
        updates_check: read.updates_check,
        updates_self: read.updates_self,
        shell_scripts: ShellScripts {
            enabled: read.shell_scripts_enabled && command_line.tools_dir.is_none(),
            location: read.shell_scripts_location,
        },
        launch_at_login: read.launch_at_login,
    }
}

pub fn check_due(check: UpdateCheck, since_last_check: Option<Duration>, stale_after: Duration) -> bool {
    check == UpdateCheck::Automatic && since_last_check.is_none_or(|since| since > stale_after)
}

fn absolute(settings: &Settings, key: &str) -> Option<PathBuf> {
    settings.get::<PathBuf>(key).filter(|path| path.is_absolute())
}

fn absolute_path(key: &str, value: &str) -> Result<PathBuf, CommandError> {
    let path = Path::new(value);
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Err(CommandError::new(t!("toolbox-settings-error-relative-path", key = key, path = value)))
    }
}

fn parse_choice<T: Choice>(key: &str, value: &str) -> Result<T, CommandError> {
    T::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(value))
        .map_err(|_| invalid(key, value, &T::NAMES.join(", ")))
}

trait Choice: serde::de::DeserializeOwned {
    const NAMES: &'static [&'static str];
}

impl Choice for Channel {
    const NAMES: &'static [&'static str] = &["stable", "nightly"];
}

impl Choice for UpdateCheck {
    const NAMES: &'static [&'static str] = &["automatic", "manual"];
}

impl Choice for UpdateMode {
    const NAMES: &'static [&'static str] = &["notify", "automatic"];
}

impl Choice for Theme {
    const NAMES: &'static [&'static str] = &["system", "dark", "light"];
}

fn valid_language(language: &str) -> bool {
    language == "system" || zen_i18n::LANGUAGES.iter().any(|(code, _)| *code == language)
}

fn language_choices() -> String {
    std::iter::once("system").chain(zen_i18n::LANGUAGES.iter().map(|(code, _)| *code)).collect::<Vec<_>>().join(", ")
}

fn invalid(key: &str, value: &str, expected: &str) -> CommandError {
    CommandError::new(t!("toolbox-settings-error-value", key = key, value = value, expected = expected))
}

fn write<T: Serialize>(settings: &mut Settings, key: &str, value: T) -> Result<(), CommandError> {
    settings
        .set(key, value)
        .map_err(|error| CommandError::new(t!("toolbox-settings-error-write", key = key, error = error.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults(dir: &Path) -> Defaults {
        Defaults {
            tools_location: Some(dir.join("tools")),
            shell_scripts_location: Some(dir.join("bin")),
            home: Some(dir.join("home")),
        }
    }

    fn english() {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn toolbox_speaks_english_until_a_language_is_chosen_and_keeps_the_choice() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        assert_eq!(
            ToolboxSettings::read(&settings, &defaults(dir.path())).language,
            "en",
            "whatever the system speaks"
        );
        let patch = SettingsPatch { language: Some("ru".to_owned()), ..SettingsPatch::default() };
        patch.apply(&mut settings, &defaults(dir.path())).unwrap();
        let saved = serde_json::to_string(&settings).unwrap();
        let reread: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(ToolboxSettings::read(&reread, &defaults(dir.path())).language, "ru", "kept: {saved}");
        let patch = SettingsPatch { language: Some("system".to_owned()), ..SettingsPatch::default() };
        patch.apply(&mut settings, &defaults(dir.path())).unwrap();
        assert_eq!(ToolboxSettings::read(&settings, &defaults(dir.path())).language, "system");
    }

    #[test]
    fn defaults_fill_every_setting() {
        let dir = tempfile::tempdir().unwrap();
        let read = ToolboxSettings::read(&Settings::default(), &defaults(dir.path()));
        assert_eq!(
            read,
            ToolboxSettings {
                tools_location: Some(dir.path().join("tools")),
                channel: Channel::Stable,
                updates_check: UpdateCheck::Automatic,
                updates_tools: UpdateMode::Notify,
                updates_self: UpdateMode::Notify,
                shell_scripts_enabled: true,
                shell_scripts_location: Some(dir.path().join("bin")),
                launch_at_login: true,
                notifications: true,
                theme: Theme::System,
                language: "en".into(),
            }
        );
    }

    #[test]
    fn launch_at_login_is_on_by_default_and_can_be_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        assert!(for_this_run(&settings, &defaults(dir.path()), &CommandLine::default()).launch_at_login);
        SettingsPatch { launch_at_login: Some(false), ..SettingsPatch::default() }
            .apply(&mut settings, &defaults(dir.path()))
            .unwrap();
        assert_eq!(settings.get::<bool>(LAUNCH_AT_LOGIN), Some(false));
        assert!(!ToolboxSettings::read(&settings, &defaults(dir.path())).launch_at_login);
        assert!(!for_this_run(&settings, &defaults(dir.path()), &CommandLine::default()).launch_at_login);
        settings.set(LAUNCH_AT_LOGIN, "yes").unwrap();
        assert!(
            ToolboxSettings::read(&settings, &defaults(dir.path())).launch_at_login,
            "a wrong value reads as the default"
        );
    }

    #[test]
    fn invalid_stored_values_read_as_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        settings.set(CHANNEL, "beta").unwrap();
        settings.set(TOOLS_LOCATION, "relative/tools").unwrap();
        settings.set(THEME, 3).unwrap();
        settings.set(LANGUAGE, "xx").unwrap();
        let read = ToolboxSettings::read(&settings, &defaults(dir.path()));
        assert_eq!(read.channel, Channel::Stable);
        assert_eq!(read.tools_location, Some(dir.path().join("tools")));
        assert_eq!(read.theme, Theme::System);
        assert_eq!(read.language, "en", "an invalid language is English");
    }

    #[test]
    fn a_partial_patch_writes_only_its_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        SettingsPatch {
            channel: Some("nightly".into()),
            shell_scripts_enabled: Some(true),
            ..SettingsPatch::default()
        }
        .apply(&mut settings, &defaults(dir.path()))
        .unwrap();
        let keys: Vec<&str> = settings.iter().map(|(key, _)| key).collect();
        assert_eq!(keys, [CHANNEL, SHELL_SCRIPTS_ENABLED]);
        let read = ToolboxSettings::read(&settings, &defaults(dir.path()));
        assert_eq!(read.channel, Channel::Nightly);
        assert!(read.shell_scripts_enabled);
        assert_eq!(read.theme, Theme::System);
    }

    #[test]
    fn every_value_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let tools = dir.path().join("elsewhere");
        let scripts = dir.path().join("scripts");
        SettingsPatch {
            tools_location: Some(tools.display().to_string()),
            channel: Some("nightly".into()),
            updates_check: Some("manual".into()),
            updates_tools: Some("automatic".into()),
            updates_self: Some("automatic".into()),
            shell_scripts_enabled: Some(true),
            shell_scripts_location: Some(scripts.display().to_string()),
            launch_at_login: Some(false),
            notifications: Some(false),
            theme: Some("light".into()),
            language: Some("en".into()),
        }
        .apply(&mut settings, &defaults(dir.path()))
        .unwrap();
        assert_eq!(
            ToolboxSettings::read(&settings, &defaults(dir.path())),
            ToolboxSettings {
                tools_location: Some(tools),
                channel: Channel::Nightly,
                updates_check: UpdateCheck::Manual,
                updates_tools: UpdateMode::Automatic,
                updates_self: UpdateMode::Automatic,
                shell_scripts_enabled: true,
                shell_scripts_location: Some(scripts),
                launch_at_login: false,
                notifications: false,
                theme: Theme::Light,
                language: "en".into(),
            }
        );
        assert_eq!(settings.get::<String>(UPDATES_CHECK).as_deref(), Some("manual"));
    }

    #[test]
    fn invalid_values_are_rejected_and_nothing_is_written() {
        english();
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let error =
            SettingsPatch { channel: Some("nightly".into()), theme: Some("blue".into()), ..SettingsPatch::default() }
                .apply(&mut settings, &defaults(dir.path()))
                .unwrap_err();
        assert_eq!(error.to_string(), "“blue” is not a valid value of ui.theme. Use one of: system, dark, light.");
        assert_eq!(settings.iter().count(), 0);

        let cases = [
            SettingsPatch { channel: Some("beta".into()), ..SettingsPatch::default() },
            SettingsPatch { updates_check: Some("hourly".into()), ..SettingsPatch::default() },
            SettingsPatch { updates_tools: Some("never".into()), ..SettingsPatch::default() },
            SettingsPatch { updates_self: Some("Notify".into()), ..SettingsPatch::default() },
            SettingsPatch { language: Some("xx".into()), ..SettingsPatch::default() },
            SettingsPatch { tools_location: Some("tools".into()), ..SettingsPatch::default() },
            SettingsPatch { tools_location: Some(String::new()), ..SettingsPatch::default() },
            SettingsPatch { shell_scripts_location: Some("./bin".into()), ..SettingsPatch::default() },
        ];
        for patch in cases {
            assert!(patch.apply(&mut settings, &defaults(dir.path())).is_err(), "{patch:?}");
        }
        assert_eq!(settings.iter().count(), 0);
        let error = SettingsPatch { tools_location: Some("tools".into()), ..SettingsPatch::default() }
            .apply(&mut settings, &defaults(dir.path()))
            .unwrap_err();
        assert_eq!(error.to_string(), "toolbox.tools.location must be an absolute path, not “tools”.");
    }

    #[test]
    fn unknown_parameters_are_rejected() {
        assert!(serde_json::from_value::<SettingsPatch>(serde_json::json!({ "chanel": "nightly" })).is_err());
        let patch: SettingsPatch = serde_json::from_value(serde_json::json!({ "updatesCheck": "manual" })).unwrap();
        assert_eq!(patch.updates_check.as_deref(), Some("manual"));
    }

    #[test]
    fn a_new_tools_location_remembers_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let defaults = defaults(dir.path());
        let mut settings = Settings::default();
        let (first, second) = (dir.path().join("first"), dir.path().join("second"));
        let to = |path: &Path| SettingsPatch {
            tools_location: Some(path.display().to_string()),
            ..SettingsPatch::default()
        };
        to(&first).apply(&mut settings, &defaults).unwrap();
        assert_eq!(previous_locations(&settings), [dir.path().join("tools")]);
        to(&second).apply(&mut settings, &defaults).unwrap();
        assert_eq!(previous_locations(&settings), [first.clone(), dir.path().join("tools")]);
        to(&first).apply(&mut settings, &defaults).unwrap();
        assert_eq!(previous_locations(&settings), [second, dir.path().join("tools")]);
        assert_eq!(ToolboxSettings::read(&settings, &defaults).tools_location, Some(first));
    }

    #[test]
    fn the_command_line_wins_for_this_run() {
        let dir = tempfile::tempdir().unwrap();
        let defaults = defaults(dir.path());
        let mut settings = Settings::default();
        SettingsPatch {
            tools_location: Some(dir.path().join("chosen").display().to_string()),
            channel: Some("nightly".into()),
            updates_check: Some("manual".into()),
            ..SettingsPatch::default()
        }
        .apply(&mut settings, &defaults)
        .unwrap();

        let from_settings = for_this_run(&settings, &defaults, &CommandLine::default());
        assert_eq!(
            from_settings,
            RunSettings {
                channel: Channel::Nightly,
                tools_dir: Some(dir.path().join("chosen")),
                other_tools_dirs: vec![dir.path().join("tools")],
                updates_check: UpdateCheck::Manual,
                updates_self: UpdateMode::Notify,
                shell_scripts: ShellScripts { enabled: true, location: Some(dir.path().join("bin")) },
                launch_at_login: true,
            }
        );

        let command_line = CommandLine { channel: Some(Channel::Stable), tools_dir: Some(dir.path().join("flag")) };
        let overridden = for_this_run(&settings, &defaults, &command_line);
        assert_eq!(overridden.channel, Channel::Stable);
        assert_eq!(overridden.tools_dir, Some(dir.path().join("flag")));
        assert!(overridden.other_tools_dirs.is_empty(), "a tools directory on the command line is the only one");
        assert_eq!(overridden.updates_check, UpdateCheck::Manual);

        let empty = for_this_run(&Settings::default(), &defaults, &CommandLine::default());
        assert_eq!(empty.channel, Channel::Stable);
        assert_eq!(empty.tools_dir, Some(dir.path().join("tools")));
    }

    #[test]
    fn manual_checks_are_never_due() {
        let hour = Duration::from_secs(3600);
        let minute = Duration::from_secs(60);
        assert!(check_due(UpdateCheck::Automatic, None, minute));
        assert!(check_due(UpdateCheck::Automatic, Some(hour), minute));
        assert!(!check_due(UpdateCheck::Automatic, Some(Duration::from_secs(1)), minute));
        assert!(!check_due(UpdateCheck::Manual, None, minute));
        assert!(!check_due(UpdateCheck::Manual, Some(hour), minute));
    }

    #[test]
    fn channels_parse_by_name() {
        assert_eq!(Channel::parse("nightly").unwrap(), Channel::Nightly);
        assert_eq!(Channel::Nightly.as_str(), "nightly");
        assert!(Channel::parse("Stable").is_err());
    }
}
