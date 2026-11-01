//! A recursive descent parser for Gleam source code.
//!
//! The grammar covered here follows the Gleam language tour:
//!
//! ```text
//! module      := {doc-comment} definition*
//! definition  := import | const | fn | type
//! expression  := let-assign | assert | pipeline
//! pipeline    := binary ('|>' binary)*
//! binary      := operators with precedence (|| < && < ==/!= < comparisons
//!                 < <> < + - < * / %)
//! primary     := literal | var | constructor | call | block | case | fn
//!                 | list | tuple | bit-array | echo | todo | panic
//! pattern     := alternative ('|' alternative)*
//!             := base ('as' var)?
//!             := "prefix" <> var
//! ```

use crate::ast::*;
use crate::lexer::{Token, TokenKind};

/// Parse a whole Gleam module.
pub fn parse_module(tokens: Vec<Token>) -> Result<Module, ParseError> {
    let mut p = Parser::new(tokens);
    p.parse_module()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

/// A call argument as parsed from the token stream. `spread_base` is set
/// when the first argument was `..expr` (a record update).
struct RawArgs {
    spread_base: Option<Expression>,
    args: Vec<RawArg>,
}

enum RawArg {
    Value(Expression),
    Labelled {
        label: String,
        value: Expression,
    },
    /// The `_` placeholder of a function capture.
    Placeholder,
    /// A labelled `_` placeholder, e.g. `f(a: _)`.
    #[allow(dead_code)]
    LabelledPlaceholder(String),
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ------------------------------------------------------------------
    // Token stream helpers
    // ------------------------------------------------------------------

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        self.tokens
            .get(self.pos + offset)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::EndOfFile)
    }

    fn current_token(&self) -> &Token {
        &self.tokens[self.pos]
    }

    /// The span of the next token (used to start expression spans).
    fn peek_span(&self) -> Span {
        self.current_token().span
    }

    fn consume(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    /// Consume a `Variable` token, returning its name.
    fn expect_variable(&mut self, what: &str) -> Result<String, ParseError> {
        match self.peek().clone() {
            TokenKind::Variable(name) => {
                self.consume();
                Ok(name)
            }
            _ => Err(self.unexpected(what)),
        }
    }

    /// Consume a `String` token, returning its value.
    fn expect_string_lit(&mut self, what: &str) -> Result<String, ParseError> {
        match self.peek().clone() {
            TokenKind::String(s) => {
                self.consume();
                Ok(s)
            }
            _ => Err(self.unexpected(what)),
        }
    }

    /// Consume a `TypeName` token, returning its name.
    fn expect_type_name(&mut self, what: &str) -> Result<String, ParseError> {
        match self.peek().clone() {
            TokenKind::TypeName(name) => {
                self.consume();
                Ok(name)
            }
            _ => Err(self.unexpected(what)),
        }
    }

    /// Consume the next token if it matches `kind`.
    fn skip(&mut self, kind: &TokenKind) -> bool {
        if self.peek() == kind {
            self.consume();
            true
        } else {
            false
        }
    }

    /// Consume the next token if it is a `Variable` (any name).
    fn skip_variable(&mut self) -> Option<String> {
        if let TokenKind::Variable(name) = self.peek().clone() {
            self.consume();
            Some(name)
        } else {
            None
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token, ParseError> {
        if self.peek() == kind {
            Ok(self.consume())
        } else {
            Err(self.unexpected(what))
        }
    }

    /// An error describing the next token as unexpected.
    fn unexpected(&self, what: &str) -> ParseError {
        let tok = self.current_token();
        ParseError::new(
            format!("expected {what}, found {}", tok.kind.describe()),
            tok.span,
            tok.line,
            tok.col,
        )
    }

    fn error_at(&self, tok: &Token, message: impl Into<String>) -> ParseError {
        ParseError::new(message, tok.span, tok.line, tok.col)
    }

    /// Wrap a parsed expression with the span from `start` to now.
    fn expr(&self, start: Span, e: Expr) -> Expression {
        let end = self.current_token().span.start;
        Expression {
            expr: e,
            span: start.merge(Span::point(end)),
        }
    }

    // ------------------------------------------------------------------
    // Module
    // ------------------------------------------------------------------

    fn parse_module(&mut self) -> Result<Module, ParseError> {
        let mut docs: Vec<String> = Vec::new();
        while let TokenKind::ModuleDocComment(text) = self.peek().clone() {
            self.consume();
            docs.push(text);
        }

        let mut definitions = Vec::new();
        while self.peek() != &TokenKind::EndOfFile {
            // Collect doc comments for the next definition.
            let mut def_docs: Vec<String> = Vec::new();
            while let TokenKind::DocComment(text) = self.peek().clone() {
                self.consume();
                def_docs.push(text);
            }
            // Module docs may also appear further down in the file.
            while let TokenKind::ModuleDocComment(text) = self.peek().clone() {
                self.consume();
                docs.push(text);
            }
            definitions.push(self.parse_definition(&def_docs)?);
        }

        Ok(Module {
            docs: join_docs(docs),
            definitions,
        })
    }

    fn parse_definition(&mut self, docs: &[String]) -> Result<Definition, ParseError> {

        // Attributes.
        let mut attributes = Vec::new();
        while matches!(self.peek(), TokenKind::External | TokenKind::Deprecated) {
            attributes.push(self.parse_attribute()?);
        }

        let defn = match self.peek().clone() {
            TokenKind::Import => {
                let import = self.parse_import()?;
                Definition::Import(import)
            }
            TokenKind::Pub => {
                self.consume();
                match self.peek().clone() {
                    TokenKind::Opaque => {
                        self.consume();
                        let ty = self.parse_type(true, true)?;
                        Definition::Type(ty)
                    }
                    TokenKind::Type => {
                        let ty = self.parse_type(true, false)?;
                        Definition::Type(ty)
                    }
                    TokenKind::Fn => {
                        let f = self.parse_fn(true)?;
                        Definition::Fn(f)
                    }
                    TokenKind::Const => {
                        let c = self.parse_const(true)?;
                        Definition::Constant(c)
                    }
                    other => return Err(self.error_at(
                        &self.current_token(),
                        format!("expected `type`, `fn` or `const` after `pub`, found {}", other.describe()),
                    )),
                }
            }
            TokenKind::Type => {
                let ty = self.parse_type(false, false)?;
                Definition::Type(ty)
            }
            TokenKind::Fn => {
                let f = self.parse_fn(false)?;
                Definition::Fn(f)
            }
            TokenKind::Const => {
                let c = self.parse_const(false)?;
                Definition::Constant(c)
            }
            other => return Err(self.error_at(
                &self.current_token(),
                format!(
                    "expected a definition (`import`, `const`, `fn` or `type`), found {}",
                    other.describe()
                ),
            )),
        };

        Ok(with_docs(attach_attributes(defn, attributes), docs))
    }

    fn parse_attribute(&mut self) -> Result<Attribute, ParseError> {
        match self.peek().clone() {
            TokenKind::External => {
                self.consume();
                self.expect(&TokenKind::OpenParen, "`(` after `@external`")?;
                let lang = self.expect_variable("the language name in `@external`")?;
                self.expect(&TokenKind::Comma, "`,` in `@external`")?;
                let module = self.expect_string_lit("the module string in `@external`")?;
                self.expect(&TokenKind::Comma, "`,` in `@external`")?;
                let function = self.expect_string_lit("the function string in `@external`")?;
                self.expect(&TokenKind::CloseParen, "`)` after `@external`")?;
                Ok(Attribute::External(ExternalFn {
                    lang,
                    module,
                    function,
                }))
            }
            TokenKind::Deprecated => {
                self.consume();
                self.expect(&TokenKind::OpenParen, "`(` after `@deprecated`")?;
                let message = self.expect_string_lit("the message string in `@deprecated`")?;
                self.expect(&TokenKind::CloseParen, "`)` after `@deprecated`")?;
                Ok(Attribute::Deprecated(message))
            }
            _ => unreachable!(),
        }
    }

    // ------------------------------------------------------------------
    // Imports
    // ------------------------------------------------------------------

    fn parse_import(&mut self) -> Result<Import, ParseError> {
        let start = self.peek_span();
        self.consume(); // `import`

        let first = self.expect_variable("the module name in `import`")?;
        let mut module = vec![first];

        // More path segments: `/name` (modern) or `.name` (legacy).
        while (self.peek() == &TokenKind::Slash || self.peek() == &TokenKind::Dot)
            && matches!(self.peek_at(1), TokenKind::Variable(_))
        {
            self.consume(); // `/` or `.`
            module.push(self.expect_variable("a module name segment")?);
        }

        // Optional alias.
        let mut alias = None;
        if self.skip(&TokenKind::As) {
            alias = Some(self.expect_variable("the module alias after `as`")?);
        }

        // Optional unqualified import list.
        let mut unqualified = Vec::new();
        if self.skip(&TokenKind::OpenBlock) {
            loop {
                if self.peek() == &TokenKind::Type {
                    self.consume();
                    unqualified.push(UnqualifiedImport::Type(
                        self.expect_type_name("a type name in the import list")?,
                    ));
                } else if matches!(self.peek(), TokenKind::Variable(_) | TokenKind::TypeName(_)) {
                    let name = self.consume().kind;
                    unqualified.push(UnqualifiedImport::Value(match name {
                        TokenKind::Variable(n) | TokenKind::TypeName(n) => n,
                        _ => unreachable!(),
                    }));
                } else if self.peek() == &TokenKind::CloseBlock {
                    break;
                } else {
                    return Err(self.unexpected("a function or `type Type` name in the import list"));
                }
                if self.skip(&TokenKind::Comma) {
                    continue;
                }
                self.expect(&TokenKind::CloseBlock, "`,` or `}` in the import list")?;
                break;
            }
        }

        Ok(Import {
            module,
            alias,
            unqualified,
            span: start.merge(self.current_token().span),
        })
    }

    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    fn parse_type(&mut self, public: bool, opaque: bool) -> Result<TypeDef, ParseError> {
        let start = self.peek_span();
        self.consume(); // `type`

        let name = self.expect_type_name("the type name")?;

        // Type parameters.
        let mut params = Vec::new();
        if self.skip(&TokenKind::OpenParen) {
            loop {
                if self.peek() == &TokenKind::CloseParen {
                    break;
                }
                params.push(self.expect_variable("a type parameter name")?);
                if !self.skip(&TokenKind::Comma) {
                    break; // `)` is consumed below.
                }
            }
            self.consume(); // `)`
        }

        // Body: alias, record variants, or external (no body).
        let kind = if self.skip(&TokenKind::Assign) {
            TypeDefKind::Alias(self.parse_type_annotation()?)
        } else if self.peek() == &TokenKind::OpenBlock {
            self.consume();
            let mut variants = Vec::new();
            while self.peek() != &TokenKind::CloseBlock {
                if self.peek() == &TokenKind::EndOfFile {
                    return Err(self.unexpected("a variant name or `}` in the type definition"));
                }
                let vname = self.expect_type_name("a variant name")?;
                let fields = if self.skip(&TokenKind::OpenParen) {
                    let mut fields = Vec::new();
                    loop {
                        if self.peek() == &TokenKind::CloseParen {
                            self.consume();
                            break;
                        }
                        // A field is `name: type`, `label name: type` or an
                        // unnamed bare type, e.g. `Person(String, Int)`.
                        if matches!(self.peek(), TokenKind::Variable(_))
                            && self.peek_at(1) == &TokenKind::Colon
                        {
                            let first = self.expect_variable("a field name")?;
                            self.consume(); // `:`
                            let ty = self.parse_type_annotation()?;
                            fields.push(RecordField {
                                label: None,
                                name: Some(first),
                                type_annotation: ty,
                            });
                        } else if matches!(self.peek(), TokenKind::Variable(_))
                            && matches!(self.peek_at(1), TokenKind::Variable(_))
                            && self.peek_at(2) == &TokenKind::Colon
                        {
                            let label = self.expect_variable("a field label")?;
                            let second = self.expect_variable("the field name after the label")?;
                            self.consume(); // `:`
                            let ty = self.parse_type_annotation()?;
                            fields.push(RecordField {
                                label: Some(label),
                                name: Some(second),
                                type_annotation: ty,
                            });
                        } else {
                            // Unnamed field: just a type.
                            let ty = self.parse_type_annotation()?;
                            fields.push(RecordField {
                                label: None,
                                name: None,
                                type_annotation: ty,
                            });
                        }
                        if self.skip(&TokenKind::Comma) {
                            continue;
                        }
                        self.expect(&TokenKind::CloseParen, "`,` or `)` in the variant fields")?;
                        break;
                    }
                    fields
                } else {
                    Vec::new()
                };
                variants.push(RecordConstructor {
                    name: vname,
                    fields,
                });
            }
            self.expect(&TokenKind::CloseBlock, "the closing `}` of the type definition")?;
            TypeDefKind::Record(variants)
        } else {
            TypeDefKind::External
        };

        Ok(TypeDef {
            name,
            params,
            kind,
            public,
            opaque,
            docs: None,
            attributes: Vec::new(),
            span: start.merge(self.current_token().span),
        })
    }

    // ------------------------------------------------------------------
    // Functions
    // ------------------------------------------------------------------

    fn parse_fn(&mut self, public: bool) -> Result<FnDef, ParseError> {
        let start = self.peek_span();
        self.consume(); // `fn`

        let name = self.expect_variable("the function name")?;

        // Arguments.
        self.expect(&TokenKind::OpenParen, "`(` after the function name")?;
        let mut args = Vec::new();
        loop {
            if self.peek() == &TokenKind::CloseParen {
                self.consume();
                break;
            }
            if self.peek() == &TokenKind::EndOfFile {
                return Err(self.unexpected("an argument name or `)` in the function arguments"));
            }
            args.push(self.parse_fn_arg()?);
            if self.skip(&TokenKind::Comma) {
                continue;
            }
            self.expect(&TokenKind::CloseParen, "`,` or `)` in the function arguments")?;
            break;
        }

        // Optional return annotation.
        let mut return_annotation = None;
        if self.skip(&TokenKind::Arrow) {
            return_annotation = Some(self.parse_type_annotation()?);
        }

        // External functions (`@external`) have no body.
        let body = if self.peek() == &TokenKind::OpenBlock {
            self.parse_block()?
        } else {
            Expression {
                expr: Expr::Block(Vec::new()),
                span: Span::point(self.pos),
            }
        };

        Ok(FnDef {
            name,
            args,
            body: Box::new(body),
            return_annotation,
            public,
            docs: None,
            attributes: Vec::new(),
            span: start.merge(self.current_token().span),
        })
    }

    fn parse_fn_arg(&mut self) -> Result<FnArg, ParseError> {
        let first = self.expect_variable("an argument name")?;

        // `name: type`
        if self.peek() == &TokenKind::Colon {
            self.consume();
            let ty = self.parse_type_annotation()?;
            return Ok(FnArg {
                label: None,
                name: first,
                type_annotation: Some(ty),
            });
        }

        // `label name: type`
        if matches!(self.peek(), TokenKind::Variable(_)) && self.peek_at(1) == &TokenKind::Colon {
            let label = first;
            let second = self.expect_variable("the argument name after the label")?;
            self.consume(); // `:`
            let ty = self.parse_type_annotation()?;
            return Ok(FnArg {
                label: Some(label),
                name: second,
                type_annotation: Some(ty),
            });
        }

        // Bare name, no annotation.
        Ok(FnArg {
            label: None,
            name: first,
            type_annotation: None,
        })
    }

    // ------------------------------------------------------------------
    // Constants
    // ------------------------------------------------------------------

    fn parse_const(&mut self, public: bool) -> Result<ConstantDef, ParseError> {
        let start = self.peek_span();
        self.consume(); // `const`

        let name = self.expect_variable("the constant name")?;

        let mut annotation = None;
        if self.skip(&TokenKind::Colon) {
            annotation = Some(self.parse_type_annotation()?);
        }

        self.expect(&TokenKind::Assign, "`=` after the constant name")?;
        let value = self.parse_const_value()?;

        Ok(ConstantDef {
            name,
            value,
            annotation,
            public,
            docs: None,
            attributes: Vec::new(),
            span: start.merge(self.current_token().span),
        })
    }

    fn parse_const_value(&mut self) -> Result<ConstValue, ParseError> {
        match self.peek().clone() {
            TokenKind::Int(_) => Ok(ConstValue::Int(int_from_token(self.consume().kind))),
            TokenKind::Float(_, _) => Ok(ConstValue::Float(float_from_token(self.consume().kind))),
            TokenKind::String(_) => Ok(ConstValue::String(string_from_token(self.consume().kind))),
            TokenKind::True => {
                self.consume();
                Ok(ConstValue::Bool(true))
            }
            TokenKind::False => {
                self.consume();
                Ok(ConstValue::Bool(false))
            }
            TokenKind::Nil => {
                self.consume();
                Ok(ConstValue::Nil)
            }
            TokenKind::Variable(_) => {
                // A reference to another constant, or a qualified
                // constructor: `option.Some`.
                let name = self.skip_variable().unwrap();
                if self.peek() == &TokenKind::Dot && matches!(self.peek_at(1), TokenKind::TypeName(_)) {
                    self.consume(); // `.`
                    let cname = self.consume().kind;
                    let cname = match cname {
                        TokenKind::TypeName(n) => n,
                        _ => unreachable!(),
                    };
                    return Ok(self.parse_const_constructor(
                        ConstructorRef { module: Some(name), name: cname },
                    )?);
                }
                Ok(ConstValue::Var(name))
            }
            TokenKind::TypeName(_) => {
                // A constructor with const arguments.
                let name = self.expect_type_name("a constructor name")?;
                return Ok(self.parse_const_constructor(
                    ConstructorRef { module: None, name },
                )?);
            }
            TokenKind::OpenSquare => self.parse_const_list(),
            TokenKind::HashOpen => self.parse_const_tuple(),
            TokenKind::BitArrayStart => {
                let ba = self.parse_bit_array::<ConstValue>(|p| p.parse_const_value())?;
                Ok(ConstValue::BitArray(ba))
            }
            other => Err(self.error_at(
                &self.current_token(),
                format!(
                    "expected a constant value (literal, list, tuple, bit array or constructor), found {}",
                    other.describe()
                ),
            )),
        }
    }

    /// Parse the optional `( ... )` argument list of a const constructor.
    fn parse_const_constructor(&mut self, ref_: ConstructorRef) -> Result<ConstValue, ParseError> {
        let ConstructorRef { module, name } = ref_;
        let mut args = Vec::new();
        if self.skip(&TokenKind::OpenParen) {
            loop {
                if self.peek() == &TokenKind::CloseParen {
                    self.consume();
                    break;
                }
                let label = if matches!(self.peek(), TokenKind::Variable(_))
                    && self.peek_at(1) == &TokenKind::Colon
                {
                    let l = self.skip_variable().unwrap();
                    self.consume(); // `:`
                    Some(l)
                } else {
                    None
                };
                let value = Box::new(self.parse_const_value()?);
                args.push(ConstArg { label, value });
                if self.skip(&TokenKind::Comma) {
                    continue;
                }
                self.expect(&TokenKind::CloseParen, "`,` or `)` in the constructor arguments")?;
                break;
            }
        }
        Ok(ConstValue::Constructor(
            ConstructorRef { module, name },
            args,
        ))
    }

    fn parse_const_list(&mut self) -> Result<ConstValue, ParseError> {
        self.consume(); // `[`
        let mut elements = Vec::new();
        let mut spread = None;
        if self.peek() != &TokenKind::CloseSquare {
            loop {
                if self.skip(&TokenKind::TwoDots) {
                    spread = Some(Box::new(self.parse_const_value()?));
                    self.expect(&TokenKind::CloseSquare, "`,` or `]` in the list")?;
                    break;
                }
                elements.push(self.parse_const_value()?);
                if self.skip(&TokenKind::Comma) {
                    continue;
                }
                self.expect(&TokenKind::CloseSquare, "an element or `]` in the list")?;
                break;
            }
        } else {
            self.consume();
        }
        Ok(ConstValue::List { elements, spread })
    }

    fn parse_const_tuple(&mut self) -> Result<ConstValue, ParseError> {
        self.consume(); // `#(`
        let mut elements = Vec::new();
        if self.peek() != &TokenKind::CloseParen {
            loop {
                elements.push(self.parse_const_value()?);
                if self.skip(&TokenKind::Comma) {
                    continue;
                }
                self.expect(&TokenKind::CloseParen, "`,` or `)` in the tuple")?;
                break;
            }
        } else {
            self.consume();
        }
        Ok(ConstValue::Tuple(elements))
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    /// A full expression: an assignment, assertion, echo, or a pipeline.
    fn parse_expression(&mut self) -> Result<Expression, ParseError> {
        let start = self.peek_span();
        match self.peek().clone() {
            TokenKind::Let => {
                self.consume();
                let is_assert = self.skip(&TokenKind::Assert);
                let pattern = self.parse_pattern()?;
                let mut annotation = None;
                if self.skip(&TokenKind::Colon) {
                    annotation = Some(self.parse_type_annotation()?);
                }
                self.expect(&TokenKind::Assign, "`=` after the let pattern")?;
                let value = Box::new(self.parse_expression()?);
                if is_assert {
                    let message = self.optional_as_message()?;
                    Ok(self.expr(start, Expr::LetAssert(LetAssert {
                        pattern,
                        value,
                        message,
                    })))
                } else {
                    Ok(self.expr(start, Expr::Let(Let {
                        pattern,
                        value,
                        annotation,
                    })))
                }
            }
            TokenKind::Assert => {
                self.consume();
                let value = Box::new(self.parse_expression()?);
                let message = self.optional_as_message()?;
                Ok(self.expr(start, Expr::Assert(Assert { value, message })))
            }
            TokenKind::Echo => {
                self.consume();
                let inner = self.parse_pipeline()?;
                Ok(self.expr(start, Expr::Echo(Box::new(inner))))
            }
            TokenKind::Use => Err(self.error_at(
                &self.current_token(),
                "`use` can only be used at the start of a block",
            )),
            _ => self.parse_pipeline(),
        }
    }

    /// The optional `as "message"` suffix on asserts, panics and todos.
    fn optional_as_message(&mut self) -> Result<Option<String>, ParseError> {
        if self.skip(&TokenKind::As) {
            Ok(Some(self.expect_string_lit("the string message after `as`")?))
        } else {
            Ok(None)
        }
    }

    fn parse_pipeline(&mut self) -> Result<Expression, ParseError> {
        let start = self.peek_span();
        let mut exprs = vec![self.parse_binary(0)?];
        let mut pipe_spans = Vec::new();
        while self.peek() == &TokenKind::PipeTo {
            pipe_spans.push(self.consume().span);
            exprs.push(self.parse_binary(0)?);
        }
        if pipe_spans.is_empty() {
            Ok(exprs.pop().unwrap())
        } else {
            Ok(self.expr(start, Expr::Pipeline { exprs, pipe_spans }))
        }
    }

    /// Precedence levels, 0 being the loosest binding.
    const PREC: [[Option<TokenKind>; 8]; 7] = [
        [Some(TokenKind::Or), None, None, None, None, None, None, None],
        [Some(TokenKind::And), None, None, None, None, None, None, None],
        [
            Some(TokenKind::EqualEqual),
            Some(TokenKind::NotEqual),
            None,
            None,
            None,
            None,
            None,
            None,
        ],
        [
            Some(TokenKind::LessThan),
            Some(TokenKind::GreaterThan),
            Some(TokenKind::LessThanOrEqual),
            Some(TokenKind::GreaterThanOrEqual),
            Some(TokenKind::LessThanDot),
            Some(TokenKind::GreaterThanDot),
            Some(TokenKind::LessThanOrEqualDot),
            Some(TokenKind::GreaterThanOrEqualDot),
        ],
        [Some(TokenKind::Concatenate), None, None, None, None, None, None, None],
        [
            Some(TokenKind::Plus),
            Some(TokenKind::Minus),
            Some(TokenKind::PlusDot),
            Some(TokenKind::MinusDot),
            None,
            None,
            None,
            None,
        ],
        [
            Some(TokenKind::Times),
            Some(TokenKind::Slash),
            Some(TokenKind::Percent),
            Some(TokenKind::TimesDot),
            Some(TokenKind::SlashDot),
            None,
            None,
            None,
        ],
    ];

    fn parse_binary(&mut self, level: usize) -> Result<Expression, ParseError> {
        let start = self.peek_span();
        if level == Self::PREC.len() {
            return self.parse_unary();
        }
        let mut lhs = self.parse_binary(level + 1)?;

        for op_opt in &Self::PREC[level] {
            let Some(op) = op_opt else {
                continue;
            };
            while self.peek() == op {
                self.consume();
                // The right hand side binds at the same precedence level,
                // which gives left associativity and correct nesting.
                let rhs = self.parse_binary(level + 1)?;
                lhs = self.expr(
                    start,
                    Expr::Binary {
                        op: token_to_binop(op.clone()),
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                );
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expression, ParseError> {
        let start = self.peek_span();
        if self.skip(&TokenKind::Minus) {
            let inner = self.parse_unary()?;
            return Ok(self.expr(start, Expr::Neg(Box::new(inner))));
        }
        if self.skip(&TokenKind::Bang) {
            let inner = self.parse_unary()?;
            return Ok(self.expr(start, Expr::Not(Box::new(inner))));
        }
        self.parse_postfix()
    }

    /// A primary expression followed by calls and field accesses.
    fn parse_postfix(&mut self) -> Result<Expression, ParseError> {
        let start = self.peek_span();
        let mut base = self.parse_primary()?;

        loop {
            if self.peek() == &TokenKind::Dot {
                // Field access: `.label` where label is an identifier or a
                // tuple index digit.
                self.consume();
                let label = match self.peek().clone() {
                    TokenKind::Variable(name) => {
                        self.consume();
                        name
                    }
                    TokenKind::Int(_) => {
                        let t = self.consume().kind;
                        match t {
                            TokenKind::Int(n) => n,
                            _ => unreachable!(),
                        }
                    }
                    other => {
                        return Err(self.error_at(
                            &self.current_token(),
                            format!("expected a field name after `.`, found {}", other.describe()),
                        ))
                    }
                };
                base = self.expr(start, Expr::FieldAccess {
                    base: Box::new(base),
                    label,
                });
            } else if self.peek() == &TokenKind::OpenParen {
                let args_start = self.consume(); // `(`
                let raw_args = self.parse_call_args()?;
                base = self.finish_call(start, base, raw_args, args_start.span)?;
            } else {
                break;
            }
        }
        Ok(base)
    }

    fn parse_call_args(&mut self) -> Result<RawArgs, ParseError> {
        let mut raw = RawArgs {
            spread_base: None,
            args: Vec::new(),
        };

        if self.peek() == &TokenKind::CloseParen {
            self.consume();
            return Ok(raw);
        }

        loop {
            // `..base` is only allowed as the first argument (record update).
            if raw.args.is_empty() && raw.spread_base.is_none() && self.peek() == &TokenKind::TwoDots {
                self.consume();
                let base = self.parse_expression()?;
                raw.spread_base = Some(base);
            } else if matches!(self.peek(), TokenKind::DiscardVariable(_)) {
                self.consume();
                raw.args.push(RawArg::Placeholder);
            } else {
                // Optional label.
                let label = if matches!(self.peek(), TokenKind::Variable(_))
                    && self.peek_at(1) == &TokenKind::Colon
                {
                    let l = self.skip_variable().unwrap();
                    self.consume(); // `:`
                    Some(l)
                } else {
                    None
                };

                // Label shorthand: `f(x:)` passes the variable `x`.
                if let Some(label) = &label {
                    if matches!(self.peek(), TokenKind::Comma | TokenKind::CloseParen) {
                        let value = self.expr(self.peek_span(), Expr::Var(label.clone()));
                        raw.args.push(RawArg::Labelled {
                            label: label.clone(),
                            value,
                        });
                    } else if matches!(self.peek(), TokenKind::DiscardVariable(_)) {
                        // Labelled placeholder: `f(a: _)`.
                        self.consume();
                        raw.args.push(RawArg::LabelledPlaceholder(label.clone()));
                    } else {
                        let value = self.parse_expression()?;
                        raw.args.push(RawArg::Labelled {
                            label: label.clone(),
                            value,
                        });
                    }
                } else {
                    let value = self.parse_expression()?;
                    raw.args.push(RawArg::Value(value));
                }
            }

            if self.skip(&TokenKind::Comma) {
                // A trailing comma is allowed.
                if self.peek() == &TokenKind::CloseParen {
                    break;
                }
                continue;
            }
            break;
        }
        self.consume(); // `)`

        Ok(raw)
    }

    /// Turn a parsed primary + raw arguments into a call, capture or record
    /// update.
    fn finish_call(
        &mut self,
        start: Span,
        fun: Expression,
        raw: RawArgs,
        _fun_paren_span: Span,
    ) -> Result<Expression, ParseError> {
        let fun_span = fun.span;

        // Record update: `Constructor(..base, label: value, ...)`
        if let Some(base) = raw.spread_base {
            let constructor = match &fun.expr {
                Expr::Constructor(ref c) => c.clone(),
                Expr::FieldAccess { base, label } if label.starts_with(|c: char| c.is_ascii_uppercase()) => {
                    // A module qualified constructor: `module.Constructor`.
                    match &base.expr {
                        Expr::Var(module) => ConstructorRef {
                            module: Some(module.clone()),
                            name: label.clone(),
                        },
                        _ => {
                            return Err(self.error_at(
                                &self.current_token(),
                                "record updates can only be used with record constructors, e.g. `Person(..person)`",
                            ))
                        }
                    }
                }
                _ => {
                    return Err(self.error_at(
                        &self.current_token(),
                        "record updates can only be used with record constructors, e.g. `Person(..person)`",
                    ))
                }
            };
            let mut args = Vec::new();
            for arg in raw.args {
                match arg {
                    RawArg::Labelled { label, value } => {
                        args.push(CallArg::Labelled { label, value });
                    }
                    RawArg::Value(_) => {
                        return Err(self.error_at(
                            &self.current_token(),
                            "only labelled arguments can follow `..` in a record update",
                        ))
                    }
                    RawArg::Placeholder | RawArg::LabelledPlaceholder(_) => {
                        return Err(self.error_at(
                            &self.current_token(),
                            "`_` placeholders are not allowed in record updates",
                        ))
                    }
                }
            }
            return Ok(self.expr(start, Expr::RecordUpdate(RecordUpdate {
                constructor,
                base: Box::new(base),
                args,
            })));
        }

        // Function capture: `name(..., _, ...)` with exactly one placeholder.
        let placeholders = raw
            .args
            .iter()
            .filter(|a| matches!(a, RawArg::Placeholder | RawArg::LabelledPlaceholder(_)))
            .count();
        if placeholders == 1 {
            let is_simple_fn = match &fun.expr {
                Expr::Var(name) => name.starts_with(|c: char| c.is_ascii_lowercase()),
                // A module qualified function: `io.println`.
                Expr::FieldAccess { base, label } => {
                    matches!(&base.expr, Expr::Var(_))
                        && label.starts_with(|c: char| c.is_ascii_lowercase())
                }
                _ => false,
            };
            if is_simple_fn {
                let args = raw
                    .args
                    .into_iter()
                    .map(|a| match a {
                        RawArg::Value(v) => CaptureArg::Value(v),
                        RawArg::Labelled { value, .. } => CaptureArg::Value(value),
                        RawArg::Placeholder | RawArg::LabelledPlaceholder(_) => {
                            CaptureArg::Placeholder
                        }
                    })
                    .collect();
                return Ok(self.expr(start, Expr::Capture(Capture {
                    fun: Box::new(fun),
                    args,
                })));
            }
        }

        // A plain call.
        let mut args = Vec::new();
        for arg in raw.args {
            match arg {
                RawArg::Value(v) => args.push(CallArg::Value(v)),
                RawArg::Labelled { label, value } => args.push(CallArg::Labelled { label, value }),
                RawArg::Placeholder | RawArg::LabelledPlaceholder(_) => {
                    return Err(self.error_at(
                        &self.current_token(),
                        "`_` can only be used as an argument in a function capture, e.g. `add(1, _)`",
                    ))
                }
            }
        }
        Ok(self.expr(start, Expr::Call(Call {
            fun: Box::new(fun),
            args,
            fun_span,
        })))
    }

    fn parse_primary(&mut self) -> Result<Expression, ParseError> {
        let start = self.peek_span();
        match self.peek().clone() {
            TokenKind::Int(_) => {
                let t = self.consume();
                Ok(self.expr(start, Expr::Int(int_from_token(t.kind))))
            }
            TokenKind::Float(_, _) => {
                let t = self.consume();
                Ok(self.expr(start, Expr::Float(float_from_token(t.kind))))
            }
            TokenKind::String(_) => {
                let t = self.consume();
                Ok(self.expr(start, Expr::String(string_from_token(t.kind))))
            }
            TokenKind::True => {
                self.consume();
                Ok(self.expr(start, Expr::Bool(true)))
            }
            TokenKind::False => {
                self.consume();
                Ok(self.expr(start, Expr::Bool(false))
            )
            }
            TokenKind::Nil => {
                self.consume();
                Ok(self.expr(start, Expr::Nil))
            }
            TokenKind::Variable(name) => {
                self.consume();
                // Dotted names (`io.println`, `person.name`) are handled by
                // the postfix loop as field access.
                Ok(self.expr(start, Expr::Var(name)))
            }
            TokenKind::TypeName(name) => {
                self.consume();
                Ok(self.expr(start, Expr::Constructor(ConstructorRef {
                    module: None,
                    name,
                })))
            }
            TokenKind::DiscardVariable(_) => Err(self.error_at(
                &self.current_token(),
                "discard variables (`_`) can only be used in patterns",
            )),
            TokenKind::OpenParen => {
                // Parentheses are transparent: parse the inner expression and
                // let postfix operators (calls, fields) bind to it.
                self.consume();
                let inner = self.parse_expression()?;
                self.expect(
                    &TokenKind::CloseParen,
                    "`)` to close the parenthesized expression",
                )?;
                Ok(inner)
            }
            TokenKind::OpenBlock => self.parse_block(),
            TokenKind::Case => self.parse_case(start),
            TokenKind::Fn => self.parse_fn_literal(start),
            TokenKind::OpenSquare => self.parse_list(start),
            TokenKind::HashOpen => self.parse_tuple(start),
            TokenKind::BitArrayStart => {
                let ba = self.parse_bit_array::<Expression>(|p| p.parse_expression())?;
                Ok(self.expr(start, Expr::BitArray(ba)))
            }
            TokenKind::Todo => {
                self.consume();
                let message = self.optional_as_message()?;
                Ok(self.expr(start, Expr::Todo { message }))
            }
            TokenKind::Panic => {
                self.consume();
                let message = self.optional_as_message()?;
                Ok(self.expr(start, Expr::Panic { message }))
            }
            other => Err(self.error_at(
                &self.current_token(),
                format!("expected an expression, found {}", other.describe()),
            )),
        }
    }

    // ------------------------------------------------------------------
    // Blocks & use
    // ------------------------------------------------------------------

    fn parse_block(&mut self) -> Result<Expression, ParseError> {
        let start = self.peek_span();
        self.expect(&TokenKind::OpenBlock, "`{` to start a block")?;

        // Collect leading `use` expressions.
        struct UseHead {
            patterns: Vec<Pattern>,
            call: Expression,
        }
        let mut uses: Vec<UseHead> = Vec::new();
        while self.peek() == &TokenKind::Use {
            self.consume();
            let mut patterns = Vec::new();
            loop {
                patterns.push(self.parse_pattern()?);
                if self.skip(&TokenKind::Comma) {
                    continue;
                }
                break;
            }
            self.expect(&TokenKind::DashArrow, "`<-` after the `use` patterns")?;
            let call = self.parse_expression()?;
            uses.push(UseHead { patterns, call });
        }

        // The rest of the block.
        let mut statements = Vec::new();
        while self.peek() != &TokenKind::CloseBlock {
            if self.peek() == &TokenKind::EndOfFile {
                return Err(self.unexpected("`}` to close the block"));
            }
            statements.push(self.parse_expression()?);
        }
        let close = self.expect(&TokenKind::CloseBlock, "the closing `}` of the block")?;

        if uses.is_empty() {
            if statements.is_empty() {
                return Err(self.error_at(
                    &close,
                    "a block cannot be empty; every function must return a value",
                ));
            }
            return Ok(Expression {
                expr: Expr::Block(statements),
                span: start.merge(close.span),
            });
        }

        // Build nested `use` expressions around the block body.
        let mut body = Expression {
            expr: Expr::Block(statements),
            span: start.merge(close.span),
        };
        for use_head in uses.into_iter().rev() {
            body = self.expr(start, Expr::Use(Use {
                patterns: use_head.patterns,
                call: Box::new(use_head.call),
                body: Box::new(body),
            }));
        }
        Ok(body)
    }

    // ------------------------------------------------------------------
    // Case expressions
    // ------------------------------------------------------------------

    fn parse_case(&mut self, start: Span) -> Result<Expression, ParseError> {
        self.consume(); // `case`

        let mut subjects = Vec::new();
        loop {
            subjects.push(self.parse_expression()?);
            if self.skip(&TokenKind::Comma) {
                continue;
            }
            break;
        }

        self.expect(&TokenKind::OpenBlock, "`{` after the case subjects")?;

        let mut clauses = Vec::new();
        while self.peek() != &TokenKind::CloseBlock {
            if self.peek() == &TokenKind::EndOfFile {
                return Err(self.unexpected("a clause or `}` in the case expression"));
            }
            let clause_start = self.peek_span();

            let mut patterns = Vec::new();
            loop {
                patterns.push(self.parse_pattern()?);
                if self.skip(&TokenKind::Comma) {
                    continue;
                }
                break;
            }

            if patterns.len() != subjects.len() {
                return Err(self.error_at(
                    &self.current_token(),
                    format!(
                        "expected {} pattern(s), one per subject, found {}",
                        subjects.len(),
                        patterns.len()
                    ),
                ));
            }

            let mut guard = None;
            if self.skip(&TokenKind::If) {
                guard = Some(self.parse_guard()?);
            }

            self.expect(&TokenKind::Arrow, "`->` after the clause pattern")?;
            let expression = self.parse_expression()?;

            clauses.push(Clause {
                patterns,
                guard,
                expression,
                span: clause_start.merge(self.current_token().span),
            });
        }
        let close = self.expect(&TokenKind::CloseBlock, "the closing `}` of the case expression")?;

        Ok(Expression {
            expr: Expr::Case(Case { subjects, clauses }),
            span: start.merge(close.span),
        })
    }

    // ------------------------------------------------------------------
    // fn literals
    // ------------------------------------------------------------------

    fn parse_fn_literal(&mut self, start: Span) -> Result<Expression, ParseError> {
        self.consume(); // `fn`
        self.expect(&TokenKind::OpenParen, "`(` after `fn`")?;

        let mut args = Vec::new();
        if self.peek() != &TokenKind::CloseParen {
            loop {
                match self.peek().clone() {
                    TokenKind::Variable(name) | TokenKind::DiscardVariable(name) => {
                        self.consume();
                        args.push(name);
                    }
                    other => {
                        return Err(self.error_at(
                            &self.current_token(),
                            format!(
                                "expected a parameter name in the anonymous function, found {}",
                                other.describe()
                            ),
                        ))
                    }
                }
                if self.skip(&TokenKind::Comma) {
                    continue;
                }
                self.expect(&TokenKind::CloseParen, "`,` or `)` in the anonymous function arguments")?;
                break;
            }
        } else {
            self.consume();
        }

        let body = self.parse_block()?;

        Ok(self.expr(start, Expr::Fn(FnLiteral {
            args,
            body: Box::new(body),
        })))
    }

    // ------------------------------------------------------------------
    // Lists & tuples
    // ------------------------------------------------------------------

    fn parse_list(&mut self, start: Span) -> Result<Expression, ParseError> {
        self.consume(); // `[`
        let mut elements = Vec::new();
        let mut spread = None;

        loop {
            if self.peek() == &TokenKind::CloseSquare {
                break;
            }
            if self.skip(&TokenKind::TwoDots) {
                spread = Some(Box::new(self.parse_expression()?));
                continue;
            }
            elements.push(self.parse_expression()?);
            if !self.skip(&TokenKind::Comma) {
                break; // `]` is consumed below.
            }
        }
        self.consume(); // `]`

        Ok(self.expr(start, Expr::List { elements, spread }))
    }

    fn parse_tuple(&mut self, start: Span) -> Result<Expression, ParseError> {
        self.consume(); // `#(`
        let mut elements = Vec::new();

        loop {
            if self.peek() == &TokenKind::CloseParen {
                break;
            }
            elements.push(self.parse_expression()?);
            if self.skip(&TokenKind::Comma) {
                continue;
            }
            // Single-element tuples require a trailing comma: `#(1,)`.
            if elements.len() < 2 {
                return Err(self.unexpected(
                    "a `,` after the single tuple element (e.g. `#(1,)`)",
                ));
            }
            break;
        }
        self.consume(); // `)`

        Ok(self.expr(start, Expr::Tuple(elements)))
    }

    // ------------------------------------------------------------------
    // Bit arrays (generic over segment value type)
    // ------------------------------------------------------------------

    fn parse_bit_array<T>(
        &mut self,
        mut parse_value: impl FnMut(&mut Self) -> Result<T, ParseError>,
    ) -> Result<BitArray<T>, ParseError> {
        self.consume(); // `<<`
        let mut segments = Vec::new();

        loop {
            if self.peek() == &TokenKind::BitArrayEnd {
                break;
            }
            let value = parse_value(self)?;
            let mut options = Vec::new();
            if self.skip(&TokenKind::Colon) {
                options = self.parse_bit_options()?;
            }
            segments.push(BitArraySegment { value, options });
            if !self.skip(&TokenKind::Comma) {
                break; // `>>` is consumed below.
            }
        }
        self.consume(); // `>>`

        Ok(BitArray { segments })
    }

    fn parse_bit_options(&mut self) -> Result<Vec<BitOption>, ParseError> {
        let mut options = Vec::new();
        loop {
            match self.peek().clone() {
                TokenKind::Variable(name) => {
                    self.consume();
                    let option = match name.as_str() {
                        "bits" => BitOption::Bits,
                        "bytes" => BitOption::Bytes,
                        "float" => BitOption::Float,
                        "int" => BitOption::Int,
                        "big" => BitOption::Big,
                        "little" => BitOption::Little,
                        "native" => BitOption::Native,
                        "utf8" => BitOption::Utf8,
                        "utf16" => BitOption::Utf16,
                        "utf32" => BitOption::Utf32,
                        "utf8_codepoint" => BitOption::Utf8Codepoint,
                        "utf16_codepoint" => BitOption::Utf16Codepoint,
                        "utf32_codepoint" => BitOption::Utf32Codepoint,
                        "signed" => BitOption::Signed,
                        "unsigned" => BitOption::Unsigned,
                        "size" | "unit" => {
                            self.expect(&TokenKind::OpenParen, "`(` after `size`/`unit`")?;
                            let e = self.parse_expression()?;
                            self.expect(&TokenKind::CloseParen, "`)` after the size/unit expression")?;
                            if name == "size" {
                                BitOption::Size(e)
                            } else {
                                BitOption::Unit(e)
                            }
                        }
                        other => {
                            return Err(self.error_at(
                                &self.current_token(),
                                format!("unknown bit array option `{other}`"),
                            ))
                        }
                    };
                    options.push(option);
                }
                _ => {
                    return Err(self.unexpected("a bit array option"))
                }
            }
            // Options are separated by `-`.
            if self.skip(&TokenKind::Minus) {
                continue;
            }
            break;
        }
        Ok(options)
    }

    // ------------------------------------------------------------------
    // Patterns
    // ------------------------------------------------------------------

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let mut alternatives = vec![self.parse_as_pattern()?];
        while self.skip(&TokenKind::Pipe) {
            alternatives.push(self.parse_as_pattern()?);
        }
        if alternatives.len() == 1 {
            Ok(alternatives.pop().unwrap())
        } else {
            Ok(Pattern::Alternative(alternatives))
        }
    }

    fn parse_as_pattern(&mut self) -> Result<Pattern, ParseError> {
        let pattern = self.parse_concat_pattern()?;
        if self.skip(&TokenKind::As) {
            let name = self.expect_variable("the variable name after `as`")?;
            Ok(Pattern::Alias {
                pattern: Box::new(pattern),
                name,
            })
        } else {
            Ok(pattern)
        }
    }

    fn parse_concat_pattern(&mut self) -> Result<Pattern, ParseError> {
        let base = self.parse_base_pattern()?;
        // String prefix pattern: `"prefix" <> var`
        if let Pattern::String(prefix) = &base {
            if self.peek() == &TokenKind::Concatenate
                && matches!(self.peek_at(1), TokenKind::Variable(_) | TokenKind::DiscardVariable(_))
            {
                self.consume(); // `<>`
                let rest_tok = self.consume().kind;
                let rest = match rest_tok {
                    TokenKind::Variable(n) => Pattern::Var(n),
                    TokenKind::DiscardVariable(n) => Pattern::Discard(n),
                    _ => unreachable!(),
                };
                return Ok(Pattern::StringPrefix {
                    prefix: prefix.clone(),
                    rest: Box::new(rest),
                });
            }
        }
        Ok(base)
    }

    fn parse_base_pattern(&mut self) -> Result<Pattern, ParseError> {
        match self.peek().clone() {
            TokenKind::Int(_) => Ok(Pattern::Int(int_from_token(self.consume().kind))),
            TokenKind::Float(_, _) => Ok(Pattern::Float(float_from_token(self.consume().kind))),
            TokenKind::String(_) => Ok(Pattern::String(string_from_token(self.consume().kind))),
            TokenKind::True => {
                self.consume();
                Ok(Pattern::Bool(true))
            }
            TokenKind::False => {
                self.consume();
                Ok(Pattern::Bool(false))
            }
            TokenKind::Nil => {
                self.consume();
                Ok(Pattern::Nil)
            }
            TokenKind::Variable(name) => {
                self.consume();
                // Module qualified constructor: `option.Some`.
                if self.peek() == &TokenKind::Dot && matches!(self.peek_at(1), TokenKind::TypeName(_)) {
                    self.consume(); // `.`
                    let name2 = self.consume().kind;
                    let name2 = match name2 {
                        TokenKind::TypeName(n) => n,
                        _ => unreachable!(),
                    };
                    return self.parse_constructor_fields(Some(name), name2);
                }
                Ok(Pattern::Var(name))
            }
            TokenKind::DiscardVariable(name) => {
                self.consume();
                Ok(Pattern::Discard(name))
            }
            TokenKind::TypeName(name) => {
                self.consume();
                self.parse_constructor_fields(None, name)
            }
            TokenKind::OpenSquare => {
                self.consume(); // `[`
                let mut elements = Vec::new();
                let mut rest = None;

                loop {
                    if self.peek() == &TokenKind::CloseSquare {
                        break;
                    }
                    if self.skip(&TokenKind::TwoDots) {
                        // `..rest`, `.._` or bare `..`
                        if let Some(name) = self.skip_variable() {
                            rest = Some(Box::new(Pattern::Var(name)));
                        } else if matches!(self.peek(), TokenKind::DiscardVariable(_)) {
                            let n = self.consume().kind;
                            rest = Some(Box::new(match n {
                                TokenKind::DiscardVariable(n) => Pattern::Discard(n),
                                _ => unreachable!(),
                            }));
                        } else {
                            rest = Some(Box::new(Pattern::Discard("_".into())));
                        }
                        // The spread must be the last element.
                        if self.peek() != &TokenKind::CloseSquare {
                            return Err(self.unexpected("`]` after the list spread"));
                        }
                        break;
                    }
                    elements.push(self.parse_pattern()?);
                    if !self.skip(&TokenKind::Comma) {
                        break; // `]` is consumed below.
                    }
                }
                self.consume(); // `]`

                Ok(Pattern::List { elements, rest })
            }
            TokenKind::HashOpen => {
                self.consume(); // `#(`
                let mut elements = Vec::new();

                loop {
                    if self.peek() == &TokenKind::CloseParen {
                        break;
                    }
                    elements.push(self.parse_pattern()?);
                    if self.skip(&TokenKind::Comma) {
                        continue;
                    }
                    // Single-element tuples require a trailing comma.
                    if elements.len() < 2 {
                        return Err(self.unexpected(
                            "a `,` after the single tuple element (e.g. `#(1,)`)",
                        ));
                    }
                    break;
                }
                self.consume(); // `)`

                Ok(Pattern::Tuple(elements))
            }
            TokenKind::BitArrayStart => {
                let ba = self.parse_bit_array::<Pattern>(|p| p.parse_pattern())?;
                Ok(Pattern::BitArray(ba))
            }
            other => Err(self.error_at(
                &self.current_token(),
                format!("expected a pattern, found {}", other.describe()),
            )),
        }
    }

    fn parse_constructor_fields(
        &mut self,
        module: Option<String>,
        name: String,
    ) -> Result<Pattern, ParseError> {
        let mut args = Vec::new();
        if self.skip(&TokenKind::OpenParen) {
            loop {
                if self.peek() == &TokenKind::CloseParen {
                    break;
                }
                // `..` discards the remaining fields (must be last).
                if self.skip(&TokenKind::TwoDots) {
                    args.push(PatternArg::DiscardRest);
                    if self.peek() != &TokenKind::CloseParen {
                        return Err(self.unexpected("`)` after `..` in the pattern"));
                    }
                    break;
                }
                // Optional label.
                let label = if matches!(self.peek(), TokenKind::Variable(_))
                    && self.peek_at(1) == &TokenKind::Colon
                {
                    let l = self.skip_variable().unwrap();
                    self.consume(); // `:`
                    Some(l)
                } else {
                    None
                };
                let pattern = Box::new(self.parse_pattern()?);
                args.push(match label {
                    Some(label) => PatternArg::Named { label, pattern },
                    None => PatternArg::Unnamed(pattern),
                });
                if !self.skip(&TokenKind::Comma) {
                    break; // `)` is consumed below.
                }
            }
            self.consume(); // `)`
        }
        Ok(Pattern::Constructor(
            ConstructorRef { module, name },
            args,
        ))
    }

    // ------------------------------------------------------------------
    // Guards
    // ------------------------------------------------------------------

    fn parse_guard(&mut self) -> Result<Guard, ParseError> {
        // Precedence (loosest first): `||`, `&&`, comparisons.
        let mut lhs = self.parse_guard_and()?;
        while self.peek() == &TokenKind::Or {
            self.consume();
            let rhs = self.parse_guard_and()?;
            lhs = Guard::Binary {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_guard_and(&mut self) -> Result<Guard, ParseError> {
        let mut lhs = self.parse_guard_comparison()?;
        while self.peek() == &TokenKind::And {
            self.consume();
            let rhs = self.parse_guard_comparison()?;
            lhs = Guard::Binary {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    /// Comparison operators (loosest arithmetic level).
    fn parse_guard_comparison(&mut self) -> Result<Guard, ParseError> {
        let mut lhs = self.parse_guard_concat()?;
        loop {
            let op = match self.peek().clone() {
                TokenKind::EqualEqual => Some(BinOp::Eq),
                TokenKind::NotEqual => Some(BinOp::NotEq),
                TokenKind::LessThan => Some(BinOp::Lt),
                TokenKind::GreaterThan => Some(BinOp::Gt),
                TokenKind::LessThanOrEqual => Some(BinOp::LtEq),
                TokenKind::GreaterThanOrEqual => Some(BinOp::GtEq),
                TokenKind::LessThanDot => Some(BinOp::LtFloat),
                TokenKind::GreaterThanDot => Some(BinOp::GtFloat),
                TokenKind::LessThanOrEqualDot => Some(BinOp::LtEqFloat),
                TokenKind::GreaterThanOrEqualDot => Some(BinOp::GtEqFloat),
                _ => None,
            };
            let Some(op) = op else { break };
            self.consume();
            let rhs = self.parse_guard_concat()?;
            lhs = Guard::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    /// String concatenation `<>`.
    fn parse_guard_concat(&mut self) -> Result<Guard, ParseError> {
        let mut lhs = self.parse_guard_add()?;
        while self.peek() == &TokenKind::Concatenate {
            self.consume();
            let rhs = self.parse_guard_add()?;
            lhs = Guard::Binary {
                op: BinOp::Concat,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    /// Addition and subtraction.
    fn parse_guard_add(&mut self) -> Result<Guard, ParseError> {
        let mut lhs = self.parse_guard_mul()?;
        loop {
            let op = match self.peek().clone() {
                TokenKind::Plus => Some(BinOp::Plus),
                TokenKind::Minus => Some(BinOp::Minus),
                TokenKind::PlusDot => Some(BinOp::PlusFloat),
                TokenKind::MinusDot => Some(BinOp::MinusFloat),
                _ => None,
            };
            let Some(op) = op else { break };
            self.consume();
            let rhs = self.parse_guard_mul()?;
            lhs = Guard::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    /// Multiplication, division and modulo.
    fn parse_guard_mul(&mut self) -> Result<Guard, ParseError> {
        let mut lhs = self.parse_guard_value()?;
        loop {
            let op = match self.peek().clone() {
                TokenKind::Times => Some(BinOp::Times),
                TokenKind::Slash => Some(BinOp::Slash),
                TokenKind::Percent => Some(BinOp::Percent),
                TokenKind::TimesDot => Some(BinOp::TimesFloat),
                TokenKind::SlashDot => Some(BinOp::SlashFloat),
                _ => None,
            };
            let Some(op) = op else { break };
            self.consume();
            let rhs = self.parse_guard_value()?;
            lhs = Guard::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_guard_value(&mut self) -> Result<Guard, ParseError> {
        match self.peek().clone() {
            TokenKind::Int(_) => Ok(Guard::Int(int_from_token(self.consume().kind))),
            TokenKind::Float(_, _) => Ok(Guard::Float(float_from_token(self.consume().kind))),
            TokenKind::String(_) => Ok(Guard::String(string_from_token(self.consume().kind))),
            TokenKind::True => {
                self.consume();
                Ok(Guard::Bool(true))
            }
            TokenKind::False => {
                self.consume();
                Ok(Guard::Bool(false))
            }
            TokenKind::Nil => {
                self.consume();
                Ok(Guard::Nil)
            }
            TokenKind::Variable(name) => {
                self.consume();
                let mut guard = Guard::Var(name);
                while self.peek() == &TokenKind::Dot {
                    self.consume();
                    let label = match self.peek().clone() {
                        TokenKind::Variable(label) => {
                            self.consume();
                            label
                        }
                        other => {
                            return Err(self.error_at(
                                &self.current_token(),
                                format!("expected a field name in the guard, found {}", other.describe()),
                            ))
                        }
                    };
                    guard = Guard::FieldAccess {
                        base: Box::new(guard),
                        label,
                    };
                }
                // Function call: `list.length(x)`.
                if self.peek() == &TokenKind::OpenParen {
                    self.consume();
                    let mut args = Vec::new();
                    loop {
                        if self.peek() == &TokenKind::CloseParen {
                            break;
                        }
                        args.push(self.parse_guard_value()?);
                        if !self.skip(&TokenKind::Comma) {
                            break; // `)` is consumed below.
                        }
                    }
                    self.consume(); // `)`
                    guard = Guard::Call {
                        fun: Box::new(guard),
                        args,
                    };
                }
                Ok(guard)
            }
            other => Err(self.error_at(
                &self.current_token(),
                format!(
                    "expected a guard expression (literals, variables and operators), found {}",
                    other.describe()
                ),
            )),
        }
    }

    // ------------------------------------------------------------------
    // Type annotations
    // ------------------------------------------------------------------

    fn parse_type_annotation(&mut self) -> Result<TypeAnnotation, ParseError> {
        match self.peek().clone() {
            TokenKind::Fn => {
                self.consume();
                self.expect(&TokenKind::OpenParen, "`(` after `fn` in the type annotation")?;
                let mut args = Vec::new();
                loop {
                    if self.peek() == &TokenKind::CloseParen {
                        break;
                    }
                    args.push(self.parse_type_annotation()?);
                    if !self.skip(&TokenKind::Comma) {
                        break; // `)` is consumed below.
                    }
                }
                self.consume(); // `)`
                self.expect(&TokenKind::Arrow, "`->` in the function type")?;
                let ret = Box::new(self.parse_type_annotation()?);
                Ok(TypeAnnotation::Fn { args, ret })
            }
            TokenKind::HashOpen => {
                self.consume();
                let mut elements = Vec::new();
                loop {
                    if self.peek() == &TokenKind::CloseParen {
                        break;
                    }
                    elements.push(self.parse_type_annotation()?);
                    if !self.skip(&TokenKind::Comma) {
                        break; // `)` is consumed below.
                    }
                }
                self.consume(); // `)`
                Ok(TypeAnnotation::Tuple(elements))
            }
            TokenKind::Nil => {
                self.consume();
                Ok(TypeAnnotation::Named {
                    module: None,
                    name: "Nil".into(),
                    args: vec![],
                })
            }
            TokenKind::Variable(name) | TokenKind::TypeName(name) => {
                self.consume();
                // Qualified: `module.Type`
                let module = if self.peek() == &TokenKind::Dot && matches!(self.peek_at(1), TokenKind::TypeName(_)) {
                    self.consume(); // `.`
                    Some(name.clone())
                } else {
                    None
                };
                let name = match self.peek().clone() {
                    TokenKind::TypeName(n) if module.is_some() => {
                        self.consume();
                        n
                    }
                    _ => name,
                };
                // `BitArray` with optional size and unit; a bare
                // `BitArray` means any size.
                if name == "BitArray" && module.is_none() {
                    if self.peek() == &TokenKind::OpenParen {
                        self.consume();
                        let mut size = None;
                        let mut unit = None;
                        if self.peek() != &TokenKind::CloseParen {
                            let t = self.consume();
                            size = Some(int_from_token(t.kind));
                            if self.skip(&TokenKind::Comma) {
                                let t = self.consume();
                                unit = Some(int_from_token(t.kind));
                            }
                            self.expect(&TokenKind::CloseParen, "`,` or `)` in the BitArray type")?;
                        } else {
                            self.consume();
                        }
                        return Ok(TypeAnnotation::BitArray { size, unit });
                    }
                    return Ok(TypeAnnotation::BitArray { size: None, unit: None });
                }
                let mut args = Vec::new();
                if self.peek() == &TokenKind::OpenParen {
                    self.consume();
                    loop {
                        if self.peek() == &TokenKind::CloseParen {
                            break;
                        }
                        args.push(self.parse_type_annotation()?);
                        if !self.skip(&TokenKind::Comma) {
                            break; // `)` is consumed below.
                        }
                    }
                    self.consume(); // `)`
                }
                Ok(TypeAnnotation::Named {
                    module,
                    name,
                    args,
                })
            }
            other => Err(self.error_at(
                &self.current_token(),
                format!("expected a type annotation, found {}", other.describe()),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn int_from_token(kind: TokenKind) -> IntValue {
    match kind {
        TokenKind::Int(raw) => IntValue { raw },
        _ => unreachable!(),
    }
}

fn float_from_token(kind: TokenKind) -> FloatValue {
    match kind {
        TokenKind::Float(value, raw) => FloatValue { value, raw },
        _ => unreachable!(),
    }
}

fn string_from_token(kind: TokenKind) -> String {
    match kind {
        TokenKind::String(s) => s,
        _ => unreachable!(),
    }
}

fn token_to_binop(kind: TokenKind) -> BinOp {
    match kind {
        TokenKind::And => BinOp::And,
        TokenKind::Or => BinOp::Or,
        TokenKind::EqualEqual => BinOp::Eq,
        TokenKind::NotEqual => BinOp::NotEq,
        TokenKind::LessThan => BinOp::Lt,
        TokenKind::GreaterThan => BinOp::Gt,
        TokenKind::LessThanOrEqual => BinOp::LtEq,
        TokenKind::GreaterThanOrEqual => BinOp::GtEq,
        TokenKind::LessThanDot => BinOp::LtFloat,
        TokenKind::GreaterThanDot => BinOp::GtFloat,
        TokenKind::LessThanOrEqualDot => BinOp::LtEqFloat,
        TokenKind::GreaterThanOrEqualDot => BinOp::GtEqFloat,
        TokenKind::Concatenate => BinOp::Concat,
        TokenKind::Plus => BinOp::Plus,
        TokenKind::Minus => BinOp::Minus,
        TokenKind::Times => BinOp::Times,
        TokenKind::Slash => BinOp::Slash,
        TokenKind::Percent => BinOp::Percent,
        TokenKind::PlusDot => BinOp::PlusFloat,
        TokenKind::MinusDot => BinOp::MinusFloat,
        TokenKind::TimesDot => BinOp::TimesFloat,
        TokenKind::SlashDot => BinOp::SlashFloat,
        _ => unreachable!("not a binary operator: {kind:?}"),
    }
}

fn join_docs(lines: Vec<String>) -> Option<String> {
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// Attach parsed attributes to a definition.
fn attach_attributes(defn: Definition, attrs: Vec<Attribute>) -> Definition {
    match defn {
        Definition::Fn(mut f) => {
            f.attributes = attrs;
            Definition::Fn(f)
        }
        Definition::Type(mut t) => {
            t.attributes = attrs;
            Definition::Type(t)
        }
        Definition::Constant(mut c) => {
            c.attributes = attrs;
            Definition::Constant(c)
        }
        other => other,
    }
}

fn with_docs(def: Definition, docs: &[String]) -> Definition {
    let joined = join_docs(docs.to_vec());
    match def {
        Definition::Fn(mut d) => {
            d.docs = joined;
            Definition::Fn(d)
        }
        Definition::Type(mut d) => {
            d.docs = joined;
            Definition::Type(d)
        }
        Definition::Constant(mut d) => {
            d.docs = joined;
            Definition::Constant(d)
        }
        other => other,
    }
}
