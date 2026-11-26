mod yaml;

use std::cell::RefCell;

pub use yaml::{Kind, Span, highlight};

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn encode(spans: &[Span]) -> Vec<u8> {
    let mut out = Vec::with_capacity(spans.len() * 12);
    for span in spans {
        for value in [span.start as u32, span.end as u32, span.kind as u32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
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
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        let text = if valid { std::str::from_utf8(&input[..len as usize]).unwrap_or("") } else { "" };
        encode(&highlight(text))
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

const RESOURCES: &[(&str, &[u8])] = &[
    ("kubernetes/all.json", include_bytes!("../schemas/kubernetes/all.json")),
    ("kubernetes/_definitions.json", include_bytes!("../schemas/kubernetes/_definitions.json")),
    ("github/workflow.json", include_bytes!("../schemas/github/workflow.json")),
];

pub fn resource(path: &str) -> Option<&'static [u8]> {
    RESOURCES.iter().find(|(name, _)| *name == path).map(|(_, bytes)| *bytes)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_resource(ptr: i32, len: i32) -> i64 {
    let found = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        let path = if valid { std::str::from_utf8(&input[..len as usize]).unwrap_or("") } else { "" };
        resource(path).unwrap_or_default().to_vec()
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = found;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    fn assert_contains(text: &str, expected: &[(&str, Kind)]) {
        let found = kinds(text);
        for item in expected {
            assert!(found.contains(item), "{item:?} in {found:?}");
        }
    }

    #[test]
    fn highlights_a_compose_file() {
        let text = "\
# Development stack
services:
  web:
    image: \"nginx:${NGINX_TAG:-1.25}\"
    ports:
      - \"${HOST_PORT:-8080}:80\"
    environment:
      DEBUG: true
      WORKERS: 4
      URL: http://db:5432/app # default
    command: echo $$HOME $USER
    healthcheck:
      test: [\"CMD\", 'curl', -f]
      retries: ~
";
        assert_eq!(
            kinds(text),
            [
                ("# Development stack", Kind::Comment),
                ("services", Kind::Keyword),
                (":", Kind::Punctuation),
                ("web", Kind::Keyword),
                (":", Kind::Punctuation),
                ("image", Kind::Keyword),
                (":", Kind::Punctuation),
                ("\"nginx:", Kind::String),
                ("${NGINX_TAG:-1.25}", Kind::Constant),
                ("\"", Kind::String),
                ("ports", Kind::Keyword),
                (":", Kind::Punctuation),
                ("-", Kind::Punctuation),
                ("\"", Kind::String),
                ("${HOST_PORT:-8080}", Kind::Constant),
                (":80\"", Kind::String),
                ("environment", Kind::Keyword),
                (":", Kind::Punctuation),
                ("DEBUG", Kind::Keyword),
                (":", Kind::Punctuation),
                ("true", Kind::Constant),
                ("WORKERS", Kind::Keyword),
                (":", Kind::Punctuation),
                ("4", Kind::Number),
                ("URL", Kind::Keyword),
                (":", Kind::Punctuation),
                ("# default", Kind::Comment),
                ("command", Kind::Keyword),
                (":", Kind::Punctuation),
                ("$USER", Kind::Constant),
                ("healthcheck", Kind::Keyword),
                (":", Kind::Punctuation),
                ("test", Kind::Keyword),
                (":", Kind::Punctuation),
                ("[", Kind::Punctuation),
                ("\"CMD\"", Kind::String),
                (",", Kind::Punctuation),
                ("'curl'", Kind::String),
                (",", Kind::Punctuation),
                ("]", Kind::Punctuation),
                ("retries", Kind::Keyword),
                (":", Kind::Punctuation),
                ("~", Kind::Constant),
            ]
        );
    }

    #[test]
    fn highlights_a_kubernetes_deployment() {
        let text = "\
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
  labels: {app: web, tier: \"frontend\"}
spec:
  replicas: 3
  template:
    spec:
      containers:
        - name: web
          image: nginx:1.25.3
          args: [\"--port\", \"8080\"]
          ports:
            - containerPort: 80
          resources:
            limits: {cpu: 500m, memory: 128Mi}
          readinessProbe:
            initialDelaySeconds: 5
            periodSeconds: 1.5
          command:
            - /bin/sh
            - -c
            - |
              echo starting
              exec nginx -g 'daemon off;'
          env:
            - name: MODE
              value: \"production\"
---
apiVersion: v1
kind: Service
";
        let found = kinds(text);
        for expected in [
            ("apiVersion", Kind::Keyword),
            ("kind", Kind::Keyword),
            ("labels", Kind::Keyword),
            ("{", Kind::Punctuation),
            ("app", Kind::Keyword),
            ("tier", Kind::Keyword),
            ("\"frontend\"", Kind::String),
            ("}", Kind::Punctuation),
            ("3", Kind::Number),
            ("containerPort", Kind::Keyword),
            ("80", Kind::Number),
            ("cpu", Kind::Keyword),
            ("\"--port\"", Kind::String),
            ("5", Kind::Number),
            ("1.5", Kind::Number),
            ("|", Kind::Punctuation),
            ("echo starting", Kind::String),
            ("exec nginx -g 'daemon off;'", Kind::String),
            ("env", Kind::Keyword),
            ("value", Kind::Keyword),
            ("\"production\"", Kind::String),
            ("---", Kind::Punctuation),
        ] {
            assert!(found.contains(&expected), "{expected:?} in {found:?}");
        }
        for plain in ["apps/v1", "Deployment", "web", "nginx:1.25.3", "500m", "128Mi", "/bin/sh", "-c", "MODE"] {
            assert!(!found.iter().any(|(word, _)| *word == plain), "{plain} stays plain in {found:?}");
        }
        assert_eq!(found.iter().filter(|(word, kind)| *word == "name" && *kind == Kind::Keyword).count(), 3);
    }

    #[test]
    fn block_scalars_end_at_dedent() {
        let text = "a:\n  run: >-\n    one ${X}\n\n    two # not a comment\n  next: 1\nb: |+ # chomp\n  - not: a key\n";
        assert_eq!(
            kinds(text),
            [
                ("a", Kind::Keyword),
                (":", Kind::Punctuation),
                ("run", Kind::Keyword),
                (":", Kind::Punctuation),
                (">-", Kind::Punctuation),
                ("one ", Kind::String),
                ("${X}", Kind::Constant),
                ("two # not a comment", Kind::String),
                ("next", Kind::Keyword),
                (":", Kind::Punctuation),
                ("1", Kind::Number),
                ("b", Kind::Keyword),
                (":", Kind::Punctuation),
                ("|+", Kind::Punctuation),
                ("# chomp", Kind::Comment),
                ("- not: a key", Kind::String),
            ]
        );
    }

    #[test]
    fn comments_need_whitespace_before_them() {
        assert_eq!(
            kinds("url: a#b #c\n'k#': \"v # x\"\n#top"),
            [
                ("url", Kind::Keyword),
                (":", Kind::Punctuation),
                ("#c", Kind::Comment),
                ("'k#'", Kind::Keyword),
                (":", Kind::Punctuation),
                ("\"v # x\"", Kind::String),
                ("#top", Kind::Comment),
            ]
        );
    }

    #[test]
    fn highlights_anchors_tags_and_merge_keys() {
        assert_contains(
            "%YAML 1.2\n--- !!map\nbase: &base\n  x: !Ref name\nchild:\n  <<: *base\n  s: !!str 1\n...\n",
            &[
                ("%YAML 1.2", Kind::Attribute),
                ("---", Kind::Punctuation),
                ("!!map", Kind::Attribute),
                ("&base", Kind::Attribute),
                ("!Ref", Kind::Attribute),
                ("<<", Kind::Attribute),
                ("*base", Kind::Attribute),
                ("!!str", Kind::Attribute),
                ("1", Kind::Number),
                ("...", Kind::Punctuation),
            ],
        );
    }

    #[test]
    fn classifies_plain_scalars() {
        for number in
            ["0", "-12", "+3", "1_000", "3.14", ".5", "1e3", "6.02E+23", "0x1F", "0o17", ".inf", "-.Inf", ".NaN"]
        {
            assert_eq!(kinds(&format!("k: {number}"))[2], (number, Kind::Number), "{number}");
        }
        for constant in ["true", "False", "YES", "no", "on", "Off", "null", "NULL", "~"] {
            assert_eq!(kinds(&format!("k: {constant}"))[2], (constant, Kind::Constant), "{constant}");
        }
        for plain in ["1.2.3", "1e", "0x", "_1", "v1", "truely", "12:30", "."] {
            assert_eq!(kinds(&format!("k: {plain}")).len(), 2, "{plain}");
        }
    }

    #[test]
    fn splits_strings_around_interpolation() {
        assert_eq!(
            kinds("a: 'x$${Y}$Z_1-${W'"),
            [
                ("a", Kind::Keyword),
                (":", Kind::Punctuation),
                ("'x$${Y}", Kind::String),
                ("$Z_1", Kind::Constant),
                ("-${W'", Kind::String)
            ]
        );
        assert_contains(
            "a: \"\\\"$A\\\\\"",
            &[("\"\\\"", Kind::String), ("$A", Kind::Constant), ("\\\\\"", Kind::String)],
        );
    }

    #[test]
    fn handles_multi_line_strings_crlf_and_unicode() {
        let text = "k: \"one\r\n  two\"\r\nключ: 'значение' # 日本\r\n- \"q\": x\r\n";
        assert_eq!(
            kinds(text),
            [
                ("k", Kind::Keyword),
                (":", Kind::Punctuation),
                ("\"one\r\n  two\"", Kind::String),
                ("ключ", Kind::Keyword),
                (":", Kind::Punctuation),
                ("'значение'", Kind::String),
                ("# 日本", Kind::Comment),
                ("-", Kind::Punctuation),
                ("\"q\"", Kind::Keyword),
                (":", Kind::Punctuation),
            ]
        );
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "'", "\"", "\\", "$", "${", "}", "{", "[", "]", ",", ":", ": ", "- ", "#", " #", "\n", "\n ", "|", ">-",
            "&", "*", "!", "---", "1", ".", "e", "x", "é", "日", "\r",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    let text = format!("{a}{b}{c}");
                    let mut previous = 0;
                    for span in highlight(&text) {
                        assert!(
                            span.start >= previous
                                && span.end > span.start
                                && span.end <= text.len()
                                && text.is_char_boundary(span.start)
                                && text.is_char_boundary(span.end),
                            "{text:?}"
                        );
                        previous = span.end;
                    }
                }
            }
        }
    }

    #[test]
    fn encodes_records() {
        assert_eq!(encode(&[Span { start: 1, end: 3, kind: Kind::Attribute }]), [1, 0, 0, 0, 3, 0, 0, 0, 9, 0, 0, 0]);
    }

    #[test]
    fn every_shipped_schema_is_a_json_object() {
        for name in ["kubernetes/all.json", "kubernetes/_definitions.json", "github/workflow.json"] {
            let bytes = resource(name).unwrap_or_else(|| panic!("{name}"));
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(text.trim_start().starts_with('{') && text.trim_end().ends_with('}'), "{name}");
        }
        assert!(resource("../Cargo.toml").is_none() && resource("").is_none());
    }
}
