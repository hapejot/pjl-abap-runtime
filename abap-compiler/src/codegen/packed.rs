//! Gepackte Zahlen (DEC) im Codegen. Ein Wert ist ein Handle auf eine `Dec` in der Laufzeit
//! (`abap-rt/src/decimal.rs`). Die Arithmetik läuft dort auf Ziffern, beliebiger Länge.
//! Der Compiler kennt nur die Skala, die Länge des Feldes und die Aufrufe.

use super::*;

impl<'ctx> Codegen<'ctx> {
    /// Handle einer Zahl. Ganze Zahlen werden zu einer `Dec` mit Skala 0.
    pub(super) fn dec_handle(&self, v: Val<'ctx>, line: usize) -> CResult<PointerValue<'ctx>> {
        match v {
            Val::Int(i) => self.call_ptr(self.rt.dec_from_int, &[i.into()]),
            Val::Dec(h) => Ok(h),
            _ => err(line, "Arithmetic only with numbers (integer or DEC)"),
        }
    }

    /// Liest ein gepacktes Feld mit `bytes` Bytes und Skala `scale`.
    pub(super) fn load_packed(
        &self,
        p: PointerValue<'ctx>,
        bytes: usize,
        scale: usize,
    ) -> CResult<Val<'ctx>> {
        let len = self.len(bytes);
        let scale_value = self.len(scale);
        let h = self.call_ptr(
            self.rt.dec_unpack,
            &[p.into(), len.into(), scale_value.into()],
        )?;
        Ok(Val::Dec(h))
    }

    /// Summe, Differenz oder Produkt, wenn mindestens ein Operand gepackt ist.
    pub(super) fn packed_arith(
        &self,
        op: BinOp,
        a: Val<'ctx>,
        b: Val<'ctx>,
        line: usize,
    ) -> CResult<Val<'ctx>> {
        let f = match op {
            BinOp::Add => self.rt.dec_add,
            BinOp::Sub => self.rt.dec_sub,
            BinOp::Mul => self.rt.dec_mul,
            // DEC: DIV nutzt bisher dieselbe Routine wie `/`; ein eigener Abschnitt fehlt noch
            BinOp::Div | BinOp::IntDiv => self.rt.dec_div,
            BinOp::Mod => self.rt.dec_mod,
            _ => return err(line, "Bit operations with DEC fields are not supported"),
        };
        let x = self.dec_handle(a, line)?;
        let y = self.dec_handle(b, line)?;
        Ok(Val::Dec(self.call_ptr(f, &[x.into(), y.into()])?))
    }

    /// Negation einer gepackten Zahl.
    pub(super) fn packed_neg(&self, v: Val<'ctx>, line: usize) -> CResult<Val<'ctx>> {
        let h = self.dec_handle(v, line)?;
        Ok(Val::Dec(self.call_ptr(self.rt.dec_neg, &[h.into()])?))
    }

    /// Vergleich, wenn mindestens ein Operand gepackt ist.
    pub(super) fn packed_compare(
        &self,
        pred: IntPredicate,
        a: Val<'ctx>,
        b: Val<'ctx>,
        line: usize,
    ) -> CResult<IntValue<'ctx>> {
        let x = self.dec_handle(a, line)?;
        let y = self.dec_handle(b, line)?;
        let c = self.call_int(self.rt.dec_cmp, &[x.into(), y.into()])?;
        llvm(
            self.builder
                .build_int_compare(pred, c, self.i32t.const_zero(), "dcmp"),
        )
    }

    /// Prüfung auf Initialwert (Null).
    pub(super) fn packed_is_zero(&self, v: Val<'ctx>, line: usize) -> CResult<IntValue<'ctx>> {
        self.packed_compare(IntPredicate::EQ, v, Val::Int(self.i32t.const_zero()), line)
    }

    /// Schreibt eine Zahl in ein gepacktes Feld. Nachkommastellen werden abgeschnitten, Überlauf bricht ab.
    pub(super) fn store_packed(
        &self,
        p: PointerValue<'ctx>,
        bytes: usize,
        scale: usize,
        v: Val<'ctx>,
        line: usize,
    ) -> CResult<()> {
        let h = match v {
            Val::Int(_) | Val::Dec(_) => self.dec_handle(v, line)?,
            _ => return err(line, "Text or structure cannot be assigned to a DEC field"),
        };
        let len = self.len(bytes);
        let scale_value = self.len(scale);
        self.call_void(
            self.rt.dec_pack,
            &[h.into(), p.into(), len.into(), scale_value.into()],
        )
    }

    /// Schreibt eine gepackte Zahl mit `WRITE`.
    pub(super) fn write_packed(&self, h: PointerValue<'ctx>) -> CResult<()> {
        self.call_void(self.rt.dec_write, &[h.into(), self.len(0).into()])
    }

    /// DEC -> ganze Zahl (Nachkommastellen fallen weg).
    pub(super) fn dec_to_i32(&self, h: PointerValue<'ctx>) -> CResult<IntValue<'ctx>> {
        self.call_int(self.rt.dec_to_int, &[h.into()])
    }
}
