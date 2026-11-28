//! SQL-Anweisungen (Open SQL, Phasen 1 und 2): `SELECT` als Einzelsatz, mit `INTO TABLE`, `APPENDING TABLE`,
//! `CORRESPONDING FIELDS OF` und als `SELECT ... ENDSELECT`; dazu `INSERT`, `UPDATE`, `DELETE`, `MODIFY` sowie
//! `COMMIT WORK` und `ROLLBACK WORK`. Die Tokens werden gegliedert, gegen die DDIC-Struktur der Tabelle geprüft
//! und als `SqlRequest` (Crate `abap_sql_ir`) in `Ir::sql_requests` abgelegt. Das Programm bekommt
//! `Stmt::Sql` bzw. `Stmt::SqlLoop` für Lesezugriffe, `Stmt::SqlWrite` für Schreibzugriffe und `Stmt::SqlCommit`
//! für die Transaktion; sie verweisen über ihre `id` auf die Anfrage. Interne Formen bleiben `ExternalOperation`.
//! Spezifikation: `doc/sql-spezifikation.md`, Abschnitte 4, 5, 10, 16 und 17.
//!
//! Annahmen, die im Bericht stehen: `NOT` ist sowohl als Präfix vor einem Prädikat als auch zwischen Spalte und
//! `IN`/`LIKE`/`BETWEEN` zulässig. Spalte gegen Spalte ist ein gültiger Vergleich. `UP TO 0 ROWS` und
//! `UP TO` zusammen mit `SINGLE` sind abgelehnt. NUMC-Spalten sind Zeichenwerte, daher ist ein Vergleich mit
//! einem numerischen Literal abgelehnt.

use super::*;
use crate::dynpro::FieldType;
use abap_sql_ir as sqlir;

/// Art des Ziels nach `INTO` bzw. `APPENDING`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IntoMode {
    Single,
    Table,
    Append,
}

/// Komponenten einer Zeile mit Typart, Offsets und Gesamtgröße.
type RowSlots = (Vec<(Ty, sqlir::CharType)>, Vec<u64>, u64);

/// Ziel nach `INTO`: bestehende Variable oder inline deklariert (`@DATA(x)`).
#[derive(Clone, PartialEq, Eq)]
enum IntoName {
    Var(String),
    Declared(String),
}

/// Klausel eines `SELECT`, die ein Schlüsselwort einleitet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clause {
    Where,
    OrderBy,
    UpTo,
    Into,
    Appending,
}

/// Gegliederte SELECT-Anweisung, noch ohne Auflösung gegen den DDIC.
struct Clauses<'a> {
    single: bool,
    /// Spaltennamen in Reihenfolge (Kleinbuchstaben) mit Zeile; `None` bei `*`
    columns: Option<Vec<(String, usize)>>,
    /// Tabellenname (Kleinbuchstaben)
    table: String,
    where_: Option<&'a [Token]>,
    order_by: Option<&'a [Token]>,
    up_to: Option<u64>,
    /// Zielart, Ziel und Zeile des Schlüsselworts
    into: (IntoMode, IntoName, usize),
    /// `INTO CORRESPONDING FIELDS OF` bzw. `APPENDING CORRESPONDING FIELDS OF`: Zuordnung über den Namen
    corresponding: bool,
}

/// Spalte der Quelltabelle aus der Strukturdefinition.
struct SqlColumn {
    name: String,
    ty: Ty,
    /// Typart (CHAR, NUMC, DATS, TIMS); ohne Angabe in der Quelle `char`
    dtype: sqlir::CharType,
    /// Open-SQL-Typ; `None` bei Typen, die Phase 1 nicht abbildet (z. B. STRING)
    abap: Option<sqlir::AbapType>,
}

/// Ziel der Anweisung: Zeilenaufbau, Komponenten mit Typart, Offsets und Gesamtgröße.
struct IntoTarget {
    decls: Vec<Stmt>,
    target: String,
    row: Ty,
    comps: Vec<(Ty, sqlir::CharType)>,
    offsets: Vec<u64>,
    size: u64,
    /// Namen der Komponenten (bei einer Struktur, sonst leer), für `CORRESPONDING FIELDS OF`
    names: Vec<String>,
}

/// Ergebnis der Übersetzung einer SELECT-Anweisung.
struct SelectPlan {
    decls: Vec<Stmt>,
    id: u32,
    params: Vec<String>,
    target: String,
}

/// Ein Operand im WHERE: Spalte, Konstante oder Hostvariable.
struct Operand {
    expr: sqlir::Expr,
    is_column: bool,
    is_char: bool,
    /// Beschreibung für Fehlermeldungen, z. B. `column carrid (CHAR)` oder `numeric value`
    desc: String,
}

impl Lowerer {
    /// `SELECT` ohne ENDSELECT: Einzelsatz oder Tabellenziel (`Stmt::Sql`).
    pub(super) fn lower_select(
        &mut self,
        s: &Statement,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        let plan = self.plan_select(s, false)?;
        out.extend(plan.decls);
        out.push(Stmt::Sql {
            id: plan.id,
            params: plan.params,
            target: plan.target,
            line: s.line,
        });
        Ok(())
    }

    /// `SELECT ... ENDSELECT`: Cursor, der Körper läuft je Zeile (`Stmt::SqlLoop`).
    pub(super) fn lower_select_loop(
        &mut self,
        head: &Statement,
        body: &[Item],
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        let plan = match self.plan_select(head, true) {
            Ok(plan) => plan,
            Err(e) => {
                // Der Körper wird trotzdem übersetzt, damit seine Fehler mit ausgegeben werden
                let mut ignored = Vec::new();
                let _ = self.lower_nodes(&nodes(body), &mut ignored);
                return Err(e);
            }
        };
        out.extend(plan.decls);
        let mut inner = Vec::new();
        self.lower_nodes(&nodes(body), &mut inner)?;
        out.push(Stmt::SqlLoop {
            id: plan.id,
            params: plan.params,
            target: plan.target,
            body: inner,
            line: head.line,
        });
        Ok(())
    }

    /// Übersetzt die SELECT-Anweisung in eine `SqlRequest` und liefert die Deklarationen davor.
    fn plan_select(&mut self, s: &Statement, looping: bool) -> Result<SelectPlan, CompileError> {
        let line = s.line;
        let toks = statement_tokens(s);
        let c = split_clauses(toks, line)?;
        let (mode, into_name, into_line) = c.into.clone();
        if mode == IntoMode::Single && !looping && !c.single {
            return err(line, "SELECT without SINGLE needs ENDSELECT");
        }
        if looping && (c.single || mode != IntoMode::Single) {
            return err(
                line,
                "SELECT with SINGLE or INTO TABLE cannot have ENDSELECT",
            );
        }
        let Some(Ty::Struct(sidx)) = self.names.get(&c.table).copied() else {
            return err(
                line,
                format!(
                    "unknown table {} (no DDIC information)",
                    c.table.to_uppercase()
                ),
            );
        };
        let table = c.table.to_uppercase();
        let cols = self.table_columns(sidx);
        let star = c.columns.is_none();
        let selected: Vec<usize> = match &c.columns {
            None => (0..cols.len()).collect(),
            Some(names) => names
                .iter()
                .map(|(name, at)| column_index(&cols, name, &table, *at))
                .collect::<Result<Vec<usize>, CompileError>>()?,
        };
        for &k in &selected {
            require_abap(&cols[k], line)?;
        }

        let result = match (mode, looping) {
            (IntoMode::Table, _) => sqlir::ResultMode::Table,
            (IntoMode::Append, _) => sqlir::ResultMode::Append,
            (IntoMode::Single, true) => sqlir::ResultMode::Loop,
            (IntoMode::Single, false) => sqlir::ResultMode::Single,
        };

        let mut params: Vec<(String, sqlir::AbapType)> = Vec::new();
        let filter = match c.where_ {
            Some(toks) => Some(self.where_expr(toks, line, &table, &cols, &mut params)?),
            None => None,
        };
        let order_by = match c.order_by {
            Some(toks) => order_items(toks, line, &table, &cols)?,
            None => Vec::new(),
        };

        let target = self.resolve_target(
            mode,
            &into_name,
            sidx,
            &cols,
            &selected,
            star,
            into_line,
            c.corresponding,
        )?;
        let columns = result_columns(&cols, &selected, &target, line, c.corresponding)?;
        let op = sqlir::SelectOp {
            table,
            columns,
            filter,
            order_by,
            up_to: c.up_to,
            result,
            target_size: target.size,
            target_fields: self.target_fields(&target),
            params: params
                .iter()
                .map(|(name, ty)| sqlir::Param {
                    name: name.clone(),
                    ty: ty.clone(),
                })
                .collect(),
        };
        let id = self.sql_requests.len() as u32;
        self.sql_requests.push(sqlir::SqlRequest {
            ir_version: sqlir::IR_VERSION,
            line: line as u32,
            connection: "DEFAULT".to_string(),
            op: sqlir::SqlOp::Select(op),
        });
        Ok(SelectPlan {
            decls: target.decls,
            id,
            params: params.into_iter().map(|(name, _)| name).collect(),
            target: target.target,
        })
    }

    /// Spalten der Strukturdefinition mit Typart und Open-SQL-Typ.
    fn table_columns(&self, sidx: usize) -> Vec<SqlColumn> {
        self.structs[sidx]
            .fields
            .iter()
            .enumerate()
            .map(|(k, (name, ty))| {
                let dtype = self.dtype_at(sidx, k);
                SqlColumn {
                    name: name.clone(),
                    ty: *ty,
                    dtype,
                    abap: open_type(*ty, dtype),
                }
            })
            .collect()
    }

    /// Komponenten des Ziels mit Open-SQL-Typ und Offset (`SqlOp::Select::target_fields`). Ein Feld hat den
    /// Namen der Variablen. Komponenten ohne Open-SQL-Typ fehlen; sie werden nicht initialisiert.
    fn target_fields(&self, target: &IntoTarget) -> Vec<sqlir::RowField> {
        let names: Vec<String> = match target.row {
            Ty::Struct(index) => self.structs[index]
                .fields
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            _ => vec![target.target.clone()],
        };
        names
            .into_iter()
            .zip(target.comps.iter().zip(target.offsets.iter()))
            .filter_map(|(column, ((ty, dtype), offset))| {
                open_type(*ty, *dtype).map(|ty| sqlir::RowField {
                    column,
                    ty,
                    offset: *offset,
                })
            })
            .collect()
    }

    /// Typart einer Komponente aus `struct_field_types`; fehlt sie, gilt `char`.
    fn dtype_at(&self, sidx: usize, k: usize) -> sqlir::CharType {
        match self.struct_field_types.get(&sidx).and_then(|v| v.get(k)) {
            Some(FieldType::Numc(_)) => sqlir::CharType::Numc,
            Some(FieldType::Date) => sqlir::CharType::Dats,
            Some(FieldType::Time) => sqlir::CharType::Tims,
            _ => sqlir::CharType::Char,
        }
    }

    /// Typart eines einfachen Namens aus `field_types`; fehlt sie, gilt `char`.
    fn plain_dtype(&self, name: &str) -> sqlir::CharType {
        match self.field_types.get(name) {
            Some(FieldType::Numc(_)) => sqlir::CharType::Numc,
            Some(FieldType::Date) => sqlir::CharType::Dats,
            Some(FieldType::Time) => sqlir::CharType::Tims,
            _ => sqlir::CharType::Char,
        }
    }

    /// Typ und Typart einer Variablen, Komponente `s-feld` oder Klassenattribut. `None`, wenn unbekannt.
    pub(super) fn host_type(&self, name: &str) -> Option<(Ty, sqlir::CharType)> {
        if let Some(ty) = self.data_types.get(name) {
            return Some((*ty, self.plain_dtype(name)));
        }
        if let Some((base, component)) = name.split_once('-') {
            if let Some(Ty::Struct(index)) = self.data_types.get(base).copied() {
                let k = self.structs[index]
                    .fields
                    .iter()
                    .position(|(field, _)| field == component)?;
                return Some((self.structs[index].fields[k].1, self.dtype_at(index, k)));
            }
        }
        self.class_attribute_type(name)
            .map(|ty| (ty, sqlir::CharType::Char))
    }

    /// Zielaufbau für `INTO`/`APPENDING`. Deklariert `@DATA(x)` die Variable und legt Typen an.
    #[allow(clippy::too_many_arguments)]
    fn resolve_target(
        &mut self,
        mode: IntoMode,
        name: &IntoName,
        sidx: usize,
        cols: &[SqlColumn],
        selected: &[usize],
        star: bool,
        line: usize,
        corresponding: bool,
    ) -> Result<IntoTarget, CompileError> {
        let mut decls = Vec::new();
        let scalar_dtype = if !star && selected.len() == 1 {
            cols[selected[0]].dtype
        } else {
            sqlir::CharType::Char
        };
        if corresponding && matches!(name, IntoName::Declared(_)) {
            return err(
                line,
                "CORRESPONDING FIELDS OF needs an existing target, not @DATA",
            );
        }
        let (target, row, row_dtype) = match (mode, name) {
            (IntoMode::Append, IntoName::Declared(v)) => {
                return err(
                    line,
                    format!("APPENDING TABLE needs an existing internal table, not @DATA({v})"),
                )
            }
            (IntoMode::Table, IntoName::Declared(v)) => {
                let row = self.declared_row(sidx, cols, selected, star, line)?;
                let elem = match row {
                    Ty::Struct(i) => Elem::Struct(i),
                    Ty::Int => Elem::Int,
                    Ty::Char(n) => Elem::Char(n),
                    _ => {
                        return err(
                            line,
                            format!(
                                "INTO TABLE @DATA({v}) of a single PACKED column is not supported"
                            ),
                        )
                    }
                };
                let table_ty = Ty::Table(elem);
                decls.push(Stmt::Data {
                    name: v.clone(),
                    ty: table_ty,
                    line,
                });
                self.data_types.insert(v.clone(), table_ty);
                if !matches!(row, Ty::Struct(_)) {
                    self.field_types
                        .insert(v.clone(), field_type_for(row, scalar_dtype));
                }
                (v.clone(), row, scalar_dtype)
            }
            (IntoMode::Single, IntoName::Declared(v)) => {
                let row = self.declared_row(sidx, cols, selected, star, line)?;
                decls.push(Stmt::Data {
                    name: v.clone(),
                    ty: row,
                    line,
                });
                self.data_types.insert(v.clone(), row);
                if !matches!(row, Ty::Struct(_)) {
                    self.field_types
                        .insert(v.clone(), field_type_for(row, scalar_dtype));
                }
                (v.clone(), row, scalar_dtype)
            }
            (_, IntoName::Var(v)) => {
                let Some((ty, dtype)) = self.host_type(v) else {
                    return err(line, format!("unknown field '{v}' in SELECT INTO"));
                };
                match (mode, ty) {
                    (IntoMode::Table | IntoMode::Append, Ty::Table(elem)) => {
                        (v.clone(), elem.ty(), self.plain_dtype(v))
                    }
                    (IntoMode::Table | IntoMode::Append, _) => {
                        return err(line, format!("INTO TABLE expects an internal table: '{v}'"))
                    }
                    (IntoMode::Single, Ty::Table(_)) => {
                        return err(line, format!("'{v}' is an internal table, use INTO TABLE"))
                    }
                    (IntoMode::Single, _) => (v.clone(), ty, dtype),
                }
            }
        };
        if corresponding && !matches!(row, Ty::Struct(_)) {
            return err(line, "CORRESPONDING FIELDS OF needs a structure as target");
        }
        let (comps, offsets, size) = self.row_slots(row, row_dtype, line)?;
        let names = match row {
            Ty::Struct(index) => self.structs[index]
                .fields
                .iter()
                .map(|(field, _)| field.clone())
                .collect(),
            _ => Vec::new(),
        };
        Ok(IntoTarget {
            decls,
            target,
            row,
            comps,
            offsets,
            size,
            names,
        })
    }

    /// Typ der Zeile für `@DATA(x)`: ganze Tabelle bei `*`, Feld bei einer Spalte, sonst anonyme Struktur.
    fn declared_row(
        &mut self,
        sidx: usize,
        cols: &[SqlColumn],
        selected: &[usize],
        star: bool,
        line: usize,
    ) -> Result<Ty, CompileError> {
        if star {
            return Ok(Ty::Struct(sidx));
        }
        if selected.len() == 1 {
            return Ok(cols[selected[0]].ty);
        }
        let fields: Vec<(String, Ty)> = selected
            .iter()
            .map(|&k| (cols[k].name.clone(), cols[k].ty))
            .collect();
        let name = self.anon_struct_name(line);
        let index = self.define_struct(&name, fields, line)?;
        self.struct_field_types.insert(
            index,
            selected
                .iter()
                .map(|&k| field_type_for(cols[k].ty, cols[k].dtype))
                .collect(),
        );
        Ok(Ty::Struct(index))
    }

    /// Name der anonymen Struktur einer Spaltenliste; bei Kollision mit einer Zahl versehen.
    fn anon_struct_name(&self, line: usize) -> String {
        let base = format!("__sql_row_{line}");
        let mut name = base.clone();
        let mut n = 1;
        while self.structs.iter().any(|s| s.name == name) {
            n += 1;
            name = format!("{base}_{n}");
        }
        name
    }

    /// Komponenten einer Zeile: Typ, Typart, Offset und Gesamtgröße (gepacktes Layout).
    fn row_slots(
        &self,
        row: Ty,
        dtype: sqlir::CharType,
        line: usize,
    ) -> Result<RowSlots, CompileError> {
        match row {
            Ty::Struct(index) => {
                let mut comps = Vec::new();
                let mut offsets = Vec::new();
                let mut size = 0u64;
                for (k, (_, ty)) in self.structs[index].fields.iter().enumerate() {
                    let bytes = sql_size(*ty).ok_or_else(|| unsupported_target(line))?;
                    comps.push((*ty, self.dtype_at(index, k)));
                    offsets.push(size);
                    size += bytes;
                }
                Ok((comps, offsets, size))
            }
            Ty::Int | Ty::Char(_) | Ty::Packed { .. } => {
                let bytes = sql_size(row).ok_or_else(|| unsupported_target(line))?;
                Ok((vec![(row, dtype)], vec![0], bytes))
            }
            _ => Err(unsupported_target(line)),
        }
    }

    /// Übersetzt den WHERE-Teil in einen Filter. Hostvariablen werden in `params` eingetragen.
    fn where_expr(
        &self,
        toks: &[Token],
        line: usize,
        table: &str,
        cols: &[SqlColumn],
        params: &mut Vec<(String, sqlir::AbapType)>,
    ) -> Result<sqlir::Expr, CompileError> {
        let mut p = CondParser {
            lw: self,
            toks,
            pos: 0,
            line,
            table,
            cols,
            params,
        };
        let expr = p.or_expr()?;
        if let Some(t) = toks.get(p.pos) {
            return err(t.line, format!("unexpected token '{}' in WHERE", t.text));
        }
        Ok(expr)
    }
}

/// Schreibende Anweisung nach der Übersetzung: Anfrage, Hostvariablen und Name des Zielbereichs.
struct WritePlan {
    op: sqlir::SqlOp,
    params: Vec<String>,
    target: String,
}

/// Name der Anweisung in `Stmt::ExternalOperation`, wenn es eine interne Form ist (unverändert gegenüber Phase 1).
fn internal_name(kw: &str) -> &'static str {
    match kw {
        "INSERT" => "insert",
        "UPDATE" => "update",
        "DELETE" => "delete_table_range",
        "MODIFY" => "modify",
        "COMMIT" => "commit",
        _ => "rollback",
    }
}

/// `COMMIT WORK [AND WAIT]` (`Some(false)`) und `ROLLBACK WORK` (`Some(true)`). Andere Formen: `None`.
fn transaction_form(toks: &[Token]) -> Option<bool> {
    match toks {
        [c, w] if c.is_word("COMMIT") && w.is_word("WORK") => Some(false),
        [c, w, a, wait]
            if c.is_word("COMMIT")
                && w.is_word("WORK")
                && a.is_word("AND")
                && wait.is_word("WAIT") =>
        {
            Some(false)
        }
        [r, w] if r.is_word("ROLLBACK") && w.is_word("WORK") => Some(true),
        _ => None,
    }
}

/// Entfernt ein Hostzeichen `@` oder `:` vor einem Namen.
fn without_host_marker(toks: &[Token]) -> &[Token] {
    match toks.first() {
        Some(p) if p.is_punct("@") || p.is_punct(":") => &toks[1..],
        _ => toks,
    }
}

/// Teilt Tokens am ersten Wort `WHERE`: Teil davor und Bedingung danach (ohne das Wort).
fn split_where(toks: &[Token]) -> (&[Token], Option<&[Token]>) {
    match toks.iter().position(|t| t.is_word("WHERE")) {
        Some(i) => (&toks[..i], Some(&toks[i + 1..])),
        None => (toks, None),
    }
}

/// Typ des Werts einer Zuweisung: Hostvariable (Typ aus der Parameterliste), Ganzzahl oder Zeichenkette.
fn value_type(expr: &sqlir::Expr, params: &[(String, sqlir::AbapType)]) -> sqlir::AbapType {
    match expr {
        sqlir::Expr::Param { index } => params[*index as usize].1.clone(),
        sqlir::Expr::Literal {
            value: sqlir::Value::Int { .. },
        } => sqlir::AbapType::Int,
        _ => sqlir::AbapType::Char {
            length: 0,
            dtype: sqlir::CharType::Char,
        },
    }
}

/// Hostparameter als IR-Liste.
fn ir_params(params: &[(String, sqlir::AbapType)]) -> Vec<sqlir::Param> {
    params
        .iter()
        .map(|(name, ty)| sqlir::Param {
            name: name.clone(),
            ty: ty.clone(),
        })
        .collect()
}

/// Namen der Hostparameter in Reihenfolge.
fn param_names(params: &[(String, sqlir::AbapType)]) -> Vec<String> {
    params.iter().map(|(name, _)| name.clone()).collect()
}

impl Lowerer {
    /// Schreibende Anweisungen (`INSERT`, `UPDATE`, `DELETE`, `MODIFY`) und `COMMIT WORK`/`ROLLBACK WORK`.
    /// Erkennt der Compiler eine Open-SQL-Form, entstehen `Stmt::SqlWrite` bzw. `Stmt::SqlCommit`. Interne Formen
    /// (z. B. `INSERT wa INTO TABLE itab`, `DELETE itab ...`) bleiben `Stmt::ExternalOperation`.
    pub(super) fn lower_write(
        &mut self,
        s: &Statement,
        kw: &str,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        let line = s.line;
        let toks = statement_tokens(s);
        let plan = match kw {
            "COMMIT" | "ROLLBACK" => match transaction_form(toks) {
                Some(rollback) => {
                    out.push(Stmt::SqlCommit { rollback, line });
                    return Ok(());
                }
                None => None,
            },
            "INSERT" => self.plan_insert(toks, line)?,
            "UPDATE" => Some(self.plan_update(toks, line)?),
            "DELETE" => self.plan_delete(toks, line)?,
            "MODIFY" => self.plan_modify(toks, line)?,
            _ => None,
        };
        match plan {
            Some(plan) => {
                let id = self.sql_requests.len() as u32;
                self.sql_requests.push(sqlir::SqlRequest {
                    ir_version: sqlir::IR_VERSION,
                    line: line as u32,
                    connection: "DEFAULT".to_string(),
                    op: plan.op,
                });
                out.push(Stmt::SqlWrite {
                    id,
                    params: plan.params,
                    target: plan.target,
                    line,
                });
            }
            None => out.push(Stmt::ExternalOperation {
                name: internal_name(kw).into(),
                line,
            }),
        }
        Ok(())
    }

    /// Ist der Name eine lokale Variable, ein Klassenattribut oder ein Feld mit Tabellentyp? Solche Namen
    /// haben Vorrang vor DB-Tabellen (interne Form, bleibt `ExternalOperation`). Nur DDIC-Namen gelten als Datenbank.
    fn is_local(&self, t: &Token) -> bool {
        self.host_type(&word(t)).is_some()
    }

    /// DDIC-Tabelle (Struktur) aus einem Namen. Ohne DDIC-Information ist es ein Übersetzungsfehler.
    fn db_table(&self, t: &Token, line: usize) -> Result<(String, usize), CompileError> {
        let name = word(t);
        match self.names.get(&name).copied() {
            Some(Ty::Struct(sidx)) => Ok((name.to_uppercase(), sidx)),
            _ => err(
                line,
                format!(
                    "unknown table {} (no DDIC information)",
                    name.to_uppercase()
                ),
            ),
        }
    }

    /// Eine Hostvariable (`wa`, `@wa`, `:wa`) mit Typ und Typart.
    fn host_name(
        &self,
        toks: &[Token],
        line: usize,
    ) -> Result<(String, Ty, sqlir::CharType), CompileError> {
        match without_host_marker(toks) {
            [t] if t.kind == Kind::Word => {
                let name = word(t);
                match self.host_type(&name) {
                    Some((ty, dtype)) => Ok((name, ty, dtype)),
                    None => err(line, format!("unknown host variable '{name}' in statement")),
                }
            }
            _ => err(line, "expected a host variable"),
        }
    }

    /// Zeilenquelle: Komponenten einer Zeile stehen nach Position den Spalten der Tabelle gegenüber.
    /// Annahme (nicht im Belegbericht): gleiche Anzahl und Art, sonst Übersetzungsfehler.
    fn row_source(
        &self,
        table: &str,
        cols: &[SqlColumn],
        row: Ty,
        dtype: sqlir::CharType,
        mode: sqlir::RowMode,
        line: usize,
    ) -> Result<sqlir::RowSource, CompileError> {
        let (comps, offsets, size) = self.row_slots(row, dtype, line)?;
        if comps.len() != cols.len() {
            return err(
                line,
                format!(
                    "structure does not match table {table}: {} components, table has {} columns",
                    comps.len(),
                    cols.len()
                ),
            );
        }
        let mut fields = Vec::with_capacity(comps.len());
        for (k, (ty, comp_dtype)) in comps.iter().enumerate() {
            let Some(source) = open_type(*ty, *comp_dtype) else {
                return err(
                    line,
                    format!("component {} of this type is not supported", k + 1),
                );
            };
            let dest = require_abap(&cols[k], line)?;
            if !kinds_compatible(&source, &dest) {
                return err(
                    line,
                    format!(
                        "structure does not match table {table}: column {} needs {}, component has {}",
                        cols[k].name,
                        type_name(&dest),
                        type_name(&source)
                    ),
                );
            }
            fields.push(sqlir::RowField {
                column: cols[k].name.clone(),
                ty: source,
                offset: offsets[k],
            });
        }
        Ok(sqlir::RowSource { mode, fields, size })
    }

    /// Zeilenquelle nach `FROM`: `wa` (Arbeitsbereich) oder `TABLE itab`. Liefert den Namen des Zielbereichs.
    fn write_source(
        &self,
        table: &str,
        cols: &[SqlColumn],
        toks: &[Token],
        line: usize,
    ) -> Result<(String, sqlir::RowSource), CompileError> {
        if toks.first().is_some_and(|t| t.is_word("TABLE")) {
            let (name, ty, _) = self.host_name(&toks[1..], line)?;
            let Ty::Table(elem) = ty else {
                return err(line, format!("TABLE expects an internal table: '{name}'"));
            };
            let rows = self.row_source(
                table,
                cols,
                elem.ty(),
                sqlir::CharType::Char,
                sqlir::RowMode::Table,
                line,
            )?;
            return Ok((name, rows));
        }
        let (name, ty, dtype) = self.host_name(toks, line)?;
        if !matches!(ty, Ty::Struct(_)) {
            return err(line, format!("work area expected: '{name}'"));
        }
        let rows = self.row_source(table, cols, ty, dtype, sqlir::RowMode::Single, line)?;
        Ok((name, rows))
    }

    /// `INSERT INTO tab VALUES wa`, `INSERT tab FROM wa`, `INSERT tab FROM TABLE itab [ACCEPTING DUPLICATE KEYS]`.
    /// `None` bei der internen Form.
    fn plan_insert(&self, toks: &[Token], line: usize) -> Result<Option<WritePlan>, CompileError> {
        match (toks.get(1), toks.get(2)) {
            (Some(into), Some(name)) if into.is_word("INTO") => {
                if name.is_word("TABLE") || name.kind != Kind::Word || self.is_local(name) {
                    return Ok(None);
                }
                match toks.get(3) {
                    Some(t) if t.is_word("VALUES") => {}
                    Some(t) if t.is_punct("(") => {
                        return err(line, "INSERT with column list is not supported")
                    }
                    _ => return err(line, "INSERT INTO expects VALUES"),
                }
                let rest = &toks[4..];
                if rest.first().is_some_and(|t| t.is_word("TABLE")) {
                    return err(line, "INSERT INTO ... VALUES expects a work area");
                }
                let (table, sidx) = self.db_table(name, line)?;
                let cols = self.table_columns(sidx);
                let (target, rows) = self.write_source(&table, &cols, rest, line)?;
                Ok(Some(WritePlan {
                    op: sqlir::SqlOp::Insert(sqlir::InsertOp {
                        table,
                        rows,
                        accepting_duplicate_keys: false,
                    }),
                    params: Vec::new(),
                    target,
                }))
            }
            (Some(name), Some(from)) if from.is_word("FROM") => {
                if name.kind != Kind::Word || self.is_local(name) {
                    return Ok(None);
                }
                let (table, sidx) = self.db_table(name, line)?;
                let cols = self.table_columns(sidx);
                let tail = &toks[3..];
                let (rest, accepting) = match tail.iter().position(|t| t.is_word("ACCEPTING")) {
                    Some(i) => match &tail[i..] {
                        [_, d, k] if d.is_word("DUPLICATE") && k.is_word("KEYS") => {
                            (&tail[..i], true)
                        }
                        _ => return err(line, "INSERT expects ACCEPTING DUPLICATE KEYS"),
                    },
                    None => (tail, false),
                };
                if accepting && !rest.first().is_some_and(|t| t.is_word("TABLE")) {
                    return err(line, "ACCEPTING DUPLICATE KEYS needs FROM TABLE");
                }
                let (target, rows) = self.write_source(&table, &cols, rest, line)?;
                Ok(Some(WritePlan {
                    op: sqlir::SqlOp::Insert(sqlir::InsertOp {
                        table,
                        rows,
                        accepting_duplicate_keys: accepting,
                    }),
                    params: Vec::new(),
                    target,
                }))
            }
            (Some(name), Some(values)) if values.is_word("VALUES") && name.kind == Kind::Word => {
                err(line, "INSERT VALUES needs INTO")
            }
            _ => Ok(None),
        }
    }

    /// `UPDATE tab SET spalte = wert, ... [WHERE ...]` oder `UPDATE tab FROM wa` (nach Schlüssel).
    fn plan_update(&self, toks: &[Token], line: usize) -> Result<WritePlan, CompileError> {
        let Some(name) = toks.get(1).filter(|t| t.kind == Kind::Word) else {
            return err(line, "UPDATE without table name");
        };
        let (table, sidx) = self.db_table(name, line)?;
        let cols = self.table_columns(sidx);
        match toks.get(2) {
            Some(t) if t.is_word("FROM") => {
                let (target, rows) = self.write_source(&table, &cols, &toks[3..], line)?;
                if rows.mode != sqlir::RowMode::Single {
                    return err(line, "UPDATE FROM expects a work area");
                }
                Ok(WritePlan {
                    op: sqlir::SqlOp::Update(sqlir::UpdateOp {
                        table,
                        assignments: Vec::new(),
                        filter: None,
                        rows: Some(rows),
                        params: Vec::new(),
                    }),
                    params: Vec::new(),
                    target,
                })
            }
            Some(t) if t.is_word("SET") => {
                let (set, where_) = split_where(&toks[3..]);
                let mut params: Vec<(String, sqlir::AbapType)> = Vec::new();
                let assignments = self.assignments(set, &table, &cols, &mut params, line)?;
                let filter = match where_ {
                    Some([]) => return err(line, "WHERE without condition"),
                    Some(cond) => Some(self.where_expr(cond, line, &table, &cols, &mut params)?),
                    None => None,
                };
                Ok(WritePlan {
                    op: sqlir::SqlOp::Update(sqlir::UpdateOp {
                        table,
                        assignments,
                        filter,
                        rows: None,
                        params: ir_params(&params),
                    }),
                    params: param_names(&params),
                    target: String::new(),
                })
            }
            _ => err(line, "UPDATE expects SET or FROM"),
        }
    }

    /// Zuweisungen der `SET`-Liste: `spalte = @x`, `spalte = :x` oder ein Literal. Hostvariablen werden in
    /// `params` eingetragen, damit Zuweisung und `WHERE` dieselbe Nummerierung haben.
    fn assignments(
        &self,
        toks: &[Token],
        table: &str,
        cols: &[SqlColumn],
        params: &mut Vec<(String, sqlir::AbapType)>,
        line: usize,
    ) -> Result<Vec<sqlir::Assignment>, CompileError> {
        if toks.is_empty() {
            return err(line, "UPDATE SET without assignment");
        }
        let mut out = Vec::new();
        for part in toks.split(|t| t.is_punct(",")) {
            let [col, eq, value @ ..] = part else {
                return err(line, "SET expects column = value");
            };
            if col.kind != Kind::Word || !eq.is_punct("=") || value.is_empty() {
                return err(col.line, "SET expects column = value");
            }
            let k = column_index(cols, &word(col), table, col.line)?;
            let dest = require_abap(&cols[k], col.line)?;
            if let [single] = value {
                if single.kind == Kind::Word
                    && !single.text.chars().all(|c| c.is_ascii_digit())
                    && self.host_type(&word(single)).is_some()
                {
                    return err(
                        col.line,
                        format!("host variable '{}' in SET needs @ or :", word(single)),
                    );
                }
            }
            let mut parser = CondParser {
                lw: self,
                toks: value,
                pos: 0,
                line: col.line,
                table,
                cols,
                params: &mut *params,
            };
            let operand = parser.operand()?;
            if parser.pos != value.len() {
                return parser.unexpected("SET value must be one host variable or literal");
            }
            if operand.is_column {
                return err(col.line, "SET value must be a host variable or a literal");
            }
            let source = value_type(&operand.expr, params);
            if !kinds_compatible(&source, &dest) {
                return err(
                    col.line,
                    format!(
                        "conversion from {} to {} not supported",
                        type_name(&source),
                        type_name(&dest)
                    ),
                );
            }
            out.push(sqlir::Assignment {
                column: cols[k].name.clone(),
                value: operand.expr,
            });
        }
        Ok(out)
    }

    /// `DELETE FROM tab [WHERE ...]`, `DELETE tab FROM wa`, `DELETE tab FROM TABLE itab`. Lokale Namen sind die
    /// interne Form (`None`).
    fn plan_delete(&self, toks: &[Token], line: usize) -> Result<Option<WritePlan>, CompileError> {
        match (toks.get(1), toks.get(2)) {
            (Some(from), Some(name)) if from.is_word("FROM") => {
                if name.is_word("TABLE") {
                    return err(line, "DELETE FROM TABLE is not supported");
                }
                if name.kind != Kind::Word || self.is_local(name) {
                    return Ok(None);
                }
                let (table, sidx) = self.db_table(name, line)?;
                let cols = self.table_columns(sidx);
                let mut params: Vec<(String, sqlir::AbapType)> = Vec::new();
                let filter = match &toks[3..] {
                    [] => None,
                    [w, cond @ ..] if w.is_word("WHERE") => {
                        if cond.is_empty() {
                            return err(line, "WHERE without condition");
                        }
                        Some(self.where_expr(cond, line, &table, &cols, &mut params)?)
                    }
                    _ => return err(line, "DELETE FROM expects WHERE or nothing"),
                };
                Ok(Some(WritePlan {
                    op: sqlir::SqlOp::Delete(sqlir::DeleteOp {
                        table,
                        filter,
                        rows: None,
                        params: ir_params(&params),
                    }),
                    params: param_names(&params),
                    target: String::new(),
                }))
            }
            (Some(name), Some(from)) if from.is_word("FROM") => {
                if name.kind != Kind::Word || self.is_local(name) {
                    return Ok(None);
                }
                let (table, sidx) = self.db_table(name, line)?;
                let cols = self.table_columns(sidx);
                let (target, rows) = self.write_source(&table, &cols, &toks[3..], line)?;
                Ok(Some(WritePlan {
                    op: sqlir::SqlOp::Delete(sqlir::DeleteOp {
                        table,
                        filter: None,
                        rows: Some(rows),
                        params: Vec::new(),
                    }),
                    params: Vec::new(),
                    target,
                }))
            }
            _ => Ok(None),
        }
    }

    /// `MODIFY tab FROM wa` und `MODIFY tab FROM TABLE itab`. Lokale Namen sind die interne Form (`None`).
    fn plan_modify(&self, toks: &[Token], line: usize) -> Result<Option<WritePlan>, CompileError> {
        match (toks.get(1), toks.get(2)) {
            (Some(name), Some(from)) if name.kind == Kind::Word && from.is_word("FROM") => {
                if self.is_local(name) {
                    return Ok(None);
                }
                let (table, sidx) = self.db_table(name, line)?;
                let cols = self.table_columns(sidx);
                let (target, rows) = self.write_source(&table, &cols, &toks[3..], line)?;
                Ok(Some(WritePlan {
                    op: sqlir::SqlOp::Modify(sqlir::ModifyOp { table, rows }),
                    params: Vec::new(),
                    target,
                }))
            }
            _ => Ok(None),
        }
    }
}

/// Tokens der Anweisung ohne Pragma (`#EC ...`).
fn statement_tokens(s: &Statement) -> &[Token] {
    let end = s
        .tokens
        .iter()
        .position(|t| t.is_punct("#"))
        .unwrap_or(s.tokens.len());
    &s.tokens[..end]
}

/// Schlüsselwörter und Konstrukte, die Phase 1 ablehnt. Die Meldung nennt das Schlüsselwort und seine Zeile.
fn reject_unsupported(toks: &[Token]) -> Result<(), CompileError> {
    for (i, t) in toks.iter().enumerate() {
        let next = toks.get(i + 1);
        let what: Option<String> = match t.kind {
            Kind::Word => match t.text.to_ascii_uppercase().as_str() {
                "JOIN" => Some("JOIN".into()),
                "AS" => Some("alias (AS)".into()),
                "DISTINCT" => Some("DISTINCT".into()),
                "OFFSET" => Some("OFFSET".into()),
                agg @ ("COUNT" | "SUM" | "MIN" | "MAX" | "AVG")
                    if next.is_some_and(|n| n.is_punct("(")) =>
                {
                    Some(format!("aggregate {agg}"))
                }
                "GROUP" if next.is_some_and(|n| n.is_word("BY")) => Some("GROUP BY".into()),
                "HAVING" => Some("HAVING".into()),
                "FOR" => Some("FOR ALL ENTRIES".into()),
                "CONNECTION" => Some("CONNECTION".into()),
                "IS" if next.is_some_and(|n| n.is_word("NULL") || n.is_word("NOT")) => {
                    Some("IS NULL".into())
                }
                "BYPASSING" => Some("BYPASSING BUFFER".into()),
                "CLIENT" if next.is_some_and(|n| n.is_word("SPECIFIED")) => {
                    Some("CLIENT SPECIFIED".into())
                }
                "PACKAGE" => Some("PACKAGE SIZE".into()),
                "SELECT" if i > 0 => Some("subquery".into()),
                _ => None,
            },
            Kind::Punct if t.is_punct("~") => Some("table alias (~)".into()),
            _ => None,
        };
        if let Some(what) = what {
            return err(t.line, format!("SELECT with {what} is not supported"));
        }
    }
    Ok(())
}

/// Gliedert eine SELECT-Anweisung in Klauseln. Die Klauseln `WHERE`, `ORDER BY`, `UP TO`, `INTO` und
/// `APPENDING` dürfen in beliebiger Reihenfolge stehen.
fn split_clauses(toks: &[Token], line: usize) -> Result<Clauses<'_>, CompileError> {
    reject_unsupported(toks)?;
    let single = toks.get(1).is_some_and(|t| t.is_word("SINGLE"));
    let start = if single { 2 } else { 1 };
    let Some(from) = (start..toks.len()).find(|&j| toks[j].is_word("FROM")) else {
        return err(line, "SELECT without FROM");
    };
    let columns = parse_columns(&toks[start..from], line)?;
    let Some(table) = toks.get(from + 1).filter(|t| t.kind == Kind::Word) else {
        return err(line, "SELECT without table name");
    };
    let table = word(table);

    // Schlüsselwörter auf Klammertiefe 0, mit Index und Beginn des Körpers
    let rest = from + 2;
    let mut keywords: Vec<(Clause, usize, usize)> = Vec::new();
    let mut depth = 0i32;
    let mut j = rest;
    while j < toks.len() {
        let t = &toks[j];
        if t.is_punct("(") {
            depth += 1;
        } else if t.is_punct(")") {
            depth -= 1;
        } else if depth == 0 && t.kind == Kind::Word {
            let next = toks.get(j + 1);
            let found = if t.is_word("WHERE") {
                Some((Clause::Where, 1))
            } else if t.is_word("ORDER") && next.is_some_and(|n| n.is_word("BY")) {
                Some((Clause::OrderBy, 2))
            } else if t.is_word("UP") && next.is_some_and(|n| n.is_word("TO")) {
                Some((Clause::UpTo, 2))
            } else if t.is_word("INTO") {
                Some((Clause::Into, 1))
            } else if t.is_word("APPENDING") {
                Some((Clause::Appending, 1))
            } else {
                None
            };
            if let Some((clause, skip)) = found {
                keywords.push((clause, j, j + skip));
                j += skip;
                continue;
            }
        }
        j += 1;
    }
    let first = keywords.first().map_or(toks.len(), |k| k.1);
    if let Some(t) = toks[rest..first].first() {
        return err(t.line, format!("unexpected token '{}' in SELECT", t.text));
    }

    let mut where_ = None;
    let mut order_by = None;
    let mut up_to = None;
    let mut into = None;
    let mut corresponding = false;
    for (n, &(clause, kw, start)) in keywords.iter().enumerate() {
        let end = keywords.get(n + 1).map_or(toks.len(), |k| k.1);
        let body = &toks[start..end];
        let kw_line = toks[kw].line;
        match clause {
            Clause::Where => {
                if body.is_empty() {
                    return err(kw_line, "WHERE without condition");
                }
                if where_.replace(body).is_some() {
                    return twice("WHERE", kw_line);
                }
            }
            Clause::OrderBy => {
                if body.is_empty() {
                    return err(kw_line, "ORDER BY without column");
                }
                if order_by.replace(body).is_some() {
                    return twice("ORDER BY", kw_line);
                }
            }
            Clause::UpTo => {
                let count = match body {
                    [count, rows]
                        if count.text.chars().all(|c| c.is_ascii_digit())
                            && rows.is_word("ROWS") =>
                    {
                        count.text.parse::<u64>().ok().filter(|n| *n >= 1)
                    }
                    _ => None,
                };
                let Some(count) = count else {
                    return err(kw_line, "UP TO expects 'n ROWS' with n >= 1");
                };
                if up_to.replace(count).is_some() {
                    return twice("UP TO", kw_line);
                }
            }
            Clause::Into | Clause::Appending => {
                // `CORRESPONDING FIELDS OF` steht vor dem Ziel (mit oder ohne `TABLE`)
                let (is_corresponding, body) = match body {
                    [c, f, o, rest @ ..]
                        if c.is_word("CORRESPONDING") && f.is_word("FIELDS") && o.is_word("OF") =>
                    {
                        (true, rest)
                    }
                    _ => (false, body),
                };
                let (mode, target) = if clause == Clause::Appending {
                    match body.split_first() {
                        Some((t, rest)) if t.is_word("TABLE") => (IntoMode::Append, rest),
                        _ => return err(kw_line, "APPENDING without TABLE is not supported"),
                    }
                } else {
                    match body.split_first() {
                        Some((t, rest)) if t.is_word("TABLE") => (IntoMode::Table, rest),
                        _ => (IntoMode::Single, body),
                    }
                };
                let name = into_name(target, kw_line)?;
                if into.replace((mode, name, kw_line)).is_some() {
                    return err(kw_line, "SELECT has more than one INTO or APPENDING");
                }
                corresponding = is_corresponding;
            }
        }
    }

    let Some(into) = into else {
        return err(line, "SELECT without INTO is not supported");
    };
    if single && into.0 != IntoMode::Single {
        return err(
            line,
            "SELECT SINGLE with INTO TABLE or APPENDING TABLE is not allowed",
        );
    }
    if single && up_to.is_some() {
        return err(line, "UP TO n ROWS with SELECT SINGLE is not allowed");
    }
    Ok(Clauses {
        single,
        columns,
        table,
        where_,
        order_by,
        up_to,
        into,
        corresponding,
    })
}

/// Doppelte Klausel.
fn twice<T>(what: &str, line: usize) -> Result<T, CompileError> {
    err(line, format!("SELECT clause {what} given twice"))
}

/// Spaltenliste: `None` bei `*`, sonst die Namen in Reihenfolge (Kommas oder Leerzeichen).
fn parse_columns(
    toks: &[Token],
    line: usize,
) -> Result<Option<Vec<(String, usize)>>, CompileError> {
    if toks.len() == 1 && toks[0].is_punct("*") {
        return Ok(None);
    }
    let mut names = Vec::new();
    for t in toks {
        if t.kind == Kind::Word {
            names.push((word(t), t.line));
        } else if !t.is_punct(",") {
            return err(t.line, "unsupported column list in SELECT");
        }
    }
    if names.is_empty() {
        return err(line, "SELECT without columns");
    }
    Ok(Some(names))
}

/// Ziel nach `INTO`: `@DATA(x)`, `@x`, `:x` oder `x`. Eine Struktur-Komponente `s-feld` bleibt ein Name.
fn into_name(toks: &[Token], line: usize) -> Result<IntoName, CompileError> {
    let toks = match toks.first() {
        Some(p) if p.is_punct("@") || p.is_punct(":") => &toks[1..],
        _ => toks,
    };
    match toks {
        [d, lp, name, rp]
            if d.is_word("DATA")
                && lp.is_punct("(")
                && name.kind == Kind::Word
                && rp.is_punct(")") =>
        {
            Ok(IntoName::Declared(word(name)))
        }
        [name] if name.kind == Kind::Word => Ok(IntoName::Var(word(name))),
        _ => err(line, "unsupported INTO target"),
    }
}

/// Sortierung: Spalten mit optional `ASCENDING` oder `DESCENDING`.
fn order_items(
    toks: &[Token],
    line: usize,
    table: &str,
    cols: &[SqlColumn],
) -> Result<Vec<sqlir::OrderItem>, CompileError> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if t.is_punct(",") {
            i += 1;
            continue;
        }
        if t.is_word("PRIMARY") {
            return err(t.line, "ORDER BY PRIMARY KEY is not supported");
        }
        if t.kind != Kind::Word {
            return err(t.line, format!("unexpected token '{}' in ORDER BY", t.text));
        }
        let k = column_index(cols, &word(t), table, t.line)?;
        require_abap(&cols[k], t.line)?;
        let mut descending = false;
        if let Some(next) = toks.get(i + 1) {
            if next.is_word("DESCENDING") {
                descending = true;
                i += 1;
            } else if next.is_word("ASCENDING") {
                i += 1;
            }
        }
        items.push(sqlir::OrderItem {
            column: cols[k].name.clone(),
            descending,
        });
        i += 1;
    }
    if items.is_empty() {
        return err(line, "ORDER BY without column");
    }
    Ok(items)
}

/// Index einer Spalte in der Tabelle, sonst Übersetzungsfehler.
fn column_index(
    cols: &[SqlColumn],
    name: &str,
    table: &str,
    line: usize,
) -> Result<usize, CompileError> {
    cols.iter()
        .position(|c| c.name == name)
        .ok_or_else(|| CompileError {
            line,
            message: format!("unknown column '{name}' in table {table}"),
        })
}

/// Open-SQL-Typ einer Spalte. Fehlt er (STRING oder anderer Typ), ist die Spalte nicht lesbar.
fn require_abap(col: &SqlColumn, line: usize) -> Result<sqlir::AbapType, CompileError> {
    col.abap.clone().ok_or_else(|| CompileError {
        line,
        message: if col.ty == Ty::Str {
            format!("column {} of type STRING is not supported", col.name)
        } else {
            format!(
                "column {} has a type that is not supported in SELECT",
                col.name
            )
        },
    })
}

/// Abbildung eines Compilertyps mit Typart auf den Open-SQL-Typ. `None` für Typen ohne Entsprechung.
fn open_type(ty: Ty, dtype: sqlir::CharType) -> Option<sqlir::AbapType> {
    match ty {
        Ty::Int => Some(sqlir::AbapType::Int),
        Ty::Char(n) => Some(sqlir::AbapType::Char {
            length: n as u32,
            dtype,
        }),
        Ty::Packed { bytes, scale } => Some(sqlir::AbapType::Packed {
            bytes: bytes as u32,
            scale: scale as u32,
        }),
        _ => None,
    }
}

/// Fehler für ein Ziel, dessen Typ Phase 1 nicht abbildet.
fn unsupported_target(line: usize) -> CompileError {
    CompileError {
        line,
        message: "target or work area of this type is not supported".to_string(),
    }
}

/// Speicherbedarf eines Feldes im gepackten Layout (Summe der Größen, ohne Auffüllung).
fn sql_size(ty: Ty) -> Option<u64> {
    match ty {
        Ty::Int => Some(4),
        Ty::Char(n) => Some(2 * n as u64),
        Ty::Packed { bytes, .. } => Some(bytes as u64),
        _ => None,
    }
}

/// Abbildung des Dynpro-Typs für `struct_field_types` und `field_types`.
fn field_type_for(ty: Ty, dtype: sqlir::CharType) -> FieldType {
    match ty {
        Ty::Int => FieldType::Int,
        Ty::Char(n) => match dtype {
            sqlir::CharType::Numc => FieldType::Numc(n),
            sqlir::CharType::Dats => FieldType::Date,
            sqlir::CharType::Tims => FieldType::Time,
            sqlir::CharType::Char => FieldType::Char(n),
        },
        Ty::Packed { bytes, scale } => FieldType::Packed {
            bytes,
            decimals: scale,
        },
        _ => FieldType::Other,
    }
}

/// Bezeichnung des Typs in Meldungen (`CHAR`, `NUMC`, `DATS`, `TIMS`, `INT`, `PACKED`).
fn type_name(ty: &sqlir::AbapType) -> &'static str {
    match ty {
        sqlir::AbapType::Int => "INT",
        sqlir::AbapType::Packed { .. } => "PACKED",
        sqlir::AbapType::Char { dtype, .. } => match dtype {
            sqlir::CharType::Char => "CHAR",
            sqlir::CharType::Numc => "NUMC",
            sqlir::CharType::Dats => "DATS",
            sqlir::CharType::Tims => "TIMS",
        },
    }
}

/// Zeichenartig (CHAR, NUMC, DATS, TIMS) oder numerisch (INT, PACKED).
fn is_char_type(ty: &sqlir::AbapType) -> bool {
    matches!(ty, sqlir::AbapType::Char { .. })
}

/// Ist die Übertragung von der Quelle ins Ziel erlaubt? Gleiche Art, außer INT in PACKED.
fn kinds_compatible(source: &sqlir::AbapType, target: &sqlir::AbapType) -> bool {
    matches!(
        (source, target),
        (sqlir::AbapType::Char { .. }, sqlir::AbapType::Char { .. })
            | (sqlir::AbapType::Int, sqlir::AbapType::Int)
            | (
                sqlir::AbapType::Packed { .. },
                sqlir::AbapType::Packed { .. }
            )
            | (sqlir::AbapType::Int, sqlir::AbapType::Packed { .. })
    )
}

/// Ergebnisspalten. Ohne `CORRESPONDING` wird Spalte `k` der Auswahl der `n`-ten Komponente des Ziels zugeordnet.
/// Mit `CORRESPONDING FIELDS OF` gilt der Name: Spalten ohne Komponente werden nicht übertragen, Komponenten ohne
/// Spalte bleiben im Ziel unberührt (Abschnitt 17 der Spezifikation, zu prüfen).
fn result_columns(
    cols: &[SqlColumn],
    selected: &[usize],
    target: &IntoTarget,
    line: usize,
    corresponding: bool,
) -> Result<Vec<sqlir::ResultColumn>, CompileError> {
    let is_struct = matches!(target.row, Ty::Struct(_));
    if !corresponding && is_struct && selected.len() != target.comps.len() {
        return err(
            line,
            format!(
                "SELECT list has {} columns but the target structure has {} components",
                selected.len(),
                target.comps.len()
            ),
        );
    }
    if !corresponding && !is_struct && selected.len() != 1 {
        return err(
            line,
            format!(
                "SELECT list has {} columns but the target has a single field",
                selected.len()
            ),
        );
    }
    // Paare (Index in der Auswahl, Index der Komponente im Ziel)
    let pairs: Vec<(usize, usize)> = if corresponding {
        selected
            .iter()
            .enumerate()
            .filter_map(|(n, &k)| {
                target
                    .names
                    .iter()
                    .position(|name| *name == cols[k].name)
                    .map(|j| (n, j))
            })
            .collect()
    } else {
        (0..selected.len()).map(|n| (n, n)).collect()
    };
    let mut out = Vec::new();
    for (n, j) in pairs {
        let k = selected[n];
        let source = require_abap(&cols[k], line)?;
        let (ty, dtype) = target.comps[j];
        let Some(dest) = open_type(ty, dtype) else {
            return err(line, "SELECT target of this type is not supported");
        };
        if !kinds_compatible(&source, &dest) {
            return err(
                line,
                format!(
                    "conversion from {} to {} not supported",
                    type_name(&source),
                    type_name(&dest)
                ),
            );
        }
        out.push(sqlir::ResultColumn {
            name: cols[k].name.clone(),
            source,
            target: dest,
            offset: target.offsets[j],
        });
    }
    Ok(out)
}

/// Parser für die WHERE-Bedingung. Rangfolge: `NOT` vor `AND` vor `OR`; Klammern gruppieren.
struct CondParser<'a, 'b> {
    lw: &'a Lowerer,
    toks: &'a [Token],
    pos: usize,
    line: usize,
    table: &'a str,
    cols: &'a [SqlColumn],
    params: &'b mut Vec<(String, sqlir::AbapType)>,
}

impl CondParser<'_, '_> {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if self.peek().is_some_and(|t| t.is_word(word)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_punct(&mut self, punct: &str) -> bool {
        if self.peek().is_some_and(|t| t.is_punct(punct)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, punct: &str) -> Result<(), CompileError> {
        if self.eat_punct(punct) {
            Ok(())
        } else {
            self.unexpected(&format!("expected '{punct}'"))
        }
    }

    fn expect_word(&mut self, word: &str) -> Result<(), CompileError> {
        if self.eat_word(word) {
            Ok(())
        } else {
            self.unexpected(&format!("expected {word}"))
        }
    }

    /// Fehler an der aktuellen Stelle, mit dem Token oder dem Ende der WHERE-Klausel.
    fn unexpected<T>(&self, detail: &str) -> Result<T, CompileError> {
        match self.peek() {
            Some(t) => err(
                t.line,
                format!("unexpected token '{}' in WHERE ({detail})", t.text),
            ),
            None => err(self.line, format!("unexpected end of WHERE ({detail})")),
        }
    }

    fn or_expr(&mut self) -> Result<sqlir::Expr, CompileError> {
        let mut left = self.and_expr()?;
        while self.eat_word("OR") {
            let right = self.and_expr()?;
            left = sqlir::Expr::Or {
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<sqlir::Expr, CompileError> {
        let mut left = self.not_expr()?;
        while self.eat_word("AND") {
            let right = self.not_expr()?;
            left = sqlir::Expr::And {
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn not_expr(&mut self) -> Result<sqlir::Expr, CompileError> {
        if self.eat_word("NOT") {
            let operand = self.not_expr()?;
            return Ok(sqlir::Expr::Not {
                operand: Box::new(operand),
            });
        }
        self.primary()
    }

    /// Klammergruppe oder Prädikat: Vergleich, `BETWEEN`, `IN`, `LIKE`, jeweils optional mit `NOT`.
    fn primary(&mut self) -> Result<sqlir::Expr, CompileError> {
        if self.eat_punct("(") {
            let inner = self.or_expr()?;
            self.expect_punct(")")?;
            return Ok(inner);
        }
        let left = self.operand()?;
        let negated = self.eat_word("NOT");
        let expr = if self.eat_word("BETWEEN") {
            self.require_column(&left, "BETWEEN")?;
            let low = self.operand()?;
            self.expect_word("AND")?;
            let high = self.operand()?;
            self.check(&left, &low)?;
            self.check(&left, &high)?;
            sqlir::Expr::Between {
                value: Box::new(left.expr),
                low: Box::new(low.expr),
                high: Box::new(high.expr),
            }
        } else if self.eat_word("IN") {
            self.require_column(&left, "IN")?;
            self.expect_punct("(")?;
            let mut list = Vec::new();
            loop {
                let item = self.operand()?;
                self.check(&left, &item)?;
                list.push(item.expr);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct(")")?;
            sqlir::Expr::In {
                value: Box::new(left.expr),
                list,
            }
        } else if self.eat_word("LIKE") {
            self.require_column(&left, "LIKE")?;
            if !left.is_char {
                return err(
                    self.line,
                    format!("type mismatch in WHERE: {} used with LIKE", left.desc),
                );
            }
            let pattern = self.operand()?;
            self.check(&left, &pattern)?;
            if self.peek().is_some_and(|t| t.is_word("ESCAPE")) {
                return err(self.line, "ESCAPE not supported");
            }
            sqlir::Expr::Like {
                value: Box::new(left.expr),
                pattern: Box::new(pattern.expr),
            }
        } else {
            if negated {
                return self.unexpected("NOT must be followed by BETWEEN, IN or LIKE");
            }
            let op = self.cmp_op()?;
            let right = self.operand()?;
            self.check(&left, &right)?;
            sqlir::Expr::Compare {
                op,
                left: Box::new(left.expr),
                right: Box::new(right.expr),
            }
        };
        Ok(if negated {
            sqlir::Expr::Not {
                operand: Box::new(expr),
            }
        } else {
            expr
        })
    }

    fn cmp_op(&mut self) -> Result<sqlir::CmpOp, CompileError> {
        let op = match self.peek() {
            Some(t) if t.is_punct("=") || t.is_word("EQ") => sqlir::CmpOp::Eq,
            Some(t) if t.is_punct("<>") || t.is_word("NE") => sqlir::CmpOp::Ne,
            Some(t) if t.is_punct("<") || t.is_word("LT") => sqlir::CmpOp::Lt,
            Some(t) if t.is_punct("<=") || t.is_word("LE") => sqlir::CmpOp::Le,
            Some(t) if t.is_punct(">") || t.is_word("GT") => sqlir::CmpOp::Gt,
            Some(t) if t.is_punct(">=") || t.is_word("GE") => sqlir::CmpOp::Ge,
            _ => return self.unexpected("expected a comparison operator"),
        };
        self.pos += 1;
        Ok(op)
    }

    /// Ein Operand: Spalte, Literal, negative Ganzzahl oder Hostvariable (`@x`, `:x`).
    fn operand(&mut self) -> Result<Operand, CompileError> {
        let Some(t) = self.peek().cloned() else {
            return self.unexpected("expected an operand");
        };
        let line = t.line;
        if t.is_punct("@") || t.is_punct(":") {
            self.pos += 1;
            let Some(name) = self.peek().filter(|n| n.kind == Kind::Word).map(word) else {
                return self.unexpected("expected a host variable name");
            };
            self.pos += 1;
            return self.host_operand(&name, line);
        }
        if t.is_punct("-") {
            if let Some(n) = self
                .toks
                .get(self.pos + 1)
                .filter(|n| n.kind == Kind::Word && n.text.chars().all(|c| c.is_ascii_digit()))
            {
                self.pos += 2;
                let value = parse_int(&n.text, line)?;
                return Ok(literal_operand(sqlir::Value::Int { value: -value }));
            }
        }
        match t.kind {
            Kind::Literal => {
                self.pos += 1;
                let Some(value) = literal_content(&t.text) else {
                    return err(line, format!("invalid literal {}", t.text));
                };
                Ok(literal_operand(sqlir::Value::Char { value }))
            }
            Kind::Word if t.text.chars().all(|c| c.is_ascii_digit()) => {
                self.pos += 1;
                let value = parse_int(&t.text, line)?;
                Ok(literal_operand(sqlir::Value::Int { value }))
            }
            Kind::Word => {
                self.pos += 1;
                let name = word(&t);
                let k = column_index(self.cols, &name, self.table, line)?;
                let column = &self.cols[k];
                let abap = require_abap(column, line)?;
                Ok(Operand {
                    expr: sqlir::Expr::Column {
                        name: column.name.clone(),
                    },
                    is_column: true,
                    is_char: is_char_type(&abap),
                    desc: format!("column {} ({})", column.name, type_name(&abap)),
                })
            }
            _ => self.unexpected("expected an operand"),
        }
    }

    /// Hostvariable oder Konstante. Hostvariablen werden als Parameter eingetragen, Konstanten als Literal.
    fn host_operand(&mut self, name: &str, line: usize) -> Result<Operand, CompileError> {
        if let Some((ty, dtype)) = self.lw.host_type(name) {
            let Some(abap) = open_type(ty, dtype) else {
                return err(
                    line,
                    format!("host variable '{name}' of this type is not supported in SELECT"),
                );
            };
            let index = match self.params.iter().position(|(n, _)| n == name) {
                Some(index) => index,
                None => {
                    self.params.push((name.to_string(), abap.clone()));
                    self.params.len() - 1
                }
            };
            let is_char = is_char_type(&abap);
            return Ok(Operand {
                expr: sqlir::Expr::Param {
                    index: index as u32,
                },
                is_column: false,
                is_char,
                desc: value_desc(is_char).to_string(),
            });
        }
        match self.lw.constants.get(name) {
            Some(Expr::Int(value)) => Ok(literal_operand(sqlir::Value::Int { value: *value })),
            Some(Expr::Lit(value)) => Ok(literal_operand(sqlir::Value::Char {
                value: value.clone(),
            })),
            _ => err(line, format!("unknown host variable '{name}' in SELECT")),
        }
    }

    /// Vergleich zweier Operanden: mindestens eine Spalte, und beide Seiten gleicher Art.
    fn check(&self, a: &Operand, b: &Operand) -> Result<(), CompileError> {
        if !a.is_column && !b.is_column {
            return err(self.line, "WHERE comparison without a column");
        }
        if a.is_char == b.is_char {
            return Ok(());
        }
        err(
            self.line,
            format!(
                "type mismatch in WHERE: {} compared with {}",
                a.desc, b.desc
            ),
        )
    }

    fn require_column(&self, op: &Operand, what: &str) -> Result<(), CompileError> {
        if op.is_column {
            Ok(())
        } else {
            err(self.line, format!("WHERE needs a column before {what}"))
        }
    }
}

/// Operand für ein Literal.
fn literal_operand(value: sqlir::Value) -> Operand {
    let is_char = matches!(value, sqlir::Value::Char { .. });
    Operand {
        expr: sqlir::Expr::Literal { value },
        is_column: false,
        is_char,
        desc: value_desc(is_char).to_string(),
    }
}

/// Beschreibung eines Werts in Fehlermeldungen.
fn value_desc(is_char: bool) -> &'static str {
    if is_char {
        "character value"
    } else {
        "numeric value"
    }
}

/// Ganzzahl aus einem Token; zu große Werte sind ein Übersetzungsfehler.
fn parse_int(text: &str, line: usize) -> Result<i64, CompileError> {
    text.parse::<i64>().map_err(|_| CompileError {
        line,
        message: "number too large".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use crate::lower::{parse_source, Ir, Stmt};
    use abap_sql_ir::{AbapType, CharType, Expr, OrderItem, ResultMode, SelectOp, SqlOp};

    /// Typen für die Tests: Flugtabelle mit CHAR, NUMC, INT und PACKED-Spalten.
    const FLUG: &str = "TYPES: BEGIN OF zflug,\n\
          carrid TYPE c LENGTH 3,\n\
          connid TYPE n LENGTH 4,\n\
          seatsmax TYPE i,\n\
        END OF zflug.\n\
        DATA ls_flug TYPE zflug.\n\
        DATA lv_von TYPE c LENGTH 3.\n\
        DATA lv_max TYPE i.\n";

    fn program(body: &str) -> String {
        format!("{FLUG}START-OF-SELECTION.\n{body}\n")
    }

    fn lower(body: &str) -> Ir {
        parse_source(&program(body)).unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn errors(body: &str) -> String {
        let errs = parse_source(&program(body)).expect_err("translation must fail");
        errs.iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn select(ir: &Ir) -> &SelectOp {
        match &ir.sql_requests[0].op {
            SqlOp::Select(op) => op,
            other => panic!("expected a SELECT, got {other:?}"),
        }
    }

    #[test]
    fn select_single_into_work_area_lowers_with_offsets_and_params() {
        let ir = lower(
            "lv_von = 'LH'.\n\
             SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = @lv_von AND connid = '0400'.",
        );
        assert_eq!(ir.sql_requests.len(), 1);
        let request = &ir.sql_requests[0];
        assert_eq!(request.ir_version, abap_sql_ir::IR_VERSION);
        assert_eq!(request.connection, "DEFAULT");
        let op = select(&ir);
        assert_eq!(op.table, "ZFLUG");
        assert_eq!(op.result, ResultMode::Single);
        // carrid CHAR(3)=6 Byte, connid NUMC(4)=8 Byte, seatsmax INT=4 Byte
        let offsets: Vec<u64> = op.columns.iter().map(|c| c.offset).collect();
        assert_eq!(offsets, vec![0, 6, 14]);
        assert_eq!(op.target_size, 18);
        assert_eq!(op.params.len(), 1);
        assert_eq!(op.params[0].name, "lv_von");
        assert_eq!(
            op.params[0].ty,
            AbapType::Char {
                length: 3,
                dtype: CharType::Char
            }
        );
        assert!(matches!(op.filter, Some(Expr::And { .. })));
        // Der Körper des Programms beginnt in Zeile 10, die SELECT-Anweisung steht in Zeile 11
        assert!(ir.stmts.iter().any(|s| matches!(
            s,
            Stmt::Sql { id: 0, target, line, .. } if target == "ls_flug" && *line == 11
        )));
    }

    #[test]
    fn select_accepts_into_before_where() {
        let ir = lower("SELECT SINGLE carrid FROM zflug INTO lv_von WHERE connid = '0400'.");
        let op = select(&ir);
        assert_eq!(op.columns.len(), 1);
        assert_eq!(op.result, ResultMode::Single);
        assert_eq!(op.target_size, 6);
    }

    #[test]
    fn select_endselect_lowers_to_loop_with_body() {
        let ir = lower(
            "SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH'.\n\
               WRITE ls_flug-connid.\n\
             ENDSELECT.",
        );
        assert_eq!(select(&ir).result, ResultMode::Loop);
        let Stmt::SqlLoop { body, .. } = ir
            .stmts
            .iter()
            .find(|s| matches!(s, Stmt::SqlLoop { .. }))
            .expect("SqlLoop statement")
        else {
            unreachable!()
        };
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn select_into_table_uses_row_size_of_structure() {
        let ir = lower(
            "DATA lt TYPE TABLE OF zflug.\n\
             SELECT * FROM zflug INTO TABLE lt ORDER BY connid DESCENDING UP TO 5 ROWS.",
        );
        let op = select(&ir);
        assert_eq!(op.result, ResultMode::Table);
        assert_eq!(op.target_size, 18);
        assert_eq!(op.up_to, Some(5));
        assert_eq!(
            op.order_by,
            vec![OrderItem {
                column: "connid".into(),
                descending: true
            }]
        );
    }

    #[test]
    fn select_appending_table_sets_append_mode() {
        // Zeilen vom Typ CHAR(3): eine Spalte, Zeilengröße 6 Byte
        let ir = lower(
            "DATA lt TYPE TABLE OF c LENGTH 3.\n\
             SELECT carrid FROM zflug APPENDING TABLE lt WHERE connid = '0400'.",
        );
        assert_eq!(select(&ir).result, ResultMode::Append);
        assert_eq!(select(&ir).target_size, 6);
    }

    #[test]
    fn inline_data_for_single_column_declares_column_type() {
        let ir = lower("SELECT SINGLE connid FROM zflug INTO @DATA(lv_nr) WHERE carrid = 'LH'.");
        let declared = ir
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::Data { name, ty, .. } if name == "lv_nr" => Some(*ty),
                _ => None,
            })
            .expect("lv_nr is declared");
        assert_eq!(declared, crate::lower::Ty::Char(4));
    }

    #[test]
    fn inline_data_for_column_list_declares_anonymous_structure() {
        let ir = lower(
            "SELECT SINGLE carrid, seatsmax FROM zflug INTO @DATA(ls_row) WHERE connid = '0400'.",
        );
        let declared = ir
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::Data { name, ty, .. } if name == "ls_row" => Some(*ty),
                _ => None,
            })
            .expect("ls_row is declared");
        let crate::lower::Ty::Struct(index) = declared else {
            panic!("expected a structure, got {declared:?}");
        };
        assert_eq!(ir.structs[index].fields.len(), 2);
        assert_eq!(ir.structs[index].fields[0].0, "carrid");
    }

    #[test]
    fn inline_data_into_table_declares_table_of_structure() {
        let ir = lower("SELECT * FROM zflug INTO TABLE @DATA(lt_flug).");
        assert!(ir.stmts.iter().any(|s| matches!(
            s,
            Stmt::Data { name, ty: crate::lower::Ty::Table(crate::lower::Elem::Struct(_)), .. }
                if name == "lt_flug"
        )));
        assert_eq!(select(&ir).target_size, 18);
    }

    #[test]
    fn where_accepts_all_comparison_predicates() {
        let ir = lower(
            "SELECT * FROM zflug INTO TABLE @DATA(lt) WHERE \
               seatsmax <> 1 AND seatsmax < 2 AND seatsmax <= 3 AND seatsmax > 4 AND seatsmax >= 5 \
               AND seatsmax BETWEEN 1 AND 9 AND carrid IN ('LH', 'AA') AND NOT carrid LIKE 'X%' \
               OR NOT ( connid = '0400' OR seatsmax = -1 ).",
        );
        let text = format!("{:?}", select(&ir).filter);
        for op in ["Ne", "Lt", "Le", "Gt", "Ge"] {
            assert!(text.contains(op), "{op} missing in {text}");
        }
        assert!(text.contains("Between"), "{text}");
        assert!(text.contains("In {"), "{text}");
        assert!(text.contains("Like"), "{text}");
        assert!(text.contains("Int { value: -1 }"), "{text}");
    }

    #[test]
    fn unknown_column_is_reported_with_table_and_line() {
        let message = errors("SELECT SINGLE xyz FROM zflug INTO lv_von.");
        assert!(
            message.contains("unknown column 'xyz' in table ZFLUG"),
            "{message}"
        );
    }

    #[test]
    fn unknown_table_is_reported_without_ddic_information() {
        let message = errors("SELECT SINGLE * FROM zunbekannt INTO ls_flug.");
        assert!(
            message.contains("unknown table ZUNBEKANNT (no DDIC information)"),
            "{message}"
        );
    }

    #[test]
    fn literal_type_mismatch_in_where_is_rejected() {
        let message = errors("SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 5.");
        assert!(
            message.contains(
                "type mismatch in WHERE: column carrid (CHAR) compared with numeric value"
            ),
            "{message}"
        );
    }

    #[test]
    fn numc_column_compared_with_numeric_literal_is_rejected() {
        // Annahme: NUMC ist eine Zeichenart; ein numerisches Literal ist kein zulässiger Vergleich.
        let message = errors("SELECT SINGLE * FROM zflug INTO ls_flug WHERE connid = 400.");
        assert!(
            message.contains(
                "type mismatch in WHERE: column connid (NUMC) compared with numeric value"
            ),
            "{message}"
        );
    }

    #[test]
    fn int_column_compared_with_character_host_variable_is_rejected() {
        let message = errors("SELECT SINGLE * FROM zflug INTO ls_flug WHERE seatsmax = @lv_von.");
        assert!(
            message.contains(
                "type mismatch in WHERE: column seatsmax (INT) compared with character value"
            ),
            "{message}"
        );
    }

    #[test]
    fn projection_char_to_int_is_a_conversion_error() {
        let message = errors("SELECT SINGLE carrid FROM zflug INTO lv_max.");
        assert!(
            message.contains("conversion from CHAR to INT not supported"),
            "{message}"
        );
    }

    #[test]
    fn structure_target_needs_matching_component_count() {
        let message = errors("SELECT SINGLE carrid FROM zflug INTO ls_flug WHERE connid = '0400'.");
        assert!(
            message.contains("SELECT list has 1 columns but the target structure has 3 components"),
            "{message}"
        );
    }

    #[test]
    fn unsupported_keywords_are_rejected_by_name() {
        let cases = [
            ("SELECT * FROM zflug AS a INTO ls_flug.", "alias (AS)"),
            ("SELECT * FROM zflug INNER JOIN zflug INTO ls_flug.", "JOIN"),
            ("SELECT DISTINCT carrid FROM zflug INTO lv_von.", "DISTINCT"),
            ("SELECT * FROM zflug INTO ls_flug OFFSET 2.", "OFFSET"),
            (
                "SELECT COUNT( * ) FROM zflug INTO lv_max.",
                "aggregate COUNT",
            ),
            (
                "SELECT carrid FROM zflug INTO lv_von GROUP BY carrid.",
                "GROUP BY",
            ),
            (
                "SELECT * FROM zflug INTO ls_flug FOR ALL ENTRIES IN lt WHERE carrid = 'LH'.",
                "FOR ALL ENTRIES",
            ),
            (
                "SELECT * FROM zflug INTO ls_flug WHERE carrid IN (SELECT carrid FROM zflug).",
                "subquery",
            ),
            (
                "SELECT * FROM zflug INTO ls_flug WHERE connid IS NULL.",
                "IS NULL",
            ),
            (
                "SELECT * FROM zflug CONNECTION abc INTO ls_flug.",
                "CONNECTION",
            ),
            ("SELECT * FROM zflug~x INTO ls_flug.", "table alias (~)"),
        ];
        for (source, keyword) in cases {
            let message = errors(&source.replace("\n", " "));
            assert!(
                message.contains(&format!("SELECT with {keyword} is not supported")),
                "{source}: {message}"
            );
        }
    }

    #[test]
    fn string_column_is_rejected() {
        let message = parse_source(
            "TYPES: BEGIN OF zt,\n  name TYPE string,\nEND OF zt.\n\
             DATA ls TYPE zt.\n\
             START-OF-SELECTION.\n\
             SELECT * FROM zt INTO ls.",
        )
        .expect_err("STRING column must be rejected")
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
        assert!(
            message.contains("column name of type STRING is not supported"),
            "{message}"
        );
    }

    #[test]
    fn host_variables_are_numbered_in_first_use_order() {
        let ir = lower(
            "SELECT * FROM zflug INTO ls_flug WHERE seatsmax > @lv_max AND carrid = @lv_von AND seatsmax < @lv_max.",
        );
        let names: Vec<&str> = select(&ir).params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["lv_max", "lv_von"]);
    }

    #[test]
    fn form_parameter_is_a_host_variable_with_its_declared_type() {
        let ir = lower_with_form(
            "FORM find USING p TYPE i.\n\
               SELECT SINGLE * FROM zflug INTO ls_flug WHERE seatsmax = @p.\n\
             ENDFORM.\n\
             START-OF-SELECTION.\n\
               PERFORM find USING lv_max.\n",
        );
        let op = select(&ir);
        assert_eq!(op.params.len(), 1);
        assert_eq!(op.params[0].name, "p");
        assert_eq!(op.params[0].ty, AbapType::Int);
    }

    fn lower_with_form(src: &str) -> Ir {
        parse_source(&format!("{FLUG}{src}")).unwrap_or_else(|e| panic!("{e:?}"))
    }

    #[test]
    fn lexer_splits_host_markers_and_operators_but_keeps_hyphenated_names() {
        use crate::inspect::abap::tokenize;
        let toks = tokenize(
            "WHERE a <> @x AND b <= :y AND c >= 'q' AND d = `r` AND zflug-carrid = <fs>-x",
        );
        let texts: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
        for expected in [
            "<>",
            "<=",
            ">=",
            "@",
            ":",
            "'q'",
            "`r`",
            "zflug-carrid",
            "<fs>-x",
        ] {
            assert!(texts.contains(&expected), "{expected} missing in {texts:?}");
        }
        // Zwei Zeichen, die nicht zusammengehören, bleiben getrennt
        assert!(!texts.contains(&"<>@"), "{texts:?}");
    }

    /// Programm mit zusätzlichen Typen und Variablen nach dem FLUG-Kopf. Liefert die Fehlermeldungen als Text.
    fn errors_with_types(types: &str, body: &str) -> String {
        let src = format!("{FLUG}{types}START-OF-SELECTION.\n{body}\n");
        parse_source(&src)
            .expect_err("translation must fail")
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Programm mit zusätzlichen Typen und Variablen; liefert die IR.
    fn lower_with_types(types: &str, body: &str) -> Ir {
        let src = format!("{FLUG}{types}START-OF-SELECTION.\n{body}\n");
        parse_source(&src).unwrap_or_else(|e| panic!("{e:?}"))
    }

    /// Typ mit einer Zeile wie `zflug`, aber mit anderer Reihenfolge der Komponenten (für CORRESPONDING).
    const ROW: &str = "TYPES: BEGIN OF zrow,\n\
          seatsmax TYPE i,\n\
          carrid TYPE c LENGTH 3,\n\
        END OF zrow.\n\
        DATA ls_row TYPE zrow.\n\
        DATA lt_row TYPE TABLE OF zrow.\n";

    /// Die Anfrage an Stelle `index` als DML-Operation.
    fn write_op(ir: &Ir, index: usize) -> &abap_sql_ir::SqlOp {
        &ir.sql_requests[index].op
    }

    fn first_write(ir: &Ir) -> (&[String], &str) {
        ir.stmts
            .iter()
            .find_map(|s| match s {
                Stmt::SqlWrite { params, target, .. } => Some((params.as_slice(), target.as_str())),
                _ => None,
            })
            .expect("SqlWrite statement")
    }

    #[test]
    fn insert_into_values_lowers_to_single_row_source_with_column_offsets() {
        let ir = lower("INSERT INTO zflug VALUES ls_flug.");
        let abap_sql_ir::SqlOp::Insert(op) = write_op(&ir, 0) else {
            panic!("expected INSERT, got {:?}", write_op(&ir, 0));
        };
        assert_eq!(op.table, "ZFLUG");
        assert_eq!(op.rows.mode, abap_sql_ir::RowMode::Single);
        assert!(!op.accepting_duplicate_keys);
        let columns: Vec<&str> = op.rows.fields.iter().map(|f| f.column.as_str()).collect();
        assert_eq!(columns, vec!["carrid", "connid", "seatsmax"]);
        // carrid CHAR(3)=6 Byte, connid NUMC(4)=8 Byte, seatsmax INT=4 Byte
        let offsets: Vec<u64> = op.rows.fields.iter().map(|f| f.offset).collect();
        assert_eq!(offsets, vec![0, 6, 14]);
        assert_eq!(op.rows.size, 18);
        assert_eq!(first_write(&ir), (&[][..], "ls_flug"));
    }

    #[test]
    fn insert_with_host_marker_and_insert_from_work_area_are_the_same_form() {
        let at = lower("INSERT INTO zflug VALUES @ls_flug.");
        let from = lower("INSERT zflug FROM ls_flug.");
        let colon = lower("INSERT INTO zflug VALUES :ls_flug.");
        assert_eq!(write_op(&at, 0), write_op(&from, 0));
        assert_eq!(write_op(&colon, 0), write_op(&from, 0));
        assert_eq!(first_write(&from), (&[][..], "ls_flug"));
    }

    #[test]
    fn insert_from_table_uses_row_size_and_accepting_flag() {
        let ir = lower(
            "DATA lt TYPE TABLE OF zflug.\n\
             INSERT zflug FROM TABLE lt ACCEPTING DUPLICATE KEYS.",
        );
        let abap_sql_ir::SqlOp::Insert(op) = write_op(&ir, 0) else {
            panic!("expected INSERT");
        };
        assert_eq!(op.rows.mode, abap_sql_ir::RowMode::Table);
        assert!(op.accepting_duplicate_keys);
        assert_eq!(op.rows.size, 18);
        assert_eq!(first_write(&ir), (&[][..], "lt"));
    }

    #[test]
    fn insert_from_table_without_accepting_keeps_the_flag_false() {
        let ir = lower("DATA lt TYPE TABLE OF zflug.\nINSERT zflug FROM TABLE lt.");
        let abap_sql_ir::SqlOp::Insert(op) = write_op(&ir, 0) else {
            panic!("expected INSERT");
        };
        assert!(!op.accepting_duplicate_keys);
    }

    #[test]
    fn insert_with_column_list_is_rejected() {
        let message = errors("INSERT INTO zflug (carrid) VALUES ls_flug.");
        assert!(
            message.contains("INSERT with column list is not supported"),
            "{message}"
        );
    }

    #[test]
    fn insert_values_without_into_is_rejected() {
        let message = errors("INSERT zflug VALUES ls_flug.");
        assert!(message.contains("INSERT VALUES needs INTO"), "{message}");
    }

    #[test]
    fn insert_accepting_without_table_is_rejected() {
        let message = errors("INSERT zflug FROM ls_flug ACCEPTING DUPLICATE KEYS.");
        assert!(
            message.contains("ACCEPTING DUPLICATE KEYS needs FROM TABLE"),
            "{message}"
        );
    }

    #[test]
    fn insert_structure_with_different_component_count_is_rejected() {
        let message = errors_with_types(
            "TYPES: BEGIN OF zshort,\n  carrid TYPE c LENGTH 3,\nEND OF zshort.\n\
             DATA ls_short TYPE zshort.\n",
            "INSERT zflug FROM ls_short.",
        );
        assert!(
            message.contains(
                "structure does not match table ZFLUG: 1 components, table has 3 columns"
            ),
            "{message}"
        );
    }

    #[test]
    fn insert_structure_with_incompatible_component_type_is_rejected() {
        // Die zweite Komponente ist INT, die Spalte connid ist NUMC (Zeichenart)
        let message = errors_with_types(
            "TYPES: BEGIN OF zbad,\n  carrid TYPE c LENGTH 3,\n  connid TYPE i,\n  seatsmax TYPE i,\nEND OF zbad.\n\
             DATA ls_bad TYPE zbad.\n",
            "INSERT zflug FROM ls_bad.",
        );
        assert!(
            message.contains(
                "structure does not match table ZFLUG: column connid needs NUMC, component has INT"
            ),
            "{message}"
        );
    }

    #[test]
    fn insert_into_unknown_table_is_reported() {
        let message = errors("INSERT INTO zunbekannt VALUES ls_flug.");
        assert!(
            message.contains("unknown table ZUNBEKANNT (no DDIC information)"),
            "{message}"
        );
    }

    #[test]
    fn insert_into_local_table_name_stays_the_internal_form() {
        let ir = lower("DATA lt TYPE TABLE OF zflug.\nINSERT ls_flug INTO TABLE lt.");
        assert!(ir.sql_requests.is_empty());
        assert!(ir.stmts.iter().any(|s| matches!(
            s,
            Stmt::ExternalOperation { name, .. } if name == "insert"
        )));
    }

    #[test]
    fn update_set_with_where_numbers_host_variables_in_order_of_appearance() {
        let ir = lower(
            "lv_von = 'LH'.\n\
             UPDATE zflug SET seatsmax = @lv_max WHERE carrid = @lv_von AND seatsmax > @lv_max.",
        );
        let abap_sql_ir::SqlOp::Update(op) = write_op(&ir, 0) else {
            panic!("expected UPDATE");
        };
        assert_eq!(op.table, "ZFLUG");
        assert_eq!(op.assignments.len(), 1);
        assert_eq!(op.assignments[0].column, "seatsmax");
        assert_eq!(
            op.assignments[0].value,
            abap_sql_ir::Expr::Param { index: 0 }
        );
        assert!(op.filter.is_some());
        assert!(op.rows.is_none());
        let names: Vec<&str> = op.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["lv_max", "lv_von"]);
        assert_eq!(
            first_write(&ir),
            (&["lv_max".to_string(), "lv_von".to_string()][..], "")
        );
    }

    #[test]
    fn update_set_with_literal_value_and_without_where_is_a_set_change() {
        let ir = lower("UPDATE zflug SET seatsmax = 5.");
        let abap_sql_ir::SqlOp::Update(op) = write_op(&ir, 0) else {
            panic!("expected UPDATE");
        };
        assert_eq!(
            op.assignments[0].value,
            abap_sql_ir::Expr::Literal {
                value: abap_sql_ir::Value::Int { value: 5 }
            }
        );
        assert!(op.filter.is_none());
        assert!(op.params.is_empty());
    }

    #[test]
    fn update_from_work_area_is_a_single_row_source_without_assignments() {
        let ir = lower("UPDATE zflug FROM ls_flug.");
        let abap_sql_ir::SqlOp::Update(op) = write_op(&ir, 0) else {
            panic!("expected UPDATE");
        };
        assert!(op.assignments.is_empty());
        assert!(op.filter.is_none());
        let rows = op.rows.as_ref().expect("row source");
        assert_eq!(rows.mode, abap_sql_ir::RowMode::Single);
        assert_eq!(rows.fields.len(), 3);
        assert_eq!(first_write(&ir), (&[][..], "ls_flug"));
    }

    #[test]
    fn update_set_with_type_mismatch_is_rejected() {
        let message = errors("UPDATE zflug SET carrid = @lv_max.");
        assert!(
            message.contains("conversion from INT to CHAR not supported"),
            "{message}"
        );
    }

    #[test]
    fn update_set_with_unknown_column_is_rejected() {
        let message = errors("UPDATE zflug SET xyz = 1.");
        assert!(
            message.contains("unknown column 'xyz' in table ZFLUG"),
            "{message}"
        );
    }

    #[test]
    fn update_set_value_that_is_a_column_is_rejected() {
        let message = errors("UPDATE zflug SET seatsmax = connid.");
        assert!(
            message.contains("SET value must be a host variable or a literal"),
            "{message}"
        );
    }

    #[test]
    fn update_set_host_variable_without_marker_is_rejected() {
        let message = errors("UPDATE zflug SET seatsmax = lv_max.");
        assert!(
            message.contains("host variable 'lv_max' in SET needs @ or :"),
            "{message}"
        );
    }

    #[test]
    fn update_with_other_form_is_rejected() {
        let message = errors("UPDATE zflug WHERE carrid = 'LH'.");
        assert!(message.contains("UPDATE expects SET or FROM"), "{message}");
    }

    #[test]
    fn delete_from_with_where_has_filter_and_no_row_source() {
        let ir = lower("DELETE FROM zflug WHERE carrid = @lv_von.");
        let abap_sql_ir::SqlOp::Delete(op) = write_op(&ir, 0) else {
            panic!("expected DELETE");
        };
        assert_eq!(op.table, "ZFLUG");
        assert!(op.filter.is_some());
        assert!(op.rows.is_none());
        assert_eq!(op.params.len(), 1);
        assert_eq!(first_write(&ir), (&["lv_von".to_string()][..], ""));
    }

    #[test]
    fn delete_from_without_where_deletes_all_rows() {
        let ir = lower("DELETE FROM zflug.");
        let abap_sql_ir::SqlOp::Delete(op) = write_op(&ir, 0) else {
            panic!("expected DELETE");
        };
        assert!(op.filter.is_none());
        assert!(op.rows.is_none());
    }

    #[test]
    fn delete_from_work_area_and_from_table_use_row_sources() {
        let key = lower("DELETE zflug FROM ls_flug.");
        let abap_sql_ir::SqlOp::Delete(op) = write_op(&key, 0) else {
            panic!("expected DELETE");
        };
        assert_eq!(
            op.rows.as_ref().map(|r| r.mode),
            Some(abap_sql_ir::RowMode::Single)
        );
        assert_eq!(first_write(&key), (&[][..], "ls_flug"));

        let table = lower("DATA lt TYPE TABLE OF zflug.\nDELETE zflug FROM TABLE lt.");
        let abap_sql_ir::SqlOp::Delete(op) = write_op(&table, 0) else {
            panic!("expected DELETE");
        };
        assert_eq!(
            op.rows.as_ref().map(|r| r.mode),
            Some(abap_sql_ir::RowMode::Table)
        );
        assert_eq!(first_write(&table), (&[][..], "lt"));
    }

    #[test]
    fn delete_with_local_table_name_stays_the_internal_form() {
        let ir = lower("DATA lt TYPE TABLE OF zflug.\nDELETE lt WHERE carrid = 'LH'.");
        assert!(ir.sql_requests.is_empty());
        assert!(ir.stmts.iter().any(|s| matches!(
            s,
            Stmt::ExternalOperation { name, .. } if name == "delete_table_range"
        )));
    }

    #[test]
    fn delete_from_table_keyword_is_rejected() {
        let message = errors("DELETE FROM TABLE lt.");
        assert!(
            message.contains("DELETE FROM TABLE is not supported"),
            "{message}"
        );
    }

    #[test]
    fn modify_from_work_area_and_from_table_are_database_forms() {
        let single = lower("MODIFY zflug FROM ls_flug.");
        let abap_sql_ir::SqlOp::Modify(op) = write_op(&single, 0) else {
            panic!("expected MODIFY");
        };
        assert_eq!(op.rows.mode, abap_sql_ir::RowMode::Single);
        assert_eq!(first_write(&single), (&[][..], "ls_flug"));

        let table = lower("DATA lt TYPE TABLE OF zflug.\nMODIFY zflug FROM TABLE lt.");
        let abap_sql_ir::SqlOp::Modify(op) = write_op(&table, 0) else {
            panic!("expected MODIFY");
        };
        assert_eq!(op.rows.mode, abap_sql_ir::RowMode::Table);
    }

    #[test]
    fn modify_internal_form_with_index_stays_external() {
        let ir = lower("DATA lt TYPE TABLE OF zflug.\nMODIFY lt INDEX 1 FROM ls_flug.");
        assert!(ir.sql_requests.is_empty());
        assert!(ir.stmts.iter().any(|s| matches!(
            s,
            Stmt::ExternalOperation { name, .. } if name == "modify"
        )));
    }

    #[test]
    fn commit_and_rollback_work_lower_to_transaction_statements() {
        let ir = lower("COMMIT WORK.\nCOMMIT WORK AND WAIT.\nROLLBACK WORK.");
        let flags: Vec<bool> = ir
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::SqlCommit { rollback, .. } => Some(*rollback),
                _ => None,
            })
            .collect();
        assert_eq!(flags, vec![false, false, true]);
        assert!(ir.sql_requests.is_empty());
    }

    #[test]
    fn other_commit_forms_stay_external() {
        let ir = lower("COMMIT CONNECTION abc.");
        assert!(ir.stmts.iter().any(|s| matches!(
            s,
            Stmt::ExternalOperation { name, .. } if name == "commit"
        )));
    }

    #[test]
    fn escape_clause_is_rejected_in_select_and_in_write_statements() {
        for body in [
            "SELECT * FROM zflug INTO ls_flug WHERE carrid LIKE 'A#%' ESCAPE '#'.",
            "DELETE FROM zflug WHERE carrid LIKE 'A#%' ESCAPE '#'.",
        ] {
            let message = errors(body);
            assert!(
                message.contains("ESCAPE not supported"),
                "{body}: {message}"
            );
        }
    }

    #[test]
    fn corresponding_fields_maps_columns_by_name_and_keeps_other_components() {
        let ir = lower_with_types(
            ROW,
            "SELECT SINGLE * FROM zflug INTO CORRESPONDING FIELDS OF ls_row.",
        );
        let op = select(&ir);
        assert_eq!(op.result, ResultMode::Single);
        // zrow: seatsmax INT (Offset 0), carrid CHAR(3) (Offset 4); connid hat keine Komponente
        let pairs: Vec<(&str, u64)> = op
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c.offset))
            .collect();
        assert_eq!(pairs, vec![("carrid", 4), ("seatsmax", 0)]);
        assert_eq!(op.target_size, 10);
    }

    #[test]
    fn corresponding_fields_into_table_maps_by_name() {
        let ir = lower_with_types(
            ROW,
            "SELECT * FROM zflug INTO CORRESPONDING FIELDS OF TABLE lt_row.",
        );
        let op = select(&ir);
        assert_eq!(op.result, ResultMode::Table);
        assert_eq!(op.columns.len(), 2);
    }

    #[test]
    fn corresponding_fields_into_declared_variable_is_rejected() {
        let message = errors("SELECT SINGLE * FROM zflug INTO CORRESPONDING FIELDS OF @DATA(ls).");
        assert!(
            message.contains("CORRESPONDING FIELDS OF needs an existing target, not @DATA"),
            "{message}"
        );
    }

    #[test]
    fn corresponding_fields_into_scalar_is_rejected() {
        let message =
            errors("SELECT SINGLE carrid FROM zflug INTO CORRESPONDING FIELDS OF lv_von.");
        assert!(
            message.contains("CORRESPONDING FIELDS OF needs a structure as target"),
            "{message}"
        );
    }

    #[test]
    fn corresponding_fields_without_any_matching_name_is_not_an_error() {
        let ir = lower_with_types(
            "TYPES: BEGIN OF zother,\n  xyz TYPE i,\nEND OF zother.\nDATA ls_other TYPE zother.\n",
            "SELECT SINGLE carrid FROM zflug INTO CORRESPONDING FIELDS OF ls_other.",
        );
        assert!(select(&ir).columns.is_empty());
    }

    #[test]
    fn select_into_structure_without_corresponding_needs_matching_component_count() {
        // Ohne CORRESPONDING gilt die Zuordnung nach Position: 3 Spalten, 2 Komponenten
        let message = errors_with_types(ROW, "SELECT * FROM zflug INTO ls_row.");
        assert!(
            message.contains("SELECT list has 3 columns but the target structure has 2 components"),
            "{message}"
        );
    }
}
