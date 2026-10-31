//! The Abstract Syntax Tree for Gleam source code.
//!
//! The tree is owned (no lifetimes) so it can be stored in long-lived
//! structures, serialized, or handed to a type checker / interpreter.

use std::fmt;

/// A byte range into the original source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// A span covering a single token of zero width (e.g. EOF).
    pub fn point(pos: usize) -> Self {
        Self { start: pos, end: pos }
    }

    /// The smallest span containing both `self` and `other`.
    pub fn merge(self, other: Self) -> Self {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// An error produced while lexing or parsing.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
    /// 1-based line number of the offending token (best effort).
    pub line: u32,
    /// 1-based column number of the offending token (best effort).
    pub col: u32,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span, line: u32, col: u32) -> Self {
        Self {
            message: message.into(),
            span,
            line,
            col,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Module level
// ---------------------------------------------------------------------------

/// A parsed Gleam module (one `.gleam` file).
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Documentation comments (`////`) at the top of the module.
    pub docs: Option<String>,
    pub definitions: Vec<Definition>,
}

/// A top level definition in a module.
#[derive(Debug, Clone, PartialEq)]
pub enum Definition {
    Fn(FnDef),
    Type(TypeDef),
    Constant(ConstantDef),
    Import(Import),
}

impl Definition {
    pub fn span(&self) -> Span {
        match self {
            Definition::Fn(d) => d.span,
            Definition::Type(d) => d.span,
            Definition::Constant(d) => d.span,
            Definition::Import(d) => d.span,
        }
    }

    /// Documentation comments (`///`) attached to this definition.
    pub fn docs(&self) -> Option<&str> {
        match self {
            Definition::Fn(d) => d.docs.as_deref(),
            Definition::Type(d) => d.docs.as_deref(),
            Definition::Constant(d) => d.docs.as_deref(),
            Definition::Import(_) => None,
        }
    }
}

/// An attribute such as `@external` or `@deprecated`.
#[derive(Debug, Clone, PartialEq)]
pub enum Attribute {
    External(ExternalFn),
    Deprecated(String),
}

/// The target of an `@external(lang, "module", "function")` attribute.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalFn {
    /// The language the external is implemented in, e.g. `erlang` or
    /// `javascript`.
    pub lang: String,
    /// The module the function lives in on the target runtime.
    pub module: String,
    /// The name of the function on the target runtime.
    pub function: String,
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// A named function definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: String,
    pub args: Vec<FnArg>,
    /// The body is always a block (or a `use` expression built from one).
    pub body: Box<Expression>,
    pub return_annotation: Option<TypeAnnotation>,
    pub public: bool,
    pub docs: Option<String>,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

/// A function argument: an optional external label and the internal name.
#[derive(Debug, Clone, PartialEq)]
pub struct FnArg {
    /// The external label, e.g. `add` in `fn f(add x: Int)`.
    pub label: Option<String>,
    /// The internal name used in the function body.
    pub name: String,
    pub type_annotation: Option<TypeAnnotation>,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A type definition: a record (custom type), an alias, or an external type.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    /// Type parameters, e.g. `a` and `b` in `type Pair(a, b)`.
    pub params: Vec<String>,
    pub kind: TypeDefKind,
    pub public: bool,
    /// `true` for `pub opaque type ...`: the type is public but its
    /// constructors are private.
    pub opaque: bool,
    pub docs: Option<String>,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeDefKind {
    /// A custom type with one or more record variants.
    Record(Vec<RecordConstructor>),
    /// A type alias, e.g. `type Number = Int`.
    Alias(TypeAnnotation),
    /// An external type with no Gleam constructors, e.g. `pub type DateTime`.
    External,
}

/// One variant of a custom type.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordConstructor {
    pub name: String,
    pub fields: Vec<RecordField>,
}

/// A field in a record variant.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    /// The external label, e.g. `name` in `Person(name: String)`.
    pub label: Option<String>,
    /// The internal name used when accessing the field; `None` for unnamed
    /// fields, e.g. `Person(String, Int)`.
    pub name: Option<String>,
    pub type_annotation: TypeAnnotation,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// A module level constant definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantDef {
    pub name: String,
    pub value: ConstValue,
    pub annotation: Option<TypeAnnotation>,
    pub public: bool,
    pub docs: Option<String>,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

/// The value of a constant: a restricted set of literals which may reference
/// other constants and (nullary/const-arg) record constructors.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(IntValue),
    Float(FloatValue),
    String(String),
    Bool(bool),
    Nil,
    /// A reference to another constant in the same module.
    Var(String),
    Constructor(ConstructorRef),
    List {
        elements: Vec<ConstValue>,
        /// `..` spread of another constant list.
        spread: Option<Box<ConstValue>>,
    },
    Tuple(Vec<ConstValue>),
    BitArray(BitArray<ConstValue>),
}

/// A labelled (or unlabelled) argument to a constructor in a const value.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstArg {
    pub label: Option<String>,
    pub value: Box<ConstValue>,
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

/// An `import` statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    /// The module path segments, e.g. `["gleam", "io"]`.
    pub module: Vec<String>,
    /// The alias from `as`, if any.
    pub alias: Option<String>,
    /// Unqualified imports in `{ ... }`, if any.
    pub unqualified: Vec<UnqualifiedImport>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnqualifiedImport {
    /// A function or constructor imported without the `type` keyword.
    Value(String),
    /// A type imported with the `type` keyword.
    Type(String),
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// An expression together with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub expr: Expr,
    pub span: Span,
}

impl Expression {
    /// Wrap an expression in a zero-width span (used for synthetic nodes).
    pub fn synthetic(expr: Expr) -> Self {
        Self {
            expr,
            span: Span::point(0),
        }
    }

    /// If this is a block, return its statements.
    pub fn as_block(&self) -> Option<&Vec<Expression>> {
        match &self.expr {
            Expr::Block(b) => Some(b),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // -- Literals ----------------------------------------------------------
    Int(IntValue),
    Float(FloatValue),
    String(String),
    Bool(bool),
    /// The `Nil` value.
    Nil,

    // -- References --------------------------------------------------------
    /// A variable, e.g. `x`.
    Var(String),
    /// Field access with the `.` syntax: `person.name`, `tuple.0`.
    ///
    /// Module qualified references (`io.println`) and qualified
    /// constructors (`option.Some`) are also parsed as field access over a
    /// variable; disambiguation happens during type checking, exactly as in
    /// the Gleam compiler.
    FieldAccess {
        base: Box<Expression>,
        label: String,
    },
    /// A record constructor used as a value without calling it, e.g. a bare
    /// `Spring` or `option.None`.
    Constructor(ConstructorRef),

    // -- Compound values ---------------------------------------------------
    List {
        elements: Vec<Expression>,
        /// `..rest` spread, if present (always the last element).
        spread: Option<Box<Expression>>,
    },
    Tuple(Vec<Expression>),
    BitArray(BitArray<Expression>),

    // -- Functions and calls ----------------------------------------------
    Call(Call),
    /// An anonymous function literal: `fn(a, b) { ... }`.
    Fn(FnLiteral),
    /// A function capture: `add(1, _)` is shorthand for `fn(x) { add(1, x) }`.
    Capture(Capture),
    /// A record update: `Teacher(..teacher, subject: "PE")`.
    RecordUpdate(RecordUpdate),

    // -- Control flow ------------------------------------------------------
    Case(Case),
    Block(Vec<Expression>),

    // -- Assignment & assertions -------------------------------------------
    Let(Let),
    /// `let assert [x] = xs as "message"`
    LetAssert(LetAssert),
    /// A boolean assertion: `assert x > 3 as "message"`.
    Assert(Assert),
    /// `use x <- f(a)` followed by the rest of the block.
    Use(Use),

    // -- Misc ---------------------------------------------------------------
    /// The `echo` debug print expression.
    Echo(Box<Expression>),
    /// `todo` with an optional message: `todo as "not done yet"`.
    Todo {
        message: Option<String>,
    },
    /// `panic` with an optional message: `panic as "impossible!"`.
    Panic {
        message: Option<String>,
    },

    // -- Operators ----------------------------------------------------------
    /// A binary operator expression.
    Binary {
        op: BinOp,
        lhs: Box<Expression>,
        rhs: Box<Expression>,
    },
    /// Unary minus: `-x`.
    Neg(Box<Expression>),
    /// Boolean negation: `!x`.
    Not(Box<Expression>),

    // -- Pipelines ----------------------------------------------------------
    /// A pipeline: the first expression piped through the rest.
    Pipeline {
        exprs: Vec<Expression>,
        /// The span of each `|>` operator (one fewer than `exprs`).
        pipe_spans: Vec<Span>,
    },
}

/// An unqualified record constructor reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructorRef {
    /// The module the constructor is qualified with, if any.
    pub module: Option<String>,
    pub name: String,
}

/// A function call.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub fun: Box<Expression>,
    pub args: Vec<CallArg>,
    /// Span of the called function expression.
    pub fun_span: Span,
}

/// An argument to a function call.
#[derive(Debug, Clone, PartialEq)]
pub enum CallArg {
    Value(Expression),
    Labelled {
        label: String,
        value: Expression,
    },
}

/// An anonymous function literal.
#[derive(Debug, Clone, PartialEq)]
pub struct FnLiteral {
    /// Parameter names (no type annotations are allowed on fn literals).
    pub args: Vec<String>,
    pub body: Box<Expression>,
}

/// A function capture like `add(1, _)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    /// The captured function: a variable or module qualified name.
    pub fun: Box<Expression>,
    /// The arguments in source order; `None` marks the `_` placeholder.
    pub args: Vec<CaptureArg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureArg {
    Value(Expression),
    /// The `_` placeholder.
    Placeholder,
}

/// A record update expression: `Constructor(..base, label: value, ...)`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordUpdate {
    pub constructor: ConstructorRef,
    /// The existing record to copy fields from.
    pub base: Box<Expression>,
    /// The labelled fields to change (always labelled).
    pub args: Vec<CallArg>,
}

// ---------------------------------------------------------------------------
// Case expressions
// ---------------------------------------------------------------------------

/// A `case` expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub subjects: Vec<Expression>,
    pub clauses: Vec<Clause>,
}

/// One clause of a case expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Clause {
    /// One pattern per subject.
    pub patterns: Vec<Pattern>,
    pub guard: Option<Guard>,
    pub expression: Expression,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Assignment & assertions
// ---------------------------------------------------------------------------

/// A `let` assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Let {
    pub pattern: Pattern,
    pub value: Box<Expression>,
    pub annotation: Option<TypeAnnotation>,
}

/// A `let assert` assignment which panics if the pattern does not match.
#[derive(Debug, Clone, PartialEq)]
pub struct LetAssert {
    pub pattern: Pattern,
    pub value: Box<Expression>,
    /// The message from `as "..."`, if any.
    pub message: Option<String>,
}

/// A boolean assertion: `assert expr as "..."`.
#[derive(Debug, Clone, PartialEq)]
pub struct Assert {
    pub value: Box<Expression>,
    pub message: Option<String>,
}

/// A `use` expression: `use x, y <- call` with the rest of the block as body.
#[derive(Debug, Clone, PartialEq)]
pub struct Use {
    /// The patterns bound by the callback (one per argument).
    pub patterns: Vec<Pattern>,
    /// The expression on the right hand side of `<-`.
    pub call: Box<Expression>,
    /// The remainder of the enclosing block.
    pub body: Box<Expression>,
}

// ---------------------------------------------------------------------------
// Bit arrays
// ---------------------------------------------------------------------------

/// A bit array literal or pattern, generic over the segment value type.
#[derive(Debug, Clone, PartialEq)]
pub struct BitArray<T> {
    pub segments: Vec<BitArraySegment<T>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BitArraySegment<T> {
    pub value: T,
    pub options: Vec<BitOption>,
}

/// An option attached to a bit array segment, e.g. `x:utf8` or
/// `x:size(16)-big`.
#[derive(Debug, Clone, PartialEq)]
pub enum BitOption {
    Bits,
    Bytes,
    Float,
    Int,
    Big,
    Little,
    Native,
    Utf8,
    Utf16,
    Utf32,
    Utf8Codepoint,
    Utf16Codepoint,
    Utf32Codepoint,
    Signed,
    Unsigned,
    Size(Expression),
    Unit(Expression),
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// A pattern used in `let`, `case` clauses and `use`.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// A variable binding.
    Var(String),
    /// A discard: `_` or `_name`.
    Discard(String),

    // Literals
    Int(IntValue),
    Float(FloatValue),
    String(String),
    Bool(bool),
    Nil,

    /// A string prefix pattern: `"Hello, " <> name`.
    StringPrefix {
        prefix: String,
        rest: Box<Pattern>,
    },

    /// A record constructor pattern: `Person(name: "x", age: 10)`.
    Constructor(ConstructorRef, Vec<PatternArg>),

    List {
        elements: Vec<Pattern>,
        /// The `..` rest of the list (a named variable or a bare discard).
        rest: Option<Box<Pattern>>,
    },
    Tuple(Vec<Pattern>),
    BitArray(BitArray<Pattern>),

    /// A pattern alias: `[1, ..] as first`.
    Alias {
        pattern: Box<Pattern>,
        name: String,
    },

    /// Alternative patterns: `2 | 4 | 6`. Only allowed at the top level of a
    /// clause pattern.
    Alternative(Vec<Pattern>),
}

/// An argument to a constructor pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternArg {
    Named {
        label: String,
        pattern: Box<Pattern>,
    },
    Unnamed(Box<Pattern>),
    /// The `..` which discards the remaining fields (must be last).
    DiscardRest,
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

/// A guard expression after `if` in a case clause.
///
/// Guards are flat and left associative: `a == b && c > d` is parsed as
/// `((a == b) && (c > d))`, matching the Gleam compiler.
#[derive(Debug, Clone, PartialEq)]
pub enum Guard {
    Int(IntValue),
    Float(FloatValue),
    String(String),
    Bool(bool),
    Nil,
    Var(String),
    /// Field access in a guard: `x.name`.
    FieldAccess {
        base: Box<Guard>,
        label: String,
    },
    /// A function call in a guard, e.g. `list.length(x)`.
    Call {
        fun: Box<Guard>,
        args: Vec<Guard>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Guard>,
        rhs: Box<Guard>,
    },
}

// ---------------------------------------------------------------------------
// Types (annotations)
// ---------------------------------------------------------------------------

/// A type annotation, e.g. `Int`, `List(Int)`, `fn(Int) -> String`,
/// `#(Int, Bool)` or `BitArray(8, 8)`.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    /// A named type (or a type variable when the name is lowercase).
    Named {
        /// Module qualifier for qualified types, e.g. `bytes_tree.BytesTree`.
        module: Option<String>,
        name: String,
        args: Vec<TypeAnnotation>,
    },
    /// A function type: `fn(Int, String) -> Bool`.
    Fn {
        args: Vec<TypeAnnotation>,
        ret: Box<TypeAnnotation>,
    },
    /// A tuple type: `#(Int, String)`.
    Tuple(Vec<TypeAnnotation>),
    /// A bit array type with optional size and unit (in bits).
    BitArray {
        size: Option<IntValue>,
        unit: Option<IntValue>,
    },
}

// ---------------------------------------------------------------------------
// Numbers & operators
// ---------------------------------------------------------------------------

/// An integer literal. The raw source text is preserved so that radix
/// prefixes (`0x`, `0b`, `0o`) and underscores survive round-tripping.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IntValue {
    /// The literal as written in the source, e.g. `"0xF"`, `"1_000"`.
    pub raw: String,
}

impl IntValue {
    /// Parse the literal into a `u64`, if it fits.
    pub fn as_u64(&self) -> Option<u64> {
        let cleaned: String = self.raw.chars().filter(|c| *c != '_').collect();
        let value = cleaned.strip_prefix("0x").or_else(|| {
            cleaned
                .strip_prefix("0X")
                .or_else(|| cleaned.strip_prefix("0b"))
                .or_else(|| cleaned.strip_prefix("0B"))
                .or_else(|| cleaned.strip_prefix("0o"))
                .or_else(|| cleaned.strip_prefix("0O"))
        });
        match value {
            Some(hex_or_others) if self.raw.to_lowercase().starts_with("0x") => {
                u64::from_str_radix(hex_or_others, 16).ok()
            }
            Some(rest) if self.raw.starts_with("0b") || self.raw.starts_with("0B") => {
                u64::from_str_radix(rest, 2).ok()
            }
            Some(rest) if self.raw.starts_with("0o") || self.raw.starts_with("0O") => {
                u64::from_str_radix(rest, 8).ok()
            }
            _ => cleaned.parse::<u64>().ok(),
        }
    }
}

/// A float literal.
#[derive(Debug, Clone, PartialEq)]
pub struct FloatValue {
    pub value: f64,
    /// The literal as written in the source.
    pub raw: String,
}

/// Binary operators shared by expressions and guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    // Bool
    And,
    Or,
    // Equality
    Eq,
    NotEq,
    // Comparisons (int)
    Lt,
    Gt,
    LtEq,
    GtEq,
    // Comparisons (float)
    LtFloat,
    GtFloat,
    LtEqFloat,
    GtEqFloat,
    // String
    Concat,
    // Int arithmetic
    Plus,
    Minus,
    Times,
    Slash,
    Percent,
    // Float arithmetic
    PlusFloat,
    MinusFloat,
    TimesFloat,
    SlashFloat,
}

impl BinOp {
    /// The source text of the operator.
    pub fn as_str(&self) -> &'static str {
        match self {
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::LtEq => "<=",
            BinOp::GtEq => ">=",
            BinOp::LtFloat => "<.",
            BinOp::GtFloat => ">.",
            BinOp::LtEqFloat => "<=.",
            BinOp::GtEqFloat => ">=.",
            BinOp::Concat => "<>",
            BinOp::Plus => "+",
            BinOp::Minus => "-",
            BinOp::Times => "*",
            BinOp::Slash => "/",
            BinOp::Percent => "%",
            BinOp::PlusFloat => "+.",
            BinOp::MinusFloat => "-.",
            BinOp::TimesFloat => "*.",
            BinOp::SlashFloat => "/.",
        }
    }
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
