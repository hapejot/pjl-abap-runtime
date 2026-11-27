//! ABAP-Compiler: ABAP -> IR -> LLVM-Objektdatei -> EXE.

pub mod analyze;
pub mod ast;
pub mod codegen;
pub mod ddic;
pub mod dynpro;
pub mod inspect;
pub mod library;
pub mod lower;
pub mod project;
pub mod symbols;
