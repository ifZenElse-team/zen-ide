use super::*;
use crate::Kind::*;

fn kinds(text: &str) -> Vec<(&str, Kind)> {
    highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
}

fn assert_well_formed(text: &str) {
    let mut last = 0;
    for span in highlight(text) {
        assert!(span.start >= last && span.end > span.start && span.end <= text.len(), "{span:?} in {text:?}");
        assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end), "{span:?} in {text:?}");
        last = span.end;
    }
    for fold in fold(text) {
        assert!(fold.end_line > fold.start_line, "{fold:?} in {text:?}");
    }
    fn check(entries: &[Entry], text: &str, from: usize, to: usize) {
        for entry in entries {
            assert!(from <= entry.start && entry.start < entry.end && entry.end <= to, "{entry:?} in {text:?}");
            assert!(!entry.name.is_empty());
            check(&entry.children, text, entry.start, entry.end);
        }
    }
    check(&outline(text), text, 0, text.len());
}

const MAIN: &str = r#"# The web server
terraform {
  required_version = ">= 1.6"
}

variable "region" {
  type    = string
  default = "eu-north-1"
}

resource "aws_instance" "web" {
  count         = 2
  ami           = data.aws_ami.ubuntu.id
  instance_type = var.size == "large" ? "m5.large" : "t3.micro"
  tags = {
    Name = "web-${count.index}"
  }

  dynamic "ingress" {
    for_each = [for port in local.ports : port if port > 0]
    content {
      from_port = ingress.value
    }
  }
}

output "ip" {
  value     = aws_instance.web[0].public_ip
  sensitive = true
}

locals {
  script = <<-EOT
    echo ${var.region}
    EOT
}
"#;

#[test]
fn highlights_blocks_attributes_and_expressions() {
    assert_well_formed(MAIN);
    let found = kinds(MAIN);
    for expected in [
        ("# The web server", Comment),
        ("terraform", Keyword),
        ("required_version", Property),
        ("\">= 1.6\"", String),
        ("variable", Keyword),
        ("\"region\"", String),
        ("type", Property),
        ("resource", Keyword),
        ("\"aws_instance\"", String),
        ("\"web\"", String),
        ("2", Number),
        ("data", Global),
        ("var", Global),
        ("==", Punctuation),
        ("\"web-", String),
        ("${", Punctuation),
        ("count", Global),
        ("}", Punctuation),
        ("\"", String),
        ("dynamic", Keyword),
        ("for", Keyword),
        ("in", Keyword),
        ("local", Global),
        ("if", Keyword),
        ("content", Keyword),
        ("true", Constant),
        ("locals", Keyword),
        ("<<-EOT", Keyword),
        ("EOT", Keyword),
    ] {
        assert!(found.contains(&expected), "{expected:?} in {found:?}");
    }
    assert!(found.contains(&("count", Property)), "an attribute named count is a property");
}

#[test]
fn highlights_functions_escapes_and_block_comments() {
    let text = "x = lower(join(\",\", [\"A\\n\", \"$${literal}\"])) /* a\nb */ // end\n";
    assert_well_formed(text);
    let found = kinds(text);
    for expected in [
        ("x", Property),
        ("lower", Function),
        ("join", Function),
        ("\\n", Constant),
        ("\"$${literal}\"", String),
        ("/* a\nb */", Comment),
        ("// end", Comment),
    ] {
        assert!(found.contains(&expected), "{expected:?} in {found:?}");
    }
    assert!(!found.iter().any(|(text, _)| *text == "${"), "an escaped interpolation stays text: {found:?}");
}

#[test]
fn interpolation_holds_nested_objects_and_strings() {
    let text = "a = \"${jsonencode({ k = \"v}\" })}\"\nb = 1\n";
    assert_well_formed(text);
    let found = kinds(text);
    assert!(found.contains(&("jsonencode", Function)), "{found:?}");
    assert!(found.contains(&("\"v}\"", String)), "{found:?}");
    assert!(found.contains(&("b", Property)), "the string ended: {found:?}");
    assert!(found.contains(&("1", Number)), "{found:?}");
}

#[test]
fn a_heredoc_ends_at_its_marker_and_interpolates() {
    let text = "user_data = <<EOF\n#!/bin/sh\necho \"${var.name}\"\nEOF\nnext = 2\n";
    assert_well_formed(text);
    let found = kinds(text);
    assert!(found.contains(&("<<EOF", Keyword)), "{found:?}");
    assert!(found.contains(&("#!/bin/sh", String)), "the body is text, not a comment: {found:?}");
    assert!(found.contains(&("var", Global)), "{found:?}");
    assert!(found.contains(&("next", Property)), "the heredoc ended: {found:?}");
    assert_eq!(fold(text), [Fold { start_line: 0, end_line: 3 }]);
}

#[test]
fn folds_blocks_lists_heredocs_and_comments() {
    let folds = fold(MAIN);
    for expected in [(1, 3), (5, 8), (10, 24), (14, 16), (18, 23), (20, 22), (26, 29), (31, 35), (32, 34)] {
        assert!(folds.contains(&Fold { start_line: expected.0, end_line: expected.1 }), "{expected:?} in {folds:?}");
    }
    assert!(fold("a = [1, 2]\n").is_empty(), "a one-line list does not fold");
    assert_eq!(fold("/*\n x\n*/\n"), [Fold { start_line: 0, end_line: 2 }]);
}

#[test]
fn the_outline_lists_blocks_with_their_labels_and_nested_blocks() {
    let entries = outline(MAIN);
    let names: Vec<(&str, &str)> = entries.iter().map(|entry| (entry.name.as_str(), entry.kind)).collect();
    assert_eq!(
        names,
        [
            ("terraform", "module"),
            ("variable \"region\"", "variable"),
            ("resource \"aws_instance\" \"web\"", "class"),
            ("output \"ip\"", "field"),
            ("locals", "module"),
        ]
    );
    let resource = &entries[2];
    assert_eq!(resource.children.len(), 1, "tags = {{ }} is an attribute, not a block: {:?}", resource.children);
    assert_eq!(resource.children[0].name, "dynamic \"ingress\"");
    assert_eq!(resource.children[0].children[0].name, "content");
    assert_eq!(&MAIN[resource.start..resource.start + 8], "resource");
    assert!(MAIN[..resource.end].ends_with('}'));
}

#[test]
fn a_tfvars_file_is_attributes() {
    let text = "region = \"eu-north-1\"\nzones  = [\"a\", \"b\"]\n";
    let found = kinds(text);
    assert!(found.contains(&("region", Property)) && found.contains(&("zones", Property)), "{found:?}");
    assert!(outline(text).is_empty());
}

#[test]
fn survives_any_input() {
    let pieces = [
        "\"",
        "${",
        "%{",
        "}",
        "{",
        "[",
        "]",
        "(",
        ")",
        "<<",
        "<<-",
        "EOT",
        "\n",
        " ",
        "#",
        "//",
        "/*",
        "*/",
        "\\",
        "=",
        "==",
        "a",
        "1",
        "1.5e3",
        "é",
        "日",
        "\u{1F600}",
        "~",
    ];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                assert_well_formed(&format!("{a}{b}{c}"));
                assert_well_formed(&format!("{a}{b}\n{c}"));
            }
        }
    }
}

#[test]
fn encodes_records_and_outline_json() {
    assert_eq!(encode(&[Span { start: 1, end: 3, kind: Punctuation }]), [1, 0, 0, 0, 3, 0, 0, 0, 11, 0, 0, 0]);
    let json = outline_json(&outline("variable \"a\\\"b\" {\n}\n"));
    assert_eq!(json, r#"[{"name":"variable \"a\\\"b\"","kind":"variable","start":0,"end":19,"children":[]}]"#);
}

fn targets(path: &str, text: &str) -> serde_like::Targets {
    let mut input = path.as_bytes().to_vec();
    input.push(0);
    input.extend_from_slice(text.as_bytes());
    serde_like::parse(&std::string::String::from_utf8(run_targets_request(&input)).unwrap())
}

mod serde_like {
    pub(super) type Targets = Vec<(u32, String, String, Vec<String>)>;

    pub(super) fn parse(json: &str) -> Targets {
        let mut out = Vec::new();
        for item in json.split("},{") {
            let field = |name: &str| {
                let start = item.find(&format!("\"{name}\":"))? + name.len() + 3;
                Some(&item[start..])
            };
            let Some(line) = field("line") else { continue };
            let line: u32 = line.split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
            let string = |name: &str| field(name).map(|rest| rest[1..].split('"').next().unwrap().to_owned()).unwrap();
            let args = field("args").unwrap();
            let args =
                args[1..args.find(']').unwrap()].split(',').map(|arg| arg.trim_matches('"').to_owned()).collect();
            assert_eq!(string("program"), "terraform");
            assert_eq!(string("cwd"), "${fileDir}");
            out.push((line, string("name"), string("id"), args));
        }
        out
    }
}

#[test]
fn the_root_module_offers_init_validate_plan_and_apply_on_its_terraform_block() {
    let found = targets("infra/versions.tf", MAIN);
    let names: Vec<(&str, &[std::string::String])> =
        found.iter().map(|(_, name, _, args)| (name.as_str(), args.as_slice())).collect();
    assert_eq!(
        names,
        [
            ("terraform init", &["init".to_owned()][..]),
            ("terraform validate", &["validate".to_owned()][..]),
            ("terraform plan", &["plan".to_owned()][..]),
            ("terraform apply", &["apply".to_owned()][..]),
        ]
    );
    assert!(found.iter().all(|(line, ..)| *line == 1), "on the terraform block: {found:?}");
    assert!(!found.iter().any(|(_, _, _, args)| args.iter().any(|arg| arg.contains("auto-approve"))), "apply asks");
    assert!(!found.iter().any(|(_, name, ..)| name.contains("destroy")), "no destroy target");
}

#[test]
fn other_files_offer_targets_only_as_the_module_s_main_file() {
    assert!(targets("infra/variables.tf", "variable \"a\" {}\n").is_empty());
    assert_eq!(targets("infra/main.tf", "resource \"x\" \"y\" {}\n").len(), 4, "main.tf stands for its module");
    assert_eq!(targets("main.tf", "resource \"x\" \"y\" {}\n")[0].0, 0);
    assert!(targets("prod.tfvars", "terraform = 1\n").is_empty());
    assert!(targets("packer.hcl", MAIN).is_empty());
    assert!(targets("", "").is_empty());
}
