//! Leitet ein best-effort Modell mit SQL-, DDIC- und Objektabhängigkeiten ab.
//! Mandantenfelder `MANDT` und `CLIENT` werden bei Feldern und Beziehungen ausgelassen.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Serialize, Serializer};

use crate::inspect::abap::{parse_l1, Item, Kind, Program, Statement, Token, TopItem};

#[derive(Debug, Default, Serialize)]
pub struct DataModel {
    pub statements: Vec<DatabaseStatement>,
    pub tables: Vec<TableModel>,
    #[serde(serialize_with = "serialize_relationships")]
    pub relationships: Vec<Relationship>,
    pub structures: Vec<StructureModel>,
    #[serde(serialize_with = "serialize_dependencies")]
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Serialize)]
pub struct DatabaseStatement {
    pub file: String,
    pub line: usize,
    pub operation: String,
    pub tables: Vec<String>,
    pub fields: Vec<String>,
    pub target: Option<String>,
    pub target_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TableModel {
    pub name: String,
    pub operations: Vec<String>,
    pub fields: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Relationship {
    pub left_table: String,
    pub left_field: String,
    pub right_table: String,
    pub right_field: String,
    pub source: &'static str,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Serialize)]
struct RelationshipGroup<'a> {
    left_table: &'a str,
    right_table: &'a str,
    fields: Vec<RelationshipField<'a>>,
}

#[derive(Serialize)]
struct RelationshipField<'a> {
    left: &'a str,
    right: &'a str,
    sources: Vec<RelationshipSource<'a>>,
}

#[derive(Serialize)]
struct RelationshipSource<'a> {
    source: &'a str,
    file: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
}

fn serialize_relationships<S>(
    relationships: &[Relationship],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    type FieldPairs<'a> = BTreeMap<(&'a str, &'a str), BTreeSet<(&'a str, &'a str, Option<usize>)>>;
    let mut groups: BTreeMap<(&str, &str), FieldPairs<'_>> = BTreeMap::new();
    for r in relationships {
        groups
            .entry((&r.left_table, &r.right_table))
            .or_default()
            .entry((&r.left_field, &r.right_field))
            .or_default()
            .insert((r.source, &r.file, r.line));
    }
    let groups: Vec<_> = groups
        .into_iter()
        .map(|((left_table, right_table), fields)| RelationshipGroup {
            left_table,
            right_table,
            fields: fields
                .into_iter()
                .map(|((left, right), sources)| RelationshipField {
                    left,
                    right,
                    sources: sources
                        .into_iter()
                        .map(|(source, file, line)| RelationshipSource { source, file, line })
                        .collect(),
                })
                .collect(),
        })
        .collect();
    groups.serialize(serializer)
}

#[derive(Debug, Serialize)]
pub struct StructureModel {
    pub name: String,
    pub fields: Vec<StructureField>,
}

#[derive(Debug, Serialize)]
pub struct StructureField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct Dependency {
    pub source_type: String,
    pub source: String,
    pub target_type: Option<String>,
    pub target: String,
    pub kind: String,
    pub in_zip: bool,
    pub file: String,
    pub line: Option<usize>,
}

#[derive(Serialize)]
struct DependencyGroup<'a> {
    source_type: &'a str,
    source: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_type: Option<&'a str>,
    target: &'a str,
    kind: &'a str,
    in_zip: bool,
    references: Vec<DependencyReference<'a>>,
}

#[derive(Serialize)]
struct DependencyReference<'a> {
    file: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
}

fn serialize_dependencies<S>(dependencies: &[Dependency], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    type References<'a> = BTreeSet<(&'a str, Option<usize>)>;
    let mut groups: BTreeMap<(&str, &str, Option<&str>, &str, &str), (bool, References<'_>)> =
        BTreeMap::new();
    for dependency in dependencies {
        groups
            .entry((
                &dependency.source_type,
                &dependency.source,
                dependency.target_type.as_deref(),
                &dependency.target,
                &dependency.kind,
            ))
            .or_insert_with(|| (dependency.in_zip, BTreeSet::new()))
            .1
            .insert((&dependency.file, dependency.line));
    }
    let groups: Vec<_> = groups
        .into_iter()
        .map(
            |((source_type, source, target_type, target, kind), (in_zip, references))| {
                DependencyGroup {
                    source_type,
                    source,
                    target_type,
                    target,
                    kind,
                    in_zip,
                    references: references
                        .into_iter()
                        .map(|(file, line)| DependencyReference { file, line })
                        .collect(),
                }
            },
        )
        .collect();
    groups.serialize(serializer)
}

#[derive(Default)]
struct TableUsage {
    operations: BTreeSet<String>,
    fields: BTreeSet<String>,
}

fn collect_items(items: &[Item], out: &mut Vec<Statement>) {
    for item in items {
        match item {
            Item::Statement(s) => out.push(s.clone()),
            Item::Block { head, body } => {
                out.push(head.clone());
                collect_items(body, out);
            }
        }
    }
}

fn collect_program(prog: &Program) -> Vec<Statement> {
    let mut out = Vec::new();
    for item in &prog.items {
        match item {
            TopItem::Statement(s) => out.push(s.clone()),
            TopItem::Block { head, body } => {
                out.push(head.clone());
                collect_items(body, &mut out);
            }
            TopItem::Routine { head, body, .. } => {
                out.push(head.clone());
                collect_items(body, &mut out);
            }
            TopItem::Class(class) => {
                out.push(class.head.clone());
                for member in &class.members {
                    out.push(member.statement.clone());
                }
                for method in &class.methods {
                    out.push(method.head.clone());
                    collect_items(&method.body, &mut out);
                }
                collect_items(&class.other, &mut out);
            }
        }
    }
    out
}

fn word(s: &Statement, i: usize) -> Option<&str> {
    s.tokens
        .get(i)
        .filter(|t| t.kind == Kind::Word)
        .map(|t| t.text.as_str())
}

fn upper(t: &Token) -> String {
    t.text.to_uppercase()
}

fn is_client_field(name: &str) -> bool {
    name.eq_ignore_ascii_case("MANDT") || name.eq_ignore_ascii_case("CLIENT")
}

fn parse_structure_info(
    statements: &[Statement],
) -> (
    BTreeMap<String, String>,
    BTreeMap<String, Vec<StructureField>>,
) {
    let mut variable_types = BTreeMap::new();
    let mut structures = BTreeMap::new();
    let mut current: Option<(String, Vec<StructureField>)> = None;

    for s in statements {
        let kw = s.keyword();
        let Some(first) = word(s, 1) else {
            continue;
        };
        let first_upper = first.to_uppercase();

        if kw == "TYPES"
            && first_upper == "BEGIN"
            && word(s, 2).is_some_and(|w| w.eq_ignore_ascii_case("OF"))
        {
            if let Some(name) = word(s, 3) {
                current = Some((name.to_lowercase(), Vec::new()));
            }
            continue;
        }
        if kw == "TYPES" && first_upper == "END" {
            if let Some((name, fields)) = current.take() {
                structures.insert(name, fields);
            }
            continue;
        }

        if let Some((_, fields)) = current.as_mut() {
            if kw == "TYPES" && first_upper != "BEGIN" && first_upper != "END" {
                let name = first.to_lowercase();
                if let Some(type_idx) = s
                    .tokens
                    .iter()
                    .position(|t| t.is_word("TYPE") || t.is_word("LIKE"))
                {
                    if let Some(type_name) =
                        declared_type(s, type_idx).filter(|_| !is_client_field(&name))
                    {
                        fields.push(StructureField {
                            name,
                            type_name: type_name.to_string(),
                        });
                    }
                }
            }
            continue;
        }

        if matches!(kw.as_str(), "DATA" | "CLASS-DATA" | "STATICS") {
            if let Some(type_idx) = s
                .tokens
                .iter()
                .position(|t| t.is_word("TYPE") || t.is_word("LIKE"))
            {
                if let Some(type_name) = declared_type(s, type_idx) {
                    variable_types.insert(first.to_lowercase(), type_name.to_lowercase());
                }
            }
        }
    }
    (variable_types, structures)
}

fn declared_type(s: &Statement, type_idx: usize) -> Option<&str> {
    let mut i = type_idx + 1;
    while word(s, i).is_some_and(|w| {
        matches!(
            w.to_uppercase().as_str(),
            "STANDARD" | "SORTED" | "HASHED" | "TABLE" | "OF" | "REF" | "TO"
        )
    }) {
        i += 1;
    }
    word(s, i)
}

fn operation(s: &Statement) -> Option<&'static str> {
    match s.keyword().as_str() {
        "SELECT" => Some("SELECT"),
        "INSERT"
            if !["VALUE", "INITIAL", "LINES", "TABLE"]
                .iter()
                .any(|kw| word(s, 1).is_some_and(|w| w.eq_ignore_ascii_case(kw)))
                && !s
                    .tokens
                    .iter()
                    .any(|t| t.is_word("INTO") || t.is_word("INDEX")) =>
        {
            Some("INSERT")
        }
        "UPDATE" => Some("UPDATE"),
        "MODIFY"
            if s.tokens.iter().any(|t| t.is_word("FROM"))
                && !s.tokens.iter().any(|t| t.is_word("INDEX"))
                && !["TABLE", "SCREEN", "EXTRACT", "TOTAL"]
                    .iter()
                    .any(|kw| word(s, 1).is_some_and(|w| w.eq_ignore_ascii_case(kw))) =>
        {
            Some("MODIFY")
        }
        "DELETE"
            if !["ADJACENT", "TABLE", "EXTRACT", "TOTAL"]
                .iter()
                .any(|kw| word(s, 1).is_some_and(|w| w.eq_ignore_ascii_case(kw)))
                && !s.tokens.iter().any(|t| t.is_word("INDEX"))
                && (s.tokens.iter().any(|t| t.is_word("FROM"))
                    || !s.tokens.iter().any(|t| t.is_word("WHERE"))
                        && !word(s, 1).is_some_and(|w| {
                            ["GT_", "LT_", "RT_", "GS_", "LS_", "ME->", "<"]
                                .iter()
                                .any(|prefix| w.to_uppercase().starts_with(prefix))
                        })) =>
        {
            Some("DELETE")
        }
        _ => None,
    }
}

fn is_clause(t: &Token) -> bool {
    t.kind == Kind::Word
        && matches!(
            upper(t).as_str(),
            "WHERE"
                | "GROUP"
                | "HAVING"
                | "ORDER"
                | "INTO"
                | "APPENDING"
                | "UP"
                | "BYPASSING"
                | "CONNECTION"
                | "CLIENT"
                | "FOR"
                | "USING"
                | "FIELDS"
                | "SET"
                | "VALUES"
                | "FROM"
                | "JOIN"
                | "INNER"
                | "LEFT"
                | "RIGHT"
                | "CROSS"
                | "OUTER"
                | "ON"
        )
}

fn source_clause_end(s: &Statement, start: usize) -> usize {
    (start..s.tokens.len())
        .find(|&i| {
            if i > 0 && s.tokens[i - 1].is_punct("~") {
                return false;
            }
            [
                "WHERE",
                "GROUP",
                "HAVING",
                "ORDER",
                "INTO",
                "APPENDING",
                "UP",
                "BYPASSING",
                "CONNECTION",
                "CLIENT",
                "FOR",
            ]
            .iter()
            .any(|kw| s.tokens[i].is_word(kw))
        })
        .unwrap_or(s.tokens.len())
}

fn table_at(s: &Statement, idx: usize) -> Option<(String, usize)> {
    let mut i = idx;
    while s.tokens.get(i).is_some_and(|t| t.is_punct("@")) {
        i += 1;
    }
    let token = s.tokens.get(i)?;
    if token.kind != Kind::Word || is_clause(token) {
        return None;
    }
    // Dynamic table names and parenthesized expressions cannot be resolved statically.
    if token.text.starts_with('<') || token.text.starts_with('(') {
        return None;
    }
    Some((token.text.to_uppercase(), i + 1))
}

fn table_references(s: &Statement, op: &str) -> (Vec<String>, BTreeMap<String, String>) {
    let mut tables = Vec::new();
    let mut aliases = BTreeMap::new();
    match op {
        "SELECT" => {
            if let Some(from) = s.tokens.iter().position(|t| t.is_word("FROM")) {
                let end = source_clause_end(s, from + 1);
                let mut i = from + 1;
                let mut expect_table = true;
                let mut in_join_condition = false;
                while i < end {
                    if s.tokens[i].is_word("ON") {
                        in_join_condition = true;
                        expect_table = false;
                        i += 1;
                        continue;
                    }
                    if s.tokens[i].is_word("JOIN") {
                        in_join_condition = false;
                        expect_table = true;
                        i += 1;
                        continue;
                    }
                    if s.tokens[i].is_punct(",") && !in_join_condition {
                        expect_table = true;
                        i += 1;
                        continue;
                    }
                    if expect_table {
                        if let Some((table, next)) = table_at(s, i) {
                            let mut after = next;
                            if s.tokens.get(after).is_some_and(|t| t.is_word("AS")) {
                                after += 1;
                            }
                            if let Some(alias) =
                                word(s, after).filter(|_| !is_clause(&s.tokens[after]))
                            {
                                aliases.insert(alias.to_lowercase(), table.clone());
                                after += 1;
                            }
                            aliases.insert(table.to_lowercase(), table.clone());
                            tables.push(table);
                            expect_table = false;
                            i = after;
                            continue;
                        }
                        expect_table = false;
                    }
                    i += 1;
                }
            }
        }
        "INSERT" => {
            if let Some((table, _)) = table_at(s, 1) {
                tables.push(table);
            }
        }
        "UPDATE" | "MODIFY" => {
            if let Some((table, _)) = table_at(s, 1) {
                tables.push(table);
            }
        }
        "DELETE" => {
            let idx = if s.tokens.get(1).is_some_and(|t| t.is_word("FROM")) {
                2
            } else {
                1
            };
            if let Some((table, _)) = table_at(s, idx) {
                tables.push(table);
            }
        }
        _ => {}
    }
    tables.sort();
    tables.dedup();
    (tables, aliases)
}

fn qualified_ref(tokens: &[Token], idx: usize) -> Option<(String, String, usize)> {
    let left = tokens.get(idx)?;
    if left.kind != Kind::Word {
        return None;
    }
    if tokens.get(idx + 1).is_some_and(|t| t.is_punct("~")) {
        let field = tokens.get(idx + 2)?;
        if field.kind == Kind::Word {
            return Some((left.text.to_lowercase(), field.text.to_uppercase(), idx + 3));
        }
    }
    None
}

fn table_for_ref(
    name: &str,
    aliases: &BTreeMap<String, String>,
    tables: &[String],
) -> Option<String> {
    aliases.get(&name.to_lowercase()).cloned().or_else(|| {
        tables
            .iter()
            .find(|t| t.eq_ignore_ascii_case(name))
            .cloned()
    })
}

fn selected_fields(
    s: &Statement,
    op: &str,
    tables: &[String],
    aliases: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let start_end = if op == "SELECT" {
        let from = s.tokens.iter().position(|t| t.is_word("FROM"));
        let fields = s.tokens.iter().position(|t| t.is_word("FIELDS"));
        match (from, fields) {
            (Some(from), Some(fields)) if fields > from => {
                let end = source_clause_end(s, fields + 1);
                Some((fields + 1, end))
            }
            (Some(from), _) if from > 1 => {
                let end = source_clause_end(s, 1).min(from);
                Some((1, end))
            }
            _ => None,
        }
    } else if op == "UPDATE" {
        let set = s.tokens.iter().position(|t| t.is_word("SET"));
        set.map(|i| {
            let end = s.tokens[i + 1..]
                .iter()
                .position(|t| t.is_word("WHERE") || t.is_word("FROM"))
                .map(|n| i + 1 + n)
                .unwrap_or(s.tokens.len());
            (i + 1, end)
        })
    } else {
        None
    };

    let mut out = Vec::new();
    if op == "UPDATE" {
        if let Some((start, end)) = start_end {
            let mut segment_start = start;
            for i in start..=end {
                if i == end || s.tokens[i].is_punct(",") {
                    if let Some(eq) = (segment_start..i).find(|&j| s.tokens[j].is_punct("=")) {
                        if let Some((qualifier, field, _)) = qualified_ref(&s.tokens, segment_start)
                        {
                            if let Some(table) = table_for_ref(&qualifier, aliases, tables) {
                                out.push((table, field));
                            }
                        } else if let Some(field) = word(s, eq.saturating_sub(1)) {
                            if tables.len() == 1 {
                                out.push((tables[0].clone(), field.to_uppercase()));
                            }
                        }
                    }
                    segment_start = i + 1;
                }
            }
        }
    } else if let Some((start, end)) = start_end {
        let mut i = start;
        while i < end {
            if let Some((qualifier, field, next)) = qualified_ref(&s.tokens, i) {
                if let Some(table) = table_for_ref(&qualifier, aliases, tables) {
                    out.push((table, field));
                }
                i = next;
                continue;
            }
            let t = &s.tokens[i];
            if t.kind == Kind::Word
                && !is_clause(t)
                && !t.is_word("DISTINCT")
                && !t.is_word("SINGLE")
                && !s.tokens.get(i + 1).is_some_and(|next| next.is_punct("("))
                && !i
                    .checked_sub(1)
                    .and_then(|j| s.tokens.get(j))
                    .is_some_and(|prev| prev.is_word("AS"))
            {
                if tables.len() == 1 && !t.text.eq_ignore_ascii_case("AS") {
                    out.push((tables[0].clone(), t.text.to_uppercase()));
                }
            }
            i += 1;
        }
    }
    out.retain(|(_, field)| !is_client_field(field));
    out.sort();
    out.dedup();
    out
}

fn target(s: &Statement) -> Option<String> {
    let into = s
        .tokens
        .iter()
        .position(|t| t.is_word("INTO") || t.is_word("APPENDING"))?;
    let mut i = into + 1;
    while i < s.tokens.len()
        && (s.tokens[i].is_word("TABLE")
            || s.tokens[i].is_word("CORRESPONDING")
            || s.tokens[i].is_word("FIELDS")
            || s.tokens[i].is_word("OF"))
    {
        i += 1;
    }
    if s.tokens.get(i).is_some_and(|t| t.is_punct("@")) {
        i += 1;
    }
    if s.tokens.get(i).is_some_and(|t| t.is_word("DATA")) {
        i += 1;
        if s.tokens.get(i).is_some_and(|t| t.is_punct("(")) {
            i += 1;
        }
    }
    word(s, i).map(str::to_lowercase)
}

fn join_relationships(
    s: &Statement,
    file: &str,
    tables: &[String],
    aliases: &BTreeMap<String, String>,
) -> Vec<Relationship> {
    let mut out = Vec::new();
    for i in 0..s.tokens.len() {
        if !(s.tokens[i].is_punct("=") || s.tokens[i].is_word("EQ")) || i < 3 {
            continue;
        }
        if let (Some((left_alias, left_field, left_end)), Some((right_alias, right_field, _))) = (
            qualified_ref(&s.tokens, i - 3),
            qualified_ref(&s.tokens, i + 1),
        ) {
            if left_end == i && !is_client_field(&left_field) && !is_client_field(&right_field) {
                if let (Some(left_table), Some(right_table)) = (
                    table_for_ref(&left_alias, aliases, tables),
                    table_for_ref(&right_alias, aliases, tables),
                ) {
                    out.push(Relationship {
                        left_table,
                        left_field,
                        right_table,
                        right_field,
                        source: "sql_equality",
                        file: file.to_string(),
                        line: Some(s.line),
                    });
                }
            }
        }
    }
    out
}

fn analyze_statements(
    file: &str,
    statements: &[Statement],
    variable_types: &BTreeMap<String, String>,
    model: &mut DataModel,
    usages: &mut BTreeMap<String, TableUsage>,
) {
    for s in statements {
        let Some(op) = operation(s) else {
            continue;
        };
        let (tables, aliases) = table_references(s, op);
        let fields = selected_fields(s, op, &tables, &aliases);
        let result_target = if op == "SELECT" { target(s) } else { None };
        let target_type = result_target
            .as_ref()
            .and_then(|name| variable_types.get(name))
            .cloned();

        for table in &tables {
            let usage = usages.entry(table.clone()).or_default();
            usage.operations.insert(op.to_string());
        }
        for (table, field) in &fields {
            usages
                .entry(table.clone())
                .or_default()
                .fields
                .insert(field.clone());
        }
        model
            .relationships
            .extend(join_relationships(s, file, &tables, &aliases));
        model.statements.push(DatabaseStatement {
            file: file.to_string(),
            line: s.line,
            operation: op.to_string(),
            tables,
            fields: fields.into_iter().map(|(_, f)| f).collect(),
            target: result_target,
            target_type,
        });
    }
}

fn inventory_entry(path: &str) -> Option<(String, String)> {
    let file = path.rsplit('/').next().unwrap_or(path);
    if file.starts_with('.') {
        return None;
    }
    let parts: Vec<_> = file.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    Some((
        parts[0].replace('#', "/").to_lowercase(),
        parts[1].to_uppercase(),
    ))
}

fn local_declarations(statements: &[Statement]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for statement in statements {
        let name = match statement.keyword().as_str() {
            "TYPES" | "CLASS" | "INTERFACE" => word(statement, 1),
            _ => None,
        };
        if let Some(name) = name {
            if !matches!(name.to_uppercase().as_str(), "BEGIN" | "END") {
                names.insert(name.to_lowercase());
            }
        }
    }
    names
}

fn add_declared_inventory(
    statements: &[Statement],
    inventory: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for statement in statements {
        let (name, object_type) = match statement.keyword().as_str() {
            "CLASS" => (word(statement, 1), Some("CLAS")),
            "INTERFACE" => (word(statement, 1), Some("INTF")),
            "FUNCTION" => (word(statement, 1), Some("FUNC")),
            _ => (None, None),
        };
        if let (Some(name), Some(object_type)) = (name, object_type) {
            inventory
                .entry(name.to_lowercase())
                .or_default()
                .insert(object_type.to_string());
        }
    }
}

fn literal_or_word(token: &Token) -> Option<String> {
    match token.kind {
        Kind::Word => Some(token.text.to_lowercase()),
        Kind::Literal if token.text.len() >= 2 => {
            Some(token.text[1..token.text.len() - 1].to_lowercase())
        }
        _ => None,
    }
}

pub(crate) fn is_builtin_type(name: &str) -> bool {
    matches!(
        name.to_uppercase().as_str(),
        "ABAP_BOOL"
            | "ABAP_CHAR1"
            | "ABAP_FALSE"
            | "ABAP_TRUE"
            | "ANY"
            | "C"
            | "CLIKE"
            | "CSEQUENCE"
            | "D"
            | "DATA"
            | "DECFLOAT16"
            | "DECFLOAT34"
            | "F"
            | "I"
            | "INT1"
            | "INT2"
            | "INT4"
            | "INT8"
            | "N"
            | "NUMERIC"
            | "P"
            | "SIMPLE"
            | "STRING"
            | "T"
            | "UTCLONG"
            | "X"
            | "XSEQUENCE"
            | "XSTRING"
    )
}

fn qualified_owner(tokens: &[Token], idx: usize) -> Option<(String, usize)> {
    let owner = literal_or_word(tokens.get(idx)?)?;
    if tokens
        .get(idx + 1)
        .is_some_and(|t| t.is_punct("=>") || t.is_punct("~"))
    {
        Some((owner, idx + 3))
    } else {
        Some((owner, idx + 1))
    }
}

fn type_reference_after(s: &Statement, idx: usize) -> Option<(String, bool)> {
    let mut i = idx + 1;
    let mut reference = false;
    if word(s, i).is_some_and(|w| w.eq_ignore_ascii_case("LINE")) {
        if word(s, i + 1).is_some_and(|w| w.eq_ignore_ascii_case("OF")) {
            i += 2;
        }
    }
    if word(s, i).is_some_and(|w| w.eq_ignore_ascii_case("RANGE")) {
        if word(s, i + 1).is_some_and(|w| w.eq_ignore_ascii_case("OF")) {
            i += 2;
        }
    }
    while word(s, i).is_some_and(|w| {
        matches!(
            w.to_uppercase().as_str(),
            "STANDARD" | "SORTED" | "HASHED" | "TABLE" | "OF"
        )
    }) {
        i += 1;
    }
    if word(s, i).is_some_and(|w| w.eq_ignore_ascii_case("REF")) {
        reference = true;
        i += 1;
    }
    if word(s, i).is_some_and(|w| w.eq_ignore_ascii_case("TO")) {
        reference = true;
        i += 1;
    }
    let (name, _) = qualified_owner(&s.tokens, i)?;
    let owner = name
        .split_once('-')
        .map_or(name.as_str(), |(owner, _)| owner)
        .to_string();
    (!owner.starts_with('<')).then_some((owner, reference))
}

fn push_dependency(
    model: &mut DataModel,
    source_type: &str,
    source: &str,
    target: String,
    kind: &str,
    inventory: &BTreeMap<String, BTreeSet<String>>,
    target_type_hint: Option<&str>,
    file: &str,
    line: Option<usize>,
) {
    let target = target.to_lowercase();
    if target.is_empty() || target == source {
        return;
    }
    let target_types = inventory.get(&target);
    let target_type = target_type_hint
        .and_then(|hint| {
            target_types
                .filter(|types| types.contains(hint))
                .map(|_| hint.to_string())
                .or_else(|| Some(hint.to_string()))
        })
        .or_else(|| {
            target_types
                .filter(|types| types.len() == 1)
                .and_then(|types| types.first().cloned())
        });
    let in_zip = inventory.contains_key(&target);
    model.dependencies.push(Dependency {
        source_type: source_type.to_string(),
        source: source.to_string(),
        target_type,
        target,
        kind: kind.to_string(),
        in_zip,
        file: file.to_string(),
        line,
    });
}

fn analyze_object_dependencies(
    file: &str,
    statements: &[Statement],
    local: &BTreeSet<String>,
    source_type: &str,
    source: &str,
    inventory: &BTreeMap<String, BTreeSet<String>>,
    model: &mut DataModel,
) {
    let mut add = |target: String, kind: &str, hint: Option<&str>, line: usize| {
        push_dependency(
            model,
            source_type,
            source,
            target,
            kind,
            inventory,
            hint,
            file,
            Some(line),
        );
    };

    for statement in statements {
        let keyword = statement.keyword();
        for (idx, token) in statement.tokens.iter().enumerate() {
            if token.is_word("TYPE") || token.is_word("LIKE") {
                if let Some((target, reference)) = type_reference_after(statement, idx) {
                    if !is_builtin_type(&target) && !local.contains(&target) {
                        add(
                            target,
                            if reference {
                                "object_reference"
                            } else {
                                "type"
                            },
                            None,
                            statement.line,
                        );
                    }
                }
            }
            if idx > 0 && token.is_punct("=>") {
                if let Some(target) = literal_or_word(&statement.tokens[idx - 1]) {
                    if !local.contains(&target) && !target.contains("->") {
                        add(target, "static_member", None, statement.line);
                    }
                }
            }
            if token.is_word("NEW") {
                if let Some((target, _)) = qualified_owner(&statement.tokens, idx + 1) {
                    if !is_builtin_type(&target) && !local.contains(&target) {
                        add(target, "construction", Some("CLAS"), statement.line);
                    }
                }
            }
            if token.is_word("CAST") {
                if let Some((target, _)) = qualified_owner(&statement.tokens, idx + 1) {
                    if !is_builtin_type(&target) && !local.contains(&target) {
                        add(target, "cast", None, statement.line);
                    }
                }
            }
        }

        let owner = statement.tokens.get(1).and_then(literal_or_word);
        match keyword.as_str() {
            "INHERITING" => {
                if let Some(idx) = statement.tokens.iter().position(|t| t.is_word("FROM")) {
                    if let Some(target) = statement.tokens.get(idx + 1).and_then(literal_or_word) {
                        add(target, "inheritance", Some("CLAS"), statement.line);
                    }
                }
            }
            "CLASS" | "INTERFACE" => {
                if let Some(from) = statement.tokens.iter().position(|t| t.is_word("FROM")) {
                    if let Some(target) = statement.tokens.get(from + 1).and_then(literal_or_word) {
                        add(target, "inheritance", Some("CLAS"), statement.line);
                    }
                }
            }
            "INTERFACES" => {
                if let Some(target) = owner {
                    add(target, "interface", Some("INTF"), statement.line);
                }
            }
            "TABLES" | "NODES" => {
                if let Some(target) = owner {
                    add(target, "declaration", Some("TABL"), statement.line);
                }
            }
            "INCLUDE" => {
                if let Some(target) = owner {
                    add(target, "include", Some("PROG"), statement.line);
                }
            }
            "CALL"
                if statement
                    .tokens
                    .get(1)
                    .is_some_and(|t| t.is_word("FUNCTION")) =>
            {
                if let Some(target) = statement.tokens.get(2).and_then(literal_or_word) {
                    add(target, "function_call", Some("FUNC"), statement.line);
                }
            }
            "SUBMIT" => {
                if let Some(target) = owner {
                    add(target, "program_call", Some("PROG"), statement.line);
                }
            }
            "CATCH" => {
                if let Some(target) = owner {
                    add(target, "exception", Some("CLAS"), statement.line);
                }
            }
            "TYPE-POOLS" => {
                if let Some(target) = owner {
                    add(target, "type_pool", Some("TYPE"), statement.line);
                }
            }
            "CALL"
                if statement
                    .tokens
                    .get(1)
                    .is_some_and(|t| t.is_word("TRANSACTION")) =>
            {
                if let Some(target) = statement.tokens.get(2).and_then(literal_or_word) {
                    add(target, "transaction_call", Some("TRAN"), statement.line);
                }
            }
            "CALL"
                if statement
                    .tokens
                    .get(1)
                    .is_some_and(|t| t.is_word("TRANSFORMATION")) =>
            {
                if let Some(target) = statement.tokens.get(2).and_then(literal_or_word) {
                    add(target, "transformation_call", Some("XSLT"), statement.line);
                }
            }
            "PERFORM" => {
                if let Some(program) = statement.tokens.iter().position(|t| t.is_word("PROGRAM")) {
                    if let Some(target) =
                        statement.tokens.get(program + 1).and_then(literal_or_word)
                    {
                        add(target, "program_call", Some("PROG"), statement.line);
                    }
                }
            }
            _ => {}
        }
    }
}

fn analyze_with_inventory(files: &[(String, String)], inventory_files: &[String]) -> DataModel {
    let mut model = DataModel::default();
    let mut usages = BTreeMap::new();
    let mut groups: BTreeMap<(String, String), Vec<(String, String)>> = BTreeMap::new();
    let mut inventory: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in inventory_files {
        if let Some((name, object_type)) = inventory_entry(path) {
            inventory.entry(name).or_default().insert(object_type);
        }
    }
    for (file, src) in files {
        add_declared_inventory(&collect_program(&parse_l1(src)), &mut inventory);
        let key = crate::inspect::objects::object_key(file)
            .unwrap_or_else(|| (String::new(), file.clone()));
        groups
            .entry(key)
            .or_default()
            .push((file.clone(), src.clone()));
    }
    for ((source_type, source), sources) in groups {
        let parsed: Vec<(String, Vec<Statement>)> = sources
            .iter()
            .map(|(file, src)| (file.clone(), collect_program(&parse_l1(src))))
            .collect();
        let all_statements: Vec<Statement> = parsed
            .iter()
            .flat_map(|(_, statements)| statements.iter().cloned())
            .collect();
        let local = local_declarations(&all_statements);
        let (variable_types, structures) = parse_structure_info(&all_statements);
        for (file, statements) in &parsed {
            analyze_statements(file, statements, &variable_types, &mut model, &mut usages);
            analyze_object_dependencies(
                file,
                statements,
                &local,
                &source_type,
                &source,
                &inventory,
                &mut model,
            );
        }
        for (name, fields) in structures {
            model.structures.push(StructureModel { name, fields });
        }
    }
    model.tables = usages
        .into_iter()
        .map(|(name, usage)| TableModel {
            name,
            operations: usage.operations.into_iter().collect(),
            fields: usage.fields.into_iter().collect(),
        })
        .collect();
    model.structures.sort_by(|a, b| a.name.cmp(&b.name));
    model
}

pub fn analyze(files: &[(String, String)]) -> DataModel {
    let inventory: Vec<String> = files.iter().map(|(file, _)| file.clone()).collect();
    analyze_with_inventory(files, &inventory)
}

pub(crate) fn xml_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut blocks = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let content = &rest[start + open.len()..];
        let Some(end) = content.find(&close) else {
            break;
        };
        blocks.push(&content[..end]);
        rest = &content[end + close.len()..];
    }
    blocks
}

pub(crate) fn xml_value(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    let value = block[start..end].trim();
    (!value.is_empty()).then(|| value.to_uppercase())
}

fn ddic_relationships(files: &[(String, String)]) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    for (file, xml) in files {
        let Some(table) = xml_blocks(xml, "DD02V")
            .first()
            .and_then(|block| xml_value(block, "TABNAME"))
        else {
            continue;
        };
        for mapping in xml_blocks(xml, "DD05M") {
            let Some(foreign_table) = xml_value(mapping, "FORTABLE") else {
                continue;
            };
            if foreign_table != table {
                continue;
            }
            let (Some(left_field), Some(right_table), Some(right_field)) = (
                xml_value(mapping, "FORKEY"),
                xml_value(mapping, "CHECKTABLE"),
                xml_value(mapping, "CHECKFIELD"),
            ) else {
                continue;
            };
            if is_client_field(&left_field) || is_client_field(&right_field) {
                continue;
            }
            relationships.push(Relationship {
                left_table: table.clone(),
                left_field,
                right_table,
                right_field,
                source: "ddic_foreign_key",
                file: file.clone(),
                line: None,
            });
        }
    }
    relationships
}

pub fn analyze_with_ddic(
    files: &[(String, String)],
    table_metadata: &[(String, String)],
) -> DataModel {
    let inventory: Vec<String> = files
        .iter()
        .map(|(file, _)| file.clone())
        .chain(table_metadata.iter().map(|(file, _)| file.clone()))
        .collect();
    analyze_with_ddic_and_inventory(files, table_metadata, &inventory)
}

pub fn analyze_with_ddic_and_inventory(
    files: &[(String, String)],
    table_metadata: &[(String, String)],
    inventory_files: &[String],
) -> DataModel {
    let mut model = analyze_with_inventory(files, inventory_files);
    model
        .relationships
        .extend(ddic_relationships(table_metadata));
    let table_usages: Vec<_> = model
        .statements
        .iter()
        .flat_map(|statement| {
            statement
                .tables
                .iter()
                .map(|table| (table.clone(), statement.file.clone(), statement.line))
        })
        .collect();
    let inventory: BTreeMap<String, BTreeSet<String>> = inventory_files
        .iter()
        .filter_map(|path| inventory_entry(path))
        .fold(BTreeMap::new(), |mut map, (name, object_type)| {
            map.entry(name).or_default().insert(object_type);
            map
        });
    for (table, file, line) in table_usages {
        if let Some((source_type, source)) = crate::inspect::objects::object_key(&file) {
            push_dependency(
                &mut model,
                &source_type,
                &source,
                table,
                "database_table",
                &inventory,
                Some("TABL"),
                &file,
                Some(line),
            );
        }
    }
    for relationship in ddic_relationships(table_metadata) {
        if let Some((source_type, source)) = inventory_entry(&relationship.file) {
            push_dependency(
                &mut model,
                &source_type,
                &source,
                relationship.right_table,
                "foreign_key",
                &inventory,
                Some("TABL"),
                &relationship.file,
                relationship.line,
            );
        }
    }
    model
}

pub fn render_json(model: &DataModel) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tables_selected_fields_join_and_result_type() {
        let definitions = "\
TYPES: BEGIN OF ty_result,
         carrier TYPE s_carr_id,
         connection TYPE s_conn_id,
       END OF ty_result.
DATA lt_result TYPE ty_result.
";
        let query = "\
CLASS zreport IMPLEMENTATION.
METHOD run.
SELECT a~carrid, b~connid
  FROM scarr AS a
  INNER JOIN spfli AS b ON a~carrid = b~carrid
  INTO TABLE @lt_result.
ENDMETHOD.
ENDCLASS.";
        let model = analyze(&[
            ("zreport.clas.locals_def.abap".into(), definitions.into()),
            ("zreport.clas.methods.abap".into(), query.into()),
        ]);

        assert_eq!(model.tables.len(), 2);
        assert_eq!(model.tables[0].name, "SCARR");
        assert_eq!(model.tables[0].fields, vec!["CARRID"]);
        assert_eq!(model.tables[1].name, "SPFLI");
        assert_eq!(model.tables[1].fields, vec!["CONNID"]);
        assert_eq!(model.relationships.len(), 1);
        assert_eq!(model.relationships[0].left_table, "SCARR");
        assert_eq!(model.relationships[0].right_table, "SPFLI");
        assert_eq!(model.statements[0].target.as_deref(), Some("lt_result"));
        assert_eq!(
            model.statements[0].target_type.as_deref(),
            Some("ty_result")
        );
        assert_eq!(model.structures[0].fields.len(), 2);
        let json = render_json(&model).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["tables"][0]["name"], "SCARR");
        assert_eq!(value["relationships"][0]["right_table"], "SPFLI");
    }

    #[test]
    fn finds_write_tables_for_basic_mutations() {
        let src = "INSERT ztab FROM wa.\nUPDATE ztab SET field = @value WHERE id = @id.\nDELETE FROM ztab WHERE id = @id.";
        let model = analyze(&[("zwrite.prog.abap".into(), src.into())]);

        assert_eq!(
            model
                .statements
                .iter()
                .map(|s| s.operation.as_str())
                .collect::<Vec<_>>(),
            vec!["INSERT", "UPDATE", "DELETE"]
        );
        assert_eq!(model.tables[0].name, "ZTAB");
        assert_eq!(
            model.tables[0].operations,
            vec!["DELETE", "INSERT", "UPDATE"]
        );
    }

    #[test]
    fn ignores_internal_table_mutations() {
        let src = "\
DELETE ADJACENT DUPLICATES FROM lt_items.
DELETE lt_items WHERE id = 1.
DELETE lt_items INDEX 1.
INSERT row INTO TABLE lt_items.
MODIFY lt_items FROM row INDEX 1.
MODIFY SCREEN.
";
        assert!(analyze(&[("zinternal.prog.abap".into(), src.into())])
            .statements
            .is_empty());
    }

    #[test]
    fn excludes_select_aliases_from_database_fields() {
        let src = "SELECT id AS identifier FROM ztab.\nENDSELECT.";
        let model = analyze(&[("zread.prog.abap".into(), src.into())]);

        assert_eq!(model.tables[0].fields, vec!["ID"]);
    }

    #[test]
    fn skips_corresponding_into_clause_when_collecting_select_fields() {
        let src = "SELECT * INTO CORRESPONDING FIELDS OF TABLE @lt_result FROM ztab.";
        let model = analyze(&[("zread.prog.abap".into(), src.into())]);

        assert!(model.statements[0].fields.is_empty());
        assert_eq!(model.statements[0].target.as_deref(), Some("lt_result"));
        assert_eq!(model.tables[0].fields, Vec::<String>::new());
    }

    #[test]
    fn finds_relations_in_where_clause_and_comma_from_list() {
        let src = "\
SELECT a~parent b~id INTO TABLE @lt_result
  FROM zchild AS a, zparent AS b
  WHERE a~parent EQ b~id.";
        let model = analyze(&[("zread.prog.abap".into(), src.into())]);

        assert_eq!(model.tables.len(), 2);
        assert_eq!(model.relationships.len(), 1);
        assert_eq!(model.relationships[0].left_table, "ZCHILD");
        assert_eq!(model.relationships[0].left_field, "PARENT");
        assert_eq!(model.relationships[0].right_table, "ZPARENT");
        assert_eq!(model.relationships[0].right_field, "ID");
        assert_eq!(model.statements[0].fields, vec!["PARENT", "ID"]);
    }

    #[test]
    fn reads_ddic_foreign_key_mappings_and_skips_client_system_rows() {
        let xml = "\
<abapGit>
  <DD02V><TABNAME>Z_CHILD</TABNAME></DD02V>
  <DD05M><FORTABLE>SYST</FORTABLE><FORKEY>MANDT</FORKEY><CHECKTABLE>Z_PARENT</CHECKTABLE><CHECKFIELD>MANDT</CHECKFIELD></DD05M>
  <DD05M><FORTABLE>Z_CHILD</FORTABLE><FORKEY>PARENT_ID</FORKEY><CHECKTABLE>Z_PARENT</CHECKTABLE><CHECKFIELD>ID</CHECKFIELD></DD05M>
</abapGit>";
        let model = analyze_with_ddic(&[], &[("src/z_child.tabl.xml".into(), xml.into())]);

        assert_eq!(model.relationships.len(), 1);
        assert_eq!(model.relationships[0].left_table, "Z_CHILD");
        assert_eq!(model.relationships[0].left_field, "PARENT_ID");
        assert_eq!(model.relationships[0].right_table, "Z_PARENT");
        assert_eq!(model.relationships[0].right_field, "ID");
        assert_eq!(model.relationships[0].source, "ddic_foreign_key");
        assert_eq!(model.relationships[0].line, None);
    }

    #[test]
    fn excludes_client_fields_and_relations_from_sql_and_ddic() {
        let src = "\
TYPES: BEGIN OF ty_result, mandt TYPE mandt, Client TYPE mandt, client_id TYPE i, END OF ty_result.
SELECT a~mandt a~Client a~client_id b~id FROM zchild AS a
  JOIN zparent AS b ON a~mandt = b~mandt AND a~client = b~id
  AND a~client_id = b~id INTO TABLE @DATA(result).";
        let xml = "\
<DD02V><TABNAME>Z_CHILD</TABNAME></DD02V>
<DD05M><FORTABLE>Z_CHILD</FORTABLE><FORKEY>MANDT</FORKEY><CHECKTABLE>Z_PARENT</CHECKTABLE><CHECKFIELD>MANDT</CHECKFIELD></DD05M>
<DD05M><FORTABLE>Z_CHILD</FORTABLE><FORKEY>CLIENT_ID</FORKEY><CHECKTABLE>Z_PARENT</CHECKTABLE><CHECKFIELD>CLIENT</CHECKFIELD></DD05M>
<DD05M><FORTABLE>Z_CHILD</FORTABLE><FORKEY>CLIENT_ID</FORKEY><CHECKTABLE>Z_PARENT</CHECKTABLE><CHECKFIELD>ID</CHECKFIELD></DD05M>";
        let model = analyze_with_ddic(
            &[("zchild.prog.abap".into(), src.into())],
            &[("zchild.tabl.xml".into(), xml.into())],
        );

        assert_eq!(model.statements[0].fields, vec!["CLIENT_ID", "ID"]);
        assert_eq!(model.tables[0].fields, vec!["CLIENT_ID"]);
        assert_eq!(model.structures[0].fields.len(), 1);
        assert_eq!(model.structures[0].fields[0].name, "client_id");
        assert_eq!(model.relationships.len(), 2);
        assert!(model
            .relationships
            .iter()
            .all(|r| r.left_field == "CLIENT_ID" && r.right_field == "ID"));
    }

    #[test]
    fn json_groups_relationship_fields_and_preserves_distinct_sources() {
        let src = "\
SELECT a~id FROM za AS a JOIN zb AS b ON a~id = b~id AND a~code = b~code INTO TABLE @DATA(result).
SELECT a~id FROM za AS a JOIN zb AS b ON a~id = b~id INTO TABLE @DATA(result2).";
        let xml = "\
<DD02V><TABNAME>ZA</TABNAME></DD02V>
<DD05M><FORTABLE>ZA</FORTABLE><FORKEY>ID</FORKEY><CHECKTABLE>ZB</CHECKTABLE><CHECKFIELD>ID</CHECKFIELD></DD05M>";
        let files = vec![("za.prog.abap".into(), src.into())];
        let metadata = vec![("za.tabl.xml".into(), xml.into())];
        let model = analyze_with_ddic(&files, &metadata);
        let json: serde_json::Value = serde_json::from_str(&render_json(&model).unwrap()).unwrap();

        assert_eq!(json["relationships"].as_array().unwrap().len(), 1);
        let relationship = &json["relationships"][0];
        assert_eq!(relationship["left_table"], "ZA");
        assert_eq!(relationship["right_table"], "ZB");
        assert_eq!(relationship["fields"].as_array().unwrap().len(), 2);
        let fields = relationship["fields"].as_array().unwrap();
        let id = fields.iter().find(|f| f["left"] == "ID").unwrap();
        assert_eq!(id["right"], "ID");
        assert_eq!(id["sources"].as_array().unwrap().len(), 3);
        assert!(id["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["source"] == "ddic_foreign_key" && s.get("line").is_none()));
        assert!(id["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["source"] == "sql_equality" && s["line"] == 2));
    }

    #[test]
    fn finds_object_type_call_and_database_dependencies() {
        let source = "\
CLASS zconsumer DEFINITION INHERITING FROM zbase.
  PUBLIC SECTION.
    INTERFACES zif_contract.
    DATA mo_helper TYPE REF TO zcl_helper.
    DATA ms_row TYPE zpersist.
    TYPES ty_local TYPE i.
    DATA ms_local TYPE ty_local.
ENDCLASS.
CLASS zconsumer IMPLEMENTATION.
  METHOD run.
    DATA(lo_factory) = NEW zcl_factory( ).
    zcl_helper=>run( ).
    CALL FUNCTION 'Z_EXTERNAL'.
    SELECT SINGLE id FROM zpersist INTO @DATA(id).
  ENDMETHOD.
ENDCLASS.
";
        let files = vec![("src/zconsumer.clas.abap".into(), source.into())];
        let inventory = vec![
            "src/zconsumer.clas.abap".into(),
            "src/zbase.clas.abap".into(),
            "src/zif_contract.intf.abap".into(),
            "src/zcl_helper.clas.abap".into(),
            "src/zcl_factory.clas.abap".into(),
            "src/zpersist.tabl.xml".into(),
        ];
        let model = analyze_with_ddic_and_inventory(&files, &[], &inventory);
        let json: serde_json::Value = serde_json::from_str(&render_json(&model).unwrap()).unwrap();
        let dependencies = json["dependencies"].as_array().unwrap();

        let target = |name: &str, kind: &str| {
            dependencies
                .iter()
                .find(|dependency| dependency["target"] == name && dependency["kind"] == kind)
        };
        assert_eq!(
            target("zbase", "inheritance").unwrap()["target_type"],
            "CLAS"
        );
        assert_eq!(target("zif_contract", "interface").unwrap()["in_zip"], true);
        assert_eq!(
            target("zcl_helper", "object_reference").unwrap()["target_type"],
            "CLAS"
        );
        assert_eq!(
            target("zcl_factory", "construction").unwrap()["in_zip"],
            true
        );
        assert!(target("zcl_helper", "static_member").is_some());
        assert!(target("z_external", "function_call").is_some());
        assert!(target("zpersist", "database_table").is_some());
        assert!(!dependencies
            .iter()
            .any(|dependency| ["i", "ty_local"].contains(&dependency["target"].as_str().unwrap())));
    }
}
