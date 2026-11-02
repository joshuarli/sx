use sx_kernel::syntax::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeAnn {
    Primitive(&'static str),
    Literal(String),
    List(Box<TypeAnn>),
    Tuple(Vec<TypeAnn>),
    Map(Box<TypeAnn>),
    Object(Vec<(String, bool, TypeAnn)>),
    Union(Vec<TypeAnn>),
    Name(String),
    Promise(Box<TypeAnn>),
    Unknown,
    Any,
    Void,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatternProp {
    pub key: String,
    pub value: Pattern,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatternKind {
    Ident(String),
    Object(Vec<PatternProp>),
    Array(Vec<Pattern>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    Int(i64),
    Float(f64),
}


#[derive(Clone, Debug, PartialEq)]
pub enum PropKey {
    Name(String),
    Str(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectItem {
    Field { key: String, key_span: Span, value: Expr },
    Spread(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArrayItem {
    Item(Expr),
    Spread(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Access {
    Field(String),
    Index(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    Access { access: Access, optional: bool, span: Span },
    Call { args: Vec<Expr>, optional: bool, span: Span },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
    Plus,
    TypeOf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    StrictEq,
    StrictNe,
    LooseEqNull,
    LooseNeNull,
    And,
    Or,
    Coalesce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: Option<Ident>,
    pub params: Vec<Pattern>,
    pub body: FunctionBody,
    pub is_async: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FunctionBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Number(Number),
    Str(String),
    Template(Vec<String>, Vec<Expr>),
    Bool(bool),
    Null,
    Ident(String),
    Array(Vec<ArrayItem>),
    Object(Vec<ObjectItem>),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Chain(Box<Expr>, Vec<Link>),
    Call(Box<Expr>, Vec<Expr>),
    New(String, Vec<Expr>),
    Await(Box<Expr>),
    Arrow(Box<Function>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Assign(AssignOp, Box<Expr>, Box<Expr>),
    Update { increment: bool, target: Box<Expr> },
    Cast(Box<Expr>, TypeAnn),
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    Const,
    Let,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Declarator {
    pub pattern: Pattern,
    pub annotation: Option<TypeAnn>,
    pub init: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Decl(DeclKind, Vec<Declarator>),
    Expr(Expr),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    ForOf { pattern: Pattern, source: Expr, body: Box<Stmt> },
    For { init: Option<Box<Stmt>>, test: Option<Expr>, update: Option<Expr>, body: Box<Stmt> },
    Try { block: Vec<Stmt>, param: Option<Pattern>, handler: Vec<Stmt> },
    Throw(Expr),
    Return(Option<Expr>),
    Break,
    Continue,
    Block(Vec<Stmt>),
    Function(Function),
    TypeDecl(String, TypeAnn),
    Empty,
    Rejected,
}
