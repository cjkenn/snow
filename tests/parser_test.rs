extern crate snowc;

use std::fs::File;

use snowc::lexer::Lexer;
use snowc::token::TknTy;
use snowc::parser::Parser;
use snowc::ast::Ast;
use snowc::symtab::SymTab;
use snowc::type_record::TyName;

#[test]
fn test_parse_empty() {
    let mut lexer = Lexer::new(File::open("./tests/parser_input/empty").unwrap());
    let mut symtab = SymTab::new();
    let ast = Parser::new(&mut lexer, &mut symtab).parse().ast.unwrap();

    match *ast {
        Ast::Prog(stmts) => {
           assert_eq!(stmts.len(), 0)
        },
        _ => assert!(false, "Expected Ast::Prog, found {:?}", *ast)
    };
}

#[test]
fn test_parse_var_decl_mutable() {
    let mut lexer = Lexer::new(File::open("./tests/parser_input/var_decl_mutable").unwrap());
    let mut symtab = SymTab::new();
    let ast = Parser::new(&mut lexer, &mut symtab).parse().ast.unwrap();

    let var_decl = &extract_head(ast)[0];
    match *var_decl {
        Ast::VarDecl(ref varty_rec, ref ident, imm) => {
            assert_eq!(varty_rec.ty, Some(TyName::Num));
            assert_eq!(imm, false);

            match ident.ty {
                TknTy::Ident(ref id) => {
                    assert_eq!(id, "x");
                },
                _ => assert!(false, "Expected Ident tkn, found {:?}", ident.ty)
            }
        },
        _ => assert!(false, "Expected Ast::VarDecl, found {:?}", var_decl)
    };
}

#[test]
fn test_parse_var_decl_imm() {
    let mut lexer = Lexer::new(File::open("./tests/parser_input/var_decl_imm").unwrap());
    let mut symtab = SymTab::new();
    let result = Parser::new(&mut lexer, &mut symtab).parse();
    assert!(result.error.len() >= 1);
}

#[test]
fn test_parse_var_assign_mutable() {
    let mut lexer = Lexer::new(File::open("./tests/parser_input/var_assign_mutable").unwrap());
    let mut symtab = SymTab::new();
    let ast = Parser::new(&mut lexer, &mut symtab).parse().ast.unwrap();

    let var_assign = &extract_head(ast)[0];
    match *var_assign {
        Ast::VarAssign(ref varty_rec, ref ident, imm, ref val_ast) => {
            assert_eq!(varty_rec.ty, Some(TyName::Num));
            assert_eq!(imm, false);

            match ident.ty {
                TknTy::Ident(ref id) => {
                    assert_eq!(id, "x");
                },
                _ => expected_tkn("Ident", &ident.ty)
            }

            let vast = val_ast.clone();
            match *vast {
                Some(ast) => {
                    match ast {
                        Ast::Primary(_) => assert!(true),
                        _ => expected_ast("Primary", &ast)
                    }
                },
                _ => ()
            }
        },
        _ => expected_ast("VarAssign", &var_assign)
    }
}

#[test]
fn test_parse_var_assign_imm() {
    let mut lexer = Lexer::new(File::open("./tests/parser_input/var_assign_imm").unwrap());
    let mut symtab = SymTab::new();
    let ast = Parser::new(&mut lexer, &mut symtab).parse().ast.unwrap();

    let var_assign = &extract_head(ast)[0];
    match *var_assign {
        Ast::VarAssign(ref varty_rec, ref ident, imm, ref rhs) => {
            assert_eq!(varty_rec.ty, Some(TyName::Num));
            assert_eq!(imm, true);

            match ident.ty {
                TknTy::Ident(ref id) => {
                    assert_eq!(id, "x");
                },
                _ => expected_tkn("Ident", &ident.ty)
            }

            let vast = rhs.clone();
            match *vast {
                Some(ast) => {
                    match ast {
                        Ast::Primary(_) => assert!(true),
                        _ => expected_ast("Primary", &ast)
                    }
                },
                _ => ()
            }
        },
        _ => expected_ast("VarAssign", &var_assign)
    }
}

#[test]
fn test_parse_unary_expr() {
    let mut lexer = Lexer::new(File::open("./tests/parser_input/unary_expr").unwrap());
    let mut symtab = SymTab::new();
    let ast = Parser::new(&mut lexer, &mut symtab).parse().ast.unwrap();

    let exprstmt = &extract_head(ast)[0];

    match *exprstmt {
        Ast::ExprStmt(ref unr) => {
            let unr2 = unr.clone();
            match *unr2 {
                Some(ast) => {
                    match ast {
                        Ast::Unary(_, _) => assert!(true),
                        _ => expected_ast("Unary", &ast)
                    }
                },
                _ => ()
            }
        },
        _ => expected_ast("ExprStmt", &exprstmt)
    }
}

fn extract_head(ast: Box<Ast>) -> Vec<Ast> {
    match *ast {
        Ast::Prog(ref stmts) => stmts.clone(),
        _ => panic!("Cannot call extract_head on an ast not of type Ast::Prog")
    }
}

fn expected_ast(expt: &str, found: &Ast) {
    assert!(false, format!("Expected {}, found {:?}", expt, found));
}

fn expected_tkn(expt: &str, found: &TknTy) {
    assert!(false, format!("Expected {}, found {:?}", expt, found));
}
