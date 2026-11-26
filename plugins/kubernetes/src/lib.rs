use std::cell::RefCell;

pub mod kube;
pub mod manifests;

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
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
pub extern "C" fn zen_highlight(_ptr: i32, _len: i32) -> i64 {
    0
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_tool_rows(ptr: i32, len: i32) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        tool_rows_request(if valid { &input[..len as usize] } else { &[] })
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_run_targets(ptr: i32, len: i32) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        run_targets_request(if valid { &input[..len as usize] } else { &[] })
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

pub fn run_targets_request(input: &[u8]) -> Vec<u8> {
    let Some(split) = input.iter().position(|byte| *byte == 0) else { return b"[]".to_vec() };
    let path = std::str::from_utf8(&input[..split]).unwrap_or("");
    let text = String::from_utf8_lossy(&input[split + 1..]);
    manifests::run_targets(path, &text).to_string().into_bytes()
}

pub fn tool_rows_request(input: &[u8]) -> Vec<u8> {
    let Some(split) = input.iter().position(|byte| *byte == 0) else { return b"[]".to_vec() };
    let section = std::str::from_utf8(&input[..split]).unwrap_or("");
    let output = String::from_utf8_lossy(&input[split + 1..]);
    tool_rows_json(&kube::rows(section, &output)).into_bytes()
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn tool_rows_json(rows: &[kube::Row]) -> String {
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        json_string(&mut out, &row.id);
        out.push_str(",\"columns\":[");
        for (at, column) in row.columns.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            json_string(&mut out, column);
        }
        out.push(']');
        if let Some(state) = &row.state {
            out.push_str(",\"state\":");
            json_string(&mut out, state);
        }
        out.push('}');
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests;
