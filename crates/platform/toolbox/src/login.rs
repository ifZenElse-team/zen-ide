use std::path::{Path, PathBuf};

use zen_i18n::t;

pub const TRAY_ARGUMENT: &str = "--tray";

pub const LABEL: &str = "space.ifzenelse.toolbox";

pub const DIR_VARIABLE: &str = "ZEN_LAUNCH_AGENTS_DIR";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    LaunchAgent,
    Autostart,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginItem {
    pub dir: PathBuf,
    pub executable: PathBuf,
    pub format: Format,
}

/// Whether Zen Toolbox starts at login, as the `toolbox.login.status` command reports it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    /// The setting, `toolbox.launchAtLogin`.
    pub enabled: bool,
    /// Whether this Zen Toolbox manages a login item here: an installed build on macOS or Linux.
    /// Development builds and the demo never register themselves.
    pub supported: bool,
    /// Whether the login item is in place, so that Zen Toolbox starts in the tray at the next login.
    pub registered: bool,
    /// The login item's file, when supported.
    pub file: Option<PathBuf>,
}

impl LoginItem {
    pub fn for_this_user(executable: PathBuf) -> Option<Self> {
        let env = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let format = if cfg!(target_os = "macos") {
            Format::LaunchAgent
        } else if cfg!(windows) {
            return None;
        } else {
            Format::Autostart
        };
        let dir = match env(DIR_VARIABLE) {
            Some(dir) => dir,
            None => match format {
                Format::LaunchAgent => env("HOME")?.join("Library/LaunchAgents"),
                Format::Autostart => {
                    env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|home| home.join(".config")))?.join("autostart")
                }
            },
        };
        Some(Self { dir, executable, format })
    }

    pub fn file(&self) -> PathBuf {
        let extension = match self.format {
            Format::LaunchAgent => "plist",
            Format::Autostart => "desktop",
        };
        self.dir.join(format!("{LABEL}.{extension}"))
    }

    pub fn contents(&self) -> String {
        let executable = self.executable.display().to_string();
        match self.format {
            Format::LaunchAgent => format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\">\n\
                 <dict>\n\
                 \t<key>Label</key>\n\
                 \t<string>{LABEL}</string>\n\
                 \t<key>AssociatedBundleIdentifiers</key>\n\
                 \t<string>{LABEL}</string>\n\
                 \t<key>ProgramArguments</key>\n\
                 \t<array>\n\
                 \t\t<string>{}</string>\n\
                 \t\t<string>{TRAY_ARGUMENT}</string>\n\
                 \t</array>\n\
                 \t<key>RunAtLoad</key>\n\
                 \t<true/>\n\
                 \t<key>ProcessType</key>\n\
                 \t<string>Interactive</string>\n\
                 \t<key>LimitLoadToSessionType</key>\n\
                 \t<string>Aqua</string>\n\
                 </dict>\n\
                 </plist>\n",
                xml_escape(&executable)
            ),
            Format::Autostart => format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=Zen Toolbox\n\
                 Exec={} {TRAY_ARGUMENT}\n\
                 Icon={LABEL}\n\
                 NoDisplay=true\n\
                 X-GNOME-Autostart-enabled=true\n\
                 X-Zen-Toolbox-Label={LABEL}\n",
                desktop_quote(&executable)
            ),
        }
    }

    pub fn is_registered(&self) -> bool {
        std::fs::read_to_string(self.file()).is_ok_and(|contents| contents == self.contents())
    }

    pub fn set(&self, enabled: bool) -> Result<bool, String> {
        let file = self.file();
        let existing = std::fs::read_to_string(&file).ok();
        if existing.as_deref().is_some_and(|contents| !ours(contents)) {
            return Err(t!("toolbox-login-not-ours", path = file.display().to_string()));
        }
        if enabled {
            let contents = self.contents();
            if existing.as_deref() == Some(contents.as_str()) {
                return Ok(false);
            }
            std::fs::create_dir_all(&self.dir).map_err(|error| describe(&self.dir, &error))?;
            std::fs::write(&file, contents).map_err(|error| describe(&file, &error))?;
            Ok(true)
        } else if existing.is_some() {
            std::fs::remove_file(&file).map_err(|error| describe(&file, &error))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn ours(contents: &str) -> bool {
    contents.contains(&format!("<string>{LABEL}</string>"))
        || contents.contains(&format!("X-Zen-Toolbox-Label={LABEL}"))
}

fn describe(path: &Path, error: &std::io::Error) -> String {
    t!("toolbox-login-io", path = path.display().to_string(), error = error.to_string())
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn desktop_quote(path: &str) -> String {
    let mut quoted = String::from("\"");
    for character in path.chars() {
        if matches!(character, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(dir: &Path, format: Format) -> LoginItem {
        LoginItem {
            dir: dir.join("LaunchAgents"),
            executable: "/Applications/Zen Toolbox.app/Contents/MacOS/zen-toolbox".into(),
            format,
        }
    }

    #[test]
    fn the_launch_agent_starts_toolbox_in_the_tray_at_login() {
        let dir = tempfile::tempdir().unwrap();
        let item = item(dir.path(), Format::LaunchAgent);
        assert_eq!(item.file(), dir.path().join("LaunchAgents/space.ifzenelse.toolbox.plist"));
        assert!(!item.is_registered());

        assert_eq!(item.set(true), Ok(true), "the folder is created and the plist written");
        let plist = std::fs::read_to_string(item.file()).unwrap();
        assert!(plist.contains("<key>Label</key>\n\t<string>space.ifzenelse.toolbox</string>"));
        assert!(plist.contains(
            "<array>\n\t\t<string>/Applications/Zen Toolbox.app/Contents/MacOS/zen-toolbox</string>\n\t\t<string>--tray</string>\n\t</array>"
        ));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(item.is_registered());
        assert_eq!(item.set(true), Ok(false), "an unchanged plist is not rewritten");

        let moved = LoginItem {
            executable: "/Users/me/Applications/Zen Toolbox.app/Contents/MacOS/zen-toolbox".into(),
            ..item.clone()
        };
        assert!(!moved.is_registered(), "a plist for another executable is out of date");
        assert_eq!(moved.set(true), Ok(true));
        assert!(moved.is_registered());

        assert_eq!(item.set(false), Ok(true), "turning it off removes the plist");
        assert!(!item.file().exists());
        assert_eq!(item.set(false), Ok(false));
    }

    #[test]
    fn a_file_toolbox_did_not_write_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let item = item(dir.path(), Format::LaunchAgent);
        std::fs::create_dir_all(&item.dir).unwrap();
        std::fs::write(item.file(), "<plist>someone else's</plist>").unwrap();
        assert!(item.set(false).is_err());
        assert!(item.set(true).is_err());
        assert_eq!(std::fs::read_to_string(item.file()).unwrap(), "<plist>someone else's</plist>");
    }

    #[test]
    fn paths_are_escaped() {
        let dir = tempfile::tempdir().unwrap();
        let mut item = item(dir.path(), Format::LaunchAgent);
        item.executable = "/Apps/R&D <test>/zen-toolbox".into();
        assert!(item.contents().contains("<string>/Apps/R&amp;D &lt;test&gt;/zen-toolbox</string>"));
        item.format = Format::Autostart;
        item.executable = "/home/me/my $apps/zen-toolbox".into();
        assert!(item.contents().contains("Exec=\"/home/me/my \\$apps/zen-toolbox\" --tray\n"));
    }

    #[test]
    fn the_autostart_entry_is_written_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let item = item(dir.path(), Format::Autostart);
        assert_eq!(item.file(), dir.path().join("LaunchAgents/space.ifzenelse.toolbox.desktop"));
        assert_eq!(item.set(true), Ok(true));
        let entry = std::fs::read_to_string(item.file()).unwrap();
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(entry.contains(" --tray\n"));
        assert_eq!(item.set(false), Ok(true));
        assert!(!item.file().exists());
    }
}
