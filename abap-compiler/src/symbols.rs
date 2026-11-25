//! Symbolkatalog für Klassen, Interfaces und deren Implementierungen.

use std::collections::{BTreeMap, BTreeSet};

use abapgit_inspect::abap::{ClassKind, MemberKind, Program, Statement, TopItem};

use crate::ast::CompileError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    Interface,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolOrigin {
    Internal,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolReference {
    pub name: String,
    pub kind: SymbolKind,
    pub origin: SymbolOrigin,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct ClassSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
    pub methods: BTreeSet<String>,
    pub implemented_methods: BTreeSet<String>,
    pub parent: Option<SymbolReference>,
    pub interfaces: Vec<SymbolReference>,
}

impl ClassSymbol {
    pub fn missing_methods(&self) -> impl Iterator<Item = &str> {
        self.methods
            .difference(&self.implemented_methods)
            .map(String::as_str)
    }
}

#[derive(Debug, Default, Clone)]
pub struct SymbolCatalog {
    classes: BTreeMap<String, ClassSymbol>,
}

impl SymbolCatalog {
    pub fn from_program(program: &Program) -> Result<Self, CompileError> {
        let mut catalog = Self::default();

        for item in &program.items {
            let TopItem::Class(class) = item else {
                continue;
            };
            if class.kind == ClassKind::Implementation {
                continue;
            }
            let kind = match class.kind {
                ClassKind::Definition => SymbolKind::Class,
                ClassKind::Interface => SymbolKind::Interface,
                ClassKind::Implementation => unreachable!(),
            };
            let name = class.name.to_lowercase();
            if catalog.classes.contains_key(&name) {
                return symbol_error(
                    class.head.line,
                    format!("Symbol '{name}' ist mehrfach definiert"),
                );
            }

            let mut methods = BTreeSet::new();
            let mut interfaces = Vec::new();
            for member in &class.members {
                match &member.kind {
                    MemberKind::Method(signature) => {
                        let method = signature.name.to_lowercase();
                        if !methods.insert(method.clone()) {
                            return symbol_error(
                                member.statement.line,
                                format!("Methode '{method}' in '{name}' ist mehrfach deklariert"),
                            );
                        }
                    }
                    MemberKind::Interfaces => {
                        if let Some(interface) = word_after(&member.statement, 0) {
                            interfaces.push((interface.to_lowercase(), member.statement.line));
                        }
                    }
                    _ => {}
                }
            }
            catalog.classes.insert(
                name.clone(),
                ClassSymbol {
                    name,
                    kind,
                    line: class.head.line,
                    methods,
                    implemented_methods: BTreeSet::new(),
                    parent: word_after_keyword(&class.head, "FROM").map(|parent| SymbolReference {
                        name: parent.to_lowercase(),
                        kind: SymbolKind::Class,
                        origin: SymbolOrigin::External,
                        line: class.head.line,
                    }),
                    interfaces: interfaces
                        .into_iter()
                        .map(|(interface, line)| SymbolReference {
                            name: interface,
                            kind: SymbolKind::Interface,
                            origin: SymbolOrigin::External,
                            line,
                        })
                        .collect(),
                },
            );
        }

        catalog.resolve_references();
        catalog.collect_implementations(program)?;
        Ok(catalog)
    }

    pub fn get(&self, name: &str) -> Option<&ClassSymbol> {
        self.classes.get(&name.to_lowercase())
    }

    pub fn classes(&self) -> impl Iterator<Item = &ClassSymbol> {
        self.classes.values()
    }

    pub fn external_references(&self) -> impl Iterator<Item = &SymbolReference> {
        self.classes()
            .flat_map(|symbol| symbol.parent.iter().chain(symbol.interfaces.iter()))
            .filter(|reference| reference.origin == SymbolOrigin::External)
    }

    fn resolve_references(&mut self) {
        let kinds: BTreeMap<String, SymbolKind> = self
            .classes
            .iter()
            .map(|(name, symbol)| (name.clone(), symbol.kind))
            .collect();
        for symbol in self.classes.values_mut() {
            if let Some(reference) = symbol.parent.as_mut() {
                if kinds
                    .get(&reference.name)
                    .is_some_and(|kind| *kind == SymbolKind::Class)
                {
                    reference.origin = SymbolOrigin::Internal;
                }
            }
            for reference in &mut symbol.interfaces {
                if kinds
                    .get(&reference.name)
                    .is_some_and(|kind| *kind == SymbolKind::Interface)
                {
                    reference.origin = SymbolOrigin::Internal;
                }
            }
        }
    }

    fn collect_implementations(&mut self, program: &Program) -> Result<(), CompileError> {
        let mut seen = BTreeSet::new();
        let interface_closures: BTreeMap<String, BTreeSet<String>> = self
            .classes
            .values()
            .map(|symbol| {
                let mut closure = BTreeSet::new();
                let mut pending: Vec<String> = symbol
                    .interfaces
                    .iter()
                    .map(|reference| reference.name.clone())
                    .collect();
                while let Some(interface) = pending.pop() {
                    if !closure.insert(interface.clone()) {
                        continue;
                    }
                    if let Some(parent) = self.classes.get(&interface) {
                        pending.extend(
                            parent
                                .interfaces
                                .iter()
                                .map(|reference| reference.name.clone()),
                        );
                    }
                }
                (symbol.name.clone(), closure)
            })
            .collect();
        for item in &program.items {
            let TopItem::Class(class) = item else {
                continue;
            };
            if class.kind != ClassKind::Implementation {
                continue;
            }
            let name = class.name.to_lowercase();
            let Some(symbol) = self.classes.get_mut(&name) else {
                return symbol_error(
                    class.head.line,
                    format!("Implementierung für unbekannte Klasse '{name}'"),
                );
            };
            if symbol.kind != SymbolKind::Class {
                return symbol_error(
                    class.head.line,
                    format!("Interface '{name}' hat keinen Implementierungskörper"),
                );
            }
            if !seen.insert(name.clone()) {
                return symbol_error(
                    class.head.line,
                    format!("Implementierung für Klasse '{name}' ist mehrfach vorhanden"),
                );
            }
            for method in &class.methods {
                let method_name = method.name.to_lowercase();
                let is_interface_method =
                    method_name
                        .split_once('~')
                        .is_some_and(|(interface, implementation)| {
                            interface_closures
                                .get(&name)
                                .is_some_and(|interfaces| interfaces.contains(interface))
                                && !implementation.is_empty()
                        });
                if !symbol.methods.contains(&method_name) && !is_interface_method {
                    return symbol_error(
                        method.head.line,
                        format!("Methode '{method_name}' ist in Klasse '{name}' nicht deklariert"),
                    );
                }
                if !symbol.implemented_methods.insert(method_name.clone()) {
                    return symbol_error(
                        method.head.line,
                        format!("Methode '{method_name}' in '{name}' ist mehrfach implementiert"),
                    );
                }
            }
        }
        Ok(())
    }
}

fn symbol_error<T>(line: usize, message: String) -> Result<T, CompileError> {
    Err(CompileError { line, message })
}

fn word_after(statement: &Statement, index: usize) -> Option<&str> {
    statement
        .tokens
        .get(index + 1)
        .filter(|token| token.kind == abapgit_inspect::abap::Kind::Word)
        .map(|token| token.text.as_str())
}

fn word_after_keyword<'a>(statement: &'a Statement, keyword: &str) -> Option<&'a str> {
    statement
        .tokens
        .iter()
        .position(|token| token.is_word(keyword))
        .and_then(|index| word_after(statement, index))
}

#[cfg(test)]
mod tests {
    use abapgit_inspect::abap::parse_l1;

    use super::{SymbolCatalog, SymbolKind, SymbolOrigin};

    #[test]
    fn resolves_internal_and_external_oo_symbols() {
        let program = parse_l1(
            "INTERFACE if_local.\n\
               METHODS run.\n\
             ENDINTERFACE.\n\
             CLASS base DEFINITION.\n\
             ENDCLASS.\n\
             CLASS base IMPLEMENTATION.\n\
             ENDCLASS.\n\
             CLASS child DEFINITION INHERITING FROM base.\n\
               PUBLIC SECTION.\n\
                 INTERFACES if_local.\n\
                 INTERFACES if_external.\n\
                 METHODS execute.\n\
             ENDCLASS.\n\
             CLASS child IMPLEMENTATION.\n\
               METHOD execute.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        );

        let symbols = SymbolCatalog::from_program(&program).unwrap();
        assert_eq!(
            symbols
                .get("child")
                .unwrap()
                .parent
                .as_ref()
                .unwrap()
                .origin,
            SymbolOrigin::Internal
        );
        assert_eq!(
            symbols.get("child").unwrap().interfaces[0].origin,
            SymbolOrigin::Internal
        );
        assert_eq!(
            symbols.get("child").unwrap().interfaces[1].origin,
            SymbolOrigin::External
        );
        assert_eq!(symbols.external_references().count(), 1);
        assert_eq!(symbols.get("child").unwrap().kind, SymbolKind::Class);
    }

    #[test]
    fn rejects_implementation_without_declaration() {
        let error = SymbolCatalog::from_program(&parse_l1("CLASS ghost IMPLEMENTATION.\nENDCLASS."))
            .unwrap_err();

        assert!(error.message.contains("unbekannte Klasse 'ghost'"));
    }

    #[test]
    fn rejects_undeclared_method_implementation() {
        let error = SymbolCatalog::from_program(&parse_l1(
            "CLASS example DEFINITION.\nENDCLASS.\n\
             CLASS example IMPLEMENTATION.\n\
               METHOD missing.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        ))
        .unwrap_err();

        assert!(error.message.contains("nicht deklariert"));
    }

    #[test]
    fn reports_missing_method_implementations() {
        let symbols = SymbolCatalog::from_program(&parse_l1(
            "CLASS example DEFINITION.\n\
               PUBLIC SECTION.\n\
                 METHODS missing.\n\
             ENDCLASS.\n\
             CLASS example IMPLEMENTATION.\n\
             ENDCLASS.",
        ))
        .unwrap();

        assert_eq!(
            symbols
                .get("example")
                .unwrap()
                .missing_methods()
                .collect::<Vec<_>>(),
            vec!["missing"]
        );
    }

    #[test]
    fn accepts_methods_from_transitively_extended_interfaces() {
        let symbols = SymbolCatalog::from_program(&parse_l1(
            "INTERFACE if_base.\n\
               METHODS load.\n\
             ENDINTERFACE.\n\
             INTERFACE if_extended.\n\
               INTERFACES if_base.\n\
             ENDINTERFACE.\n\
             CLASS consumer DEFINITION.\n\
               PUBLIC SECTION.\n\
                 INTERFACES if_extended.\n\
             ENDCLASS.\n\
             CLASS consumer IMPLEMENTATION.\n\
               METHOD if_base~load.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        ))
        .unwrap();

        assert_eq!(
            symbols
                .get("consumer")
                .unwrap()
                .implemented_methods
                .iter()
                .collect::<Vec<_>>(),
            vec!["if_base~load"]
        );
    }
}
