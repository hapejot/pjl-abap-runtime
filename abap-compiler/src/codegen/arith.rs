//! Ganzzahl-Arithmetik mit ABAP-Semantik für `TYPE i`: Ein Ergebnis außerhalb von `i32` oder eine
//! Division durch 0 bricht das Programm mit einer Laufzeitmeldung ab. Es wird nicht still umgebrochen.
//! Die Rechnung läuft in `i64`; zwei `i32`-Werte passen dort immer, daher ist die Bereichsprüfung exakt.

use super::*;

/// Fehlerarten, die `abap_rt_arith_error` entgegennimmt.
pub(super) const ERR_OVERFLOW: u64 = 0;
pub(super) const ERR_DIV_ZERO: u64 = 1;

impl<'ctx> Codegen<'ctx> {
    /// Bricht bei `cond` mit dem Fehler `kind` ab, sonst geht es im Folgeblock weiter.
    fn trap_if(&mut self, cond: IntValue<'ctx>, kind: u64) -> CResult<()> {
        let trap = self.new_block("arith_trap")?;
        let cont = self.new_block("arith_ok")?;
        llvm(self.builder.build_conditional_branch(cond, trap, cont))?;
        self.builder.position_at_end(trap);
        let code = self.i32t.const_int(kind, false);
        self.call_void(self.rt.arith_error, &[code.into()])?;
        llvm(self.builder.build_unreachable())?;
        self.builder.position_at_end(cont);
        Ok(())
    }

    /// Ganzzahlige Operation mit Überlauf- und Divisionsprüfung. Das Ergebnis ist `i32`.
    pub(super) fn checked_int(&mut self, op: BinOp, x: IntValue<'ctx>, y: IntValue<'ctx>) -> CResult<IntValue<'ctx>> {
        if op == BinOp::BitAnd {
            return llvm(self.builder.build_and(x, y, "bit_and"));
        }
        let x64 = llvm(self.builder.build_int_s_extend(x, self.i64t, "x64"))?;
        let y64 = llvm(self.builder.build_int_s_extend(y, self.i64t, "y64"))?;
        if matches!(op, BinOp::Div | BinOp::Mod) {
            let is_zero = llvm(self.builder.build_int_compare(
                IntPredicate::EQ,
                y64,
                self.i64t.const_zero(),
                "divzero",
            ))?;
            self.trap_if(is_zero, ERR_DIV_ZERO)?;
        }
        let r64 = match op {
            BinOp::Add => llvm(self.builder.build_int_add(x64, y64, "add"))?,
            BinOp::Sub => llvm(self.builder.build_int_sub(x64, y64, "sub"))?,
            BinOp::Mul => llvm(self.builder.build_int_mul(x64, y64, "mul"))?,
            // ABAP: Division schneidet zur Null hin ab, wie signed div
            BinOp::Div => llvm(self.builder.build_int_signed_div(x64, y64, "div"))?,
            BinOp::Mod => llvm(self.builder.build_int_signed_rem(x64, y64, "mod"))?,
            BinOp::BitAnd => unreachable!("BitAnd wird oben behandelt"),
        };
        let r32 = llvm(self.builder.build_int_truncate(r64, self.i32t, "r32"))?;
        let back = llvm(self.builder.build_int_s_extend(r32, self.i64t, "back"))?;
        let fits = llvm(self.builder.build_int_compare(IntPredicate::EQ, back, r64, "fits"))?;
        let overflow = llvm(self.builder.build_not(fits, "overflow"))?;
        self.trap_if(overflow, ERR_OVERFLOW)?;
        Ok(r32)
    }

    /// Negation mit Überlaufprüfung (`-(-2^31)` passt nicht in `i32`).
    pub(super) fn checked_neg(&mut self, x: IntValue<'ctx>) -> CResult<IntValue<'ctx>> {
        self.checked_int(BinOp::Sub, self.i32t.const_zero(), x)
    }
}
