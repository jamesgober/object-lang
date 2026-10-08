//! A small `readelf`: prints what the reader makes of an ELF file.
//!
//! ```text
//! cargo run --example inspect -- path/to/file.o
//! ```

use std::process::ExitCode;

use object_lang::{RelocationTarget, SymbolSection};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: inspect <elf-file>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("error: cannot read {path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let obj = match object_lang::elf::read(&bytes) {
        Ok(obj) => obj,
        Err(err) => {
            eprintln!("error: {path}: {err}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "{path}: {:?} {:?}, entry {:#x}, executable stack: {}",
        obj.kind(),
        obj.architecture(),
        obj.entry(),
        obj.executable_stack()
    );
    println!("\nsections:");
    for (i, s) in obj.sections().iter().enumerate() {
        println!(
            "  [{i:>3}] {:<24} {:<14?} {:?} addr {:#x} size {} align {} relocations {}",
            s.name(),
            s.kind(),
            s.flags(),
            s.address(),
            s.size(),
            s.align(),
            s.relocations().len()
        );
    }
    println!("\nsymbols:");
    for (i, sym) in obj.symbols().iter().enumerate() {
        let place = match sym.section {
            SymbolSection::Section(id) => obj.sections()[id.index()].name().to_owned(),
            other => format!("{other:?}"),
        };
        println!(
            "  [{i:>3}] {:<32} {:?} {:?} {:?} in {place} value {:#x} size {}",
            sym.name, sym.kind, sym.binding, sym.visibility, sym.value, sym.size
        );
    }
    for s in obj.sections() {
        if s.relocations().is_empty() {
            continue;
        }
        println!("\nrelocations in {}:", s.name());
        for r in s.relocations() {
            let target = match r.target {
                RelocationTarget::Symbol(id) => obj.symbols()[id.index()].name.clone(),
                RelocationTarget::Section(id) => {
                    format!("section {}", obj.sections()[id.index()].name())
                }
                other => format!("{other:?}"),
            };
            println!("  {:#08x} {:?} {target} {:+}", r.offset, r.kind, r.addend);
        }
    }
    ExitCode::SUCCESS
}
