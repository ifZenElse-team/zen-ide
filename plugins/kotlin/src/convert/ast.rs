#[derive(Clone, Debug, Default)]
pub(crate) struct Unit {
    pub(crate) package: Option<String>,
    pub(crate) imports: Vec<Import>,
    pub(crate) types: Vec<TypeDecl>,
    pub(crate) trailing: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Import {
    pub(crate) path: String,
    pub(crate) is_static: bool,
    pub(crate) wildcard: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Mods {
    pub(crate) words: Vec<String>,
    pub(crate) annotations: Vec<String>,
}

impl Mods {
    pub(crate) fn has(&self, word: &str) -> bool {
        self.words.iter().any(|known| known == word)
    }

    pub(crate) fn has_annotation(&self, name: &str) -> bool {
        self.annotations.iter().any(|annotation| annotation == name || annotation.starts_with(&format!("{name}(")))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
}

#[derive(Clone, Debug)]
pub(crate) struct TypeDecl {
    pub(crate) comments: Vec<usize>,
    pub(crate) mods: Mods,
    pub(crate) kind: TypeKind,
    pub(crate) name: String,
    pub(crate) params: Vec<TypeParam>,
    pub(crate) extends: Vec<Type>,
    pub(crate) implements: Vec<Type>,
    pub(crate) components: Vec<Param>,
    pub(crate) constants: Vec<EnumConstant>,
    pub(crate) members: Vec<Member>,
}

#[derive(Clone, Debug)]
pub(crate) struct TypeParam {
    pub(crate) name: String,
    pub(crate) bounds: Vec<Type>,
}

#[derive(Clone, Debug)]
pub(crate) struct EnumConstant {
    pub(crate) comments: Vec<usize>,
    pub(crate) name: String,
    pub(crate) args: Vec<Expr>,
    pub(crate) body: Option<Vec<Member>>,
}

#[derive(Clone, Debug)]
pub(crate) enum Member {
    Field { comments: Vec<usize>, mods: Mods, ty: Type, vars: Vec<Var> },
    Method(Box<Method>),
    Init { comments: Vec<usize>, is_static: bool, body: Block },
    Type(Box<TypeDecl>),
    Raw { comments: Vec<usize>, start: usize, end: usize },
}

#[derive(Clone, Debug)]
pub(crate) struct Method {
    pub(crate) comments: Vec<usize>,
    pub(crate) mods: Mods,
    pub(crate) params_of_type: Vec<TypeParam>,
    pub(crate) result: Option<Type>,
    pub(crate) name: String,
    pub(crate) params: Vec<Param>,
    pub(crate) body: Option<Block>,
}

#[derive(Clone, Debug)]
pub(crate) struct Param {
    pub(crate) ty: Type,
    pub(crate) name: String,
    pub(crate) varargs: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Var {
    pub(crate) name: String,
    pub(crate) dims: usize,
    pub(crate) init: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Type {
    Named { name: String, args: Vec<TypeArg> },
    Array(Box<Type>),
    Var,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TypeArg {
    Type(Type),
    Any,
    Extends(Type),
    Super(Type),
}

impl Type {
    pub(crate) fn named(name: &str) -> Type {
        Type::Named { name: name.to_owned(), args: Vec::new() }
    }

    pub(crate) fn simple_name(&self) -> Option<&str> {
        match self {
            Type::Named { name, .. } => Some(name.rsplit('.').next().unwrap_or(name)),
            _ => None,
        }
    }

    pub(crate) fn is_primitive(&self) -> bool {
        matches!(self, Type::Named { name, .. } if PRIMITIVES.contains(&name.as_str()))
    }

    pub(crate) fn is_array(&self) -> bool {
        matches!(self, Type::Array(_))
    }
}

pub(crate) const PRIMITIVES: &[&str] = &["int", "long", "short", "byte", "char", "boolean", "float", "double"];

#[derive(Clone, Debug, Default)]
pub(crate) struct Block {
    pub(crate) stmts: Vec<Stmt>,
    pub(crate) end_comments: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Stmt {
    pub(crate) comments: Vec<usize>,
    pub(crate) kind: StmtKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum StmtKind {
    Block(Block),
    Local { is_final: bool, ty: Type, vars: Vec<Var> },
    LocalType(Box<TypeDecl>),
    Expr(Expr),
    If { cond: Expr, then: Box<Stmt>, otherwise: Option<Box<Stmt>> },
    While { cond: Expr, body: Box<Stmt> },
    DoWhile { body: Box<Stmt>, cond: Expr },
    For { init: Vec<Stmt>, cond: Option<Expr>, update: Vec<Expr>, body: Box<Stmt> },
    ForEach { ty: Type, name: String, iterable: Expr, body: Box<Stmt> },
    Return(Option<Expr>),
    Throw(Expr),
    Break(Option<String>),
    Continue(Option<String>),
    Yield(Expr),
    Try { resources: Vec<Resource>, body: Block, catches: Vec<Catch>, finally: Option<Block> },
    Switch { subject: Expr, cases: Vec<Case> },
    Labeled { label: String, body: Box<Stmt> },
    Sync { lock: Expr, body: Block },
    Assert { cond: Expr, message: Option<Expr> },
    ThisCall { is_super: bool, args: Vec<Expr> },
    Empty,
    Raw,
}

#[derive(Clone, Debug)]
pub(crate) struct Resource {
    pub(crate) name: Option<String>,
    pub(crate) value: Expr,
}

#[derive(Clone, Debug)]
pub(crate) struct Catch {
    pub(crate) types: Vec<Type>,
    pub(crate) name: String,
    pub(crate) body: Block,
}

#[derive(Clone, Debug)]
pub(crate) struct Case {
    pub(crate) labels: Vec<Expr>,
    pub(crate) is_default: bool,
    pub(crate) arrow: bool,
    pub(crate) body: Vec<Stmt>,
    pub(crate) start: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Literal(Lit, String),
    Name(String),
    Field(Box<Expr>, String),
    Call { target: Option<Box<Expr>>, name: String, args: Vec<Expr> },
    New { ty: Type, args: Vec<Expr>, body: Option<Vec<Member>> },
    NewArray { ty: Type, dims: Vec<Expr>, extra: usize, init: Option<Vec<Expr>> },
    ArrayInit(Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Unary { op: String, expr: Box<Expr>, postfix: bool },
    Binary { op: String, left: Box<Expr>, right: Box<Expr> },
    Assign { op: String, target: Box<Expr>, value: Box<Expr> },
    Cond { cond: Box<Expr>, then: Box<Expr>, otherwise: Box<Expr> },
    InstanceOf { expr: Box<Expr>, ty: Type, binding: Option<String> },
    Cast { ty: Type, expr: Box<Expr> },
    Lambda { params: Vec<String>, body: LambdaBody },
    MethodRef { target: Box<Expr>, name: String },
    TypeRef(Type),
    This,
    Super,
    ClassLit(Type),
    Paren(Box<Expr>),
    Switch { subject: Box<Expr>, cases: Vec<Case> },
}

#[derive(Clone, Debug)]
pub(crate) enum LambdaBody {
    Expr(Box<Expr>),
    Block(Block),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lit {
    Int,
    Long,
    Float,
    Double,
    Char,
    Str,
    TextBlock,
    True,
    False,
    Null,
}
