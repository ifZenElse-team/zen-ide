use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use super::ast::*;
use super::lex::Comment;

const KOTLIN_KEYWORDS: &[&str] = &["as", "fun", "in", "is", "object", "typealias", "typeof", "val", "var", "when"];

const MUTABLE: &[(&str, &str)] = &[
    ("List", "MutableList"),
    ("java.util.List", "MutableList"),
    ("Map", "MutableMap"),
    ("java.util.Map", "MutableMap"),
    ("Set", "MutableSet"),
    ("java.util.Set", "MutableSet"),
    ("Collection", "MutableCollection"),
    ("java.util.Collection", "MutableCollection"),
    ("Iterator", "MutableIterator"),
    ("java.util.Iterator", "MutableIterator"),
    ("ListIterator", "MutableListIterator"),
    ("Map.Entry", "MutableMap.MutableEntry"),
    ("Entry", "MutableMap.MutableEntry"),
    ("java.util.Map.Entry", "MutableMap.MutableEntry"),
];

const DROPPED_IMPORTS: &[&str] = &[
    "java.util.List",
    "java.util.Map",
    "java.util.Set",
    "java.util.Collection",
    "java.util.Iterator",
    "java.util.ListIterator",
    "java.util.Map.Entry",
];

const MAPS: &[&str] =
    &["Map", "HashMap", "LinkedHashMap", "TreeMap", "SortedMap", "NavigableMap", "ConcurrentHashMap", "java.util.Map"];

const ENTRIES: &[&str] = &["Map.Entry", "Entry", "java.util.Map.Entry"];

pub(crate) struct Printer<'a> {
    text: &'a str,
    comments: &'a [Comment],
    out: String,
    depth: usize,
    pub(crate) problems: Vec<(usize, usize, String)>,
    scopes: Vec<HashMap<String, Type>>,
    fields: Vec<HashMap<String, Type>>,
    open: HashSet<String>,
    assigned: Vec<HashSet<String>>,
    lambda_label: Vec<Option<String>>,
    at: (usize, usize),
    signatures: HashMap<(String, usize), Vec<Type>>,
}

fn name(raw: &str) -> String {
    if KOTLIN_KEYWORDS.contains(&raw) { format!("`{raw}`") } else { raw.to_owned() }
}

fn boxed(name: &str) -> Option<&'static str> {
    Some(match name {
        "int" | "Integer" | "java.lang.Integer" => "Int",
        "long" | "Long" | "java.lang.Long" => "Long",
        "short" | "Short" => "Short",
        "byte" | "Byte" => "Byte",
        "char" | "Character" => "Char",
        "boolean" | "Boolean" => "Boolean",
        "float" | "Float" => "Float",
        "double" | "Double" => "Double",
        "void" | "Void" => "Unit",
        "Object" | "java.lang.Object" => "Any",
        "String" | "java.lang.String" => "String",
        "CharSequence" => "CharSequence",
        "Iterable" => "Iterable",
        "Number" => "Number",
        "Throwable" => "Throwable",
        "Comparable" => "Comparable",
        _ => return None,
    })
}

pub(crate) fn ty(ty: &Type) -> String {
    match ty {
        Type::Var => String::new(),
        Type::Array(inner) => match inner.as_ref() {
            Type::Named { name, args }
                if args.is_empty() && PRIMITIVES.contains(&name.as_str()) && name != "boolean" =>
            {
                format!("{}Array", boxed(name).unwrap_or("Int"))
            }
            Type::Named { name, .. } if name == "boolean" => "BooleanArray".to_owned(),
            other => format!("Array<{}>", self::ty(other)),
        },
        Type::Named { name, args } => {
            let base = boxed(name)
                .map(str::to_owned)
                .or_else(|| MUTABLE.iter().find(|(java, _)| java == name).map(|(_, kotlin)| (*kotlin).to_owned()))
                .unwrap_or_else(|| name.clone());
            if args.is_empty() {
                base
            } else {
                let args: Vec<String> = args.iter().map(type_arg).collect();
                format!("{base}<{}>", args.join(", "))
            }
        }
    }
}

fn type_arg(arg: &TypeArg) -> String {
    match arg {
        TypeArg::Type(t) => ty(t),
        TypeArg::Any => "*".to_owned(),
        TypeArg::Extends(t) => format!("out {}", ty(t)),
        TypeArg::Super(t) => format!("in {}", ty(t)),
    }
}

fn default_value(t: &Type) -> &'static str {
    match t {
        Type::Named { name, .. } => match name.as_str() {
            "int" | "short" | "byte" => "0",
            "long" => "0L",
            "float" => "0f",
            "double" => "0.0",
            "boolean" => "false",
            "char" => "'\\u0000'",
            _ => "null",
        },
        _ => "null",
    }
}

fn literal_type(expr: &Expr) -> Option<&'static str> {
    match expr {
        Expr::Literal(Lit::Int, _) => Some("int"),
        Expr::Literal(Lit::Long, _) => Some("long"),
        Expr::Literal(Lit::Double, _) => Some("double"),
        Expr::Literal(Lit::Float, _) => Some("float"),
        Expr::Literal(Lit::Char, _) => Some("char"),
        Expr::Literal(Lit::True | Lit::False, _) => Some("boolean"),
        Expr::Literal(Lit::Str | Lit::TextBlock, _) => Some("String"),
        _ => None,
    }
}

fn level(op: &str) -> u8 {
    match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" | ">>>" => 8,
        "+" | "-" => 9,
        _ => 10,
    }
}

fn infix(op: &str) -> Option<&'static str> {
    Some(match op {
        "&" => "and",
        "|" => "or",
        "^" => "xor",
        "<<" => "shl",
        ">>" => "shr",
        ">>>" => "ushr",
        _ => return None,
    })
}

fn looks_like_interface(name: &str) -> bool {
    let simple = name.rsplit('.').next().unwrap_or(name);
    const KNOWN: &[&str] = &[
        "Runnable",
        "Callable",
        "Comparator",
        "Supplier",
        "Consumer",
        "Function",
        "Predicate",
        "BiFunction",
        "BiConsumer",
        "Iterator",
        "Iterable",
        "AutoCloseable",
        "Closeable",
        "Comparable",
        "Thread.UncaughtExceptionHandler",
    ];
    KNOWN.contains(&simple)
        || ["Listener", "Handler", "Callback", "Observer", "Visitor", "Factory", "Provider"]
            .iter()
            .any(|end| simple.ends_with(end))
        || (simple.len() > 1 && simple.starts_with('I') && simple[1..].starts_with(|c: char| c.is_ascii_uppercase()))
}

impl<'a> Printer<'a> {
    pub(crate) fn new(text: &'a str, comments: &'a [Comment]) -> Printer<'a> {
        Printer {
            text,
            comments,
            out: String::new(),
            depth: 0,
            problems: Vec::new(),
            scopes: Vec::new(),
            fields: Vec::new(),
            open: HashSet::new(),
            assigned: Vec::new(),
            lambda_label: Vec::new(),
            at: (0, 0),
            signatures: HashMap::new(),
        }
    }

    pub(crate) fn finish(self) -> (String, Vec<(usize, usize, String)>) {
        let mut out = self.out;
        while out.ends_with("\n\n") {
            out.pop();
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        (out, self.problems)
    }

    fn line(&mut self, text: &str) {
        if text.is_empty() {
            self.out.push('\n');
            return;
        }
        for _ in 0..self.depth {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") && !self.out.ends_with("{\n") {
            self.out.push('\n');
        }
    }

    fn comments(&mut self, indices: &[usize]) {
        for &index in indices {
            let Some(comment) = self.comments.get(index) else { continue };
            let raw = &self.text[comment.start..comment.end];
            for (at, line) in raw.lines().enumerate() {
                let trimmed = if at == 0 { line.trim_end() } else { line.trim() };
                let shown = if at > 0 && trimmed.starts_with('*') { format!(" {trimmed}") } else { trimmed.to_owned() };
                self.line(&shown);
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<&Type> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .or_else(|| self.fields.iter().rev().find_map(|f| f.get(name)))
    }

    fn declare(&mut self, name: &str, t: Type) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), t);
        }
    }

    pub(crate) fn unit(&mut self, unit: &Unit) {
        if let Some(package) = &unit.package {
            self.line(&format!("package {package}"));
            self.out.push('\n');
        }
        let imports: Vec<&Import> = unit
            .imports
            .iter()
            .filter(|import| {
                !(import.path.starts_with("java.lang.") && import.path.matches('.').count() == 2 && !import.is_static)
            })
            .filter(|import| import.wildcard || import.is_static || !DROPPED_IMPORTS.contains(&import.path.as_str()))
            .collect();
        for import in &imports {
            let path: Vec<String> = import.path.split('.').map(name).collect();
            let path = path.join(".");
            self.line(&if import.wildcard { format!("import {path}.*") } else { format!("import {path}") });
        }
        if !imports.is_empty() {
            self.out.push('\n');
        }
        for decl in &unit.types {
            collect_open(decl, &mut self.open);
            collect_signatures(decl, &mut self.signatures);
        }
        for (index, decl) in unit.types.iter().enumerate() {
            if index > 0 {
                self.blank();
            }
            self.type_decl(decl, true);
        }
        let trailing = unit.trailing.clone();
        self.comments(&trailing);
    }

    fn visibility(&self, mods: &Mods, in_interface: bool) -> &'static str {
        if mods.has("private") {
            "private "
        } else if mods.has("protected") {
            "protected "
        } else if mods.has("public") || in_interface {
            ""
        } else {
            "internal "
        }
    }

    fn annotations(&mut self, mods: &Mods) {
        for annotation in &mods.annotations {
            let simple = annotation.split('(').next().unwrap_or(annotation);
            if matches!(simple, "Override" | "java.lang.Override" | "FunctionalInterface" | "SafeVarargs") {
                continue;
            }
            let shown = if simple == "Deprecated" && !annotation.contains('(') {
                "Deprecated(\"\")".to_owned()
            } else {
                annotation.clone()
            };
            self.line(&format!("@{shown}"));
        }
    }

    fn type_params(params: &[TypeParam]) -> String {
        if params.is_empty() {
            return String::new();
        }
        let list: Vec<String> = params
            .iter()
            .map(|param| match param.bounds.as_slice() {
                [] => param.name.clone(),
                [bound] => format!("{} : {}", param.name, ty(bound)),
                [first, ..] => format!("{} : {}", param.name, ty(first)),
            })
            .collect();
        format!("<{}>", list.join(", "))
    }

    fn where_clause(params: &[TypeParam]) -> String {
        let extra: Vec<String> = params
            .iter()
            .filter(|param| param.bounds.len() > 1)
            .flat_map(|param| param.bounds.iter().skip(1).map(move |bound| format!("{} : {}", param.name, ty(bound))))
            .collect();
        if extra.is_empty() { String::new() } else { format!(" where {}", extra.join(", ")) }
    }

    fn type_decl(&mut self, decl: &TypeDecl, top: bool) {
        self.comments(&decl.comments);
        self.annotations(&decl.mods);
        let mut fields = HashMap::new();
        for member in &decl.members {
            if let Member::Field { ty: t, vars, .. } = member {
                for var in vars {
                    fields.insert(var.name.clone(), wrap_dims(t, var.dims));
                }
            }
        }
        for component in &decl.components {
            fields.insert(component.name.clone(), component.ty.clone());
        }
        self.fields.push(fields);
        let visibility = if top && !decl.mods.has("public") && !decl.mods.has("private") {
            "internal "
        } else {
            self.visibility(&decl.mods, false)
        };
        let visibility = if top && decl.mods.has("public") { "" } else { visibility };
        let mut words = visibility.to_owned();
        match decl.kind {
            TypeKind::Interface => {
                words.push_str("interface ");
            }
            TypeKind::Annotation => {
                words.push_str("annotation class ");
            }
            TypeKind::Enum => words.push_str("enum class "),
            TypeKind::Record => words.push_str("data class "),
            TypeKind::Class => {
                if decl.mods.has("abstract") {
                    words.push_str("abstract ");
                } else if self.open.contains(&decl.name) && !decl.mods.has("final") {
                    words.push_str("open ");
                }
                if decl.mods.has("sealed") {
                    words = words.replace("abstract ", "");
                    words.push_str("sealed ");
                }
                if !top && !decl.mods.has("static") {
                    words.push_str("inner ");
                }
                words.push_str("class ");
            }
        }
        let header = format!("{words}{}{}", name(&decl.name), Self::type_params(&decl.params));
        let constructors: Vec<&Method> = decl
            .members
            .iter()
            .filter_map(|member| match member {
                Member::Method(method) if method.result.is_none() && method.name == decl.name => Some(method.as_ref()),
                _ => None,
            })
            .collect();
        let primary = self.primary(decl, &constructors);
        let mut head = header;
        if decl.kind == TypeKind::Record {
            let parts: Vec<String> = decl
                .components
                .iter()
                .map(|component| format!("val {}: {}", name(&component.name), self.param_type(component)))
                .collect();
            let _ = write!(head, "({})", parts.join(", "));
        } else if let Some(primary) = &primary
            && !primary.params.is_empty()
        {
            let _ = write!(head, "({})", primary.params.join(", "));
        }
        let mut supers = Vec::new();
        let is_interface = decl.kind == TypeKind::Interface;
        for base in &decl.extends {
            if is_interface {
                supers.push(ty(base));
            } else {
                let args = primary.as_ref().and_then(|primary| primary.super_args.clone());
                if constructors.len() > 1 {
                    supers.push(ty(base));
                } else {
                    supers.push(format!("{}({})", ty(base), args.unwrap_or_default()));
                }
            }
        }
        for base in &decl.implements {
            supers.push(ty(base));
        }
        if !supers.is_empty() {
            let _ = write!(head, " : {}", supers.join(", "));
        }
        head.push_str(&Self::where_clause(&decl.params));
        let statics: Vec<&Member> = decl.members.iter().filter(|member| is_static_member(member, decl.kind)).collect();
        let instance: Vec<&Member> =
            decl.members.iter().filter(|member| !is_static_member(member, decl.kind)).collect();
        if instance.is_empty()
            && statics.is_empty()
            && decl.constants.is_empty()
            && primary.as_ref().is_none_or(|p| p.init.is_empty())
        {
            self.line(&head);
            self.fields.pop();
            return;
        }
        self.line(&format!("{head} {{"));
        self.depth += 1;
        if decl.kind == TypeKind::Enum {
            let count = decl.constants.len();
            for (index, constant) in decl.constants.iter().enumerate() {
                self.comments(&constant.comments);
                let args =
                    if constant.args.is_empty() { String::new() } else { format!("({})", self.args(&constant.args)) };
                let end = if index + 1 == count { ";" } else { "," };
                match &constant.body {
                    Some(body) => {
                        self.line(&format!("{}{args} {{", name(&constant.name)));
                        self.depth += 1;
                        self.members(body, decl, None);
                        self.depth -= 1;
                        self.line(&format!("}}{end}"));
                    }
                    None => self.line(&format!("{}{args}{end}", name(&constant.name))),
                }
            }
            if !instance.is_empty() || !statics.is_empty() {
                self.out.push('\n');
            }
        }
        let owned: Vec<Member> = instance.into_iter().cloned().collect();
        self.members(&owned, decl, primary.as_ref());
        if !statics.is_empty() {
            self.blank();
            self.line("companion object {");
            self.depth += 1;
            let owned: Vec<Member> = statics.into_iter().cloned().collect();
            self.members(&owned, decl, None);
            self.depth -= 1;
            self.line("}");
        }
        self.depth -= 1;
        self.line("}");
        self.fields.pop();
    }

    fn param_type(&self, param: &Param) -> String {
        ty(&param.ty)
    }

    fn primary(&mut self, decl: &TypeDecl, constructors: &[&Method]) -> Option<Primary> {
        if decl.kind == TypeKind::Interface || decl.kind == TypeKind::Record || constructors.len() > 1 {
            return None;
        }
        let Some(constructor) = constructors.first() else {
            return Some(Primary::default());
        };
        let body = constructor.body.clone().unwrap_or_default();
        let field_names: HashMap<String, (Mods, Type, bool)> = decl
            .members
            .iter()
            .filter_map(|member| match member {
                Member::Field { mods, ty: t, vars, .. } if !mods.has("static") => Some(
                    vars.iter()
                        .map(|var| (var.name.clone(), (mods.clone(), wrap_dims(t, var.dims), var.init.is_some())))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect();
        let mut primary = Primary::default();
        let mut taken = HashSet::new();
        let mut moved = HashSet::new();
        let mut rest = Vec::new();
        for (index, stmt) in body.stmts.iter().enumerate() {
            if index == 0
                && let StmtKind::ThisCall { is_super: true, args } = &stmt.kind
            {
                primary.super_args = Some(self.args(args));
                continue;
            }
            if let StmtKind::Expr(Expr::Assign { op, target, value }) = &stmt.kind
                && op == "="
                && let Some(field) = assigned_field(target, &constructor.params)
                && let Some((mods, field_type, initialised)) = field_names.get(&field)
                && !initialised
                && !moved.contains(&field)
                && stmt.comments.is_empty()
                && let Expr::Name(param) = value.as_ref()
                && let Some(p) = constructor.params.iter().find(|p| &p.name == param)
                && *param == field
                && p.ty == *field_type
                && !taken.contains(param)
            {
                let keyword = if mods.has("final") { "val" } else { "var" };
                primary.params.push(format!(
                    "{}{keyword} {}: {}",
                    self.visibility(mods, false),
                    name(param),
                    ty(field_type)
                ));
                taken.insert(param.clone());
                moved.insert(field.clone());
                continue;
            }
            rest.push(stmt.clone());
        }
        for param in &constructor.params {
            if !taken.contains(&param.name) {
                let shown = if param.varargs {
                    format!("vararg {}: {}", name(&param.name), self.param_type(param))
                } else {
                    format!("{}: {}", name(&param.name), ty(&param.ty))
                };
                primary.params.push(shown);
            }
        }
        primary.moved = moved;
        primary.init = rest;
        primary.init_comments = constructor.comments.clone();
        primary.init_params = constructor.params.clone();
        Some(primary)
    }

    fn members(&mut self, members: &[Member], decl: &TypeDecl, primary: Option<&Primary>) {
        let in_interface = decl.kind == TypeKind::Interface;
        let mut previous_was_field = false;
        let mut init_done = false;
        for member in members {
            match member {
                Member::Field { comments, mods, ty: t, vars } => {
                    let pending: Vec<&Var> = vars
                        .iter()
                        .filter(|var| primary.is_none_or(|primary| !primary.moved.contains(&var.name)))
                        .collect();
                    if pending.is_empty() {
                        continue;
                    }
                    if !previous_was_field {
                        self.blank();
                    }
                    self.comments(comments);
                    for var in pending {
                        self.field(mods, t, var, decl, in_interface);
                    }
                    previous_was_field = true;
                    continue;
                }
                Member::Method(method) if method.result.is_none() && method.name == decl.name => {
                    if let Some(primary) = primary {
                        if !init_done && !primary.init.is_empty() {
                            self.blank();
                            self.comments(&primary.init_comments);
                            self.line("init {");
                            self.depth += 1;
                            self.scopes.push(HashMap::new());
                            for param in &primary.init_params {
                                self.declare(&param.name, param.ty.clone());
                            }
                            self.assigned.push(assigned_in(&primary.init));
                            for stmt in &primary.init {
                                self.stmt(stmt);
                            }
                            self.assigned.pop();
                            self.scopes.pop();
                            self.depth -= 1;
                            self.line("}");
                        } else if !primary.init_comments.is_empty() {
                            self.comments(&primary.init_comments);
                        }
                        init_done = true;
                    } else {
                        self.blank();
                        self.secondary(method, decl);
                    }
                }
                Member::Method(method) => {
                    self.blank();
                    self.method(method, decl, in_interface);
                }
                Member::Init { comments, body, .. } => {
                    self.blank();
                    self.comments(comments);
                    self.line("init {");
                    self.depth += 1;
                    self.block_body(body, &[]);
                    self.depth -= 1;
                    self.line("}");
                }
                Member::Type(inner) => {
                    self.blank();
                    self.type_decl(inner, false);
                }
                Member::Raw { comments, start, end } => {
                    self.blank();
                    self.comments(comments);
                    self.raw(*start, *end);
                }
            }
            previous_was_field = false;
        }
        if let Some(primary) = primary
            && !init_done
            && !primary.init.is_empty()
        {
            self.blank();
            self.line("init {");
            self.depth += 1;
            for stmt in &primary.init {
                self.stmt(stmt);
            }
            self.depth -= 1;
            self.line("}");
        }
    }

    fn raw(&mut self, start: usize, end: usize) {
        self.line("/* Not converted:");
        for line in self.text[start..end].lines() {
            let line = line.trim_end().replace("*/", "* /");
            self.line(&line);
        }
        self.line("*/");
    }

    fn field(&mut self, mods: &Mods, t: &Type, var: &Var, decl: &TypeDecl, in_interface: bool) {
        self.annotations(mods);
        let field_type = wrap_dims(t, var.dims);
        let is_static = mods.has("static") || in_interface;
        let is_final = mods.has("final") || in_interface;
        let constant = is_static
            && is_final
            && var.init.as_ref().is_some_and(|init| {
                literal_type(init).is_some()
                    && (field_type.is_primitive() || field_type.simple_name() == Some("String"))
            });
        let visibility = self.visibility(mods, in_interface);
        let keyword = if constant {
            "const val"
        } else if is_final {
            "val"
        } else {
            "var"
        };
        let shown = ty(&field_type);
        let line = match &var.init {
            Some(init) => {
                let value = self.init_value(init, &field_type);
                let nullable = matches!(init, Expr::Literal(Lit::Null, _));
                let typed = if nullable { format!("{shown}?") } else { shown };
                format!("{visibility}{keyword} {}: {typed} = {value}", name(&var.name))
            }
            None => {
                let default = default_value(&field_type);
                let assigned_in_init = decl.members.iter().any(|member| matches!(member, Member::Init { .. }));
                if is_final && !is_static && assigned_in_init {
                    format!("{visibility}val {}: {shown}", name(&var.name))
                } else if default == "null" {
                    format!("{visibility}{} {}: {shown}? = null", if is_final { "val" } else { "var" }, name(&var.name))
                } else {
                    format!("{visibility}{keyword} {}: {shown} = {default}", name(&var.name))
                }
            }
        };
        self.line(&line);
        let _ = decl;
    }

    fn init_value(&mut self, init: &Expr, target: &Type) -> String {
        match init {
            Expr::Lambda { .. } if matches!(target, Type::Named { .. }) && boxed(&ty(target)).is_none() => {
                format!("{} {}", ty(target), self.expr(init))
            }
            Expr::ArrayInit(items) => self.array_of(target, items),
            Expr::Literal(Lit::Int, raw) if matches!(target.simple_name(), Some("long")) => {
                format!("{}L", self.number(raw))
            }
            Expr::Literal(Lit::Int, raw) if matches!(target.simple_name(), Some("double")) => {
                format!("{}.0", self.number(raw))
            }
            Expr::Literal(Lit::Int, raw) if matches!(target.simple_name(), Some("float")) => {
                format!("{}f", self.number(raw))
            }
            _ => self.expr(init),
        }
    }

    fn array_of(&mut self, target: &Type, items: &[Expr]) -> String {
        let element = match target {
            Type::Array(inner) => inner.as_ref().clone(),
            _ => Type::named("Object"),
        };
        let values: Vec<String> = items.iter().map(|item| self.init_value(item, &element)).collect();
        let function = match &element {
            Type::Named { name, .. } if PRIMITIVES.contains(&name.as_str()) => {
                format!("{}ArrayOf", boxed(name).unwrap_or("int").to_ascii_lowercase())
            }
            _ => "arrayOf".to_owned(),
        };
        format!("{function}({})", values.join(", "))
    }

    fn method(&mut self, method: &Method, decl: &TypeDecl, in_interface: bool) {
        self.comments(&method.comments);
        self.annotations(&method.mods);
        let is_main = method.name == "main"
            && method.mods.has("static")
            && method.result.as_ref().is_some_and(|r| r.simple_name() == Some("void"))
            && method.params.len() == 1;
        if is_main {
            self.line("@JvmStatic");
        }
        let mut words = self.visibility(&method.mods, in_interface).to_owned();
        if method.mods.has_annotation("Override") || method.mods.has_annotation("java.lang.Override") {
            words.push_str("override ");
        } else if method.mods.has("abstract") && !in_interface {
            words.push_str("abstract ");
        } else if self.open.contains(&decl.name)
            && !method.mods.has("final")
            && !method.mods.has("private")
            && !method.mods.has("static")
            && decl.kind == TypeKind::Class
        {
            words.push_str("open ");
        }
        if method.mods.has("synchronized") {
            self.line("@Synchronized");
        }
        let type_params = Self::type_params(&method.params_of_type);
        let type_params = if type_params.is_empty() { String::new() } else { format!("{type_params} ") };
        self.scopes.push(HashMap::new());
        let params = self.param_list(&method.params);
        let result = method.result.as_ref().filter(|r| r.simple_name() != Some("void"));
        let returns_null = method.body.as_ref().is_some_and(block_returns_null);
        let result_text =
            result.map(|r| format!(": {}{}", ty(r), if returns_null { "?" } else { "" })).unwrap_or_default();
        let head = format!(
            "{words}fun {type_params}{}({params}){result_text}{}",
            name(&method.name),
            Self::where_clause(&method.params_of_type)
        );
        match &method.body {
            None => self.line(&head),
            Some(body) => {
                if let [Stmt { kind: StmtKind::Return(Some(value)), comments, .. }] = body.stmts.as_slice()
                    && comments.is_empty()
                    && body.end_comments.is_empty()
                    && !matches!(value, Expr::Lambda { .. } | Expr::Switch { .. })
                {
                    self.assigned.push(HashSet::new());
                    let value = self.expr(value);
                    self.assigned.pop();
                    if value.len() + head.len() < 100 && !value.contains('\n') {
                        self.line(&format!("{head} {{"));
                        self.depth += 1;
                        self.line(&format!("return {value}"));
                        self.depth -= 1;
                        self.line("}");
                        self.scopes.pop();
                        return;
                    }
                }
                self.line(&format!("{head} {{"));
                self.depth += 1;
                self.block_body(body, &method.params);
                self.depth -= 1;
                self.line("}");
            }
        }
        self.scopes.pop();
    }

    fn param_list(&mut self, params: &[Param]) -> String {
        let list: Vec<String> = params
            .iter()
            .map(|param| {
                self.declare(
                    &param.name,
                    if param.varargs { Type::Array(Box::new(param.ty.clone())) } else { param.ty.clone() },
                );
                if param.varargs {
                    format!("vararg {}: {}", name(&param.name), ty(&param.ty))
                } else {
                    format!("{}: {}", name(&param.name), ty(&param.ty))
                }
            })
            .collect();
        list.join(", ")
    }

    fn secondary(&mut self, method: &Method, decl: &TypeDecl) {
        self.comments(&method.comments);
        self.scopes.push(HashMap::new());
        let params = self.param_list(&method.params);
        let body = method.body.clone().unwrap_or_default();
        let mut stmts = body.stmts.clone();
        let mut delegate = String::new();
        if let Some(Stmt { kind: StmtKind::ThisCall { is_super, args }, .. }) = stmts.first() {
            delegate = format!(" : {}({})", if *is_super { "super" } else { "this" }, self.args(args));
            stmts.remove(0);
        } else if !decl.extends.is_empty() && decl.kind == TypeKind::Class {
            delegate = " : super()".to_owned();
        }
        let visibility = self.visibility(&method.mods, false);
        let visibility = if decl.kind == TypeKind::Enum { "" } else { visibility };
        let head = format!("{visibility}constructor({params}){delegate}");
        if stmts.is_empty() {
            self.line(&head);
        } else {
            self.line(&format!("{head} {{"));
            self.depth += 1;
            self.block_body(&Block { stmts, end_comments: body.end_comments.clone() }, &[]);
            self.depth -= 1;
            self.line("}");
        }
        self.scopes.pop();
    }

    fn block_body(&mut self, block: &Block, _params: &[Param]) {
        self.scopes.push(HashMap::new());
        self.assigned.push(assigned_in(&block.stmts));
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        self.comments(&block.end_comments);
        self.assigned.pop();
        self.scopes.pop();
    }

    fn reassigned(&self, var: &str) -> bool {
        self.assigned.iter().any(|set| set.contains(var))
    }

    fn braced(&mut self, head: &str, body: &Stmt) {
        self.line(&format!("{head} {{"));
        self.depth += 1;
        match &body.kind {
            StmtKind::Block(block) => self.block_body(block, &[]),
            _ => {
                self.scopes.push(HashMap::new());
                self.stmt(body);
                self.scopes.pop();
            }
        }
        self.depth -= 1;
    }

    pub(crate) fn stmt(&mut self, stmt: &Stmt) {
        self.comments(&stmt.comments);
        self.at = (stmt.start, stmt.end);
        match &stmt.kind {
            StmtKind::Block(block) => {
                self.line("run {");
                self.depth += 1;
                self.block_body(block, &[]);
                self.depth -= 1;
                self.line("}");
            }
            StmtKind::Empty => {}
            StmtKind::Raw => {
                self.raw(stmt.start, stmt.end);
            }
            StmtKind::Local { is_final, ty: t, vars } => {
                for var in vars {
                    self.local(*is_final, t, var);
                }
            }
            StmtKind::LocalType(decl) => self.type_decl(decl, true),
            StmtKind::Expr(expr) => {
                let text = self.expr_stmt(expr);
                self.line(&text);
            }
            StmtKind::If { cond, then, otherwise } => {
                let cond = self.expr(cond);
                self.braced(&format!("if ({cond})"), then);
                let mut next = otherwise.as_deref();
                while let Some(branch) = next {
                    if let StmtKind::If { cond, then, otherwise } = &branch.kind
                        && branch.comments.is_empty()
                    {
                        let cond = self.expr(cond);
                        self.line(&format!("}} else if ({cond}) {{"));
                        self.depth += 1;
                        self.inner(then);
                        self.depth -= 1;
                        next = otherwise.as_deref();
                    } else {
                        self.line("} else {");
                        self.depth += 1;
                        self.inner(branch);
                        self.depth -= 1;
                        next = None;
                    }
                }
                self.line("}");
            }
            StmtKind::While { cond, body } => {
                let cond = self.expr(cond);
                self.braced(&format!("while ({cond})"), body);
                self.line("}");
            }
            StmtKind::DoWhile { body, cond } => {
                self.braced("do", body);
                let cond = self.expr(cond);
                self.line(&format!("}} while ({cond})"));
            }
            StmtKind::ForEach { ty: t, name: var, iterable, body, .. } => {
                let iterable = self.expr(iterable);
                self.scopes.push(HashMap::new());
                if *t != Type::Var {
                    self.declare(var, t.clone());
                }
                self.braced(&format!("for ({} in {iterable})", name(var)), body);
                self.line("}");
                self.scopes.pop();
            }
            StmtKind::For { init, cond, update, body } => self.for_loop(stmt, init, cond.as_ref(), update, body),
            StmtKind::Return(value) => {
                let label = self.lambda_label.last().cloned().flatten();
                match (value, label) {
                    (Some(value), None) => {
                        let value = self.expr(value);
                        self.line(&format!("return {value}"));
                    }
                    (None, None) => self.line("return"),
                    (Some(value), Some(label)) => {
                        let value = self.expr(value);
                        self.line(&format!("return@{label} {value}"));
                    }
                    (None, Some(label)) => self.line(&format!("return@{label}")),
                }
            }
            StmtKind::Throw(value) => {
                let value = self.expr(value);
                self.line(&format!("throw {value}"));
            }
            StmtKind::Break(label) => {
                self.line(&label.as_ref().map_or("break".to_owned(), |label| format!("break@{label}")))
            }
            StmtKind::Continue(label) => {
                self.line(&label.as_ref().map_or("continue".to_owned(), |label| format!("continue@{label}")))
            }
            StmtKind::Yield(value) => {
                let value = self.expr(value);
                self.line(&value);
            }
            StmtKind::Labeled { label, body } => {
                let before = self.out.len();
                self.stmt(body);
                let indent = "    ".repeat(self.depth);
                let tail = self.out[before..].to_owned();
                self.out.truncate(before);
                let labelled = match tail.strip_prefix(&indent) {
                    Some(rest) => format!("{indent}{label}@ {rest}"),
                    None => tail,
                };
                self.out.push_str(&labelled);
            }
            StmtKind::Sync { lock, body } => {
                let lock = self.expr(lock);
                self.line(&format!("synchronized({lock}) {{"));
                self.depth += 1;
                self.block_body(body, &[]);
                self.depth -= 1;
                self.line("}");
            }
            StmtKind::Assert { cond, message } => {
                let cond = self.expr(cond);
                match message {
                    Some(message) => {
                        let message = self.expr(message);
                        self.line(&format!("assert({cond}) {{ {message} }}"));
                    }
                    None => self.line(&format!("assert({cond})")),
                }
            }
            StmtKind::ThisCall { is_super, args } => {
                let args = self.args(args);
                self.problems.push((
                    stmt.start,
                    stmt.end,
                    "a constructor call here has no Kotlin form; it is kept as written".to_owned(),
                ));
                self.line(&format!("{}({args})", if *is_super { "super" } else { "this" }));
            }
            StmtKind::Try { resources, body, catches, finally } => {
                self.try_stmt(resources, body, catches, finally.as_ref())
            }
            StmtKind::Switch { subject, cases } => {
                let text = self.when(stmt.start, subject, cases, false);
                for line in text.lines() {
                    self.line(line);
                }
            }
        }
    }

    fn inner(&mut self, body: &Stmt) {
        match &body.kind {
            StmtKind::Block(block) => self.block_body(block, &[]),
            _ => self.stmt(body),
        }
    }

    fn local(&mut self, is_final: bool, t: &Type, var: &Var) {
        let local_type = wrap_dims(t, var.dims);
        let mutable = !is_final && self.reassigned(&var.name);
        let keyword = if mutable { "var" } else { "val" };
        let line = match &var.init {
            Some(init) => {
                let shown = ty(&local_type);
                let value = self.init_value(init, &local_type);
                let inferred = *t == Type::Var
                    || literal_type(init).is_some_and(|literal| {
                        let declared = match &local_type {
                            Type::Named { name, .. } => name.as_str(),
                            _ => "",
                        };
                        literal == declared || (literal == "int" && declared == "int")
                    })
                    || matches!(init, Expr::New { ty: made, body: None, .. } if made.simple_name() == local_type.simple_name() && local_type.simple_name().is_some())
                    || matches!(init, Expr::ArrayInit(_) | Expr::NewArray { .. } | Expr::Lambda { .. });
                if matches!(init, Expr::Literal(Lit::Null, _)) {
                    format!("{keyword} {}: {shown}? = null", name(&var.name))
                } else if inferred {
                    format!("{keyword} {} = {value}", name(&var.name))
                } else {
                    format!("{keyword} {}: {shown} = {value}", name(&var.name))
                }
            }
            None => format!("{keyword} {}: {}", name(&var.name), ty(&local_type)),
        };
        if *t != Type::Var {
            self.declare(&var.name, local_type);
        }
        self.line(&line);
    }

    fn for_loop(&mut self, stmt: &Stmt, init: &[Stmt], cond: Option<&Expr>, update: &[Expr], body: &Stmt) {
        if let Some(range) = self.range_loop(init, cond, update, body) {
            self.scopes.push(HashMap::new());
            self.braced(&range, body);
            self.line("}");
            self.scopes.pop();
            return;
        }
        if contains_continue(body) && !update.is_empty() {
            self.problems.push((
                stmt.start,
                stmt.end,
                "a `continue` in this loop would skip its update in the converted `while`: check it".to_owned(),
            ));
        }
        self.scopes.push(HashMap::new());
        for part in init {
            self.stmt(part);
        }
        let cond = cond.map_or("true".to_owned(), |cond| self.expr(cond));
        self.line(&format!("while ({cond}) {{"));
        self.depth += 1;
        self.inner(body);
        for part in update {
            let text = self.expr_stmt(part);
            self.line(&text);
        }
        self.depth -= 1;
        self.line("}");
        self.scopes.pop();
    }

    fn range_loop(&mut self, init: &[Stmt], cond: Option<&Expr>, update: &[Expr], body: &Stmt) -> Option<String> {
        let [Stmt { kind: StmtKind::Local { vars, ty: t, .. }, .. }] = init else { return None };
        let [Var { name: var, init: Some(start), dims: 0 }] = vars.as_slice() else { return None };
        if !t.is_primitive() && *t != Type::Var {
            return None;
        }
        let Some(Expr::Binary { op, left, right }) = cond else { return None };
        if !matches!(left.as_ref(), Expr::Name(n) if n == var) {
            return None;
        }
        let [step] = update else { return None };
        let (up, amount) = match step {
            Expr::Unary { op, expr, .. } if matches!(expr.as_ref(), Expr::Name(n) if n == var) => (op == "++", None),
            Expr::Assign { op, target, value }
                if matches!(target.as_ref(), Expr::Name(n) if n == var) && (op == "+=" || op == "-=") =>
            {
                (op == "+=", Some(value.as_ref().clone()))
            }
            _ => return None,
        };
        if assigns(body, var) {
            return None;
        }
        let start = self.expr(start);
        let end = self.operand(right, 9);
        let range = match (op.as_str(), up) {
            ("<", true) => format!("{start} until {end}"),
            ("<=", true) => format!("{start}..{end}"),
            (">", false) => format!("{start} downTo {end} + 1"),
            (">=", false) => format!("{start} downTo {end}"),
            _ => return None,
        };
        let range = match amount {
            Some(amount) => format!("{range} step {}", self.operand(&amount, 11)),
            None => range,
        };
        self.declare(var, t.clone());
        Some(format!("for ({} in {range})", name(var)))
    }

    fn try_stmt(&mut self, resources: &[Resource], body: &Block, catches: &[Catch], finally: Option<&Block>) {
        if !resources.is_empty() {
            let simple = catches.is_empty() && finally.is_none();
            if !simple {
                self.line("try {");
                self.depth += 1;
            }
            let mut opened = 0;
            for resource in resources {
                let value = self.expr(&resource.value);
                let param = resource.name.as_ref().map(|n| format!(" {} ->", name(n))).unwrap_or_default();
                self.line(&format!("{value}.use {{{param}"));
                self.depth += 1;
                opened += 1;
            }
            self.block_body(body, &[]);
            for _ in 0..opened {
                self.depth -= 1;
                self.line("}");
            }
            if simple {
                return;
            }
            self.depth -= 1;
        } else {
            self.line("try {");
            self.depth += 1;
            self.block_body(body, &[]);
            self.depth -= 1;
        }
        for catch in catches {
            for caught in &catch.types {
                self.line(&format!("}} catch ({}: {}) {{", name(&catch.name), ty(caught)));
                self.depth += 1;
                self.scopes.push(HashMap::new());
                self.declare(&catch.name, caught.clone());
                self.block_body(&catch.body, &[]);
                self.scopes.pop();
                self.depth -= 1;
            }
        }
        if let Some(finally) = finally {
            self.line("} finally {");
            self.depth += 1;
            self.block_body(finally, &[]);
            self.depth -= 1;
        }
        self.line("}");
    }

    fn when(&mut self, start: usize, subject: &Expr, cases: &[Case], value: bool) -> String {
        let enum_type = self.type_of(subject).and_then(|t| match t {
            Type::Named { name, args }
                if args.is_empty() && !PRIMITIVES.contains(&name.as_str()) && boxed(&name).is_none() =>
            {
                Some(name)
            }
            _ => None,
        });
        let subject = self.expr(subject);
        let mut out = format!("when ({subject}) {{\n");
        let mut pending: Vec<String> = Vec::new();
        for (index, case) in cases.iter().enumerate() {
            let mut labels: Vec<String> = case
                .labels
                .iter()
                .map(|label| match (label, &enum_type) {
                    (Expr::Name(constant), Some(owner)) if self.lookup(constant).is_none() => {
                        format!("{owner}.{}", name(constant))
                    }
                    _ => self.expr(label),
                })
                .collect();
            if case.is_default {
                labels = vec!["else".to_owned()];
            }
            if !case.arrow && case.body.is_empty() && index + 1 < cases.len() {
                pending.extend(labels);
                continue;
            }
            let mut all = std::mem::take(&mut pending);
            all.extend(labels);
            if all.iter().any(|label| label == "else") {
                all = vec!["else".to_owned()];
            }
            let mut body: Vec<Stmt> = case.body.clone();
            if !case.arrow {
                let ends = body.last().is_some_and(|last| {
                    matches!(
                        last.kind,
                        StmtKind::Break(None)
                            | StmtKind::Return(_)
                            | StmtKind::Throw(_)
                            | StmtKind::Continue(_)
                            | StmtKind::Yield(_)
                    )
                });
                if !ends && index + 1 < cases.len() {
                    self.problems.push((
                        case.start,
                        case.start + 4,
                        "this case falls through to the next one, which `when` cannot do: check the converted branch"
                            .to_owned(),
                    ));
                }
                if matches!(body.last().map(|last| &last.kind), Some(StmtKind::Break(None))) {
                    body.pop();
                }
            }
            let saved = std::mem::take(&mut self.out);
            let depth = self.depth;
            self.depth = 1;
            if let [Stmt { kind: StmtKind::Yield(single), comments, .. }] = body.as_slice()
                && comments.is_empty()
            {
                let single = self.expr(single);
                self.out = format!("    {} -> {single}\n", all.join(", "));
            } else if let [Stmt { kind: StmtKind::Block(block), .. }] = body.as_slice() {
                self.line(&format!("{} -> {{", all.join(", ")));
                self.depth += 1;
                self.block_body(block, &[]);
                self.depth -= 1;
                self.line("}");
            } else if body.len() == 1 && !matches!(body[0].kind, StmtKind::Local { .. }) && body[0].comments.is_empty()
            {
                let saved_inner = std::mem::take(&mut self.out);
                self.depth = 0;
                self.stmt(&body[0]);
                let inner = std::mem::replace(&mut self.out, saved_inner);
                self.depth = 1;
                if inner.lines().count() == 1 {
                    self.line(&format!("{} -> {}", all.join(", "), inner.trim()));
                } else {
                    self.line(&format!("{} -> {{", all.join(", ")));
                    for line in inner.lines() {
                        self.out.push_str("        ");
                        self.out.push_str(line);
                        self.out.push('\n');
                    }
                    self.line("}");
                }
            } else {
                self.line(&format!("{} -> {{", all.join(", ")));
                self.depth += 1;
                self.scopes.push(HashMap::new());
                for stmt in &body {
                    self.stmt(stmt);
                }
                self.scopes.pop();
                self.depth -= 1;
                self.line("}");
            }
            let branch = std::mem::replace(&mut self.out, saved);
            self.depth = depth;
            out.push_str(&branch);
        }
        if value && !cases.iter().any(|case| case.is_default) {
            let _ = start;
        }
        out.push('}');
        out
    }

    fn expr_stmt(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Unary { op, expr: inner, .. } if op == "++" || op == "--" => {
                let target = self.expr(inner);
                format!("{target}{op}")
            }
            _ => self.expr(expr),
        }
    }

    pub(crate) fn args(&mut self, args: &[Expr]) -> String {
        let list: Vec<String> = args.iter().map(|arg| self.expr(arg)).collect();
        list.join(", ")
    }

    fn primitive_of(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Literal(..) => literal_type(expr).map(str::to_owned),
            Expr::Name(n) => match self.lookup(n) {
                Some(Type::Named { name, .. }) if PRIMITIVES.contains(&name.as_str()) => Some(name.clone()),
                _ => None,
            },
            Expr::Paren(inner) | Expr::Unary { expr: inner, .. } => self.primitive_of(inner),
            Expr::Binary { op, left, right } if matches!(op.as_str(), "+" | "-" | "*" | "/" | "%") => {
                let (left, right) = (self.primitive_of(left)?, self.primitive_of(right)?);
                let wider = if rank(&left)? >= rank(&right)? { left } else { right };
                Some(if matches!(wider.as_str(), "byte" | "short" | "char") { "int".to_owned() } else { wider })
            }
            _ => None,
        }
    }

    fn widened(&mut self, arg: &Expr, target: &Type) -> String {
        let target_name = match target {
            Type::Named { name, args } if args.is_empty() => name.clone(),
            _ => return self.expr(arg),
        };
        let (Some(from), Some(to)) = (self.primitive_of(arg).as_deref().and_then(rank), rank(&target_name)) else {
            return self.expr(arg);
        };
        if to <= from || from == 2 {
            return self.expr(arg);
        }
        if let Expr::Literal(Lit::Int, raw) = arg {
            let number = self.number(raw);
            return match target_name.as_str() {
                "long" => format!("{number}L"),
                "float" => format!("{number}f"),
                "double" => format!("{number}.0"),
                _ => number,
            };
        }
        let function = boxed(&target_name).unwrap_or("Int");
        format!("{}.to{function}()", self.postfix_operand(arg))
    }

    fn known_args(&mut self, callee: &str, args: &[Expr]) -> Option<String> {
        let types = self.signatures.get(&(callee.to_owned(), args.len())).cloned()?;
        let list: Vec<String> = args.iter().zip(&types).map(|(arg, target)| self.widened(arg, target)).collect();
        Some(list.join(", "))
    }

    fn number(&self, raw: &str) -> String {
        let (digits, suffix) = match raw.strip_suffix(['l', 'L']) {
            Some(digits) => (digits.to_owned(), "L"),
            None => (raw.to_owned(), ""),
        };
        if digits.len() > 1 && digits.starts_with('0') && digits.chars().all(|c| c.is_ascii_digit() || c == '_') {
            let clean: String = digits.chars().filter(|c| *c != '_').collect();
            if let Ok(value) = u64::from_str_radix(&clean[1..], 8) {
                return format!("{value}{suffix}");
            }
        }
        format!("{digits}{suffix}")
    }

    fn float(raw: &str) -> String {
        let float = raw.ends_with(['f', 'F']);
        let mut body = raw.trim_end_matches(['f', 'F', 'd', 'D']).to_owned();
        if body.starts_with('.') {
            body.insert(0, '0');
        }
        if body.ends_with('.') {
            body.push('0');
        }
        if !body.contains(['.', 'e', 'E']) && !float {
            body.push_str(".0");
        }
        if float { format!("{body}f") } else { body }
    }

    fn string(raw: &str) -> String {
        let inner = &raw[1..raw.len().saturating_sub(1).max(1)];
        let mut out = String::from("\"");
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '$' => out.push_str("\\$"),
                '\\' => match chars.next() {
                    Some(d @ '0'..='7') => {
                        let mut value = d.to_digit(8).unwrap_or(0);
                        while let Some(next) = chars.peek().and_then(|n| n.to_digit(8)) {
                            if value * 8 + next > 0o377 {
                                break;
                            }
                            value = value * 8 + next;
                            chars.next();
                        }
                        let _ = write!(out, "\\u{value:04x}");
                    }
                    Some('s') => out.push(' '),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push_str("\\\\"),
                },
                other => out.push(other),
            }
        }
        out.push('"');
        out
    }

    fn text_block(raw: &str) -> String {
        let body = raw.strip_prefix("\"\"\"").unwrap_or(raw);
        let body = body.strip_suffix("\"\"\"").unwrap_or(body);
        let body = body.split_once('\n').map_or("", |(_, rest)| rest);
        let lines: Vec<&str> = body.split('\n').collect();
        let indent = lines
            .iter()
            .enumerate()
            .filter(|(index, line)| !line.trim().is_empty() || *index + 1 == lines.len())
            .map(|(_, line)| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        let mut joined = String::new();
        for (index, line) in lines.iter().enumerate() {
            let stripped = if line.len() >= indent { &line[indent..] } else { line.trim_start() };
            let stripped = stripped.trim_end();
            if index + 1 == lines.len() {
                joined.push_str(stripped);
            } else if let Some(continued) = stripped.strip_suffix('\\') {
                joined.push_str(continued);
            } else {
                joined.push_str(stripped);
                joined.push_str("\\n");
            }
        }
        let escaped = joined.replace('"', "\\\"").replace("\\\\\"", "\\\"");
        Self::string(&format!("\"{escaped}\""))
    }

    fn operand(&mut self, expr: &Expr, parent: u8) -> String {
        let text = self.expr(expr);
        let needs = match expr {
            Expr::Binary { op, .. } => infix(op).is_some() || level(op) < parent,
            Expr::Cond { .. } | Expr::Assign { .. } | Expr::Lambda { .. } | Expr::InstanceOf { .. } => true,
            Expr::Cast { .. } => parent > 0,
            _ => false,
        };
        if needs { format!("({text})") } else { text }
    }

    fn is_reference(&self, expr: &Expr) -> bool {
        match expr {
            Expr::This => true,
            Expr::Name(name) => self.lookup(name).is_some_and(|t| {
                !t.is_primitive()
                    && boxed(&ty(t)).is_none()
                    && !matches!(
                        t.simple_name(),
                        Some(
                            "String"
                                | "Integer"
                                | "Long"
                                | "Double"
                                | "Float"
                                | "Boolean"
                                | "Character"
                                | "Short"
                                | "Byte"
                        )
                    )
            }),
            _ => false,
        }
    }

    pub(crate) fn expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(lit, raw) => match lit {
                Lit::Int | Lit::Long => self.number(raw),
                Lit::Float | Lit::Double => Self::float(raw),
                Lit::Char => raw.clone(),
                Lit::Str => Self::string(raw),
                Lit::TextBlock => Self::text_block(raw),
                Lit::True | Lit::False | Lit::Null => raw.clone(),
            },
            Expr::Name(n) => name(n),
            Expr::This => "this".to_owned(),
            Expr::Super => "super".to_owned(),
            Expr::TypeRef(t) => ty(t),
            Expr::ClassLit(t) => format!("{}::class.java", ty(t)),
            Expr::Paren(inner) => format!("({})", self.expr(inner)),
            Expr::Field(target, field) => {
                if field == "length"
                    && matches!(target.as_ref(), Expr::Name(n) if self.lookup(n).is_some_and(Type::is_array))
                {
                    return format!("{}.size", self.expr(target));
                }
                if let Expr::Field(system, out) = target.as_ref()
                    && matches!(system.as_ref(), Expr::Name(n) if n == "System")
                {
                    return format!("System.{out}.{}", name(field));
                }
                let base = self.postfix_operand(target);
                format!("{base}.{}", name(field))
            }
            Expr::Index(target, index) => {
                let base = self.postfix_operand(target);
                format!("{base}[{}]", self.expr(index))
            }
            Expr::Call { target, name: method, args } => self.call(target.as_deref(), method, args),
            Expr::New { ty: made, args, body } => match body {
                None => {
                    let callee = made.simple_name().unwrap_or_default().to_owned();
                    let shown = self.known_args(&callee, args).unwrap_or_else(|| self.args(args));
                    format!("{}({shown})", ty(made))
                }
                Some(members) => self.anonymous(made, args, members),
            },
            Expr::NewArray { ty: element, dims, extra, init } => self.new_array(element, dims, *extra, init.as_deref()),
            Expr::ArrayInit(items) => self.array_of(&Type::Array(Box::new(Type::named("Object"))), items),
            Expr::Unary { op, expr: inner, postfix } => {
                let operand = self.operand(inner, 11);
                match (op.as_str(), postfix) {
                    ("~", _) => format!("{}.inv()", self.postfix_operand(inner)),
                    (_, true) => format!("{operand}{op}"),
                    _ => format!("{op}{operand}"),
                }
            }
            Expr::Binary { op, left, right } => {
                let level = level(op);
                let left_text = self.operand(left, level);
                let right_text = self.operand(right, level + 1);
                if let Some(word) = infix(op) {
                    return format!("{left_text} {word} {right_text}");
                }
                let shown = match op.as_str() {
                    "==" if self.is_reference(left) && self.is_reference(right) => "===",
                    "!=" if self.is_reference(left) && self.is_reference(right) => "!==",
                    other => other,
                };
                format!("{left_text} {shown} {right_text}")
            }
            Expr::Assign { op, target, value } => {
                let target_text = self.expr(target);
                let value_text = self.expr(value);
                let base = op.trim_end_matches('=');
                match infix(base) {
                    Some(word) => format!("{target_text} = {target_text} {word} {value_text}"),
                    None => format!("{target_text} {op} {value_text}"),
                }
            }
            Expr::Cond { cond, then, otherwise } => {
                let cond = self.expr(cond);
                let then = self.expr(then);
                let otherwise = self.expr(otherwise);
                format!("if ({cond}) {then} else {otherwise}")
            }
            Expr::InstanceOf { expr: inner, ty: checked, binding } => {
                let operand = self.operand(inner, 7);
                if let Some(binding) = binding {
                    let (start, end) = self.at;
                    self.problems.push((start, end, format!("the pattern variable `{binding}` has no Kotlin form; after `is`, use the checked value itself")));
                }
                format!("{operand} is {}", ty(checked))
            }
            Expr::Cast { ty: target, expr: inner } => {
                let operand = self.postfix_operand(inner);
                match target {
                    Type::Named { name: n, args }
                        if args.is_empty() && PRIMITIVES.contains(&n.as_str()) && n != "boolean" =>
                    {
                        format!("{operand}.to{}()", boxed(n).unwrap_or("Int"))
                    }
                    _ => format!("{} as {}", self.operand(inner, 11), ty(target)),
                }
            }
            Expr::Lambda { params, body } => self.lambda(params, body),
            Expr::MethodRef { target, name: method } => {
                if method == "new" {
                    return format!("::{}", self.expr(target));
                }
                let base = match target.as_ref() {
                    Expr::This => "this".to_owned(),
                    other => self.expr(other),
                };
                format!("{base}::{}", name(method))
            }
            Expr::Switch { subject, cases } => self.when(0, subject, cases, true),
        }
    }

    fn postfix_operand(&mut self, expr: &Expr) -> String {
        let text = self.expr(expr);
        match expr {
            Expr::Binary { .. }
            | Expr::Cond { .. }
            | Expr::Assign { .. }
            | Expr::Unary { postfix: false, .. }
            | Expr::Cast { .. }
            | Expr::Lambda { .. }
            | Expr::InstanceOf { .. } => {
                format!("({text})")
            }
            _ => text,
        }
    }

    fn type_of(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Name(n) => self.lookup(n).cloned(),
            Expr::Field(_, f) => self.fields.iter().rev().find_map(|fields| fields.get(f)).cloned(),
            Expr::Paren(inner) => self.type_of(inner),
            _ => None,
        }
    }

    fn receiver_is(&self, receiver: &Expr, names: &[&str]) -> bool {
        matches!(self.type_of(receiver), Some(Type::Named { name, .. }) if names.contains(&name.as_str()))
    }

    fn call(&mut self, target: Option<&Expr>, method: &str, args: &[Expr]) -> String {
        if let Some(Expr::Field(system, stream)) = target
            && matches!(system.as_ref(), Expr::Name(n) if n == "System")
            && stream == "out"
            && (method == "println" || method == "print")
        {
            return format!("{method}({})", self.args(args));
        }
        if let Some(Expr::Name(class)) = target
            && class == "String"
            && method == "valueOf"
            && args.len() == 1
        {
            return format!("{}.toString()", self.postfix_operand(&args[0]));
        }
        if let Some(receiver) = target {
            match (method, args.len()) {
                ("equals", 1) if !matches!(receiver, Expr::Super) => {
                    let left = self.operand(receiver, 7);
                    let right = self.operand(&args[0], 7);
                    return format!("{left} == {right}");
                }
                ("length" | "size", 0) if !matches!(receiver, Expr::Super | Expr::This) => {
                    return format!("{}.{method}", self.postfix_operand(receiver));
                }
                ("charAt", 1) => {
                    return format!("{}[{}]", self.postfix_operand(receiver), self.expr(&args[0]));
                }
                ("getClass", 0) => return format!("{}.javaClass", self.postfix_operand(receiver)),
                ("entrySet" | "keySet" | "values", 0) if self.receiver_is(receiver, MAPS) => {
                    let property = match method {
                        "entrySet" => "entries",
                        "keySet" => "keys",
                        _ => "values",
                    };
                    return format!("{}.{property}", self.postfix_operand(receiver));
                }
                ("getKey" | "getValue", 0) if self.receiver_is(receiver, ENTRIES) => {
                    let property = if method == "getKey" { "key" } else { "value" };
                    return format!("{}.{property}", self.postfix_operand(receiver));
                }
                ("getMessage" | "getCause" | "getLocalizedMessage" | "getStackTrace", 0) => {
                    let property = match method {
                        "getMessage" => "message",
                        "getCause" => "cause",
                        "getLocalizedMessage" => "localizedMessage",
                        _ => "stackTrace",
                    };
                    return format!("{}.{property}", self.postfix_operand(receiver));
                }
                ("toUpperCase" | "toLowerCase", 0) => {
                    let function = if method == "toUpperCase" { "uppercase" } else { "lowercase" };
                    return format!("{}.{function}()", self.postfix_operand(receiver));
                }
                _ => {}
            }
        } else if method == "getClass" && args.is_empty() {
            return "javaClass".to_owned();
        }
        let base = target.map(|receiver| format!("{}.", self.postfix_operand(receiver))).unwrap_or_default();
        let lambda_last = matches!(args.last(), Some(Expr::Lambda { body: LambdaBody::Block(_), .. }));
        if matches!(target, None | Some(Expr::This))
            && !args.iter().any(|arg| matches!(arg, Expr::Lambda { .. }))
            && let Some(shown) = self.known_args(method, args)
        {
            return format!("{base}{}({shown})", name(method));
        }
        self.lambda_label.push(Some(method.to_owned()));
        let list: Vec<String> = args.iter().map(|arg| self.expr(arg)).collect();
        self.lambda_label.pop();
        if lambda_last && list.len() == 1 {
            return format!("{base}{} {}", name(method), list[0]);
        }
        format!("{base}{}({})", name(method), list.join(", "))
    }

    fn lambda(&mut self, params: &[String], body: &LambdaBody) -> String {
        let head = if params.is_empty() {
            String::new()
        } else {
            format!("{} -> ", params.iter().map(|p| name(p)).collect::<Vec<_>>().join(", "))
        };
        match body {
            LambdaBody::Expr(value) => {
                self.lambda_label.push(None);
                let value = self.expr(value);
                self.lambda_label.pop();
                format!("{{ {head}{value} }}")
            }
            LambdaBody::Block(block) => {
                let label = self.lambda_label.last().cloned().flatten();
                let saved = std::mem::take(&mut self.out);
                let depth = self.depth;
                self.depth = 1;
                let mut stmts = block.stmts.clone();
                let last_value = match stmts.last() {
                    Some(Stmt { kind: StmtKind::Return(Some(value)), comments, .. }) if comments.is_empty() => {
                        let value = value.clone();
                        stmts.pop();
                        Some(value)
                    }
                    _ => None,
                };
                self.lambda_label.push(label.or(Some("lambda".to_owned())));
                self.block_body(&Block { stmts, end_comments: block.end_comments.clone() }, &[]);
                if let Some(value) = last_value {
                    let value = self.expr(&value);
                    self.line(&value);
                }
                self.lambda_label.pop();
                let inner = std::mem::replace(&mut self.out, saved);
                self.depth = depth;
                let indent = "    ".repeat(depth);
                let mut out = format!("{{ {}\n", head.trim_end());
                for line in inner.lines() {
                    out.push_str(&indent);
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str(&indent);
                out.push('}');
                out.replace("{ \n", "{\n")
            }
        }
    }

    fn anonymous(&mut self, made: &Type, args: &[Expr], members: &[Member]) -> String {
        let interface = args.is_empty() && looks_like_interface(&ty(made));
        let base = if interface { ty(made) } else { format!("{}({})", ty(made), self.args(args)) };
        let saved = std::mem::take(&mut self.out);
        let depth = self.depth;
        self.depth = 1;
        let decl = TypeDecl {
            comments: Vec::new(),
            mods: Mods::default(),
            kind: TypeKind::Class,
            name: String::new(),
            params: Vec::new(),
            extends: Vec::new(),
            implements: Vec::new(),
            components: Vec::new(),
            constants: Vec::new(),
            members: members.to_vec(),
        };
        self.fields.push(HashMap::new());
        self.members(members, &decl, None);
        self.fields.pop();
        let inner = std::mem::replace(&mut self.out, saved);
        self.depth = depth;
        let indent = "    ".repeat(depth);
        let mut out = format!("object : {base} {{\n");
        for line in inner.lines() {
            if !line.is_empty() {
                out.push_str(&indent);
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&indent);
        out.push('}');
        out
    }

    fn new_array(&mut self, element: &Type, dims: &[Expr], extra: usize, init: Option<&[Expr]>) -> String {
        let total = dims.len() + extra;
        let mut array = element.clone();
        for _ in 0..total {
            array = Type::Array(Box::new(array));
        }
        if let Some(items) = init {
            return self.array_of(&array, items);
        }
        let sizes: Vec<String> = dims.iter().map(|dim| self.expr(dim)).collect();
        fn build(element: &Type, sizes: &[String], remaining: usize) -> String {
            let inner_type = {
                let mut t = element.clone();
                for _ in 1..remaining {
                    t = Type::Array(Box::new(t));
                }
                t
            };
            match sizes {
                [] => "null".to_owned(),
                [size] if remaining == 1 => match element {
                    Type::Named { name, .. } if PRIMITIVES.contains(&name.as_str()) => {
                        format!("{}({size})", ty(&Type::Array(Box::new(element.clone()))))
                    }
                    _ => format!("arrayOfNulls<{}>({size})", ty(element)),
                },
                [size] => format!("arrayOfNulls<{}>({size})", ty(&inner_type)),
                [size, rest @ ..] => format!("Array({size}) {{ {} }}", build(element, rest, remaining - 1)),
            }
        }
        build(element, &sizes, total)
    }
}

#[derive(Default)]
struct Primary {
    params: Vec<String>,
    super_args: Option<String>,
    moved: HashSet<String>,
    init: Vec<Stmt>,
    init_comments: Vec<usize>,
    init_params: Vec<Param>,
}

fn wrap_dims(t: &Type, dims: usize) -> Type {
    let mut result = t.clone();
    for _ in 0..dims {
        result = Type::Array(Box::new(result));
    }
    result
}

fn is_static_member(member: &Member, kind: TypeKind) -> bool {
    match member {
        Member::Field { mods, .. } => mods.has("static") || kind == TypeKind::Interface,
        Member::Method(method) => method.mods.has("static"),
        Member::Init { is_static, .. } => *is_static,
        Member::Type(_) | Member::Raw { .. } => false,
    }
}

fn assigned_field(target: &Expr, params: &[Param]) -> Option<String> {
    match target {
        Expr::Field(this, field) if matches!(this.as_ref(), Expr::This) => Some(field.clone()),
        Expr::Name(field) if !params.iter().any(|p| &p.name == field) => Some(field.clone()),
        _ => None,
    }
}

fn rank(name: &str) -> Option<usize> {
    ["byte", "short", "char", "int", "long", "float", "double"].iter().position(|known| *known == name)
}

fn collect_signatures(decl: &TypeDecl, found: &mut HashMap<(String, usize), Vec<Type>>) {
    let mut seen: HashMap<(String, usize), usize> = HashMap::new();
    for member in &decl.members {
        match member {
            Member::Method(method) if !method.params.iter().any(|p| p.varargs) => {
                let key = (method.name.clone(), method.params.len());
                *seen.entry(key.clone()).or_default() += 1;
                found.insert(key, method.params.iter().map(|p| p.ty.clone()).collect());
            }
            Member::Type(inner) => collect_signatures(inner, found),
            _ => {}
        }
    }
    for (key, count) in seen {
        if count > 1 {
            found.remove(&key);
        }
    }
}

fn collect_open(decl: &TypeDecl, open: &mut HashSet<String>) {
    for base in &decl.extends {
        if decl.kind == TypeKind::Class
            && let Some(simple) = base.simple_name()
        {
            open.insert(simple.to_owned());
        }
    }
    for member in &decl.members {
        if let Member::Type(inner) = member {
            collect_open(inner, open);
        }
    }
}

fn block_returns_null(block: &Block) -> bool {
    block.stmts.iter().any(stmt_returns_null)
}

fn stmt_returns_null(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(Some(Expr::Literal(Lit::Null, _))) => true,
        StmtKind::Block(block) => block_returns_null(block),
        StmtKind::If { then, otherwise, .. } => {
            stmt_returns_null(then) || otherwise.as_deref().is_some_and(stmt_returns_null)
        }
        StmtKind::While { body, .. }
        | StmtKind::ForEach { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::DoWhile { body, .. } => stmt_returns_null(body),
        StmtKind::Labeled { body, .. } => stmt_returns_null(body),
        StmtKind::Try { body, catches, finally, .. } => {
            block_returns_null(body)
                || catches.iter().any(|c| block_returns_null(&c.body))
                || finally.as_ref().is_some_and(block_returns_null)
        }
        StmtKind::Switch { cases, .. } => cases.iter().any(|case| case.body.iter().any(stmt_returns_null)),
        StmtKind::Sync { body, .. } => block_returns_null(body),
        _ => false,
    }
}

fn assigned_in(stmts: &[Stmt]) -> HashSet<String> {
    let mut found = HashSet::new();
    for stmt in stmts {
        visit_stmt(stmt, &mut |expr| match expr {
            Expr::Assign { target, .. } => {
                if let Expr::Name(n) = target.as_ref() {
                    found.insert(n.clone());
                }
            }
            Expr::Unary { op, expr, .. } if op == "++" || op == "--" => {
                if let Expr::Name(n) = expr.as_ref() {
                    found.insert(n.clone());
                }
            }
            _ => {}
        });
    }
    found
}

fn assigns(stmt: &Stmt, var: &str) -> bool {
    assigned_in(std::slice::from_ref(stmt)).contains(var)
}

fn contains_continue(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Continue(_) => true,
        StmtKind::Block(block) => block.stmts.iter().any(contains_continue),
        StmtKind::If { then, otherwise, .. } => {
            contains_continue(then) || otherwise.as_deref().is_some_and(contains_continue)
        }
        StmtKind::Labeled { body, .. } => contains_continue(body),
        StmtKind::Try { body, catches, finally, .. } => {
            body.stmts.iter().any(contains_continue)
                || catches.iter().any(|c| c.body.stmts.iter().any(contains_continue))
                || finally.as_ref().is_some_and(|f| f.stmts.iter().any(contains_continue))
        }
        StmtKind::Switch { cases, .. } => cases.iter().any(|case| case.body.iter().any(contains_continue)),
        StmtKind::Sync { body, .. } => body.stmts.iter().any(contains_continue),
        _ => false,
    }
}

fn visit_stmt(stmt: &Stmt, visit: &mut dyn FnMut(&Expr)) {
    match &stmt.kind {
        StmtKind::Block(block) => block.stmts.iter().for_each(|s| visit_stmt(s, visit)),
        StmtKind::Local { vars, .. } => vars.iter().filter_map(|v| v.init.as_ref()).for_each(|e| visit_expr(e, visit)),
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::Yield(e) => visit_expr(e, visit),
        StmtKind::Return(e) => {
            if let Some(e) = e {
                visit_expr(e, visit);
            }
        }
        StmtKind::If { cond, then, otherwise } => {
            visit_expr(cond, visit);
            visit_stmt(then, visit);
            if let Some(o) = otherwise {
                visit_stmt(o, visit);
            }
        }
        StmtKind::While { cond, body } | StmtKind::DoWhile { body, cond } => {
            visit_expr(cond, visit);
            visit_stmt(body, visit);
        }
        StmtKind::For { init, cond, update, body } => {
            init.iter().for_each(|s| visit_stmt(s, visit));
            if let Some(c) = cond {
                visit_expr(c, visit);
            }
            update.iter().for_each(|e| visit_expr(e, visit));
            visit_stmt(body, visit);
        }
        StmtKind::ForEach { iterable, body, .. } => {
            visit_expr(iterable, visit);
            visit_stmt(body, visit);
        }
        StmtKind::Try { resources, body, catches, finally } => {
            resources.iter().for_each(|r| visit_expr(&r.value, visit));
            body.stmts.iter().for_each(|s| visit_stmt(s, visit));
            catches.iter().for_each(|c| c.body.stmts.iter().for_each(|s| visit_stmt(s, visit)));
            if let Some(f) = finally {
                f.stmts.iter().for_each(|s| visit_stmt(s, visit));
            }
        }
        StmtKind::Switch { subject, cases } => {
            visit_expr(subject, visit);
            cases.iter().for_each(|c| c.body.iter().for_each(|s| visit_stmt(s, visit)));
        }
        StmtKind::Labeled { body, .. } => visit_stmt(body, visit),
        StmtKind::Sync { lock, body } => {
            visit_expr(lock, visit);
            body.stmts.iter().for_each(|s| visit_stmt(s, visit));
        }
        StmtKind::Assert { cond, message } => {
            visit_expr(cond, visit);
            if let Some(m) = message {
                visit_expr(m, visit);
            }
        }
        StmtKind::ThisCall { args, .. } => args.iter().for_each(|e| visit_expr(e, visit)),
        StmtKind::LocalType(_) | StmtKind::Break(_) | StmtKind::Continue(_) | StmtKind::Empty | StmtKind::Raw => {}
    }
}

fn visit_expr(expr: &Expr, visit: &mut dyn FnMut(&Expr)) {
    visit(expr);
    match expr {
        Expr::Field(t, _) => visit_expr(t, visit),
        Expr::Call { target, args, .. } => {
            if let Some(t) = target {
                visit_expr(t, visit);
            }
            args.iter().for_each(|a| visit_expr(a, visit));
        }
        Expr::New { args, .. } => args.iter().for_each(|a| visit_expr(a, visit)),
        Expr::NewArray { dims, init, .. } => {
            dims.iter().for_each(|d| visit_expr(d, visit));
            if let Some(items) = init {
                items.iter().for_each(|i| visit_expr(i, visit));
            }
        }
        Expr::ArrayInit(items) => items.iter().for_each(|i| visit_expr(i, visit)),
        Expr::Index(a, b) | Expr::Binary { left: a, right: b, .. } | Expr::Assign { target: a, value: b, .. } => {
            visit_expr(a, visit);
            visit_expr(b, visit);
        }
        Expr::Unary { expr, .. } | Expr::Paren(expr) | Expr::Cast { expr, .. } | Expr::InstanceOf { expr, .. } => {
            visit_expr(expr, visit)
        }
        Expr::Cond { cond, then, otherwise } => {
            visit_expr(cond, visit);
            visit_expr(then, visit);
            visit_expr(otherwise, visit);
        }
        Expr::Lambda { body, .. } => match body {
            LambdaBody::Expr(e) => visit_expr(e, visit),
            LambdaBody::Block(b) => b.stmts.iter().for_each(|s| visit_stmt(s, visit)),
        },
        Expr::MethodRef { target, .. } => visit_expr(target, visit),
        Expr::Switch { subject, cases } => {
            visit_expr(subject, visit);
            cases.iter().for_each(|c| c.body.iter().for_each(|s| visit_stmt(s, visit)));
        }
        _ => {}
    }
}
