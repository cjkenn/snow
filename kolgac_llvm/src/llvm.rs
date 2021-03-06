use crate::{classtab::ClassTab, valtab::ValTab};
//use crate::fpm::FPM;

use kolgac_errors::gen::{GenErr, GenErrTy};

use kolgac::{
    ast::Ast,
    token::{TknTy, Token},
    ty_rec::{KolgaTy, TyRecord},
};

use llvm_sys::{
    core::*,
    prelude::*,
    {LLVMRealPredicate, LLVMTypeKind},
};

use std::{collections::HashMap, ffi::CString, ptr, slice};

const LLVM_FALSE: LLVMBool = 0;

#[derive(Debug)]
struct GenCtx<'gc> {
    pub clsctx: &'gc mut GenClsCtx,
}

impl<'gc> GenCtx<'gc> {
    pub fn new(cctx: &'gc mut GenClsCtx) -> GenCtx<'gc> {
        GenCtx { clsctx: cctx }
    }
}

#[derive(Debug)]
struct GenClsCtx {
    pub curr_cls: String,
    pub curr_props: HashMap<String, usize>,
    pub curr_self: Option<LLVMValueRef>,
}

impl GenClsCtx {
    pub fn new() -> GenClsCtx {
        GenClsCtx {
            curr_cls: String::new(), // TODO: is this needed?
            curr_props: HashMap::new(),
            curr_self: None,
        }
    }

    pub fn reset(&mut self) {
        self.curr_cls = String::new();
        self.curr_props.clear();
        self.curr_self = None;
    }
}

/// CodeGenerator handles the code generation for LLVM IR. Converts an AST to LLVM IR. We assume
/// there are no parsing errors and that each node in the AST can be safely unwrapped. Each
/// variable can be assumed to exist.
pub struct CodeGenerator<'t, 'v> {
    /// Parsed AST
    ast: &'t Ast,

    /// Value table stores LLVMValueRef's for lookup.
    valtab: &'v mut ValTab,

    /// Class table stores LLVmStructTypes so we can look them up before allocating.
    classtab: ClassTab,

    /// LLVM Context.
    context: LLVMContextRef,

    /// LLVM Builder.
    builder: LLVMBuilderRef,

    /// LLVM Module. We use only a single module for single file programs.
    pub module: LLVMModuleRef,

    /// Owned CStrings that we use for naming things in our LLVM module.
    strings: Vec<CString>,

    /// Vector of potential errors to return.
    pub errors: Vec<GenErr>,
    // LLVM Function pass manager, for some optimization passes after function codegen.
    //fpm: FPM
}

/// We implement Drop for the CodeGenerator to ensure that our LLVM structs are safely
/// disposed of when the CodeGenerator goes out of scope.
impl<'t, 'v> Drop for CodeGenerator<'t, 'v> {
    fn drop(&mut self) {
        unsafe {
            LLVMDisposeBuilder(self.builder);
            LLVMDisposeModule(self.module);
            LLVMContextDispose(self.context);
        }
    }
}

impl<'t, 'v> CodeGenerator<'t, 'v> {
    /// Creates a new CodeGenerator, given a properly parsed AST, symbol table, and value table.
    /// We assume that the symbol table already contains all the required variables in this module,
    /// and that the value table is newly defined and should be empty.
    /// This function also sets up all the required LLVM structures needed to generate the IR:
    /// the context, the builder, and the module.
    pub fn new(ast: &'t Ast, valtab: &'v mut ValTab) -> CodeGenerator<'t, 'v> {
        unsafe {
            let context = LLVMContextCreate();
            let module = LLVMModuleCreateWithNameInContext(c_str!("kolga"), context);
            CodeGenerator {
                ast: ast,
                valtab: valtab,
                classtab: ClassTab::new(),
                errors: Vec::new(),
                context: context,
                builder: LLVMCreateBuilderInContext(context),
                module: module,
                strings: Vec::new(), //fpm: FPM::new(module)
            }
        }
    }

    /// Initial entry point for LLVM IR code generation. Loops through each statement in the
    /// program and generates LLVM IR for each of them. The code is written to the module,
    /// to be converted to assembly later.
    pub fn gen_ir(&mut self) {
        let mut cctx = GenClsCtx::new();
        let mut gctx = GenCtx::new(&mut cctx);

        match self.ast {
            Ast::Prog { meta: _, stmts } => {
                for stmt in stmts {
                    self.gen_stmt(&mut gctx, stmt);
                }
            }
            _ => (),
        }
    }

    /// Dumps the current module's IR to stdout.
    pub fn dump_ir(&self) {
        unsafe {
            LLVMDumpModule(self.module);
        }
    }

    /// Saves the current module's IR to a file.
    pub fn print_ir(&self, filename: String) {
        unsafe {
            LLVMPrintModuleToFile(
                self.module,
                filename.as_bytes().as_ptr() as *const i8,
                ptr::null_mut(),
            );
        }
    }

    pub fn get_mod(&self) -> LLVMModuleRef {
        self.module
    }

    /// Generate LLVM IR for a kolga statement. This handles all statement types, and will also
    /// call through to self.gen_expr() when needed. This is a recursive function, and will walk
    /// the AST for any nested statements or block statements.
    ///
    /// Returns a vector of LLVMValueRef's, which may be needed to generate PHI blocks or to make
    /// checks after recursive calls return. If there is no generated values, returns empty vec.
    // TODO: This is a bit of a hack, should probably return a result
    // instead of an empty vec (statements dont evaluate to anything, so there's never an
    // LLVMValueRef returned). But in the case that we do have to generate an expression,
    // we need to know which values we generated.
    fn gen_stmt(&mut self, gctx: &mut GenCtx, stmt: &Ast) -> Vec<LLVMValueRef> {
        match stmt {
            Ast::BlckStmt {
                meta: _,
                stmts,
                sc: _,
            } => {
                let mut generated = Vec::new();
                for stmt in stmts {
                    let mb_gen = self.gen_stmt(gctx, &stmt.clone());
                    generated.extend(mb_gen);
                }

                generated
            }
            Ast::ExprStmt { meta: _, expr } => {
                let ast = expr.clone();
                let val = self.gen_expr(gctx, &ast);
                match val {
                    Some(exprval) => vec![exprval],
                    None => {
                        self.error(GenErrTy::InvalidAst);
                        Vec::new()
                    }
                }
            }
            Ast::IfStmt {
                meta: _,
                cond_expr,
                if_stmts,
                elif_exprs,
                el_stmts,
            } => self.if_stmt(gctx, cond_expr, if_stmts, elif_exprs, el_stmts),
            Ast::WhileStmt {
                meta: _,
                cond_expr,
                stmts,
            } => self.while_stmt(gctx, cond_expr, stmts),
            Ast::ForStmt {
                meta: _,
                for_var_decl,
                for_cond_expr,
                for_step_expr,
                stmts,
            } => self.for_stmt(gctx, for_var_decl, for_cond_expr, for_step_expr, stmts),
            Ast::FnDeclStmt {
                meta: _,
                ident_tkn,
                fn_params,
                ret_ty,
                fn_body,
                sc: _,
            } => self.fn_decl_stmt(gctx, ident_tkn, fn_params, ret_ty, fn_body),
            Ast::VarAssignExpr {
                meta: _,
                ty_rec,
                ident_tkn,
                is_imm: _,
                is_global,
                value,
            } => self.var_assign_expr(gctx, ty_rec, ident_tkn, *is_global, value),
            Ast::VarDeclExpr {
                meta: _,
                ty_rec,
                ident_tkn,
                is_imm: _,
                is_global,
            } => match is_global {
                // Similar to var assignments, we generate different IR based on
                // whether the var is global or not. For global declarations, we
                // add a global without setting the initializer. For locals, we
                // build an alloca/store pair, but with no expression value
                // to store.
                true => unsafe {
                    let c_name = self.c_str(&ident_tkn.get_name());
                    let llvm_ty = self.llvm_ty_from_ty_rec(ty_rec, false);
                    let global = LLVMAddGlobal(self.module, llvm_ty, c_name);
                    self.valtab.store(&ident_tkn.get_name(), global);
                    vec![global]
                },
                false => unsafe {
                    let insert_bb = LLVMGetInsertBlock(self.builder);
                    let llvm_func = LLVMGetBasicBlockParent(insert_bb);
                    let alloca_instr = self.build_entry_bb_alloca(
                        llvm_func,
                        ty_rec.clone(),
                        &ident_tkn.get_name(),
                    );
                    self.valtab.store(&ident_tkn.get_name(), alloca_instr);
                    vec![alloca_instr]
                },
            },
            Ast::ClassDeclStmt {
                meta: _,
                ty_rec: _,
                ident_tkn,
                methods,
                props,
                prop_pos,
                ..
            } => self.class_decl_stmt(gctx, ident_tkn, methods, props, prop_pos),
            _ => unimplemented!("Ast type {:?} is not implemented for codegen", stmt),
        }
    }

    /// Generate LLVM IR for expression type ASTs. This handles building comparisons and constant
    /// ints and strings, as well as function call expressions.
    /// This is a recursive function, and will walk the expression AST until we reach a point
    /// to terminate on.
    fn gen_expr(&mut self, gctx: &mut GenCtx, expr: &Ast) -> Option<LLVMValueRef> {
        match expr {
            Ast::PrimaryExpr {
                meta: _,
                ty_rec,
                is_self,
            } => self.primary_expr(gctx, &ty_rec, *is_self),
            Ast::BinaryExpr {
                meta: _,
                ty_rec: _,
                op_tkn,
                lhs,
                rhs,
            }
            | Ast::LogicalExpr {
                meta: _,
                ty_rec: _,
                op_tkn,
                lhs,
                rhs,
            } => {
                // Recursively generate the LLVMValueRef's for the LHS and RHS. This is just
                // a single call for each if they are primary expressions.
                let mb_lhs_llvm_val = self.gen_expr(gctx, &lhs.clone());
                let mb_rhs_llvm_val = self.gen_expr(gctx, &rhs.clone());

                if mb_lhs_llvm_val.is_none() || mb_rhs_llvm_val.is_none() {
                    return None;
                }

                let lhs_llvm_val = mb_lhs_llvm_val.unwrap();
                let rhs_llvm_val = mb_rhs_llvm_val.unwrap();

                // Convert the operator to an LLVM instruction once we have the
                // LHS and RHS values.
                self.llvm_val_from_op(&op_tkn.ty, lhs_llvm_val, rhs_llvm_val)
            }
            Ast::UnaryExpr {
                meta: _,
                ty_rec: _,
                op_tkn,
                rhs,
            } => self.unary_expr(gctx, op_tkn, rhs),
            Ast::FnCallExpr {
                meta: _,
                ty_rec: _,
                fn_tkn,
                fn_params,
            } => self.fn_call_expr(gctx, fn_tkn, fn_params),
            Ast::ClassFnCallExpr {
                meta: _,
                ty_rec: _,
                class_tkn,
                class_name,
                fn_tkn,
                fn_params,
                ..
            } => self.class_fn_call_expr(gctx, class_tkn, class_name, fn_tkn, fn_params),
            Ast::VarAssignExpr {
                meta: _,
                ty_rec: _,
                ident_tkn,
                is_imm: _,
                is_global: _,
                value,
            } => {
                // This is a variable re-assign, not a new declaration and assign. Thus,
                // we look up the alloca instruction from the value table, and build
                // a new store instruction for it. We don't need to update the value table
                // (I don't THINK we need to), because we still want to manipulate the old
                // alloca instruction.
                // TODO: what if this isn't a re-assign?
                unsafe {
                    let curr_alloca_instr = self.valtab.retrieve(&ident_tkn.get_name()).unwrap();
                    let raw_val = value.clone();
                    let val = self.gen_expr(gctx, &raw_val).unwrap();

                    LLVMBuildStore(self.builder, val, curr_alloca_instr);
                    Some(val)
                }
            }
            Ast::ClassConstrExpr {
                meta: _,
                ty_rec: _,
                class_name,
                ..
            } => match self.classtab.retrieve(&class_name) {
                Some(ty_ref) => {
                    let c_name = self.c_str(&class_name);
                    unsafe {
                        let llvm_val = LLVMBuildAlloca(self.builder, ty_ref, c_name);
                        return Some(llvm_val);
                    }
                }
                None => {
                    self.error(GenErrTy::InvalidClass(class_name.clone()));
                    None
                }
            },
            Ast::ClassPropAccessExpr {
                meta: _,
                ty_rec: _,
                ident_tkn,
                prop_name,
                idx,
                ..
            } => self.class_prop_expr(gctx, ident_tkn, prop_name, *idx, None),
            Ast::ClassPropSetExpr {
                meta: _,
                ty_rec: _,
                ident_tkn,
                prop_name,
                idx,
                owner_class: _,
                assign_val,
            } => self.class_prop_expr(gctx, ident_tkn, prop_name, *idx, Some(assign_val)),
            _ => unimplemented!("Ast type {:#?} is not implemented for codegen", expr),
        }
    }

    /// Generate LLVM IR for a primary expression. This returns an Option because
    /// it's possible that we can't retrieve an identifier from the value table (if it's
    /// undefined).
    fn primary_expr(
        &mut self,
        gctx: &mut GenCtx,
        ty_rec: &TyRecord,
        is_self: bool,
    ) -> Option<LLVMValueRef> {
        match ty_rec.tkn.ty {
            TknTy::Val(ref val) => unsafe { Some(LLVMConstReal(self.double_ty(), *val)) },
            TknTy::Str(ref lit) => unsafe {
                Some(LLVMBuildGlobalStringPtr(
                    self.builder,
                    self.c_str(lit),
                    self.c_str(""),
                ))
            },
            TknTy::True => unsafe { Some(LLVMConstInt(self.i8_ty(), 1, LLVM_FALSE)) },
            TknTy::False => unsafe { Some(LLVMConstInt(self.i8_ty(), 0, LLVM_FALSE)) },
            TknTy::Ident(ref name) => match self.valtab.retrieve(name) {
                Some(val) => unsafe {
                    let c_name = self.c_str(&name);
                    Some(LLVMBuildLoad(self.builder, val, c_name))
                },
                None if is_self => {
                    // In this branch, we don't have a value in the value table, but
                    // the variable belongs to self, which means it's declared inside
                    // a class we're generating code for. We need to get the position
                    // of the property, as well as the pointer to self from the gen context.
                    // Then, we build a GEP instruction to get the class property, and then
                    // load it.
                    let c_name = self.c_str(&name);
                    let pos = gctx.clsctx.curr_props.get(name).unwrap();
                    let ptr = gctx.clsctx.curr_self.unwrap();

                    unsafe {
                        let gep_val = LLVMBuildStructGEP(self.builder, ptr, *pos as u32, c_name);
                        let ld_val = LLVMBuildLoad(self.builder, gep_val, self.c_str(&name));
                        Some(ld_val)
                    }
                }
                None => None,
            },
            _ => unimplemented!("Tkn ty {:?} is unimplemented in codegen", ty_rec.tkn.ty),
        }
    }

    /// Generate LLVM IR for an if statement. This handles elif and else conditions as well.
    /// Returns a vector of LLVM values that are created during generation. If there are no
    /// values created, returns an empty vector.
    fn if_stmt(
        &mut self,
        gctx: &mut GenCtx,
        if_cond: &Box<Ast>,
        then_stmts: &Box<Ast>,
        else_if_stmts: &Vec<Ast>,
        else_stmts: &Vec<Ast>,
    ) -> Vec<LLVMValueRef> {
        let mut return_stmt_vec = Vec::new();
        unsafe {
            let has_elif = else_if_stmts.len() > 0;
            let has_else = else_stmts.len() > 0 && else_stmts.len() < 2;

            // Set up our required blocks. We need an initial block to start building
            // from (insert_bb), and a block representing the then branch (then_bb), which
            // is always present. We always keep an else block for conditional branching,
            // and a merge block to branch to after we have evaluated all the code in the
            // if statement. Blocks are manually reordered here as well, to account
            // for any nested if statements. If there is no nesting, these re-orders
            // effectively do nothing.
            let insert_bb = LLVMGetInsertBlock(self.builder);
            let fn_val = LLVMGetBasicBlockParent(insert_bb);

            let then_bb = LLVMAppendBasicBlockInContext(self.context, fn_val, self.c_str("then"));
            LLVMMoveBasicBlockAfter(then_bb, insert_bb);

            let else_bb = LLVMAppendBasicBlockInContext(self.context, fn_val, self.c_str("el"));
            LLVMMoveBasicBlockAfter(else_bb, then_bb);

            let merge_bb = LLVMAppendBasicBlockInContext(self.context, fn_val, self.c_str("merge"));
            LLVMMoveBasicBlockAfter(merge_bb, else_bb);

            // Build any necessary blocks for elif conditions. This is a vector of conditional blocks
            // that we use to decide branching instructions.
            let mut elif_bb_vec = Vec::new();
            for i in 0..else_if_stmts.len() {
                let name = format!("{}{}{}", "elifcond", i, "\0");
                let tmp_bb = LLVMAppendBasicBlockInContext(
                    self.context,
                    fn_val,
                    name.as_bytes().as_ptr() as *const i8,
                );
                elif_bb_vec.push(tmp_bb);
            }

            // Move position to end of merge block to create our phi block at the end of the
            // conditional. We immediately move it back to the start of the conditional so
            // we're still in the correct position.
            LLVMPositionBuilderAtEnd(self.builder, merge_bb);
            let phi_bb = LLVMBuildPhi(self.builder, self.double_ty(), self.c_str("phi"));
            LLVMPositionBuilderAtEnd(self.builder, insert_bb);

            // Calculate the LLVMValueRef for the if conditional expression. We use this
            // to build a conditional branch from the then block to the else block, if needed.
            let cond_val = self.gen_expr(gctx, &if_cond.clone());
            if cond_val.is_none() {
                self.error(GenErrTy::InvalidAst);
                return Vec::new();
            }

            // Build the conditional branch from the then block to the next required block. If we
            // have any else ifs, we branch to the first else if conditional block, otherwise
            // we check if there is an else block. If so, we branch there. If not, we branch to the
            // merge block.
            LLVMPositionBuilderAtEnd(self.builder, insert_bb);
            let else_cond_br = match has_elif {
                true => elif_bb_vec[0],
                false => match has_else {
                    true => else_bb,
                    false => merge_bb,
                },
            };
            LLVMBuildCondBr(self.builder, cond_val.unwrap(), then_bb, else_cond_br);

            // Build then block values and branch to merge block from inside the then block.
            LLVMPositionBuilderAtEnd(self.builder, then_bb);
            let mut then_expr_vals = self.gen_stmt(gctx, &then_stmts.clone());
            return_stmt_vec.extend(then_expr_vals.clone());
            LLVMBuildBr(self.builder, merge_bb);

            let then_end_bb = LLVMGetInsertBlock(self.builder);
            LLVMPositionBuilderAtEnd(self.builder, merge_bb);
            if then_expr_vals.len() > 0 {
                LLVMAddIncoming(
                    phi_bb,
                    then_expr_vals.as_mut_ptr(),
                    vec![then_end_bb].as_mut_ptr(),
                    1,
                );
            }

            // Generate blocks for any elif statements.
            // This block is used to correctly position the else block, if any. We want the
            // else block to sit after the elifs, and not after the then block.
            let mut final_elif_bb = then_bb;
            for (idx, stmt) in else_if_stmts.iter().enumerate() {
                match stmt.clone() {
                    Ast::ElifStmt {
                        meta: _,
                        cond_expr,
                        stmts,
                    } => {
                        // Get the conditional block from the vector made above. Create a seperate
                        // block to the elif code to live in, that we can branch to from the
                        // elif conditioanl block.
                        let elif_cond_bb = elif_bb_vec[idx];
                        LLVMPositionBuilderAtEnd(self.builder, elif_cond_bb);
                        LLVMMoveBasicBlockAfter(elif_cond_bb, else_bb);
                        let name = format!("{}{}{}", "elifblck", idx, "\0");
                        let elif_code_bb = LLVMAppendBasicBlockInContext(
                            self.context,
                            fn_val,
                            name.as_ptr() as *const i8,
                        );

                        LLVMMoveBasicBlockAfter(elif_code_bb, elif_cond_bb);

                        let elif_cond_val = self.gen_expr(gctx, &cond_expr.clone());
                        if elif_cond_val.is_none() {
                            self.error(GenErrTy::InvalidAst);
                            continue;
                        }

                        // If we're in the last elif block, we want to branch to the else block.
                        // If there's no else block, we branch to the merge block. If we're not
                        // in the last elif block,  we branch to the next elif conditional block
                        // in the elif block vector.
                        LLVMPositionBuilderAtEnd(self.builder, elif_cond_bb);
                        let else_cond_br = match idx == else_if_stmts.len() - 1 {
                            true => match has_else {
                                true => else_bb,
                                false => merge_bb,
                            },
                            false => elif_bb_vec[idx + 1],
                        };

                        LLVMBuildCondBr(
                            self.builder,
                            elif_cond_val.unwrap(),
                            elif_code_bb,
                            else_cond_br,
                        );
                        LLVMPositionBuilderAtEnd(self.builder, elif_code_bb);

                        // Evaluate the elif block statements and branch to the merge block
                        // from inside the elif block.
                        let mut elif_expr_vals = self.gen_stmt(gctx, &stmts.clone());
                        return_stmt_vec.extend(elif_expr_vals.clone());
                        LLVMBuildBr(self.builder, merge_bb);
                        let elif_end_bb = LLVMGetInsertBlock(self.builder);
                        LLVMPositionBuilderAtEnd(self.builder, merge_bb);
                        LLVMAddIncoming(
                            phi_bb,
                            elif_expr_vals.as_mut_ptr(),
                            vec![elif_end_bb].as_mut_ptr(),
                            1,
                        );
                        LLVMPositionBuilderAtEnd(self.builder, elif_code_bb);
                        final_elif_bb = elif_code_bb;
                    }
                    _ => (),
                }
            }

            // Generate code for the else block, if we have one.
            if has_else {
                LLVMMoveBasicBlockAfter(else_bb, final_elif_bb);
                LLVMPositionBuilderAtEnd(self.builder, else_bb);
                let mut else_expr_vals = self.gen_stmt(gctx, &else_stmts[0]);
                return_stmt_vec.extend(else_expr_vals.clone());

                LLVMBuildBr(self.builder, merge_bb);
                let else_end_bb = LLVMGetInsertBlock(self.builder);
                LLVMPositionBuilderAtEnd(self.builder, merge_bb);
                LLVMAddIncoming(
                    phi_bb,
                    else_expr_vals.as_mut_ptr(),
                    vec![else_end_bb].as_mut_ptr(),
                    1,
                );
            } else {
                LLVMPositionBuilderAtEnd(self.builder, merge_bb);
            }

            return_stmt_vec
        }
    }

    /// Generates LLVM IR for a while loop statement, and returns a vector of values
    /// that are created during that code gen. If there are no values, the vector is
    /// empty.
    fn while_stmt(
        &mut self,
        gctx: &mut GenCtx,
        cond_expr: &Box<Ast>,
        stmts: &Box<Ast>,
    ) -> Vec<LLVMValueRef> {
        let mut return_stmt_vec = Vec::new();
        unsafe {
            let insert_bb = LLVMGetInsertBlock(self.builder);
            let fn_val = LLVMGetBasicBlockParent(insert_bb);

            // Set up our blocks
            let while_bb = LLVMAppendBasicBlockInContext(self.context, fn_val, self.c_str("while"));
            let merge_bb = LLVMAppendBasicBlockInContext(self.context, fn_val, self.c_str("merge"));
            LLVMPositionBuilderAtEnd(self.builder, insert_bb);

            // Evaluate the conditional expression
            let cond_val = self.gen_expr(gctx, &cond_expr.clone());
            if cond_val.is_none() {
                self.error(GenErrTy::InvalidAst);
                return Vec::new();
            }

            // Buld the conditional branch
            LLVMBuildCondBr(self.builder, cond_val.unwrap(), while_bb, merge_bb);
            LLVMPositionBuilderAtEnd(self.builder, while_bb);

            let stmt_vals = self.gen_stmt(gctx, &stmts.clone());
            return_stmt_vec.extend(stmt_vals.clone());

            // Evaluate the conditional expression again. This will handle reading
            // the updated loop variable (if any) to properly branch out of the loop
            // if necessary. We build another conditional branch in the loop to handle
            // this.
            let updated_cond_val = self.gen_expr(gctx, &cond_expr.clone());
            LLVMBuildCondBr(self.builder, updated_cond_val.unwrap(), while_bb, merge_bb);
            let _ = LLVMGetInsertBlock(self.builder);
            LLVMPositionBuilderAtEnd(self.builder, merge_bb);
        }

        return_stmt_vec
    }

    /// Generates LLVM IR for a for loop statement, and returns a vector of values
    /// that are created during that code gen. If there are no values, the vector is
    /// empty.
    fn for_stmt(
        &mut self,
        gctx: &mut GenCtx,
        for_var_decl: &Box<Ast>,
        for_cond_expr: &Box<Ast>,
        for_step_expr: &Box<Ast>,
        stmts: &Box<Ast>,
    ) -> Vec<LLVMValueRef> {
        let mut return_stmt_vec = Vec::new();

        unsafe {
            let insert_bb = LLVMGetInsertBlock(self.builder);
            let fn_val = LLVMGetBasicBlockParent(insert_bb);

            let entry_bb = LLVMAppendBasicBlockInContext(self.context, fn_val, self.c_str("entry"));
            let for_bb = LLVMAppendBasicBlockInContext(self.context, fn_val, self.c_str("for"));
            let merge_bb = LLVMAppendBasicBlockInContext(self.context, fn_val, self.c_str("merge"));

            LLVMPositionBuilderAtEnd(self.builder, merge_bb);
            let phi_bb = LLVMBuildPhi(self.builder, self.double_ty(), self.c_str("phi"));
            LLVMPositionBuilderAtEnd(self.builder, entry_bb);

            // Codegen the var declaration and save the loop counter variable. We do this
            // first to store the loop var and to make sure it's allocated.
            self.gen_stmt(gctx, &for_var_decl.clone());
            LLVMBuildBr(self.builder, for_bb);
            LLVMPositionBuilderAtEnd(self.builder, for_bb);

            // Codegen the for loop body
            let mut stmt_vals = self.gen_stmt(gctx, &stmts.clone());
            return_stmt_vec.extend(stmt_vals.clone());

            // Codegen the loop step counter
            self.gen_stmt(gctx, &for_step_expr.clone());

            // Codegen the conditional for exit the loop
            let cond_val = self.gen_stmt(gctx, &for_cond_expr.clone())[0];
            LLVMBuildCondBr(self.builder, cond_val, for_bb, merge_bb);

            let for_end_bb = LLVMGetInsertBlock(self.builder);
            LLVMPositionBuilderAtEnd(self.builder, merge_bb);
            LLVMAddIncoming(
                phi_bb,
                stmt_vals.as_mut_ptr(),
                vec![for_end_bb].as_mut_ptr(),
                1,
            );
        }

        return_stmt_vec
    }

    /// Generate LLVM IR for a function declaration statement. If no values are generated,
    /// an empty vector is returned.
    fn fn_decl_stmt(
        &mut self,
        gctx: &mut GenCtx,
        ident_tkn: &Token,
        fn_params: &Vec<TyRecord>,
        ret_ty: &TyRecord,
        fn_body: &Box<Ast>,
    ) -> Vec<LLVMValueRef> {
        self.valtab.init_sc();

        let fn_name = self.c_str(&ident_tkn.get_name());
        let fn_ty = self.llvm_ty_from_ty_rec(ret_ty, false);

        // Convert our params to an array of LLVMTypeRef's. We then pass these
        // types to the function to encode the types of our params. After we create
        // our function, we can add it to the builder and position it at
        // the end of the new basic block.
        let mut param_tys = self.llvm_tys_from_ty_rec_arr(fn_params, true);

        unsafe {
            let llvm_fn_ty = LLVMFunctionType(
                fn_ty,
                param_tys.as_mut_ptr(),
                param_tys.len() as u32,
                LLVM_FALSE,
            );

            let llvm_fn = LLVMAddFunction(self.module, fn_name, llvm_fn_ty);
            let fn_val = LLVMAppendBasicBlockInContext(self.context, llvm_fn, fn_name);
            LLVMPositionBuilderAtEnd(self.builder, fn_val);

            // Get the params from the function we created. This is a little weird since
            // we pass in an array of LLVMTypeRef's to the function, but we want
            // LLVMValueRef's to store in the symbol table and to give them names. We need
            // to get the params and loop through them again.
            let llvm_params: *mut LLVMValueRef = Vec::with_capacity(param_tys.len()).as_mut_ptr();
            LLVMGetParams(llvm_fn, llvm_params);
            let param_value_vec: Vec<LLVMValueRef> =
                slice::from_raw_parts(llvm_params, param_tys.len()).to_vec();

            for (idx, param) in param_value_vec.iter().enumerate() {
                let name = &fn_params[idx].tkn.get_name();
                let c_name = self.c_str(name);
                LLVMSetValueName2(*param, c_name, name.len());
                if name == "self" {
                    // set gctx.currself here
                    gctx.clsctx.curr_self = Some(*param);
                }

                // If the param is a pointer, we dont want to
                // build an alloca/store for it.
                if LLVMGetTypeKind(LLVMTypeOf(*param)) == LLVMTypeKind::LLVMPointerTypeKind {
                    continue;
                }

                let alloca_instr = self.build_entry_bb_alloca(
                    llvm_fn,
                    fn_params[idx].clone(),
                    &fn_params[idx].tkn.get_name(),
                );

                LLVMBuildStore(self.builder, *param, alloca_instr);
                self.valtab
                    .store(&fn_params[idx].tkn.get_name(), alloca_instr);
            }

            // Store the function symbol inside the value table before parsing the
            // body, so we can accept recursive calls.
            self.valtab.store(&ident_tkn.get_name(), llvm_fn);

            // Iterate the function body and generate ir for the statements within. We also
            // generate the IR for the return expression here.
            match *fn_body.clone() {
                Ast::BlckStmt {
                    meta: _,
                    stmts,
                    sc: _,
                } => self.fn_body(gctx, &stmts),
                // Skip anything that isn't a block statement (there shouldn't be
                // anything that isn't or we'd have a parse error).
                _ => (),
            }

            // Run the function pass through our manager
            // TODO: this is commented out because of compile times
            //self.fpm.run(llvm_fn);

            // Close the function level scope, which will pop off any params and
            // variable declared here (we don't need these anymore, since we aren't
            // going to be making another pass over them later). Add the llvm function
            // to the value table so we can look it up later for a call.
            self.valtab.close_sc();
            self.valtab.store(&ident_tkn.get_name(), llvm_fn);
        }

        Vec::new()
    }

    /// Generate LLVM IR for a function body. This iterates all function statements
    /// and calls gen_stmts() for them. Also builds return statements when it
    /// finds them. Returns nothing as the actual generation is handled by gen_stmt().
    fn fn_body(&mut self, gctx: &mut GenCtx, stmts: &Vec<Ast>) {
        for stmt in stmts {
            // We have the return type already, but we don't know
            // if the function returns an expression we need to generate as well.
            // We find the return statement and either return a null ptr (for void)
            // or generate IR for a return expression.
            match *stmt {
                Ast::RetStmt {
                    meta: _,
                    ref ret_expr,
                } if ret_expr.is_none() => unsafe {
                    LLVMBuildRet(self.builder, ptr::null_mut());
                },
                Ast::RetStmt {
                    meta: _,
                    ref ret_expr,
                } if ret_expr.is_some() => {
                    let llvm_val = self.gen_expr(gctx, &ret_expr.clone().unwrap());
                    unsafe {
                        LLVMBuildRet(self.builder, llvm_val.unwrap());
                    }
                }
                _ => {
                    self.gen_stmt(gctx, &stmt);
                }
            }
        }
    }

    /// Generate LLVM IR for a variable assign expression block. Also calls
    /// gen_expr() to recursively generate IR for inner expressions.
    /// This returns a vector of LLVMValue's based on what the contained expressions evaluate to.
    fn var_assign_expr(
        &mut self,
        gctx: &mut GenCtx,
        ty_rec: &TyRecord,
        ident_tkn: &Token,
        is_global: bool,
        value: &Box<Ast>,
    ) -> Vec<LLVMValueRef> {
        // We match on the is_global flag because we need to treat global vars
        // differently from non-globals.
        //
        // For global variable assignments, we want to register a new global using LLVMAddGlobal,
        // and then set the initializer.
        // For non-globals, we find the nearest insert block and build a store instruction
        // to hold the variable in that block.
        match is_global {
            true => self.global_var_assign(gctx, ty_rec, ident_tkn, value),
            false => self.local_var_assign(gctx, ty_rec, ident_tkn, value),
        }
    }

    /// Generate LLVM IR for global variable assignments. Returns a vector of LLVMValueRefs,
    /// which are the values potentially generated by expressions within the assignment.
    fn global_var_assign(
        &mut self,
        gctx: &mut GenCtx,
        ty_rec: &TyRecord,
        ident_tkn: &Token,
        value: &Box<Ast>,
    ) -> Vec<LLVMValueRef> {
        let c_name = self.c_str(&ident_tkn.get_name());
        let var_ident = ident_tkn.get_name();

        // For global class constructors, we add the global without an initializer.
        // We also have to find the class from the class table to ensure we aren't
        // trying to create a class object that isn't defined.
        match *value.clone() {
            Ast::ClassConstrExpr {
                meta: _,
                ty_rec: _,
                class_name,
                ..
            } => {
                let llvm_ty = self.classtab.retrieve(&class_name);
                if llvm_ty.is_none() {
                    self.error(GenErrTy::InvalidClass(ident_tkn.get_name()));
                    return Vec::new();
                }
                unsafe {
                    let global = LLVMAddGlobal(self.module, llvm_ty.unwrap(), c_name);
                    self.valtab.store(&var_ident, global);
                    vec![global]
                }
            }
            _ => unsafe {
                // For other variabel types, create a global and set the initializer.
                // We are certain to have an initializer here, because we're parsing
                // a var assign, and not a var decl.
                let llvm_ty = self.llvm_ty_from_ty_rec(ty_rec, false);
                let global = LLVMAddGlobal(self.module, llvm_ty, c_name);

                let val = self.gen_expr(gctx, &value.clone()).unwrap();
                // TODO: this doesn't work for class prop accesses, because it
                // returns a load instruction
                LLVMSetInitializer(global, val);
                self.valtab.store(&ident_tkn.get_name(), global);
                vec![global]
            },
        }
    }

    /// Generate LLVM IR for local variable assignments. Alloca/store
    /// instructions are built for local vars, with the exception of classes.
    /// Returns a vector of LLVMValueRefs, which are the values potentially
    /// generated by expressions within the assignment.
    fn local_var_assign(
        &mut self,
        gctx: &mut GenCtx,
        ty_rec: &TyRecord,
        ident_tkn: &Token,
        value: &Box<Ast>,
    ) -> Vec<LLVMValueRef> {
        unsafe {
            let insert_bb = LLVMGetInsertBlock(self.builder);
            let llvm_func = LLVMGetBasicBlockParent(insert_bb);
            let alloca_instr =
                self.build_entry_bb_alloca(llvm_func, ty_rec.clone(), &ident_tkn.get_name());

            let raw_val = value.clone();
            // We don't need to store anything for class types, since they
            // are already built into structs in the class declaration. The class
            // here should already be a struct type (if we tried to create a class
            // before declaring it we would not pass parsing).
            match *value.clone() {
                Ast::ClassConstrExpr { .. } => {
                    self.valtab.store(&ident_tkn.get_name(), alloca_instr);
                    vec![alloca_instr]
                }
                _ => {
                    let val = self.gen_expr(gctx, &raw_val).unwrap();
                    LLVMBuildStore(self.builder, val, alloca_instr);
                    self.valtab.store(&ident_tkn.get_name(), alloca_instr);
                    vec![alloca_instr]
                }
            }
        }
    }

    /// Generate IR for a class declaration. Classes are mapped to Structs in LLVM, so this
    /// creates a struct with each class property as a member of the struct. In order to
    /// generate IR for class methods, we manually add a new param to each method in the
    /// class declaration. This param is a pointer to the class contructor, so that class
    /// props can be accessed from the method.
    fn class_decl_stmt(
        &mut self,
        gctx: &mut GenCtx,
        ident_tkn: &Token,
        methods: &Vec<Ast>,
        props: &Vec<Ast>,
        prop_pos: &HashMap<String, usize>,
    ) -> Vec<LLVMValueRef> {
        let mut prop_tys = Vec::new();

        for pr in props {
            // Here we just want to lay out the props,
            // we don't actually want to allocate them until we
            // create an object of this class.
            // So, we want the llvm type of the props, but we
            // don't want to generate any code for them yet.
            match &pr {
                Ast::VarDeclExpr {
                    meta: _, ty_rec, ..
                } => {
                    let llvm_ty = self.llvm_ty_from_ty_rec(ty_rec, false);
                    prop_tys.push(llvm_ty);
                }
                _ => (),
            }
        }

        let class_name = ident_tkn.get_name();
        unsafe {
            let llvm_struct = LLVMStructCreateNamed(self.context, self.c_str(&class_name));
            LLVMStructSetBody(
                llvm_struct,
                prop_tys.as_mut_ptr(),
                prop_tys.len() as u32,
                LLVM_FALSE,
            );

            // Store the struct type in a special class table, so we can look it up
            // later when we want to allocate one. This is not the same as a the value table,
            // as it doesn't represent an allocated value, just the type info for the class.
            // Note: This must be stored before we process the class method declarations,
            // because they need to look up the class name from the symbol table in order
            // to insert the class as a 'self' param.
            self.classtab.store(&class_name, llvm_struct);
        }

        gctx.clsctx.curr_cls = class_name.clone();
        gctx.clsctx.curr_props = prop_pos.clone();

        // Methods are generated like any other method, but with a pointer to
        // the enclosing class as the first parameter ('self'). This pointer can be
        // used to access class variables and other class methods
        // These don't "belong" to the class in the llvm ir, but just
        // live anywhere in the output
        let class_tkn = ident_tkn;
        for mtod in methods {
            match &mtod {
                Ast::FnDeclStmt {
                    meta,
                    ident_tkn,
                    fn_params,
                    ret_ty,
                    fn_body,
                    ..
                } => {
                    // We need to add the class declaration type to the list of
                    // params so we obtain a pointer to it inside the method body.
                    // The name isn't important here, as long the ty value is correct.
                    let mut param_tkn = class_tkn.clone();
                    param_tkn.ty = TknTy::Ident("self".to_string());

                    let fake_class_param = TyRecord {
                        name: "self".to_string(),
                        ty: KolgaTy::Class(class_name.clone()),
                        tkn: param_tkn,
                    };

                    let mut new_params = fn_params.clone();
                    new_params.insert(0, fake_class_param);

                    // We change the name of the function by prepending the
                    // class name so we avoid storing duplicates in the value table.
                    // This is kind of a hack, but in a normal program you can't create
                    // a function name with a period in it, because it would probably
                    // be parsed as a property anyway.
                    let curr_name = ident_tkn.get_name();
                    let new_name = format!("{}.{}", class_tkn.get_name(), curr_name);
                    let new_tkn = Token::new(TknTy::Ident(new_name), ident_tkn.line, ident_tkn.pos);

                    let new_method = Ast::FnDeclStmt {
                        meta: meta.clone(),
                        ident_tkn: new_tkn,
                        fn_params: new_params,
                        ret_ty: ret_ty.clone(),
                        fn_body: fn_body.clone(),
                        sc: 0,
                    };

                    self.gen_stmt(gctx, &new_method);
                }
                _ => (),
            }
        }

        gctx.clsctx.reset();

        Vec::new()
    }

    /// Generate LLVM IR for unary expressions. Returns the value generated or None
    /// if there is no value or on error.
    fn unary_expr(
        &mut self,
        gctx: &mut GenCtx,
        op_tkn: &Token,
        rhs: &Box<Ast>,
    ) -> Option<LLVMValueRef> {
        // Recursively generate LLVM value for the rhs of the expression. If there
        // is an error when generating, return None.
        let mb_rhs_llvm_val = self.gen_expr(gctx, &rhs);
        if mb_rhs_llvm_val.is_none() {
            return None;
        }

        let rhs_llvm_val = mb_rhs_llvm_val.unwrap();

        // Build the correct instruction by matching on the unary operator. For unary
        // minus, we build a neg instruction, and for unary logical negation, we
        // use and xor to flip the boolean value.
        match op_tkn.ty {
            TknTy::Minus => unsafe {
                Some(LLVMBuildFNeg(
                    self.builder,
                    rhs_llvm_val,
                    self.c_str("tmpneg"),
                ))
            },
            TknTy::Bang => {
                unsafe {
                    // There isn't any logical not instruction, so we use XOR to
                    // flip the value (which is of type i8 now) from 0/1 to represent
                    // the opposite boolean value.
                    let xor_rhs = LLVMConstInt(self.i8_ty(), 1, LLVM_FALSE);
                    Some(LLVMBuildXor(
                        self.builder,
                        rhs_llvm_val,
                        xor_rhs,
                        self.c_str("tmpnot"),
                    ))
                }
            }
            _ => None,
        }
    }

    /// Generate LLVM IR for function calls. Returns the value generated or None
    /// if there is no value or on error.
    fn fn_call_expr(
        &mut self,
        gctx: &mut GenCtx,
        fn_tkn: &Token,
        fn_params: &Vec<Ast>,
    ) -> Option<LLVMValueRef> {
        // Check if the function was defined in the IR. We should always have
        // the function defined in the IR though, since we wouldn't pass the parsing
        // phase if we tried to call an undefined function name.
        let fn_name = fn_tkn.clone().get_name();
        let llvm_fn = self.valtab.retrieve(&fn_name);
        if llvm_fn.is_none() {
            self.error(GenErrTy::InvalidFn(fn_name));
            return None;
        }

        // Recursively generate LLVMValueRef's for the function params, which
        // might be non-primary expressions themselves. We store these in a vector,
        // so we can pass it to the LLVM IR function call instruction.
        let mut param_tys: Vec<LLVMValueRef> = Vec::new();
        for param in fn_params {
            let llvm_val = self.gen_expr(gctx, param);
            if llvm_val.is_none() {
                self.error(GenErrTy::InvalidFnParam);
                return None;
            }

            param_tys.push(llvm_val.unwrap());
        }

        unsafe {
            Some(LLVMBuildCall(
                self.builder,
                llvm_fn.unwrap(),
                param_tys.as_mut_ptr(),
                param_tys.len() as u32,
                self.c_str(""),
            ))
        }
    }

    /// Generate LLVM IR for class function calls. This is handeled separately from
    /// function calls because we need to look up additional information from the class
    /// (class variables, etc.) that are not present in the FnCall AST.
    fn class_fn_call_expr(
        &mut self,
        gctx: &mut GenCtx,
        class_tkn: &Token,
        class_name: &str,
        fn_tkn: &Token,
        fn_params: &Vec<Ast>,
    ) -> Option<LLVMValueRef> {
        // The class function is stored under a different name in the
        // value table, with the class name prepended.
        let fn_name = fn_tkn.get_name();
        let class_fn_name = format!("{}.{}", class_name, fn_name);
        let llvm_fn = self.valtab.retrieve(&class_fn_name);

        if llvm_fn.is_none() {
            self.error(GenErrTy::InvalidFn(fn_name));
            return None;
        }

        // We need to insert a pointer to the class instance as the first param in order to
        // call the class function. We get that pointer from the value table (the pointer
        // is the actual instance of the class that has been created).
        let mut fn_args: Vec<LLVMValueRef> = Vec::new();
        let class_instance = self.valtab.retrieve(&class_tkn.get_name());
        fn_args.push(class_instance.unwrap());

        // Recursively generate any LLVMValue's from the function params, as they
        // may be expressions themselves.
        for param in fn_params {
            let llvm_val = self.gen_expr(gctx, param);
            if llvm_val.is_none() {
                self.error(GenErrTy::InvalidFnParam);
                return None;
            }

            fn_args.push(llvm_val.unwrap());
        }

        unsafe {
            Some(LLVMBuildCall(
                self.builder,
                llvm_fn.unwrap(),
                fn_args.as_mut_ptr(),
                fn_args.len() as u32,
                self.c_str(""),
            ))
        }
    }

    fn class_prop_expr(
        &mut self,
        gctx: &mut GenCtx,
        ident_tkn: &Token,
        prop_name: &str,
        idx: usize,
        assign_val: Option<&Box<Ast>>,
    ) -> Option<LLVMValueRef> {
        let name = ident_tkn.get_name();
        let class = self.valtab.retrieve(&name);
        if class.is_none() {
            self.error(GenErrTy::InvalidClass(name));
            return None;
        }

        let classptr = class.unwrap();
        let c_name = self.c_str(prop_name);
        unsafe {
            let gep_val = LLVMBuildStructGEP(self.builder, classptr, idx as u32, c_name);

            match assign_val {
                Some(ref ast) => {
                    let assign = self.gen_expr(gctx, ast).unwrap();
                    let store_val = LLVMBuildStore(self.builder, assign, gep_val);
                    Some(store_val)
                }
                None => {
                    // GEP returns the address of the prop we want to access. We can load it
                    // into a variable here so that we return a non-pointer type.
                    // TODO: can this be set as a global variable?
                    let ld_val = LLVMBuildLoad(self.builder, gep_val, c_name);
                    Some(ld_val)
                }
            }
        }
    }

    /// Builds an alloca instruction at the beginning of a function so we can store
    /// parameters on the function stack. This uses a new builder so the current builder
    /// doesn't move positions. We would have to move it back to its original spot, which
    /// makes this function more complex than it needs to be.
    fn build_entry_bb_alloca(
        &mut self,
        func: LLVMValueRef,
        ty_rec: TyRecord,
        name: &str,
    ) -> LLVMValueRef {
        unsafe {
            let builder = LLVMCreateBuilderInContext(self.context);
            let entry_bb = LLVMGetEntryBasicBlock(func);
            let entry_first_instr = LLVMGetFirstInstruction(entry_bb);
            LLVMPositionBuilder(builder, entry_bb, entry_first_instr);

            let llvm_ty = self.llvm_ty_from_ty_rec(&ty_rec, false);
            let c_name = self.c_str(name);

            LLVMBuildAlloca(builder, llvm_ty, c_name)
        }
    }

    /// Converts a TyRecord type to an LLVMTypeRef. If class_to_ptr is true,
    /// class types are returned as pointers to that class in LLVM.
    fn llvm_ty_from_ty_rec(&self, ty_rec: &TyRecord, class_to_ptr: bool) -> LLVMTypeRef {
        match ty_rec.ty.clone() {
            KolgaTy::String => self.str_ty(),
            KolgaTy::Num => self.double_ty(),
            KolgaTy::Bool => self.i8_ty(),
            KolgaTy::Void => self.void_ty(),
            KolgaTy::Class(name) => {
                if class_to_ptr {
                    return self.ptr_ty(self.classtab.retrieve(&name).unwrap());
                }

                self.classtab.retrieve(&name).unwrap()
            }
            KolgaTy::Symbolic(_) => panic!("Found a type in codegen that wasn't inferred!"),
        }
    }

    /// Converts a vector of TyRecords into a vector of LLVMTypeRefs. If class_to_ptr is set
    /// to true, class type params are converted to pointers to the class.
    fn llvm_tys_from_ty_rec_arr(
        &self,
        ty_recs: &Vec<TyRecord>,
        class_to_ptr: bool,
    ) -> Vec<LLVMTypeRef> {
        let mut llvm_tys = Vec::new();
        for ty_rec in ty_recs {
            llvm_tys.push(self.llvm_ty_from_ty_rec(&ty_rec, class_to_ptr));
        }

        llvm_tys
    }

    /// Creates a new LLVMValueRef from a binary expression. The type of LLVM IR is determined by
    /// the operator type passed in. We assume that the LHS and RHS values given here are fully
    /// generated already. Comparison instructions are built from each function argument, if the
    /// operator given is of the logical type.
    /// We return None if the operator given is not supported.
    fn llvm_val_from_op(
        &mut self,
        op: &TknTy,
        lhs: LLVMValueRef,
        rhs: LLVMValueRef,
    ) -> Option<LLVMValueRef> {
        unsafe {
            match op {
                TknTy::Plus => Some(LLVMBuildFAdd(self.builder, lhs, rhs, self.c_str("addtmp"))),
                TknTy::Minus => Some(LLVMBuildFSub(self.builder, lhs, rhs, self.c_str("subtmp"))),
                TknTy::Star => Some(LLVMBuildFMul(self.builder, lhs, rhs, self.c_str("multmp"))),
                TknTy::Slash => Some(LLVMBuildFDiv(self.builder, lhs, rhs, self.c_str("divtmp"))),
                TknTy::AmpAmp | TknTy::And => {
                    Some(LLVMBuildAnd(self.builder, lhs, rhs, self.c_str("andtmp")))
                }
                TknTy::PipePipe | TknTy::Or => {
                    Some(LLVMBuildOr(self.builder, lhs, rhs, self.c_str("ortmp")))
                }
                TknTy::Lt => Some(LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealULT,
                    lhs,
                    rhs,
                    self.c_str("lttmp"),
                )),
                TknTy::Gt => Some(LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealUGT,
                    lhs,
                    rhs,
                    self.c_str("gttmp"),
                )),
                TknTy::LtEq => Some(LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealULE,
                    lhs,
                    rhs,
                    self.c_str("ltetmp"),
                )),
                TknTy::GtEq => Some(LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealUGE,
                    lhs,
                    rhs,
                    self.c_str("gtetmp"),
                )),
                TknTy::EqEq => Some(LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealUEQ,
                    lhs,
                    rhs,
                    self.c_str("eqtmp"),
                )),
                TknTy::BangEq => Some(LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealUNE,
                    lhs,
                    rhs,
                    self.c_str("neqtmp"),
                )),
                _ => None,
            }
        }
    }

    fn void_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMVoidTypeInContext(self.context) }
    }

    fn str_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMPointerType(self.i8_ty(), 0) }
    }

    fn double_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMDoubleTypeInContext(self.context) }
    }

    fn i8_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMInt8TypeInContext(self.context) }
    }

    fn ptr_ty(&self, ty: LLVMTypeRef) -> LLVMTypeRef {
        unsafe { LLVMPointerType(ty, 0) }
    }

    fn c_str(&mut self, s: &str) -> *mut i8 {
        let cstr = CString::new(s).unwrap();
        let cstr_ptr = cstr.as_ptr() as *mut _;
        self.strings.push(cstr);

        cstr_ptr
    }

    fn error(&mut self, ty: GenErrTy) {
        let err = GenErr::new(ty);
        self.errors.push(err);
    }
}
