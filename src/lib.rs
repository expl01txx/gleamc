//! # gleam-to-c
//!
//! A compiler for a subset of the [Gleam](https://gleam-lang.io/) functional
//! programming language to C11, implemented in Rust.
//!
//! The crate is split into four parts:
//!
//! - [`lexer`] — turns source text into a stream of [`Token`s].
//! - [`ast`] — the abstract syntax tree types.
//! - [`parser`] — a recursive descent parser producing a [`Module`].
//! - [`typechecker`] — a Hindley–Milner style type checker with unification.
//!
//! Code generation lives in [`codegen`]: a parsed and type checked module is
//! translated to C that links against the embedded runtime.
//!
//! ## Usage
//!
//! ```
//! use gleam_to_c::{parse, translate};
//!
//! let module = parse(r#"
//!     import gleam/io
//!
//!     /// Double a number.
//!     pub fn double(x: Int) -> Int {
//!         x * 2
//!     }
//! "#).expect("should parse");
//!
//! assert_eq!(module.definitions.len(), 2);
//! ```

pub mod ast;
pub mod codegen;
pub mod lexer;
pub mod parser;
pub mod typechecker;

pub use ast::*;
pub use codegen::{generate_c, translate, write_c_project, CodegenError, CompileError};
pub use lexer::{lex, Token, TokenKind};
pub use parser::parse_module;
pub use typechecker::{typecheck, typecheck_source, TypeCheckError};

/// Parse a Gleam source file into an AST.
///
/// Returns the first lexing or parsing error encountered.
pub fn parse(source: &str) -> Result<Module, ParseError> {
    let tokens = lex(source)?;
    parse_module(tokens)
}
