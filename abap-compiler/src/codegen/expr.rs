//! Bedingungen und Ausdrücke.

use super::*;

impl<'ctx> Codegen<'ctx> {
    // --- Bedingungen und Ausdrücke -----------------------------------------

    pub(super) fn cond(&mut self, c: &Cond, line: usize) -> CResult<IntValue<'ctx>> {
        match c {
            Cond::Cmp(op, a, b) => {
                let x = self.eval(a, line)?;
                let y = self.eval(b, line)?;
                self.compare(*op, x, y, line)
            }
            Cond::And(a, b) => {
                let x = self.cond(a, line)?;
                let y = self.cond(b, line)?;
                llvm(self.builder.build_and(x, y, "and"))
            }
            Cond::Or(a, b) => {
                let x = self.cond(a, line)?;
                let y = self.cond(b, line)?;
                llvm(self.builder.build_or(x, y, "or"))
            }
            Cond::Not(a) => {
                let x = self.cond(a, line)?;
                llvm(self.builder.build_not(x, "not"))
            }
        }
    }

    /// Vergleich zweier Werte. Zeichen gegen Zeichen mit Leerzeichen-Auffüllung,
    /// Strings exakt (Länge und Inhalt), Zahlen nur gegen Zahlen.
    pub(super) fn compare(&mut self, op: CmpOp, a: Val<'ctx>, b: Val<'ctx>, line: usize) -> CResult<IntValue<'ctx>> {
        let pred = match op {
            CmpOp::Eq => IntPredicate::EQ,
            CmpOp::Ne => IntPredicate::NE,
            CmpOp::Lt => IntPredicate::SLT,
            CmpOp::Gt => IntPredicate::SGT,
            CmpOp::Le => IntPredicate::SLE,
            CmpOp::Ge => IntPredicate::SGE,
        };
        let zero = self.i32t.const_zero();
        match (a, b) {
            (Val::Int(x), Val::Int(y)) => llvm(self.builder.build_int_compare(pred, x, y, "cmp")),
            (Val::Chars(p1, l1), Val::Chars(p2, l2)) => {
                let r = self.call_int(self.rt.chars_cmp, &[p1.into(), self.len(l1).into(), p2.into(), self.len(l2).into()])?;
                llvm(self.builder.build_int_compare(pred, r, zero, "ccmp"))
            }
            _ => {
                // Zeichenfeld wird für den Vergleich mit String ohne nachgestellte Leerzeichen gelesen
                let x = self.as_handle_trimmed(a, line)?;
                let y = self.as_handle_trimmed(b, line)?;
                let r = self.call_int(self.rt.str_cmp, &[x.into(), y.into()])?;
                llvm(self.builder.build_int_compare(pred, r, zero, "scmp"))
            }
        }
    }

    /// Text als String-Handle; Zahlen werden abgelehnt.
    pub(super) fn as_handle(&mut self, v: Val<'ctx>, line: usize, what: &str) -> CResult<PointerValue<'ctx>> {
        match v {
            Val::Str(h) => Ok(h),
            Val::Chars(p, n) => self.call_ptr(self.rt.str_new, &[p.into(), self.len(n).into()]),
            Val::Int(_) => err(line, format!("Zahl kann nicht {what} werden (nur Text)")),
            Val::Mem(..) => err(line, format!("Struktur kann nicht {what} werden (nur Text)")),
            Val::Ref(..) => err(line, format!("Objektreferenz kann nicht {what} werden (nur Text)")),
        }
    }

    /// Wie `as_handle`, aber Zeichenfelder verlieren nachgestellte Leerzeichen.
    pub(super) fn as_handle_trimmed(&mut self, v: Val<'ctx>, line: usize) -> CResult<PointerValue<'ctx>> {
        match v {
            Val::Chars(p, n) => self.call_ptr(self.rt.str_from_chars, &[p.into(), self.len(n).into()]),
            other => self.as_handle(other, line, "verglichen"),
        }
    }

    pub(super) fn eval(&mut self, e: &Expr, line: usize) -> CResult<Val<'ctx>> {
        match e {
            Expr::Int(v) => {
                let v32 = i32::try_from(*v)
                    .or_else(|_| err(line, format!("Zahl außerhalb des Bereichs von i: {v}")))?;
                Ok(Val::Int(self.i32t.const_int(v32 as u64, true)))
            }
            Expr::Lit(s) => Ok(Val::Chars(self.lit(s), s.encode_utf16().count())),
            Expr::Attr { obj, attr } => {
                let (p, c) = self.object(obj, line)?;
                let (fp, fty) = self.field_of(p, c, attr, line)?;
                self.load_val(fp, fty, line)
            }
            Expr::Call { recv, method, args } => {
                match self.emit_method_call_val(recv, method, args, line)? {
                    Some(v) => Ok(v),
                    None => err(line, format!("Methode '{method}' liefert keinen Wert (RETURNING fehlt)")),
                }
            }
            Expr::Var(name) => {
                let (p, ty) = self.data(name, line)?;
                self.load_val(p, ty, line)
            }
            Expr::Slice { name, off, len } => {
                let (p, ty) = self.data(name, line)?;
                let Ty::Char(n) = ty else {
                    return err(line, format!("Teilfeld nur für Zeichenfelder möglich: '{name}'"));
                };
                if off + len > n {
                    return err(line, format!("Teilfeld {name}+{off}({len}) liegt außerhalb des Feldes"));
                }
                Ok(Val::Chars(self.slice_ptr(p, *off)?, *len))
            }
            Expr::Compressed(parts) => {
                // Zeichenfelder ohne nachgestellte Leerzeichen, dann verketten
                let mut acc: Option<PointerValue<'ctx>> = None;
                for p in parts {
                    let v = self.eval(p, line)?;
                    let h = match v {
                        Val::Int(_) => return err(line, "Zahl kann nicht verkettet werden (nur Text)"),
                        other => self.as_handle_trimmed(other, line)?,
                    };
                    acc = Some(match acc {
                        None => h,
                        Some(prev) => self.call_ptr(self.rt.str_concat, &[prev.into(), h.into()])?,
                    });
                }
                match acc {
                    Some(h) => Ok(Val::Str(h)),
                    None => err(line, "leere Verkettung"),
                }
            }
            Expr::Concat(parts) => {
                let mut acc: Option<PointerValue<'ctx>> = None;
                for p in parts {
                    let v = self.eval(p, line)?;
                    let h = match v {
                        Val::Int(_) => return err(line, "Zahl kann nicht verkettet werden (nur Text)"),
                        other => self.as_handle(other, line, "verkettet")?,
                    };
                    acc = Some(match acc {
                        None => h,
                        Some(prev) => self.call_ptr(self.rt.str_concat, &[prev.into(), h.into()])?,
                    });
                }
                match acc {
                    Some(h) => Ok(Val::Str(h)),
                    None => err(line, "leere Verkettung"),
                }
            }
            Expr::Neg(inner) => {
                let v = self.int_operand(inner, line)?;
                Ok(Val::Int(llvm(self.builder.build_int_neg(v, "neg"))?))
            }
            Expr::Bin(op, a, b) => {
                let x = self.int_operand(a, line)?;
                let y = self.int_operand(b, line)?;
                let r = match op {
                    BinOp::Add => llvm(self.builder.build_int_add(x, y, "add"))?,
                    BinOp::Sub => llvm(self.builder.build_int_sub(x, y, "sub"))?,
                    BinOp::Mul => llvm(self.builder.build_int_mul(x, y, "mul"))?,
                    // ABAP: Division bei i schneidet ab (wie C)
                    BinOp::Div => llvm(self.builder.build_int_signed_div(x, y, "div"))?,
                    BinOp::Mod => llvm(self.builder.build_int_signed_rem(x, y, "mod"))?,
                };
                Ok(Val::Int(r))
            }
        }
    }

    pub(super) fn int_operand(&mut self, e: &Expr, line: usize) -> CResult<IntValue<'ctx>> {
        match self.eval(e, line)? {
            Val::Int(i) => Ok(i),
            _ => err(line, "Arithmetik nur mit ganzzahligen Feldern"),
        }
    }
}
