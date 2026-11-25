//! DDIC-Definitionen aus einem abapGit-Export: Strukturen (TABL) und Datenelemente (DTEL, DOMA).
//!
//! Nur Typen, die der Compiler abbilden kann, werden übernommen: `CHAR`, `NUMC`, `DATS`, `TIMS`,
//! `INT4`, `STRG`. Eine Struktur mit einem anderen Feldtyp (z.B. `DEC`) oder einem unbekannten
//! Datenelement wird nicht übernommen; der Grund steht in `Ddic::skipped`.

use std::collections::BTreeMap;

use crate::inspect::model::{xml_blocks, xml_value};

/// Typ eines DDIC-Felds, soweit der Compiler ihn abbildet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdicType {
    /// Zeichenfeld fester Länge (CHAR, NUMC, DATS, TIMS, ...)
    Char(usize),
    /// 4-Byte-Ganzzahl (INT4)
    Int,
    /// Variable Zeichenkette (STRG)
    Str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ddic {
    /// Strukturen (Name in Kleinbuchstaben) mit ihren Feldern in Reihenfolge
    pub structs: Vec<(String, Vec<(String, DdicType)>)>,
    /// Elementare Typen (Datenelemente) in Kleinbuchstaben
    pub elements: Vec<(String, DdicType)>,
    /// Übersprungene Objekte mit Grund
    pub skipped: Vec<String>,
}

/// Abbildung eines DDIC-Datentyps mit Länge auf einen Compilertyp.
fn base_type(datatype: &str, leng: usize) -> Option<DdicType> {
    match datatype {
        "CHAR" | "NUMC" | "CLNT" | "LANG" | "ACCP" => Some(DdicType::Char(leng)),
        "DATS" => Some(DdicType::Char(8)),
        "TIMS" => Some(DdicType::Char(6)),
        "STRG" => Some(DdicType::Str),
        "INT4" => Some(DdicType::Int),
        _ => None,
    }
}

fn leng(block: &str) -> usize {
    xml_value(block, "LENG")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Baut das DDIC aus den XML-Dateien eines Exports: `(Pfad, Inhalt)`.
pub fn from_files(files: &[(String, String)]) -> Ddic {
    let mut domains: BTreeMap<String, DdicType> = BTreeMap::new();
    let mut elements: BTreeMap<String, DdicType> = BTreeMap::new();
    let mut skipped = Vec::new();

    // Domänen: Datentyp und Länge
    for (path, xml) in files.iter().filter(|(p, _)| p.ends_with(".doma.xml")) {
        let Some(block) = xml_blocks(xml, "DD01V").first().copied() else {
            continue;
        };
        let name = xml_value(block, "DOMNAME").unwrap_or_default().to_lowercase();
        let datatype = xml_value(block, "DATATYPE").unwrap_or_default();
        match base_type(&datatype, leng(block)) {
            Some(t) => {
                domains.insert(name, t);
            }
            None => skipped.push(format!("{path}: Domäne {name} hat Typ {datatype}, der nicht unterstützt wird")),
        }
    }

    // Datenelemente: direkter Datentyp oder über die Domäne
    for (path, xml) in files.iter().filter(|(p, _)| p.ends_with(".dtel.xml")) {
        let Some(block) = xml_blocks(xml, "DD04V").first().copied() else {
            continue;
        };
        let name = xml_value(block, "ROLLNAME").unwrap_or_default().to_lowercase();
        let datatype = xml_value(block, "DATATYPE").unwrap_or_default();
        let ty = if datatype.is_empty() {
            let dom = xml_value(block, "DOMNAME").unwrap_or_default().to_lowercase();
            domains.get(&dom).cloned()
        } else {
            base_type(&datatype, leng(block))
        };
        match ty {
            Some(t) => {
                elements.insert(name, t);
            }
            None if datatype.is_empty() => {
                let dom = xml_value(block, "DOMNAME").unwrap_or_default();
                skipped.push(format!("{path}: Datenelement {name} verweist auf Domäne {dom}, die nicht im Export liegt"));
            }
            None => skipped.push(format!("{path}: Datenelement {name} hat Typ {datatype}, der nicht unterstützt wird")),
        }
    }

    // Strukturen: nur wenn alle Felder abbildbar sind
    let mut structs = Vec::new();
    for (path, xml) in files.iter().filter(|(p, _)| p.ends_with(".tabl.xml")) {
        let Some(head) = xml_blocks(xml, "DD02V").first().copied() else {
            continue;
        };
        let name = xml_value(head, "TABNAME").unwrap_or_default().to_lowercase();
        let mut fields = Vec::new();
        let mut reason: Option<String> = None;
        for block in xml_blocks(xml, "DD03P") {
            let field = xml_value(block, "FIELDNAME").unwrap_or_default();
            if field.starts_with('.') || xml_value(block, "COMPTYPE").as_deref() == Some("S") {
                reason = Some("enthält eine INCLUDE-Struktur".into());
                break;
            }
            let ty = match xml_value(block, "DATATYPE") {
                Some(dt) => base_type(&dt, leng(block)),
                None => xml_value(block, "ROLLNAME")
                    .and_then(|r| elements.get(&r.to_lowercase()).cloned()),
            };
            match ty {
                Some(t) => fields.push((field.to_lowercase(), t)),
                None => {
                    reason = Some(format!("Feld {field} hat einen nicht unterstützten Typ"));
                    break;
                }
            }
        }
        match reason {
            None if !fields.is_empty() => structs.push((name, fields)),
            None => {}
            Some(r) => skipped.push(format!("{path}: {r}")),
        }
    }

    Ddic {
        structs,
        elements: elements.into_iter().collect(),
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABL: &str = "<DD02V><TABNAME>ZT</TABNAME></DD02V>\
<DD03P_TABLE><DD03P><FIELDNAME>MANDT</FIELDNAME><ROLLNAME>MANDT</ROLLNAME></DD03P>\
<DD03P><FIELDNAME>MENGE</FIELDNAME><ROLLNAME>ZMENGE</ROLLNAME></DD03P>\
<DD03P><FIELDNAME>NAME</FIELDNAME><DATATYPE>CHAR</DATATYPE><LENG>000020</LENG></DD03P></DD03P_TABLE>";

    #[test]
    fn struct_with_char_and_element_fields() {
        let files = vec![
            ("zt.tabl.xml".into(), TABL.to_string()),
            (
                "zmenge.dtel.xml".into(),
                "<DD04V><ROLLNAME>ZMENGE</ROLLNAME><DATATYPE>INT4</DATATYPE><LENG>000010</LENG></DD04V>".into(),
            ),
            (
                "mandt.dtel.xml".into(),
                "<DD04V><ROLLNAME>MANDT</ROLLNAME><DATATYPE>CLNT</DATATYPE><LENG>000003</LENG></DD04V>".into(),
            ),
        ];
        let d = from_files(&files);
        assert!(d.skipped.is_empty(), "{:?}", d.skipped);
        let (name, fields) = &d.structs[0];
        assert_eq!(name, "zt");
        assert_eq!(
            fields,
            &vec![
                ("mandt".to_string(), DdicType::Char(3)),
                ("menge".to_string(), DdicType::Int),
                ("name".to_string(), DdicType::Char(20)),
            ]
        );
    }

    #[test]
    fn unsupported_field_skips_whole_struct_with_reason() {
        let tabl = "<DD02V><TABNAME>ZD</TABNAME></DD02V>\
<DD03P_TABLE><DD03P><FIELDNAME>PREIS</FIELDNAME><DATATYPE>DEC</DATATYPE><LENG>000010</LENG></DD03P></DD03P_TABLE>";
        let d = from_files(&[("zd.tabl.xml".into(), tabl.into())]);
        assert!(d.structs.is_empty());
        assert_eq!(d.skipped.len(), 1);
        assert!(d.skipped[0].contains("PREIS"));
    }

    #[test]
    fn element_via_domain() {
        let files = vec![
            (
                "zdom.doma.xml".into(),
                "<DD01V><DOMNAME>ZDOM</DOMNAME><DATATYPE>CHAR</DATATYPE><LENG>000002</LENG></DD01V>".into(),
            ),
            (
                "zel.dtel.xml".into(),
                "<DD04V><ROLLNAME>ZEL</ROLLNAME><DOMNAME>ZDOM</DOMNAME></DD04V>".into(),
            ),
        ];
        let d = from_files(&files);
        assert_eq!(d.elements, vec![("zel".to_string(), DdicType::Char(2))]);
    }
}
