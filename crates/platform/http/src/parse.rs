use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "PATCH", "OPTIONS", "TRACE"];

/// A header as written in the file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    /// The header's name.
    pub name: String,
    /// The header's value, with its `{{variables}}`.
    pub value: String,
}

/// One request of a file, as written, before its variables are resolved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// Its place among the file's requests, from 0.
    pub index: usize,
    /// The name given by `# @name` or the text after `###`.
    pub name: Option<String>,
    /// The method; `GET` when the request line has none.
    pub method: String,
    /// The URL with its `{{variables}}`, continuation lines joined.
    pub url: String,
    /// The protocol version written at the end of the request line, e.g. `HTTP/1.1`.
    pub version: Option<String>,
    /// The headers in the order written.
    pub headers: Vec<Header>,
    /// The body, without the blank line before it and the blank lines after it.
    pub body: Option<String>,
    /// The request line's line in the file.
    pub line: usize,
    /// The first line of the request's block: the line after `###`, or the file's first.
    pub start_line: usize,
    /// The last line of the request that is not blank.
    pub end_line: usize,
    /// `# @no-redirect`: redirects are not followed.
    pub no_redirect: bool,
    /// `# @no-proxy`: the request connects directly, whatever Settings → HTTP Proxy says.
    pub no_proxy: bool,
}

pub fn is_http_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("http") || extension.eq_ignore_ascii_case("rest"))
}

pub fn parse(text: &str) -> (Vec<Request>, Vec<(String, String)>) {
    let lines: Vec<&str> = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
    let mut requests = Vec::new();
    let mut variables = Vec::new();
    let mut start = 0;
    let mut title = None;
    for (number, line) in lines.iter().enumerate() {
        if let Some(rest) = line.trim_start().strip_prefix("###") {
            block(&lines, start, number, title.take(), &mut requests, &mut variables);
            start = number + 1;
            title = Some(rest.trim().to_owned()).filter(|rest| !rest.is_empty());
        }
    }
    block(&lines, start, lines.len(), title, &mut requests, &mut variables);
    (requests, variables)
}

fn block(
    lines: &[&str],
    start: usize,
    end: usize,
    title: Option<String>,
    requests: &mut Vec<Request>,
    variables: &mut Vec<(String, String)>,
) {
    let mut name = title;
    let mut no_redirect = false;
    let mut no_proxy = false;
    let mut at = start;
    while at < end {
        let line = lines[at].trim();
        if line.is_empty() {
            at += 1;
            continue;
        }
        if let Some(comment) = comment(line) {
            let directive = comment.trim_start();
            if let Some(value) =
                directive.strip_prefix("@name").filter(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '=']))
            {
                let value = value.trim_start_matches([' ', '\t', '=']).trim();
                if !value.is_empty() {
                    name = Some(value.to_owned());
                }
            } else if directive.split_whitespace().next() == Some("@no-redirect") {
                no_redirect = true;
            } else if directive.split_whitespace().next() == Some("@no-proxy") {
                no_proxy = true;
            }
            at += 1;
            continue;
        }
        if let Some((key, value)) = variable(line) {
            variables.push((key, value));
            at += 1;
            continue;
        }
        break;
    }
    if at >= end {
        return;
    }
    let line = at;
    let (method, mut url, version) = request_line(lines[at].trim());
    at += 1;
    while at < end && lines[at].starts_with([' ', '\t']) && !lines[at].trim().is_empty() {
        url.push_str(lines[at].trim());
        at += 1;
    }
    let mut headers = Vec::new();
    let mut body_start = None;
    while at < end {
        let raw = lines[at];
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            body_start = Some(at + 1);
            break;
        }
        if comment(trimmed).is_some() {
            at += 1;
            continue;
        }
        match header(trimmed) {
            Some(header) => headers.push(header),
            None => {
                body_start = Some(at);
                break;
            }
        }
        at += 1;
    }
    let mut last = at.min(end).saturating_sub(1).max(line);
    let mut body = None;
    if let Some(body_start) = body_start {
        let stop = (body_start..end).find(|&number| handler(lines[number])).unwrap_or(end);
        let content: Vec<&str> = lines[body_start.min(stop)..stop].to_vec();
        let kept = content.iter().rposition(|line| !line.trim().is_empty()).map_or(0, |last| last + 1);
        if kept > 0 {
            body = Some(content[..kept].join("\n"));
            last = body_start + kept - 1;
        }
        if let Some(final_line) = (stop..end).rev().find(|&number| !lines[number].trim().is_empty()) {
            last = last.max(final_line);
        }
    }
    let index = requests.len();
    requests.push(Request {
        index,
        name,
        method,
        url,
        version,
        headers,
        body,
        line,
        start_line: start,
        end_line: last,
        no_redirect,
        no_proxy,
    });
}

fn comment(line: &str) -> Option<&str> {
    line.strip_prefix("//").or_else(|| line.strip_prefix('#'))
}

fn variable(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix('@')?;
    let (key, value) = rest.split_once('=')?;
    let key = key.trim();
    let valid = !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'));
    valid.then(|| (key.to_owned(), value.trim().to_owned()))
}

fn request_line(line: &str) -> (String, String, Option<String>) {
    let mut words: Vec<&str> = line.split_whitespace().collect();
    let method = match words.first() {
        Some(first) if METHODS.contains(first) => {
            let method = (*first).to_owned();
            words.remove(0);
            method
        }
        _ => "GET".to_owned(),
    };
    let version = match words.last() {
        Some(last) if words.len() > 1 && last.starts_with("HTTP/") => {
            let version = (*last).to_owned();
            words.pop();
            Some(version)
        }
        _ => None,
    };
    (method, words.join(" "), version)
}

fn header(line: &str) -> Option<Header> {
    let (name, value) = line.split_once(':')?;
    let name = name.trim();
    let valid =
        !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte));
    valid.then(|| Header { name: name.to_owned(), value: value.trim().to_owned() })
}

fn handler(line: &str) -> bool {
    line.starts_with("> ") || line.starts_with(">>") || line.starts_with(">{%") || line.starts_with("<> ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requests(text: &str) -> Vec<Request> {
        parse(text).0
    }

    #[test]
    fn a_bare_url_is_a_get() {
        let found = requests("https://example.test/items\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].method, "GET");
        assert_eq!(found[0].url, "https://example.test/items");
        assert_eq!((found[0].line, found[0].start_line, found[0].end_line), (0, 0, 0));
        assert!(found[0].headers.is_empty());
        assert_eq!(found[0].body, None);
    }

    #[test]
    fn method_headers_body_and_version() {
        let text = "POST http://localhost:8080/api/items HTTP/1.1\r\nContent-Type: application/json\r\nX-Token: {{token}}\r\n\r\n{\n  \"name\": \"tea\"\n}\r\n\r\n\r\n";
        let found = requests(text);
        assert_eq!(found.len(), 1);
        let request = &found[0];
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "http://localhost:8080/api/items");
        assert_eq!(request.version.as_deref(), Some("HTTP/1.1"));
        assert_eq!(
            request.headers,
            [
                Header { name: "Content-Type".into(), value: "application/json".into() },
                Header { name: "X-Token".into(), value: "{{token}}".into() },
            ]
        );
        assert_eq!(request.body.as_deref(), Some("{\n  \"name\": \"tea\"\n}"));
        assert_eq!(request.end_line, 6);
    }

    #[test]
    fn requests_are_separated_by_three_hashes_with_names() {
        let text = "### List items\nGET /items\nHost: localhost\n\n###\n# @name create\nPOST /items\n\nbody\n### \n// a comment only\n";
        let found = requests(text);
        assert_eq!(found.len(), 2, "a block without a request line is no request");
        assert_eq!(found[0].name.as_deref(), Some("List items"));
        assert_eq!((found[0].index, found[0].line, found[0].start_line, found[0].end_line), (0, 1, 1, 2));
        assert_eq!(found[1].name.as_deref(), Some("create"));
        assert_eq!((found[1].index, found[1].line, found[1].start_line, found[1].end_line), (1, 6, 5, 8));
        assert_eq!(found[1].body.as_deref(), Some("body"));
    }

    #[test]
    fn name_directive_wins_over_the_separator_text_and_takes_an_equals_sign() {
        let found = requests("### Separator title\n// @name = fromDirective\nDELETE http://h/x\n");
        assert_eq!(found[0].name.as_deref(), Some("fromDirective"));
        assert_eq!(found[0].method, "DELETE");
        let found = requests("# @names are not a name\nGET http://h/x\n");
        assert_eq!(found[0].name, None);
    }

    #[test]
    fn comments_in_headers_and_continued_urls() {
        let text = "GET http://h/search\n    ?q=tea\n    &page=2\n# skipped\nAccept: */*\n// also skipped\n";
        let found = requests(text);
        assert_eq!(found[0].url, "http://h/search?q=tea&page=2");
        assert_eq!(found[0].headers, [Header { name: "Accept".into(), value: "*/*".into() }]);
        assert_eq!(found[0].end_line, 5);
    }

    #[test]
    fn no_proxy_is_a_directive_of_one_request() {
        let (requests, _) = parse("# @no-proxy\nGET http://a/\n\n###\nGET http://b/\n");
        assert!(requests[0].no_proxy);
        assert!(!requests[1].no_proxy);
    }

    #[test]
    fn variables_and_no_redirect() {
        let text = "@host = localhost:8080\n@token=abc\n\n# @no-redirect\nGET http://{{host}}/a\n";
        let (found, variables) = parse(text);
        assert_eq!(variables, [("host".into(), "localhost:8080".into()), ("token".into(), "abc".into())]);
        assert!(found[0].no_redirect);
        assert_eq!(found[0].line, 4);
    }

    #[test]
    fn response_handlers_and_redirections_are_not_the_body() {
        let text = "POST http://h/a\n\n{\"a\": 1}\n\n> {%\n  client.global.set(\"x\", 1);\n%}\n>> out.json\n";
        let found = requests(text);
        assert_eq!(found[0].body.as_deref(), Some("{\"a\": 1}"));
        assert_eq!(found[0].end_line, 7);
    }

    #[test]
    fn a_line_without_a_colon_starts_the_body() {
        let found = requests("PUT http://h/a\nContent-Type: text/plain\nplain body\n");
        assert_eq!(found[0].headers.len(), 1);
        assert_eq!(found[0].body.as_deref(), Some("plain body"));
    }

    #[test]
    fn every_method_and_lowercase_urls() {
        for method in METHODS {
            let found = requests(&format!("{method} http://h/\n"));
            assert_eq!(found[0].method, *method);
        }
        let found = requests("get http://h/\n");
        assert_eq!((found[0].method.as_str(), found[0].url.as_str()), ("GET", "get http://h/"));
    }

    #[test]
    fn empty_and_comment_only_files_have_no_requests() {
        assert!(requests("").is_empty());
        assert!(requests("\n\n# nothing\n### \n// still nothing\n").is_empty());
    }

    #[test]
    fn http_files_by_extension() {
        assert!(is_http_file(Path::new("/p/api.http")));
        assert!(is_http_file(Path::new("/p/api.REST")));
        assert!(!is_http_file(Path::new("/p/api.json")));
        assert!(!is_http_file(Path::new("/p/http")));
    }
}
