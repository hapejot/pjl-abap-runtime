//! Einzelne Statements (DATA, MOVE, CALL, PERFORM ...).

use super::*;

impl Lowerer {
    pub(super) fn lower_stmt(&mut self, s: &Statement, out: &mut Vec<Stmt>) -> Result<(), CompileError> {
        let line = s.line;
        let toks = &s.tokens;
        let kw = s.keyword();

        match kw.as_str() {
            "REPORT" | "TYPES" => Ok(()),
            "DATA" => {
                // DATA name TYPE typ
                let name = match toks.get(1) {
                    Some(t) if t.kind == Kind::Word => word(t),
                    _ => return err(line, "DATA-Deklaration unvollständig"),
                };
                if !toks.get(2).is_some_and(|t| t.is_word("TYPE")) {
                    return err(line, "DATA ohne TYPE wird nicht unterstützt");
                }
                let ty = self.ty_of(&toks[3..], line)?;
                out.push(Stmt::Data { name, ty, line });
                Ok(())
            }
            "FIELD-SYMBOLS" => {
                // FIELD-SYMBOLS <fs> TYPE typ
                let name = match toks.get(1) {
                    Some(t) if t.kind == Kind::Word && t.text.starts_with('<') && t.text.ends_with('>') => word(t),
                    _ => return err(line, "FIELD-SYMBOLS erwartet <name>"),
                };
                if !toks.get(2).is_some_and(|t| t.is_word("TYPE")) {
                    return err(line, "FIELD-SYMBOLS ohne TYPE wird nicht unterstützt");
                }
                let ty = self.elem_of(&toks[3..], line)?.ty();
                out.push(Stmt::FieldSymbol { name, ty, line });
                Ok(())
            }
            "CLEAR" => {
                let target = parse_target(&toks[1..], line)?;
                out.push(Stmt::Clear { target, line });
                Ok(())
            }
            "MOVE" => {
                let Some(to) = toks.iter().position(|t| t.is_word("TO")) else {
                    return err(line, "MOVE ohne TO");
                };
                let target = parse_target(&toks[to + 1..], line)?;
                let value = parse_expr(&toks[1..to], line)?;
                out.push(Stmt::Assign { target, value, line });
                Ok(())
            }
            "CONCATENATE" => {
                // CONCATENATE a b c INTO r [COMPRESSED]. (Operanden: Felder, Komponenten oder Literale)
                let Some(into) = toks.iter().position(|t| t.is_word("INTO")) else {
                    return err(line, "CONCATENATE ohne INTO");
                };
                let operands = &toks[1..into];
                if operands.is_empty() {
                    return err(line, "CONCATENATE ohne Operanden");
                }
                let mut target_toks = &toks[into + 1..];
                let compressed = target_toks.last().is_some_and(|t| t.is_word("COMPRESSED"));
                if compressed {
                    target_toks = &target_toks[..target_toks.len() - 1];
                }
                let parts = operands.iter().map(|t| single_operand(t, line)).collect::<Result<Vec<_>, _>>()?;
                let target = parse_target(target_toks, line)?;
                let value = match (parts.len(), compressed) {
                    (1, false) => parts.into_iter().next().unwrap(),
                    (_, false) => Expr::Concat(parts),
                    (_, true) => Expr::Compressed(parts),
                };
                out.push(Stmt::Assign { target, value, line });
                Ok(())
            }
            "WRITE" => {
                // WRITE [/] [wert]. Ein `/` beginnt eine neue Zeile; ohne `/` wird angehängt.
                let (newline, rest) = match toks.get(1) {
                    Some(t) if t.is_punct("/") => (true, &toks[2..]),
                    Some(t) if t.kind == Kind::Word && t.text.starts_with('/') => {
                        return err(line, "Nach '/' bitte ein Leerzeichen setzen (WRITE / wert)");
                    }
                    _ => (false, &toks[1..]),
                };
                let value = if rest.is_empty() { None } else { Some(parse_expr(rest, line)?) };
                out.push(Stmt::Write { newline, value, line });
                Ok(())
            }
            "APPEND" => {
                // APPEND wert TO tabelle
                let Some(to) = toks.iter().rposition(|t| t.is_word("TO")) else {
                    return err(line, "APPEND ohne TO");
                };
                if to < 2 || toks.len() != to + 2 || toks[to + 1].kind != Kind::Word {
                    return err(line, "APPEND erwartet 'APPEND wert TO tabelle.'");
                }
                let value = parse_expr(&toks[1..to], line)?;
                out.push(Stmt::Append { table: word(&toks[to + 1]), value, line });
                Ok(())
            }
            "DESCRIBE" => {
                // DESCRIBE TABLE tabelle LINES ziel
                if toks.len() != 5 || !toks[1].is_word("TABLE") || !toks[3].is_word("LINES") {
                    return err(line, "DESCRIBE unterstützt nur 'DESCRIBE TABLE tab LINES ziel.'");
                }
                out.push(Stmt::Describe { table: word(&toks[2]), target: word(&toks[4]), line });
                Ok(())
            }
            "PERFORM" => {
                // PERFORM name [USING a b] [CHANGING c]
                let Some(name_tok) = toks.get(1).filter(|t| t.kind == Kind::Word) else {
                    return err(line, "PERFORM ohne Routinennamen");
                };
                let args = parse_args(&toks[2..], line)?;
                out.push(Stmt::Perform { name: word(name_tok), args, line });
                Ok(())
            }
            "CALL" => {
                // CALL METHOD obj->meth [EXPORTING ...] [RECEIVING r = x]   (auch mit Klammern)
                if toks.len() < 5 || !toks[1].is_word("METHOD") || toks[2].kind != Kind::Word || !toks[3].is_punct("->") || toks[4].kind != Kind::Word {
                    return err(line, "CALL unterstützt nur 'CALL METHOD obj->meth ...'");
                }
                let mut rest = &toks[5..];
                if rest.first().is_some_and(|t| t.is_punct("(")) {
                    if !rest.last().is_some_and(|t| t.is_punct(")")) || rest.len() < 2 {
                        return err(line, "fehlende schließende Klammer");
                    }
                    rest = &rest[1..rest.len() - 1];
                }
                let (args, rec) = parse_call_args(rest, line)?;
                let receiving = match rec {
                    Some(e) => Some(expr_to_target(e, line)?),
                    None => None,
                };
                out.push(Stmt::MethodCall { recv: word(&toks[2]), method: word(&toks[4]), args, receiving, line });
                Ok(())
            }
            "CREATE" => {
                if toks.len() != 3 || !toks[1].is_word("OBJECT") || toks[2].kind != Kind::Word {
                    return err(line, "CREATE unterstützt nur 'CREATE OBJECT obj.'");
                }
                out.push(Stmt::Create { name: word(&toks[2]), line });
                Ok(())
            }
            "EXIT" => {
                if toks.len() != 1 {
                    return err(line, "EXIT mit Zusatz wird nicht unterstützt");
                }
                out.push(Stmt::Exit { line });
                Ok(())
            }
            "CHECK" => {
                let cond = parse_cond(&toks[1..], line)?;
                out.push(Stmt::Check { cond, line });
                Ok(())
            }
            "RETURN" => {
                if toks.len() != 1 {
                    return err(line, "RETURN mit Zusatz wird nicht unterstützt");
                }
                out.push(Stmt::Return { line });
                Ok(())
            }
            _ => {
                // Methodenaufruf: obj->methode( [p = a ...] )
                if toks.len() >= 5 && toks[1].is_punct("->") && toks[3].is_punct("(") && toks.last().is_some_and(|t| t.is_punct(")")) {
                    let (args, rec) = parse_call_args(&toks[4..toks.len() - 1], line)?;
                    if rec.is_some() {
                        return err(line, "RECEIVING ist nur bei CALL METHOD erlaubt");
                    }
                    out.push(Stmt::MethodCall { recv: word(&toks[0]), method: word(&toks[2]), args, receiving: None, line });
                    return Ok(());
                }
                // Zuweisung: ziel = ausdruck.
                if let Some(eq) = toks.iter().position(|t| t.is_punct("=")) {
                    if eq >= 1 && toks[0].kind == Kind::Word {
                        let target = parse_target(&toks[..eq], line)?;
                        let value = parse_expr(&toks[eq + 1..], line)?;
                        out.push(Stmt::Assign { target, value, line });
                        return Ok(());
                    }
                }
                err(line, format!("Nicht unterstütztes Statement: {kw}"))
            }
        }
    }
}
