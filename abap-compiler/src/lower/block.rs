//! Blöcke: IF, CASE, DO, WHILE, LOOP.

use super::*;

/// Werte eines WHEN: `WHEN 1.`, `WHEN 2 OR 3.`; `WHEN OTHERS` liefert None.
pub(super) fn when_values(toks: &[Token], line: usize) -> Result<Option<Vec<Expr>>, CompileError> {
    if toks.len() == 1 && toks[0].is_word("OTHERS") {
        return Ok(None);
    }
    let mut values = Vec::new();
    for part in toks.split(|t| t.is_word("OR")) {
        if part.is_empty() {
            return err(line, "WHEN without value");
        }
        values.push(parse_expr(part, line)?);
    }
    Ok(Some(values))
}

impl Lowerer {
    pub(super) fn lower_block(
        &mut self,
        head: &Statement,
        body: &[Item],
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        let line = head.line;
        let toks = &head.tokens;
        match head.keyword().as_str() {
            "IF" => {
                // Segmente: (Bedingung, Körper); ELSEIF und ELSE beginnen ein neues Segment.
                let mut segs: Vec<(Option<Cond>, Vec<Item>)> =
                    vec![(Some(parse_cond(&toks[1..], line)?), Vec::new())];
                for item in body {
                    match item {
                        Item::Statement(s) if s.keyword() == "ELSEIF" => {
                            if matches!(segs.last(), Some((None, _))) {
                                return err(s.line, "ELSEIF after ELSE");
                            }
                            segs.push((Some(parse_cond(&s.tokens[1..], s.line)?), Vec::new()));
                        }
                        Item::Statement(s) if s.keyword() == "ELSE" && s.tokens.len() == 1 => {
                            if matches!(segs.last(), Some((None, _))) {
                                return err(s.line, "duplicate ELSE");
                            }
                            segs.push((None, Vec::new()));
                        }
                        _ => segs.last_mut().unwrap().1.push(item.clone()),
                    }
                }
                let mut branches = Vec::new();
                let mut otherwise = Vec::new();
                for (cond, items) in segs {
                    let mut lowered = Vec::new();
                    self.lower_nodes(&nodes(&items), &mut lowered)?;
                    match cond {
                        Some(c) => branches.push((c, lowered)),
                        None => otherwise = lowered,
                    }
                }
                out.push(Stmt::If {
                    branches,
                    otherwise,
                    line,
                });
                Ok(())
            }
            "CASE" => {
                if toks.len() < 2 {
                    return err(line, "CASE without expression");
                }
                let subject = parse_expr(&toks[1..], line)?;
                // Segmente: (Werte, Körper); None = WHEN OTHERS
                let mut segs: Vec<(Option<Vec<Expr>>, Vec<Item>)> = Vec::new();
                for item in body {
                    match item {
                        Item::Statement(s) if s.keyword() == "WHEN" => {
                            if matches!(segs.last(), Some((None, _))) {
                                return err(s.line, "WHEN after WHEN OTHERS");
                            }
                            let values = when_values(&s.tokens[1..], s.line)?;
                            segs.push((values, Vec::new()));
                        }
                        _ => match segs.last_mut() {
                            Some(seg) => seg.1.push(item.clone()),
                            None => return err(line, "Statement before the first WHEN in CASE"),
                        },
                    }
                }
                let mut whens = Vec::new();
                let mut other = Vec::new();
                for (values, items) in segs {
                    let mut lowered = Vec::new();
                    self.lower_nodes(&nodes(&items), &mut lowered)?;
                    match values {
                        Some(v) => whens.push((v, lowered)),
                        None => other = lowered,
                    }
                }
                out.push(Stmt::Case {
                    subject,
                    whens,
                    other,
                    line,
                });
                Ok(())
            }
            "DO" => {
                let times = if toks.len() == 1 {
                    None
                } else {
                    let last = toks.last().unwrap();
                    if toks.len() < 3 || !last.is_word("TIMES") {
                        return err(line, "DO expects 'DO n TIMES.' or 'DO.'");
                    }
                    Some(parse_expr(&toks[1..toks.len() - 1], line)?)
                };
                let mut inner = Vec::new();
                self.lower_nodes(&nodes(body), &mut inner)?;
                out.push(Stmt::Do {
                    times,
                    body: inner,
                    line,
                });
                Ok(())
            }
            "WHILE" => {
                let cond = parse_cond(&toks[1..], line)?;
                let mut inner = Vec::new();
                self.lower_nodes(&nodes(body), &mut inner)?;
                out.push(Stmt::While {
                    cond,
                    body: inner,
                    line,
                });
                Ok(())
            }
            "LOOP" => {
                // LOOP AT tabelle INTO arbeitsbereich | ASSIGNING <feldsymbol>
                if toks.get(3).is_some_and(|token| token.is_punct("->")) {
                    out.push(Stmt::ExternalOperation {
                        name: "loop_attribute_table".into(),
                        line,
                    });
                    self.lower_nodes(&nodes(body), out)?;
                    return Ok(());
                }
                if toks.iter().any(|token| token.is_word("REFERENCE")) {
                    out.push(Stmt::ExternalOperation {
                        name: "loop_reference_into".into(),
                        line,
                    });
                    self.lower_nodes(&nodes(body), out)?;
                    return Ok(());
                }
                if toks.iter().any(|token| token.is_word("TRANSPORTING")) {
                    out.push(Stmt::ExternalOperation {
                        name: "loop_transporting_no_fields".into(),
                        line,
                    });
                    self.lower_nodes(&nodes(body), out)?;
                    return Ok(());
                }
                if !toks
                    .iter()
                    .any(|token| token.is_word("INTO") || token.is_word("ASSIGNING"))
                {
                    out.push(Stmt::ExternalOperation {
                        name: "loop_screen".into(),
                        line,
                    });
                    self.lower_nodes(&nodes(body), out)?;
                    return Ok(());
                }
                if toks
                    .iter()
                    .any(|token| token.is_word("FROM") || token.is_word("TO"))
                {
                    out.push(Stmt::ExternalOperation {
                        name: "loop_range".into(),
                        line,
                    });
                    self.lower_nodes(&nodes(body), out)?;
                    return Ok(());
                }
                if toks.len() < 5 || !toks[1].is_word("AT") || toks[2].kind != Kind::Word {
                    return err(
                        line,
                        "LOOP supports only 'LOOP AT table INTO|ASSIGNING target.'",
                    );
                }
                let table = word(&toks[2]);
                let target = if toks[3].is_word("INTO") {
                    LoopTarget::Into(word(&toks[4]))
                } else if toks[3].is_word("ASSIGNING") {
                    LoopTarget::Assigning(word(&toks[4]))
                } else {
                    return err(line, "LOOP expects INTO or ASSIGNING");
                };
                let filter = match toks.iter().position(|token| token.is_word("WHERE")) {
                    None => None,
                    Some(where_index) => {
                        if toks
                            .get(where_index + 1)
                            .is_some_and(|token| token.is_punct("("))
                        {
                            None
                        } else if toks[where_index + 1..]
                            .iter()
                            .any(|token| token.is_word("AND") || token.is_word("OR"))
                        {
                            None
                        } else {
                            if !toks
                                .get(where_index + 2)
                                .is_some_and(|token| token.is_punct("="))
                            {
                                None
                            } else {
                                let component = toks
                                    .get(where_index + 1)
                                    .filter(|token| token.kind == Kind::Word)
                                    .ok_or_else(|| CompileError {
                                        line,
                                        message: "LOOP WHERE without component".into(),
                                    })?;
                                Some(LoopFilter {
                                    component: word(component),
                                    value: parse_expr(&toks[where_index + 3..], line)?,
                                })
                            }
                        }
                    }
                };
                let mut inner = Vec::new();
                self.lower_nodes(&nodes(body), &mut inner)?;
                out.push(Stmt::Loop {
                    table,
                    target,
                    filter,
                    body: inner,
                    line,
                });
                Ok(())
            }
            "AT" => {
                out.push(Stmt::ExternalOperation {
                    name: format!("at_{}", word(&toks[1])),
                    line,
                });
                self.lower_nodes(&nodes(body), out)?;
                Ok(())
            }
            "TRY" => {
                let mut protected = Vec::new();
                let mut catches: Vec<(String, Option<String>, usize, Vec<Item>)> = Vec::new();
                let mut cleanup = None;
                let mut destination = &mut protected;
                for item in body {
                    match item {
                        Item::Statement(statement) if statement.keyword() == "CATCH" => {
                            if statement.tokens.len() < 2 {
                                return err(statement.line, "CATCH expects an exception class");
                            }
                            let class = word(&statement.tokens[1]);
                            let target = statement
                                .tokens
                                .iter()
                                .position(|token| token.is_word("INTO"))
                                .and_then(|index| statement.tokens.get(index + 1))
                                .filter(|token| token.kind == Kind::Word)
                                .map(word);
                            catches.push((class, target, statement.line, Vec::new()));
                            destination = &mut catches.last_mut().unwrap().3;
                        }
                        Item::Statement(statement)
                            if statement.keyword() == "CLEANUP" && statement.tokens.len() == 1 =>
                        {
                            cleanup = Some(Vec::new());
                            destination = cleanup.as_mut().unwrap();
                        }
                        _ => destination.push(item.clone()),
                    }
                }
                let lowered_body = {
                    let mut lowered = Vec::new();
                    self.lower_nodes(&nodes(&protected), &mut lowered)?;
                    lowered
                };
                let mut lowered_catches = Vec::new();
                for (class, target, line, body) in catches {
                    let mut lowered = Vec::new();
                    self.lower_nodes(&nodes(&body), &mut lowered)?;
                    lowered_catches.push(CatchClause {
                        class,
                        target,
                        body: lowered,
                        line,
                    });
                }
                let mut lowered_cleanup = Vec::new();
                if let Some(items) = cleanup {
                    self.lower_nodes(&nodes(&items), &mut lowered_cleanup)?;
                }
                out.push(Stmt::Try {
                    body: lowered_body,
                    catches: lowered_catches,
                    cleanup: lowered_cleanup,
                    line,
                });
                Ok(())
            }
            "SELECT" => self.lower_select_loop(head, body, out),
            other => err(line, format!("Unsupported block statement: {other}")),
        }
    }
}
