//! LLVM-IR-Erzeugung über inkwell.
//!
//! Aufbau von `main`:
//!   entry  -- Allokation und Initialisierung aller Variablen, endet mit `br body`
//!   body   -- Programmcode
//!   exit   -- `ret 0`
//!
//! Werte: Ganzzahl (`i32`), Zeichenfeld (Zeiger + Länge in Zeichen), String (Handle der Runtime).
//! Interne Tabellen sind Handles auf `RtTable`; Feldsymbole sind Zeiger auf Zeilen oder Felder.

mod layout;
mod emit;
mod decl;
mod data;
mod routines;
mod stmt;
mod expr;
mod packed;

use std::collections::HashMap;
use std::fmt::Display;

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::types::{ArrayType, BasicMetadataTypeEnum, IntType, PointerType};
use inkwell::values::{ArrayValue, BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate};

use crate::ast::{err, BinOp, CmpOp, CompileError, Cond, Expr, NamedArg};
use crate::lower::{Arg, CatchClause, Elem, Form, Ir, LoopFilter, LoopTarget, Stmt, Target, Ty};

type CResult<T> = Result<T, CompileError>;

/// Fehler aus LLVM (Builder) in einen Übersetzungsfehler umwandeln.
fn llvm<T, E: Display>(r: Result<T, E>) -> CResult<T> {
    r.map_err(|e| CompileError { line: 0, message: format!("LLVM-Fehler: {e}") })
}

/// Layout einer Struktur: gepackte LLVM-Struktur mit den Komponenten in Reihenfolge.
#[derive(Clone)]
struct Layout<'ctx> {
    st: inkwell::types::StructType<'ctx>,
    fields: Vec<(String, Ty)>,
}

/// Methode einer Klasse: LLVM-Funktion und (Typ, CHANGING) je Parameter.
#[derive(Clone)]
struct MethodInfo<'ctx> {
    name: String,
    is_static: bool,
    f: FunctionValue<'ctx>,
    /// Formalparametername je Position
    pnames: Vec<String>,
    params: Vec<(Ty, bool)>,
    /// Rückgabetyp (RETURNING); der Rückgabewert wird über einen zusätzlichen Zeiger übergeben
    ret: Option<Ty>,
}

/// Klasse zur Laufzeit: Attribut-Layout und Methoden.
#[derive(Clone)]
struct ClassInfo<'ctx> {
    name: String,
    struct_idx: usize,
    parent: Option<usize>,
    external_parent: Option<String>,
    methods: Vec<MethodInfo<'ctx>>,
    aliases: HashMap<String, String>,
}

/// Ausgewerteter Ausdruck.
#[derive(Clone, Copy)]
enum Val<'ctx> {
    /// Objektreferenz: Zeiger auf das Objekt und Klassenindex
    Ref(PointerValue<'ctx>, usize),
    Table(PointerValue<'ctx>),
    /// Struktur: Zeiger auf den Speicher und Index des Strukturtyps
    Mem(PointerValue<'ctx>, usize),
    Int(IntValue<'ctx>),
    /// Gepackte Zahl: Handle auf einen Laufzeitwert `Dec`
    Dec(PointerValue<'ctx>),
    /// Zeiger auf Zeichen und deren Länge
    Chars(PointerValue<'ctx>, usize),
    /// Handle auf einen Runtime-String
    Str(PointerValue<'ctx>),
}

/// Speicherort eines Schreibziels.
#[derive(Clone, Copy)]
enum Dest<'ctx> {
    /// Objektreferenz-Speicherplatz und Klassenindex
    Ref(PointerValue<'ctx>, usize),
    Mem(PointerValue<'ctx>, usize),
    Int(PointerValue<'ctx>),
    /// Gepackte Zahl: Speicherplatz, Bytezahl, Nachkommastellen
    Packed(PointerValue<'ctx>, usize, usize),
    Chars(PointerValue<'ctx>, usize),
    /// Slot, der das String-Handle hält
    Str(PointerValue<'ctx>),
}

/// Signatur einer FORM: LLVM-Funktion und (Typ, CHANGING) je Parameter.
#[derive(Clone)]
struct FormSig<'ctx> {
    f: FunctionValue<'ctx>,
    params: Vec<(Ty, bool)>,
}

#[derive(Clone)]
struct Var<'ctx> {
    /// Speicherplatz der Variablen; bei Feldsymbolen der Zeiger auf das Ziel
    slot: PointerValue<'ctx>,
    ty: Ty,
    fs: bool,
    /// Typ-Tag (Zeiger auf den Typdeskriptor); nur bei Feldsymbolen
    tag: Option<PointerValue<'ctx>>,
}

/// Sprungziele einer Schleife: EXIT springt nach `exit`, CHECK nach `next`.
#[derive(Clone, Copy)]
struct LoopCtx<'ctx> {
    exit: BasicBlock<'ctx>,
    next: BasicBlock<'ctx>,
}

/// Deklarierte Runtime-Funktionen.
struct Rt<'ctx> {
    write_int: FunctionValue<'ctx>,
    dec_unpack: FunctionValue<'ctx>,
    dec_pack: FunctionValue<'ctx>,
    dec_from_int: FunctionValue<'ctx>,
    dec_add: FunctionValue<'ctx>,
    dec_sub: FunctionValue<'ctx>,
    dec_mul: FunctionValue<'ctx>,
    dec_div: FunctionValue<'ctx>,
    dec_mod: FunctionValue<'ctx>,
    dec_mark: FunctionValue<'ctx>,
    dec_release: FunctionValue<'ctx>,
    dec_neg: FunctionValue<'ctx>,
    dec_cmp: FunctionValue<'ctx>,
    dec_to_int: FunctionValue<'ctx>,
    dec_write: FunctionValue<'ctx>,
    write_chars: FunctionValue<'ctx>,
    write_str: FunctionValue<'ctx>,
    newline: FunctionValue<'ctx>,
    newfield: FunctionValue<'ctx>,
    finish: FunctionValue<'ctx>,
    chars_move: FunctionValue<'ctx>,
    chars_cmp: FunctionValue<'ctx>,
    str_new: FunctionValue<'ctx>,
    str_from_chars: FunctionValue<'ctx>,
    str_to_chars: FunctionValue<'ctx>,
    str_concat: FunctionValue<'ctx>,
    str_cmp: FunctionValue<'ctx>,
    int_to_str: FunctionValue<'ctx>,
    table_new: FunctionValue<'ctx>,
    table_append: FunctionValue<'ctx>,
    table_count: FunctionValue<'ctx>,
    table_row: FunctionValue<'ctx>,
    table_clear: FunctionValue<'ctx>,
    fs_unassigned: FunctionValue<'ctx>,
    type_mismatch: FunctionValue<'ctx>,
    obj_new: FunctionValue<'ctx>,
    unbound: FunctionValue<'ctx>,
    exception_raise: FunctionValue<'ctx>,
    exception_pending: FunctionValue<'ctx>,
    exception_class: FunctionValue<'ctx>,
    exception_object: FunctionValue<'ctx>,
    exception_clear: FunctionValue<'ctx>,
    external_call: FunctionValue<'ctx>,
    pattern_match: FunctionValue<'ctx>,
    str_contains: FunctionValue<'ctx>,
    str_contains_any: FunctionValue<'ctx>,
    str_contains_only: FunctionValue<'ctx>,
    str_length: FunctionValue<'ctx>,
    str_slice: FunctionValue<'ctx>,
}

pub struct Codegen<'ctx> {
    /// Aktuell festgelegter Typ dynamischer Feldsymbole innerhalb einer spezialisierten Anweisung
    fs_override: HashMap<String, Ty>,
    /// Typwelt für dynamische Feldsymbole
    dynamic_types: Vec<Ty>,
    ctx: &'ctx Context,
    pub module: Module<'ctx>,
    builder: Builder<'ctx>,
    /// Builder für Allokationen; positioniert am Ende von `entry`
    alloca: Builder<'ctx>,
    i32t: IntType<'ctx>,
    i64t: IntType<'ctx>,
    ptrt: PointerType<'ctx>,
    /// Programmvariablen (LLVM-Globals), in Routinen sichtbar
    globals: HashMap<String, Var<'ctx>>,
    /// Lokale Variablen der gerade erzeugten FORM
    vars: HashMap<String, Var<'ctx>>,
    in_form: bool,
    /// Initialisierung von Globals (Strings, Tabellen), vor `body` ausgeführt
    pending_init: Vec<(PointerValue<'ctx>, Ty)>,
    forms: HashMap<String, FormSig<'ctx>>,
    layouts: Vec<Layout<'ctx>>,
    classes: Vec<ClassInfo<'ctx>>,
    constants: HashMap<String, Expr>,
    /// Im Methodenrumpf: Speicherplatz des Objektzeigers (`me`) und Klassenindex
    self_ctx: Option<(PointerValue<'ctx>, usize)>,
    loops: Vec<LoopCtx<'ctx>>,
    /// Marken der gerade übersetzten Anweisungen (innen zuletzt): Grundlage für die Freigabe bei Sprüngen.
    stmt_marks: Vec<IntValue<'ctx>>,
    exception_target: Option<BasicBlock<'ctx>>,
    /// Ziel von CHECK außerhalb von Schleifen (Ende des Programms)
    exit_fn: Option<BasicBlock<'ctx>>,
    body: Option<BasicBlock<'ctx>>,
    rt: Rt<'ctx>,
    /// Gesammelte Fehler; die Erzeugung läuft nach einem Fehler weiter.
    errors: Vec<CompileError>,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(ctx: &'ctx Context) -> Self {
        let module = ctx.create_module("abap_main");
        let builder = ctx.create_builder();
        let alloca = ctx.create_builder();
        let void = ctx.void_type();
        let i32t = ctx.i32_type();
        let i64t = ctx.i64_type();
        let ptrt = ctx.ptr_type(AddressSpace::default());

        let f = |name: &str, ty| module.add_function(name, ty, Some(Linkage::External));
        let rt = Rt {
            write_int: f("abap_rt_write_int", void.fn_type(&[i32t.into()], false)),
            dec_unpack: f("abap_rt_dec_unpack", ptrt.fn_type(&[ptrt.into(), i64t.into(), i64t.into()], false)),
            dec_pack: f("abap_rt_dec_pack", void.fn_type(&[ptrt.into(), ptrt.into(), i64t.into(), i64t.into()], false)),
            dec_from_int: f("abap_rt_dec_from_int", ptrt.fn_type(&[i32t.into()], false)),
            dec_add: f("abap_rt_dec_add", ptrt.fn_type(&[ptrt.into(), ptrt.into()], false)),
            dec_sub: f("abap_rt_dec_sub", ptrt.fn_type(&[ptrt.into(), ptrt.into()], false)),
            dec_mul: f("abap_rt_dec_mul", ptrt.fn_type(&[ptrt.into(), ptrt.into()], false)),
            dec_div: f("abap_rt_dec_div", ptrt.fn_type(&[ptrt.into(), ptrt.into()], false)),
            dec_mod: f("abap_rt_dec_mod", ptrt.fn_type(&[ptrt.into(), ptrt.into()], false)),
            dec_mark: f("abap_rt_dec_mark", i64t.fn_type(&[], false)),
            dec_release: f("abap_rt_dec_release", void.fn_type(&[i64t.into()], false)),
            dec_neg: f("abap_rt_dec_neg", ptrt.fn_type(&[ptrt.into()], false)),
            dec_cmp: f("abap_rt_dec_cmp", i32t.fn_type(&[ptrt.into(), ptrt.into()], false)),
            dec_to_int: f("abap_rt_dec_to_int", i32t.fn_type(&[ptrt.into()], false)),
            dec_write: f("abap_rt_dec_write", void.fn_type(&[ptrt.into(), i64t.into()], false)),
            write_chars: f("abap_rt_write_chars", void.fn_type(&[ptrt.into(), i64t.into()], false)),
            write_str: f("abap_rt_write_str", void.fn_type(&[ptrt.into()], false)),
            newline: f("abap_rt_newline", void.fn_type(&[], false)),
            newfield: f("abap_rt_newfield", void.fn_type(&[], false)),
            finish: f("abap_rt_finish", void.fn_type(&[], false)),
            chars_move: f(
                "abap_rt_chars_move",
                void.fn_type(&[ptrt.into(), i64t.into(), ptrt.into(), i64t.into()], false),
            ),
            chars_cmp: f(
                "abap_rt_chars_cmp",
                i32t.fn_type(&[ptrt.into(), i64t.into(), ptrt.into(), i64t.into()], false),
            ),
            str_new: f("abap_rt_str_new", ptrt.fn_type(&[ptrt.into(), i64t.into()], false)),
            str_from_chars: f("abap_rt_str_from_chars", ptrt.fn_type(&[ptrt.into(), i64t.into()], false)),
            str_to_chars: f("abap_rt_str_to_chars", void.fn_type(&[ptrt.into(), i64t.into(), ptrt.into()], false)),
            str_concat: f("abap_rt_str_concat", ptrt.fn_type(&[ptrt.into(), ptrt.into()], false)),
            str_cmp: f("abap_rt_str_cmp", i32t.fn_type(&[ptrt.into(), ptrt.into()], false)),
            int_to_str: f("abap_rt_int_to_str", ptrt.fn_type(&[i32t.into()], false)),
            table_new: f("abap_rt_table_new", ptrt.fn_type(&[i64t.into()], false)),
            table_append: f("abap_rt_table_append", void.fn_type(&[ptrt.into(), ptrt.into(), i64t.into()], false)),
            table_count: f("abap_rt_table_count", i64t.fn_type(&[ptrt.into()], false)),
            table_row: f("abap_rt_table_row", ptrt.fn_type(&[ptrt.into(), i64t.into()], false)),
            table_clear: f("abap_rt_table_clear", void.fn_type(&[ptrt.into()], false)),
            fs_unassigned: f("abap_rt_fs_unassigned", void.fn_type(&[], false)),
            type_mismatch: f("abap_rt_type_mismatch", void.fn_type(&[], false)),
            obj_new: f("abap_rt_obj_new", ptrt.fn_type(&[i64t.into()], false)),
            unbound: f("abap_rt_unbound", void.fn_type(&[], false)),
            exception_raise: f(
                "abap_rt_exception_raise",
                void.fn_type(&[ptrt.into(), i64t.into()], false),
            ),
            exception_pending: f(
                "abap_rt_exception_pending",
                i32t.fn_type(&[], false),
            ),
            exception_class: f(
                "abap_rt_exception_class",
                i64t.fn_type(&[], false),
            ),
            exception_object: f(
                "abap_rt_exception_object",
                ptrt.fn_type(&[], false),
            ),
            exception_clear: f("abap_rt_exception_clear", void.fn_type(&[], false)),
            external_call: f(
                "abap_rt_external_call",
                void.fn_type(&[ptrt.into(), i64t.into()], false),
            ),
            pattern_match: f(
                "abap_rt_pattern_match",
                i32t.fn_type(&[ptrt.into(), ptrt.into()], false),
            ),
            str_contains: f(
                "abap_rt_str_contains",
                i32t.fn_type(&[ptrt.into(), ptrt.into()], false),
            ),
            str_contains_any: f(
                "abap_rt_str_contains_any",
                i32t.fn_type(&[ptrt.into(), ptrt.into()], false),
            ),
            str_contains_only: f(
                "abap_rt_str_contains_only",
                i32t.fn_type(&[ptrt.into(), ptrt.into()], false),
            ),
            str_length: f("abap_rt_str_length", i64t.fn_type(&[ptrt.into()], false)),
            str_slice: f(
                "abap_rt_str_slice",
                ptrt.fn_type(&[ptrt.into(), i32t.into(), i32t.into()], false),
            ),
        };

        Codegen {
            ctx,
            module,
            builder,
            alloca,
            i32t,
            i64t,
            ptrt,
            globals: HashMap::new(),
            vars: HashMap::new(),
            in_form: false,
            pending_init: Vec::new(),
            forms: HashMap::new(),
            layouts: Vec::new(),
            classes: Vec::new(),
            constants: HashMap::new(),
            self_ctx: None,
            loops: Vec::new(),
            stmt_marks: Vec::new(),
            exception_target: None,
            exit_fn: None,
            body: None,
            rt,
            errors: Vec::new(),
            dynamic_types: Vec::new(),
            fs_override: HashMap::new(),
        }
    }

    /// Merkt einen Fehler, statt die Erzeugung abzubrechen.
    fn collect(&mut self, result: CResult<()>) {
        if let Err(e) = result {
            self.errors.push(e);
        }
    }

    /// Erzeugt `main` mit allen Statements und die FORMs als eigene Funktionen.
    /// Liefert alle gesammelten Fehler; leer heißt Erfolg.
    pub fn emit_program(&mut self, ir: &Ir) -> Result<(), Vec<CompileError>> {
        let result = self.emit_program_body(ir);
        self.collect(result);
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    fn emit_program_body(&mut self, ir: &Ir) -> CResult<()> {
        self.constants = ir.constants.clone();
        self.dynamic_types = ir.dynamic_types.clone();
        self.build_layouts(&ir.structs);
        // Klassen: Methoden als Funktionen deklarieren (Objektzeiger als erster Parameter)
        let void = self.ctx.void_type();
        for c in &ir.classes {
            let mut methods = Vec::new();
            for m in &c.methods {
                let mut ps: Vec<BasicMetadataTypeEnum<'ctx>> = if m.is_static {
                    Vec::new()
                } else {
                    vec![self.ptrt.into()]
                };
                ps.extend(m.params.iter().map(|_| BasicMetadataTypeEnum::from(self.ptrt)));
                if m.ret.is_some() {
                    ps.push(self.ptrt.into());
                }
                let f = self.module.add_function(
                    &format!("abap_m_{}_{}", c.name, m.name),
                    void.fn_type(&ps, false),
                    None,
                );
                methods.push(MethodInfo {
                    name: m.name.clone(),
                    is_static: m.is_static,
                    f,
                    pnames: m.params.iter().map(|p| p.name.clone()).collect(),
                    params: m.params.iter().map(|p| (p.ty, p.changing)).collect(),
                    ret: m.ret.as_ref().map(|r| r.ty),
                });
            }
            let parent = match c.parent {
                Some(crate::lower::ClassParent::Internal(index)) => Some(index),
                _ => None,
            };
            let external_parent = match &c.parent {
                Some(crate::lower::ClassParent::External(name)) => Some(name.clone()),
                _ => None,
            };
            self.classes.push(ClassInfo {
                name: c.name.clone(),
                struct_idx: c.struct_idx,
                parent,
                external_parent,
                methods,
                aliases: c.aliases.iter().cloned().collect(),
            });
        }
        // Formen zuerst deklarieren, damit PERFORM auch vorwärts aufrufen kann
        for f in &ir.forms {
            if self.forms.contains_key(&f.name) {
                return err(f.line, format!("FORM '{}' ist bereits definiert", f.name));
            }
            let params: Vec<BasicMetadataTypeEnum<'ctx>> = f.params.iter().map(|_| self.ptrt.into()).collect();
            let fun = self.module.add_function(&format!("abap_form_{}", f.name), void.fn_type(&params, false), None);
            let sig = f.params.iter().map(|p| (p.ty, p.changing)).collect();
            self.forms.insert(f.name.clone(), FormSig { f: fun, params: sig });
        }

        let fn_type = self.i32t.fn_type(&[], false);
        let main = self.module.add_function("main", fn_type, None);
        let entry = self.ctx.append_basic_block(main, "entry");
        let body = self.ctx.append_basic_block(main, "body");
        let exit = self.ctx.append_basic_block(main, "exit");
        self.body = Some(body);
        self.exit_fn = Some(exit);

        self.alloca.position_at_end(entry);
        for class in &ir.classes {
            for (name, ty) in &class.static_fields {
                let qualified = format!("{}=>{}", class.name, name);
                let slot = self.global_slot(&qualified, *ty);
                self.pending_init.push((slot, *ty));
                self.globals.insert(
                    qualified.clone(),
                    Var {
                        slot,
                        ty: *ty,
                        fs: false,
                        tag: None,
                    },
                );
            }
        }
        self.hoist(&ir.stmts)?;
        self.flush_inits()?;

        self.builder.position_at_end(body);
        self.emit_block(&ir.stmts)?;
        self.br_if_open(exit)?;

        self.builder.position_at_end(exit);
        self.call_void(self.rt.finish, &[])?;
        llvm(self.builder.build_return(Some(&self.i32t.const_zero())))?;
        llvm(self.alloca.build_unconditional_branch(body))?;

        for f in &ir.forms {
            let result = self.emit_form(f);
            self.collect(result);
        }
        for (ci, c) in ir.classes.iter().enumerate() {
            for m in &c.methods {
                let result = self.emit_method(ci, m);
                self.collect(result);
            }
        }
        llvm(self.module.verify().map_err(|e| e.to_string()))
    }
}
