use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostics::Diagnostic;

use super::ty::Ty;

/// Type environment: maps names to types, scoped for let-bindings within rules.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    /// Top-level type definitions: name → Ty
    pub type_defs: HashMap<String, Ty>,
    /// Variable scope stack (innermost last)
    scopes: Vec<HashMap<String, Ty>>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            type_defs: HashMap::new(),
            scopes: vec![HashMap::new()],
        }
    }

    /// First pass: collect all type definitions from the source file.
    pub fn collect_type_defs(&mut self, source: &SourceFile, diagnostics: &mut Vec<Diagnostic>) {
        for item in &source.items {
            if let Item::TypeDef(td) = &item.node {
                let ty = self.resolve_type_def(td, diagnostics);
                self.type_defs.insert(td.name.node.clone(), ty);
            }
        }
    }

    fn resolve_type_def(&self, td: &TypeDef, diagnostics: &mut Vec<Diagnostic>) -> Ty {
        match &td.kind {
            TypeDefKind::Alias(type_expr) => self.resolve_type_expr(&type_expr.node, diagnostics),
            TypeDefKind::Record(fields) => Ty::Record {
                name: td.name.node.clone(),
                fields: fields
                    .iter()
                    .map(|f| {
                        (
                            f.name.node.clone(),
                            self.resolve_type_expr(&f.ty.node, diagnostics),
                        )
                    })
                    .collect(),
            },
            TypeDefKind::Enum(variants) => Ty::Enum {
                name: td.name.node.clone(),
                variants: variants
                    .iter()
                    .map(|v| {
                        (
                            v.name.node.clone(),
                            v.fields
                                .iter()
                                .map(|f| self.resolve_type_expr(&f.node, diagnostics))
                                .collect(),
                        )
                    })
                    .collect(),
            },
        }
    }

    /// Resolve an AST TypeExpr to an internal Ty.
    pub fn resolve_type_expr(&self, te: &TypeExpr, diagnostics: &mut Vec<Diagnostic>) -> Ty {
        match te {
            TypeExpr::Named { name, args } => {
                // Built-in types
                match name.as_str() {
                    "Bits" => {
                        if args.len() == 1 {
                            if let TypeExpr::Named {
                                name: width_str,
                                args: inner_args,
                            } = &args[0].node
                                && inner_args.is_empty()
                                && let Ok(n) = width_str.parse::<u64>()
                            {
                                return Ty::Bits(n);
                            }
                            diagnostics.push(Diagnostic::error(
                                args[0].span.clone(),
                                "Bits requires an integer width",
                            ));
                            Ty::Error
                        } else {
                            Ty::Bits(32) // default
                        }
                    }
                    "Bool" => Ty::Bool,
                    "Array" if args.len() == 2 => {
                        let size = if let TypeExpr::Named {
                            name: size_str,
                            args: inner_args,
                        } = &args[0].node
                        {
                            if inner_args.is_empty() {
                                size_str.parse::<u64>().unwrap_or(0)
                            } else {
                                0
                            }
                        } else {
                            0
                        };
                        let elem = self.resolve_type_expr(&args[1].node, diagnostics);
                        Ty::Array {
                            elem: Box::new(elem),
                            size,
                        }
                    }
                    _ => {
                        // Look up user-defined type
                        if let Some(ty) = self.type_defs.get(name) {
                            ty.clone()
                        } else {
                            // Return named reference — may resolve later
                            Ty::Named(name.clone())
                        }
                    }
                }
            }
            TypeExpr::Product(parts) => {
                let tys: Vec<Ty> = parts
                    .iter()
                    .map(|p| self.resolve_type_expr(&p.node, diagnostics))
                    .collect();
                Ty::Tuple(tys)
            }
            TypeExpr::Queue { elem, depth } => Ty::Queue {
                elem: Box::new(self.resolve_type_expr(&elem.node, diagnostics)),
                depth: *depth,
            },
            TypeExpr::Cell { elem, .. } => Ty::Cell {
                elem: Box::new(self.resolve_type_expr(&elem.node, diagnostics)),
            },
            TypeExpr::AsyncQueue { elem, depth } => Ty::AsyncQueue {
                elem: Box::new(self.resolve_type_expr(&elem.node, diagnostics)),
                depth: *depth,
            },
        }
    }

    /// Push a new variable scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the current variable scope.
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Define a variable in the current scope.
    pub fn define(&mut self, name: String, ty: Ty) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    /// Look up a variable, searching from innermost scope outward.
    pub fn lookup(&self, name: &str) -> Option<&Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }
}
