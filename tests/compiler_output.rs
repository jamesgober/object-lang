//! The reader on real compiler output: objects clang 18 built from
//! `tests/fixtures/sample.c` (the command lines are in that file). These pin the reader
//! against a toolchain it did not write, and check that rewriting what it read gives an
//! object that reads back identically.

use object_lang::{
    Aarch64Reloc, Architecture, Binding, FileKind, Object, RelocationKind, RelocationTarget,
    SectionFlags, SectionKind, SymbolKind, SymbolSection, X86_64Reloc,
};

const X86_64: &[u8] = include_bytes!("fixtures/sample-x86_64.o");
const AARCH64: &[u8] = include_bytes!("fixtures/sample-aarch64.o");

fn read(bytes: &[u8]) -> Object {
    match object_lang::elf::read(bytes) {
        Ok(obj) => obj,
        Err(err) => panic!("clang output was refused: {err}"),
    }
}

fn section<'a>(obj: &'a Object, name: &str) -> &'a object_lang::Section {
    let id = obj
        .section_by_name(name)
        .unwrap_or_else(|| panic!("no section {name}"));
    &obj.sections()[id.index()]
}

fn symbol<'a>(obj: &'a Object, name: &str) -> &'a object_lang::Symbol {
    let id = obj
        .symbol_by_name(name)
        .unwrap_or_else(|| panic!("no symbol {name}"));
    &obj.symbols()[id.index()]
}

fn target_name(obj: &Object, target: RelocationTarget) -> String {
    match target {
        RelocationTarget::Symbol(id) => obj.symbols()[id.index()].name.clone(),
        RelocationTarget::Section(id) => obj.sections()[id.index()].name().to_owned(),
        _ => String::from("?"),
    }
}

fn check_common_shape(obj: &Object, arch: Architecture) {
    assert_eq!(obj.kind(), FileKind::Relocatable);
    assert_eq!(obj.architecture(), arch);
    // clang marks the stack non-executable.
    assert!(!obj.executable_stack());

    // .symtab, .strtab, .shstrtab, .rela.*, .note.GNU-stack and .llvm_addrsig are not
    // content sections; what is left is exactly the compiled contents.
    let mut names: Vec<&str> = obj.sections().iter().map(|s| s.name()).collect();
    names.sort_unstable();
    assert_eq!(names, [".bss", ".data", ".rodata.str1.1", ".text"]);

    let text = section(obj, ".text");
    assert_eq!(text.kind(), SectionKind::Text);
    assert!(
        text.flags()
            .contains(SectionFlags::ALLOC | SectionFlags::EXEC)
    );

    let strings = section(obj, ".rodata.str1.1");
    assert_eq!(strings.kind(), SectionKind::ReadOnlyData);
    assert_eq!(
        strings.flags(),
        SectionFlags::ALLOC | SectionFlags::MERGE | SectionFlags::STRINGS
    );
    assert_eq!(strings.entry_size(), 1);
    assert_eq!(strings.data(), b"alpha\0beta\0gamma\0");

    let bss = section(obj, ".bss");
    assert_eq!(bss.kind(), SectionKind::Bss);
    assert_eq!(bss.size(), 4);

    let data = section(obj, ".data");
    assert_eq!(data.kind(), SectionKind::Data);

    // Symbols: the file symbol and the static come first (locals), then the rest.
    assert_eq!(symbol(obj, "sample.c").kind, SymbolKind::File);
    let hits = symbol(obj, "hits");
    assert_eq!(
        (hits.binding, hits.kind, hits.size),
        (Binding::Local, SymbolKind::Data, 4)
    );
    let hook = symbol(obj, "hook");
    assert_eq!(
        (hook.binding, hook.kind),
        (Binding::Weak, SymbolKind::Function)
    );
    assert_eq!(symbol(obj, "entry").binding, Binding::Global);
    assert_eq!(symbol(obj, "puts").section, SymbolSection::Undefined);
    assert_eq!(
        symbol(obj, "shared_counter").section,
        SymbolSection::Undefined
    );
    let names_sym = symbol(obj, "names");
    assert_eq!((names_sym.kind, names_sym.size), (SymbolKind::Data, 24));
    let first_global = obj
        .symbols()
        .iter()
        .position(|s| s.binding != Binding::Local)
        .unwrap_or(obj.symbols().len());
    assert!(
        obj.symbols()[first_global..]
            .iter()
            .all(|s| s.binding != Binding::Local)
    );

    // The pointer table: three absolute pointers into the merged strings, through the
    // section symbol (which the reader turns into a section target).
    let table: Vec<(u64, String, i64)> = data
        .relocations()
        .iter()
        .map(|r| (r.offset, target_name(obj, r.target), r.addend))
        .collect();
    let base = names_sym.value;
    assert_eq!(
        table,
        [
            (base, String::from(".rodata.str1.1"), 0),
            (base + 8, String::from(".rodata.str1.1"), 6),
            (base + 16, String::from(".rodata.str1.1"), 11),
        ]
    );
}

fn check_rewrite_is_stable(obj: &Object) {
    let bytes = object_lang::elf::write(obj).unwrap_or_else(|e| panic!("rewrite failed: {e}"));
    let again = read(&bytes);
    assert_eq!(&again, obj, "rewriting clang's object changed it");
    assert_eq!(object_lang::elf::write(&again).ok(), Some(bytes));
}

#[test]
fn reads_clang_x86_64_object() {
    let obj = read(X86_64);
    check_common_shape(&obj, Architecture::X86_64);

    let text = section(&obj, ".text");
    let relocs: Vec<(u64, X86_64Reloc, String, i64)> = text
        .relocations()
        .iter()
        .map(|r| match r.kind {
            RelocationKind::X86_64(k) => (r.offset, k, target_name(&obj, r.target), r.addend),
            other => panic!("wrong-architecture relocation {other:?}"),
        })
        .collect();
    assert_eq!(
        relocs,
        [
            (0x15, X86_64Reloc::Pc32, String::from("shared_counter"), -4),
            (0x1e, X86_64Reloc::Pc32, String::from("table_size"), -4),
            (0x29, X86_64Reloc::Abs32Signed, String::from("names"), 0),
            (0x2e, X86_64Reloc::Plt32, String::from("puts"), -4),
            (0x34, X86_64Reloc::Pc32, String::from(".bss"), -4),
            (0x3b, X86_64Reloc::Plt32, String::from("hook"), -4),
        ]
    );
    check_rewrite_is_stable(&obj);
}

#[test]
fn reads_clang_aarch64_object() {
    let obj = read(AARCH64);
    check_common_shape(&obj, Architecture::Aarch64);

    let text = section(&obj, ".text");
    let relocs: Vec<(u64, Aarch64Reloc, String)> = text
        .relocations()
        .iter()
        .map(|r| match r.kind {
            RelocationKind::Aarch64(k) => (r.offset, k, target_name(&obj, r.target)),
            other => panic!("wrong-architecture relocation {other:?}"),
        })
        .collect();
    let expected = [
        (0x0c, Aarch64Reloc::AdrPrelPgHi21, "table_size"),
        (0x10, Aarch64Reloc::AdrPrelPgHi21, "shared_counter"),
        (0x18, Aarch64Reloc::Ldst32AbsLo12Nc, "table_size"),
        (0x1c, Aarch64Reloc::Ldst32AbsLo12Nc, "shared_counter"),
        (0x34, Aarch64Reloc::AdrPrelPgHi21, "names"),
        (0x38, Aarch64Reloc::AddAbsLo12Nc, "names"),
        (0x3c, Aarch64Reloc::Ldst32AbsLo12Nc, "shared_counter"),
        (0x44, Aarch64Reloc::Call26, "puts"),
        (0x48, Aarch64Reloc::AdrPrelPgHi21, ".bss"),
        (0x50, Aarch64Reloc::Ldst32AbsLo12Nc, ".bss"),
        (0x58, Aarch64Reloc::Ldst32AbsLo12Nc, ".bss"),
        (0x5c, Aarch64Reloc::Call26, "hook"),
    ];
    let expected: Vec<(u64, Aarch64Reloc, String)> = expected
        .iter()
        .map(|&(o, k, n)| (o, k, String::from(n)))
        .collect();
    assert_eq!(relocs, expected);
    check_rewrite_is_stable(&obj);
}

#[test]
fn every_truncation_of_compiler_output_is_refused_without_panicking() {
    for bytes in [X86_64, AARCH64] {
        for len in 0..bytes.len() {
            // Most prefixes cut a structure short; none may panic, and none may
            // decode, because the section header table sits at the end.
            assert!(
                object_lang::elf::read(&bytes[..len]).is_err(),
                "prefix of {len} bytes"
            );
        }
    }
}
