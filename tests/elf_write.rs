//! The ELF writer: known-answer header fields, symbol-table rules, extended section
//! numbering, every model rule's error, and scale.

#![allow(clippy::unwrap_used, reason = "test failures should be loud")]

mod common;

use object_lang::{
    Aarch64Reloc, Architecture, Binding, FileKind, ModelError, Object, Relocation,
    RelocationProblem, RelocationTarget, Section, SectionFlags, SectionId, SectionKind,
    SectionProblem, Symbol, SymbolId, SymbolKind, SymbolProblem, SymbolSection, Visibility,
    WriteError, X86_64Reloc,
};

// ---------------------------------------------------------------------------
// Known answers
// ---------------------------------------------------------------------------

#[test]
fn empty_relocatable_object_is_byte_exact() {
    let bytes = object_lang::elf::write(&Object::relocatable(Architecture::X86_64)).unwrap();
    // e_ident: magic, ELFCLASS64, ELFDATA2LSB, EV_CURRENT, ELFOSABI_NONE, ABI 0, padding.
    assert_eq!(
        &bytes[..16],
        b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00"
    );
    assert_eq!(common::u16_at(&bytes, 0x10), 1, "e_type = ET_REL");
    assert_eq!(common::u16_at(&bytes, 0x12), 62, "e_machine = EM_X86_64");
    assert_eq!(common::u32_at(&bytes, 0x14), 1, "e_version");
    assert_eq!(common::u64_at(&bytes, 0x18), 0, "e_entry");
    assert_eq!(common::u64_at(&bytes, 0x20), 0, "e_phoff");
    assert_eq!(common::u32_at(&bytes, 0x30), 0, "e_flags");
    assert_eq!(common::u16_at(&bytes, 0x34), 64, "e_ehsize");
    assert_eq!(common::u16_at(&bytes, 0x36), 0, "e_phentsize");
    assert_eq!(common::u16_at(&bytes, 0x38), 0, "e_phnum");
    assert_eq!(common::u16_at(&bytes, 0x3a), 64, "e_shentsize");
    // null, .note.GNU-stack, .symtab, .strtab, .shstrtab
    assert_eq!(common::u16_at(&bytes, 0x3c), 5, "e_shnum");
    assert_eq!(common::u16_at(&bytes, 0x3e), 4, "e_shstrndx");
    // Layout: header (64) | .symtab at 64 (24) | .strtab at 88 (1) | .shstrtab at 89
    // ("\0.note.GNU-stack\0.symtab\0.strtab\0.shstrtab\0" = 43) | pad to 136 | 5 headers.
    assert_eq!(common::u64_at(&bytes, 0x28), 136, "e_shoff");
    assert_eq!(bytes.len(), 136 + 5 * 64);

    let elf = common::parse(&bytes);
    let names: Vec<&str> = elf.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["", ".note.GNU-stack", ".symtab", ".strtab", ".shstrtab"]
    );
    let symtab = &elf.sections[2];
    assert_eq!((symtab.ty, symtab.offset, symtab.size), (2, 64, 24));
    assert_eq!(
        (symtab.link, symtab.info, symtab.align, symtab.entsize),
        (3, 1, 8, 24)
    );
    let note = &elf.sections[1];
    assert_eq!((note.ty, note.flags, note.size), (1, 0, 0));
}

#[test]
fn aarch64_machine_and_executable_stack_marker() {
    let mut obj = Object::relocatable(Architecture::Aarch64);
    obj.set_executable_stack(true);
    let bytes = object_lang::elf::write(&obj).unwrap();
    assert_eq!(common::u16_at(&bytes, 0x12), 183, "e_machine = EM_AARCH64");
    let elf = common::parse(&bytes);
    let note = &elf.sections[elf.index_of(".note.GNU-stack").unwrap()];
    assert_eq!(note.flags, 0x4, "SHF_EXECINSTR marks an executable stack");
    assert!(object_lang::elf::read(&bytes).unwrap().executable_stack());
}

#[test]
fn exit42_executable_is_byte_exact() {
    let code = [0xbf, 0x2a, 0, 0, 0, 0xb8, 0x3c, 0, 0, 0, 0x0f, 0x05];
    let bytes = object_lang::elf::executable(Architecture::X86_64, &code).unwrap();
    assert_eq!(common::u16_at(&bytes, 0x10), 2, "e_type = ET_EXEC");
    assert_eq!(common::u64_at(&bytes, 0x18), 0x40_1000, "e_entry");
    assert_eq!(common::u64_at(&bytes, 0x20), 64, "e_phoff");
    assert_eq!(common::u16_at(&bytes, 0x36), 56, "e_phentsize");
    assert_eq!(common::u16_at(&bytes, 0x38), 3, "e_phnum");
    // Headers: 64 + 3 * 56 = 232 bytes; the code starts on the next page in the file.
    let elf = common::parse(&bytes);
    let load = |i: usize| elf.segments[i];
    assert_eq!(
        (
            load(0).ty,
            load(0).flags,
            load(0).offset,
            load(0).vaddr,
            load(0).filesz,
            load(0).memsz,
            load(0).align
        ),
        (1, 4, 0, 0x40_0000, 232, 232, 0x1000)
    );
    assert_eq!(
        (
            load(1).ty,
            load(1).flags,
            load(1).offset,
            load(1).vaddr,
            load(1).filesz,
            load(1).memsz,
            load(1).align
        ),
        (1, 5, 0x1000, 0x40_1000, 12, 12, 0x1000)
    );
    assert_eq!(load(1).paddr, load(1).vaddr);
    assert_eq!(
        (load(2).ty, load(2).flags, load(2).align),
        (0x6474_e551, 6, 16)
    );
    assert_eq!(&bytes[0x1000..0x100c], &code);
    // .text, .symtab (null + _start), .strtab, .shstrtab after the code.
    let names: Vec<&str> = elf.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["", ".text", ".symtab", ".strtab", ".shstrtab"]);
    assert_eq!(
        (elf.sections[1].addr, elf.sections[1].offset),
        (0x40_1000, 0x1000)
    );
    assert_eq!(bytes.len(), 4528);
}

#[test]
fn aarch64_executables_use_64k_pages() {
    let bytes = object_lang::elf::executable(Architecture::Aarch64, &[0, 0, 0, 0x14]).unwrap();
    let elf = common::parse(&bytes);
    assert!(
        elf.segments
            .iter()
            .filter(|s| s.ty == 1)
            .all(|s| s.align == 0x1_0000)
    );
    assert_eq!(elf.segments[1].vaddr, 0x41_0000);
    assert_eq!(elf.segments[1].offset % 0x1_0000, 0);
}

// ---------------------------------------------------------------------------
// Symbol table rules
// ---------------------------------------------------------------------------

#[test]
fn locals_come_first_and_sh_info_points_past_them() {
    let mut obj = Object::relocatable(Architecture::X86_64);
    let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0x90; 16]));
    let data = obj.add_section(Section::new(".data", SectionKind::Data).with_data(vec![0; 8]));
    obj.add_symbol(Symbol::function("g1", text, 0, 4));
    obj.add_symbol(Symbol::function("l1", text, 4, 4).with_binding(Binding::Local));
    obj.add_symbol(Symbol::undefined("w1").with_binding(Binding::Weak));
    obj.add_symbol(Symbol::file("x.c"));
    obj.add_symbol(Symbol::data("l2", data, 0, 8).with_binding(Binding::Local));
    // A relocation against .data itself needs a section symbol.
    obj.add_relocation(
        text,
        Relocation::new(8, X86_64Reloc::Pc32, RelocationTarget::Section(data), -4),
    )
    .unwrap();

    let bytes = object_lang::elf::write(&obj).unwrap();
    let elf = common::parse(&bytes);
    let symbols = elf.symbols(&bytes);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    // null, the .data section symbol, locals in order, then non-locals in order.
    assert_eq!(names, ["", "", "l1", "x.c", "l2", "g1", "w1"]);
    assert_eq!(symbols[1].info, 3, "STT_SECTION, STB_LOCAL");
    assert_eq!(symbols[1].shndx, 2, ".data is header 2");
    let symtab = elf.sections.iter().find(|s| s.ty == 2).unwrap();
    assert_eq!(symtab.info, 5);

    // The relocation names the section symbol, and the reader turns it back into a
    // section target.
    let relocs = elf.relocations(&bytes);
    assert_eq!(relocs[&1][0].sym, 1);
    assert_eq!(relocs[&1][0].ty, 2, "R_X86_64_PC32");
    let back = object_lang::elf::read(&bytes).unwrap();
    assert_eq!(
        back.sections()[0].relocations()[0].target,
        RelocationTarget::Section(SectionId::new(1))
    );
    let back_names: Vec<&str> = back.symbols().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(back_names, ["l1", "x.c", "l2", "g1", "w1"]);
}

#[test]
fn symbol_fields_are_encoded_per_the_gabi() {
    let mut obj = Object::relocatable(Architecture::X86_64);
    let tdata = obj.add_section(
        Section::new(".tdata", SectionKind::Data)
            .with_flags(SectionKind::Data.default_flags() | SectionFlags::TLS)
            .with_data(vec![0; 8]),
    );
    obj.add_symbol(Symbol::new(
        "t",
        SymbolKind::Tls,
        Binding::Global,
        SymbolSection::Section(tdata),
        4,
        4,
    ));
    obj.add_symbol(Symbol::common("c", 64, 16));
    obj.add_symbol(Symbol::absolute("a", 0xdead_beef).with_visibility(Visibility::Hidden));
    obj.add_symbol(Symbol::undefined("p").with_visibility(Visibility::Protected));
    let bytes = object_lang::elf::write(&obj).unwrap();
    let symbols = common::parse(&bytes).symbols(&bytes);
    let get = |n: &str| symbols.iter().find(|s| s.name == n).unwrap().clone();
    let t = get("t");
    assert_eq!(
        (t.info, t.other, t.shndx, t.value, t.size),
        (0x16, 0, 1, 4, 4)
    );
    let c = get("c");
    assert_eq!((c.info, c.shndx, c.value, c.size), (0x11, 0xfff2, 16, 64));
    let a = get("a");
    assert_eq!(
        (a.info, a.other, a.shndx, a.value),
        (0x10, 2, 0xfff1, 0xdead_beef)
    );
    let p = get("p");
    assert_eq!((p.info, p.other, p.shndx), (0x10, 3, 0));
}

#[test]
fn string_tables_share_suffixes() {
    let mut obj = Object::relocatable(Architecture::X86_64);
    for name in ["foobar", "bar", "ar", "bar"] {
        obj.add_symbol(Symbol::undefined(name));
    }
    let bytes = object_lang::elf::write(&obj).unwrap();
    let elf = common::parse(&bytes);
    let strtab = elf.sections.iter().find(|s| s.name == ".strtab").unwrap();
    assert_eq!(
        elf.contents(&bytes, elf.index_of(".strtab").unwrap()),
        b"\0foobar\0"
    );
    assert_eq!(strtab.size, 8);
    let names: Vec<String> = elf.symbols(&bytes).into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["", "foobar", "bar", "ar", "bar"]);
}

#[test]
fn relocation_sections_are_named_linked_and_typed() {
    let mut obj = Object::relocatable(Architecture::Aarch64);
    let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0; 8]));
    let callee = obj.add_symbol(Symbol::undefined("callee"));
    obj.add_relocation(
        text,
        Relocation::new(4, Aarch64Reloc::Call26, RelocationTarget::Symbol(callee), 0),
    )
    .unwrap();
    obj.add_relocation(
        text,
        Relocation::new(
            0,
            Aarch64Reloc::AdrPrelPgHi21,
            RelocationTarget::Symbol(callee),
            -8,
        ),
    )
    .unwrap();
    let bytes = object_lang::elf::write(&obj).unwrap();
    let elf = common::parse(&bytes);
    let rela = elf.sections.iter().find(|s| s.ty == 4).unwrap();
    assert_eq!(rela.name, ".rela.text");
    assert_eq!(
        (rela.flags, rela.info, rela.entsize, rela.align, rela.size),
        (0x40, 1, 24, 8, 48)
    );
    assert_eq!(elf.sections[rela.link as usize].name, ".symtab");
    let relocs = &elf.relocations(&bytes)[&1];
    assert_eq!(
        (relocs[0].offset, relocs[0].ty, relocs[0].addend),
        (4, 283, 0)
    );
    assert_eq!(
        (relocs[1].offset, relocs[1].ty, relocs[1].addend),
        (0, 275, -8)
    );
}

// ---------------------------------------------------------------------------
// Extended numbering and scale
// ---------------------------------------------------------------------------

#[test]
fn extended_section_numbering_round_trips() {
    // More sections than a 16-bit e_shnum holds: the count, the name-table index, and
    // the symbols' section indices all need ELF's escapes.
    const COUNT: u32 = 70_000;
    let mut obj = Object::relocatable(Architecture::X86_64);
    for i in 0..COUNT {
        let id = obj.add_section(
            Section::new(format!(".text.f{i}"), SectionKind::Text).with_data(vec![0xc3; 8]),
        );
        obj.add_symbol(Symbol::function(format!("f{i}"), id, 0, 1));
    }
    let last = SectionId::new(COUNT - 1);
    let first = SectionId::new(0);
    obj.add_relocation(
        first,
        Relocation::new(0, X86_64Reloc::Pc32, RelocationTarget::Section(last), -4),
    )
    .unwrap();
    obj.add_relocation(
        last,
        Relocation::new(
            4,
            X86_64Reloc::Plt32,
            RelocationTarget::Symbol(SymbolId::new(0)),
            -4,
        ),
    )
    .unwrap();

    let bytes = object_lang::elf::write(&obj).unwrap();
    assert_eq!(common::u16_at(&bytes, 0x3c), 0, "e_shnum escaped");
    assert_eq!(common::u16_at(&bytes, 0x3e), 0xffff, "e_shstrndx escaped");
    let elf = common::parse(&bytes);
    assert_eq!(elf.sections[0].size as usize, elf.sections.len());
    assert_eq!(elf.sections[0].link as usize, elf.shstrndx);
    assert!(
        elf.sections.iter().any(|s| s.ty == 18),
        ".symtab_shndx present"
    );
    let symbols = elf.symbols(&bytes);
    let high = symbols.iter().find(|s| s.name == "f69999").unwrap();
    assert_eq!(high.shndx, COUNT, "resolved through .symtab_shndx");

    let back = object_lang::elf::read(&bytes).unwrap();
    assert_eq!(back, obj);
    assert_eq!(object_lang::elf::write(&back).unwrap(), bytes);
}

#[test]
fn large_object_round_trips() {
    // 100k symbols, 200k relocations in 1,000 sections.
    let mut obj = Object::relocatable(Architecture::Aarch64);
    let mut ids = Vec::new();
    for i in 0..1000 {
        ids.push(obj.add_section(
            Section::new(format!(".text.{i}"), SectionKind::Text).with_data(vec![0; 1600]),
        ));
    }
    for i in 0..100_000u32 {
        let binding = if i % 3 == 0 {
            Binding::Local
        } else {
            Binding::Global
        };
        let id = ids[(i % 1000) as usize];
        obj.add_symbol(
            Symbol::function(format!("sym_{i:06}"), id, u64::from(i % 400) * 4, 4)
                .with_binding(binding),
        );
    }
    for i in 0..200_000u32 {
        let id = ids[(i % 1000) as usize];
        let target = RelocationTarget::Symbol(SymbolId::new((i * 7) % 100_000));
        obj.add_relocation(
            id,
            Relocation::new(
                u64::from((i / 1000) % 400) * 4,
                Aarch64Reloc::Call26,
                target,
                0,
            ),
        )
        .unwrap();
    }
    let bytes = object_lang::elf::write(&obj).unwrap();
    let back = object_lang::elf::read(&bytes).unwrap();
    assert_eq!(back.symbols().len(), 100_000);
    assert_eq!(
        back.sections()
            .iter()
            .map(|s| s.relocations().len())
            .sum::<usize>(),
        200_000
    );
    assert_eq!(object_lang::elf::write(&back).unwrap(), bytes);
}

// ---------------------------------------------------------------------------
// Errors: every model rule, and every writer error
// ---------------------------------------------------------------------------

fn section_error(section: Section) -> ModelError {
    let mut obj = Object::relocatable(Architecture::X86_64);
    obj.add_section(section);
    obj.validate().unwrap_err()
}

fn section_problem(section: Section) -> SectionProblem {
    match section_error(section) {
        ModelError::Section { problem, .. } => problem,
        other => panic!("expected a section problem, got {other:?}"),
    }
}

#[test]
fn every_section_rule_is_enforced() {
    use SectionKind as K;
    assert_eq!(
        section_problem(Section::new("a\0b", K::Data)),
        SectionProblem::NameContainsNul
    );
    assert_eq!(
        section_problem(Section::new("x", K::Data).with_align(0)),
        SectionProblem::BadAlignment
    );
    assert_eq!(
        section_problem(Section::new("x", K::Data).with_align(24)),
        SectionProblem::BadAlignment
    );
    for (kind, flags) in [
        (K::Text, SectionFlags::ALLOC),
        (
            K::Text,
            SectionFlags::ALLOC | SectionFlags::EXEC | SectionFlags::TLS,
        ),
        (
            K::Data,
            SectionFlags::ALLOC | SectionFlags::WRITE | SectionFlags::EXEC,
        ),
        (K::ReadOnlyData, SectionFlags::ALLOC | SectionFlags::WRITE),
        (K::ReadOnlyData, SectionFlags::empty()),
        (K::Bss, SectionFlags::empty()),
        (K::Note, SectionFlags::WRITE),
        (K::InitArray, SectionFlags::ALLOC),
        (K::Other, SectionFlags::ALLOC),
    ] {
        assert_eq!(
            section_problem(Section::new("x", kind).with_flags(flags)),
            SectionProblem::FlagsDoNotMatchKind,
            "{kind:?} with {flags:?}"
        );
    }
    let merge = SectionFlags::ALLOC | SectionFlags::MERGE;
    assert_eq!(
        section_problem(Section::new("x", K::ReadOnlyData).with_flags(merge)),
        SectionProblem::MergeWithoutEntrySize
    );
    assert_eq!(
        section_problem(Section::new("x", K::Bss).with_data(vec![1])),
        SectionProblem::ContentsDoNotMatchKind
    );
    assert_eq!(
        section_problem(Section::new("x", K::Data).with_bss_size(4)),
        SectionProblem::ContentsDoNotMatchKind
    );
    assert_eq!(
        section_problem(Section::new("x", K::Data).with_align(16).with_address(8)),
        SectionProblem::MisalignedAddress
    );
    assert_eq!(
        section_problem(
            Section::new("x", K::Bss)
                .with_address(u64::MAX)
                .with_bss_size(2)
        ),
        SectionProblem::AddressOverflow
    );
}

#[test]
fn section_builders_refuse_bad_requests() {
    let mut bss = Section::new(".bss", SectionKind::Bss);
    assert_eq!(bss.append(&[1], 1), Err(ModelError::AppendToBss));
    assert_eq!(
        bss.reserve(1, 3),
        Err(ModelError::InvalidAlignment { align: 3 })
    );
    assert_eq!(bss.reserve(u64::MAX, 1), Ok(0));
    assert_eq!(bss.reserve(1, 1), Err(ModelError::SizeOverflow));
    let mut data = Section::new(".data", SectionKind::Data);
    assert_eq!(
        data.append(&[1], 0),
        Err(ModelError::InvalidAlignment { align: 0 })
    );
    assert_eq!(data.reserve(u64::MAX, 1), Err(ModelError::SizeOverflow));
    assert_eq!(data.reserve(3, 4), Ok(0));
    assert_eq!(data.data(), &[0, 0, 0]);
    let mut obj = Object::relocatable(Architecture::X86_64);
    let reloc = Relocation::new(
        0,
        X86_64Reloc::Abs64,
        RelocationTarget::Section(SectionId::new(0)),
        0,
    );
    assert_eq!(
        obj.add_relocation(SectionId::new(0), reloc),
        Err(ModelError::UnknownSection {
            section: SectionId::new(0)
        })
    );
}

fn symbol_problem(obj: &mut Object, symbol: Symbol) -> SymbolProblem {
    obj.add_symbol(symbol);
    match obj.validate().unwrap_err() {
        ModelError::Symbol { problem, .. } => problem,
        other => panic!("expected a symbol problem, got {other:?}"),
    }
}

#[test]
fn every_symbol_rule_is_enforced() {
    let base = || {
        let mut obj = Object::relocatable(Architecture::X86_64);
        obj.add_section(Section::new(".data", SectionKind::Data).with_data(vec![0; 8]));
        obj
    };
    let data = SectionId::new(0);
    let p = |s: Symbol| symbol_problem(&mut base(), s);
    assert_eq!(p(Symbol::undefined("a\0")), SymbolProblem::NameContainsNul);
    assert_eq!(
        p(Symbol::data("x", SectionId::new(5), 0, 0)),
        SymbolProblem::UnknownSection
    );
    assert_eq!(
        p(Symbol::data("x", data, 9, 0)),
        SymbolProblem::OutOfSection
    );
    assert_eq!(
        p(Symbol::data("x", data, 4, 5)),
        SymbolProblem::OutOfSection
    );
    assert_eq!(
        p(Symbol::data("x", data, 1, u64::MAX)),
        SymbolProblem::OutOfSection
    );
    assert_eq!(
        p(Symbol::undefined("x").with_binding(Binding::Local)),
        SymbolProblem::LocalUndefined
    );
    assert_eq!(
        p(Symbol::common("x", 4, 4).with_binding(Binding::Local)),
        SymbolProblem::LocalUndefined
    );
    assert_eq!(
        p(Symbol::file("f").with_binding(Binding::Global)),
        SymbolProblem::BadFileSymbol
    );
    let mut file = Symbol::file("f");
    file.value = 1;
    assert_eq!(p(file), SymbolProblem::BadFileSymbol);
    assert_eq!(p(Symbol::common("x", 4, 3)), SymbolProblem::BadCommonSymbol);
    assert_eq!(
        p(Symbol::common("x", 4, 4).with_binding(Binding::Weak)),
        SymbolProblem::BadCommonSymbol
    );
    assert_eq!(
        p(Symbol::data("x", data, 0, 4).with_kind(SymbolKind::Tls)),
        SymbolProblem::TlsOutsideTlsSection
    );
    // A symbol exactly at the end of its section is fine.
    let mut ok = base();
    ok.add_symbol(Symbol::data("end", data, 8, 0));
    assert!(ok.validate().is_ok());

    let exe = || {
        let mut obj = Object::executable(Architecture::X86_64);
        obj.add_section(
            Section::new(".text", SectionKind::Text)
                .with_address(0x40_1000)
                .with_data(vec![0xc3]),
        );
        obj.set_entry(0x40_1000);
        obj
    };
    assert_eq!(
        symbol_problem(&mut exe(), Symbol::common("x", 4, 4)),
        SymbolProblem::CommonInExecutable
    );
    assert_eq!(
        symbol_problem(&mut exe(), Symbol::undefined("x")),
        SymbolProblem::UndefinedInExecutable
    );
    // Executable symbols are addresses: offset 0 is outside a section at 0x401000.
    assert_eq!(
        symbol_problem(&mut exe(), Symbol::function("f", SectionId::new(0), 0, 1)),
        SymbolProblem::OutOfSection
    );
    let mut weak = exe();
    weak.add_symbol(Symbol::undefined("x").with_binding(Binding::Weak));
    weak.add_symbol(Symbol::function("f", SectionId::new(0), 0x40_1000, 1));
    assert!(weak.validate().is_ok());
}

fn relocation_problem(
    arch: Architecture,
    kind: FileKind,
    reloc: Relocation,
    bss: bool,
) -> RelocationProblem {
    let mut obj = Object::new(kind, arch);
    let section = if bss {
        Section::new(".bss", SectionKind::Bss).with_bss_size(16)
    } else {
        Section::new(".text", SectionKind::Text)
            .with_address(0x40_1000)
            .with_data(vec![0; 16])
    };
    let id = obj.add_section(section);
    obj.set_entry(0x40_1000);
    obj.add_symbol(Symbol::undefined("x").with_binding(Binding::Weak));
    obj.add_relocation(id, reloc).unwrap();
    match obj.validate().unwrap_err() {
        ModelError::Relocation {
            problem, index: 0, ..
        } => problem,
        other => panic!("expected a relocation problem, got {other:?}"),
    }
}

#[test]
fn every_relocation_rule_is_enforced() {
    let sym = RelocationTarget::Symbol(SymbolId::new(0));
    let rel = FileKind::Relocatable;
    let x86 = Architecture::X86_64;
    let arm = Architecture::Aarch64;
    let r = |offset, kind: object_lang::RelocationKind, target| {
        Relocation::new(offset, kind, target, 0)
    };
    assert_eq!(
        relocation_problem(x86, rel, r(0, Aarch64Reloc::Abs64.into(), sym), false),
        RelocationProblem::WrongArchitecture
    );
    assert_eq!(
        relocation_problem(x86, rel, r(9, X86_64Reloc::Abs64.into(), sym), false),
        RelocationProblem::OutOfSection
    );
    assert_eq!(
        relocation_problem(
            x86,
            rel,
            r(u64::MAX - 1, X86_64Reloc::Pc32.into(), sym),
            false
        ),
        RelocationProblem::OutOfSection
    );
    assert_eq!(
        relocation_problem(arm, rel, r(2, Aarch64Reloc::Call26.into(), sym), false),
        RelocationProblem::Misaligned
    );
    assert_eq!(
        relocation_problem(
            x86,
            rel,
            r(
                0,
                X86_64Reloc::Pc32.into(),
                RelocationTarget::Symbol(SymbolId::new(1))
            ),
            false
        ),
        RelocationProblem::UnknownTarget
    );
    assert_eq!(
        relocation_problem(
            x86,
            rel,
            r(
                0,
                X86_64Reloc::Pc32.into(),
                RelocationTarget::Section(SectionId::new(1))
            ),
            false
        ),
        RelocationProblem::UnknownTarget
    );
    assert_eq!(
        relocation_problem(x86, rel, r(0, X86_64Reloc::Pc32.into(), sym), true),
        RelocationProblem::InUninitializedSection
    );
    assert_eq!(
        relocation_problem(
            x86,
            FileKind::Executable,
            r(0, X86_64Reloc::Pc32.into(), sym),
            false
        ),
        RelocationProblem::InExecutable
    );
    // Data relocations on AArch64 need no alignment; at the very end of a section is fine.
    let mut obj = Object::relocatable(arm);
    let id = obj.add_section(Section::new(".data", SectionKind::Data).with_data(vec![0; 9]));
    obj.add_symbol(Symbol::undefined("x"));
    obj.add_relocation(id, r(1, Aarch64Reloc::Abs64.into(), sym))
        .unwrap();
    assert!(obj.validate().is_ok());
}

fn exe_with(sections: Vec<Section>, entry: u64) -> Object {
    let mut obj = Object::executable(Architecture::X86_64);
    for s in sections {
        obj.add_section(s);
    }
    obj.set_entry(entry);
    obj
}

fn text_at(address: u64, len: usize) -> Section {
    Section::new(".text", SectionKind::Text)
        .with_address(address)
        .with_data(vec![0xc3; len])
}

#[test]
fn executable_layout_rules_are_enforced() {
    // Overlap.
    let obj = exe_with(
        vec![
            text_at(0x40_1000, 16),
            Section::new(".rodata", SectionKind::ReadOnlyData)
                .with_address(0x40_1008)
                .with_data(vec![0; 4]),
        ],
        0x40_1000,
    );
    assert_eq!(
        obj.validate(),
        Err(ModelError::SectionsOverlap {
            first: SectionId::new(0),
            second: SectionId::new(1)
        })
    );
    // Entry outside code, or in non-executable data.
    assert_eq!(
        exe_with(vec![text_at(0x40_1000, 16)], 0x40_1010).validate(),
        Err(ModelError::EntryNotExecutable { entry: 0x40_1010 })
    );
    let data = Section::new(".data", SectionKind::Data)
        .with_address(0x40_2000)
        .with_data(vec![0; 4]);
    assert_eq!(
        exe_with(vec![text_at(0x40_1000, 16), data.clone()], 0x40_2000).validate(),
        Err(ModelError::EntryNotExecutable { entry: 0x40_2000 })
    );
    // Different permissions on one page.
    let close = Section::new(".data", SectionKind::Data)
        .with_address(0x40_1800)
        .with_data(vec![0; 4]);
    assert_eq!(
        object_lang::elf::write(&exe_with(vec![text_at(0x40_1000, 16), close], 0x40_1000)),
        Err(WriteError::SegmentsSharePage {
            first: SectionId::new(0),
            second: SectionId::new(1)
        })
    );
    // Initialized data after BSS on the same page.
    let bss = Section::new(".bss", SectionKind::Bss)
        .with_address(0x40_2000)
        .with_bss_size(8);
    let after = Section::new(".data2", SectionKind::Data)
        .with_address(0x40_2008)
        .with_data(vec![1]);
    assert_eq!(
        object_lang::elf::write(&exe_with(
            vec![text_at(0x40_1000, 16), bss.clone(), after],
            0x40_1000
        )),
        Err(WriteError::SegmentsSharePage {
            first: SectionId::new(1),
            second: SectionId::new(2)
        })
    );
    // ...but a page later it is a new segment.
    let later = Section::new(".data2", SectionKind::Data)
        .with_address(0x40_3000)
        .with_data(vec![1]);
    let ok = exe_with(vec![text_at(0x40_1000, 16), bss, later], 0x40_1000);
    let bytes = object_lang::elf::write(&ok).unwrap();
    assert_eq!(
        common::parse(&bytes)
            .segments
            .iter()
            .filter(|s| s.ty == 1)
            .count(),
        4
    );
    // No room below the lowest section for the headers.
    assert_eq!(
        object_lang::elf::write(&exe_with(vec![text_at(0, 16)], 0)),
        Err(WriteError::NoRoomForHeaders)
    );
    // Thread-local sections are not supported in executables yet.
    let tdata = Section::new(".tdata", SectionKind::Data)
        .with_flags(SectionKind::Data.default_flags() | SectionFlags::TLS)
        .with_address(0x40_2000)
        .with_data(vec![0; 4]);
    assert!(matches!(
        object_lang::elf::write(&exe_with(vec![text_at(0x40_1000, 16), tdata], 0x40_1000)),
        Err(WriteError::Unsupported { .. })
    ));
}

#[test]
fn huge_bss_in_an_executable_is_written_and_read_back() {
    // BSS reaching nearly to the top of the address space: offsets in the file stay
    // small, and the nominal section offsets must not overflow.
    let bss_size = u64::MAX - 0x40_3000 - 0x1000;
    let obj = exe_with(
        vec![
            text_at(0x40_1000, 16),
            Section::new(".data", SectionKind::Data)
                .with_address(0x40_2000)
                .with_data(vec![1]),
            Section::new(".bss", SectionKind::Bss)
                .with_address(0x40_2008)
                .with_bss_size(0x1000),
            Section::new(".bss2", SectionKind::Bss)
                .with_address(0x40_3008)
                .with_bss_size(bss_size),
        ],
        0x40_1000,
    );
    obj.validate().unwrap();
    let bytes = object_lang::elf::write(&obj).unwrap();
    assert!(bytes.len() < 0x4000);
    assert_eq!(object_lang::elf::read(&bytes).unwrap(), obj);
}

#[test]
fn reserved_names_and_failed_writes_leave_the_buffer_alone() {
    let mut obj = Object::relocatable(Architecture::X86_64);
    let note = obj.add_section(Section::new(".note.GNU-stack", SectionKind::Other));
    let mut buffer = b"keep".to_vec();
    assert_eq!(
        object_lang::elf::write_into(&obj, &mut buffer),
        Err(WriteError::ReservedSectionName { section: note })
    );
    assert_eq!(buffer, b"keep");
    // The name is only reserved in relocatable objects.
    let mut exe = exe_with(vec![text_at(0x40_1000, 4)], 0x40_1000);
    exe.add_section(Section::new(".note.GNU-stack", SectionKind::Other));
    assert!(object_lang::elf::write(&exe).is_ok());
    // A model error is a write error too.
    let bad = exe_with(vec![text_at(0x40_1000, 4)], 0);
    assert_eq!(
        object_lang::elf::write_into(&bad, &mut buffer),
        Err(WriteError::Invalid(ModelError::EntryNotExecutable {
            entry: 0
        }))
    );
    assert_eq!(buffer, b"keep");
}

#[test]
fn write_into_appends_after_existing_bytes() {
    let obj = Object::relocatable(Architecture::X86_64);
    let alone = object_lang::elf::write(&obj).unwrap();
    let mut buffer = vec![0xaa; 3];
    object_lang::elf::write_into(&obj, &mut buffer).unwrap();
    assert_eq!(&buffer[..3], &[0xaa; 3]);
    assert_eq!(&buffer[3..], &alone[..]);
}

#[test]
fn errors_display_actionable_messages() {
    let msg = ModelError::Section {
        section: SectionId::new(2),
        problem: SectionProblem::BadAlignment,
    }
    .to_string();
    assert_eq!(msg, "section #2: alignment is not a power of two");
    let msg = WriteError::SegmentsSharePage {
        first: SectionId::new(0),
        second: SectionId::new(1),
    }
    .to_string();
    assert!(msg.contains("share a page"));
    let source = std::error::Error::source(&WriteError::Invalid(ModelError::AppendToBss));
    assert!(source.is_some());
}
