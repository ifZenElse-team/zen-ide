use serde_json::json;
use zen_core::DispatchError;
use zen_i18n::t;
use zen_workbench::notifications::{Action, Group, Kind, NewNotification};

pub(super) fn is_debugger_missing(error: &DispatchError) -> bool {
    matches!(error, DispatchError::Failed { error, .. } if error.code.as_deref() == Some(zen_workbench::debug::DEBUGGER_MISSING))
}

pub(super) fn notification(offer: &zen_modules::DebuggerOffer) -> NewNotification {
    NewNotification::message(Group::Debugger, Kind::Warning, t!(offer.detail))
        .titled(t!(offer.title))
        .action(Action::new("install", t!(offer.install), "view.debugger.install", json!({})))
}

#[cfg(test)]
mod tests {
    use zen_core::CommandError;

    use super::*;

    #[test]
    fn only_a_missing_debugger_offers_delve() {
        let failed = |code: Option<&str>| DispatchError::Failed {
            id: zen_core::CommandId::new("debug.session.start").unwrap(),
            error: match code {
                Some(code) => CommandError::with_code(code, "no"),
                None => CommandError::new("the build failed"),
            },
        };
        assert!(is_debugger_missing(&failed(Some(zen_workbench::debug::DEBUGGER_MISSING))));
        assert!(!is_debugger_missing(&failed(None)));
        let offer = notification(&zen_modules::DebuggerOffer {
            title: "title",
            detail: "detail",
            install: "Install Delve",
            installing: "installing",
            command: "toolchain.dlv.install",
        });
        assert_eq!((offer.group, offer.kind), (Group::Debugger, Kind::Warning));
        assert_eq!(offer.actions[0].label, "Install Delve");
        assert_eq!(offer.actions[0].command, "view.debugger.install");
        assert!(offer.balloon);
    }
}
