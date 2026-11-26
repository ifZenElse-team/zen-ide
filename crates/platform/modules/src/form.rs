use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A settings page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Form {
    /// What the page shows, from top to bottom.
    pub items: Vec<Item>,
    /// Whether something runs in the background (an installation), so that the page asks again soon.
    pub busy: bool,
}

/// One thing on a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    /// The title of a group of settings.
    Group { title: String },
    Text {
        /// The text.
        text: String,
        /// How it is drawn.
        #[serde(default)]
        tone: Tone,
        /// Whether a spinner goes before it, for something running.
        #[serde(default)]
        spinner: bool,
    },
    /// A link to a web page or to another settings page.
    Link {
        /// The link's text.
        label: String,
        /// Where it goes.
        target: Target,
    },
    /// Items side by side.
    Row {
        /// The items, from left to right.
        items: Vec<Item>,
    },
    /// A drop-down list; picking an option runs the action with its value.
    Choice {
        /// What the list chooses, for screen readers and agents.
        label: String,
        /// The options.
        options: Vec<Choice>,
        /// The value of the option chosen now.
        selected: Option<String>,
        /// What picking another option runs.
        action: Action,
    },
    /// A button that opens a menu of actions.
    Menu {
        /// The button's text.
        label: String,
        /// The menu's entries.
        entries: Vec<MenuEntry>,
    },
    /// A button.
    Button {
        /// Its text.
        /// The field's label.
        label: String,
        /// Whether it can be clicked now.
        #[serde(default = "yes")]
        enabled: bool,
        /// What a click runs.
        action: Action,
    },
    /// A field for a program's path; when the text changes and the field loses focus, the action
    /// runs with the path, or with `null` when the field was emptied.
    Path {
        label: String,
        /// The path saved now, if any.
        value: Option<String>,
        /// What a change runs.
        action: Action,
    },
    /// A check box whose change runs the action with `true` or `false`.
    Check {
        label: String,
        /// Whether it is ticked now.
        value: bool,
        /// What a change runs.
        action: Action,
    },
    /// A text field; when the text changes and the field loses focus, the action runs with the
    /// text, or with `null` when the field was emptied.
    Field {
        label: String,
        /// The text saved now.
        value: String,
        /// What a change runs.
        action: Action,
        /// What its Browse button picks, if it has one.
        #[serde(default)]
        browse: Browse,
    },
    /// An on/off setting of the IDE kept in `settings.json`, applied with the dialog's OK: a key of
    /// the IDE's own (`reformatOnSave`, `optimizeImportsOnSave`) or a declared setting's key.
    Setting {
        /// The setting's key.
        key: String,
        /// The check box's text.
        label: String,
        /// Its value until the user changes it, for a declared setting.
        #[serde(default)]
        default: bool,
    },
}

/// How a text line is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    /// As ordinary text.
    #[default]
    Normal,
    /// Quieter, as a note.
    Secondary,
    /// As an error.
    Error,
}

/// What a field's Browse button picks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Browse {
    /// No Browse button.
    #[default]
    None,
    File,
    Folder,
}

/// Where a link goes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    /// A web page, opened in the browser.
    Url { url: String },
    /// Another settings page, by the name agents use (`actionsOnSave`).
    Page { page: String },
}

/// One option of a [`Item::Choice`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Choice {
    /// The value the action gets.
    pub value: String,
    /// What the list shows.
    pub label: String,
}

/// One entry of a [`Item::Menu`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MenuEntry {
    /// Its text.
    pub label: String,
    /// What it runs.
    pub action: Action,
}

/// What the user does on a page: a command with its parameters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Action {
    /// The command's id.
    pub command: String,
    /// Its parameters, an object.
    pub params: serde_json::Map<String, serde_json::Value>,
    /// The parameter that gets the value chosen, typed or picked, if any.
    pub value: Option<String>,
    /// When set, a folder is picked first (this is the picker's title) and goes into `value`.
    pub pick_folder: Option<String>,
}

impl Action {
    pub fn command(command: &str) -> Self {
        Self { command: command.to_owned(), ..Self::default() }
    }

    pub fn with_value(command: &str, value: &str) -> Self {
        Self { value: Some(value.to_owned()), ..Self::command(command) }
    }

    pub fn param(mut self, name: &str, value: impl Into<serde_json::Value>) -> Self {
        self.params.insert(name.to_owned(), value.into());
        self
    }

    pub fn picking_folder(mut self, title: &str) -> Self {
        self.pick_folder = Some(title.to_owned());
        self
    }

    pub fn params_with(&self, value: Option<serde_json::Value>) -> serde_json::Value {
        let mut params = self.params.clone();
        if let Some(name) = &self.value {
            params.insert(name.clone(), value.unwrap_or(serde_json::Value::Null));
        }
        serde_json::Value::Object(params)
    }
}

fn yes() -> bool {
    true
}

impl Form {
    pub fn texts(&self) -> Vec<&str> {
        fn walk<'a>(items: &'a [Item], found: &mut Vec<&'a str>) {
            for item in items {
                match item {
                    Item::Group { title } => found.push(title),
                    Item::Text { text, .. } => found.push(text),
                    Item::Link { label, .. }
                    | Item::Choice { label, .. }
                    | Item::Button { label, .. }
                    | Item::Path { label, .. }
                    | Item::Check { label, .. }
                    | Item::Field { label, .. }
                    | Item::Setting { label, .. } => found.push(label),
                    Item::Menu { label, entries } => {
                        found.push(label);
                        found.extend(entries.iter().map(|entry| entry.label.as_str()));
                    }
                    Item::Row { items } => walk(items, found),
                }
            }
        }
        let mut found = Vec::new();
        walk(&self.items, &mut found);
        found
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_form_reads_and_writes_the_json_agents_see() {
        let form = Form {
            items: vec![
                Item::Group { title: "GOROOT".to_owned() },
                Item::Row {
                    items: vec![
                        Item::Choice {
                            label: "GOROOT".to_owned(),
                            options: vec![Choice { value: "/sdk".to_owned(), label: "go1.27 /sdk".to_owned() }],
                            selected: Some("/sdk".to_owned()),
                            action: Action::with_value("toolchain.go.choose", "root"),
                        },
                        Item::Button {
                            label: "Refresh".to_owned(),
                            enabled: true,
                            action: Action::command("toolchain.go.refresh"),
                        },
                    ],
                },
                Item::Text { text: "Note".to_owned(), tone: Tone::Secondary, spinner: false },
            ],
            busy: false,
        };
        let value = serde_json::to_value(&form).unwrap();
        assert_eq!(value["items"][1]["kind"], "row");
        assert_eq!(value["items"][1]["items"][0]["action"]["value"], "root");
        assert_eq!(serde_json::from_value::<Form>(value).unwrap(), form);
        assert_eq!(form.texts(), ["GOROOT", "GOROOT", "Refresh", "Note"]);
        let button: Item = serde_json::from_value(json!({
            "kind": "button", "label": "Install", "action": { "command": "toolchain.dlv.install" }
        }))
        .unwrap();
        assert!(matches!(button, Item::Button { enabled: true, .. }), "a button is enabled unless it says not");
    }

    #[test]
    fn the_value_goes_into_the_named_parameter() {
        let action = Action::with_value("toolchain.gopls.choose", "path").param("fixed", 1);
        assert_eq!(action.params_with(Some(json!("/bin/gopls"))), json!({ "fixed": 1, "path": "/bin/gopls" }));
        assert_eq!(action.params_with(None), json!({ "fixed": 1, "path": null }));
        assert_eq!(Action::command("toolchain.go.refresh").params_with(Some(json!("x"))), json!({}));
    }
}
