use crate::ast::*;
use crate::codegen::{
    generate_c, generate_c_standalone, is_stdlib_module, write_generated_project,
};
use crate::{parse, typecheck};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// A source or compilation error in a Gleam project.
#[derive(Debug)]
pub struct SourceError {
    path: PathBuf,
    location: Option<(u32, u32)>,
    message: String,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())?;
        if let Some((line, column)) = self.location {
            write!(f, ":{line}:{column}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for SourceError {}

/// Generated C and the native source files that must be linked with it.
#[derive(Debug)]
pub struct CTranslation {
    pub source: String,
    pub native_sources: Vec<PathBuf>,
}

/// Load the entry file and its imports and return the generated C.
pub fn translate_file(entry: impl AsRef<Path>, standalone: bool) -> Result<String, SourceError> {
    translate_file_with_externals(entry, standalone).map(|translation| translation.source)
}

/// Load the entry file and its imports, and resolve C externals in the source tree.
pub fn translate_file_with_externals(
    entry: impl AsRef<Path>,
    standalone: bool,
) -> Result<CTranslation, SourceError> {
    let entry = entry.as_ref();
    let path = entry.canonicalize().map_err(|error| SourceError {
        path: entry.to_owned(),
        location: None,
        message: error.to_string(),
    })?;
    let root = path
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "src"))
        .or_else(|| path.parent())
        .unwrap_or(Path::new("."));
    let mut project = Project {
        root,
        files: Vec::new(),
        loaded: HashMap::new(),
        loading: HashSet::new(),
        definitions: Vec::new(),
        offset: 0,
    };
    project.load(&path, true)?;
    let module = Module {
        docs: None,
        definitions: std::mem::take(&mut project.definitions),
    };
    typecheck(&module).map_err(|error| project.error(error.span, error.message))?;
    let generate = if standalone {
        generate_c_standalone
    } else {
        generate_c
    };
    let source = generate(&module).map_err(|error| project.error(error.span, error.message))?;
    let native_sources = project.native_sources(&module)?;
    Ok(CTranslation {
        source,
        native_sources,
    })
}

/// Compile a source tree and write a C translation unit and its runtime.
pub fn write_c_project_file(
    entry: impl AsRef<Path>,
    directory: impl AsRef<Path>,
) -> Result<(), SourceError> {
    write_c_project_file_with_externals(entry, directory).map(|_| ())
}

/// Write the generated C and runtime and return the additional C files to link.
pub fn write_c_project_file_with_externals(
    entry: impl AsRef<Path>,
    directory: impl AsRef<Path>,
) -> Result<Vec<PathBuf>, SourceError> {
    let translation = translate_file_with_externals(entry, false)?;
    let directory = directory.as_ref();
    write_generated_project(&translation.source, directory).map_err(|error| SourceError {
        path: directory.into(),
        location: None,
        message: error.to_string(),
    })?;
    Ok(translation.native_sources)
}

struct Source {
    path: PathBuf,
    text: String,
    start: usize,
}

struct Project<'a> {
    root: &'a Path,
    files: Vec<Source>,
    loaded: HashMap<PathBuf, Names>,
    loading: HashSet<PathBuf>,
    definitions: Vec<Definition>,
    offset: usize,
}

#[derive(Default)]
struct Names {
    values: HashMap<String, Symbol>,
    types: HashMap<String, Symbol>,
}

struct Symbol {
    name: String,
    public: bool,
    constant: bool,
}

impl Names {
    fn register(module: &Module, id: usize, entry: bool) -> Self {
        let mut names = Self::default();
        for definition in &module.definitions {
            match definition {
                Definition::Fn(function) => {
                    let name = if entry && function.name == "main" {
                        "main".into()
                    } else {
                        format!("_m_{id}_{}", function.name)
                    };
                    names.values.insert(
                        function.name.clone(),
                        Symbol {
                            name,
                            public: function.public,
                            constant: false,
                        },
                    );
                }
                Definition::Constant(constant) => {
                    let name = format!("_m_{id}_{}", constant.name);
                    names.values.insert(
                        constant.name.clone(),
                        Symbol {
                            name,
                            public: constant.public,
                            constant: true,
                        },
                    );
                }
                Definition::Type(ty) => {
                    let name = format!("M{id}_{}", ty.name);
                    names.types.insert(
                        ty.name.clone(),
                        Symbol {
                            name,
                            public: ty.public,
                            constant: false,
                        },
                    );
                    if let TypeDefKind::Record(variants) = &ty.kind {
                        for variant in variants {
                            let name = format!("M{id}_{}", variant.name);
                            names.values.insert(
                                variant.name.clone(),
                                Symbol {
                                    name,
                                    public: ty.public && !ty.opaque,
                                    constant: false,
                                },
                            );
                        }
                    }
                }
                Definition::Import(_) => {}
            }
        }
        names
    }
}

impl Project<'_> {
    fn source_at(&self, span: Span) -> &Source {
        self.files
            .iter()
            .rev()
            .find(|file| file.start <= span.start)
            .expect("at least one source has been loaded")
    }

    fn error(&self, span: Span, message: String) -> SourceError {
        let file = self.source_at(span);
        let start = span.start.saturating_sub(file.start).min(file.text.len());
        SourceError {
            path: file.path.clone(),
            location: Some(Span::point(start).line_col(&file.text)),
            message,
        }
    }

    fn native_sources(&self, module: &Module) -> Result<Vec<PathBuf>, SourceError> {
        let mut paths = BTreeSet::new();
        for definition in &module.definitions {
            let Definition::Fn(function) = definition else {
                continue;
            };
            let Some(external) = function.c_externals().next() else {
                continue;
            };
            let native = Path::new(&external.module);
            if !native.extension().is_some_and(|extension| extension == "c") {
                continue;
            }
            let source = self.source_at(function.span);
            let directory = source.path.parent().expect("absolute source file path");
            let native = directory.join(native);
            let path = native.canonicalize().map_err(|error| {
                self.error(
                    function.span,
                    format!(
                        "cannot load C external source `{}`: {error}",
                        native.display()
                    ),
                )
            })?;
            if !path.is_file() {
                return Err(self.error(
                    function.span,
                    format!("C external source `{}` is not a file", native.display()),
                ));
            }
            paths.insert(path);
        }
        Ok(paths.into_iter().collect())
    }

    fn load(&mut self, path: &Path, entry: bool) -> Result<(), SourceError> {
        if self.loaded.contains_key(path) {
            return Ok(());
        }
        let text = fs::read_to_string(path).map_err(|error| SourceError {
            path: path.to_owned(),
            location: None,
            message: error.to_string(),
        })?;
        let mut module = parse(&text).map_err(|error| SourceError {
            path: path.to_owned(),
            location: Some((error.line, error.col)),
            message: error.message,
        })?;
        let id = self.files.len();
        let offset = self.offset;
        self.offset += text.len() + 1;
        self.files.push(Source {
            path: path.to_owned(),
            text,
            start: offset,
        });
        self.loading.insert(path.to_owned());

        for definition in &module.definitions {
            if let Definition::Import(import) = definition {
                let imported = self.import_path(import);
                match imported.canonicalize() {
                    Ok(imported) => {
                        if self.loading.contains(&imported) {
                            return Err(self.error(
                                shifted(import.span, offset),
                                format!("cyclic import of `{}`", import.module.join("/")),
                            ));
                        }
                        self.load(&imported, false)?;
                    }
                    Err(error) => {
                        if !native_module(&import.module.join("/")) {
                            return Err(self.error(
                                shifted(import.span, offset),
                                format!(
                                    "cannot load module `{}`: {error}",
                                    import.module.join("/")
                                ),
                            ));
                        }
                    }
                }
            }
        }
        let names = Names::register(&module, id, entry);
        let mut resolver = Resolver {
            local: &names,
            imports: HashMap::new(),
            values: HashMap::new(),
            types: HashMap::new(),
            scopes: Vec::new(),
            offset,
            span: Span::point(offset),
        };
        let mut native_imports = Vec::new();
        for definition in &mut module.definitions {
            let Definition::Import(import) = definition else {
                continue;
            };
            let path = self.import_path(import);
            let dependency = path
                .canonicalize()
                .ok()
                .and_then(|path| self.loaded.get(&path));
            let native_alias = format!("gm_{id}_{}", native_imports.len());
            resolver
                .add_import(import, dependency, &native_alias, id)
                .map_err(|message| self.error(shifted(import.span, offset), message))?;
            if dependency.is_none() {
                import.alias = Some(native_alias);
                for item in &mut import.unqualified {
                    let (name, alias, is_type) = item.names();
                    *item = if is_type {
                        UnqualifiedImport::TypeAlias {
                            name: name.into(),
                            alias: format!("GI{id}_{alias}"),
                        }
                    } else {
                        UnqualifiedImport::ValueAlias {
                            name: name.into(),
                            alias: format!("_gi_{id}_{alias}"),
                        }
                    };
                }
                import.span = shifted(import.span, offset);
                native_imports.push(Definition::Import(import.clone()));
            }
        }
        self.definitions.extend(native_imports);
        for mut definition in module.definitions {
            if matches!(definition, Definition::Import(_)) {
                continue;
            }
            resolver
                .definition(&mut definition)
                .map_err(|message| self.error(resolver.span, message))?;
            self.definitions.push(definition);
        }
        self.loading.remove(path);
        self.loaded.insert(path.to_owned(), names);
        Ok(())
    }

    fn import_path(&self, import: &Import) -> PathBuf {
        let mut path = self.root.to_owned();
        path.extend(&import.module);
        path.set_extension("gleam");
        path
    }
}

fn native_module(path: &str) -> bool {
    is_stdlib_module(path) || matches!(path, "gleam/option" | "gleam/order" | "gleam/result")
}

fn shifted(span: Span, offset: usize) -> Span {
    Span::new(span.start + offset, span.end + offset)
}

enum Imported<'a> {
    Source(&'a Names),
    Native(String),
}

struct Resolver<'a> {
    local: &'a Names,
    imports: HashMap<String, Imported<'a>>,
    values: HashMap<String, String>,
    types: HashMap<String, String>,
    scopes: Vec<HashSet<String>>,
    offset: usize,
    span: Span,
}

impl<'a> Resolver<'a> {
    fn add_import(
        &mut self,
        import: &Import,
        names: Option<&'a Names>,
        native: &str,
        id: usize,
    ) -> Result<(), String> {
        let module = import.module.join("/");
        let alias = import
            .alias
            .as_ref()
            .or_else(|| import.module.last())
            .expect("import path");
        if !alias.starts_with('_') {
            let resolved = names.map_or_else(|| Imported::Native(native.into()), Imported::Source);
            if self.imports.insert(alias.clone(), resolved).is_some() {
                return Err(format!("duplicate module alias `{alias}`"));
            }
        }
        for item in &import.unqualified {
            let (name, alias, is_type) = item.names();
            let resolved = if let Some(names) = names {
                Self::exported(names, name, is_type, &module)?.name.clone()
            } else if is_type {
                format!("GI{id}_{alias}")
            } else if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                name.into()
            } else {
                format!("_gi_{id}_{alias}")
            };
            let table = if is_type {
                &mut self.types
            } else {
                &mut self.values
            };
            if table.insert(alias.into(), resolved).is_some() {
                return Err(format!("duplicate unqualified import `{alias}`"));
            }
        }
        Ok(())
    }

    fn exported(
        names: &'a Names,
        name: &str,
        is_type: bool,
        module: &str,
    ) -> Result<&'a Symbol, String> {
        let table = if is_type { &names.types } else { &names.values };
        table
            .get(name)
            .filter(|symbol| symbol.public)
            .ok_or_else(|| {
                format!(
                    "module `{module}` has no public {} `{name}`",
                    if is_type { "type" } else { "value" }
                )
            })
    }

    fn definition(&mut self, definition: &mut Definition) -> Result<(), String> {
        self.span = shifted(definition.span(), self.offset);
        match definition {
            Definition::Fn(function) => {
                function.span = self.span;
                let Some(symbol) = self.local.values.get(&function.name) else {
                    unreachable!()
                };
                function.name.clone_from(&symbol.name);
                self.scopes.push(HashSet::new());
                for arg in &mut function.args {
                    self.bind(&arg.name);
                    self.annotation_opt(&mut arg.type_annotation)?;
                }
                self.annotation_opt(&mut function.return_annotation)?;
                self.expression(&mut function.body)?;
                self.scopes.pop();
            }
            Definition::Constant(constant) => {
                constant.span = self.span;
                constant
                    .name
                    .clone_from(&self.local.values[&constant.name].name);
                self.annotation_opt(&mut constant.annotation)?;
                self.constant(&mut constant.value)?;
            }
            Definition::Type(ty) => {
                ty.span = self.span;
                ty.name.clone_from(&self.local.types[&ty.name].name);
                match &mut ty.kind {
                    TypeDefKind::Alias(annotation) => self.annotation(annotation)?,
                    TypeDefKind::Record(variants) => {
                        for variant in variants {
                            variant
                                .name
                                .clone_from(&self.local.values[&variant.name].name);
                            for field in &mut variant.fields {
                                self.annotation(&mut field.type_annotation)?;
                            }
                        }
                    }
                    TypeDefKind::External => {}
                }
            }
            Definition::Import(_) => unreachable!(),
        }
        Ok(())
    }

    fn annotation_opt(&self, annotation: &mut Option<TypeAnnotation>) -> Result<(), String> {
        if let Some(annotation) = annotation {
            self.annotation(annotation)?;
        }
        Ok(())
    }

    fn annotation(&self, annotation: &mut TypeAnnotation) -> Result<(), String> {
        match annotation {
            TypeAnnotation::Named { module, name, args } => {
                if let Some(alias) = module {
                    match self.imports.get(alias) {
                        Some(Imported::Source(names)) => {
                            name.clone_from(&Self::exported(names, name, true, alias)?.name);
                            *module = None;
                        }
                        Some(Imported::Native(native)) => alias.clone_from(native),
                        None => return Err(format!("unknown module `{alias}`")),
                    }
                } else if let Some(symbol) = self.local.types.get(name) {
                    name.clone_from(&symbol.name);
                } else if let Some(renamed) = self.types.get(name) {
                    name.clone_from(renamed);
                }
                for arg in args {
                    self.annotation(arg)?;
                }
            }
            TypeAnnotation::Fn { args, ret } => {
                for arg in args {
                    self.annotation(arg)?;
                }
                self.annotation(ret)?;
            }
            TypeAnnotation::Tuple(args) => {
                for arg in args {
                    self.annotation(arg)?;
                }
            }
            TypeAnnotation::BitArray { .. } | TypeAnnotation::Hole => {}
        }
        Ok(())
    }

    fn symbol(&self, name: &str) -> Option<&str> {
        self.local
            .values
            .get(name)
            .map(|symbol| symbol.name.as_str())
            .or_else(|| self.values.get(name).map(String::as_str))
    }

    fn bind(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.into());
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| scope.contains(name))
    }

    fn resolve_var(&self, name: &mut String) {
        if !self.is_local(name) {
            if let Some(symbol) = self.symbol(name) {
                *name = symbol.into();
            }
        }
    }

    fn constructor(&self, reference: &mut ConstructorRef) -> Result<(), String> {
        if let Some(alias) = &mut reference.module {
            match self.imports.get(alias) {
                Some(Imported::Source(names)) => {
                    reference
                        .name
                        .clone_from(&Self::exported(names, &reference.name, false, alias)?.name);
                    reference.module = None;
                }
                Some(Imported::Native(native)) => alias.clone_from(native),
                None => return Err(format!("unknown module `{alias}`")),
            }
        } else if let Some(symbol) = self.symbol(&reference.name) {
            reference.name = symbol.into();
        }
        Ok(())
    }

    fn constant(&mut self, value: &mut ConstValue) -> Result<(), String> {
        match value {
            ConstValue::Var(name) => self.resolve_var(name),
            ConstValue::Qualified { module, name } => {
                if let Some(Imported::Source(names)) = self.imports.get(module) {
                    let symbol = Self::exported(names, name, false, module)?;
                    if symbol.constant {
                        *value = ConstValue::Var(symbol.name.clone());
                        return Ok(());
                    }
                }
                return Err(format!("unknown module constant `{module}.{name}`"));
            }
            ConstValue::Concat(left, right) => {
                self.constant(left)?;
                self.constant(right)?;
            }
            ConstValue::Constructor(reference, args) => {
                self.constructor(reference)?;
                for arg in args {
                    self.constant(&mut arg.value)?;
                }
            }
            ConstValue::List { elements, spread } => {
                for element in elements {
                    self.constant(element)?;
                }
                if let Some(spread) = spread {
                    self.constant(spread)?;
                }
            }
            ConstValue::Tuple(values) => {
                for value in values {
                    self.constant(value)?;
                }
            }
            ConstValue::BitArray(array) => {
                for segment in &mut array.segments {
                    self.constant(&mut segment.value)?;
                    self.bit_options(&mut segment.options)?;
                }
            }
            ConstValue::Int(_)
            | ConstValue::Float(_)
            | ConstValue::String(_)
            | ConstValue::Bool(_)
            | ConstValue::Nil => {}
        }
        Ok(())
    }

    fn bit_options(&mut self, options: &mut [BitOption]) -> Result<(), String> {
        for option in options {
            if let BitOption::Size(value) | BitOption::Unit(value) = option {
                self.expression(value)?;
            }
        }
        Ok(())
    }

    fn pattern(&mut self, pattern: &mut Pattern) -> Result<(), String> {
        match pattern {
            Pattern::Var(name) => self.bind(name),
            Pattern::Alias { pattern, name } => {
                self.pattern(pattern)?;
                self.bind(name);
            }
            Pattern::StringPrefix {
                prefix_alias, rest, ..
            } => {
                if let Some(name) = prefix_alias {
                    self.bind(name);
                }
                self.pattern(rest)?;
            }
            Pattern::Constructor(reference, args) => {
                self.constructor(reference)?;
                for arg in args {
                    match arg {
                        PatternArg::Named { pattern, .. } | PatternArg::Unnamed(pattern) => {
                            self.pattern(pattern)?
                        }
                        PatternArg::DiscardRest => {}
                    }
                }
            }
            Pattern::List { elements, rest } => {
                for element in elements {
                    self.pattern(element)?;
                }
                if let Some(rest) = rest {
                    self.pattern(rest)?;
                }
            }
            Pattern::Tuple(elements) | Pattern::Alternative(elements) => {
                for element in elements {
                    self.pattern(element)?;
                }
            }
            Pattern::BitArray(array) => {
                for segment in &mut array.segments {
                    self.bit_options(&mut segment.options)?;
                    self.pattern(&mut segment.value)?;
                }
            }
            Pattern::Discard(_)
            | Pattern::Int(_)
            | Pattern::Float(_)
            | Pattern::String(_)
            | Pattern::Bool(_)
            | Pattern::Nil => {}
        }
        Ok(())
    }

    fn args(&mut self, args: &mut [CallArg]) -> Result<(), String> {
        for arg in args {
            match arg {
                CallArg::Value(value) | CallArg::Labelled { value, .. } => {
                    self.expression(value)?
                }
            }
        }
        Ok(())
    }

    fn qualified_value(&self, module: &mut String, name: &str) -> Result<Option<String>, String> {
        if self.is_local(module) {
            return Ok(None);
        }
        match self.imports.get(module) {
            Some(Imported::Source(names)) => Ok(Some(
                Self::exported(names, name, false, module)?.name.clone(),
            )),
            Some(Imported::Native(native)) => {
                module.clone_from(native);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    fn expression(&mut self, expr: &mut Expression) -> Result<(), String> {
        expr.span = shifted(expr.span, self.offset);
        self.span = expr.span;
        match &mut expr.expr {
            Expr::Var(name) => self.resolve_var(name),
            Expr::FieldAccess { base, label } => {
                if let Expr::Var(module) = &mut base.expr {
                    let original = module.clone();
                    if let Some(symbol) = self.qualified_value(module, label)? {
                        expr.expr = Expr::Var(symbol);
                        return Ok(());
                    }
                    if *module != original {
                        return Ok(());
                    }
                }
                self.expression(base)?;
            }
            Expr::Constructor(reference) => self.constructor(reference)?,
            Expr::List { elements, spread } => {
                for element in elements {
                    self.expression(element)?;
                }
                if let Some(rest) = spread {
                    self.expression(rest)?;
                }
            }
            Expr::Tuple(elements)
            | Expr::Pipeline {
                exprs: elements, ..
            } => {
                for element in elements {
                    self.expression(element)?;
                }
            }
            Expr::Block(elements) => {
                self.scopes.push(HashSet::new());
                for element in elements {
                    self.expression(element)?;
                }
                self.scopes.pop();
            }
            Expr::BitArray(array) => {
                for segment in &mut array.segments {
                    self.expression(&mut segment.value)?;
                    self.bit_options(&mut segment.options)?;
                }
            }
            Expr::Call(call) => {
                call.fun_span = shifted(call.fun_span, self.offset);
                self.expression(&mut call.fun)?;
                self.args(&mut call.args)?;
            }
            Expr::Fn(function) => {
                self.scopes.push(function.args.iter().cloned().collect());
                for annotation in &mut function.annotations {
                    self.annotation_opt(annotation)?;
                }
                self.annotation_opt(&mut function.return_annotation)?;
                self.expression(&mut function.body)?;
                self.scopes.pop();
            }
            Expr::Capture(capture) => {
                self.expression(&mut capture.fun)?;
                for arg in &mut capture.args {
                    if let CaptureArg::Value(value) | CaptureArg::Labelled { value, .. } = arg {
                        self.expression(value)?;
                    }
                }
            }
            Expr::RecordUpdate(update) => {
                self.constructor(&mut update.constructor)?;
                self.expression(&mut update.base)?;
                self.args(&mut update.args)?;
            }
            Expr::Case(case) => {
                for subject in &mut case.subjects {
                    self.expression(subject)?;
                }
                for clause in &mut case.clauses {
                    clause.span = shifted(clause.span, self.offset);
                    self.scopes.push(HashSet::new());
                    for group in &mut clause.alternatives {
                        self.scopes.push(HashSet::new());
                        for pattern in group {
                            self.pattern(pattern)?;
                        }
                        self.scopes.pop();
                    }
                    for pattern in &mut clause.patterns {
                        self.pattern(pattern)?;
                    }
                    if let Some(guard) = &mut clause.guard {
                        self.guard(guard)?;
                    }
                    self.expression(&mut clause.expression)?;
                    self.scopes.pop();
                }
            }
            Expr::Let(let_) => {
                self.expression(&mut let_.value)?;
                self.annotation_opt(&mut let_.annotation)?;
                self.pattern(&mut let_.pattern)?;
            }
            Expr::LetAssert(let_) => {
                self.expression(&mut let_.value)?;
                self.message(&mut let_.message)?;
                self.annotation_opt(&mut let_.annotation)?;
                self.pattern(&mut let_.pattern)?;
            }
            Expr::Assert(assert) => {
                self.expression(&mut assert.value)?;
                self.message(&mut assert.message)?;
            }
            Expr::Todo { message } | Expr::Panic { message } => self.message(message)?,
            Expr::Use(use_) => {
                self.expression(&mut use_.call)?;
                self.scopes.push(HashSet::new());
                for annotation in &mut use_.annotations {
                    self.annotation_opt(annotation)?;
                }
                for pattern in &mut use_.patterns {
                    self.pattern(pattern)?;
                }
                self.expression(&mut use_.body)?;
                self.scopes.pop();
            }
            Expr::Echo(inner) | Expr::Neg(inner) | Expr::Not(inner) => self.expression(inner)?,
            Expr::Binary { lhs, rhs, .. } => {
                self.expression(lhs)?;
                self.expression(rhs)?;
            }
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Nil
            | Expr::EchoPipe => {}
        }
        Ok(())
    }

    fn message(&mut self, message: &mut Option<Box<Expression>>) -> Result<(), String> {
        if let Some(message) = message {
            self.expression(message)?;
        }
        Ok(())
    }

    fn guard(&self, guard: &mut Guard) -> Result<(), String> {
        match guard {
            Guard::Var(name) => self.resolve_var(name),
            Guard::FieldAccess { base, label } => {
                if let Guard::Var(module) = &mut **base {
                    let original = module.clone();
                    if let Some(symbol) = self.qualified_value(module, label)? {
                        *guard = Guard::Var(symbol);
                        return Ok(());
                    }
                    if *module != original {
                        return Ok(());
                    }
                }
                self.guard(base)?;
            }
            Guard::Call { fun, args } => {
                self.guard(fun)?;
                for arg in args {
                    self.guard(arg)?;
                }
            }
            Guard::Binary { lhs, rhs, .. } => {
                self.guard(lhs)?;
                self.guard(rhs)?;
            }
            Guard::Not(inner) => self.guard(inner)?,
            Guard::Int(_) | Guard::Float(_) | Guard::String(_) | Guard::Bool(_) | Guard::Nil => {}
        }
        Ok(())
    }
}
