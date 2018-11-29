use std::collections::HashMap;
use token::Token;
use ty_rec::TyRec;

#[derive(Clone, Debug, PartialEq)]
pub enum Ast {
    Prog {
        stmts: Vec<Ast>,
    },

    BlckStmt {
        stmts: Vec<Ast>,
        sc: usize,
    },

    IfStmt {
        cond_expr: Box<Ast>,
        if_stmts: Box<Ast>,
        elif_exprs: Vec<Ast>,
        el_stmts: Vec<Ast>,
    },

    ElifStmt {
        cond_expr: Box<Ast>,
        stmts: Box<Ast>,
    },

    WhileStmt {
        cond_expr: Box<Ast>,
        stmts: Box<Ast>,
    },

    ForStmt {
        for_var_decl: Box<Ast>,
        for_cond_expr: Box<Ast>,
        for_step_expr: Box<Ast>,
        stmts: Box<Ast>,
    },

    // Return expr, if any
    RetStmt(Box<Option<Ast>>),

    // expr
    ExprStmt(Box<Ast>),

    VarDeclExpr {
        ty_rec: TyRec,
        ident_tkn: Token,
        is_imm: bool,
        is_global: bool,
    },

    VarAssignExpr {
        ty_rec: TyRec,
        ident_tkn: Token,
        is_imm: bool,
        is_global: bool,
        value: Box<Ast>,
    },

    LogicalExpr {
        ty_rec: TyRec,
        op_tkn: Token,
        lhs: Box<Ast>,
        rhs: Box<Ast>,
    },

    BinaryExpr {
        ty_rec: TyRec,
        op_tkn: Token,
        lhs: Box<Ast>,
        rhs: Box<Ast>,
    },

    UnaryExpr {
        ty_rec: TyRec,
        op_tkn: Token,
        rhs: Box<Ast>,
    },

    FnDecl {
        ident_tkn: Token,
        fn_params: Vec<TyRec>,
        ret_ty: TyRec,
        fn_body: Box<Ast>,
        sc: usize,
    },

    FnCall {
        fn_tkn: Token,
        fn_params: Vec<Ast>,
    },

    ClassDecl {
        ident_tkn: Token,
        methods: Vec<Ast>,
        props: Vec<Ast>,
        prop_pos: HashMap<String, usize>,
        sc: usize,
    },

    ClassPropAccess {
        ident_tkn: Token,
        prop_name: String,
        idx: usize,
        owner_class: Box<Ast>,
    },

    ClassPropSet {
        ident_tkn: Token,
        prop_name: String,
        idx: usize,
        owner_class: Box<Ast>,
        assign_val: Box<Ast>,
    },

    ClassFnCall {
        class_tkn: Token,
        class_name: String,
        fn_tkn: Token,
        fn_params: Vec<Ast>,
        sc: usize,
    },

    PrimaryExpr {
        ty_rec: TyRec,
    },
}

impl Ast {
    pub fn is_primary(&self) -> bool {
        match self {
            Ast::PrimaryExpr { .. } => true,
            _ => false,
        }
    }

    pub fn extract_primary_ty_rec(&self) -> TyRec {
        match self {
            Ast::PrimaryExpr { ty_rec } => ty_rec.clone(),
            _ => panic!(),
        }
    }

    pub fn extract_params(&self) -> Vec<Ast> {
        match self {
            Ast::FnCall {
                fn_tkn: _,
                fn_params,
            } => fn_params.clone(),
            _ => Vec::new(),
        }
    }
}
