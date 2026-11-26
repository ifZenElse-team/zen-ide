use std::cell::RefCell;

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_abi() -> i32 {
    0
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_alloc(len: i32) -> i32 {
    INPUT.with_borrow_mut(|input| {
        input.clear();
        input.resize(usize::try_from(len).unwrap_or(0), 0);
        input.as_mut_ptr() as usize as i32
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_highlight(_ptr: i32, _len: i32) -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_module_answers_the_abi_and_highlights_nothing() {
        assert_eq!(zen_abi(), 0);
        let at = zen_alloc(16);
        assert_eq!(zen_highlight(at, 16) & 0xffff_ffff, 0, "no spans");
    }

    #[test]
    fn the_manifest_declares_react_projects_and_vite_configurations_for_vite_only() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
        assert_eq!(manifest["extensions"], serde_json::json!([]));
        assert!(manifest.get("requires").is_none() && manifest.get("languageServers").is_none());
        let project = &manifest["project"];
        assert_eq!(project["markers"]["packages"], serde_json::json!(["react"]));
        let configurations = project["runConfigurations"].as_array().unwrap();
        let names: Vec<&str> = configurations.iter().filter_map(|c| c["name"].as_str()).collect();
        assert_eq!(names, ["vite", "vite build", "vite preview"]);
        assert!(configurations.iter().all(|c| c["when"]["packages"] == serde_json::json!(["vite"])));
    }
}
