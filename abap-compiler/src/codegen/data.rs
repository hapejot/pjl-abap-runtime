//! Zugriff auf Variablen, Komponenten, Attribute, Tabellen und Schreibziele.

use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn var(&self, name: &str, line: usize) -> CResult<&Var<'ctx>> {
        if let Some(v) = self.vars.get(name) {
            return Ok(v);
        }
        match self.globals.get(name) {
            Some(v) => Ok(v),
            None => err(line, format!("Feld '{name}' ist nicht deklariert")),
        }
    }

    /// Datenzeiger und Typ eines Namens. `s-feld` ist die Komponente `feld` der Struktur `s`.
    pub(super) fn data(&self, name: &str, line: usize) -> CResult<(PointerValue<'ctx>, Ty)> {
        let Some((base, field)) = name.split_once('-') else {
            return self.data_base(name, line);
        };
        let (p, ty) = self.data_base(base, line)?;
        let Ty::Struct(i) = ty else {
            return err(line, format!("'{base}' ist keine Struktur, Komponente '{field}' nicht zugreifbar"));
        };
        let layout = self.layouts[i].clone();
        let Some(k) = layout.fields.iter().position(|(n, _)| n == field) else {
            return err(line, format!("Komponente '{field}' unbekannt in '{base}'"));
        };
        let fty = layout.fields[k].1;
        let fp = llvm(self.builder.build_struct_gep(layout.st, p, k as u32, "comp"))?;
        Ok((fp, fty))
    }

    /// Datenzeiger und Typ eines einfachen Namens; Feldsymbole werden durch ihren Zeiger aufgelöst.
    /// Einfacher Name: lokal, dann Attribut des Objekts (in Methoden), dann Programmvariable.
    pub(super) fn data_base(&self, name: &str, line: usize) -> CResult<(PointerValue<'ctx>, Ty)> {
        if let Some(v) = self.vars.get(name) {
            return self.resolve_var(v, name, line);
        }
        if let Some((slot, c)) = self.self_ctx {
            let layout = self.layouts[self.classes[c].struct_idx].clone();
            if let Some(k) = layout.fields.iter().position(|(n, _)| n == name) {
                let obj = llvm(self.builder.build_load(self.ptrt, slot, "me"))?.into_pointer_value();
                let fp = llvm(self.builder.build_struct_gep(layout.st, obj, k as u32, "attr"))?;
                return Ok((fp, layout.fields[k].1));
            }
        }
        match self.globals.get(name) {
            Some(v) => self.resolve_var(v, name, line),
            None => err(line, format!("Feld '{name}' ist nicht deklariert")),
        }
    }

    /// Speicherplatz einer Variablen; bei Feldsymbolen Zeiger laden und auf null prüfen.
    pub(super) fn resolve_var(&self, v: &Var<'ctx>, name: &str, line: usize) -> CResult<(PointerValue<'ctx>, Ty)> {
        if !v.fs {
            return Ok((v.slot, v.ty));
        }
        let p = llvm(self.builder.build_load(self.ptrt, v.slot, "fsptr"))?.into_pointer_value();
        let _ = (name, line);
        self.check_bound_fs(p)?;
        Ok((p, v.ty))
    }

    /// Nicht zugewiesenes Feldsymbol: Laufzeitfehler statt Zugriff auf null.
    pub(super) fn check_bound_fs(&self, p: PointerValue<'ctx>) -> CResult<()> {
        let is_null = llvm(self.builder.build_is_null(p, "fs_null"))?;
        let bad = self.new_block("fs_unassigned")?;
        let ok = self.new_block("fs_ok")?;
        llvm(self.builder.build_conditional_branch(is_null, bad, ok))?;
        self.builder.position_at_end(bad);
        self.call_void(self.rt.fs_unassigned, &[])?;
        llvm(self.builder.build_unreachable())?;
        self.builder.position_at_end(ok);
        Ok(())
    }

    /// Objekt hinter einer Referenzvariablen (oder `me`): Zeiger auf das Objekt und Klassenindex.
    pub(super) fn object(&self, name: &str, line: usize) -> CResult<(PointerValue<'ctx>, usize)> {
        if name == "me" {
            let Some((slot, c)) = self.self_ctx else {
                return err(line, "'me' nur innerhalb einer Methode");
            };
            let p = llvm(self.builder.build_load(self.ptrt, slot, "me"))?.into_pointer_value();
            return Ok((p, c));
        }
        let (slot, ty) = self.data_base(name, line)?;
        let Ty::Ref(c) = ty else {
            return err(line, format!("'{name}' ist keine Objektreferenz"));
        };
        let obj = llvm(self.builder.build_load(self.ptrt, slot, "obj"))?.into_pointer_value();
        let is_null = llvm(self.builder.build_is_null(obj, "obj_null"))?;
        let bad = self.new_block("unbound")?;
        let ok = self.new_block("bound")?;
        llvm(self.builder.build_conditional_branch(is_null, bad, ok))?;
        self.builder.position_at_end(bad);
        self.call_void(self.rt.unbound, &[])?;
        llvm(self.builder.build_unreachable())?;
        self.builder.position_at_end(ok);
        Ok((obj, c))
    }

    /// Attribut eines Objekts: Zeiger auf das Feld und dessen Typ.
    pub(super) fn field_of(&self, obj: PointerValue<'ctx>, c: usize, attr: &str, line: usize) -> CResult<(PointerValue<'ctx>, Ty)> {
        let layout = self.layouts[self.classes[c].struct_idx].clone();
        let Some(k) = layout.fields.iter().position(|(n, _)| n == attr) else {
            return err(line, format!("Attribut '{attr}' unbekannt"));
        };
        let fp = llvm(self.builder.build_struct_gep(layout.st, obj, k as u32, "attr"))?;
        Ok((fp, layout.fields[k].1))
    }

    /// Handle einer internen Tabelle und ihr Zeilentyp.
    pub(super) fn table(&self, name: &str, line: usize) -> CResult<(PointerValue<'ctx>, Elem)> {
        let v = self.var(name, line)?;
        match (v.ty, v.fs) {
            (Ty::Table(e), false) => {
                let h = llvm(self.builder.build_load(self.ptrt, v.slot, "tab"))?.into_pointer_value();
                Ok((h, e))
            }
            _ => err(line, format!("'{name}' ist keine interne Tabelle")),
        }
    }

    /// Schreibziel auflösen; Typprüfung für Teilfelder.
    pub(super) fn dest(&self, target: &Target, line: usize) -> CResult<Dest<'ctx>> {
        match target {
            Target::Attr { obj, attr } => {
                let (p, c) = self.object(obj, line)?;
                let (fp, fty) = self.field_of(p, c, attr, line)?;
                self.dest_of(fty, fp, line)
            }
            Target::Var(name) => {
                let (p, ty) = self.data(name, line)?;
                self.dest_of(ty, p, line)
            }
            Target::Slice { name, off, len } => {
                let (p, ty) = self.data(name, line)?;
                let Ty::Char(n) = ty else {
                    return err(line, format!("Teilfeld nur für Zeichenfelder möglich: '{name}'"));
                };
                if off + len > n {
                    return err(line, format!("Teilfeld {name}+{off}({len}) liegt außerhalb des Feldes"));
                }
                Ok(Dest::Chars(self.slice_ptr(p, *off)?, *len))
            }
        }
    }

    pub(super) fn dest_of(&self, ty: Ty, p: PointerValue<'ctx>, line: usize) -> CResult<Dest<'ctx>> {
        match ty {
            Ty::Int => Ok(Dest::Int(p)),
            Ty::Char(n) => Ok(Dest::Chars(p, n)),
            Ty::Str => Ok(Dest::Str(p)),
            Ty::Table(_) => err(line, "Tabelle kann nicht direkt zugewiesen werden"),
            Ty::Struct(i) => Ok(Dest::Mem(p, i)),
            Ty::Ref(c) => Ok(Dest::Ref(p, c)),
        }
    }

    pub(super) fn slice_ptr(&self, base: PointerValue<'ctx>, off: usize) -> CResult<PointerValue<'ctx>> {
        let idx = self.len(off);
        // Zeiger auf das Zeichen an Position `off` im Zeichenfeld
        unsafe { llvm(self.builder.build_gep(self.ctx.i16_type(), base, &[idx], "slice")) }
    }

    /// Liest einen Wert aus einem Speicherplatz des angegebenen Typs.
    pub(super) fn load_val(&self, p: PointerValue<'ctx>, ty: Ty, line: usize) -> CResult<Val<'ctx>> {
        match ty {
            Ty::Int => Ok(Val::Int(llvm(self.builder.build_load(self.i32t, p, "ld"))?.into_int_value())),
            Ty::Char(n) => Ok(Val::Chars(p, n)),
            Ty::Str => Ok(Val::Str(llvm(self.builder.build_load(self.ptrt, p, "lds"))?.into_pointer_value())),
            Ty::Table(_) => err(line, "Tabelle kann nicht als Wert verwendet werden"),
            Ty::Struct(i) => Ok(Val::Mem(p, i)),
            Ty::Ref(c) => Ok(Val::Ref(llvm(self.builder.build_load(self.ptrt, p, "ref"))?.into_pointer_value(), c)),
        }
    }

    /// Liest eine Tabellenzeile (Zeiger `row`) als Wert.
    pub(super) fn read_row(&self, row: PointerValue<'ctx>, e: Elem) -> CResult<Val<'ctx>> {
        self.load_val(row, e.ty(), 0)
    }
}
