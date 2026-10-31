//! # gleam-parser
//!
//! A tokenizer and AST parser for the [Gleam](https://gleam-lang.io/)
//! functional programming language, implemented in Rust.
//!
//! The crate is split into three parts:
//!
//! - [`lexer`] — turns source text into a stream of [`Token`s].
//! - [`ast`] — the abstract syntax tree types.
//! - [`parser`] — a recursive descent parser producing a [`Module`].
//!
//! ## Usage
//!
//! ```
//! use gleam_parser::parse;
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

pub use ast::*;
pub use codegen::{generate_c, translate, write_c_project, CodegenError, CompileError};
pub use lexer::{lex, Token, TokenKind};
pub use parser::parse_module;

/// Parse a Gleam source file into an AST.
///
/// Returns the first lexing or parsing error encountered.
pub fn parse(source: &str) -> Result<Module, ParseError> {
    let tokens = lex(source)?;
    parse_module(tokens)
}
