use crate::IndexSet;

use super::*;

impl Module {
    pub(crate) fn new_from_syntax(name: &str, syntax: &SyntaxNode) -> Option<Module> {
        Rhai::cast(syntax.clone()).map(|rhai| {
            let mut m = Module {
                name: name.into(),
                syntax: Some(syntax.into()),
                ..Module::default()
            };
            let root_scope = m.create_scope(None, Some(syntax.into()));
            m.root_scope = root_scope;
            m.add_statements(root_scope, rhai.statements());
            m
        })
    }

    #[tracing::instrument(skip(self), level = "trace")]
    fn create_scope(&mut self, parent_symbol: Option<Symbol>, syntax: Option<SyntaxInfo>) -> Scope {
        let data = ScopeData {
            syntax,
            parent_symbol,
            ..ScopeData::default()
        };
        self.scopes.insert(data)
    }

    fn add_statements(&mut self, scope: Scope, statements: impl Iterator<Item = Stmt>) {
        for statement in statements {
            self.add_statement(scope, statement);
        }
    }

    #[tracing::instrument(skip(self), level = "trace")]
    fn add_statement(&mut self, scope: Scope, stmt: Stmt) -> Option<Symbol> {
        stmt.item().and_then(|item| {
            item.expr()
                .and_then(|expr| match self.add_expression(scope, expr) {
                    Some(symbol) => {
                        match &mut self.symbol_unchecked_mut(symbol).kind {
                            SymbolKind::Fn(f) => f.docs = item.docs_content(),
                            SymbolKind::Decl(decl) => decl.docs = item.docs_content(),
                            _ => {}
                        };
                        Some(symbol)
                    }
                    None => None,
                })
        })
    }

    #[tracing::instrument(skip(self), level = "trace")]
    fn add_expression(&mut self, scope: Scope, expr: Expr) -> Option<Symbol> {
        match expr {
            Expr::Ident(expr) => {
                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: Some(
                        expr.ident_token()
                            .map_or_else(|| expr.syntax().into(), |t| t.into()),
                    ),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Reference(ReferenceSymbol {
                        name: expr
                            .ident_token()
                            .map(|s| s.text().to_string())
                            .unwrap_or_default(),
                        ..ReferenceSymbol::default()
                    }),
                    parent_scope: Scope::default(),
                });

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Path(expr_path) => {
                let segments = match expr_path.path() {
                    Some(p) => p.segments(),
                    None => return None,
                };
                let symbol = SymbolData {
                    parent_scope: Scope::default(),
                    syntax: Some(expr_path.syntax().into()),
                    selection_syntax: None,
                    kind: SymbolKind::Path(PathSymbol {
                        segments: segments
                            .map(|s| {
                                let symbol = self.symbols.insert(SymbolData {
                                    selection_syntax: Some(s.clone().into()),
                                    parent_scope: Scope::default(),
                                    kind: SymbolKind::Reference(ReferenceSymbol {
                                        name: s.text().to_string(),
                                        part_of_path: true,
                                        ..ReferenceSymbol::default()
                                    }),
                                    syntax: Some(s.into()),
                                });
                                self.add_to_scope(scope, symbol, false);
                                symbol
                            })
                            .collect(),
                    }),
                };
                let sym = self.symbols.insert(symbol);

                self.add_to_scope(scope, sym, false);
                Some(sym)
            }
            Expr::Lit(expr) => {
                let symbol = self.symbols.insert(SymbolData {
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    selection_syntax: None,
                    kind: SymbolKind::Lit(LitSymbol {
                        ..LitSymbol::default()
                    }),
                });

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            // `let` and `const` values have a separate scope created for them
            Expr::Let(expr) => {
                let value = expr.expr().map(|expr| {
                    let scope = self.create_scope(None, Some(expr.syntax().into()));
                    self.add_expression(scope, expr);
                    scope
                });

                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: expr.ident_token().map(Into::into),
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Decl(DeclSymbol {
                        name: expr
                            .ident_token()
                            .map(|s| s.text().to_string())
                            .unwrap_or_default(),
                        value,
                        ..DeclSymbol::default()
                    }),
                });

                if let Some(value_scope) = value {
                    self.set_as_parent_symbol(symbol, value_scope);
                }

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Const(expr) => {
                let value = expr.expr().map(|expr| {
                    let scope = self.create_scope(None, Some(expr.syntax().into()));
                    self.add_expression(scope, expr);
                    scope
                });

                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: expr.ident_token().map(Into::into),
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Decl(DeclSymbol {
                        name: expr
                            .ident_token()
                            .map(|s| s.text().to_string())
                            .unwrap_or_default(),
                        is_const: true,
                        value,
                        ..DeclSymbol::default()
                    }),
                });

                if let Some(value_scope) = value {
                    self.set_as_parent_symbol(symbol, value_scope);
                }

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Block(expr) => {
                let block_scope = self.create_scope(None, Some(expr.syntax().into()));

                let symbol = self.symbols.insert(SymbolData {
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    selection_syntax: None,
                    kind: SymbolKind::Block(BlockSymbol { scope }),
                });

                self.set_as_parent_symbol(symbol, block_scope);
                self.add_statements(block_scope, expr.statements());

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Unary(expr) => {
                let rhs = expr.expr().and_then(|rhs| self.add_expression(scope, rhs));

                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Unary(UnarySymbol {
                        op: expr.op_token().map(|t| t.kind()),
                        rhs,
                    }),
                });

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Binary(expr) => {
                let lhs = expr.lhs().and_then(|lhs| self.add_expression(scope, lhs));

                let rhs = expr.rhs().and_then(|rhs| self.add_expression(scope, rhs));

                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Binary(BinarySymbol {
                        rhs,
                        op: expr.op_token().map(|t| t.kind()),
                        lhs,
                    }),
                });

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Paren(expr) => expr
                .expr()
                .and_then(|expr| self.add_expression(scope, expr)),
            Expr::Array(expr) => {
                let symbol_data = SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Array(ArraySymbol {
                        values: expr
                            .values()
                            .filter_map(|expr| self.add_expression(scope, expr))
                            .collect(),
                    }),
                };

                let symbol = self.symbols.insert(symbol_data);

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Index(expr) => {
                let base = expr
                    .base()
                    .and_then(|base| self.add_expression(scope, base));

                let index = expr
                    .index()
                    .and_then(|index| self.add_expression(scope, index));

                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Index(IndexSymbol { base, index }),
                });

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Object(expr) => {
                let symbol_data = SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Object(ObjectSymbol {
                        fields: expr
                            .fields()
                            .filter_map(|field| match (field.property(), field.expr()) {
                                (Some(name), Some(expr)) => Some((
                                    name.text().to_string(),
                                    ObjectField {
                                        property_name: name.text().to_string(),
                                        property_syntax: Some(name.into()),
                                        field_syntax: Some(field.syntax().into()),
                                        value: self.add_expression(scope, expr),
                                    },
                                )),
                                _ => None,
                            })
                            .collect(),
                    }),
                };

                let symbol = self.symbols.insert(symbol_data);
                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Call(expr) => {
                let lhs = expr
                    .expr()
                    .and_then(|expr| self.add_expression(scope, expr));

                let symbol_data = SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Call(CallSymbol {
                        lhs,
                        arguments: match expr.arg_list() {
                            Some(arg_list) => arg_list
                                .arguments()
                                .filter_map(|expr| self.add_expression(scope, expr))
                                .collect(),
                            None => IndexSet::default(),
                        },
                    }),
                };

                let symbol = self.symbols.insert(symbol_data);
                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Closure(expr) => {
                let closure_scope = self.create_scope(None, Some(expr.syntax().into()));

                if let Some(param_list) = expr.param_list() {
                    for param in param_list.params() {
                        let symbol = self.symbols.insert(SymbolData {
                            selection_syntax: Some(param.syntax().into()),
                            syntax: Some(param.syntax().into()),
                            parent_scope: Scope::default(),
                            kind: SymbolKind::Decl(DeclSymbol {
                                name: param
                                    .ident_token()
                                    .map(|s| s.text().to_string())
                                    .unwrap_or_default(),
                                is_param: true,
                                ..DeclSymbol::default()
                            }),
                        });

                        self.add_to_scope(closure_scope, symbol, false);
                    }
                }

                let closure_expr_symbol = expr
                    .body()
                    .and_then(|body| self.add_expression(closure_scope, body));

                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Closure(ClosureSymbol {
                        scope,
                        expr: closure_expr_symbol,
                    }),
                });

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::If(expr) => {
                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::If(IfSymbol::default()),
                });

                // Here we flatten the branches of the `if` expression
                // from the recursive syntax tree.
                let mut next_branch = Some(expr);

                while let Some(branch) = next_branch.take() {
                    let branch_condition = branch
                        .expr()
                        .and_then(|expr| self.add_expression(scope, expr));

                    let then_scope = self
                        .create_scope(None, branch.then_branch().map(|body| body.syntax().into()));
                    self.set_as_parent_symbol(symbol, then_scope);

                    if let Some(body) = branch.then_branch() {
                        self.add_statements(then_scope, body.statements());
                    }

                    self.symbol_unchecked_mut(symbol)
                        .kind
                        .as_if_mut()
                        .unwrap()
                        .branches
                        .push((branch_condition, then_scope));

                    // trailing `else` branch
                    if let Some(else_body) = branch.else_branch() {
                        let then_scope = self.create_scope(None, Some(else_body.syntax().into()));
                        self.set_as_parent_symbol(symbol, then_scope);
                        self.add_statements(then_scope, else_body.statements());
                        self.symbol_unchecked_mut(symbol)
                            .kind
                            .as_if_mut()
                            .unwrap()
                            .branches
                            .push((None, then_scope));
                        break;
                    }

                    next_branch = branch.else_if_branch();
                }

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Loop(expr) => {
                let loop_scope =
                    self.create_scope(None, expr.loop_body().map(|body| body.syntax().into()));

                if let Some(body) = expr.loop_body() {
                    self.add_statements(loop_scope, body.statements());
                }

                let symbol = self.symbols.insert(SymbolData {
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    selection_syntax: None,
                    kind: SymbolKind::Loop(LoopSymbol { scope: loop_scope }),
                });

                self.set_as_parent_symbol(symbol, loop_scope);

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::For(expr) => {
                let for_scope =
                    self.create_scope(None, expr.loop_body().map(|body| body.syntax().into()));

                if let Some(pat) = expr.pat() {
                    for ident in pat.idents() {
                        let ident_symbol = self.symbols.insert(SymbolData {
                            syntax: Some(ident.clone().into()),
                            selection_syntax: Some(ident.clone().into()),
                            parent_scope: Scope::default(),
                            kind: SymbolKind::Decl(DeclSymbol {
                                name: ident.text().into(),
                                docs: String::new(),
                                is_pat: true,
                                ..DeclSymbol::default()
                            }),
                        });
                        self.add_to_scope(for_scope, ident_symbol, false);
                    }
                }

                if let Some(body) = expr.loop_body() {
                    self.add_statements(for_scope, body.statements());
                }

                let sym = SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::For(ForSymbol {
                        iterable: expr
                            .iterable()
                            .and_then(|expr| self.add_expression(scope, expr)),
                        scope,
                    }),
                };

                let symbol = self.symbols.insert(sym);
                self.set_as_parent_symbol(symbol, for_scope);
                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::While(expr) => {
                let while_scope =
                    self.create_scope(None, expr.loop_body().map(|body| body.syntax().into()));

                if let Some(body) = expr.loop_body() {
                    self.add_statements(while_scope, body.statements());
                }

                let symbol_data = SymbolData {
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    selection_syntax: None,
                    kind: SymbolKind::While(WhileSymbol {
                        scope: while_scope,
                        condition: expr
                            .expr()
                            .and_then(|expr| self.add_expression(scope, expr)),
                    }),
                };

                let symbol = self.symbols.insert(symbol_data);

                self.set_as_parent_symbol(symbol, while_scope);

                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Break(expr) => {
                let symbol_data = SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Break(BreakSymbol {
                        expr: expr
                            .expr()
                            .and_then(|expr| self.add_expression(scope, expr)),
                    }),
                };

                let symbol = self.symbols.insert(symbol_data);
                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Continue(expr) => {
                let symbol_data = SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Continue(ContinueSymbol {}),
                };

                let symbol = self.symbols.insert(symbol_data);
                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Switch(expr) => {
                let target = expr
                    .expr()
                    .and_then(|expr| self.add_expression(scope, expr));

                let arms = expr
                    .switch_arm_list()
                    .map(|arm_list| {
                        arm_list
                            .arms()
                            .map(|arm| {
                                let mut left = None;
                                let mut right = None;

                                if let Some(discard) = arm.discard_token() {
                                    left = Some(self.symbols.insert(SymbolData {
                                        syntax: Some(discard.into()),
                                        selection_syntax: None,
                                        parent_scope: Scope::default(),
                                        kind: SymbolKind::Discard(DiscardSymbol {}),
                                    }));
                                }

                                if let Some(expr) = arm.pattern_expr() {
                                    left = self.add_expression(scope, expr);
                                }

                                if let Some(expr) = arm.value_expr() {
                                    right = self.add_expression(scope, expr);
                                }

                                (left, right)
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let symbol = self.symbols.insert(SymbolData {
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    selection_syntax: None,
                    kind: SymbolKind::Switch(SwitchSymbol { target, arms }),
                });

                self.add_to_scope(scope, symbol, true);
                Some(symbol)
            }
            Expr::Return(expr) => {
                let symbol_data = SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Return(ReturnSymbol {
                        expr: expr
                            .expr()
                            .and_then(|expr| self.add_expression(scope, expr)),
                    }),
                };

                let symbol = self.symbols.insert(symbol_data);
                self.add_to_scope(scope, symbol, false);
                Some(symbol)
            }
            Expr::Fn(expr) => {
                let fn_scope = self.create_scope(None, Some(expr.syntax().into()));

                if let Some(param_list) = expr.param_list() {
                    for param in param_list.params() {
                        let symbol = self.symbols.insert(SymbolData {
                            selection_syntax: param.ident_token().map(Into::into),
                            syntax: Some(param.syntax().into()),
                            parent_scope: Scope::default(),
                            kind: SymbolKind::Decl(DeclSymbol {
                                name: param
                                    .ident_token()
                                    .map(|s| s.text().to_string())
                                    .unwrap_or_default(),
                                is_param: true,
                                ..DeclSymbol::default()
                            }),
                        });

                        self.add_to_scope(fn_scope, symbol, false);
                    }
                }

                if let Some(body) = expr.body() {
                    self.add_statements(scope, body.statements());
                }
                let symbol = self.symbols.insert(SymbolData {
                    selection_syntax: expr.ident_token().map(Into::into),
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Fn(FnSymbol {
                        name: expr
                            .ident_token()
                            .map(|s| s.text().to_string())
                            .unwrap_or_default(),
                        scope: fn_scope,
                        ..FnSymbol::default()
                    }),
                });

                self.add_to_scope(scope, symbol, true);
                self.set_as_parent_symbol(symbol, fn_scope);
                Some(symbol)
            }
            Expr::Import(expr) => {
                let symbol_data = SymbolData {
                    selection_syntax: None,
                    parent_scope: Scope::default(),
                    syntax: Some(expr.syntax().into()),
                    kind: SymbolKind::Import(ImportSymbol {
                        alias: expr.alias().map(|alias| {
                            self.symbols.insert(SymbolData {
                                selection_syntax: Some(alias.clone().into()),
                                syntax: Some(alias.clone().into()),
                                kind: SymbolKind::Decl(DeclSymbol {
                                    name: alias.text().into(),
                                    ..DeclSymbol::default()
                                }),
                                parent_scope: Scope::default(),
                            })
                        }),
                        expr: expr
                            .expr()
                            .and_then(|expr| self.add_expression(scope, expr)),
                    }),
                };

                let symbol = self.symbols.insert(symbol_data);

                self.add_to_scope(scope, symbol, true);
                Some(symbol)
            }
        }
    }

    fn add_to_scope(&mut self, scope: Scope, symbol: Symbol, hoist: bool) {
        let s = self.scope_unchecked_mut(scope);
        debug_assert!(!s.symbols.contains(&symbol));
        debug_assert!(!s.hoisted_symbols.contains(&symbol));

        if hoist {
            s.hoisted_symbols.insert(symbol);
        } else {
            s.symbols.insert(symbol);
        }

        let sym_data = self.symbol_unchecked_mut(symbol);

        debug_assert!(sym_data.parent_scope == Scope::default());

        sym_data.parent_scope = scope;

        tracing::debug!(
            symbol_kind = Into::<&'static str>::into(&sym_data.kind),
            hoist,
            ?scope,
            ?symbol,
            "added symbol to scope"
        );
    }

    fn set_as_parent_symbol(&mut self, symbol: Symbol, scope: Scope) {
        let s = self.scope_unchecked_mut(scope);
        debug_assert!(s.parent_symbol.is_none());
        s.parent_symbol = Some(symbol);

        tracing::debug!(
            symbol_kind = Into::<&'static str>::into(&self.symbol_unchecked(symbol).kind),
            ?scope,
            ?symbol,
            "set parent symbol of scope"
        );
    }
}

impl Module {
    pub(crate) fn resolve_references(&mut self) {
        let self_ptr = self as *mut Module;

        for (symbol, ref_kind) in self
            .symbols
            .iter_mut()
            .filter_map(|(s, d)| match &mut d.kind {
                SymbolKind::Reference(r) => Some((s, r)),
                _ => None,
            })
        {
            ref_kind.target = None;

            // safety: This is safe because we only operate
            //  on separate elements (declarations and refs)
            //  and we don't touch the map itself.
            //
            // Without this unsafe block, we'd have to unnecessarily
            // allocate a vector of symbols.
            unsafe {
                for vis_symbol in (&*self_ptr).visible_symbols_from_symbol(symbol) {
                    let vis_symbol_data = (*self_ptr).symbols.get_unchecked_mut(vis_symbol);
                    if let Some(n) = vis_symbol_data.name() {
                        if n != ref_kind.name {
                            continue;
                        }
                    }

                    match &mut vis_symbol_data.kind {
                        SymbolKind::Fn(target) => {
                            target.references.insert(symbol);
                        }
                        SymbolKind::Decl(target) => {
                            target.references.insert(symbol);
                        }
                        _ => unreachable!(),
                    }

                    ref_kind.target = Some(ReferenceTarget::Symbol(vis_symbol));
                    break;
                }
            }
        }
    }
}