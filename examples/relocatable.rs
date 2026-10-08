//! Building a relocatable object by hand: code that calls an external function and
//! loads a string, with the relocations a linker needs to finish it.
//!
//! ```text
//! cargo run --example relocatable -- hello.o
//! ```
//!
//! The result links with any ELF linker, for example
//! `ld.lld -static -e main hello.o other.o`.

use std::process::ExitCode;

use object_lang::{
    Architecture, Object, Relocation, RelocationTarget, Section, SectionFlags, SectionKind, Symbol,
    Visibility, X86_64Reloc,
};

fn build() -> Result<Object, object_lang::ModelError> {
    let mut obj = Object::relocatable(Architecture::X86_64);
    obj.add_symbol(Symbol::file("hello.rs"));

    // Mergeable C strings, the way compilers emit literals.
    let mut strings = Section::new(".rodata.str1.1", SectionKind::ReadOnlyData)
        .with_flags(SectionFlags::ALLOC | SectionFlags::MERGE | SectionFlags::STRINGS)
        .with_entry_size(1);
    let greeting = strings.append(b"hello, world\0", 1)?;
    let strings = obj.add_section(strings);

    // main:
    //   lea  rdi, [rip + greeting]   ; 48 8d 3d <pc32>
    //   jmp  puts                    ; e9 <plt32>
    let mut text = Section::new(".text", SectionKind::Text);
    let main = text.append(&[0x48, 0x8d, 0x3d, 0, 0, 0, 0, 0xe9, 0, 0, 0, 0], 16)?;
    let text = obj.add_section(text);

    obj.add_symbol(Symbol::function("main", text, main, 12).with_visibility(Visibility::Default));
    let puts = obj.add_symbol(Symbol::undefined("puts"));

    // The displacement is relative to the end of each instruction, hence the -4.
    obj.add_relocation(
        text,
        Relocation::new(
            main + 3,
            X86_64Reloc::Pc32,
            RelocationTarget::Section(strings),
            greeting as i64 - 4,
        ),
    )?;
    obj.add_relocation(
        text,
        Relocation::new(
            main + 8,
            X86_64Reloc::Plt32,
            RelocationTarget::Symbol(puts),
            -4,
        ),
    )?;
    Ok(obj)
}

fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("hello.o"));
    let obj = match build() {
        Ok(obj) => obj,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    let bytes = match object_lang::elf::write(&obj) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = std::fs::write(&path, &bytes) {
        eprintln!("error: cannot write {path}: {err}");
        return ExitCode::FAILURE;
    }
    println!("wrote {} bytes to {path}", bytes.len());
    ExitCode::SUCCESS
}
