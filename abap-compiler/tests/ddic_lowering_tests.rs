use abap_compiler::ddic::{Ddic, DdicType};
use abap_compiler::inspect::abap::parse_l1;
use abap_compiler::lower::lower_program_with;

fn ddic() -> Ddic {
    Ddic {
        structs: vec![(
            "zt".to_string(),
            vec![
                ("name".to_string(), DdicType::Char(10)),
                ("menge".to_string(), DdicType::Int),
            ],
        )],
        elements: vec![("zmenge".to_string(), DdicType::Int)],
        skipped: vec![],
    }
}

#[test]
fn ddic_struct_is_usable_in_program() {
    let prog = parse_l1("REPORT zt.\nDATA ls TYPE zt.\nDATA lv TYPE zmenge.\nls-menge = 4.\nlv = ls-menge.\n");
    let ir = lower_program_with(&prog, &ddic()).expect("DDIC-Typen müssen bekannt sein");
    assert!(ir.structs.iter().any(|s| s.name == "zt" && s.fields.len() == 2));
}

#[test]
fn unknown_ddic_type_is_recorded_as_external_dependency() {
    let prog = parse_l1("REPORT zt.\nDATA ls TYPE zunbekannt.\n");
    // Unbekannte Typnamen gelten als externe DDIC-Typen und werden als Abhängigkeit vermerkt
    let ir = lower_program_with(&prog, &ddic()).expect("Übersetzung läuft weiter");
    assert!(ir.dependencies.iter().any(|d| d.contains("zunbekannt")));
}
