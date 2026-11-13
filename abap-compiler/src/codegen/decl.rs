//! Deklarationen: Variablen, Feldsymbole, Initialisierung von `main`.

use super::*;

impl<'ctx> Codegen<'ctx> {
    /// Initialisiert Globals mit Laufzeitwerten im Einstieg von `main`.
    pub(super) fn flush_inits(&mut self) -> CResult<()> {
        for (slot, ty) in std::mem::take(&mut self.pending_init) {
            self.init_value(&self.alloca, slot, ty)?;
        }
        Ok(())
    }

    // --- Deklarationen -----------------------------------------------------

    /// Legt alle Deklarationen (rekursiv) im Einstieg an.
    pub(super) fn hoist(&mut self, stmts: &[Stmt]) -> CResult<()> {
        for s in stmts {
            match s {
                Stmt::Data { name, ty, line } => self.declare(name, *ty, *line)?,
                Stmt::FieldSymbol { name, ty, line } => self.declare_fs(name, *ty, *line)?,
                Stmt::If { branches, otherwise, .. } => {
                    for (_, b) in branches {
                        self.hoist(b)?;
                    }
                    self.hoist(otherwise)?;
                }
                Stmt::Case { whens, other, .. } => {
                    for (_, b) in whens {
                        self.hoist(b)?;
                    }
                    self.hoist(other)?;
                }
                Stmt::Do { body, .. } | Stmt::While { body, .. } | Stmt::Loop { body, .. } => self.hoist(body)?,
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn check_new(&self, name: &str, line: usize) -> CResult<()> {
        if self.vars.contains_key(name) || self.globals.contains_key(name) {
            return err(line, format!("Feld '{name}' ist bereits deklariert"));
        }
        Ok(())
    }

    /// Deklaration: in FORMs lokal (Allokation im Einstieg), sonst als Programmvariable (Global).
    pub(super) fn declare(&mut self, name: &str, ty: Ty, line: usize) -> CResult<()> {
        self.check_new(name, line)?;
        if self.in_form {
            let slot = self.alloca_slot(name, ty)?;
            self.vars.insert(name.to_string(), Var { slot, ty, fs: false });
        } else {
            let slot = self.global_slot(name, ty);
            self.pending_init.push((slot, ty));
            self.globals.insert(name.to_string(), Var { slot, ty, fs: false });
        }
        Ok(())
    }

    /// Lokaler Speicherplatz mit Initialwert.
    pub(super) fn alloca_slot(&self, name: &str, ty: Ty) -> CResult<PointerValue<'ctx>> {
        let p = llvm(self.alloca.build_alloca(self.llvm_ty(ty), name))?;
        self.init_value(&self.alloca, p, ty)?;
        Ok(p)
    }

    /// Globaler Speicherplatz; Strings, Tabellen und Strukturkomponenten werden in `flush_inits` gesetzt.
    pub(super) fn global_slot(&self, name: &str, ty: Ty) -> PointerValue<'ctx> {
        let lt = self.llvm_ty(ty);
        let init: BasicValueEnum<'ctx> = match ty {
            Ty::Int => self.i32t.const_zero().into(),
            Ty::Char(n) => self.const_chars("", n).into(),
            Ty::Str | Ty::Table(_) | Ty::Ref(_) => self.ptrt.const_null().into(),
            Ty::Struct(i) => self.layouts[i].st.const_zero().into(),
        };
        let g = self.module.add_global(lt, None, &format!("abap_g_{name}"));
        g.set_initializer(&init);
        g.as_pointer_value()
    }

    pub(super) fn declare_fs(&mut self, name: &str, ty: Ty, line: usize) -> CResult<()> {
        self.check_new(name, line)?;
        if self.in_form {
            let slot = llvm(self.alloca.build_alloca(self.ptrt, name))?;
            llvm(self.alloca.build_store(slot, self.ptrt.const_null()))?;
            self.vars.insert(name.to_string(), Var { slot, ty, fs: true });
        } else {
            let g = self.module.add_global(self.ptrt, None, &format!("abap_g_{name}"));
            g.set_initializer(&self.ptrt.const_null());
            self.globals
                .insert(name.to_string(), Var { slot: g.as_pointer_value(), ty, fs: true });
        }
        Ok(())
    }
}
