//! Speicherlayout, LLVM-Typen und Initialwerte.

use super::*;

impl<'ctx> Codegen<'ctx> {
    /// Baut die LLVM-Typen aller Strukturen (gepackt, damit die Größe der Summe der Komponenten entspricht).
    pub(super) fn build_layouts(&mut self, structs: &[crate::lower::StructDef]) {
        for def in structs {
            let field_tys: Vec<inkwell::types::BasicTypeEnum<'ctx>> =
                def.fields.iter().map(|(_, t)| self.llvm_ty(*t)).collect();
            let st = self.ctx.struct_type(&field_tys, true);
            self.layouts.push(Layout {
                st,
                fields: def.fields.clone(),
            });
        }
    }

    /// LLVM-Typ eines ABAP-Typs. Zeiger für Strings und Tabellen.
    pub(super) fn llvm_ty(&self, ty: Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        match ty {
            Ty::Dynamic => self.ptrt.into(),
            Ty::Int => self.i32t.into(),
            Ty::Char(n) => self.char_array(n).into(),
            Ty::Packed { bytes, .. } => self.ctx.i8_type().array_type(bytes as u32).into(),
            Ty::Str | Ty::Table(_) | Ty::Ref(_) | Ty::External(_) => self.ptrt.into(),
            Ty::Struct(i) => self.layouts[i].st.into(),
        }
    }

    /// Initialwert einer gepackten Zahl: Ziffer 0, Vorzeichen positiv (`C`) im letzten Halbbyte.
    pub(super) fn packed_zero(&self, bytes: usize) -> inkwell::values::ArrayValue<'ctx> {
        let i8t = self.ctx.i8_type();
        let mut values: Vec<_> = (0..bytes).map(|_| i8t.const_zero()).collect();
        if let Some(last) = values.last_mut() {
            *last = i8t.const_int(0x0C, false);
        }
        i8t.const_array(&values)
    }

    /// Speicherbedarf eines Werts in Bytes (entspricht dem gepackten Layout).
    pub(super) fn size_of(&self, ty: Ty) -> u64 {
        match ty {
            Ty::Dynamic => 8,
            Ty::Int => 4,
            Ty::Char(n) => 2 * n as u64,
            Ty::Packed { bytes, .. } => bytes as u64,
            Ty::Str | Ty::Table(_) | Ty::Ref(_) | Ty::External(_) => 8,
            Ty::Struct(i) => self.layouts[i]
                .fields
                .iter()
                .map(|(_, t)| self.size_of(*t))
                .sum(),
        }
    }

    pub(super) fn elem_size(&self, e: Elem) -> u64 {
        self.size_of(e.ty())
    }

    /// Leerer String, aufgerufen mit dem angegebenen Builder.
    pub(super) fn empty_on(&self, b: &Builder<'ctx>) -> CResult<PointerValue<'ctx>> {
        match self.call_on(
            b,
            self.rt.str_new,
            &[self.ptrt.const_null().into(), self.len(0).into()],
        )? {
            Some(v) => Ok(v.into_pointer_value()),
            None => err(0, "str_new without return value"),
        }
    }

    /// Setzt einen Speicherplatz auf den Initialwert seines Typs: Zahl 0, Zeichen Leerzeichen,
    /// String leer, Tabelle neu angelegt, Struktur mit Leerzeichen bzw. leeren Strings in den Komponenten.
    pub(super) fn init_value(
        &self,
        b: &Builder<'ctx>,
        slot: PointerValue<'ctx>,
        ty: Ty,
    ) -> CResult<()> {
        match ty {
            Ty::Dynamic => {
                llvm(b.build_store(slot, self.ptrt.const_null()))?;
            }
            Ty::Int => {
                llvm(b.build_store(slot, self.i32t.const_zero()))?;
            }
            Ty::Char(n) => {
                llvm(b.build_store(slot, self.const_chars("", n)))?;
            }
            Ty::Packed { bytes, .. } => {
                llvm(b.build_store(slot, self.packed_zero(bytes)))?;
            }
            Ty::Str => {
                let e = self.empty_on(b)?;
                llvm(b.build_store(slot, e))?;
            }
            Ty::Table(e) => {
                let size = self.len(self.elem_size(e) as usize);
                let t = match self.call_on(b, self.rt.table_new, &[size.into()])? {
                    Some(v) => v.into_pointer_value(),
                    None => return err(0, "table_new without return value"),
                };
                llvm(b.build_store(slot, t))?;
            }
            Ty::Ref(_) | Ty::External(_) => {
                llvm(b.build_store(slot, self.ptrt.const_null()))?;
            }
            Ty::Struct(i) => {
                let layout = self.layouts[i].clone();
                llvm(b.build_store(slot, layout.st.const_zero()))?;
                for (k, (_, fty)) in layout.fields.iter().enumerate() {
                    if matches!(
                        fty,
                        Ty::Str | Ty::Char(_) | Ty::Packed { .. } | Ty::Table(_) | Ty::Struct(_)
                    ) {
                        let fp = llvm(b.build_struct_gep(layout.st, slot, k as u32, "fi"))?;
                        self.init_value(b, fp, *fty)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn temp_type(&self, ty: Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        self.llvm_ty(ty)
    }

    /// Leerer String zur Laufzeit (nicht im Einstieg).
    pub(super) fn empty_string_now(&self) -> CResult<PointerValue<'ctx>> {
        self.call_ptr(
            self.rt.str_new,
            &[self.ptrt.const_null().into(), self.len(0).into()],
        )
    }

    /// Temporärer Speicherplatz für einen Zeilenwert, im Einstieg angelegt.
    pub(super) fn temp(&self, e: Elem) -> CResult<(PointerValue<'ctx>, Dest<'ctx>)> {
        match e {
            Elem::Int => {
                let p = llvm(self.alloca.build_alloca(self.i32t, "tmp"))?;
                Ok((p, Dest::Int(p)))
            }
            Elem::Char(n) => {
                let p = llvm(self.alloca.build_alloca(self.char_array(n), "tmp"))?;
                Ok((p, Dest::Chars(p, n)))
            }
            Elem::Str => {
                let p = llvm(self.alloca.build_alloca(self.ptrt, "tmp"))?;
                Ok((p, Dest::Str(p)))
            }
            Elem::Struct(i) => {
                let p = llvm(self.alloca.build_alloca(self.llvm_ty(Ty::Struct(i)), "tmp"))?;
                Ok((p, Dest::Mem(p, i)))
            }
            Elem::External(_) => {
                let p = llvm(self.alloca.build_alloca(self.ptrt, "tmp"))?;
                Ok((p, Dest::Str(p)))
            }
        }
    }
}
