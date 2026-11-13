//! abapc: übersetzt ein ABAP-Report-Programm in eine ausführbare Datei.
//!
//! Aufruf: abapc <prog.abap> -o <exe> [--emit-ir <datei.ll>] [--runtime <libabap_rt.a>]

use std::path::{Path, PathBuf};
use std::process::{self, Command};

use abap_compiler::{codegen::Codegen, lower::lower_source};
use inkwell::context::Context;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::OptimizationLevel;

struct Args {
    input: PathBuf,
    output: PathBuf,
    emit_ir: Option<PathBuf>,
    runtime: Option<PathBuf>,
}

fn usage() -> ! {
    eprintln!("Verwendung: abapc <prog.abap> -o <exe> [--emit-ir <datei.ll>] [--runtime <libabap_rt.a>]");
    process::exit(2);
}

fn parse_args() -> Args {
    let mut input = None;
    let mut output = None;
    let mut emit_ir = None;
    let mut runtime = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => output = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "--emit-ir" => emit_ir = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "--runtime" => runtime = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "-h" | "--help" => usage(),
            _ if input.is_none() => input = Some(PathBuf::from(a)),
            _ => usage(),
        }
    }
    Args {
        input: input.unwrap_or_else(|| usage()),
        output: output.unwrap_or_else(|| usage()),
        emit_ir,
        runtime,
    }
}

/// Standard: libabap_rt.a liegt neben dem abapc-Binary.
fn default_runtime() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("libabap_rt.a")))
        .unwrap_or_else(|| PathBuf::from("libabap_rt.a"))
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("abapc: {msg}");
    process::exit(1);
}

fn emit_object(module: &inkwell::module::Module, path: &Path) -> Result<(), String> {
    Target::initialize_native(&InitializationConfig::default()).map_err(|e| e.to_string())?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    let cpu = TargetMachine::get_host_cpu_name();
    let features = TargetMachine::get_host_cpu_features();
    let tm = target
        .create_target_machine(
            &triple,
            cpu.to_str().unwrap_or("generic"),
            features.to_str().unwrap_or(""),
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or("Zielmaschine konnte nicht erstellt werden")?;
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    tm.write_to_file(module, FileType::Object, path)
        .map_err(|e| e.to_string())
}

fn link(obj: &Path, runtime: &Path, exe: &Path) -> Result<(), String> {
    let status = Command::new("cc")
        .arg(obj)
        .arg(runtime)
        .arg("-o")
        .arg(exe)
        .args(["-lpthread", "-ldl", "-lm", "-lgcc_s"])
        .status()
        .map_err(|e| format!("Linker 'cc' nicht startbar: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("Linken fehlgeschlagen".into())
    }
}

fn main() {
    let args = parse_args();
    let src = std::fs::read_to_string(&args.input)
        .unwrap_or_else(|e| fail(format!("{}: {e}", args.input.display())));

    let ir = match lower_source(&src) {
        Ok(ir) => ir,
        Err(e) => fail(e),
    };

    let ctx = Context::create();
    let mut cg = Codegen::new(&ctx);
    if let Err(e) = cg.emit_program(&ir) {
        fail(e);
    }
    if let Some(ll) = &args.emit_ir {
        if let Err(e) = cg.module.print_to_file(ll) {
            fail(format!("IR-Datei nicht schreibbar: {e}"));
        }
    }

    let obj = args.output.with_extension("o");
    if let Err(e) = emit_object(&cg.module, &obj) {
        fail(format!("Objektdatei: {e}"));
    }
    let runtime = args.runtime.clone().unwrap_or_else(default_runtime);
    if !runtime.exists() {
        fail(format!("Laufzeitbibliothek nicht gefunden: {}", runtime.display()));
    }
    if let Err(e) = link(&obj, &runtime, &args.output) {
        fail(e);
    }
    let _ = std::fs::remove_file(&obj);
}
