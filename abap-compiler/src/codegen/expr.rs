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
            Cond::Initial(value) => {
                let value = self.eval(value, line)?;
                self.is_initial(value, line)
            }
            Cond::Assigned(value) => self.is_assigned(value, line),
            Cond::Bound(value) => {
                let Val::Ref(pointer, _) = self.eval(value, line)? else {
                    return err(line, "IS BOUND expects an object reference");
                };
                let is_null = llvm(self.builder.build_is_null(pointer, "bound_null"))?;
                llvm(self.builder.build_not(is_null, "is_bound"))
            }
            Cond::Supplied => Ok(self.i32t.const_int(1, false)),
            Cond::In(_, _) => Ok(self.i32t.const_int(0, false)),
            Cond::Truthy(value) => match self.eval(value, line)? {
                Val::Int(value) => llvm(self.builder.build_int_compare(
                    IntPredicate::NE,
                    value,
                    self.i32t.const_zero(),
                    "truthy_int",
                )),
                value => self
                    .is_initial(value, line)
                    .and_then(|initial| llvm(self.builder.build_not(initial, "truthy_value"))),
            },
            Cond::BitSet(value, mask) => {
                let value = self.int_operand(value, line)?;
                let mask = self.int_operand(mask, line)?;
                let bits = llvm(self.builder.build_and(value, mask, "bit_set"))?;
                llvm(self.builder.build_int_compare(
                    IntPredicate::NE,
                    bits,
                    self.i32t.const_zero(),
                    "is_bit_set",
                ))
            }
            Cond::BitClear(value, mask) => {
                let value = self.int_operand(value, line)?;
                let mask = self.int_operand(mask, line)?;
                let bits = llvm(self.builder.build_and(value, mask, "bit_clear"))?;
                llvm(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    bits,
                    self.i32t.const_zero(),
                    "is_bit_clear",
                ))
            }
            Cond::Between(value, low, high) => {
                let value = self.eval(value, line)?;
                let low = self.eval(low, line)?;
                let high = self.eval(high, line)?;
                let lower = self.compare(CmpOp::Ge, value, low, line)?;
                let upper = self.compare(CmpOp::Le, value, high, line)?;
                llvm(self.builder.build_and(lower, upper, "between"))
            }
            Cond::Pattern {
                value,
                pattern,
                negated,
            } => {
                let value = self.eval(value, line)?;
                let value = self.as_handle_trimmed(value, line)?;
                let pattern = self.eval(pattern, line)?;
                let pattern = self.as_handle_trimmed(pattern, line)?;
                let result =
                    self.call_int(self.rt.pattern_match, &[value.into(), pattern.into()])?;
                if *negated {
                    llvm(self.builder.build_int_compare(
                        IntPredicate::EQ,
                        result,
                        self.i32t.const_zero(),
                        "np",
                    ))
                } else {
                    llvm(self.builder.build_int_compare(
                        IntPredicate::NE,
                        result,
                        self.i32t.const_zero(),
                        "cp",
                    ))
                }
            }
            Cond::Contains {
                value,
                needle,
                negated,
            } => {
                let value = self.eval(value, line)?;
                let value = self.as_handle_trimmed(value, line)?;
                let needle = self.eval(needle, line)?;
                let needle = self.as_handle_trimmed(needle, line)?;
                let result = self.call_int(self.rt.str_contains, &[value.into(), needle.into()])?;
                if *negated {
                    llvm(self.builder.build_int_compare(
                        IntPredicate::EQ,
                        result,
                        self.i32t.const_zero(),
                        "ns",
                    ))
                } else {
                    llvm(self.builder.build_int_compare(
                        IntPredicate::NE,
                        result,
                        self.i32t.const_zero(),
                        "cs",
                    ))
                }
            }
            Cond::ContainsAny {
                value,
                chars,
                negated,
            } => {
                let value = self.eval(value, line)?;
                let value = self.as_handle_trimmed(value, line)?;
                let chars = self.eval(chars, line)?;
                let chars = self.as_handle_trimmed(chars, line)?;
                let result =
                    self.call_int(self.rt.str_contains_any, &[value.into(), chars.into()])?;
                llvm(self.builder.build_int_compare(
                    if *negated {
                        IntPredicate::EQ
                    } else {
                        IntPredicate::NE
                    },
                    result,
                    self.i32t.const_zero(),
                    if *negated { "na" } else { "ca" },
                ))
            }
            Cond::ContainsOnly {
                value,
                chars,
                negated,
            } => {
                let value = self.eval(value, line)?;
                let value = self.as_handle_trimmed(value, line)?;
                let chars = self.eval(chars, line)?;
                let chars = self.as_handle_trimmed(chars, line)?;
                let result =
                    self.call_int(self.rt.str_contains_only, &[value.into(), chars.into()])?;
                llvm(self.builder.build_int_compare(
                    if *negated {
                        IntPredicate::EQ
                    } else {
                        IntPredicate::NE
                    },
                    result,
                    self.i32t.const_zero(),
                    if *negated { "cn" } else { "co" },
                ))
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
    pub(super) fn compare(
        &mut self,
        op: CmpOp,
        a: Val<'ctx>,
        b: Val<'ctx>,
        line: usize,
    ) -> CResult<IntValue<'ctx>> {
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
            (Val::Dec(_), _) | (_, Val::Dec(_)) => self.packed_compare(pred, a, b, line),
            (Val::Chars(p1, l1), Val::Chars(p2, l2)) => {
                let r = self.call_int(
                    self.rt.chars_cmp,
                    &[
                        p1.into(),
                        self.len(l1).into(),
                        p2.into(),
                        self.len(l2).into(),
                    ],
                )?;
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
    pub(super) fn as_handle(
        &mut self,
        v: Val<'ctx>,
        line: usize,
        what: &str,
    ) -> CResult<PointerValue<'ctx>> {
        match v {
            Val::Str(h) => Ok(h),
            Val::Chars(p, n) => self.call_ptr(self.rt.str_new, &[p.into(), self.len(n).into()]),
            Val::Int(_) | Val::Dec(_) => err(line, format!("Number cannot be {what} (text only)")),
            Val::Mem(..) => err(line, format!("Structure cannot be {what} (text only)")),
            Val::Ref(..) => err(
                line,
                format!("Object reference cannot be {what} (text only)"),
            ),
            Val::Table(_) => err(line, format!("Table cannot be {what} (text only)")),
        }
    }

    /// Wie `as_handle`, aber Zeichenfelder verlieren nachgestellte Leerzeichen.
    pub(super) fn as_handle_trimmed(
        &mut self,
        v: Val<'ctx>,
        line: usize,
    ) -> CResult<PointerValue<'ctx>> {
        match v {
            Val::Chars(p, n) => {
                self.call_ptr(self.rt.str_from_chars, &[p.into(), self.len(n).into()])
            }
            other => self.as_handle(other, line, "compared"),
        }
    }

    pub(super) fn eval(&mut self, e: &Expr, line: usize) -> CResult<Val<'ctx>> {
        match e {
            Expr::Int(v) => {
                let v32 = i32::try_from(*v)
                    .or_else(|_| err(line, format!("Number out of range for i: {v}")))?;
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
                    None => err(
                        line,
                        format!("Method '{method}' returns no value (RETURNING missing)"),
                    ),
                }
            }
            Expr::StaticCall {
                class,
                method,
                args,
            } => match self.emit_method_call_val(class, method, args, line)? {
                Some(v) => Ok(v),
                None => err(
                    line,
                    format!("Method '{method}' returns no value (RETURNING missing)"),
                ),
            },
            Expr::Lines(table) => {
                let Expr::Var(name) = table.as_ref() else {
                    return err(line, "lines() expects an internal table");
                };
                let (handle, _) = self.table(name, line)?;
                let count = self.call_int(self.rt.table_count, &[handle.into()])?;
                Ok(Val::Int(llvm(
                    self.builder.build_int_truncate(count, self.i32t, "lines"),
                )?))
            }
            Expr::Chain { .. } => err(line, "Chained method calls are not translated yet"),
            Expr::Bool(condition) => {
                let value = self.cond(condition, line)?;
                let true_value =
                    self.call_ptr(self.rt.str_new, &[self.lit("X").into(), self.len(1).into()])?;
                let false_value = self.empty_on(&self.builder)?;
                let selected =
                    llvm(
                        self.builder
                            .build_select(value, true_value, false_value, "boolc"),
                    )?
                    .into_pointer_value();
                Ok(Val::Str(selected))
            }
            Expr::StrLen(value) => {
                let handle = self.eval(value, line)?;
                let handle = self.as_handle_trimmed(handle, line)?;
                let length = self.call_int(self.rt.str_length, &[handle.into()])?;
                Ok(Val::Int(llvm(
                    self.builder.build_int_truncate(length, self.i32t, "strlen"),
                )?))
            }
            Expr::DynamicSlice { name, off, len } => {
                let value = self.data(name, line)?;
                let handle = self.load_val(value.0, value.1, line)?;
                let handle = self.as_handle_trimmed(handle, line)?;
                let offset = self.int_operand(off, line)?;
                let length = self.int_operand(len, line)?;
                let sliced = self.call_ptr(
                    self.rt.str_slice,
                    &[handle.into(), offset.into(), length.into()],
                )?;
                Ok(Val::Str(sliced))
            }
            Expr::Var(name) => {
                // sy-ucomm und sy-dynnr sind Globals (Dynpro); die übrigen SY-Felder haben feste Werte
                if let Some(component) = name
                    .strip_prefix("sy-")
                    .filter(|_| !self.globals.contains_key(name))
                {
                    if matches!(
                        component,
                        "subrc" | "index" | "tabix" | "fdpos" | "dbcnt" | "dynnr"
                    ) {
                        return Ok(Val::Int(self.i32t.const_zero()));
                    }
                    return Ok(Val::Chars(self.lit(""), 0));
                }
                let qualified = self
                    .self_ctx
                    .map(|(_, class)| format!("{}=>{}", self.classes[class].name, name));
                if let Some(constant) = self
                    .constants
                    .get(&name.to_lowercase())
                    .or_else(|| qualified.as_ref().and_then(|key| self.constants.get(key)))
                    .cloned()
                {
                    return self.eval(&constant, line);
                }
                if qualified.as_ref().is_some_and(|prefix| {
                    self.constants
                        .keys()
                        .any(|key| key.starts_with(&format!("{prefix}-")))
                }) {
                    return Ok(Val::Str(self.empty_on(&self.builder)?));
                }
                match self.data(name, line) {
                    Ok((p, ty)) => self.load_val(p, ty, line),
                    Err(_) if name.contains("=>") => Ok(Val::Str(self.empty_on(&self.builder)?)),
                    Err(error) => Err(error),
                }
            }
            Expr::Slice { name, off, len } => {
                let (p, ty) = self.data(name, line)?;
                let Ty::Char(n) = ty else {
                    return err(
                        line,
                        format!("Substring only possible for character fields: '{name}'"),
                    );
                };
                if *off > n || *len > n - *off {
                    return err(
                        line,
                        format!("Substring {name}+{off}({len}) is outside the field"),
                    );
                }
                Ok(Val::Chars(self.slice_ptr(p, *off)?, *len))
            }
            Expr::Compressed(parts) => {
                // Zeichenfelder ohne nachgestellte Leerzeichen, dann verketten
                let mut acc: Option<PointerValue<'ctx>> = None;
                for p in parts {
                    let v = self.eval(p, line)?;
                    let h = match v {
                        Val::Int(_) => {
                            return err(line, "Number cannot be concatenated (text only)")
                        }
                        other => self.as_handle_trimmed(other, line)?,
                    };
                    acc = Some(match acc {
                        None => h,
                        Some(prev) => {
                            self.call_ptr(self.rt.str_concat, &[prev.into(), h.into()])?
                        }
                    });
                }
                match acc {
                    Some(h) => Ok(Val::Str(h)),
                    None => err(line, "empty concatenation"),
                }
            }
            Expr::Template(parts) => {
                let mut values = Vec::new();
                for part in parts {
                    let handle = match self.eval(part, line)? {
                        Val::Int(number) => self.call_ptr(self.rt.int_to_str, &[number.into()])?,
                        other if matches!(part, Expr::Lit(_)) => {
                            self.as_handle(other, line, "used in template")?
                        }
                        other => self.as_handle_trimmed(other, line)?,
                    };
                    values.push(handle);
                }
                let first = values.first().copied().ok_or_else(|| CompileError {
                    line,
                    message: "String template is empty".into(),
                })?;
                let mut result = first;
                for next in values.into_iter().skip(1) {
                    result = self.call_ptr(self.rt.str_concat, &[result.into(), next.into()])?;
                }
                Ok(Val::Str(result))
            }
            Expr::Concat(parts) => {
                let mut acc: Option<PointerValue<'ctx>> = None;
                for p in parts {
                    let v = self.eval(p, line)?;
                    let h = match v {
                        Val::Int(_) => {
                            return err(line, "Number cannot be concatenated (text only)")
                        }
                        other => self.as_handle(other, line, "concatenated")?,
                    };
                    acc = Some(match acc {
                        None => h,
                        Some(prev) => {
                            self.call_ptr(self.rt.str_concat, &[prev.into(), h.into()])?
                        }
                    });
                }
                match acc {
                    Some(h) => Ok(Val::Str(h)),
                    None => err(line, "empty concatenation"),
                }
            }
            Expr::Neg(inner) => match self.eval(inner, line)? {
                Val::Int(v) => Ok(Val::Int(self.checked_neg(v)?)),
                v @ Val::Dec(_) => self.packed_neg(v, line),
                _ => err(line, "Arithmetic only with integer fields or DEC"),
            },
            Expr::Bin(op, a, b) => {
                let va = self.eval(a, line)?;
                let vb = self.eval(b, line)?;
                if matches!(va, Val::Dec(_)) || matches!(vb, Val::Dec(_)) {
                    return self.packed_arith(*op, va, vb, line);
                }
                let (Val::Int(x), Val::Int(y)) = (va, vb) else {
                    return err(line, "Arithmetic only with integer fields or DEC");
                };
                Ok(Val::Int(self.checked_int(*op, x, y)?))
            }
        }
    }

    pub(super) fn int_operand(&mut self, e: &Expr, line: usize) -> CResult<IntValue<'ctx>> {
        match self.eval(e, line)? {
            Val::Int(i) => Ok(i),
            _ => err(line, "Arithmetic only with integer fields"),
        }
    }

    fn is_initial(&mut self, value: Val<'ctx>, line: usize) -> CResult<IntValue<'ctx>> {
        match value {
            Val::Int(value) => llvm(self.builder.build_int_compare(
                IntPredicate::EQ,
                value,
                self.i32t.const_zero(),
                "is_initial_int",
            )),
            v @ Val::Dec(_) => self.packed_is_zero(v, line),
            Val::Chars(pointer, length) => {
                let empty = self.lit("");
                let compared = self.call_int(
                    self.rt.chars_cmp,
                    &[
                        pointer.into(),
                        self.len(length).into(),
                        empty.into(),
                        self.len(0).into(),
                    ],
                )?;
                llvm(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    compared,
                    self.i32t.const_zero(),
                    "is_initial_chars",
                ))
            }
            Val::Str(handle) => {
                let empty = self.empty_on(&self.builder)?;
                self.compare(CmpOp::Eq, Val::Str(handle), Val::Str(empty), line)
            }
            Val::Table(handle) => {
                let count = self.call_int(self.rt.table_count, &[handle.into()])?;
                llvm(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    count,
                    self.i64t.const_zero(),
                    "is_initial_table",
                ))
            }
            Val::Ref(pointer, _) => llvm(self.builder.build_is_null(pointer, "is_initial_ref")),
            Val::Mem(pointer, structure) => {
                let layout = self.layouts[structure].clone();
                let mut result = self.i32t.const_int(1, false);
                for (index, (_, ty)) in layout.fields.iter().enumerate() {
                    let field = llvm(self.builder.build_struct_gep(
                        layout.st,
                        pointer,
                        index as u32,
                        "initial_field",
                    ))?;
                    let value = self.load_val(field, *ty, line)?;
                    let initial = self.is_initial(value, line)?;
                    result = llvm(self.builder.build_and(result, initial, "initial_and"))?;
                }
                Ok(result)
            }
        }
    }

    fn is_assigned(&mut self, value: &Expr, line: usize) -> CResult<IntValue<'ctx>> {
        let Expr::Var(name) = value else {
            return err(line, "IS ASSIGNED expects a field symbol");
        };
        let variable = self.var(name, line)?;
        if !variable.fs {
            return err(line, format!("'{name}' is not a field symbol"));
        }
        let pointer = llvm(
            self.builder
                .build_load(self.ptrt, variable.slot, "assigned_pointer"),
        )?
        .into_pointer_value();
        let is_null = llvm(self.builder.build_is_null(pointer, "assigned_null"))?;
        llvm(self.builder.build_not(is_null, "is_assigned"))
    }
}
