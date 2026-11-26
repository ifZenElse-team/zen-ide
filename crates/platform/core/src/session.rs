use std::any::{Any, TypeId};
use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Default)]
pub struct Session {
    settings: Settings,
    states: HashMap<TypeId, Box<dyn Any + Send>>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn state<T: Any + Send>(&self) -> Option<&T> {
        self.states.get(&TypeId::of::<T>()).and_then(|state| state.downcast_ref())
    }

    pub fn state_mut<T: Any + Send + Default>(&mut self) -> &mut T {
        self.states
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut()
            .expect("a state slot always holds the type it is keyed by")
    }

    pub fn set_state<T: Any + Send>(&mut self, state: T) {
        self.states.insert(TypeId::of::<T>(), Box::new(state));
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Settings {
    values: BTreeMap<String, serde_json::Value>,
}

impl Settings {
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.values.get(key).and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    pub fn value(&self, key: &str) -> Option<&serde_json::Value> {
        self.values.get(key)
    }

    pub fn get_or<T: DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.get(key).unwrap_or(default)
    }

    pub fn set<T: Serialize>(&mut self, key: &str, value: T) -> Result<(), serde_json::Error> {
        self.values.insert(key.to_owned(), serde_json::to_value(value)?);
        Ok(())
    }

    pub fn reset(&mut self, key: &str) {
        self.values.remove(key);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &serde_json::Value)> {
        self.values.iter().map(|(key, value)| (key.as_str(), value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_settings() {
        let mut settings = Settings::default();
        assert_eq!(settings.get::<bool>("toolbox.updates.automatic"), None);
        assert!(settings.get_or("toolbox.updates.automatic", true));
        settings.set("toolbox.updates.automatic", false).unwrap();
        assert_eq!(settings.get::<bool>("toolbox.updates.automatic"), Some(false));
        assert_eq!(settings.get::<String>("toolbox.updates.automatic"), None, "wrong type reads as unset");
        settings.reset("toolbox.updates.automatic");
        assert_eq!(settings.iter().count(), 0);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let mut settings = Settings::default();
        settings.set("a.b", 1).unwrap();
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(json, r#"{"a.b":1}"#);
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn feature_state_slots() {
        #[derive(Default, Debug, PartialEq)]
        struct Downloads(u32);
        let mut session = Session::new();
        assert_eq!(session.state::<Downloads>(), None);
        session.state_mut::<Downloads>().0 += 1;
        assert_eq!(session.state::<Downloads>(), Some(&Downloads(1)));
        session.set_state(Downloads(7));
        assert_eq!(session.state::<Downloads>(), Some(&Downloads(7)));
    }
}
