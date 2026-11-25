//! ABAP-Compiler: ABAP -> IR -> LLVM-Objektdatei -> EXE.

pub mod analyze;
pub mod ast;
pub mod inspect;
pub mod codegen;
pub mod lower;
pub mod symbols;
