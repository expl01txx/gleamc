use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub(crate) fn constants(module: &Module) -> Result<Vec<&ConstantDef>, &ConstantDef> {
    let definitions: Vec<_> = module
        .definitions
        .iter()
        .filter_map(|definition| match definition {
            Definition::Constant(constant) => Some(constant),
            _ => None,
        })
        .collect();
    let names: HashMap<_, _> = definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.name.as_str(), index))
        .collect();
    let graph: Vec<_> = definitions
        .iter()
        .map(|definition| {
            let mut refs = Vec::new();
            constant_refs(&definition.value, &mut refs);
            refs.into_iter()
                .filter_map(|name| names.get(name).copied())
                .collect()
        })
        .collect();
    dependency_groups(&graph)
        .into_iter()
        .map(|indices| {
            let index = indices[0];
            if indices.len() > 1 || graph[index].contains(&index) {
                Err(definitions[index])
            } else {
                Ok(definitions[index])
            }
        })
        .collect()
}

fn constant_refs<'a>(value: &'a ConstValue, refs: &mut Vec<&'a str>) {
    match value {
        ConstValue::Var(name) => refs.push(name),
        ConstValue::Tuple(values) => {
            for value in values {
                constant_refs(value, refs);
            }
        }
        ConstValue::Constructor(_, args) => {
            for arg in args {
                constant_refs(&arg.value, refs);
            }
        }
        ConstValue::List { elements, spread } => {
            for value in elements.iter().chain(spread.as_deref()) {
                constant_refs(value, refs);
            }
        }
        ConstValue::Concat(left, right) => {
            constant_refs(left, refs);
            constant_refs(right, refs);
        }
        ConstValue::BitArray(array) => {
            for segment in &array.segments {
                constant_refs(&segment.value, refs);
            }
        }
        ConstValue::Int(_)
        | ConstValue::Float(_)
        | ConstValue::Bool(_)
        | ConstValue::String(_)
        | ConstValue::Nil
        | ConstValue::Qualified { .. } => {}
    }
}

pub(crate) fn functions(module: &Module) -> Vec<Vec<&FnDef>> {
    let definitions: Vec<_> = module
        .definitions
        .iter()
        .filter_map(|definition| match definition {
            Definition::Fn(function) => Some(function),
            _ => None,
        })
        .collect();
    let names: HashMap<_, _> = definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.name.as_str(), index))
        .collect();
    let graph: Vec<_> = definitions
        .iter()
        .map(|definition| {
            let mut refs = FunctionRefs {
                names: &names,
                scopes: vec![definition.args.iter().map(|arg| arg.name.clone()).collect()],
                edges: Vec::new(),
            };
            refs.expression(&definition.body);
            refs.edges.sort_unstable();
            refs.edges.dedup();
            refs.edges
        })
        .collect();
    dependency_groups(&graph)
        .into_iter()
        .map(|indices| {
            indices
                .into_iter()
                .map(|index| definitions[index])
                .collect()
        })
        .collect()
}

fn dependency_groups(graph: &[Vec<usize>]) -> Vec<Vec<usize>> {
    fn visit(start: usize, graph: &[Vec<usize>], seen: &mut [bool], order: &mut Vec<usize>) {
        if std::mem::replace(&mut seen[start], true) {
            return;
        }
        for &next in &graph[start] {
            visit(next, graph, seen, order);
        }
        order.push(start);
    }

    let mut reversed = vec![Vec::new(); graph.len()];
    for (index, dependencies) in graph.iter().enumerate() {
        for &dep in dependencies {
            reversed[dep].push(index);
        }
    }
    let mut order = Vec::with_capacity(graph.len());
    let mut seen = vec![false; graph.len()];
    for index in 0..graph.len() {
        visit(index, &reversed, &mut seen, &mut order);
    }

    let mut groups = Vec::new();
    seen.fill(false);
    for index in order.into_iter().rev() {
        if !seen[index] {
            let mut group = Vec::new();
            visit(index, graph, &mut seen, &mut group);
            groups.push(group);
        }
    }
    groups
}

struct FunctionRefs<'a> {
    names: &'a HashMap<&'a str, usize>,
    scopes: Vec<HashSet<String>>,
    edges: Vec<usize>,
}

impl FunctionRefs<'_> {
    fn expression(&mut self, expression: &Expression) {
        match &expression.expr {
            Expr::Var(name) => {
                if let Some(&index) = self.names.get(name.as_str()) {
                    if !self.scopes.iter().any(|scope| scope.contains(name)) {
                        self.edges.push(index);
                    }
                }
            }
            Expr::FieldAccess { base, .. }
            | Expr::Neg(base)
            | Expr::Not(base)
            | Expr::Echo(base) => self.expression(base),
            Expr::List { elements, spread } => {
                for element in elements.iter().chain(spread.as_deref()) {
                    self.expression(element);
                }
            }
            Expr::Tuple(elements)
            | Expr::Pipeline {
                exprs: elements, ..
            } => {
                for element in elements {
                    self.expression(element);
                }
            }
            Expr::BitArray(array) => {
                for segment in &array.segments {
                    self.expression(&segment.value);
                    self.bit_options(&segment.options);
                }
            }
            Expr::Call(call) => {
                self.expression(&call.fun);
                self.args(&call.args);
            }
            Expr::Capture(capture) => {
                self.expression(&capture.fun);
                for arg in &capture.args {
                    if let CaptureArg::Value(value) | CaptureArg::Labelled { value, .. } = arg {
                        self.expression(value);
                    }
                }
            }
            Expr::RecordUpdate(update) => {
                self.expression(&update.base);
                self.args(&update.args);
            }
            Expr::Fn(function) => {
                self.scopes.push(function.args.iter().cloned().collect());
                self.expression(&function.body);
                self.scopes.pop();
            }
            Expr::Case(case) => {
                for subject in &case.subjects {
                    self.expression(subject);
                }
                for clause in &case.clauses {
                    for group in clause.pattern_groups() {
                        self.scopes.push(HashSet::new());
                        for pattern in group {
                            self.pattern(pattern);
                        }
                        if let Some(guard) = &clause.guard {
                            self.expression(&guard.to_expression(clause.span));
                        }
                        self.expression(&clause.expression);
                        self.scopes.pop();
                    }
                }
            }
            Expr::Block(elements) => {
                self.scopes.push(HashSet::new());
                for element in elements {
                    self.expression(element);
                }
                self.scopes.pop();
            }
            Expr::Let(let_) => {
                self.expression(&let_.value);
                self.pattern(&let_.pattern);
            }
            Expr::LetAssert(assert) => {
                self.expression(&assert.value);
                self.message(assert.message.as_deref());
                self.pattern(&assert.pattern);
            }
            Expr::Assert(assert) => {
                self.expression(&assert.value);
                self.message(assert.message.as_deref());
            }
            Expr::Use(use_) => {
                self.expression(&use_.call);
                self.scopes.push(HashSet::new());
                for pattern in &use_.patterns {
                    self.pattern(pattern);
                }
                self.expression(&use_.body);
                self.scopes.pop();
            }
            Expr::Todo { message } | Expr::Panic { message } => self.message(message.as_deref()),
            Expr::Binary { lhs, rhs, .. } => {
                self.expression(lhs);
                self.expression(rhs);
            }
            Expr::Constructor(_)
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Nil
            | Expr::EchoPipe => {}
        }
    }

    fn message(&mut self, message: Option<&Expression>) {
        if let Some(message) = message {
            self.expression(message);
        }
    }

    fn args(&mut self, args: &[CallArg]) {
        for arg in args {
            match arg {
                CallArg::Value(value) | CallArg::Labelled { value, .. } => self.expression(value),
            }
        }
    }

    fn bit_options(&mut self, options: &[BitOption]) {
        for option in options {
            if let BitOption::Size(expr) | BitOption::Unit(expr) = option {
                self.expression(expr);
            }
        }
    }

    fn bind(&mut self, name: &str) {
        self.scopes
            .last_mut()
            .expect("function scope")
            .insert(name.into());
    }

    fn pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Var(name) => self.bind(name),
            Pattern::Alias { name, pattern } => {
                self.pattern(pattern);
                self.bind(name);
            }
            Pattern::StringPrefix {
                prefix_alias, rest, ..
            } => {
                if let Some(name) = prefix_alias {
                    self.bind(name);
                }
                self.pattern(rest);
            }
            Pattern::Constructor(_, args) => {
                for arg in args {
                    match arg {
                        PatternArg::Named { pattern, .. } | PatternArg::Unnamed(pattern) => {
                            self.pattern(pattern)
                        }
                        PatternArg::DiscardRest => {}
                    }
                }
            }
            Pattern::List { elements, rest } => {
                for pattern in elements.iter().chain(rest.as_deref()) {
                    self.pattern(pattern);
                }
            }
            Pattern::Tuple(elements) | Pattern::Alternative(elements) => {
                for element in elements {
                    self.pattern(element);
                }
            }
            Pattern::BitArray(array) => {
                for segment in &array.segments {
                    self.bit_options(&segment.options);
                    self.pattern(&segment.value);
                }
            }
            Pattern::Discard(_)
            | Pattern::Int(_)
            | Pattern::Float(_)
            | Pattern::String(_)
            | Pattern::Bool(_)
            | Pattern::Nil => {}
        }
    }
}
