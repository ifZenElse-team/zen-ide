use std::collections::BTreeSet;
use std::ops::Range;
use std::path::Path;

use zen_i18n::t;
use zen_modules::generate::{Chooser, ChooserOption, Chosen, CodeGenerator, GenerateContext, Insertion, Member, Offer};
use zen_syntax::{OutlineItem, Outliner, SymbolKind};

use crate::GoOutliner;
use crate::interfaces::{Catalog, Interface, Target, imports_of, method_set, qualified};
use crate::tags::{Case, GENERATED_KEYS, OMIT_EMPTY, align, with_keys};

const CONSTRUCTOR: &str = "constructor";

const GETTER: &str = "getter";

const SETTER: &str = "setter";

const GETTER_AND_SETTER: &str = "getterAndSetter";

const IMPLEMENT: &str = "implement";

const POINTER: &str = "pointer";

#[derive(Clone, Debug, Default)]
pub struct GoGenerators {
    catalog: Catalog,
}

struct Field {
    name: String,
    type_text: String,
    start: usize,
    end: usize,
    taggable: bool,
}

struct Struct {
    name: String,
    type_parameters: String,
    start: usize,
    end: usize,
    fields: Vec<Field>,
}

struct Receiver {
    name: String,
    type_parameters: String,
    variable: Option<String>,
    end: usize,
}

fn type_parameters_of(item: &OutlineItem, slice: &impl Fn(usize, usize) -> String) -> String {
    let header = slice(item.selection_start + item.name.chars().count(), item.end);
    header
        .trim_start()
        .strip_prefix('[')
        .and_then(|rest| {
            let mut depth = 1usize;
            let close = rest.char_indices().find_map(|(at, c)| {
                match c {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(at)
            })?;
            Some(rest[..close].to_owned())
        })
        .unwrap_or_default()
}

fn receiver_variable(source: &str) -> Option<String> {
    let inside = source.strip_prefix("func")?.trim_start().strip_prefix('(')?;
    let mut words = inside.split(')').next()?.split_whitespace();
    let name = words.next()?;
    words.next().map(|_| name.to_owned())
}

fn receiver_at(text: &str, offset: usize) -> Option<Receiver> {
    let chars: Vec<char> = text.chars().collect();
    let slice =
        |start: usize, end: usize| chars[start.min(chars.len())..end.min(chars.len())].iter().collect::<String>();
    let inside = |item: &OutlineItem| item.start <= offset && offset <= item.end;
    let items = GoOutliner.outline(text);
    if let Some(item) = items.iter().find(|item| {
        matches!(item.kind, SymbolKind::Struct | SymbolKind::Type)
            && (inside(item) || item.children.iter().any(&inside))
    }) {
        return Some(Receiver {
            name: item.name.clone(),
            type_parameters: type_parameters_of(item, &slice),
            variable: item.children.iter().find_map(|child| receiver_variable(&slice(child.start, child.end))),
            end: item.end,
        });
    }
    let method = items.iter().find(|item| item.kind == SymbolKind::Method && inside(item))?;
    let type_text = method.detail.as_deref()?.trim_start_matches('*');
    let (name, parameters) = type_text.split_once('[').unwrap_or((type_text, ""));
    Some(Receiver {
        name: name.to_owned(),
        type_parameters: parameters.trim_end_matches(']').to_owned(),
        variable: receiver_variable(&slice(method.start, method.end)),
        end: method.end,
    })
}

fn struct_at(text: &str, offset: usize) -> Option<Struct> {
    let chars: Vec<char> = text.chars().collect();
    let slice =
        |start: usize, end: usize| chars[start.min(chars.len())..end.min(chars.len())].iter().collect::<String>();
    let item = GoOutliner
        .outline(text)
        .into_iter()
        .find(|item| item.kind == SymbolKind::Struct && item.start <= offset && offset <= item.end)?;
    let type_parameters = type_parameters_of(&item, &slice);
    let fields = item
        .children
        .iter()
        .filter(|child| child.kind != SymbolKind::Method)
        .map(|field| Field {
            name: field.name.clone(),
            type_text: field_type(field, &slice),
            start: field.start,
            end: field.end,
            taggable: field.detail.is_some()
                && field.name.starts_with(char::is_uppercase)
                && item.children.iter().filter(|other| other.end == field.end).count() == 1,
        })
        .collect();
    Some(Struct { name: item.name, type_parameters, start: item.start, end: item.end, fields })
}

fn field_type(field: &OutlineItem, slice: &impl Fn(usize, usize) -> String) -> String {
    if let Some(detail) = &field.detail {
        return detail.clone();
    }
    let source = slice(field.start, field.end);
    let source = source.split(['`', '"']).next().unwrap_or_default();
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn type_parameter_names(parameters: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0usize;
    let mut part = String::new();
    for c in parameters.chars().chain([',']) {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                if let Some(name) = part.split_whitespace().next() {
                    names.push(name.to_owned());
                }
                part.clear();
                continue;
            }
            _ => {}
        }
        part.push(c);
    }
    names
}

fn parameter_name(field: &str, taken: &[String]) -> String {
    let mut chars = field.chars();
    let lowered: String = chars.next().map(|first| first.to_lowercase().chain(chars).collect()).unwrap_or_default();
    let mut name = if crate::is_keyword(&lowered) || lowered == "_" { format!("{lowered}_") } else { lowered };
    let base = name.clone();
    let mut count = 1;
    while taken.contains(&name) {
        count += 1;
        name = format!("{base}{count}");
    }
    name
}

fn constructor_name(name: &str) -> String {
    let exported = name.chars().next().is_some_and(char::is_uppercase);
    let mut chars = name.chars();
    let rest: String = chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default();
    format!("{}{rest}", if exported { "New" } else { "new" })
}

fn constructor(found: &Struct, chosen: &[&Field], pointer: bool, indent: &str) -> String {
    let instance = match type_parameter_names(&found.type_parameters).join(", ") {
        names if names.is_empty() => found.name.clone(),
        names => format!("{}[{names}]", found.name),
    };
    let generics =
        if found.type_parameters.is_empty() { String::new() } else { format!("[{}]", found.type_parameters) };
    let mut names: Vec<String> = Vec::new();
    for field in chosen {
        let name = parameter_name(&field.name, &names);
        names.push(name);
    }
    let parameters = chosen
        .iter()
        .zip(&names)
        .map(|(field, name)| format!("{name} {}", field.type_text))
        .collect::<Vec<_>>()
        .join(", ");
    let (result, literal) =
        if pointer { (format!("*{instance}"), format!("&{instance}")) } else { (instance.clone(), instance) };
    let mut text = format!("\n\nfunc {}{generics}({parameters}) {result} {{\n", constructor_name(&found.name));
    if chosen.is_empty() {
        text.push_str(&format!("{indent}return {literal}{{}}\n}}"));
        return text;
    }
    text.push_str(&format!("{indent}return {literal}{{\n"));
    let widest = chosen.iter().map(|field| field.name.chars().count()).max().unwrap_or(0);
    for (field, name) in chosen.iter().zip(&names) {
        let pad = " ".repeat(widest - field.name.chars().count());
        text.push_str(&format!("{indent}{indent}{}:{pad} {name},\n", field.name));
    }
    text.push_str(&format!("{indent}}}\n}}"));
    text
}

fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn getter_name(field: &str) -> String {
    if field.chars().next().is_some_and(char::is_uppercase) { format!("Get{field}") } else { capitalized(field) }
}

fn setter_name(field: &str) -> String {
    format!("Set{}", capitalized(field))
}

fn receiver_text(receiver: &Receiver) -> (String, String) {
    let variable = receiver.variable.clone().unwrap_or_else(|| {
        let first = receiver.name.chars().find(|c| c.is_alphabetic()).unwrap_or('t');
        first.to_lowercase().collect()
    });
    let names = type_parameter_names(&receiver.type_parameters);
    let names = if names.is_empty() { receiver.type_parameters.clone() } else { names.join(", ") };
    let instance = if names.is_empty() { receiver.name.clone() } else { format!("{}[{names}]", receiver.name) };
    (variable.clone(), format!("{variable} *{instance}"))
}

fn package_methods(context: &GenerateContext<'_>, receiver: &str) -> BTreeSet<String> {
    let collect = |text: &str, names: &mut BTreeSet<String>| {
        for item in GoOutliner.outline(text) {
            if item.kind == SymbolKind::Method {
                let type_text = item.detail.as_deref().unwrap_or_default().trim_start_matches('*');
                if type_text.split('[').next() == Some(receiver) {
                    names.insert(item.name);
                }
            } else if item.name == receiver {
                names
                    .extend(item.children.into_iter().filter(|child| child.kind == SymbolKind::Method).map(|c| c.name));
            }
        }
    };
    let mut names = BTreeSet::new();
    collect(context.text, &mut names);
    let siblings =
        context.path.parent().filter(|folder| folder.is_absolute()).and_then(|folder| folder.read_dir().ok());
    for entry in siblings.into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path != context.path && name.ends_with(".go") && !name.ends_with("_test.go") {
            collect(&std::fs::read_to_string(&path).unwrap_or_default(), &mut names);
        }
    }
    names
}

fn wanted_accessors(id: &str, field: &str, existing: &BTreeSet<String>) -> (Option<String>, Option<String>) {
    let getter = getter_name(field);
    let setter = setter_name(field);
    (
        (id != SETTER && !existing.contains(&getter)).then_some(getter),
        (id != GETTER && !existing.contains(&setter)).then_some(setter),
    )
}

fn accessors(id: &str, receiver: &Receiver, fields: &[&Field], existing: &BTreeSet<String>, indent: &str) -> String {
    let (variable, declared) = receiver_text(receiver);
    let mut text = String::new();
    for field in fields {
        let (getter, setter) = wanted_accessors(id, &field.name, existing);
        let (name, kind) = (&field.name, &field.type_text);
        if let Some(getter) = getter {
            text.push_str(&format!("\n\nfunc ({declared}) {getter}() {kind} {{\n{indent}return {variable}.{name}\n}}"));
        }
        if let Some(setter) = setter {
            let parameter = parameter_name(name, std::slice::from_ref(&variable));
            text.push_str(&format!(
                "\n\nfunc ({declared}) {setter}({parameter} {kind}) {{\n{indent}{variable}.{name} = {parameter}\n}}"
            ));
        }
    }
    text
}

fn import_edit(text: &str, paths: &BTreeSet<String>) -> Option<(Range<usize>, String)> {
    let mut changed = text.to_owned();
    for path in paths {
        if let Some((range, inserted)) = crate::add_import(&changed, path) {
            changed.replace_range(range, &inserted);
        }
    }
    let before: Vec<char> = text.chars().collect();
    let after: Vec<char> = changed.chars().collect();
    let start = before.iter().zip(&after).take_while(|(a, b)| a == b).count();
    let tail = before[start..].iter().rev().zip(after[start..].iter().rev()).take_while(|(a, b)| a == b).count();
    (before != after).then(|| (start..before.len() - tail, after[start..after.len() - tail].iter().collect()))
}

fn member_id(interface: &Interface) -> String {
    format!("{}#{}", interface.import, interface.name)
}

impl GoGenerators {
    fn target<'a>(context: &'a GenerateContext<'_>) -> Target<'a> {
        Target { folder: context.path.parent().unwrap_or(Path::new("")), imports: imports_of(context.text) }
    }

    fn implement_chooser(&self, context: &GenerateContext<'_>) -> Option<Chooser> {
        receiver_at(context.text, context.offset)?;
        let target = Self::target(context);
        let all = self.catalog.interfaces();
        let mut usable: Vec<&Interface> = all
            .iter()
            .filter(|interface| {
                let here = interface.folder == target.folder;
                (here || (interface.exported() || interface.is_builtin()) && interface.package != "main")
                    && (here || !interface.import.is_empty())
                    && method_set(&all, interface).is_some()
            })
            .collect();
        usable.sort_by_key(|interface| (interface.folder != target.folder, interface.name.to_lowercase()));
        Some(Chooser {
            title: t!("go-generate-implement-title"),
            members: usable
                .into_iter()
                .map(|interface| Member {
                    id: member_id(interface),
                    name: interface.name.clone(),
                    detail: Some(interface.import.clone()),
                    group: None,
                    selected: false,
                })
                .collect(),
            options: Vec::new(),
            allow_empty: false,
            search: true,
        })
    }

    fn implement(&self, context: &GenerateContext<'_>, chosen: &Chosen) -> Result<Insertion, String> {
        let receiver = receiver_at(context.text, context.offset).ok_or_else(|| t!("go-generate-no-type"))?;
        let (_, declared) = receiver_text(&receiver);
        let target = Self::target(context);
        let all = self.catalog.interfaces();
        let mut existing = package_methods(context, &receiver.name);
        let mut needed = BTreeSet::new();
        let mut text = String::new();
        for interface in all.iter().filter(|interface| chosen.members.contains(&member_id(interface))) {
            for (owner, method) in method_set(&all, interface).unwrap_or_default() {
                if existing.insert(method.name.clone()) {
                    let signature = qualified(owner, &method.signature, &target, &mut needed);
                    text.push_str(&format!(
                        "\n\nfunc ({declared}) {}{signature} {{\n{}panic(\"implement me\")\n}}",
                        method.name, context.indent
                    ));
                }
            }
        }
        if text.is_empty() {
            return Err(t!("go-generate-nothing-missing"));
        }
        Ok(Insertion {
            at: receiver.end,
            end: None,
            text,
            also: import_edit(context.text, &needed).into_iter().collect(),
        })
    }
}

fn case_of(id: &str) -> Option<Case> {
    Case::ALL.into_iter().find(|case| case.id() == id)
}

fn tags_chooser(found: &Struct) -> Chooser {
    let members = found.fields.iter().enumerate().filter(|(_, field)| field.taggable);
    let keys = GENERATED_KEYS.iter().enumerate().map(|(index, key)| ChooserOption {
        id: (*key).to_owned(),
        label: (*key).to_owned(),
        value: index == 0,
    });
    Chooser {
        title: t!("go-generate-tags-title"),
        members: members
            .map(|(index, field)| Member {
                id: index.to_string(),
                name: field.name.clone(),
                detail: Some(field.type_text.clone()),
                group: Some(found.name.clone()),
                selected: true,
            })
            .collect(),
        options: keys
            .chain([ChooserOption { id: OMIT_EMPTY.to_owned(), label: OMIT_EMPTY.to_owned(), value: false }])
            .collect(),
        allow_empty: false,
        search: false,
    }
}

fn tagged(text: &str, found: &Struct, case: Case, chooser: &Chooser, chosen: &Chosen) -> Result<Insertion, String> {
    let keys: Vec<String> =
        GENERATED_KEYS.iter().filter(|key| chosen.option(chooser, key)).map(|key| (*key).to_owned()).collect();
    if keys.is_empty() {
        return Err(t!("go-generate-tags-no-key"));
    }
    let mut chars: Vec<char> = text.chars().collect();
    let omit_empty = chosen.option(chooser, OMIT_EMPTY);
    for (index, field) in found.fields.iter().enumerate().rev() {
        if !field.taggable || !chosen.members.iter().any(|member| *member == index.to_string()) {
            continue;
        }
        let source: String = chars[field.start..field.end].iter().collect();
        let (code, tag) = source
            .split_once('`')
            .map_or((source.as_str(), ""), |(code, rest)| (code, rest.strip_suffix('`').unwrap_or(rest)));
        if source.contains('"') && !source.contains('`') {
            continue;
        }
        let Some(new) = with_keys(tag, &keys, &field.name, case, omit_empty) else { continue };
        let new = format!("{} `{new}`", code.trim_end());
        chars.splice(field.start..field.end, new.chars());
    }
    let end = (found.end + chars.len()).saturating_sub(text.chars().count());
    let source: String = chars[found.start..end].iter().collect();
    let text = align(&source, true).unwrap_or(source);
    Ok(Insertion { at: found.start, end: Some(found.end), text, also: Vec::new() })
}

impl CodeGenerator for GoGenerators {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn offers(&self, context: &GenerateContext<'_>) -> Vec<Offer> {
        let offer = |id: &str, title: String, action: Option<&str>| Offer {
            id: id.to_owned(),
            title,
            chooser: true,
            action: action.map(str::to_owned),
        };
        let mut offers = Vec::new();
        if let Some(found) = struct_at(context.text, context.offset) {
            offers.push(offer(CONSTRUCTOR, t!("go-generate-constructor"), None));
            offers.push(offer(GETTER, t!("go-generate-getter"), None));
            offers.push(offer(SETTER, t!("go-generate-setter"), None));
            offers.push(offer(GETTER_AND_SETTER, t!("go-generate-getter-and-setter"), None));
            if found.fields.iter().any(|field| field.taggable) {
                offers.extend(Case::ALL.map(|case| offer(case.id(), case.title(), None)));
            }
        }
        if receiver_at(context.text, context.offset).is_some() {
            self.catalog.refresh(context.path);
            offers.push(offer(IMPLEMENT, t!("go-generate-implement"), Some(IMPLEMENT)));
        }
        offers
    }

    fn pending(&self, context: &GenerateContext<'_>, id: &str) -> bool {
        id == IMPLEMENT && self.catalog.pending(context.path)
    }

    fn chooser(&self, context: &GenerateContext<'_>, id: &str) -> Option<Chooser> {
        if id == IMPLEMENT {
            return self.implement_chooser(context);
        }
        if case_of(id).is_some() {
            return struct_at(context.text, context.offset).map(|found| tags_chooser(&found));
        }
        let found = struct_at(context.text, context.offset)?;
        if id != CONSTRUCTOR {
            let existing = package_methods(context, &found.name);
            let wanted = |field: &Field| {
                let (getter, setter) = wanted_accessors(id, &field.name, &existing);
                field.name != "_" && (getter.is_some() || setter.is_some())
            };
            let title = match id {
                GETTER => t!("go-generate-getter-title"),
                SETTER => t!("go-generate-setter-title"),
                _ => t!("go-generate-getter-and-setter-title"),
            };
            return Some(Chooser {
                title,
                members: found
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, field)| wanted(field))
                    .map(|(index, field)| Member {
                        id: index.to_string(),
                        name: field.name.clone(),
                        detail: Some(field.type_text.clone()),
                        group: Some(found.name.clone()),
                        selected: true,
                    })
                    .collect(),
                options: Vec::new(),
                allow_empty: false,
                search: false,
            });
        }
        Some(Chooser {
            title: t!("go-generate-constructor-title"),
            members: found
                .fields
                .iter()
                .enumerate()
                .map(|(index, field)| Member {
                    id: index.to_string(),
                    name: field.name.clone(),
                    detail: Some(field.type_text.clone()),
                    group: Some(found.name.clone()),
                    selected: true,
                })
                .collect(),
            options: vec![ChooserOption {
                id: POINTER.to_owned(),
                label: t!("go-generate-return-pointer"),
                value: true,
            }],
            allow_empty: true,
            search: false,
        })
    }

    fn generate(&self, context: &GenerateContext<'_>, id: &str, chosen: &Chosen) -> Result<Insertion, String> {
        if id == IMPLEMENT {
            return self.implement(context, chosen);
        }
        let found = struct_at(context.text, context.offset).ok_or_else(|| t!("go-generate-no-struct"))?;
        if let Some(case) = case_of(id) {
            return tagged(context.text, &found, case, &tags_chooser(&found), chosen);
        }
        let fields: Vec<&Field> = found
            .fields
            .iter()
            .enumerate()
            .filter(|(index, _)| chosen.members.iter().any(|member| *member == index.to_string()))
            .map(|(_, field)| field)
            .collect();
        if id != CONSTRUCTOR {
            let receiver = receiver_at(context.text, context.offset).ok_or_else(|| t!("go-generate-no-struct"))?;
            let existing = package_methods(context, &found.name);
            let text = accessors(id, &receiver, &fields, &existing, context.indent);
            return Ok(Insertion { at: found.end, end: None, text, also: Vec::new() });
        }
        let pointer = chosen.options.get(POINTER).copied().unwrap_or(true);
        Ok(Insertion {
            at: found.end,
            end: None,
            text: constructor(&found, &fields, pointer, context.indent),
            also: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const SOURCE: &str = "package shapes\n\n// Точка.\ntype Point struct {\n\tX, Y int\n\tType string `json:\"type\"`\n\t*sync.Mutex\n}\n\ntype pair[K comparable, V any] struct {\n\tkey K\n\tvalue V\n}\n";

    fn context(offset: usize) -> GenerateContext<'static> {
        GenerateContext { path: Path::new("shapes.go"), text: SOURCE, offset, indent: "\t" }
    }

    fn offset_of(needle: &str) -> usize {
        SOURCE[..SOURCE.find(needle).unwrap()].chars().count()
    }

    fn generated(offset: usize, members: &[&str], pointer: bool) -> String {
        let chosen = Chosen {
            members: members.iter().map(|member| (*member).to_owned()).collect(),
            options: [(POINTER.to_owned(), pointer)].into(),
        };
        let insertion = GoGenerators::default().generate(&context(offset), CONSTRUCTOR, &chosen).unwrap();
        let mut text: Vec<char> = SOURCE.chars().collect();
        text.splice(insertion.at..insertion.at, insertion.text.chars());
        text.into_iter().collect()
    }

    fn with_tags(source: &str, offset: usize, id: &str, members: &[&str], options: &[(&str, bool)]) -> String {
        let context = GenerateContext { path: Path::new("a.go"), text: source, offset, indent: "\t" };
        let chosen = Chosen {
            members: members.iter().map(|member| (*member).to_owned()).collect(),
            options: options.iter().map(|(id, on)| ((*id).to_owned(), *on)).collect(),
        };
        let insertion = GoGenerators::default().generate(&context, id, &chosen).unwrap();
        let mut text: Vec<char> = source.chars().collect();
        text.splice(insertion.at..insertion.end.unwrap(), insertion.text.chars());
        text.into_iter().collect()
    }

    const TAGGED: &str = "package a\n\n// Запись.\ntype User struct {\n\tUserID int\n\tName   string `yaml:\"n\"` // кто\n\tage    int\n\tX, Y   int\n\t*sync.Mutex\n}\n\nvar after = 1\n";

    #[test]
    fn tags_are_generated_for_the_chosen_fields_in_the_chosen_case() {
        zen_i18n::register(crate::LOCALES);
        let offset = TAGGED[..TAGGED.find("UserID").unwrap()].chars().count();
        let chooser = GoGenerators::default()
            .chooser(&GenerateContext { path: Path::new("a.go"), text: TAGGED, offset, indent: "\t" }, Case::Camel.id())
            .unwrap();
        let names: Vec<&str> = chooser.members.iter().map(|member| member.name.as_str()).collect();
        assert_eq!(names, ["UserID", "Name"], "only exported fields of their own line");
        let ids: Vec<&str> = chooser.members.iter().map(|member| member.id.as_str()).collect();
        let text = with_tags(TAGGED, offset, Case::Camel.id(), &ids, &[("json", true), ("omitempty", true)]);
        assert!(text.contains("\tUserID int    `json:\"userId,omitempty\"`\n"), "{text}");
        assert!(text.contains("\tName   string `yaml:\"n\" json:\"name,omitempty\"` // кто\n"), "{text}");
        assert!(text.contains("\tage    int\n") && text.ends_with("}\n\nvar after = 1\n"), "{text}");
    }

    #[test]
    fn generating_tags_twice_changes_nothing() {
        let offset = TAGGED[..TAGGED.find("UserID").unwrap()].chars().count();
        let once = with_tags(TAGGED, offset, Case::Snake.id(), &["0", "1"], &[("json", true)]);
        let offset = once[..once.find("UserID").unwrap()].chars().count();
        assert_eq!(with_tags(&once, offset, Case::Snake.id(), &["0", "1"], &[("json", true)]), once);
    }

    #[test]
    fn tags_need_a_key_and_a_struct_of_only_unexported_fields_is_not_offered_them() {
        zen_i18n::register(crate::LOCALES);
        let offset = TAGGED[..TAGGED.find("UserID").unwrap()].chars().count();
        let context = GenerateContext { path: Path::new("a.go"), text: TAGGED, offset, indent: "\t" };
        let none = Chosen { members: vec!["0".to_owned()], options: [("json".to_owned(), false)].into() };
        assert_eq!(
            GoGenerators::default().generate(&context, Case::Snake.id(), &none).err(),
            Some(t!("go-generate-tags-no-key"))
        );
        let source = "package a\n\ntype p struct {\n\tname string\n}\n";
        let context = GenerateContext { path: Path::new("a.go"), text: source, offset: 20, indent: "\t" };
        assert!(
            !GoGenerators::default().offers(&context).iter().any(|offer| case_of(&offer.id).is_some()),
            "no field to tag"
        );
    }

    #[test]
    fn the_constructor_is_offered_inside_a_struct_with_its_fields() {
        zen_i18n::register(crate::LOCALES);
        assert!(GoGenerators::default().offers(&context(0)).is_empty(), "not outside a struct");
        let offers = GoGenerators::default().offers(&context(offset_of("X, Y")));
        assert_eq!(
            offers.iter().map(|offer| (offer.id.as_str(), offer.action.as_deref())).collect::<Vec<_>>(),
            [
                (CONSTRUCTOR, None),
                (GETTER, None),
                (SETTER, None),
                (GETTER_AND_SETTER, None),
                ("structTags:snake", None),
                ("structTags:camel", None),
                ("structTags:kebab", None),
                ("structTags:pascal", None),
                (IMPLEMENT, Some(IMPLEMENT))
            ]
        );
        assert!(offers.iter().all(|offer| offer.chooser));
        let chooser = GoGenerators::default().chooser(&context(offset_of("X, Y")), CONSTRUCTOR).unwrap();
        let members: Vec<(&str, Option<&str>)> =
            chooser.members.iter().map(|member| (member.name.as_str(), member.detail.as_deref())).collect();
        assert_eq!(
            members,
            [("X", Some("int")), ("Y", Some("int")), ("Type", Some("string")), ("Mutex", Some("*sync.Mutex"))]
        );
        assert!(chooser.members.iter().all(|member| member.selected && member.group.as_deref() == Some("Point")));
        assert_eq!(chooser.options[0].id, POINTER);
    }

    #[test]
    fn the_constructor_initializes_the_chosen_fields_in_gofmt_layout() {
        zen_i18n::register(crate::LOCALES);
        let text = generated(offset_of("type Point"), &["0", "2", "3"], true);
        let expected = "}\n\nfunc NewPoint(x int, type_ string, mutex *sync.Mutex) *Point {\n\treturn &Point{\n\t\tX:     x,\n\t\tType:  type_,\n\t\tMutex: mutex,\n\t}\n}\n\ntype pair";
        assert!(text.contains(expected), "{text}");
        let text = generated(offset_of("value V"), &["0", "1"], false);
        let expected = "}\n\nfunc newPair[K comparable, V any](key K, value V) pair[K, V] {\n\treturn pair[K, V]{\n\t\tkey:   key,\n\t\tvalue: value,\n\t}\n}\n";
        assert!(text.ends_with(expected), "{text}");
        let text = generated(offset_of("value V"), &[], true);
        assert!(
            text.ends_with("func newPair[K comparable, V any]() *pair[K, V] {\n\treturn &pair[K, V]{}\n}\n"),
            "{text}"
        );
    }

    fn insert(text: &str, insertion: Insertion) -> String {
        let mut chars: Vec<char> = text.chars().collect();
        chars.splice(insertion.at..insertion.at, insertion.text.chars());
        for (range, inserted) in insertion.also.into_iter().rev() {
            chars.splice(range, inserted.chars());
        }
        chars.into_iter().collect()
    }

    #[test]
    fn accessors_are_written_for_the_chosen_fields_with_go_names() {
        zen_i18n::register(crate::LOCALES);
        let source = "package a\n\ntype Point struct {\n\tname string\n\tAge int\n\tp *Point\n}\n";
        let context = GenerateContext { path: Path::new("a.go"), text: source, offset: 30, indent: "\t" };
        let chosen = Chosen { members: vec!["0".to_owned(), "1".to_owned()], ..Chosen::default() };
        let generators = GoGenerators::default();
        let both = generators.generate(&context, GETTER_AND_SETTER, &chosen).unwrap();
        assert_eq!(
            both.text,
            "\n\nfunc (p *Point) Name() string {\n\treturn p.name\n}\n\nfunc (p *Point) SetName(name string) {\n\tp.name = name\n}\n\nfunc (p *Point) GetAge() int {\n\treturn p.Age\n}\n\nfunc (p *Point) SetAge(age int) {\n\tp.Age = age\n}"
        );
        let only = Chosen { members: vec!["2".to_owned()], ..Chosen::default() };
        let setter = generators.generate(&context, SETTER, &only).unwrap();
        assert!(setter.text.contains("func (p *Point) SetP(p2 *Point) {\n\tp.p = p2\n}"), "{}", setter.text);
    }

    #[test]
    fn accessors_that_exist_are_left_out_and_the_receiver_keeps_its_name() {
        zen_i18n::register(crate::LOCALES);
        let source = "package a\n\ntype Box[T any] struct {\n\tvalue T\n\tsize int\n}\n\nfunc (self *Box[T]) Value() T {\n\treturn self.value\n}\n";
        let context = GenerateContext { path: Path::new("a.go"), text: source, offset: 30, indent: "\t" };
        let generators = GoGenerators::default();
        let chooser = generators.chooser(&context, GETTER).unwrap();
        let names: Vec<&str> = chooser.members.iter().map(|member| member.name.as_str()).collect();
        assert_eq!(names, ["size"], "value has its getter");
        let all = generators.chooser(&context, SETTER).unwrap();
        assert_eq!(all.members.len(), 2);
        let chosen = Chosen { members: vec!["1".to_owned()], ..Chosen::default() };
        let getter = generators.generate(&context, GETTER, &chosen).unwrap();
        assert_eq!(getter.text, "\n\nfunc (self *Box[T]) Size() int {\n\treturn self.size\n}");
    }

    const IO: &str = "package io\n\ntype Reader interface {\n\tRead(p []byte) (n int, err error)\n}\n\ntype Writer interface {\n\tWrite(p []byte) (n int, err error)\n}\n\ntype ReadWriter interface {\n\tReader\n\tWriter\n}\n\ntype Hook interface {\n\tOn(from Reader, at time.Time)\n}\n";

    fn catalog() -> GoGenerators {
        let text = IO.replace("package io\n", "package io\n\nimport \"time\"\n");
        let interfaces = crate::interfaces::declared(&text, "io", Path::new("/go/src/io"));
        GoGenerators { catalog: Catalog::filled(interfaces, Path::new("/no/such/a.go")) }
    }

    #[test]
    fn implement_methods_searches_interfaces_and_writes_stubs_of_the_missing_methods() {
        zen_i18n::register(crate::LOCALES);
        let source = "package a\n\ntype File struct {\n\tname string\n}\n\nfunc (f *File) Write(p []byte) (int, error) {\n\treturn 0, nil\n}\n";
        let path = Path::new("/no/such/a.go");
        let context = GenerateContext { path, text: source, offset: 25, indent: "\t" };
        let generators = catalog();
        let chooser = generators.chooser(&context, IMPLEMENT).unwrap();
        assert!(chooser.search && !chooser.allow_empty);
        let names: Vec<&str> = chooser.members.iter().map(|member| member.name.as_str()).collect();
        assert_eq!(names, ["Hook", "Reader", "ReadWriter", "Writer"]);
        assert_eq!(chooser.members[1].detail.as_deref(), Some("io"));
        let chosen = Chosen { members: vec!["io#ReadWriter".to_owned()], ..Chosen::default() };
        let done = generators.generate(&context, IMPLEMENT, &chosen).unwrap();
        assert_eq!(
            insert(source, done),
            "package a\n\ntype File struct {\n\tname string\n}\n\nfunc (f *File) Read(p []byte) (n int, err error) {\n\tpanic(\"implement me\")\n}\n\nfunc (f *File) Write(p []byte) (int, error) {\n\treturn 0, nil\n}\n"
        );
        let chosen = Chosen { members: vec!["io#Writer".to_owned()], ..Chosen::default() };
        assert_eq!(generators.generate(&context, IMPLEMENT, &chosen), Err(t!("go-generate-nothing-missing")));
    }

    #[test]
    fn implement_methods_adds_the_imports_its_signatures_need() {
        zen_i18n::register(crate::LOCALES);
        let source = "package a\n\nimport \"fmt\"\n\ntype File struct{}\n\nvar _ = fmt.Sprint\n";
        let offset = source.find("File").unwrap();
        let context = GenerateContext { path: Path::new("/no/such/a.go"), text: source, offset, indent: "\t" };
        let chosen = Chosen { members: vec!["io#Hook".to_owned()], ..Chosen::default() };
        let done = catalog().generate(&context, IMPLEMENT, &chosen).unwrap();
        assert_eq!(
            insert(source, done),
            "package a\n\nimport (\n\t\"fmt\"\n\t\"io\"\n\t\"time\"\n)\n\ntype File struct{}\n\nfunc (f *File) On(from io.Reader, at time.Time) {\n\tpanic(\"implement me\")\n}\n\nvar _ = fmt.Sprint\n"
        );
    }

    #[test]
    fn implement_methods_is_offered_on_a_method_whose_type_is_in_another_file() {
        zen_i18n::register(crate::LOCALES);
        let source = "package a\n\nfunc (s Server) Start() {}\n";
        let context = GenerateContext { path: Path::new("/no/such/a.go"), text: source, offset: 20, indent: "\t" };
        let chosen = Chosen { members: vec!["io#Reader".to_owned()], ..Chosen::default() };
        let done = catalog().generate(&context, IMPLEMENT, &chosen).unwrap();
        assert_eq!(done.at, source.trim_end().chars().count());
        assert!(done.text.starts_with("\n\nfunc (s *Server) Read(p []byte)"), "{}", done.text);
    }
}
