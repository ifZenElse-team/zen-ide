use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub columns: Vec<String>,
    pub state: Option<String>,
}

pub fn rows(section: &str, output: &str) -> Vec<Row> {
    output.lines().filter_map(object).filter_map(|fields| row(section, &fields)).collect()
}

fn row(section: &str, fields: &BTreeMap<String, String>) -> Option<Row> {
    let field = |name: &str| fields.get(name).cloned().unwrap_or_default();
    let (id, columns, state) = match section {
        "containers" => {
            let state = Some(field("State")).filter(|state| !state.is_empty()).or_else(|| state_of(&field("Status")));
            (field("ID"), vec![field("Names"), field("Image"), field("Status"), field("Ports")], state)
        }
        "images" => {
            let (repository, tag) = (field("Repository"), field("Tag"));
            let image = if repository.is_empty() || repository == "<none>" {
                String::new()
            } else if tag.is_empty() || tag == "<none>" {
                repository
            } else {
                format!("{repository}:{tag}")
            };
            (field("ID"), vec![image, field("Size"), field("CreatedSince")], None)
        }
        "volumes" => (field("Name"), vec![field("Name"), field("Driver")], None),
        "networks" => (field("ID"), vec![field("Name"), field("Driver"), field("Scope")], None),
        _ => return None,
    };
    (!id.is_empty()).then_some(Row { id, columns, state })
}

fn state_of(status: &str) -> Option<String> {
    let state = if status.starts_with("Up") && status.contains("(Paused)") {
        "paused"
    } else if status.starts_with("Up") {
        "running"
    } else if status.starts_with("Exited") {
        "exited"
    } else if status.starts_with("Created") {
        "created"
    } else if status.starts_with("Restarting") {
        "restarting"
    } else {
        return None;
    };
    Some(state.to_owned())
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip_space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.skip_space();
        let found = self.bytes.get(self.at) == Some(&byte);
        if found {
            self.at += 1;
        }
        found
    }

    fn hex(&mut self) -> Option<u32> {
        let digits = self.bytes.get(self.at..self.at + 4)?;
        self.at += 4;
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat(b'"') {
            return None;
        }
        let mut out = Vec::new();
        loop {
            let byte = *self.bytes.get(self.at)?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let escape = *self.bytes.get(self.at)?;
                    self.at += 1;
                    let ch = match escape {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let high = self.hex()?;
                            let code = if (0xD800..0xDC00).contains(&high)
                                && self.bytes.get(self.at..self.at + 2) == Some(b"\\u")
                            {
                                self.at += 2;
                                let low = self.hex()?;
                                0x10000 + ((high - 0xD800) << 10) + (low.checked_sub(0xDC00)? & 0x3FF)
                            } else {
                                high
                            };
                            char::from_u32(code).unwrap_or('\u{fffd}')
                        }
                        other => char::from(other),
                    };
                    let mut buffer = [0; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                }
                other => out.push(other),
            }
        }
    }

    fn value(&mut self) -> Option<Option<String>> {
        self.skip_space();
        match *self.bytes.get(self.at)? {
            b'"' => self.string().map(Some),
            b'{' | b'[' => {
                let mut depth = 0usize;
                loop {
                    match *self.bytes.get(self.at)? {
                        b'"' => {
                            self.string()?;
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                self.at += 1;
                                return Some(None);
                            }
                        }
                        _ => {}
                    }
                    self.at += 1;
                }
            }
            _ => {
                let start = self.at;
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|byte| !matches!(byte, b',' | b'}') && !byte.is_ascii_whitespace())
                {
                    self.at += 1;
                }
                let token = std::str::from_utf8(&self.bytes[start..self.at]).ok()?;
                Some((token != "null").then(|| token.to_owned()))
            }
        }
    }
}

fn object(line: &str) -> Option<BTreeMap<String, String>> {
    let mut reader = Reader { bytes: line.as_bytes(), at: 0 };
    if !reader.eat(b'{') {
        return None;
    }
    let mut fields = BTreeMap::new();
    if reader.eat(b'}') {
        return Some(fields);
    }
    loop {
        let key = reader.string()?;
        if !reader.eat(b':') {
            return None;
        }
        if let Some(value) = reader.value()? {
            fields.insert(key, value);
        }
        if reader.eat(b'}') {
            return Some(fields);
        }
        if !reader.eat(b',') {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers_are_read_from_the_json_lines_of_the_listing() {
        let output = concat!(
            r#"{"Command":"\"nginx -g\"","ID":"3f2a","Image":"nginx:1.27","Names":"shop-web-1","Ports":"0.0.0.0:8080->80/tcp","State":"running","Status":"Up 2 hours"}"#,
            "\n",
            r#"{"ID":"9c1b","Image":"postgres","Names":"shop-db-1","Ports":"","Status":"Exited (0) 3 days ago","Labels":{"a":"b"}}"#,
            "\nnot json\n",
            r#"{"ID":"","Names":"nameless"}"#,
            "\n"
        );
        let found = rows("containers", output);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "3f2a");
        assert_eq!(found[0].columns, ["shop-web-1", "nginx:1.27", "Up 2 hours", "0.0.0.0:8080->80/tcp"]);
        assert_eq!(found[0].state.as_deref(), Some("running"));
        assert_eq!(found[1].state.as_deref(), Some("exited"), "an older daemon has no State: it is read from Status");
    }

    #[test]
    fn images_volumes_and_networks_have_their_own_columns() {
        let image = rows(
            "images",
            r#"{"ID":"sha1","Repository":"nginx","Tag":"1.27","Size":"192MB","CreatedSince":"2 weeks ago"}"#,
        );
        assert_eq!(image[0].columns, ["nginx:1.27", "192MB", "2 weeks ago"]);
        let dangling = rows("images", r#"{"ID":"sha2","Repository":"<none>","Tag":"<none>"}"#);
        assert_eq!(dangling[0].columns[0], "", "a dangling image has no name to pull");
        let volume = rows("volumes", r#"{"Driver":"local","Name":"shop_data","Mountpoint":"/var/lib"}"#);
        assert_eq!(
            (volume[0].id.as_str(), volume[0].columns.clone()),
            ("shop_data", vec!["shop_data".to_owned(), "local".to_owned()])
        );
        let network =
            rows("networks", r#"{"ID":"ab12","Name":"bridge","Driver":"bridge","Scope":"local","IPv6":false}"#);
        assert_eq!(network[0].columns, ["bridge", "bridge", "local"]);
        assert!(rows("secrets", r#"{"ID":"x"}"#).is_empty());
    }

    #[test]
    fn escapes_and_other_values_are_read() {
        let fields = object(r#"{"a":"é😀\t","b":12,"c":null,"d":[1,"]"],"e":true}"#).unwrap();
        assert_eq!(fields["a"], "é😀\t");
        assert_eq!(fields["b"], "12");
        assert_eq!(fields["e"], "true");
        assert!(!fields.contains_key("c") && !fields.contains_key("d"));
        assert!(object(r#"{"a":"open"#).is_none());
        assert!(object(r#"{"a" "b"}"#).is_none());
    }
}
