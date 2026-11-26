use std::cell::RefCell;

pub mod cluster;
pub mod model;
pub mod targets;
pub mod yaml;

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn index_request(input: &[u8]) -> Vec<u8> {
    let Ok(input) = serde_json::from_slice::<serde_json::Value>(input) else { return Vec::new() };
    let files: Vec<model::File> = input["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            Some(model::File { path: file["path"].as_str()?.to_owned(), text: file["text"].as_str()?.to_owned() })
        })
        .collect();
    let paths: Vec<String> =
        input["paths"].as_array().into_iter().flatten().filter_map(|path| path.as_str().map(str::to_owned)).collect();
    let remotes: Vec<String> =
        input["remotes"].as_array().into_iter().flatten().filter_map(|url| url.as_str().map(str::to_owned)).collect();
    let index = model::build_with(&files, &paths, &remotes);
    if index.kinds.is_empty() {
        return Vec::new();
    }
    model::to_json(&index).to_string().into_bytes()
}

fn call(ptr: i32, len: i32, run: impl FnOnce(&[u8]) -> Vec<u8>) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        run(if valid { &input[..len as usize] } else { &[] })
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_abi() -> i32 {
    1
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
pub extern "C" fn zen_highlight(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |_| Vec::new())
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_index(ptr: i32, len: i32) -> i64 {
    call(ptr, len, index_request)
}

pub fn run_targets_request(input: &[u8]) -> Vec<u8> {
    let Some(split) = input.iter().position(|byte| *byte == 0) else { return b"[]".to_vec() };
    let path = std::str::from_utf8(&input[..split]).unwrap_or("");
    let text = String::from_utf8_lossy(&input[split + 1..]);
    targets::run_targets(path, &text).to_string().into_bytes()
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_run_targets(ptr: i32, len: i32) -> i64 {
    call(ptr, len, run_targets_request)
}

pub fn tool_rows_request(input: &[u8]) -> Vec<u8> {
    let Some(split) = input.iter().position(|byte| *byte == 0) else { return b"[]".to_vec() };
    let section = std::str::from_utf8(&input[..split]).unwrap_or("");
    let output = String::from_utf8_lossy(&input[split + 1..]);
    cluster::tool_rows(section, &output).to_string().into_bytes()
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_tool_rows(ptr: i32, len: i32) -> i64 {
    call(ptr, len, tool_rows_request)
}

#[cfg(test)]
mod tests;
