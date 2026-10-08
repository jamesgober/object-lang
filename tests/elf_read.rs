//! The ELF reader on hostile input: each corruption of a valid file is refused with the
//! error that names it, every budget is enforced, and nothing panics.

#![allow(clippy::unwrap_used, reason = "test failures should be loud")]

mod common;

use object_lang::{
    Architecture, Binding, FileKind, Limits, Object, ReadError, Relocation, RelocationTarget,
    Section, SectionKind, Symbol, SymbolId, X86_64Reloc,
};

fn sample() -> Object {
    let mut obj = Object::relocatable(Architecture::X86_64);
    let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0x90; 16]));
    let data = obj.add_section(Section::new(".data", SectionKind::Data).with_data(vec![1; 8]));
    obj.add_section(Section::new(".bss", SectionKind::Bss).with_bss_size(32));
    obj.add_symbol(Symbol::file("sample.c"));
    obj.add_symbol(Symbol::data("local", data, 0, 4).with_binding(Binding::Local));
    obj.add_symbol(Symbol::function("main", text, 0, 16));
    let ext = obj.add_symbol(Symbol::undefined("ext"));
    obj.add_relocation(
        text,
        Relocation::new(1, X86_64Reloc::Plt32, RelocationTarget::Symbol(ext), -4),
    )
    .unwrap();
    obj.add_relocation(
        text,
        Relocation::new(8, X86_64Reloc::Pc32, RelocationTarget::Section(data), -4),
    )
    .unwrap();
    obj
}

fn bytes() -> Vec<u8> {
    object_lang::elf::write(&sample()).unwrap()
}

fn read(b: &[u8]) -> Result<Object, ReadError> {
    object_lang::elf::read(b)
}

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// File offset of section header `index`.
fn shdr(b: &[u8], index: usize) -> usize {
    common::u64_at(b, 0x28) as usize + index * 64
}

fn section_index(b: &[u8], name: &str) -> usize {
    common::parse(b).index_of(name).unwrap()
}

/// File offset of symbol `index` in `.symtab`.
fn sym(b: &[u8], index: usize) -> usize {
    let elf = common::parse(b);
    elf.sections[elf.index_of(".symtab").unwrap()].offset as usize + index * 24
}

/// File offset of relocation `index` in `.rela.text`.
fn rela(b: &[u8], index: usize) -> usize {
    let elf = common::parse(b);
    elf.sections[elf.index_of(".rela.text").unwrap()].offset as usize + index * 24
}

fn malformed(what: &'static str) -> Result<Object, ReadError> {
    Err(ReadError::Malformed { what })
}

#[test]
fn the_sample_reads_back() {
    assert_eq!(read(&bytes()).unwrap(), sample());
}

#[test]
fn identification_errors() {
    assert_eq!(
        read(&[]),
        Err(ReadError::Truncated {
            what: "file header"
        })
    );
    assert_eq!(
        read(&bytes()[..63]),
        Err(ReadError::Truncated {
            what: "file header"
        })
    );
    let mut b = bytes();
    b[0] = b'M';
    assert_eq!(read(&b), Err(ReadError::BadMagic));

    let field = |at: usize, value: u8| {
        let mut b = bytes();
        b[at] = value;
        read(&b)
    };
    assert!(
        matches!(field(4, 1), Err(ReadError::Unsupported { value: 1, .. })),
        "ELFCLASS32"
    );
    assert!(
        matches!(field(5, 2), Err(ReadError::Unsupported { value: 2, .. })),
        "big-endian"
    );
    assert_eq!(field(6, 0), malformed("ELF version is not 1"));
    assert!(
        matches!(field(7, 9), Err(ReadError::Unsupported { value: 9, .. })),
        "FreeBSD ABI"
    );
    assert!(
        matches!(field(8, 1), Err(ReadError::Unsupported { value: 1, .. })),
        "ABI version"
    );
    // ELFOSABI_GNU (what GNU tools write when they use GNU extensions) is accepted.
    assert_eq!(field(7, 3).unwrap(), sample());
}

#[test]
fn header_field_errors() {
    let mut b = bytes();
    put16(&mut b, 0x10, 3);
    assert!(
        matches!(read(&b), Err(ReadError::Unsupported { value: 3, .. })),
        "ET_DYN"
    );
    let mut b = bytes();
    put16(&mut b, 0x12, 40);
    assert!(
        matches!(read(&b), Err(ReadError::Unsupported { value: 40, .. })),
        "EM_ARM"
    );
    let mut b = bytes();
    put32(&mut b, 0x14, 2);
    assert_eq!(read(&b), malformed("ELF version is not 1"));
    let mut b = bytes();
    put32(&mut b, 0x30, 1);
    assert!(
        matches!(read(&b), Err(ReadError::Unsupported { value: 1, .. })),
        "e_flags"
    );
    let mut b = bytes();
    put16(&mut b, 0x34, 52);
    assert_eq!(read(&b), malformed("e_ehsize is not 64"));
    let mut b = bytes();
    put16(&mut b, 0x3a, 40);
    assert_eq!(read(&b), malformed("e_shentsize is not 64"));
    let mut b = bytes();
    put16(&mut b, 0x38, 1);
    put16(&mut b, 0x36, 56);
    assert_eq!(
        read(&b),
        malformed("a relocatable object has program headers")
    );
}

#[test]
fn section_table_errors() {
    let mut b = bytes();
    put64(&mut b, 0x28, 0);
    assert_eq!(read(&b), malformed("the file has no section header table"));
    let mut b = bytes();
    put64(&mut b, 0x28, u64::MAX - 10);
    assert_eq!(
        read(&b),
        Err(ReadError::Truncated {
            what: "section header table"
        })
    );
    let mut b = bytes();
    let len = b.len() as u64;
    put64(&mut b, 0x28, len - 32);
    assert_eq!(
        read(&b),
        Err(ReadError::Truncated {
            what: "section header table"
        })
    );
    let mut b = bytes();
    put16(&mut b, 0x3c, 0xfeff);
    assert_eq!(
        read(&b),
        Err(ReadError::Truncated {
            what: "section header table"
        })
    );
    let mut b = bytes();
    put16(&mut b, 0x3e, 0);
    assert_eq!(
        read(&b),
        malformed("section name string table index is out of range")
    );
    let mut b = bytes();
    put16(&mut b, 0x3e, 0xff10);
    assert_eq!(read(&b), malformed("e_shstrndx is a reserved index"));
    let mut b = bytes();
    put16(&mut b, 0x3e, 1);
    assert_eq!(
        read(&b),
        malformed("section name table is not a string table")
    );
    // A non-null section header 0.
    let mut b = bytes();
    let at = shdr(&b, 0);
    put32(&mut b, at + 4, 1);
    assert_eq!(read(&b), malformed("section header 0 is not a null header"));
    // An extended count of zero.
    let mut b = bytes();
    put16(&mut b, 0x3c, 0);
    assert_eq!(read(&b), malformed("section header count is zero"));
}

#[test]
fn section_errors() {
    let text = section_index(&bytes(), ".text");
    // Contents past the end.
    let mut b = bytes();
    let at = shdr(&b, text);
    let len = b.len() as u64;
    put64(&mut b, at + 24, len);
    assert_eq!(
        read(&b),
        Err(ReadError::Truncated {
            what: "section contents"
        })
    );
    // Name offset past the table.
    let mut b = bytes();
    let at = shdr(&b, text);
    put32(&mut b, at, 0x7fff_ffff);
    assert_eq!(
        read(&b),
        malformed("name offset is past the end of its string table")
    );
    // Unknown type, unknown flag, REL instead of RELA, link set on a content section.
    for (field, offset, value, expected) in [
        ("type", 4, 17u64, "section type"),
        ("type", 4, 9, "section type"),
        ("flags", 8, 0x206, "section flags"),
    ] {
        let mut b = bytes();
        let at = shdr(&b, text);
        if field == "type" {
            put32(&mut b, at + offset, value as u32);
        } else {
            put64(&mut b, at + offset, value);
        }
        assert!(
            matches!(read(&b), Err(ReadError::Unsupported { what, .. }) if what == expected),
            "{field} = {value:#x}"
        );
    }
    let mut b = bytes();
    let at = shdr(&b, text);
    put32(&mut b, at + 40, 1);
    assert_eq!(
        read(&b),
        malformed("a content section has sh_link or sh_info set")
    );
    // A second null header.
    let mut b = bytes();
    let at = shdr(&b, text);
    put32(&mut b, at + 4, 0);
    assert_eq!(
        read(&b),
        malformed("an inactive section header follows the first")
    );
    // An alignment that is not a power of two is a model error.
    let mut b = bytes();
    let at = shdr(&b, text);
    put64(&mut b, at + 48, 3);
    assert!(matches!(read(&b), Err(ReadError::Invalid(_))));
}

#[test]
fn string_errors() {
    let b = bytes();
    let elf = common::parse(&b);
    let strtab = &elf.sections[elf.index_of(".strtab").unwrap()];
    // Overwrite the final NUL of .strtab: the last name runs off the table.
    let mut b2 = b.clone();
    b2[(strtab.offset + strtab.size - 1) as usize] = b'x';
    assert_eq!(read(&b2), malformed("name is not NUL-terminated"));
    // Invalid UTF-8 in a name.
    let mut b3 = b.clone();
    b3[(strtab.offset + 1) as usize] = 0xff;
    assert_eq!(read(&b3), malformed("name is not valid UTF-8"));
    // An unreferenced extra string table.
    let mut b4 = b.clone();
    let at = shdr(&b4, section_index(&b, ".data"));
    put32(&mut b4, at + 4, 3);
    put64(&mut b4, at + 8, 0);
    assert!(matches!(
        read(&b4),
        Err(ReadError::Unsupported {
            what: "string table that nothing refers to",
            ..
        })
    ));
}

#[test]
fn symbol_errors() {
    let b = bytes();
    let symtab = section_index(&b, ".symtab");
    let mut b2 = b.clone();
    let at = shdr(&b2, symtab);
    put64(&mut b2, at + 56, 16);
    assert_eq!(read(&b2), malformed("symbol entry size is not 24"));
    let mut b2 = b.clone();
    let at = shdr(&b2, symtab);
    put32(&mut b2, at + 44, 99);
    assert_eq!(
        read(&b2),
        malformed("the symbol table's first non-local index is out of range")
    );
    // The names' table retyped as plain data: it is now a content section, and the
    // symbol table links to something that is not a string table.
    let mut b2 = b.clone();
    let at = shdr(&b2, section_index(&b, ".strtab"));
    put32(&mut b2, at + 4, 1);
    assert_eq!(
        read(&b2),
        malformed("the symbol table's string table is not a string table")
    );
    // Linking elsewhere leaves .strtab referenced by nothing.
    let mut b2 = b.clone();
    let at = shdr(&b2, symtab);
    put32(&mut b2, at + 40, symtab as u32);
    assert!(matches!(
        read(&b2),
        Err(ReadError::Unsupported {
            what: "string table that nothing refers to",
            ..
        })
    ));
    // Non-null symbol 0.
    let mut b2 = b.clone();
    let at = sym(&b2, 0);
    b2[at + 4] = 1;
    assert_eq!(read(&b2), malformed("symbol 0 is not the null symbol"));
    // Symbol order is null, section symbol for .data, file, local, main, ext.
    // A local among the globals.
    let mut b2 = b.clone();
    let at = sym(&b2, 4);
    b2[at + 4] = 0x02;
    assert_eq!(
        read(&b2),
        malformed("local and non-local symbols are interleaved")
    );
    // Section index past the table, and an undefined-only reserved index.
    let mut b2 = b.clone();
    let at = sym(&b2, 4);
    put16(&mut b2, at + 6, 500);
    assert_eq!(
        read(&b2),
        malformed("a symbol is defined in a section with no contents")
    );
    let mut b2 = b.clone();
    let at = sym(&b2, 4);
    put16(&mut b2, at + 6, 0xff20);
    assert!(matches!(
        read(&b2),
        Err(ReadError::Unsupported {
            what: "reserved symbol section index",
            ..
        })
    ));
    let mut b2 = b.clone();
    let at = sym(&b2, 4);
    put16(&mut b2, at + 6, 0xffff);
    assert_eq!(
        read(&b2),
        malformed("a symbol uses an extended section index but there is no index table")
    );
    // Unsupported type (STT_GNU_IFUNC), binding (STB_GNU_UNIQUE), visibility.
    for (offset, value, what) in [
        (4, 0x1a, "symbol type"),
        (4, 0xa2, "symbol binding"),
        (5, 1, "symbol visibility or st_other flags"),
        (5, 0x80, "symbol visibility or st_other flags"),
    ] {
        let mut b2 = b.clone();
        let at = sym(&b2, 4);
        b2[at + offset] = value;
        assert!(
            matches!(read(&b2), Err(ReadError::Unsupported { what: w, .. }) if w == what),
            "{what}"
        );
    }
    // A section symbol that is global, or has a value.
    let mut b2 = b.clone();
    let at = sym(&b2, 1);
    put64(&mut b2, at + 8, 4);
    assert_eq!(
        read(&b2),
        malformed("a section symbol is not local with value zero")
    );
    // A symbol outside its section is a model error.
    let mut b2 = b.clone();
    let at = sym(&b2, 4);
    put64(&mut b2, at + 8, 1000);
    assert!(matches!(read(&b2), Err(ReadError::Invalid(_))));
}

#[test]
fn relocation_errors() {
    let b = bytes();
    let rela_index = section_index(&b, ".rela.text");
    let mut b2 = b.clone();
    let at = rela(&b2, 0);
    put32(&mut b2, at + 8, 23); // R_X86_64_TPOFF32: thread-local, not modelled yet
    assert!(matches!(
        read(&b2),
        Err(ReadError::Unsupported {
            what: "relocation type",
            value: 23
        })
    ));
    let mut b2 = b.clone();
    let at = rela(&b2, 0);
    put32(&mut b2, at + 12, 0);
    assert!(matches!(
        read(&b2),
        Err(ReadError::Unsupported {
            what: "relocation against no symbol",
            ..
        })
    ));
    let mut b2 = b.clone();
    let at = rela(&b2, 0);
    put32(&mut b2, at + 12, 77);
    assert_eq!(
        read(&b2),
        malformed("relocation refers to a symbol that does not exist")
    );
    let mut b2 = b.clone();
    let at = rela(&b2, 0);
    put64(&mut b2, at, 1000);
    assert!(
        matches!(read(&b2), Err(ReadError::Invalid(_))),
        "offset past the section"
    );
    let mut b2 = b.clone();
    let at = shdr(&b2, rela_index);
    put64(&mut b2, at + 56, 16);
    assert_eq!(read(&b2), malformed("relocation entry size is not 24"));
    let mut b2 = b.clone();
    let at = shdr(&b2, rela_index);
    put32(&mut b2, at + 40, 0);
    assert_eq!(
        read(&b2),
        malformed("a relocation section is not linked to the symbol table")
    );
    let mut b2 = b.clone();
    let at = shdr(&b2, rela_index);
    put32(&mut b2, at + 44, section_index(&b, ".symtab") as u32);
    assert_eq!(
        read(&b2),
        malformed("a relocation section patches a section with no contents")
    );
    // Relocations against BSS decode, and are refused by the model.
    let mut b2 = b.clone();
    let at = shdr(&b2, rela_index);
    put32(&mut b2, at + 44, section_index(&b, ".bss") as u32);
    assert!(matches!(
        read(&b2),
        Err(ReadError::Invalid(object_lang::ModelError::Relocation {
            problem: object_lang::RelocationProblem::InUninitializedSection,
            ..
        }))
    ));
    let mut b2 = b.clone();
    let at = shdr(&b2, rela_index);
    put64(&mut b2, at + 8, 0x42);
    assert!(matches!(
        read(&b2),
        Err(ReadError::Unsupported {
            what: "relocation section flags",
            ..
        })
    ));
    // Two relocation sections for .text: retarget the GNU-stack note's header as a copy.
    let mut b2 = b.clone();
    let note = section_index(&b, ".note.GNU-stack");
    let src = shdr(&b2, rela_index);
    let dst = shdr(&b2, note);
    let copy = b2[src..src + 64].to_vec();
    b2[dst..dst + 64].copy_from_slice(&copy);
    assert_eq!(
        read(&b2),
        malformed("two relocation sections patch the same section")
    );
}

#[test]
fn gnu_stack_marker_rules() {
    let b = bytes();
    let note = section_index(&b, ".note.GNU-stack");
    let mut b2 = b.clone();
    let at = shdr(&b2, note);
    put64(&mut b2, at + 32, 4);
    assert_eq!(
        read(&b2),
        malformed(".note.GNU-stack is not an empty marker section")
    );
    let mut b2 = b.clone();
    let at = shdr(&b2, note);
    put64(&mut b2, at + 8, 2);
    assert_eq!(
        read(&b2),
        malformed(".note.GNU-stack is not an empty marker section")
    );
    // Without the marker the object asks for an executable stack, as GNU ld assumes.
    let mut b2 = b.clone();
    let at = shdr(&b2, note);
    let shstrtab = &common::parse(&b).sections[common::parse(&b).shstrndx];
    // Rename it to ".note.GNU-stacK" (the name is in .shstrtab).
    let name = common::u32_at(&b2, at) as usize;
    b2[shstrtab.offset as usize + name + 14] = b'K';
    let obj = read(&b2).unwrap();
    assert!(obj.executable_stack());
    assert!(obj.section_by_name(".note.GNU-stacK").is_some());
}

#[test]
fn budgets_are_enforced() {
    let b = bytes();
    let limit = |f: fn(&mut Limits)| {
        let mut limits = Limits::default();
        f(&mut limits);
        object_lang::elf::read_with_limits(&b, &limits)
    };
    assert_eq!(
        limit(|l| l.max_sections = 4),
        Err(ReadError::LimitExceeded {
            limit: "max_sections"
        })
    );
    assert_eq!(
        limit(|l| l.max_symbols = 3),
        Err(ReadError::LimitExceeded {
            limit: "max_symbols"
        })
    );
    assert_eq!(
        limit(|l| l.max_relocations = 1),
        Err(ReadError::LimitExceeded {
            limit: "max_relocations"
        })
    );
    assert_eq!(
        limit(|l| l.max_name_len = 4),
        Err(ReadError::LimitExceeded {
            limit: "max_name_len"
        })
    );
    assert_eq!(
        limit(|l| l.max_name_bytes = 20),
        Err(ReadError::LimitExceeded {
            limit: "max_name_bytes"
        })
    );
    // Exactly at the limits is fine.
    let elf = common::parse(&b);
    let count = elf.sections.len() as u32;
    let mut limits = Limits::default();
    limits.max_sections = count;
    limits.max_symbols = 6;
    limits.max_relocations = 2;
    assert!(object_lang::elf::read_with_limits(&b, &limits).is_ok());
}

#[test]
fn many_symbols_sharing_one_long_name_hit_the_name_budget() {
    // A hostile file points every symbol at one long name, so the names copied out
    // would be far larger than the file. The aggregate budget stops it.
    let mut obj = Object::relocatable(Architecture::X86_64);
    obj.add_symbol(Symbol::undefined("n".repeat(4000)));
    for i in 0..2000 {
        obj.add_symbol(Symbol::undefined(format!("s{i}")));
    }
    let mut b = object_lang::elf::write(&obj).unwrap();
    let long = common::u32_at(&b, sym(&b, 1));
    for i in 2..=2001 {
        let at = sym(&b, i);
        put32(&mut b, at, long);
    }
    // ~8 MB of names from a ~60 KB file.
    let mut limits = Limits::default();
    limits.max_name_bytes = 1_000_000;
    assert_eq!(
        object_lang::elf::read_with_limits(&b, &limits),
        Err(ReadError::LimitExceeded {
            limit: "max_name_bytes"
        })
    );
    // With the default budget the copy is allowed, and correct.
    let read = object_lang::elf::read(&b).unwrap();
    assert!(read.symbols().iter().all(|s| s.name.len() == 4000));
}

#[test]
fn overlapping_section_data_cannot_multiply_memory() {
    // Many headers pointing at the same large contents would copy them many times.
    let mut obj = Object::relocatable(Architecture::X86_64);
    obj.add_section(Section::new(".big", SectionKind::ReadOnlyData).with_data(vec![7; 10_000]));
    for i in 0..8 {
        obj.add_section(
            Section::new(format!(".s{i}"), SectionKind::ReadOnlyData).with_data(vec![0]),
        );
    }
    let mut b = object_lang::elf::write(&obj).unwrap();
    let big = shdr(&b, 1);
    let (offset, size) = (common::u64_at(&b, big + 24), common::u64_at(&b, big + 32));
    for i in 2..=9 {
        let at = shdr(&b, i);
        put64(&mut b, at + 24, offset);
        put64(&mut b, at + 32, size);
    }
    assert_eq!(
        read(&b),
        Err(ReadError::LimitExceeded {
            limit: "section data exceeds the input size"
        })
    );
}

fn exe_bytes() -> Vec<u8> {
    object_lang::elf::executable(Architecture::X86_64, &[0xeb, 0xfe]).unwrap()
}

/// File offset of program header `index`.
fn phdr(b: &[u8], index: usize) -> usize {
    common::u64_at(b, 0x20) as usize + index * 56
}

#[test]
fn executable_segment_errors() {
    let b = exe_bytes();
    assert_eq!(read(&b).unwrap().kind(), FileKind::Executable);
    // PT_INTERP and PT_TLS are not supported.
    for ty in [3u32, 7, 2] {
        let mut b2 = b.clone();
        let at = phdr(&b2, 2);
        put32(&mut b2, at, ty);
        assert!(
            matches!(
                read(&b2),
                Err(ReadError::Unsupported {
                    what: "program header type",
                    ..
                })
            ),
            "p_type {ty}"
        );
    }
    // PT_PHDR carries nothing the model needs and is accepted.
    let mut b2 = b.clone();
    let at = phdr(&b2, 2);
    put32(&mut b2, at, 6);
    assert!(
        read(&b2).unwrap().executable_stack(),
        "no PT_GNU_STACK left"
    );
    // filesz > memsz, contents past the end, offset/address mismatch.
    let mut b2 = b.clone();
    let at = phdr(&b2, 1);
    put64(&mut b2, at + 40, 1);
    assert_eq!(
        read(&b2),
        malformed("a segment's file size exceeds its memory size")
    );
    let mut b2 = b.clone();
    let at = phdr(&b2, 1);
    put64(&mut b2, at + 8, 0x10_0000);
    assert_eq!(
        read(&b2),
        Err(ReadError::Truncated {
            what: "segment contents"
        })
    );
    let mut b2 = b.clone();
    let at = phdr(&b2, 1);
    put64(&mut b2, at + 16, 0x40_1004);
    assert_eq!(
        read(&b2),
        malformed("segment offset and address disagree modulo the alignment")
    );
    // Out of order.
    let mut b2 = b.clone();
    let at = phdr(&b2, 0);
    put64(&mut b2, at + 16, 0x50_0000);
    assert_eq!(
        read(&b2),
        malformed("loadable segments overlap or are out of order")
    );
    // Code segment no longer executable: the section exceeds its permissions.
    let mut b2 = b.clone();
    let at = phdr(&b2, 1);
    put32(&mut b2, at + 4, 4);
    assert_eq!(
        read(&b2),
        malformed("a section's flags exceed its segment's permissions")
    );
    // Section moved outside every segment.
    let mut b2 = b.clone();
    let at = shdr(&b2, 1);
    put64(&mut b2, at + 16, 0x90_0000);
    assert_eq!(
        read(&b2),
        malformed("a loadable section is outside every segment")
    );
    // Section file offset disagreeing with its segment.
    let mut b2 = b.clone();
    let at = shdr(&b2, 1);
    put64(&mut b2, at + 24, 0x100);
    assert_eq!(
        read(&b2),
        malformed("a section's file offset does not match its segment")
    );
    // Entry outside code.
    let mut b2 = b.clone();
    put64(&mut b2, 0x18, 0x40_0000);
    assert_eq!(
        read(&b2),
        malformed("the entry point is not in an executable segment")
    );
    // Two PT_GNU_STACK headers.
    let mut b2 = b.clone();
    let at = phdr(&b2, 0);
    put32(&mut b2, at, 0x6474_e551);
    assert_eq!(read(&b2), malformed("more than one PT_GNU_STACK header"));
    // An executable stack request is reported.
    let mut b2 = b.clone();
    let at = phdr(&b2, 2);
    put32(&mut b2, at + 4, 7);
    assert!(read(&b2).unwrap().executable_stack());
    // Extended program header count is not supported.
    let mut b2 = b.clone();
    put16(&mut b2, 0x38, 0xffff);
    assert!(matches!(
        read(&b2),
        Err(ReadError::Unsupported {
            what: "extended program header count",
            ..
        })
    ));
}

#[test]
fn relocations_in_an_executable_are_refused() {
    // Retype the executable's .symtab as SHT_RELA: a relocation section in an
    // executable is refused as soon as its header is seen.
    let b = exe_bytes();
    let symtab = section_index(&b, ".symtab");
    let mut b2 = b.clone();
    let at = shdr(&b2, symtab);
    put32(&mut b2, at + 4, 4);
    assert!(matches!(
        read(&b2),
        Err(ReadError::Unsupported {
            what: "relocation section in an executable",
            ..
        })
    ));
}

#[test]
fn every_prefix_of_every_kind_of_file_is_refused_without_panicking() {
    for b in [bytes(), exe_bytes()] {
        for len in 0..b.len() {
            assert!(read(&b[..len]).is_err(), "prefix of {len} bytes");
        }
    }
}

#[test]
fn every_single_byte_corruption_is_survived() {
    // Flip each byte of the headers and tables to three values; nothing may panic,
    // and whatever is accepted must write back and read back unchanged.
    let b = bytes();
    for at in 0..b.len() {
        for value in [0x00, 0xff, b[at] ^ 0x80] {
            let mut b2 = b.clone();
            b2[at] = value;
            if let Ok(obj) = read(&b2) {
                let again = object_lang::elf::write(&obj).unwrap();
                assert_eq!(read(&again).unwrap(), obj, "byte {at} = {value:#x}");
            }
        }
    }
}

#[test]
fn symbol_ids_follow_elf_order() {
    let obj = read(&bytes()).unwrap();
    assert_eq!(obj.symbol_by_name("sample.c"), Some(SymbolId::new(0)));
    assert_eq!(obj.symbol_by_name("ext"), Some(SymbolId::new(3)));
}
