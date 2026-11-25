//! Einzelne Statements (DATA, MOVE, CALL, PERFORM ...).

use super::*;

impl Lowerer {
    pub(super) fn lower_stmt(
        &mut self,
        s: &Statement,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        let line = s.line;
        let pragma_start = s
            .tokens
            .iter()
            .position(|token| token.is_punct("#"))
            .unwrap_or(s.tokens.len());
        let toks = &s.tokens[..pragma_start];
        let kw = s.keyword();

        if let Some(expr) = expression_statement(toks, line) {
            out.push(Stmt::ExprStmt { expr, line });
            return Ok(());
        }

        if !toks[0].text.starts_with('<')
            && toks.get(1).is_some_and(|token| token.is_punct("("))
            && toks.last().is_some_and(|token| token.is_punct(")"))
        {
            if toks.iter().any(|token| token.is_punct("->")) {
                out.push(Stmt::ExternalOperation {
                    name: "chained_method_call".into(),
                    line,
                });
                return Ok(());
            }
            if toks.len() >= 7
                && toks[1].is_punct("=>")
                && toks[3].is_punct("~")
                && toks[5].is_punct("(")
                && toks.last().is_some_and(|token| token.is_punct(")"))
            {
                let (args, rec) = parse_call_args(&toks[6..toks.len() - 1], line)?;
                out.push(Stmt::MethodCall {
                    recv: word(&toks[0]),
                    method: format!("{}~{}", word(&toks[2]), word(&toks[4])),
                    args,
                    receiving: rec.map(|value| expr_to_target(value, line)).transpose()?,
                    line,
                });
                return Ok(());
            }
            let (args, receiving) = parse_call_args(&toks[2..toks.len() - 1], line)?;
            out.push(Stmt::MethodCall {
                recv: "me".into(),
                method: word(&toks[0]),
                args,
                receiving: receiving
                    .map(|value| expr_to_target(value, line))
                    .transpose()?,
                line,
            });
            return Ok(());
        }

        match kw.as_str() {
            "REPORT" | "TYPES" => Ok(()),
            "SELECTION-SCREEN" | "TABLES" | "PARAMETERS" | "SELECT-OPTIONS" => Ok(()),
            // Auflösbare Includes ersetzt project::expand_includes; was hier ankommt, fehlt im Export.
            // INCLUDE name IF FOUND: fehlt die Datei, ist das kein Fehler
            "INCLUDE" if toks.iter().any(|t| t.is_word("FOUND")) => Ok(()),
            "INCLUDE" => err(
                line,
                format!(
                    "INCLUDE {} konnte nicht aufgelöst werden (Datei nicht im Export)",
                    toks.get(1).map(word).unwrap_or_default()
                ),
            ),
            "SELECT" | "UPDATE" | "MODIFY" => {
                out.push(Stmt::ExternalOperation {
                    name: kw.to_lowercase(),
                    line,
                });
                Ok(())
            }
            "CONSTANTS" => {
                let Some(value_index) = toks.iter().rposition(|token| token.is_word("VALUE"))
                else {
                    return Ok(());
                };
                let name = toks
                    .get(1)
                    .filter(|token| token.kind == Kind::Word)
                    .ok_or_else(|| CompileError {
                        line,
                        message: "CONSTANTS ohne Namen".into(),
                    })?;
                self.constants.insert(
                    word(name),
                    parse_constant_expr(
                        &toks[value_index + 1..{
                            let values = &toks[value_index + 1..];
                            value_index
                                + 1
                                + values
                                    .iter()
                                    .position(|token| token.is_punct("#"))
                                    .unwrap_or(values.len())
                        }],
                        line,
                    )?,
                );
                Ok(())
            }

            "CLASS" | "INTERFACE" if toks.last().is_some_and(|t| t.is_word("DEFERRED")) => Ok(()),
            "DATA" => {
                // DATA name TYPE typ
                let name = match toks.get(1) {
                    Some(t) if t.kind == Kind::Word => word(t),
                    _ => return err(line, "DATA-Deklaration unvollständig"),
                };
                if !toks
                    .get(2)
                    .is_some_and(|t| t.is_word("TYPE") || t.is_word("LIKE"))
                {
                    return err(line, "DATA ohne TYPE/LIKE wird nicht unterstützt");
                }
                let ty = if toks[2].is_word("LIKE") {
                    self.ty_of_like(&toks[3..], line)?
                } else {
                    self.ty_of(&toks[3..], line)?
                };
                self.data_types.insert(name.clone(), ty);
                out.push(Stmt::Data { name, ty, line });
                Ok(())
            }
            "FIELD-SYMBOLS" => {
                // FIELD-SYMBOLS <fs> TYPE typ
                let name = match toks.get(1) {
                    Some(t)
                        if t.kind == Kind::Word
                            && t.text.starts_with('<')
                            && t.text.ends_with('>') =>
                    {
                        word(t)
                    }
                    _ => return err(line, "FIELD-SYMBOLS erwartet <name>"),
                };
                // Ohne TYPE, mit TYPE any/data: Typ erst zur Laufzeit (dynamisch)
                let ty = if toks.len() == 2 {
                    Ty::Dynamic
                } else if toks.get(2).is_some_and(|t| t.is_word("TYPE")) {
                    if toks.len() == 4 && (toks[3].is_word("any") || toks[3].is_word("data")) {
                        Ty::Dynamic
                    } else {
                        self.ty_of(&toks[3..], line)?
                    }
                } else if toks.get(2).is_some_and(|t| t.is_word("LIKE")) {
                    self.field_symbol_like_type(&toks[3..], line)?
                } else {
                    return err(line, "FIELD-SYMBOLS erwartet TYPE oder LIKE");
                };
                // Feldsymbol ist für LIKE wie eine Variable verwendbar
                self.data_types.insert(name.clone(), ty);
                out.push(Stmt::FieldSymbol { name, ty, line });
                Ok(())
            }
            "CLEAR" => {
                if toks.iter().any(|token| token.is_punct("+")) {
                    out.push(Stmt::ExternalOperation {
                        name: "dynamic_slice_clear".into(),
                        line,
                    });
                    return Ok(());
                }
                let target = parse_target(&toks[1..], line)?;
                out.push(Stmt::Clear { target, line });
                Ok(())
            }
            "MOVE" => {
                if toks.get(1).is_some_and(|token| token.is_word("EXACT")) {
                    out.push(Stmt::ExternalOperation {
                        name: "move_exact".into(),
                        line,
                    });
                    return Ok(());
                }
                let Some(to) = toks.iter().position(|t| t.is_word("TO")) else {
                    return err(line, "MOVE ohne TO");
                };
                let target = parse_target(&toks[to + 1..], line)?;
                let value = parse_expr(&toks[1..to], line)?;
                out.push(Stmt::Assign {
                    target,
                    value,
                    line,
                });
                Ok(())
            }
            "MOVE-CORRESPONDING" => {
                out.push(Stmt::ExternalOperation {
                    name: "move_corresponding".into(),
                    line,
                });
                Ok(())
            }
            "SET" => {
                out.push(Stmt::ExternalOperation {
                    name: "set_handler".into(),
                    line,
                });
                Ok(())
            }
            "ASSIGN" => {
                let Some(component_index) =
                    toks.iter().position(|token| token.is_word("COMPONENT"))
                else {
                    let Some(to_index) = toks.iter().position(|token| token.is_word("TO")) else {
                        return err(line, "ASSIGN erwartet TO");
                    };
                    if to_index == 1 && toks.get(to_index + 1).is_some() {
                        out.push(Stmt::ExternalOperation {
                            name: "assign".into(),
                            line,
                        });
                        return Ok(());
                    }
                    // ASSIGN quelle TO <fs>
                    if to_index == 2
                        && toks.len() == 4
                        && toks[1].kind == Kind::Word
                        && toks[3].kind == Kind::Word
                        && toks[3].text.starts_with('<')
                    {
                        out.push(Stmt::AssignFs {
                            fs: word(&toks[3]),
                            source: word(&toks[1]),
                            line,
                        });
                        return Ok(());
                    }
                    out.push(Stmt::ExternalOperation {
                        name: "assign_dynamic".into(),
                        line,
                    });
                    return Ok(());
                };
                let Some(structure_index) =
                    toks.iter().position(|token| token.is_word("STRUCTURE"))
                else {
                    return err(line, "ASSIGN COMPONENT erwartet OF STRUCTURE");
                };
                let Some(to_index) = toks.iter().position(|token| token.is_word("TO")) else {
                    return err(line, "ASSIGN erwartet TO");
                };
                let component_token =
                    toks.get(component_index + 1).ok_or_else(|| CompileError {
                        line,
                        message: "ASSIGN COMPONENT ohne Komponente".into(),
                    })?;
                let component = match literal_content(&component_token.text) {
                    Some(component) => component.to_lowercase(),
                    None => {
                        out.push(Stmt::ExternalOperation {
                            name: "assign_component_dynamic".into(),
                            line,
                        });
                        return Ok(());
                    }
                };
                if structure_index <= component_index + 1
                    || !toks[component_index + 2..structure_index]
                        .iter()
                        .any(|token| token.is_word("OF"))
                {
                    return err(
                        line,
                        "ASSIGN COMPONENT erwartet 'COMPONENT name OF STRUCTURE'",
                    );
                }
                let structure = toks
                    .get(structure_index + 1)
                    .filter(|token| token.kind == Kind::Word)
                    .ok_or_else(|| CompileError {
                        line,
                        message: "ASSIGN COMPONENT erwartet Strukturnamen".into(),
                    })?;
                let target = toks
                    .get(to_index + 1)
                    .filter(|token| token.kind == Kind::Word)
                    .ok_or_else(|| CompileError {
                        line,
                        message: "ASSIGN erwartet Feldsymbol".into(),
                    })?;
                out.push(Stmt::AssignComponent {
                    component,
                    structure: word(structure),
                    target: word(target),
                    line,
                });
                Ok(())
            }
            "READ" if toks.get(1).is_some_and(|token| token.is_word("TABLE")) => {
                let table = toks
                    .get(2)
                    .filter(|token| token.kind == Kind::Word)
                    .ok_or_else(|| CompileError {
                        line,
                        message: "READ TABLE erwartet Tabellennamen".into(),
                    })?;
                let assigning_index = toks.iter().position(|token| token.is_word("ASSIGNING"));
                let Some(assigning) = assigning_index
                    .and_then(|index| toks.get(index + 1))
                    .filter(|token| token.kind == Kind::Word)
                else {
                    out.push(Stmt::ExternalOperation {
                        name: "read_table".into(),
                        line,
                    });
                    return Ok(());
                };
                let Some(index) = toks.iter().position(|token| token.is_word("INDEX")) else {
                    out.push(Stmt::ExternalOperation {
                        name: "read_table_by_key".into(),
                        line,
                    });
                    return Ok(());
                };
                let end = assigning_index
                    .filter(|assigning_index| *assigning_index > index)
                    .unwrap_or_else(|| (index + 2).min(toks.len()));
                out.push(Stmt::ReadTable {
                    table: word(table),
                    target: word(assigning),
                    index: parse_expr(&toks[index + 1..end], line)?,
                    line,
                });
                Ok(())
            }
            "READ" => {
                out.push(Stmt::ExternalOperation {
                    name: "read_system".into(),
                    line,
                });
                Ok(())
            }
            "CONCATENATE" => {
                if toks.iter().any(|token| {
                    token.is_punct("+")
                        || token.is_punct("=>")
                        || token.is_word("BYTE")
                        || token.is_word("MODE")
                        || token.is_word("RESPECTING")
                }) {
                    out.push(Stmt::ExternalOperation {
                        name: "concatenate_bytes".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks.iter().any(|token| token.is_word("SEPARATED")) {
                    out.push(Stmt::ExternalOperation {
                        name: "concatenate_separated".into(),
                        line,
                    });
                    return Ok(());
                }
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
                let parts = operands
                    .iter()
                    .map(|t| single_operand(t, line))
                    .collect::<Result<Vec<_>, _>>()?;
                let target = parse_target(target_toks, line)?;
                let value = match (parts.len(), compressed) {
                    (1, false) => parts.into_iter().next().unwrap(),
                    (_, false) => Expr::Concat(parts),
                    (_, true) => Expr::Compressed(parts),
                };
                out.push(Stmt::Assign {
                    target,
                    value,
                    line,
                });
                Ok(())
            }
            "WRITE" => {
                if toks.iter().any(|token| token.is_word("TO")) {
                    out.push(Stmt::ExternalOperation {
                        name: "write_to".into(),
                        line,
                    });
                    return Ok(());
                }
                // WRITE [/] [wert]. Ein `/` beginnt eine neue Zeile; ohne `/` wird angehängt.
                let (newline, rest) = match toks.get(1) {
                    Some(t) if t.is_punct("/") => (true, &toks[2..]),
                    Some(t) if t.kind == Kind::Word && t.text.starts_with('/') => {
                        return err(line, "Nach '/' bitte ein Leerzeichen setzen (WRITE / wert)");
                    }
                    _ => (false, &toks[1..]),
                };
                let value = if rest.is_empty() {
                    None
                } else {
                    Some(parse_expr(rest, line)?)
                };
                out.push(Stmt::Write {
                    newline,
                    value,
                    line,
                });
                Ok(())
            }
            "APPEND" => {
                if toks.get(1).is_some_and(|token| token.is_word("LINES")) {
                    out.push(Stmt::ExternalOperation {
                        name: "append_lines".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks.iter().any(|token| token.is_word("ASSIGNING")) {
                    out.push(Stmt::ExternalOperation {
                        name: "append_assigning".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks.iter().any(|token| token.is_punct("->")) {
                    out.push(Stmt::ExternalOperation {
                        name: "append_object_attribute".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks.get(1).is_some_and(|token| token.is_word("INITIAL"))
                    && toks.get(2).is_some_and(|token| token.is_word("LINE"))
                {
                    out.push(Stmt::ExternalOperation {
                        name: "append_initial_line".into(),
                        line,
                    });
                    return Ok(());
                }
                // APPEND wert TO tabelle
                let Some(to) = toks.iter().rposition(|t| t.is_word("TO")) else {
                    return err(line, "APPEND ohne TO");
                };
                if to < 2 || toks.len() != to + 2 || toks[to + 1].kind != Kind::Word {
                    return err(line, "APPEND erwartet 'APPEND wert TO tabelle.'");
                }
                let value = parse_expr(&toks[1..to], line)?;
                out.push(Stmt::Append {
                    table: word(&toks[to + 1]),
                    value,
                    line,
                });
                Ok(())
            }
            "DESCRIBE" => {
                // DESCRIBE TABLE tabelle LINES ziel
                if toks.len() != 5 || !toks[1].is_word("TABLE") || !toks[3].is_word("LINES") {
                    return err(
                        line,
                        "DESCRIBE unterstützt nur 'DESCRIBE TABLE tab LINES ziel.'",
                    );
                }
                out.push(Stmt::Describe {
                    table: word(&toks[2]),
                    target: word(&toks[4]),
                    line,
                });
                Ok(())
            }
            "PERFORM" => {
                if toks.iter().any(|token| token.is_word("PROGRAM")) {
                    out.push(Stmt::ExternalOperation {
                        name: "perform_dynamic".into(),
                        line,
                    });
                    return Ok(());
                }
                // PERFORM name [USING a b] [CHANGING c]
                let Some(name_tok) = toks.get(1).filter(|t| t.kind == Kind::Word) else {
                    return err(line, "PERFORM ohne Routinennamen");
                };
                let args = parse_args(&toks[2..], line)?;
                out.push(Stmt::Perform {
                    name: word(name_tok),
                    args,
                    line,
                });
                Ok(())
            }
            "CALL" => {
                if toks.get(1).is_some_and(|token| token.is_word("FUNCTION")) {
                    let Some(name) = toks.get(2).and_then(|token| literal_content(&token.text))
                    else {
                        out.push(Stmt::ExternalOperation {
                            name: "call_function_dynamic".into(),
                            line,
                        });
                        return Ok(());
                    };
                    out.push(Stmt::ExternalFunctionCall {
                        name: name.to_lowercase(),
                        line,
                    });
                    return Ok(());
                }
                if toks.get(1).is_some_and(|token| token.is_word("METHOD"))
                    && (toks.get(2).is_some_and(|token| token.is_punct("("))
                        || toks.get(4).is_some_and(|token| token.is_punct("(")))
                {
                    out.push(Stmt::ExternalOperation {
                        name: "call_method_dynamic".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks
                    .get(1)
                    .is_some_and(|token| token.is_word("TRANSFORMATION"))
                {
                    out.push(Stmt::ExternalOperation {
                        name: "call_transformation".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks
                    .get(1)
                    .is_some_and(|token| token.is_word("SELECTION-SCREEN"))
                {
                    out.push(Stmt::ExternalOperation {
                        name: "call_selection_screen".into(),
                        line,
                    });
                    return Ok(());
                }
                // CALL METHOD obj->meth [EXPORTING ...] [RECEIVING r = x]   (auch mit Klammern)
                if toks.len() < 5
                    || !toks[1].is_word("METHOD")
                    || toks[2].kind != Kind::Word
                    || !toks[3].is_punct("->")
                    || toks[4].kind != Kind::Word
                {
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
                out.push(Stmt::MethodCall {
                    recv: word(&toks[2]),
                    method: word(&toks[4]),
                    args,
                    receiving,
                    line,
                });
                Ok(())
            }
            "CREATE" => {
                if toks.get(1).is_some_and(|token| token.is_word("DATA")) {
                    out.push(Stmt::ExternalOperation {
                        name: "create_data".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks.len() < 3 || !toks[1].is_word("OBJECT") || toks[2].kind != Kind::Word {
                    return err(line, "CREATE OBJECT erwartet einen Objektnamen");
                }
                let end = toks[3..]
                    .iter()
                    .position(|token| token.is_punct("#"))
                    .map(|offset| 3 + offset)
                    .unwrap_or(toks.len());
                let (args, receiving) = parse_call_args(&toks[3..end], line)?;
                if receiving.is_some() {
                    return err(line, "CREATE OBJECT unterstützt kein RECEIVING");
                }
                out.push(Stmt::Create {
                    name: word(&toks[2]),
                    args,
                    line,
                });
                Ok(())
            }
            "EXIT" => {
                if toks.len() != 1 {
                    return err(line, "EXIT mit Zusatz wird nicht unterstützt");
                }
                out.push(Stmt::Exit { line });
                Ok(())
            }
            "CONTINUE" => {
                out.push(Stmt::Continue { line });
                Ok(())
            }
            "CHECK" => {
                let cond = parse_cond(&toks[1..], line)?;
                out.push(Stmt::Check { cond, line });
                Ok(())
            }
            "ASSERT" => {
                let cond = parse_cond(&toks[1..], line)?;
                out.push(Stmt::Assert { cond, line });
                Ok(())
            }
            "RETURN" => {
                if toks.len() != 1 {
                    return err(line, "RETURN mit Zusatz wird nicht unterstützt");
                }
                out.push(Stmt::Return { line });
                Ok(())
            }
            "RAISE" if toks.get(1).is_some_and(|token| token.is_word("EXCEPTION")) => {
                let exception = toks
                    .get(2)
                    .filter(|token| token.kind == Kind::Word)
                    .ok_or_else(|| CompileError {
                        line,
                        message: "RAISE EXCEPTION erwartet eine Referenz".into(),
                    })?;
                out.push(Stmt::Raise {
                    exception: word(exception),
                    line,
                });
                Ok(())
            }
            "RAISE" => {
                out.push(Stmt::ExternalOperation {
                    name: "raise".into(),
                    line,
                });
                Ok(())
            }
            "REPLACE" => {
                out.push(Stmt::ExternalOperation {
                    name: "replace_all_in_table".into(),
                    line,
                });
                Ok(())
            }
            "DELETE" => {
                out.push(Stmt::ExternalOperation {
                    name: "delete_table_range".into(),
                    line,
                });
                Ok(())
            }
            "INSERT" | "COLLECT" => {
                out.push(Stmt::ExternalOperation {
                    name: kw.to_lowercase(),
                    line,
                });
                Ok(())
            }
            "MESSAGE" => {
                out.push(Stmt::ExternalOperation {
                    name: "message".into(),
                    line,
                });
                Ok(())
            }
            "CONVERT" => {
                out.push(Stmt::ExternalOperation {
                    name: "convert_timestamp".into(),
                    line,
                });
                Ok(())
            }
            "COMMIT" | "ROLLBACK" => {
                out.push(Stmt::ExternalOperation {
                    name: kw.to_lowercase(),
                    line,
                });
                Ok(())
            }
            "SUBMIT" => {
                out.push(Stmt::ExternalOperation {
                    name: "submit".into(),
                    line,
                });
                Ok(())
            }
            "LEAVE" => {
                out.push(Stmt::ExternalOperation {
                    name: "leave_screen".into(),
                    line,
                });
                Ok(())
            }
            "EXPORT" | "IMPORT" => {
                out.push(Stmt::ExternalOperation {
                    name: kw.to_lowercase(),
                    line,
                });
                Ok(())
            }
            "RECEIVE" => {
                out.push(Stmt::ExternalOperation {
                    name: "receive_results".into(),
                    line,
                });
                Ok(())
            }
            "WAIT" => {
                out.push(Stmt::ExternalOperation {
                    name: "wait".into(),
                    line,
                });
                Ok(())
            }
            "AUTHORITY-CHECK" => {
                out.push(Stmt::ExternalOperation {
                    name: "authority_check".into(),
                    line,
                });
                Ok(())
            }
            "FREE" => {
                let target = toks.get(1).map(|_| "free").unwrap_or("free");
                out.push(Stmt::ExternalOperation {
                    name: target.into(),
                    line,
                });
                Ok(())
            }
            "UNASSIGN" => {
                out.push(Stmt::ExternalOperation {
                    name: "unassign".into(),
                    line,
                });
                Ok(())
            }
            "GET" if toks.get(1).is_some_and(|token| token.is_word("REFERENCE")) => {
                out.push(Stmt::ExternalOperation {
                    name: "get_reference".into(),
                    line,
                });
                Ok(())
            }
            "GET" if toks.get(1).is_some_and(|token| token.is_word("TIME")) => {
                out.push(Stmt::ExternalOperation {
                    name: "get_timestamp".into(),
                    line,
                });
                Ok(())
            }
            "GET" => {
                out.push(Stmt::ExternalOperation {
                    name: "get_bit".into(),
                    line,
                });
                Ok(())
            }
            "FIND" | "SEARCH" | "SHIFT" | "SPLIT" | "CONDENSE" | "TRANSLATE" | "OVERLAY" => {
                out.push(Stmt::ExternalOperation {
                    name: kw.to_lowercase(),
                    line,
                });
                Ok(())
            }
            _ => {
                if toks.get(1).is_some_and(|token| token.is_punct("("))
                    && toks.iter().any(|token| token.is_punct("->"))
                {
                    out.push(Stmt::ExternalOperation {
                        name: "chained_method_call".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks.len() >= 5
                    && toks[1].is_punct("~")
                    && toks[3].is_punct("(")
                    && toks.last().is_some_and(|token| token.is_punct(")"))
                {
                    let (args, rec) = parse_call_args(&toks[4..toks.len() - 1], line)?;
                    if rec.is_some() {
                        return err(line, "RECEIVING ist nur bei CALL METHOD erlaubt");
                    }
                    out.push(Stmt::MethodCall {
                        recv: "me".into(),
                        method: format!("{}~{}", word(&toks[0]), word(&toks[2])),
                        args,
                        receiving: None,
                        line,
                    });
                    return Ok(());
                }
                if !toks
                    .iter()
                    .any(|token| token.is_punct("=") || token.is_punct("?="))
                    && toks
                        .iter()
                        .filter(|token| token.is_punct("->") || token.is_punct("=>"))
                        .count()
                        > 1
                {
                    out.push(Stmt::ExternalOperation {
                        name: "chained_method_call".into(),
                        line,
                    });
                    return Ok(());
                }
                if toks.len() >= 3
                    && toks[0].kind == Kind::Word
                    && !toks[0].text.starts_with('<')
                    && toks[1].is_punct("(")
                    && toks.last().is_some_and(|token| token.is_punct(")"))
                {
                    let (args, receiving) = parse_call_args(&toks[2..toks.len() - 1], line)?;
                    out.push(Stmt::MethodCall {
                        recv: "me".into(),
                        method: word(&toks[0]),
                        args,
                        receiving: receiving
                            .map(|value| expr_to_target(value, line))
                            .transpose()?,
                        line,
                    });
                    return Ok(());
                }
                if toks.len() >= 5
                    && toks[1].is_punct("=>")
                    && toks[3].is_punct("~")
                    && toks[5].is_punct("(")
                    && toks.last().is_some_and(|token| token.is_punct(")"))
                {
                    let (args, rec) = parse_call_args(&toks[6..toks.len() - 1], line)?;
                    out.push(Stmt::MethodCall {
                        recv: word(&toks[0]),
                        method: format!("{}~{}", word(&toks[2]), word(&toks[4])),
                        args,
                        receiving: rec.map(|value| expr_to_target(value, line)).transpose()?,
                        line,
                    });
                    return Ok(());
                }
                if toks.len() >= 5
                    && toks[1].is_punct("=>")
                    && toks[3].is_punct("(")
                    && toks.last().is_some_and(|token| token.is_punct(")"))
                {
                    let (args, rec) = parse_call_args(&toks[4..toks.len() - 1], line)?;
                    out.push(Stmt::MethodCall {
                        recv: word(&toks[0]),
                        method: word(&toks[2]),
                        args,
                        receiving: rec.map(|value| expr_to_target(value, line)).transpose()?,
                        line,
                    });
                    return Ok(());
                }
                // Methodenaufruf: obj->methode( [p = a ...] )
                if toks.len() >= 7
                    && toks[1].is_punct("->")
                    && toks[3].is_punct("~")
                    && toks[5].is_punct("(")
                    && toks.last().is_some_and(|t| t.is_punct(")"))
                {
                    let (args, rec) = parse_call_args(&toks[6..toks.len() - 1], line)?;
                    if rec.is_some() {
                        return err(line, "RECEIVING ist nur bei CALL METHOD erlaubt");
                    }
                    out.push(Stmt::MethodCall {
                        recv: word(&toks[0]),
                        method: format!("{}~{}", word(&toks[2]), word(&toks[4])),
                        args,
                        receiving: None,
                        line,
                    });
                    return Ok(());
                }
                if toks.len() >= 5
                    && toks[1].is_punct("->")
                    && toks[3].is_punct("(")
                    && toks.last().is_some_and(|t| t.is_punct(")"))
                {
                    let (args, rec) = parse_call_args(&toks[4..toks.len() - 1], line)?;
                    out.push(Stmt::MethodCall {
                        recv: word(&toks[0]),
                        method: word(&toks[2]),
                        args,
                        receiving: rec.map(|value| expr_to_target(value, line)).transpose()?,
                        line,
                    });
                    return Ok(());
                }
                // Zuweisung: ziel = ausdruck.
                if let Some(eq) = toks
                    .iter()
                    .position(|t| t.is_punct("=") || t.is_punct("?="))
                {
                    if eq >= 1 && toks[0].kind == Kind::Word {
                        let dynamic_slice = toks[..eq]
                            .iter()
                            .position(|token| token.is_punct("+"))
                            .is_some_and(|plus| {
                                !toks[..eq].get(plus + 1).is_some_and(|token| {
                                    token.text.chars().all(|c| c.is_ascii_digit())
                                }) || !toks[..eq].get(plus + 3).is_some_and(|token| {
                                    token.text.chars().all(|c| c.is_ascii_digit())
                                })
                            });
                        if dynamic_slice {
                            out.push(Stmt::ExternalOperation {
                                name: "dynamic_slice_assign".into(),
                                line,
                            });
                            return Ok(());
                        }
                        let target = parse_target(&toks[..eq], line)?;
                        let value = parse_expr(&toks[eq + 1..], line)?;
                        out.push(Stmt::Assign {
                            target,
                            value,
                            line,
                        });
                        return Ok(());
                    }
                }

                out.push(Stmt::NotSupported {
                    tokens: toks.into(),
                });
                Ok(())
            }
        }
    }
}

pub(super) fn parse_constant_expr(tokens: &[Token], line: usize) -> Result<Expr, CompileError> {
    if tokens
        .iter()
        .all(|token| token.kind == Kind::Literal || token.is_punct("&"))
    {
        let mut value = String::new();
        for token in tokens {
            if token.kind == Kind::Literal {
                value.push_str(&literal_content(&token.text).ok_or_else(|| CompileError {
                    line,
                    message: "ungültiges Konstanten-Literal".into(),
                })?);
            }
        }
        return Ok(Expr::Lit(value));
    }
    parse_expr(tokens, line)
}

/// Erkennt verkettete Aufrufe als Anweisung wie `a->m( )->n( x = 1 ).` oder `zcl=>m( )->n( ).`.
/// Ohne Zuweisung auf Klammerebene 0. Einfache Aufrufe ohne Verkettung bleiben beim bisherigen Pfad.
fn expression_statement(toks: &[Token], line: usize) -> Option<Expr> {
    if toks.first()?.kind != Kind::Word {
        return None;
    }
    if !toks.iter().any(|t| t.is_punct("->") || t.is_punct("=>")) {
        return None;
    }
    let mut depth = 0i32;
    for t in toks {
        if t.is_punct("(") {
            depth += 1;
        } else if t.is_punct(")") {
            depth -= 1;
        } else if depth == 0 && t.is_punct("=") {
            return None;
        }
    }
    match parse_expr(toks, line) {
        Ok(expr @ Expr::Chain { .. }) => Some(expr),
        _ => None,
    }
}
