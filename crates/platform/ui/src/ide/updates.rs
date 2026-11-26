use zen_core::{Registry, Session};
use zen_toolbox::{Config, ReleaseNote, Source, ToolState, Tools};

const CHECK_EVERY: f64 = 30.0 * 60.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct UpdateOffer {
    pub(super) version: String,
    pub(super) notes: Vec<ReleaseNote>,
    pub(super) progress: Option<f32>,
    pub(super) error: Option<String>,
}

pub(super) struct Updater {
    registry: Registry,
    session: Session,
    product_id: String,
    checked: Option<f64>,
    dismissed: Option<String>,
    error: Option<String>,
}

impl Updater {
    pub(super) fn new(product_id: &str) -> Option<Self> {
        let manifest_url =
            std::env::var("ZEN_RELEASES").unwrap_or_else(|_| zen_update::OFFICIAL_MANIFEST_URL.to_owned());
        if manifest_url == "none" {
            return None;
        }
        let public_key = std::env::var("ZEN_TRUST_KEY").unwrap_or_else(|_| zen_update::OFFICIAL_PUBLIC_KEY.to_owned());
        let mut registry = Registry::new();
        zen_toolbox::register(&mut registry).ok()?;
        let mut session = Session::new();
        session.set_state(Tools::new(Config {
            source: Some(Source { manifest_url, public_key, channel: "stable".to_owned() }),
            tools_dir: zen_update::install::Installation::default_root(),
            applications: zen_update::install::Installation::default_applications(),
            self_version: String::new(),
            self_executable: None,
            ..Config::default()
        }));
        Some(Self { registry, session, product_id: product_id.to_owned(), checked: None, dismissed: None, error: None })
    }

    pub(super) fn tick(&mut self, now: f64) {
        if self.checked.is_some_and(|checked| now - checked < CHECK_EVERY) {
            return;
        }
        self.checked = Some(now);
        let _ = self.registry.dispatch(&mut self.session, "toolbox.releases.refresh", serde_json::json!({}));
    }

    pub(super) fn offer(&mut self) -> Option<UpdateOffer> {
        if let Some(message) = self.session.state::<Tools>().and_then(|tools| tools.take_messages().pop()) {
            self.error = Some(message);
        }
        let tools = self.session.state::<Tools>()?;
        let snapshot = tools.snapshot();
        let tool = snapshot.tools.iter().find(|tool| tool.id == self.product_id)?;
        let ToolState::Installed { version: installed, update: Some(version), .. } = &tool.state else { return None };
        let progress = snapshot.jobs.get(&self.product_id).map(|job| job.fraction().unwrap_or(0.0));
        if progress.is_none() && self.dismissed.as_deref() == Some(version.as_str()) {
            return None;
        }
        let notes = tools.release_notes(&self.product_id, Some(installed), None).unwrap_or_default();
        Some(UpdateOffer { version: version.clone(), notes, progress, error: self.error.clone() })
    }

    pub(super) fn notes(&self) -> Option<Vec<ReleaseNote>> {
        self.session.state::<Tools>()?.release_notes(&self.product_id, None, None)
    }

    pub(super) fn update(&mut self) -> Result<(), String> {
        self.error = None;
        self.registry
            .dispatch(&mut self.session, "toolbox.tool.update", serde_json::json!({ "id": self.product_id }))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(super) fn dismiss(&mut self, version: &str) {
        self.dismissed = Some(version.to_owned());
    }
}
