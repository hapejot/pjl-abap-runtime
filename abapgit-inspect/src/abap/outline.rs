//! Gliederungsausgabe des ABAP-AST.

use super::parser::{ClassKind, Item, MemberKind, Program, Statement, TopItem, Visibility};

const MAX_LEN: usize = 110;

fn fmt_stmt(s: &Statement) -> String {
    let t = s.text();
    let short: String = if t.chars().count() > MAX_LEN {
        t.chars().take(MAX_LEN).collect::<String>() + "..."
    } else {
        t
    };
    format!("L{}: {}", s.line, short)
}

fn vis_label(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    }
}

fn dump_items(out: &mut String, items: &[Item], depth: usize) {
    let ind = "  ".repeat(depth);
    for item in items {
        match item {
            Item::Statement(s) => out.push_str(&format!("{ind}{}\n", fmt_stmt(s))),
            Item::Block { head, body } => {
                out.push_str(&format!("{ind}{} [{}]\n", fmt_stmt(head), body.len()));
                dump_items(out, body, depth + 1);
            }
        }
    }
}

/// Erzeugt eine lesbare Gliederung des Programms.
pub fn outline(prog: &Program) -> String {
    let mut out = String::new();
    for item in &prog.items {
        match item {
            TopItem::Statement(s) => out.push_str(&format!("{}\n", fmt_stmt(s))),
            TopItem::Block { head, body } => {
                out.push_str(&format!("{} [{}]\n", fmt_stmt(head), body.len()));
                dump_items(&mut out, body, 1);
            }
            TopItem::Class(c) => {
                let label = match c.kind {
                    ClassKind::Definition => "CLASS DEFINITION",
                    ClassKind::Implementation => "CLASS IMPLEMENTATION",
                    ClassKind::Interface => "INTERFACE",
                };
                out.push_str(&format!("{label} {} ({})\n", c.name, fmt_stmt(&c.head)));
                for m in &c.members {
                    let vis = vis_label(m.visibility);
                    match &m.kind {
                        MemberKind::Method(sig) => {
                            let cls = if sig.is_class { "class " } else { "" };
                            out.push_str(&format!("  [{vis}] {cls}method {}\n", sig.name));
                            for (sec, raw) in &sig.sections {
                                out.push_str(&format!("      {sec}: {raw}\n"));
                            }
                        }
                        _ => {
                            out.push_str(&format!("  [{vis}] {}\n", m.statement.text()));
                        }
                    }
                }
                for m in &c.methods {
                    out.push_str(&format!("  METHOD {} ({})\n", m.name, fmt_stmt(&m.head)));
                    dump_items(&mut out, &m.body, 2);
                }
                if !c.other.is_empty() {
                    out.push_str("  (sonstiges)\n");
                    dump_items(&mut out, &c.other, 2);
                }
            }
            TopItem::Routine { kind, name, head, body } => {
                let label = if name == kind { kind.clone() } else { format!("{kind} {name}") };
                out.push_str(&format!("{label} ({})\n", fmt_stmt(head)));
                dump_items(&mut out, body, 1);
            }
        }
    }
    if !prog.errors.is_empty() {
        out.push_str(&format!("\nFehler ({}):\n", prog.errors.len()));
        for e in &prog.errors {
            out.push_str(&format!("  L{}: {}\n", e.line, e.message));
        }
    }
    out
}
