use std::path::{Path, PathBuf};

use zen_project::settings::{InspectionProfile, InspectionTool, ProjectSettings, ReadFiles};

use super::xml;

const PROFILES: &str = ".idea/inspectionProfiles";
const SETTINGS: &str = "profiles_settings.xml";
const MANAGER: &str = "InspectionProjectProfileManager";
const DEFAULT_PROFILE: &str = "Project Default";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileChoice {
    pub project_profile: String,
    pub use_project_profile: bool,
}

impl Default for ProfileChoice {
    fn default() -> Self {
        Self { project_profile: DEFAULT_PROFILE.to_owned(), use_project_profile: true }
    }
}

pub fn read(root: &Path, files: &dyn ReadFiles, sources: &[PathBuf], settings: &mut ProjectSettings) {
    let in_folder: Vec<&PathBuf> =
        sources.iter().filter(|source| source.parent() == Some(Path::new(PROFILES))).collect();
    let mut choice = ProfileChoice::default();
    let mut profiles = Vec::new();
    for source in in_folder {
        let Some(text) = files.read(source) else { continue };
        let parsed = if source.file_name().is_some_and(|name| name == SETTINGS) {
            profile_choice(&text).map(|found| choice = found)
        } else {
            profile(&text, &root.join(source)).map(|found| profiles.extend(found))
        };
        if let Err(error) = parsed {
            settings.problems.push(super::parse_problem(root, source, &error));
        }
    }
    for profile in &mut profiles {
        profile.is_project_profile = choice.use_project_profile && profile.name == choice.project_profile;
    }
    settings.inspection_profiles.extend(profiles);
}

pub fn profile_choice(text: &str) -> Result<ProfileChoice, String> {
    let document = xml::parse(text)?;
    let mut choice = ProfileChoice::default();
    let Some(manager) = xml::component(&document, MANAGER) else { return Ok(choice) };
    for settings in manager.children().filter(|node| node.has_tag_name("settings")) {
        if let Some(name) = xml::option(settings, "PROJECT_PROFILE") {
            choice.project_profile = name.to_owned();
        }
        if let Some(used) = xml::option(settings, "USE_PROJECT_PROFILE") {
            choice.use_project_profile = used == "true";
        }
    }
    Ok(choice)
}

pub fn profile(text: &str, file: &Path) -> Result<Option<InspectionProfile>, String> {
    let document = xml::parse(text)?;
    let Some(manager) = xml::component(&document, MANAGER) else { return Ok(None) };
    let Some(node) = manager.children().find(|node| node.has_tag_name("profile")) else { return Ok(None) };
    let stem = file.file_stem().map(|stem| stem.to_string_lossy().replace('_', " ")).unwrap_or_default();
    let name = xml::option(node, "myName").map_or(stem, str::to_owned);
    let tools = node
        .children()
        .filter(|child| child.has_tag_name("inspection_tool"))
        .filter_map(|child| {
            Some(InspectionTool {
                class: child.attribute("class")?.to_owned(),
                enabled: child.attribute("enabled") == Some("true"),
                level: child.attribute("level").unwrap_or_default().to_owned(),
            })
        })
        .collect();
    Ok(Some(InspectionProfile { name, file: file.to_owned(), is_project_profile: false, tools }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_defaults_to_the_project_default_profile() {
        assert_eq!(profile_choice("<component name=\"Other\" />"), Ok(ProfileChoice::default()));
        let text = r#"<component name="InspectionProjectProfileManager"><settings>
            <option name="PROJECT_PROFILE" value="Strict" /><option name="USE_PROJECT_PROFILE" value="false" />
        </settings></component>"#;
        let choice = profile_choice(text).unwrap();
        assert_eq!(choice, ProfileChoice { project_profile: "Strict".to_owned(), use_project_profile: false });
        assert!(profile_choice("<component").is_err());
    }

    #[test]
    fn a_profile_without_a_name_is_named_after_its_file() {
        let text = r#"<component name="InspectionProjectProfileManager"><profile version="1.0">
            <inspection_tool class="A" enabled="true" level="ERROR" />
            <inspection_tool enabled="true" />
            <inspection_tool class="B" />
        </profile></component>"#;
        let found = profile(text, Path::new("/p/.idea/inspectionProfiles/Team_Strict.xml")).unwrap().unwrap();
        assert_eq!(found.name, "Team Strict");
        assert_eq!(
            found.tools,
            [
                InspectionTool { class: "A".to_owned(), enabled: true, level: "ERROR".to_owned() },
                InspectionTool { class: "B".to_owned(), enabled: false, level: String::new() },
            ]
        );
        assert_eq!(profile("<component name=\"InspectionProjectProfileManager\" />", Path::new("x.xml")), Ok(None));
    }
}
