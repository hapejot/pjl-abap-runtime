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
            return err(line, "WHEN ohne Wert");
        }
        values.push(parse_expr(part, line)?);
    }
    Ok(Some(values))
}

impl Lowerer {
    pub(super) fn lower_block(&mut self, head: &Statement, body: &[Item], out: &mut Vec<Stmt>) -> Result<(), CompileError> {
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
                                return err(s.line, "ELSEIF nach ELSE");
                            }
                            segs.push((Some(parse_cond(&s.tokens[1..], s.line)?), Vec::new()));
                        }
                        Item::Statement(s) if s.keyword() == "ELSE" && s.tokens.len() == 1 => {
                            if matches!(segs.last(), Some((None, _))) {
                                return err(s.line, "doppeltes ELSE");
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
                out.push(Stmt::If { branches, otherwise, line });
                Ok(())
            }
            "CASE" => {
                if toks.len() < 2 {
                    return err(line, "CASE ohne Ausdruck");
                }
                let subject = parse_expr(&toks[1..], line)?;
                // Segmente: (Werte, Körper); None = WHEN OTHERS
                let mut segs: Vec<(Option<Vec<Expr>>, Vec<Item>)> = Vec::new();
                for item in body {
                    match item {
                        Item::Statement(s) if s.keyword() == "WHEN" => {
                            if matches!(segs.last(), Some((None, _))) {
                                return err(s.line, "WHEN nach WHEN OTHERS");
                            }
                            let values = when_values(&s.tokens[1..], s.line)?;
                            segs.push((values, Vec::new()));
                        }
                        _ => match segs.last_mut() {
                            Some(seg) => seg.1.push(item.clone()),
                            None => return err(line, "Anweisung vor dem ersten WHEN in CASE"),
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
                out.push(Stmt::Case { subject, whens, other, line });
                Ok(())
            }
            "DO" => {
                let times = if toks.len() == 1 {
                    None
                } else {
                    let last = toks.last().unwrap();
                    if toks.len() < 3 || !last.is_word("TIMES") {
                        return err(line, "DO erwartet 'DO n TIMES.' oder 'DO.'");
                    }
                    Some(parse_expr(&toks[1..toks.len() - 1], line)?)
                };
                let mut inner = Vec::new();
                self.lower_nodes(&nodes(body), &mut inner)?;
                out.push(Stmt::Do { times, body: inner, line });
                Ok(())
            }
            "WHILE" => {
                let cond = parse_cond(&toks[1..], line)?;
                let mut inner = Vec::new();
                self.lower_nodes(&nodes(body), &mut inner)?;
                out.push(Stmt::While { cond, body: inner, line });
                Ok(())
            }
            "LOOP" => {
                // LOOP AT tabelle INTO arbeitsbereich | ASSIGNING <feldsymbol>
                if toks.len() != 5 || !toks[1].is_word("AT") || toks[2].kind != Kind::Word {
                    return err(line, "LOOP unterstützt nur 'LOOP AT tabelle INTO|ASSIGNING ziel.'");
                }
                let table = word(&toks[2]);
                let target = if toks[3].is_word("INTO") {
                    LoopTarget::Into(word(&toks[4]))
                } else if toks[3].is_word("ASSIGNING") {
                    LoopTarget::Assigning(word(&toks[4]))
                } else {
                    return err(line, "LOOP erwartet INTO oder ASSIGNING");
                };
                let mut inner = Vec::new();
                self.lower_nodes(&nodes(body), &mut inner)?;
                out.push(Stmt::Loop { table, target, body: inner, line });
                Ok(())
            }
            other => err(line, format!("Nicht unterstütztes Statement: {other}")),
        }
    }
}
