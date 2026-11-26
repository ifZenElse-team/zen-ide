use crate::complete::Completions;
use crate::structure::Symbol;
use std::fmt::Write;

pub fn outline_json(symbols: &[Symbol]) -> String {
    let mut out = String::new();
    symbols_json(&mut out, symbols);
    out
}

pub fn completions_json(completions: &Completions) -> String {
    let mut out = format!("{{\"from\":{},\"items\":[", completions.from);
    for (index, item) in completions.items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        string(&mut out, &item.label);
        out.push_str(",\"kind\":");
        string(&mut out, item.kind);
        out.push_str(",\"insert\":");
        string(&mut out, &item.insert);
        out.push('}');
    }
    out.push_str("]}");
    out
}

fn symbols_json(out: &mut String, symbols: &[Symbol]) {
    out.push('[');
    for (index, symbol) in symbols.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        string(out, &symbol.name);
        out.push_str(",\"kind\":");
        string(out, symbol.kind);
        let _ = write!(out, ",\"start\":{},\"end\":{},\"children\":", symbol.start, symbol.end);
        symbols_json(out, &symbol.children);
        out.push('}');
    }
    out.push(']');
}

fn string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complete::Item;

    #[test]
    fn escapes_and_nests() {
        let symbols = [Symbol {
            name: "a[title=\"x\\y\"]\u{1}é".to_owned(),
            kind: "module",
            start: 0,
            end: 9,
            children: vec![Symbol { name: "b".to_owned(), kind: "rule", start: 1, end: 2, children: Vec::new() }],
        }];
        assert_eq!(
            outline_json(&symbols),
            r#"[{"name":"a[title=\"x\\y\"]\u0001é","kind":"module","start":0,"end":9,"children":[{"name":"b","kind":"rule","start":1,"end":2,"children":[]}]}]"#
        );
        assert_eq!(outline_json(&[]), "[]");
        let completions = Completions {
            from: 3,
            items: vec![Item {
                label: "font-face".to_owned(),
                kind: "keyword",
                insert: "@font-face {\n\t$0\n}".to_owned(),
            }],
        };
        assert_eq!(
            completions_json(&completions),
            r#"{"from":3,"items":[{"label":"font-face","kind":"keyword","insert":"@font-face {\n\t$0\n}"}]}"#
        );
    }
}
