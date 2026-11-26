use std::path::Path;
use std::sync::{Arc, RwLock};

pub type Gate = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenRoot(pub std::path::PathBuf);

static GATE: RwLock<Option<Gate>> = RwLock::new(None);

pub fn set_gate(gate: Option<Gate>) {
    *GATE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = gate;
}

pub fn trusted(root: &Path) -> bool {
    let gate = GATE.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    gate.is_some_and(|gate| gate(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_is_trusted_only_when_the_gate_says_so() {
        assert!(!trusted(Path::new("/p")), "no gate, no trust");
        set_gate(Some(Arc::new(|root: &Path| root.starts_with("/trusted"))));
        assert!(trusted(Path::new("/trusted/p")));
        assert!(!trusted(Path::new("/p")));
        set_gate(None);
        assert!(!trusted(Path::new("/trusted/p")));
    }
}
