#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleBarAction {
    Zoom,
    Minimise,
    Nothing,
}

pub fn title_bar_action(action: Option<&str>, miniaturize: Option<bool>) -> TitleBarAction {
    match action.map(str::trim) {
        Some(action) if action.eq_ignore_ascii_case("Minimize") => TitleBarAction::Minimise,
        Some(action) if action.eq_ignore_ascii_case("None") => TitleBarAction::Nothing,
        Some(action) if !action.is_empty() => TitleBarAction::Zoom,
        _ if miniaturize == Some(true) => TitleBarAction::Minimise,
        _ => TitleBarAction::Zoom,
    }
}

pub fn system_title_bar_action() -> TitleBarAction {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSString, NSUserDefaults};
        let defaults = NSUserDefaults::standardUserDefaults();
        let action =
            defaults.stringForKey(&NSString::from_str("AppleActionOnDoubleClick")).map(|text| text.to_string());
        let key = NSString::from_str("AppleMiniaturizeOnDoubleClick");
        let miniaturize = defaults.objectForKey(&key).map(|_| defaults.boolForKey(&key));
        title_bar_action(action.as_deref(), miniaturize)
    }
    #[cfg(not(target_os = "macos"))]
    TitleBarAction::Zoom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_decides_and_zoom_is_the_default() {
        assert_eq!(title_bar_action(None, None), TitleBarAction::Zoom, "nothing set: the system default");
        assert_eq!(title_bar_action(Some("Maximize"), None), TitleBarAction::Zoom);
        assert_eq!(title_bar_action(Some("Fill"), None), TitleBarAction::Zoom, "Fill (macOS 15) zooms too");
        assert_eq!(title_bar_action(Some("Minimize"), Some(false)), TitleBarAction::Minimise);
        assert_eq!(title_bar_action(Some("None"), Some(true)), TitleBarAction::Nothing, "the newer key wins");
        assert_eq!(title_bar_action(None, Some(true)), TitleBarAction::Minimise, "the older key alone");
        assert_eq!(title_bar_action(None, Some(false)), TitleBarAction::Zoom);
        assert_eq!(title_bar_action(Some(""), Some(true)), TitleBarAction::Minimise);
    }

    #[test]
    fn the_system_setting_can_be_read() {
        let _ = system_title_bar_action();
    }
}
