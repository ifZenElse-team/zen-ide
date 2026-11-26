use super::ast::*;
use super::lex::{Comment, Tk, Token, lex};

pub(crate) struct Parser<'a> {
    pub(crate) text: &'a str,
    toks: Vec<Token>,
    pub(crate) comments: Vec<Comment>,
    pos: usize,
    taken: usize,
    pub(crate) problems: Vec<(usize, usize, String)>,
}

const MODIFIERS: &[&str] = &[
    "public",
    "protected",
    "private",
    "static",
    "final",
    "abstract",
    "native",
    "synchronized",
    "transient",
    "volatile",
    "strictfp",
    "default",
    "sealed",
    "non-sealed",
];

impl<'a> Parser<'a> {
    pub(crate) fn new(text: &'a str) -> Parser<'a> {
        let (toks, comments) = lex(text);
        Parser { text, toks, comments, pos: 0, taken: 0, problems: Vec::new() }
    }

    fn tok(&self) -> Token {
        self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn peek(&self, ahead: usize) -> Token {
        self.toks[(self.pos + ahead).min(self.toks.len() - 1)]
    }

    fn src(&self, token: Token) -> &'a str {
        &self.text[token.start..token.end]
    }

    fn is(&self, text: &str) -> bool {
        let token = self.tok();
        token.kind != Tk::Eof && token.kind != Tk::Str && token.kind != Tk::Char && self.src(token) == text
    }

    fn is_at(&self, ahead: usize, text: &str) -> bool {
        let token = self.peek(ahead);
        matches!(token.kind, Tk::Ident | Tk::Punct | Tk::At) && self.src(token) == text
    }

    fn eat(&mut self, text: &str) -> bool {
        if self.is(text) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, text: &str) -> Result<(), ()> {
        if self.eat(text) { Ok(()) } else { Err(()) }
    }

    fn at_eof(&self) -> bool {
        self.tok().kind == Tk::Eof
    }

    fn ident(&mut self) -> Result<String, ()> {
        let token = self.tok();
        if token.kind == Tk::Ident {
            self.pos += 1;
            Ok(self.src(token).to_owned())
        } else {
            Err(())
        }
    }

    pub(crate) fn take_comments(&mut self, before: usize) -> Vec<usize> {
        let mut found = Vec::new();
        while self.taken < self.comments.len() && self.comments[self.taken].end <= before {
            found.push(self.taken);
            self.taken += 1;
        }
        found
    }

    fn here(&self) -> usize {
        self.tok().start
    }

    fn last_end(&self) -> usize {
        if self.pos == 0 { 0 } else { self.toks[self.pos - 1].end }
    }

    fn report(&mut self, start: usize, end: usize, message: &str) {
        self.problems.push((start, end, message.to_owned()));
    }

    fn skip_balanced_until(&mut self, stops: &[&str]) {
        let mut depth = 0i32;
        while !self.at_eof() {
            if depth == 0 && stops.iter().any(|stop| self.is(stop)) {
                return;
            }
            if self.is("(") || self.is("{") || self.is("[") {
                depth += 1;
            } else if self.is(")") || self.is("}") || self.is("]") {
                if depth == 0 {
                    return;
                }
                depth -= 1;
            }
            self.pos += 1;
        }
    }

    pub(crate) fn unit(&mut self) -> Unit {
        let mut unit = Unit::default();
        let mods_start = self.pos;
        let _ = self.annotations();
        if self.is("package") {
            self.pos += 1;
            unit.package = self.qualified().ok();
            let _ = self.eat(";");
        } else {
            self.pos = mods_start;
        }
        while self.is("import") {
            self.pos += 1;
            let is_static = self.eat("static");
            let mut path = self.ident().unwrap_or_default();
            let mut wildcard = false;
            while self.eat(".") {
                if self.eat("*") {
                    wildcard = true;
                    break;
                }
                path.push('.');
                path.push_str(&self.ident().unwrap_or_default());
            }
            let _ = self.eat(";");
            unit.imports.push(Import { path, is_static, wildcard });
        }
        while !self.at_eof() {
            if self.eat(";") {
                continue;
            }
            let start = self.pos;
            match self.type_decl() {
                Ok(decl) => unit.types.push(decl),
                Err(()) => {
                    self.pos = start;
                    let from = self.here();
                    self.skip_balanced_until(&[";"]);
                    if self.is("}") {
                        self.pos += 1;
                    }
                    let _ = self.eat(";");
                    let to = self.last_end().max(from + 1).min(self.text.len());
                    self.report(from, to, "this declaration could not be read");
                    if self.pos == start {
                        self.pos += 1;
                    }
                }
            }
        }
        unit.trailing = self.take_comments(self.text.len());
        unit
    }

    fn qualified(&mut self) -> Result<String, ()> {
        let mut name = self.ident()?;
        while self.is(".") && self.peek(1).kind == Tk::Ident {
            self.pos += 1;
            name.push('.');
            name.push_str(&self.ident()?);
        }
        Ok(name)
    }

    fn annotation(&mut self) -> Result<String, ()> {
        let start = self.here();
        self.expect("@")?;
        if self.is("interface") {
            return Err(());
        }
        let _ = self.qualified()?;
        if self.is("(") {
            self.pos += 1;
            self.skip_balanced_until(&[")"]);
            self.expect(")")?;
        }
        Ok(self.text[start + 1..self.last_end()].to_owned())
    }

    fn annotations(&mut self) -> Vec<String> {
        let mut found = Vec::new();
        while self.is("@") && !self.is_at(1, "interface") {
            let save = self.pos;
            match self.annotation() {
                Ok(annotation) => found.push(annotation),
                Err(()) => {
                    self.pos = save;
                    break;
                }
            }
        }
        found
    }

    fn mods(&mut self) -> Mods {
        let mut mods = Mods::default();
        loop {
            if self.is("@") && !self.is_at(1, "interface") {
                match self.annotation() {
                    Ok(annotation) => mods.annotations.push(annotation),
                    Err(()) => break,
                }
            } else if self.is("non") && self.is_at(1, "-") && self.is_at(2, "sealed") {
                self.pos += 3;
                mods.words.push("non-sealed".to_owned());
            } else if MODIFIERS.contains(&self.src(self.tok())) && self.tok().kind == Tk::Ident {
                if self.is("default") && (self.is_at(1, ":") || self.is_at(1, "->")) {
                    break;
                }
                mods.words.push(self.src(self.tok()).to_owned());
                self.pos += 1;
            } else {
                break;
            }
        }
        mods
    }

    fn type_decl(&mut self) -> Result<TypeDecl, ()> {
        let comments = self.take_comments(self.here());
        let start = self.here();
        let mods = self.mods();
        self.type_decl_after(comments, mods, start)
    }

    fn type_decl_after(&mut self, comments: Vec<usize>, mods: Mods, _start: usize) -> Result<TypeDecl, ()> {
        let kind = if self.eat("class") {
            TypeKind::Class
        } else if self.eat("interface") {
            TypeKind::Interface
        } else if self.eat("enum") {
            TypeKind::Enum
        } else if self.is("record") && self.peek(1).kind == Tk::Ident {
            self.pos += 1;
            TypeKind::Record
        } else if self.is("@") && self.is_at(1, "interface") {
            self.pos += 2;
            TypeKind::Annotation
        } else {
            return Err(());
        };
        let name = self.ident()?;
        let params = self.type_params()?;
        let mut components = Vec::new();
        if kind == TypeKind::Record {
            components = self.params()?;
        }
        let mut extends = Vec::new();
        let mut implements = Vec::new();
        loop {
            if self.eat("extends") {
                extends = self.type_list()?;
            } else if self.eat("implements") {
                implements = self.type_list()?;
            } else if self.eat("permits") {
                let _ = self.type_list()?;
            } else {
                break;
            }
        }
        self.expect("{")?;
        let mut constants = Vec::new();
        if kind == TypeKind::Enum {
            while !self.is(";") && !self.is("}") && !self.at_eof() {
                let comments = self.take_comments(self.here());
                let _ = self.annotations();
                let name = self.ident()?;
                let args = if self.is("(") { self.args()? } else { Vec::new() };
                let body = if self.is("{") { Some(self.class_body()?) } else { None };
                constants.push(EnumConstant { comments, name, args, body });
                if !self.eat(",") {
                    break;
                }
            }
            let _ = self.eat(";");
        }
        let members = self.members_until_close()?;
        Ok(TypeDecl { comments, mods, kind, name, params, extends, implements, components, constants, members })
    }

    fn type_list(&mut self) -> Result<Vec<Type>, ()> {
        let mut list = vec![self.ty()?];
        while self.eat(",") {
            list.push(self.ty()?);
        }
        Ok(list)
    }

    fn type_params(&mut self) -> Result<Vec<TypeParam>, ()> {
        let mut params = Vec::new();
        if !self.eat("<") {
            return Ok(params);
        }
        loop {
            let _ = self.annotations();
            let name = self.ident()?;
            let mut bounds = Vec::new();
            if self.eat("extends") {
                bounds.push(self.ty()?);
                while self.eat("&") {
                    bounds.push(self.ty()?);
                }
            }
            params.push(TypeParam { name, bounds });
            if !self.eat(",") {
                break;
            }
        }
        self.close_angle()?;
        Ok(params)
    }

    fn close_angle(&mut self) -> Result<(), ()> {
        let token = self.tok();
        let text = self.src(token);
        if text == ">" {
            self.pos += 1;
            return Ok(());
        }
        if token.kind == Tk::Punct && text.starts_with('>') {
            let rest = Token { kind: Tk::Punct, start: token.start + 1, end: token.end };
            self.toks[self.pos] = rest;
            return Ok(());
        }
        Err(())
    }

    fn class_body(&mut self) -> Result<Vec<Member>, ()> {
        self.expect("{")?;
        self.members_until_close()
    }

    fn members_until_close(&mut self) -> Result<Vec<Member>, ()> {
        let mut members = Vec::new();
        while !self.is("}") {
            if self.at_eof() {
                return Err(());
            }
            if self.eat(";") {
                continue;
            }
            let start = self.pos;
            match self.member() {
                Ok(member) => members.push(member),
                Err(()) => {
                    self.pos = start;
                    let comments = self.take_comments(self.here());
                    let from = self.here();
                    self.skip_balanced_until(&[";", "}"]);
                    if self.is("{") {
                        self.skip_block();
                    }
                    let _ = self.eat(";");
                    if self.pos == start {
                        self.pos += 1;
                    }
                    let to = self.last_end().max(from);
                    self.report(from, to, "this member could not be converted and is kept as Java");
                    members.push(Member::Raw { comments, start: from, end: to });
                }
            }
        }
        self.expect("}")?;
        Ok(members)
    }

    fn skip_block(&mut self) {
        if self.eat("{") {
            self.skip_balanced_until(&[]);
            let _ = self.eat("}");
        }
    }

    fn member(&mut self) -> Result<Member, ()> {
        let comments = self.take_comments(self.here());
        let start = self.here();
        if self.is("{") || (self.is("static") && self.is_at(1, "{")) {
            let is_static = self.eat("static");
            let body = self.block()?;
            return Ok(Member::Init { comments, is_static, body });
        }
        let mods = self.mods();
        if self.is("class")
            || self.is("interface")
            || self.is("enum")
            || (self.is("@") && self.is_at(1, "interface"))
            || (self.is("record") && self.peek(1).kind == Tk::Ident && self.is_at(2, "("))
            || (self.is("record") && self.peek(1).kind == Tk::Ident && self.is_at(2, "<"))
        {
            return Ok(Member::Type(Box::new(self.type_decl_after(comments, mods, start)?)));
        }
        let params_of_type = self.type_params()?;
        if self.tok().kind == Tk::Ident && self.is_at(1, "(") {
            let name = self.ident()?;
            let params = self.params()?;
            self.skip_throws()?;
            let body = Some(self.block()?);
            return Ok(Member::Method(Box::new(Method {
                comments,
                mods,
                params_of_type,
                result: None,
                name,
                params,
                body,
            })));
        }
        if self.tok().kind == Tk::Ident && self.is_at(1, "{") {
            let name = self.ident()?;
            let body = Some(self.block()?);
            return Ok(Member::Method(Box::new(Method {
                comments,
                mods,
                params_of_type,
                result: None,
                name,
                params: Vec::new(),
                body,
            })));
        }
        let ty = self.ty()?;
        let name = self.ident()?;
        if self.is("(") {
            let params = self.params()?;
            let mut extra = 0;
            while self.eat("[") {
                self.expect("]")?;
                extra += 1;
            }
            let mut result = ty;
            for _ in 0..extra {
                result = Type::Array(Box::new(result));
            }
            self.skip_throws()?;
            if self.eat("default") {
                let _ = self.expr()?;
            }
            let body = if self.eat(";") { None } else { Some(self.block()?) };
            return Ok(Member::Method(Box::new(Method {
                comments,
                mods,
                params_of_type,
                result: Some(result),
                name,
                params,
                body,
            })));
        }
        let vars = self.vars_after_name(name)?;
        self.expect(";")?;
        Ok(Member::Field { comments, mods, ty, vars })
    }

    fn skip_throws(&mut self) -> Result<(), ()> {
        if self.eat("throws") {
            let _ = self.type_list()?;
        }
        Ok(())
    }

    fn vars_after_name(&mut self, first: String) -> Result<Vec<Var>, ()> {
        let mut vars = Vec::new();
        let mut name = first;
        loop {
            let mut dims = 0;
            while self.eat("[") {
                self.expect("]")?;
                dims += 1;
            }
            let init = if self.eat("=") { Some(self.var_init()?) } else { None };
            vars.push(Var { name, dims, init });
            if !self.eat(",") {
                break;
            }
            name = self.ident()?;
        }
        Ok(vars)
    }

    fn var_init(&mut self) -> Result<Expr, ()> {
        if self.is("{") { self.array_init() } else { self.expr() }
    }

    fn array_init(&mut self) -> Result<Expr, ()> {
        self.expect("{")?;
        let mut items = Vec::new();
        while !self.is("}") {
            items.push(self.var_init()?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        Ok(Expr::ArrayInit(items))
    }

    fn params(&mut self) -> Result<Vec<Param>, ()> {
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.is(")") {
            let _ = self.mods();
            let ty = self.ty()?;
            let varargs = self.eat("...");
            if self.is("this") {
                self.pos += 1;
            } else {
                let name = self.ident()?;
                let mut ty = ty;
                while self.eat("[") {
                    self.expect("]")?;
                    ty = Type::Array(Box::new(ty));
                }
                params.push(Param { ty, name, varargs });
            }
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(params)
    }

    pub(crate) fn ty(&mut self) -> Result<Type, ()> {
        let _ = self.annotations();
        if self.eat("var") && self.tok().kind == Tk::Ident {
            return Ok(Type::Var);
        }
        if self.is_at(0, "var") {
            return Err(());
        }
        let mut name = self.ident()?;
        let mut args = self.type_args()?;
        while self.is(".") && self.peek(1).kind == Tk::Ident {
            self.pos += 1;
            let _ = self.annotations();
            name.push('.');
            name.push_str(&self.ident()?);
            let more = self.type_args()?;
            if !more.is_empty() {
                args = more;
            }
        }
        let mut ty = Type::Named { name, args };
        while self.is("[") && self.is_at(1, "]") {
            self.pos += 2;
            ty = Type::Array(Box::new(ty));
        }
        Ok(ty)
    }

    fn type_args(&mut self) -> Result<Vec<TypeArg>, ()> {
        let mut args = Vec::new();
        if !self.is("<") {
            return Ok(args);
        }
        self.pos += 1;
        if self.is(">") {
            self.pos += 1;
            return Ok(args);
        }
        loop {
            let _ = self.annotations();
            if self.eat("?") {
                if self.eat("extends") {
                    args.push(TypeArg::Extends(self.ty()?));
                } else if self.eat("super") {
                    args.push(TypeArg::Super(self.ty()?));
                } else {
                    args.push(TypeArg::Any);
                }
            } else {
                args.push(TypeArg::Type(self.ty()?));
            }
            if !self.eat(",") {
                break;
            }
        }
        self.close_angle()?;
        Ok(args)
    }

    pub(crate) fn block(&mut self) -> Result<Block, ()> {
        self.expect("{")?;
        let mut block = Block::default();
        while !self.is("}") {
            if self.at_eof() {
                return Err(());
            }
            block.stmts.push(self.stmt_or_raw());
        }
        block.end_comments = self.take_comments(self.here());
        self.expect("}")?;
        Ok(block)
    }

    fn stmt_or_raw(&mut self) -> Stmt {
        let save = self.pos;
        let taken = self.taken;
        match self.stmt() {
            Ok(stmt) => stmt,
            Err(()) => {
                self.pos = save;
                self.taken = taken;
                let comments = self.take_comments(self.here());
                let from = self.here();
                self.skip_balanced_until(&[";"]);
                let _ = self.eat(";");
                if self.pos == save {
                    self.pos += 1;
                }
                let to = self.last_end().max(from);
                self.report(from, to, "this statement could not be converted and is kept as Java");
                Stmt { comments, kind: StmtKind::Raw, start: from, end: to }
            }
        }
    }

    fn stmt(&mut self) -> Result<Stmt, ()> {
        let comments = self.take_comments(self.here());
        let start = self.here();
        let kind = self.stmt_kind()?;
        Ok(Stmt { comments, kind, start, end: self.last_end() })
    }

    fn boxed_stmt(&mut self) -> Result<Box<Stmt>, ()> {
        Ok(Box::new(self.stmt()?))
    }

    fn paren_expr(&mut self) -> Result<Expr, ()> {
        self.expect("(")?;
        let expr = self.expr()?;
        self.expect(")")?;
        Ok(expr)
    }

    fn stmt_kind(&mut self) -> Result<StmtKind, ()> {
        if self.is("{") {
            return Ok(StmtKind::Block(self.block()?));
        }
        if self.eat(";") {
            return Ok(StmtKind::Empty);
        }
        if self.tok().kind == Tk::Ident && self.is_at(1, ":") && !self.is("default") {
            let label = self.ident()?;
            self.pos += 1;
            return Ok(StmtKind::Labeled { label, body: self.boxed_stmt()? });
        }
        let word = if self.tok().kind == Tk::Ident { self.src(self.tok()) } else { "" };
        match word {
            "if" => {
                self.pos += 1;
                let cond = self.paren_expr()?;
                let then = self.boxed_stmt()?;
                let otherwise = if self.eat("else") { Some(self.boxed_stmt()?) } else { None };
                return Ok(StmtKind::If { cond, then, otherwise });
            }
            "while" => {
                self.pos += 1;
                let cond = self.paren_expr()?;
                return Ok(StmtKind::While { cond, body: self.boxed_stmt()? });
            }
            "do" => {
                self.pos += 1;
                let body = self.boxed_stmt()?;
                self.expect("while")?;
                let cond = self.paren_expr()?;
                self.expect(";")?;
                return Ok(StmtKind::DoWhile { body, cond });
            }
            "for" => return self.for_stmt(),
            "return" => {
                self.pos += 1;
                let value = if self.is(";") { None } else { Some(self.expr()?) };
                self.expect(";")?;
                return Ok(StmtKind::Return(value));
            }
            "throw" => {
                self.pos += 1;
                let value = self.expr()?;
                self.expect(";")?;
                return Ok(StmtKind::Throw(value));
            }
            "break" | "continue" => {
                self.pos += 1;
                let label = if self.tok().kind == Tk::Ident { Some(self.ident()?) } else { None };
                self.expect(";")?;
                return Ok(if word == "break" { StmtKind::Break(label) } else { StmtKind::Continue(label) });
            }
            "yield" if !self.is_at(1, "=") && !self.is_at(1, "(") && !self.is_at(1, ".") => {
                self.pos += 1;
                let value = self.expr()?;
                self.expect(";")?;
                return Ok(StmtKind::Yield(value));
            }
            "try" => return self.try_stmt(),
            "switch" => {
                self.pos += 1;
                let subject = self.paren_expr()?;
                let cases = self.cases()?;
                return Ok(StmtKind::Switch { subject, cases });
            }
            "synchronized" if self.is_at(1, "(") => {
                self.pos += 1;
                let lock = self.paren_expr()?;
                return Ok(StmtKind::Sync { lock, body: self.block()? });
            }
            "assert" => {
                self.pos += 1;
                let cond = self.expr()?;
                let message = if self.eat(":") { Some(self.expr()?) } else { None };
                self.expect(";")?;
                return Ok(StmtKind::Assert { cond, message });
            }
            "this" | "super" if self.is_at(1, "(") => {
                let is_super = word == "super";
                let save = self.pos;
                self.pos += 1;
                let args = self.args()?;
                if self.eat(";") {
                    return Ok(StmtKind::ThisCall { is_super, args });
                }
                self.pos = save;
            }
            "class" | "interface" | "enum" | "abstract" | "static" => {
                let start = self.here();
                let mods = self.mods();
                return Ok(StmtKind::LocalType(Box::new(self.type_decl_after(Vec::new(), mods, start)?)));
            }
            "record" if self.peek(1).kind == Tk::Ident && self.is_at(2, "(") => {
                let start = self.here();
                return Ok(StmtKind::LocalType(Box::new(self.type_decl_after(Vec::new(), Mods::default(), start)?)));
            }
            _ => {}
        }
        if let Some(local) = self.try_local()? {
            return Ok(local);
        }
        let expr = self.expr()?;
        self.expect(";")?;
        Ok(StmtKind::Expr(expr))
    }

    fn try_local(&mut self) -> Result<Option<StmtKind>, ()> {
        let save = self.pos;
        let mods = self.mods();
        let is_final = mods.has("final");
        if self.is("class") || self.is("record") && self.peek(1).kind == Tk::Ident && self.is_at(2, "(") {
            let start = self.here();
            return Ok(Some(StmtKind::LocalType(Box::new(self.type_decl_after(Vec::new(), mods, start)?))));
        }
        if let Ok(ty) = self.ty()
            && self.tok().kind == Tk::Ident
            && (self.is_at(1, "=")
                || self.is_at(1, ";")
                || self.is_at(1, ",")
                || self.is_at(1, "[")
                || self.is_at(1, ":"))
        {
            let name = self.ident()?;
            let vars = self.vars_after_name(name)?;
            self.expect(";")?;
            return Ok(Some(StmtKind::Local { is_final, ty, vars }));
        }
        self.pos = save;
        Ok(None)
    }

    fn for_stmt(&mut self) -> Result<StmtKind, ()> {
        self.pos += 1;
        self.expect("(")?;
        let save = self.pos;
        let _ = self.mods();
        if let Ok(ty) = self.ty()
            && self.tok().kind == Tk::Ident
            && self.is_at(1, ":")
        {
            let name = self.ident()?;
            self.pos += 1;
            let iterable = self.expr()?;
            self.expect(")")?;
            let body = self.boxed_stmt()?;
            return Ok(StmtKind::ForEach { ty, name, iterable, body });
        }
        self.pos = save;
        let mut init = Vec::new();
        if !self.eat(";") {
            let start = self.here();
            if let Some(local) = self.try_local()? {
                init.push(Stmt { comments: Vec::new(), kind: local, start, end: self.last_end() });
            } else {
                loop {
                    let start = self.here();
                    let expr = self.expr()?;
                    init.push(Stmt { comments: Vec::new(), kind: StmtKind::Expr(expr), start, end: self.last_end() });
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(";")?;
            }
        }
        let cond = if self.is(";") { None } else { Some(self.expr()?) };
        self.expect(";")?;
        let mut update = Vec::new();
        while !self.is(")") {
            update.push(self.expr()?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        let body = self.boxed_stmt()?;
        Ok(StmtKind::For { init, cond, update, body })
    }

    fn try_stmt(&mut self) -> Result<StmtKind, ()> {
        self.pos += 1;
        let mut resources = Vec::new();
        if self.eat("(") {
            while !self.is(")") {
                let save = self.pos;
                let _ = self.mods();
                let resource = match self.ty() {
                    Ok(ty) if self.tok().kind == Tk::Ident && self.is_at(1, "=") => {
                        let name = self.ident()?;
                        self.pos += 1;
                        let value = self.expr()?;
                        let _ = ty;
                        Resource { name: Some(name), value }
                    }
                    _ => {
                        self.pos = save;
                        Resource { name: None, value: self.expr()? }
                    }
                };
                resources.push(resource);
                if !self.eat(";") {
                    break;
                }
            }
            self.expect(")")?;
        }
        let body = self.block()?;
        let mut catches = Vec::new();
        while self.eat("catch") {
            self.expect("(")?;
            let _ = self.mods();
            let mut types = vec![self.ty()?];
            while self.eat("|") {
                types.push(self.ty()?);
            }
            let name = self.ident()?;
            self.expect(")")?;
            catches.push(Catch { types, name, body: self.block()? });
        }
        let finally = if self.eat("finally") { Some(self.block()?) } else { None };
        Ok(StmtKind::Try { resources, body, catches, finally })
    }

    fn cases(&mut self) -> Result<Vec<Case>, ()> {
        self.expect("{")?;
        let mut cases = Vec::new();
        while !self.is("}") {
            let start = self.here();
            let mut labels = Vec::new();
            let is_default;
            if self.eat("default") {
                is_default = true;
            } else {
                self.expect("case")?;
                is_default = false;
                loop {
                    if self.eat("default") {
                        break;
                    }
                    labels.push(self.ternary()?);
                    if !self.eat(",") {
                        break;
                    }
                }
            }
            let mut body = Vec::new();
            let arrow = if self.eat("->") {
                if self.is("{") {
                    let begin = self.here();
                    let block = self.block()?;
                    body.push(Stmt {
                        comments: Vec::new(),
                        kind: StmtKind::Block(block),
                        start: begin,
                        end: self.last_end(),
                    });
                } else if self.is("throw") {
                    body.push(self.stmt()?);
                } else {
                    let begin = self.here();
                    let expr = self.expr()?;
                    self.expect(";")?;
                    body.push(Stmt {
                        comments: Vec::new(),
                        kind: StmtKind::Yield(expr),
                        start: begin,
                        end: self.last_end(),
                    });
                }
                true
            } else {
                self.expect(":")?;
                while !self.is("case") && !self.is("default") && !self.is("}") {
                    if self.at_eof() {
                        return Err(());
                    }
                    if self.is("default") && !(self.is_at(1, ":") || self.is_at(1, "->")) {
                        break;
                    }
                    body.push(self.stmt_or_raw());
                }
                false
            };
            cases.push(Case { labels, is_default, arrow, body, start });
        }
        self.expect("}")?;
        Ok(cases)
    }

    fn args(&mut self) -> Result<Vec<Expr>, ()> {
        self.expect("(")?;
        let mut args = Vec::new();
        while !self.is(")") {
            args.push(self.expr()?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(args)
    }

    pub(crate) fn expr(&mut self) -> Result<Expr, ()> {
        if let Some(lambda) = self.try_lambda()? {
            return Ok(lambda);
        }
        let target = self.ternary()?;
        let token = self.tok();
        let op = self.src(token);
        if token.kind == Tk::Punct
            && matches!(op, "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" | ">>>=")
        {
            let op = op.to_owned();
            self.pos += 1;
            let value = self.expr()?;
            return Ok(Expr::Assign { op, target: Box::new(target), value: Box::new(value) });
        }
        if token.kind == Tk::Punct && op == ">" && self.is_at(1, ">=") {
            self.pos += 2;
            let value = self.expr()?;
            return Ok(Expr::Assign { op: ">>=".to_owned(), target: Box::new(target), value: Box::new(value) });
        }
        Ok(target)
    }

    fn try_lambda(&mut self) -> Result<Option<Expr>, ()> {
        let save = self.pos;
        let mut params = Vec::new();
        if self.tok().kind == Tk::Ident && self.is_at(1, "->") {
            params.push(self.ident()?);
        } else if self.is("(") {
            self.pos += 1;
            let mut ok = true;
            while !self.is(")") {
                let _ = self.mods();
                let bare = self.tok().kind == Tk::Ident && (self.is_at(1, ",") || self.is_at(1, ")"));
                if bare || (self.ty().is_ok() && self.tok().kind == Tk::Ident) {
                    params.push(self.ident()?);
                } else {
                    ok = false;
                    break;
                }
                if !self.eat(",") {
                    break;
                }
            }
            if !ok || !self.eat(")") || !self.is("->") {
                self.pos = save;
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
        if !self.eat("->") {
            self.pos = save;
            return Ok(None);
        }
        let body =
            if self.is("{") { LambdaBody::Block(self.block()?) } else { LambdaBody::Expr(Box::new(self.expr()?)) };
        Ok(Some(Expr::Lambda { params, body }))
    }

    fn ternary(&mut self) -> Result<Expr, ()> {
        let cond = self.binary(0)?;
        if self.eat("?") {
            let then = self.expr_no_assign()?;
            self.expect(":")?;
            let otherwise = self.expr_no_assign()?;
            return Ok(Expr::Cond { cond: Box::new(cond), then: Box::new(then), otherwise: Box::new(otherwise) });
        }
        Ok(cond)
    }

    fn expr_no_assign(&mut self) -> Result<Expr, ()> {
        if let Some(lambda) = self.try_lambda()? {
            return Ok(lambda);
        }
        self.ternary()
    }

    fn binary_op(&self) -> Option<(String, u8, usize)> {
        let token = self.tok();
        if token.kind != Tk::Punct && !(token.kind == Tk::Ident && self.src(token) == "instanceof") {
            return None;
        }
        let text = self.src(token);
        if text == ">" && self.is_at(1, ">") && self.peek(1).start == token.end {
            if self.is_at(2, ">") && self.peek(2).start == self.peek(1).end {
                return Some((">>>".to_owned(), 7, 3));
            }
            return Some((">>".to_owned(), 7, 2));
        }
        let level = match text {
            "||" => 1,
            "&&" => 2,
            "|" => 3,
            "^" => 4,
            "&" => 5,
            "==" | "!=" => 6,
            "<" | ">" | "<=" | ">=" | "instanceof" => 7,
            "<<" => 8,
            "+" | "-" => 9,
            "*" | "/" | "%" => 10,
            _ => return None,
        };
        Some((text.to_owned(), level, 1))
    }

    fn binary(&mut self, min: u8) -> Result<Expr, ()> {
        let mut left = self.unary()?;
        while let Some((op, level, width)) = self.binary_op() {
            let level = if op == ">>" || op == ">>>" { 8 } else { level };
            if level <= min {
                break;
            }
            self.pos += width;
            if op == "instanceof" {
                let _ = self.eat("final");
                let ty = self.ty()?;
                let binding =
                    if self.tok().kind == Tk::Ident && !self.is("instanceof") { Some(self.ident()?) } else { None };
                left = Expr::InstanceOf { expr: Box::new(left), ty, binding };
                continue;
            }
            let right = self.binary(level)?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ()> {
        let token = self.tok();
        let text = self.src(token);
        if token.kind == Tk::Punct && matches!(text, "+" | "-" | "!" | "~" | "++" | "--") {
            self.pos += 1;
            let expr = self.unary()?;
            return Ok(Expr::Unary { op: text.to_owned(), expr: Box::new(expr), postfix: false });
        }
        if self.is("(")
            && let Some(cast) = self.try_cast()?
        {
            return Ok(cast);
        }
        let mut expr = self.postfix()?;
        while self.is("++") || self.is("--") {
            let op = self.src(self.tok()).to_owned();
            self.pos += 1;
            expr = Expr::Unary { op, expr: Box::new(expr), postfix: true };
        }
        Ok(expr)
    }

    fn try_cast(&mut self) -> Result<Option<Expr>, ()> {
        let save = self.pos;
        self.pos += 1;
        let Ok(ty) = self.ty() else {
            self.pos = save;
            return Ok(None);
        };
        while self.eat("&") {
            if self.ty().is_err() {
                self.pos = save;
                return Ok(None);
            }
        }
        if !self.eat(")") {
            self.pos = save;
            return Ok(None);
        }
        let next = self.tok();
        let next_text = self.src(next);
        let starts_operand = match next.kind {
            Tk::Ident => !matches!(next_text, "instanceof"),
            Tk::Int | Tk::Float | Tk::Char | Tk::Str | Tk::TextBlock => true,
            Tk::Punct => matches!(next_text, "(" | "!" | "~") || (ty.is_primitive() && matches!(next_text, "-" | "+")),
            _ => false,
        };
        if !starts_operand {
            self.pos = save;
            return Ok(None);
        }
        let expr = if let Some(lambda) = self.try_lambda()? { lambda } else { self.unary()? };
        Ok(Some(Expr::Cast { ty, expr: Box::new(expr) }))
    }

    fn postfix(&mut self) -> Result<Expr, ()> {
        let mut expr = self.primary()?;
        loop {
            if self.is(".") {
                self.pos += 1;
                if self.is("<") {
                    let _ = self.type_args()?;
                }
                if self.eat("new") {
                    let ty = self.ty()?;
                    let args = self.args()?;
                    let body = if self.is("{") { Some(self.class_body()?) } else { None };
                    expr = Expr::New { ty, args, body };
                    continue;
                }
                if self.eat("this") {
                    expr = Expr::This;
                    continue;
                }
                if self.eat("class") {
                    expr = Expr::ClassLit(expr_type(&expr).ok_or(())?);
                    continue;
                }
                let name = self.ident()?;
                if self.is("(") {
                    let args = self.args()?;
                    expr = Expr::Call { target: Some(Box::new(expr)), name, args };
                } else {
                    expr = Expr::Field(Box::new(expr), name);
                }
            } else if self.is("[") {
                if self.is_at(1, "]") {
                    let mut ty = expr_type(&expr).ok_or(())?;
                    while self.is("[") && self.is_at(1, "]") {
                        self.pos += 2;
                        ty = Type::Array(Box::new(ty));
                    }
                    if self.eat("::") {
                        let name = if self.eat("new") { "new".to_owned() } else { self.ident()? };
                        expr = Expr::MethodRef { target: Box::new(Expr::TypeRef(ty)), name };
                        continue;
                    }
                    self.expect(".")?;
                    self.expect("class")?;
                    expr = Expr::ClassLit(ty);
                    continue;
                }
                self.pos += 1;
                let index = self.expr()?;
                self.expect("]")?;
                expr = Expr::Index(Box::new(expr), Box::new(index));
            } else if self.is("::") {
                self.pos += 1;
                let name = if self.eat("new") { "new".to_owned() } else { self.ident()? };
                expr = Expr::MethodRef { target: Box::new(expr), name };
            } else if self.is("<") && matches!(expr, Expr::Name(_) | Expr::Field(..)) {
                let save = self.pos;
                if let Ok(args) = self.type_args()
                    && self.is("::")
                {
                    let ty = match expr_type(&expr).ok_or(())? {
                        Type::Named { name, .. } => Type::Named { name, args },
                        other => other,
                    };
                    expr = Expr::TypeRef(ty);
                    continue;
                }
                self.pos = save;
                break;
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, ()> {
        let token = self.tok();
        let text = self.src(token);
        match token.kind {
            Tk::Int => {
                self.pos += 1;
                let lit = if text.ends_with(['l', 'L']) { Lit::Long } else { Lit::Int };
                return Ok(Expr::Literal(lit, text.to_owned()));
            }
            Tk::Float => {
                self.pos += 1;
                let lit = if text.ends_with(['f', 'F']) { Lit::Float } else { Lit::Double };
                return Ok(Expr::Literal(lit, text.to_owned()));
            }
            Tk::Char => {
                self.pos += 1;
                return Ok(Expr::Literal(Lit::Char, text.to_owned()));
            }
            Tk::Str => {
                self.pos += 1;
                return Ok(Expr::Literal(Lit::Str, text.to_owned()));
            }
            Tk::TextBlock => {
                self.pos += 1;
                return Ok(Expr::Literal(Lit::TextBlock, text.to_owned()));
            }
            _ => {}
        }
        if self.is("(") {
            self.pos += 1;
            let inner = self.expr()?;
            self.expect(")")?;
            return Ok(Expr::Paren(Box::new(inner)));
        }
        if token.kind == Tk::Ident {
            match text {
                "true" => {
                    self.pos += 1;
                    return Ok(Expr::Literal(Lit::True, text.to_owned()));
                }
                "false" => {
                    self.pos += 1;
                    return Ok(Expr::Literal(Lit::False, text.to_owned()));
                }
                "null" => {
                    self.pos += 1;
                    return Ok(Expr::Literal(Lit::Null, text.to_owned()));
                }
                "this" => {
                    self.pos += 1;
                    if self.is("(") {
                        return Err(());
                    }
                    return Ok(Expr::This);
                }
                "super" => {
                    self.pos += 1;
                    return Ok(Expr::Super);
                }
                "new" => return self.new_expr(),
                "switch" => {
                    self.pos += 1;
                    let subject = self.paren_expr()?;
                    let cases = self.cases()?;
                    return Ok(Expr::Switch { subject: Box::new(subject), cases });
                }
                _ => {}
            }
            if PRIMITIVES.contains(&text) || text == "void" {
                let mut ty = self.ty()?;
                while self.is("[") && self.is_at(1, "]") {
                    self.pos += 2;
                    ty = Type::Array(Box::new(ty));
                }
                if self.eat("::") {
                    let name = if self.eat("new") { "new".to_owned() } else { self.ident()? };
                    return Ok(Expr::MethodRef { target: Box::new(Expr::TypeRef(ty)), name });
                }
                self.expect(".")?;
                self.expect("class")?;
                return Ok(Expr::ClassLit(ty));
            }
            let name = self.ident()?;
            if self.is("(") {
                let args = self.args()?;
                return Ok(Expr::Call { target: None, name, args });
            }
            return Ok(Expr::Name(name));
        }
        Err(())
    }

    fn new_expr(&mut self) -> Result<Expr, ()> {
        self.pos += 1;
        let _ = self.annotations();
        let mut name = self.ident()?;
        let mut args = self.type_args()?;
        while self.is(".") {
            self.pos += 1;
            name.push('.');
            name.push_str(&self.ident()?);
            let more = self.type_args()?;
            if !more.is_empty() {
                args = more;
            }
        }
        let base = Type::Named { name, args };
        if self.is("[") {
            let mut dims = Vec::new();
            let mut extra = 0;
            while self.eat("[") {
                if self.eat("]") {
                    extra += 1;
                } else {
                    dims.push(self.expr()?);
                    self.expect("]")?;
                }
            }
            let init = if self.is("{") {
                match self.array_init()? {
                    Expr::ArrayInit(items) => Some(items),
                    _ => None,
                }
            } else {
                None
            };
            return Ok(Expr::NewArray { ty: base, dims, extra, init });
        }
        let args = self.args()?;
        let body = if self.is("{") { Some(self.class_body()?) } else { None };
        Ok(Expr::New { ty: base, args, body })
    }
}

fn expr_type(expr: &Expr) -> Option<Type> {
    fn path(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Name(name) => Some(name.clone()),
            Expr::Field(target, name) => Some(format!("{}.{name}", path(target)?)),
            _ => None,
        }
    }
    match expr {
        Expr::TypeRef(ty) => Some(ty.clone()),
        _ => path(expr).map(|name| Type::Named { name, args: Vec::new() }),
    }
}
