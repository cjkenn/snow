use ast::Ast;
use symtab::SymbolTable;
use sym::{Sym, SymTy};
use lexer::Lexer;
use token::{Token, TknTy};
use ty_rec::{TyName, TyRec};
use error::KolgaErr;
use error::parse::{ParseErrTy, ParseErr};
use std::collections::HashMap;
use std::rc::Rc;

const FN_PARAM_MAX_LEN: usize = 64;

pub struct ParserResult {
    /// The resulting AST from parsing
    pub ast: Option<Box<Ast>>,

    /// Vector of any parser errors
    pub error: Vec<ParseErr>
}

impl ParserResult {
    pub fn new() -> ParserResult {
        ParserResult {
            ast: None,
            error: Vec::new()
        }
    }
}

pub struct Parser<'l, 's> {
    /// Reference to the lexer needed to get characters from the file
    lexer: &'l mut Lexer,
    symtab: &'s mut SymbolTable,
    errors: Vec<ParseErr>,
    currtkn: Token
}

impl<'l, 's> Parser<'l, 's> {
    pub fn new(lex: &'l mut Lexer, symt: &'s mut SymbolTable) -> Parser<'l, 's> {
        let firsttkn = lex.lex();

        Parser {
            lexer: lex,
            symtab: symt,
            errors: Vec::new(),
            currtkn: firsttkn
        }
    }

    /// Main entry point to the recursive descent parser. Calling this method will parse the entire
    /// file and return a result containing the AST and any parsing errors encountered.
    /// The error vector should be checked after parsing, and any errors should
    /// be handled before continuing to future compiler passes.
    pub fn parse(&mut self) -> ParserResult {
        let mut stmts: Vec<Ast> = Vec::new();

        while self.currtkn.ty != TknTy::Eof {
            match self.decl() {
                Ok(a) => stmts.push(a),
                Err(e) => {
                    e.emit();
                    match e.continuable() {
                        true => (),
                        false => break
                    };
                }
            }
        }

        // Finalize the global scope to access scopes in future passes.
        self.symtab.finalize_global_sc();

        let head = Ast::Prog{stmts: stmts};
        ParserResult {
            ast: Some(Box::new(head)),
            error: self.errors.clone()
        }
    }

    /// Parses a declaration. In kolga we can declare variables, functions, and classes.
    fn decl(&mut self) -> Result<Ast, ParseErr> {
        match self.currtkn.ty {
            TknTy::Let => self.var_decl(),
            TknTy::Fn => self.fn_decl(),
            TknTy::Class => self.class_decl(),
            _ => self.stmt()
        }
    }

    /// Parses a variable declaration
    fn var_decl(&mut self) -> Result<Ast, ParseErr> {
        self.expect(TknTy::Let)?;

        let is_imm = match self.currtkn.ty {
            TknTy::Imm => {
                self.consume();
                true
            },
            _ => false
        };

        let ident_tkn = self.match_ident_tkn();
        self.expect(TknTy::Tilde)?;

        let mut is_class_type = false;
        let mut var_err = None;

        let var_ty_tkn = if self.currtkn.is_ty() {
            // But Void isn't a valid type for a variable, just a function that returns nothing
            if self.currtkn.ty == TknTy::Void {
                let ty_str = self.currtkn.ty.to_string();
                return Err(self.error(ParseErrTy::InvalidTy(ty_str)));
            }

            let tkn = Some(self.currtkn.clone());
            self.consume();
            tkn
        } else {
            let ty_name = self.currtkn.get_name();
            let maybe_class_sym = self.symtab.retrieve(&ty_name);
            if maybe_class_sym.is_none() {
                let ty_str = self.currtkn.ty.to_string();
                var_err = Some(self.error(ParseErrTy::InvalidTy(ty_str)));
                None
            } else if maybe_class_sym.unwrap().sym_ty == SymTy::Class {
                is_class_type = true;
                let tkn = Some(self.currtkn.clone());
                self.consume();
                tkn
            } else {
                let ty_str = self.currtkn.ty.to_string();
                var_err = Some(self.error(ParseErrTy::InvalidTy(ty_str)));
                None
            }
        };

        if var_ty_tkn.is_none() {
            return Err(var_err.unwrap());
        }

        match self.currtkn.ty {
            TknTy::Eq => {
                self.consume();
                let var_val = self.expr()?;
                self.expect(TknTy::Semicolon)?;

                let ty_rec = TyRec::new_from_tkn(var_ty_tkn.unwrap());
                let sym = Sym::new(SymTy::Var,
                                   is_imm,
                                   ty_rec.clone(),
                                   ident_tkn.clone().unwrap(),
                                   Some(var_val.clone()),
                                   None);

                let name = &ident_tkn.clone().unwrap().get_name();
                self.symtab.store(name, sym);

                Ok(Ast::VarAssignExpr {
                    ty_rec: ty_rec,
                    ident_tkn: ident_tkn.unwrap(),
                    is_imm: is_imm,
                    is_global: self.symtab.is_global(),
                    value: Box::new(var_val)
                })
            },
            TknTy::Semicolon => {
                if is_imm {
                    let ty_str = self.currtkn.ty.to_string();
                    return Err(self.error(ParseErrTy::ImmDecl(ty_str)));
                }
                self.consume();

                if is_class_type {
                    let class_sym = self.symtab.retrieve(&var_ty_tkn.clone().unwrap().get_name()).unwrap();
                    let cl_ty_rec = TyRec::new_from_tkn(var_ty_tkn.clone().unwrap());
                    let cl_assign = class_sym.assign_val.clone();
                    let cl_sym = Sym::new(SymTy::Var,
                                          is_imm,
                                          cl_ty_rec.clone(),
                                          ident_tkn.clone().unwrap(),
                                          cl_assign.clone(),
                                          None);

                    let name = &ident_tkn.clone().unwrap().get_name();
                    self.symtab.store(name, cl_sym);

                    return Ok(Ast::VarAssignExpr {
                        ty_rec: cl_ty_rec,
                        ident_tkn: ident_tkn.clone().unwrap(),
                        is_imm: is_imm,
                        is_global: self.symtab.is_global(),
                        value: Box::new(cl_assign.unwrap())
                    });
                }

                let ty_rec = TyRec::new_from_tkn(var_ty_tkn.unwrap());
                let sym = Sym::new(SymTy::Var,
                                   is_imm,
                                   ty_rec.clone(),
                                   ident_tkn.clone().unwrap(),
                                   None,
                                   None);

                let name = &ident_tkn.clone().unwrap().get_name();
                self.symtab.store(name, sym);

                Ok(Ast::VarDeclExpr {
                    ty_rec: ty_rec,
                    ident_tkn: ident_tkn.unwrap(),
                    is_imm: is_imm,
                    is_global: self.symtab.is_global()
                })
            },
            _ => {
                let ty_str = self.currtkn.ty.to_string();
                Err(self.error(ParseErrTy::InvalidAssign(ty_str)))
            }
        }
    }

    fn fn_decl(&mut self) -> Result<Ast, ParseErr> {
        self.expect(TknTy::Fn)?;
        let fn_ident_tkn = self.currtkn.clone();
        self.consume();

        let mut params = Vec::new();
        self.expect(TknTy::LeftParen)?;

        while self.currtkn.ty != TknTy::RightParen {
            if params.len() > FN_PARAM_MAX_LEN {
                return Err(self.error(ParseErrTy::FnParamCntExceeded(FN_PARAM_MAX_LEN)));
            }

            let ident_tkn = self.currtkn.clone();
            self.consume();
            self.expect(TknTy::Tilde)?;

            let mut ty_rec = TyRec::new_from_tkn(self.currtkn.clone());
            ty_rec.tkn = ident_tkn.clone();

            // We must create an assign value if the parameter is a class. This is because
            // when parsing the function body, we might need to access the class props/methods
            // and we can't do that unless we store the class declaration there.
            let assign_val = match ty_rec.ty.clone().unwrap() {
                TyName::Class(name) => {
                    let class_sym = self.symtab.retrieve(&name);
                    if class_sym.is_none() {
                        return Err(self.error(ParseErrTy::UndeclaredSym(name)));
                    }

                    class_sym.unwrap().assign_val.clone()
                },
                _ => None
            };

            params.push(ty_rec.clone());

            // Store param variable name in the symbol table for the function scope.
            let param_sym = Sym::new(SymTy::Param, false, ty_rec, ident_tkn.clone(), assign_val, None);
            self.symtab.store(&ident_tkn.get_name(), param_sym);

            self.consume();
            if self.currtkn.ty == TknTy::RightParen {
                break;
            }
            self.expect(TknTy::Comma)?;
        }

        self.expect(TknTy::RightParen)?;
        self.expect(TknTy::Tilde)?;

        let fn_ret_ty_tkn = match self.currtkn.is_ty() {
            true => {
                let tkn = self.currtkn.clone();
                self.consume();
                Some(tkn)
            },
            false => None
        };

        if fn_ret_ty_tkn.is_none() {
            let ty_str = self.currtkn.ty.to_string();
            return Err(self.error(ParseErrTy::InvalidTy(ty_str)));
        }

        // Create and store the function sym before we parse the body and
        // set an actual value. This is so that when parsing the body, if we
        // encounter a recursive call, we won't report an error for trying
        // to call an undefined function.
        let fn_ty_rec = TyRec::new_from_tkn(fn_ret_ty_tkn.clone().unwrap());
        let fn_sym = Sym::new(SymTy::Fn,
                              true,
                              fn_ty_rec.clone(),
                              fn_ident_tkn.clone(),
                              None,
                              Some(params.clone()));

        let name = &fn_ident_tkn.get_name();
        self.symtab.store(name, fn_sym);

        // Now we parse the function body, update the symbol and store it with
        // the updated body.
        let fn_body = self.block_stmt()?;
        let new_sym = Sym::new(SymTy::Fn,
                              true,
                              fn_ty_rec.clone(),
                              fn_ident_tkn.clone(),
                              Some(fn_body.clone()),
                              Some(params.clone()));

        self.symtab.store(name, new_sym);

        Ok(Ast::FnDecl {
            ident_tkn: fn_ident_tkn,
            fn_params: params,
            ret_ty: fn_ty_rec,
            fn_body: Box::new(fn_body),
            sc: self.symtab.finalized_level
        })
    }

    /// Parses a class declaration
    fn class_decl(&mut self) -> Result<Ast, ParseErr> {
        self.expect(TknTy::Class)?;
        let class_tkn = self.currtkn.clone();
        self.consume();
        self.expect(TknTy::LeftBrace)?;

        // Initialize a new scope for the class methods + props
        self.symtab.init_sc();
        let mut methods = Vec::new();
        let mut props = Vec::new();
        let mut prop_map = HashMap::new();

        let mut prop_ctr = 0;
        loop {
            match self.currtkn.ty {
                TknTy::Let => {
                    let prop_ast = self.var_decl()?;
                    match prop_ast.clone() {
                        Ast::VarDeclExpr{ty_rec:_,ident_tkn, is_imm:_, is_global:_} => {
                            prop_map.insert(ident_tkn.get_name(), prop_ctr);
                        },
                        _ => {
                            return Err(self.error(ParseErrTy::InvalidClassProp));
                        }
                    }
                    props.push(prop_ast);
                    prop_ctr = prop_ctr + 1;
                },
                TknTy::Fn => {
                    let result = self.fn_decl()?;
                    methods.push(result);
                },
                TknTy::RightBrace => {
                    self.consume();
                    break;
                },
                _ => {
                    let ty_str = self.currtkn.ty.to_string();
                    self.error(ParseErrTy::InvalidTkn(ty_str));
                    break;
                }
            }
        }

        let final_sc_lvl = self.symtab.finalize_sc();
        let ast = Ast::ClassDecl {
            ident_tkn: class_tkn.clone(),
            methods: methods,
            props: props,
            prop_pos: prop_map,
            sc: final_sc_lvl
        };

        // This should be stored in the starting level of the symbol table, not the
        // scope opened to store the class methods/props (which is why we close the
        // current scope before this call to store()).
        let sym = Sym::new(SymTy::Class,
                           true,
                           TyRec::new_from_tkn(class_tkn.clone()),
                           class_tkn.clone(),
                           Some(ast.clone()),
                           None);
        self.symtab.store(&class_tkn.get_name(), sym);

        Ok(ast)
    }

    /// Parses a statement. This function does not perform any scope management, which
    /// is delegated to each statement type.
    fn stmt(&mut self) -> Result<Ast, ParseErr> {
        match self.currtkn.ty {
            TknTy::If => self.if_stmt(),
            TknTy::While => self.while_stmt(),
            TknTy::For => self.for_stmt(),
            TknTy::Return => self.ret_stmt(),
            TknTy::LeftBrace => self.block_stmt(),
            _ => self.expr_stmt()
        }
    }

    /// Parses a block statement, beginning with a '{' token. This creates a new scope,
    /// parses any statements within the block, and closes the block scope at the end.
    fn block_stmt(&mut self) -> Result<Ast, ParseErr> {
        self.expect(TknTy::LeftBrace)?;
        let mut stmts = Vec::new();
        self.symtab.init_sc();

        loop {
            match self.currtkn.ty {
                TknTy::RightBrace | TknTy::Eof => break,
                _ => {
                    let result = self.decl()?;
                    stmts.push(result);
                }
            };
        }

        self.expect(TknTy::RightBrace)?;
        let sc_lvl = self.symtab.finalize_sc();

        Ok(Ast::BlckStmt{
            stmts: stmts,
            sc: sc_lvl
        })
    }

    /// Parse an if statement, including else and elif blocks. These are stored in the
    /// IfStmt Ast type.
    fn if_stmt(&mut self) -> Result<Ast, ParseErr> {
        self.expect(TknTy::If)?;

        let if_cond = self.expr()?;
        let if_blck = self.block_stmt()?;
        let mut else_blck = None;
        let mut else_ifs = Vec::new();

        loop {
            match self.currtkn.ty {
                TknTy::Elif => {
                    self.consume();
                    let elif_ast = self.expr()?;
                    let elif_blck = self.block_stmt()?;
                    else_ifs.push(Ast::ElifStmt(Box::new(elif_ast), Box::new(elif_blck)));
                },
                TknTy::Else => {
                    self.consume();
                    let blck = self.block_stmt()?;
                    else_blck = Some(blck);
                },
                _ => break
            };
        }

        Ok(Ast::IfStmt(Box::new(if_cond),
                     Box::new(if_blck),
                     else_ifs,
                     Box::new(else_blck)))
    }

    fn while_stmt(&mut self) -> Result<Ast, ParseErr> {
        self.expect(TknTy::While)?;
        // TODO: skip expr for infinite loop when we have a break stmt
        let while_cond = self.expr()?;
        let while_stmts = self.block_stmt()?;
        Ok(Ast::WhileStmt(Box::new(while_cond), Box::new(while_stmts)))
    }

    fn for_stmt(&mut self) -> Result<Ast, ParseErr> {
        self.expect(TknTy::For)?;
        let mut for_var_decl = None;
        let mut for_var_cond = None;
        let mut for_incr_expr = None;

        match self.currtkn.ty {
            TknTy::Semicolon => self.consume(),
            TknTy::Let => {
                let var = self.var_decl()?;
                for_var_decl = Some(var);
            },
            _ => {
                return Err(self.error(ParseErrTy::InvalidForStmt));
            }
        };

        match self.currtkn.ty {
            TknTy::Semicolon => self.consume(),
            _ => {
                let expr = self.expr_stmt()?;
                for_var_cond = Some(expr);
            }
        };

        match self.currtkn.ty {
            TknTy::Semicolon => self.consume(),
            _ => {
                let expr = self.expr_stmt()?;
                for_incr_expr = Some(expr);
            }
        };

        let for_stmt = self.block_stmt()?;

        Ok(Ast::ForStmt{
            for_var_decl: Box::new(for_var_decl.unwrap()),
            for_cond_expr: Box::new(for_var_cond.unwrap()),
            for_step_expr: Box::new(for_incr_expr.unwrap()),
            stmts: Box::new(for_stmt)
        })
    }

    fn ret_stmt(&mut self) -> Result<Ast, ParseErr> {
        self.expect(TknTy::Return)?;
        match self.currtkn.ty {
            TknTy::Semicolon => {
                self.consume();
                Ok(Ast::RetStmt(Box::new(None)))
            },
            _ => {
                let ret_expr = self.expr()?;
                self.expect(TknTy::Semicolon)?;
                Ok(Ast::RetStmt(Box::new(Some(ret_expr))))
            }
        }
    }

    fn expr_stmt(&mut self) -> Result<Ast, ParseErr> {
        let expr = self.expr()?;
        self.expect(TknTy::Semicolon)?;
        Ok(Ast::ExprStmt(Box::new(expr)))
    }

    fn expr(&mut self) -> Result<Ast, ParseErr> {
        self.assign_expr()
    }

    fn assign_expr(&mut self) -> Result<Ast, ParseErr> {
        let ast = self.logicor_expr()?;

        match self.currtkn.ty {
            TknTy::Eq => {
                let op = self.currtkn.clone();
                self.consume();
                let rhs = self.assign_expr()?;

                match ast.clone() {
                    Ast::PrimaryExpr{ty_rec} => {
                        match ty_rec.tkn.ty {
                            TknTy::Ident(name) => {
                                let maybe_sym = self.symtab.retrieve(&name);
                                if maybe_sym.is_none() {
                                    return Err(self.error(ParseErrTy::UndeclaredSym(name)));
                                }

                                let sym = maybe_sym.unwrap();
                                if sym.imm {
                                    return Err(self.error(ParseErrTy::InvalidImmAssign(name)));
                                }

                                return Ok(Ast::VarAssignExpr {
                                    ty_rec: sym.ty_rec.clone(),
                                    ident_tkn: sym.ident_tkn.clone(),
                                    is_imm: sym.imm,
                                    is_global: self.symtab.is_global(),
                                    value: Box::new(rhs)
                                });
                            },
                            _ => {
                                return Err(
                                    self.error(ParseErrTy::InvalidAssign(ty_rec.tkn.ty.clone().to_string()))
                                );
                            }
                        };
                    },
                    Ast::ClassPropAccess{ident_tkn, prop_name, idx, owner_class} => {
                        return Ok(Ast::ClassPropSet{
                            ident_tkn: ident_tkn,
                            prop_name: prop_name,
                            idx: idx,
                            owner_class: owner_class,
                            assign_val: Box::new(rhs)
                        });
                    },
                    _ => {
                        return Err(
                            self.error_w_pos(op.line, op.pos, ParseErrTy::InvalidAssign(op.ty.to_string()))
                        );
                    }
                }
            },
            _ => ()
        };

        Ok(ast)
    }

    fn logicor_expr(&mut self) -> Result<Ast, ParseErr> {
        let mut ast = self.logicand_expr()?;
        loop {
            match self.currtkn.ty {
                TknTy::PipePipe | TknTy::Or => {
                    let op = self.currtkn.clone();
                    self.consume();
                    let rhs = self.logicand_expr()?;
                    ast = Ast::LogicalExpr {
                        ty_rec: TyRec::empty(&op),
                        op_tkn: op,
                        lhs: Box::new(ast),
                        rhs: Box::new(rhs)
                    };
                },
                _ => break
            }
        }

        Ok(ast)
    }

    fn logicand_expr(&mut self) -> Result<Ast, ParseErr> {
        let mut ast = self.eq_expr()?;
        loop {
            match self.currtkn.ty {
                TknTy::AmpAmp | TknTy::And => {
                    let op = self.currtkn.clone();
                    self.consume();
                    let rhs = self.eq_expr()?;
                    ast = Ast::LogicalExpr {
                        ty_rec: TyRec::empty(&op),
                        op_tkn: op,
                        lhs: Box::new(ast),
                        rhs: Box::new(rhs)
                    };
                },
                _ => break
            }
        }

        Ok(ast)
    }

    fn eq_expr(&mut self) -> Result<Ast, ParseErr> {
        let mut ast = self.cmp_expr()?;
        loop {
            match self.currtkn.ty {
                TknTy::BangEq | TknTy::EqEq => {
                    let op = self.currtkn.clone();
                    self.consume();
                    let rhs = self.cmp_expr()?;
                    ast = Ast::BinaryExpr {
                        ty_rec: TyRec::empty(&op),
                        op_tkn: op,
                        lhs: Box::new(ast),
                        rhs: Box::new(rhs)
                    };
                },
                _ => break
            }
        }

        Ok(ast)
    }

    fn cmp_expr(&mut self) -> Result<Ast, ParseErr> {
        let mut ast = self.addsub_expr()?;
        loop {
            match self.currtkn.ty {
                TknTy::Lt | TknTy::LtEq | TknTy::Gt | TknTy::GtEq => {
                    let op = self.currtkn.clone();
                    self.consume();
                    let rhs = self.addsub_expr()?;
                    ast = Ast::BinaryExpr {
                        ty_rec: TyRec::empty(&op),
                        op_tkn: op,
                        lhs: Box::new(ast),
                        rhs: Box::new(rhs)
                    };
                },
                _ => break
            }
        }

        Ok(ast)
    }

    fn addsub_expr(&mut self) -> Result<Ast, ParseErr> {
        let mut ast = self.muldiv_expr()?;
        loop {
            match self.currtkn.ty {
                TknTy::Plus | TknTy::Minus => {
                    let op = self.currtkn.clone();
                    self.consume();
                    let rhs = self.muldiv_expr()?;
                    ast = Ast::BinaryExpr {
                        ty_rec: TyRec::empty(&op),
                        op_tkn: op,
                        lhs: Box::new(ast),
                        rhs: Box::new(rhs)
                    };
                },
                _ => break
            }
        }

        Ok(ast)
    }

    fn muldiv_expr(&mut self) -> Result<Ast, ParseErr> {
        let mut ast = self.unary_expr()?;
        loop {
            match self.currtkn.ty {
                TknTy::Star | TknTy::Slash => {
                    let op = self.currtkn.clone();
                    self.consume();
                    let rhs = self.unary_expr()?;
                    ast = Ast::BinaryExpr {
                        ty_rec: TyRec::empty(&op),
                        op_tkn: op,
                        lhs: Box::new(ast),
                        rhs: Box::new(rhs)
                    };
                },
                _ => break
            }
        }

        Ok(ast)
    }

    fn unary_expr(&mut self) -> Result<Ast, ParseErr> {
        match self.currtkn.ty {
            TknTy::Bang | TknTy::Minus => {
                let op = self.currtkn.clone();
                self.consume();
                let rhs = self.unary_expr()?;

                return Ok(Ast::UnaryExpr {
                    ty_rec: TyRec::empty(&op),
                    op_tkn: op,
                    rhs: Box::new(rhs)
                });
            },
            _ => self.fncall_expr()
        }
    }

    fn fncall_expr(&mut self) -> Result<Ast, ParseErr> {
        let mut ast = self.primary_expr()?;
        let ident_tkn = match ast.clone() {
            Ast::PrimaryExpr{ty_rec} => Some(ty_rec.tkn),
            _ => None
        };

        // If this is a class ident, we expect a period and then either a property name
        // or a function call. If this is a regular function ident, we expect an
        // opening paren next.
        match self.currtkn.ty {
            TknTy::LeftParen => {
                ast = self.fnparams_expr(ident_tkn, None)?;
            },
            TknTy::Period => {
                ast = self.class_expr(ident_tkn)?;
            },
            _ => ()
        };

        Ok(ast)
    }

    /// Parses calling class methods or getting/setting class props.
    fn class_expr(&mut self, class_tkn: Option<Token>) -> Result<Ast, ParseErr> {
        // Consume period token
        self.expect(TknTy::Period)?;

        // This token can be a function name, a class prop name, or
        // another class name.
        let name_tkn = self.match_ident_tkn();
        let ast = match self.currtkn.ty {
            TknTy::LeftParen => {
                // Calling a function that belongs to the class
                let class_sym = self.symtab.retrieve(&class_tkn.clone().unwrap().get_name());
                let (sc_lvl, class_name) = match class_sym.clone().unwrap().assign_val.clone().unwrap() {
                    Ast::ClassDecl{ident_tkn, methods:_,props:_, prop_pos:_, sc} => {
                        (sc, ident_tkn.get_name())
                    },
                    _ => {
                        self.error(ParseErrTy::UndeclaredSym(name_tkn.clone().unwrap().get_name()));
                        (0, String::new())
                    }
                };

                let fn_ast = self.fnparams_expr(name_tkn.clone(), class_sym.clone())?;
                let params = fn_ast.extract_params();

                Ok(Ast::ClassFnCall {
                    class_tkn: class_tkn.clone().unwrap(),
                    class_name: class_name,
                    fn_tkn: name_tkn.unwrap().clone(),
                    fn_params: params,
                    sc: sc_lvl
                })
            },
            TknTy::Period => {
                // Accessing another class within this class
                self.class_expr(name_tkn)
            }
            _ => {
                let class_sym = self.symtab.retrieve(&class_tkn.clone().unwrap().get_name());
                if class_sym.is_none() {
                    return Err(self.error(ParseErrTy::UndeclaredSym(name_tkn.clone().unwrap().get_name())));
                }
                let class_ptr = class_sym.unwrap();
                let owner = class_ptr.assign_val.clone().unwrap();
                let pos = match &owner {
                    Ast::ClassDecl{ident_tkn:_, methods:_, props:_, prop_pos, sc:_} => {
                        let map = prop_pos.clone();
                        let idx = map.get(&name_tkn.clone().unwrap().get_name());
                        match idx {
                            Some(num) => num.clone() as usize,
                            None => {
                                self.error(ParseErrTy::InvalidClassProp);
                                0 as usize
                            }
                        }
                    },
                    _ => 0 as usize
                };

                Ok(Ast::ClassPropAccess {
                    ident_tkn: class_tkn.unwrap(),
                    prop_name: name_tkn.unwrap().get_name(),
                    idx: pos,
                    owner_class: Box::new(owner)
                })
            }
        };

        ast
    }

    /// Parses the parameters of a function call. Because the function could be a class method,
    /// this accepts an optional class symbol, which should be taken out of the symbol table. If this
    /// is not a class method being parsed, maybe_class_sym should be None.
    /// This symbol is used to find the expected function params, so that we can ensure that
    /// what is passed in is correct.
    fn fnparams_expr(&mut self,
                     fn_tkn: Option<Token>,
                     maybe_class_sym: Option<Rc<Sym>>) -> Result<Ast, ParseErr> {
        self.expect(TknTy::LeftParen)?;

        let fn_sym = self.symtab.retrieve(&fn_tkn.clone().unwrap().get_name());

        // If the fn_sym doesn't exist, we need to handle the case that it might be
        // a class method, so we check the class symbol if one exists.
        let maybe_expected_params = match fn_sym {
            // If there is no class sym and no fn sym, we have no expected params.
            None if maybe_class_sym.is_none() => {
                None
            },
            // If there is a class sym, check for the method in the class methods list
            // and get the expected params. If the method doesn't exist on the class,
            // we return None.
            None => {
                let class_decl_ast = maybe_class_sym.unwrap().assign_val.clone().unwrap();

                let params = match class_decl_ast {
                    Ast::ClassDecl{ident_tkn:_, methods, props:_, prop_pos:_, sc:_} => {
                        let mut expected_params = None;

                        for mtod_ast in methods {
                            match mtod_ast {
                                Ast::FnDecl{ident_tkn, fn_params, ret_ty:_, fn_body:_, sc:_} => {
                                    if ident_tkn.get_name() == fn_tkn.clone().unwrap().get_name() {
                                        expected_params = Some(fn_params);
                                    }
                                },
                                _ => ()
                            }
                        }

                        expected_params
                    },
                    _ => None
                };

                params
            },
            // If the fn sym exists, simply take its params.
            Some(sym) => {
                sym.fn_params.clone()
            }
        };

        // If we have no expected params after checking the fn_sym and the class_sym,
        // we report an error and return None early.
        if maybe_expected_params.is_none() {
            let tkn = fn_tkn.clone().unwrap();
            return Err(self.error_w_pos(tkn.line, tkn.pos, ParseErrTy::UndeclaredSym(tkn.get_name())));
        }

        let expected_params = maybe_expected_params.unwrap();
        let mut params: Vec<Ast> = Vec::new();
        while self.currtkn.ty != TknTy::RightParen {
            if params.len() > FN_PARAM_MAX_LEN {
                return Err(self.error(ParseErrTy::FnParamCntExceeded(FN_PARAM_MAX_LEN)));
            }

            let parm = self.expr()?;
            params.push(parm);

            if self.currtkn.ty == TknTy::RightParen {
                break;
            }
            self.expect(TknTy::Comma)?;
        }

        self.expect(TknTy::RightParen)?;

        if expected_params.len() != params.len() {
            let tkn = fn_tkn.clone().unwrap();
            self.error_w_pos(tkn.line,
                             tkn.pos,
                             ParseErrTy::WrongFnParamCnt(expected_params.len(), params.len()));
        }

        Ok(Ast::FnCall{
            fn_tkn: fn_tkn.unwrap(),
            fn_params: params
        })
    }

    fn primary_expr(&mut self) -> Result<Ast, ParseErr> {
        match self.currtkn.ty.clone() {
            TknTy::Str(_) |
            TknTy::Val(_) |
            TknTy::True |
            TknTy::False |
            TknTy::Null => {
                let ast = Ok(Ast::PrimaryExpr {
                    ty_rec: TyRec::new_from_tkn(self.currtkn.clone())
                });
                self.consume();
                ast
            },
            TknTy::Ident(ref ident_name) => {
                let mb_sym = self.symtab.retrieve(ident_name);
                if mb_sym.is_none() {
                    let err = self.error(ParseErrTy::UndeclaredSym(ident_name.to_string()));
                    self.consume();
                    return Err(err);
                }

                let sym = mb_sym.unwrap();
                // If was have no assign value, but we are looking at a param
                // or function decl sym, we can return the sym. But no assign value on
                // any other type requires a check that we are assigning to it, otherwise
                // we are trying to access an udnefined variable.
                // Fn is here to support recursive calls.
                if sym.assign_val.is_none() && (sym.sym_ty != SymTy::Param && sym.sym_ty != SymTy::Fn) {
                    let next_tkn = self.lexer.peek_tkn();
                    // If the following token is '=', we don't need to report an error
                    // for unitialized var (we are initializing it here).
                    if next_tkn.ty != TknTy::Eq {
                        let err = self.error(ParseErrTy::UnassignedVar(ident_name.to_string()));
                        self.consume();
                        return Err(err);
                    }
                }

                let mut ty_rec = sym.ty_rec.clone();
                ty_rec.tkn = self.currtkn.clone();
                let ast = Ok(Ast::PrimaryExpr{
                    ty_rec: ty_rec.clone()
                });
                self.consume();
                ast
            },
            TknTy::LeftParen => {
                self.consume();
                let ast = self.expr()?;
                self.expect(TknTy::RightParen)?;
                Ok(ast)
            },
            TknTy::String | TknTy::Num | TknTy::Bool => {
                let ty_str = self.currtkn.ty.to_string();
                let err = self.error(ParseErrTy::InvalidAssign(ty_str));
                self.consume();
                Err(err)
            },
            _ => {
                let ty_str = self.currtkn.ty.to_string();
                let err = self.error(ParseErrTy::InvalidTkn(ty_str));
                self.consume();
                Err(err)
            }
        }
    }

    fn match_ident_tkn(&mut self) -> Option<Token> {
        match self.currtkn.ty {
            TknTy::Ident(_) => {
                let tkn = Some(self.currtkn.clone());
                self.consume();
                tkn
            },
            _ => {
                let ty_str = self.currtkn.ty.to_string();
                self.error(ParseErrTy::InvalidIdent(ty_str));
                None
            }
        }
    }

    /// Check that the current token is the same as the one we expect. If it is, consume the
    /// token and advance. If it isn't report an error.
    fn expect(&mut self, tknty: TknTy) -> Result<(), ParseErr> {
        if self.currtkn.ty == tknty {
            self.consume();
            Ok(())
        } else {
            let ty_str = self.currtkn.ty.to_string();
            let err_ty = ParseErrTy::TknMismatch(tknty.to_string(), ty_str);
            Err(ParseErr::new(self.currtkn.line, self.currtkn.pos, err_ty))
        }
    }

    /// Advance to the next token, discarded the previously read token.
    fn consume(&mut self) {
        self.currtkn = self.lexer.lex();
    }

    /// Report a parsing error from the current token, with the given parser error type.
    fn error(&mut self, ty: ParseErrTy) -> ParseErr {
        let err = ParseErr::new(self.currtkn.line, self.currtkn.pos, ty);
        self.errors.push(err.clone());
        err
    }

    /// Report a parsing error at a given location with a provided error type.
    fn error_w_pos(&mut self, line: usize, pos: usize, ty: ParseErrTy) -> ParseErr {
        let err = ParseErr::new(line, pos, ty);
        self.errors.push(err.clone());
        err
    }
}