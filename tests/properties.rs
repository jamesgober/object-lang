//! Property tests for the invariants in `dev/DIRECTIVES.md` §4, against references
//! written in the test (`tests/common`):
//!
//! - **Round trip.** Every object the writer accepts reads back as the same object
//!   (symbols in ELF order: locals first, otherwise as given), for relocatable objects
//!   and executables over random models.
//! - **Writer against a reference decoder.** A naive decoder written from the ELF
//!   specification checks every header, section, symbol, and relocation the writer
//!   emits against the model it came from.
//! - **Executables against a reference loader.** A page-level simulation of the Linux
//!   loader maps every `PT_LOAD`; every section's bytes must appear at its address
//!   with at least its permissions, BSS must read as zeros, and no page may be mapped
//!   twice.
//! - **Determinism.** Writing is byte-identical across calls and across a read/write
//!   cycle.
//! - **Hostile input.** The reader never panics on arbitrary bytes or on mutated and
//!   truncated valid files; whatever it accepts, the writer writes, and that reads back
//!   as the same object.

mod common;

use object_lang::{
    Aarch64Reloc, Architecture, Binding, FileKind, Object, Relocation, RelocationKind,
    RelocationTarget, Section, SectionFlags, SectionId, SectionKind, Symbol, SymbolId, SymbolKind,
    SymbolSection, Visibility, X86_64Reloc,
};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

const X86_KINDS: [X86_64Reloc; 9] = [
    X86_64Reloc::Abs64,
    X86_64Reloc::Abs32,
    X86_64Reloc::Abs32Signed,
    X86_64Reloc::Pc32,
    X86_64Reloc::Pc64,
    X86_64Reloc::Plt32,
    X86_64Reloc::GotPcRel,
    X86_64Reloc::GotPcRelX,
    X86_64Reloc::RexGotPcRelX,
];

const ARM_KINDS: [Aarch64Reloc; 18] = [
    Aarch64Reloc::Abs64,
    Aarch64Reloc::Abs32,
    Aarch64Reloc::Prel64,
    Aarch64Reloc::Prel32,
    Aarch64Reloc::Call26,
    Aarch64Reloc::Jump26,
    Aarch64Reloc::CondBr19,
    Aarch64Reloc::TstBr14,
    Aarch64Reloc::AdrPrelLo21,
    Aarch64Reloc::AdrPrelPgHi21,
    Aarch64Reloc::AddAbsLo12Nc,
    Aarch64Reloc::Ldst8AbsLo12Nc,
    Aarch64Reloc::Ldst16AbsLo12Nc,
    Aarch64Reloc::Ldst32AbsLo12Nc,
    Aarch64Reloc::Ldst64AbsLo12Nc,
    Aarch64Reloc::Ldst128AbsLo12Nc,
    Aarch64Reloc::AdrGotPage,
    Aarch64Reloc::Ld64GotLo12Nc,
];

const KINDS: [SectionKind; 9] = [
    SectionKind::Text,
    SectionKind::Data,
    SectionKind::ReadOnlyData,
    SectionKind::Bss,
    SectionKind::Note,
    SectionKind::InitArray,
    SectionKind::FiniArray,
    SectionKind::PreinitArray,
    SectionKind::Other,
];

const NAMES: [&str; 8] = ["", ".text", ".data", "x", "ext", "a.b", "αβ", ".text"];

fn arch(choice: bool) -> Architecture {
    if choice {
        Architecture::Aarch64
    } else {
        Architecture::X86_64
    }
}

/// Raw choices for one section; `build_section` turns them into a valid section.
type SectionSeed = (usize, u8, u32, Vec<u8>, u64, u8, u8);

fn section_seed() -> impl Strategy<Value = SectionSeed> {
    (
        0..KINDS.len(),
        0u8..13,
        any::<u32>(),
        prop::collection::vec(any::<u8>(), 0..48),
        any::<u64>(),
        any::<u8>(),
        0u8..8,
    )
}

fn build_section(seed: &SectionSeed, address_of: Option<u64>) -> Section {
    let (kind, align_log, extra, data, bss, flags_seed, name) = seed;
    let kind = KINDS[*kind];
    let align = 1u64 << align_log;
    let mut flags = kind.default_flags();
    let mut entry_size = u64::from(*extra % 5);
    let merge_ok = matches!(
        kind,
        SectionKind::Text | SectionKind::Data | SectionKind::ReadOnlyData | SectionKind::Other
    );
    if merge_ok && flags_seed & 1 != 0 {
        flags |= SectionFlags::MERGE | SectionFlags::STRINGS;
        entry_size = entry_size.max(1);
    }
    if matches!(kind, SectionKind::Data | SectionKind::Bss) && flags_seed & 2 != 0 {
        flags |= SectionFlags::TLS;
    }
    if kind == SectionKind::Text && flags_seed & 4 != 0 {
        flags |= SectionFlags::WRITE;
    }
    if kind == SectionKind::Note && flags_seed & 8 != 0 {
        flags |= SectionFlags::ALLOC;
    }
    let address = address_of.unwrap_or_else(|| (u64::from(*extra) >> 8) % 16 * align);
    let section = Section::new(NAMES[*name as usize % NAMES.len()], kind)
        .with_flags(flags)
        .with_align(align)
        .with_entry_size(entry_size)
        .with_address(address);
    if kind.is_uninitialized() {
        section.with_bss_size(bss % (1 << 40))
    } else {
        section.with_data(data.clone())
    }
}

/// Raw choices for one symbol.
type SymbolSeed = (u8, u8, u8, u8, u8, u64, u64, u8);

fn symbol_seed() -> impl Strategy<Value = SymbolSeed> {
    (
        0u8..5,
        0u8..3,
        0u8..3,
        0u8..4,
        any::<u8>(),
        any::<u64>(),
        any::<u64>(),
        0u8..8,
    )
}

/// Turns raw choices into a symbol that is valid for `obj` (in its current state).
fn build_symbol(seed: &SymbolSeed, obj: &Object) -> Symbol {
    let (kind, binding, visibility, where_, section, value, size, name) = *seed;
    let name = format!("{}{}", NAMES[name as usize % NAMES.len()], kind);
    let kind = [
        SymbolKind::NoType,
        SymbolKind::Function,
        SymbolKind::Data,
        SymbolKind::Tls,
        SymbolKind::File,
    ][kind as usize];
    let binding = [Binding::Local, Binding::Global, Binding::Weak][binding as usize];
    let visibility = [
        Visibility::Default,
        Visibility::Hidden,
        Visibility::Protected,
    ][visibility as usize];
    let executable = obj.kind() == FileKind::Executable;

    if kind == SymbolKind::File {
        return Symbol::file(name);
    }
    let sections = obj.sections();
    let mut symbol = match where_ {
        0 => Symbol::new(name, kind, binding, SymbolSection::Undefined, 0, size % 64),
        1 => Symbol::new(name, kind, binding, SymbolSection::Absolute, value, size),
        2 if !executable => Symbol::new(
            name,
            kind,
            Binding::Global,
            SymbolSection::Common,
            1 << (value % 12),
            size % 4096,
        ),
        _ if !sections.is_empty() => {
            let index = section as usize % sections.len();
            let s = &sections[index];
            let base = if executable { s.address() } else { 0 };
            let offset = if s.size() == 0 {
                0
            } else {
                value % (s.size() + 1)
            };
            let len = if s.size() - offset == 0 {
                0
            } else {
                size % (s.size() - offset + 1)
            };
            Symbol::new(
                name,
                kind,
                binding,
                SymbolSection::Section(sid(index)),
                base + offset,
                len,
            )
        }
        _ => Symbol::new(name, kind, binding, SymbolSection::Absolute, value, size),
    };
    symbol.visibility = visibility;
    match symbol.section {
        SymbolSection::Undefined if symbol.binding == Binding::Local || executable => {
            symbol.binding = Binding::Weak;
        }
        SymbolSection::Section(id)
            if symbol.kind == SymbolKind::Tls
                && !obj.sections()[id.index()]
                    .flags()
                    .contains(SectionFlags::TLS) =>
        {
            symbol.kind = SymbolKind::Data;
        }
        _ => {}
    }
    symbol
}

type RelocSeed = (u8, u64, bool, u8, i64);

fn reloc_seed() -> impl Strategy<Value = RelocSeed> {
    (
        any::<u8>(),
        any::<u64>(),
        any::<bool>(),
        any::<u8>(),
        any::<i64>(),
    )
}

fn add_relocations(obj: &mut Object, seeds: &[RelocSeed]) {
    let arch = obj.architecture();
    let section_count = obj.sections().len();
    let symbol_count = obj.symbols().len();
    if section_count == 0 {
        return;
    }
    for (i, &(kind, offset, to_symbol, target, addend)) in seeds.iter().enumerate() {
        let index = i % section_count;
        let section = &obj.sections()[index];
        let kind: RelocationKind = match arch {
            Architecture::X86_64 => X86_KINDS[kind as usize % X86_KINDS.len()].into(),
            _ => ARM_KINDS[kind as usize % ARM_KINDS.len()].into(),
        };
        if section.kind().is_uninitialized() || section.size() < kind.size() {
            continue;
        }
        let mut offset = offset % (section.size() - kind.size() + 1);
        if kind.is_instruction() {
            offset &= !3;
        }
        let target = if to_symbol && symbol_count > 0 {
            RelocationTarget::Symbol(SymbolId::new((target as usize % symbol_count) as u32))
        } else {
            RelocationTarget::Section(sid(target as usize % section_count))
        };
        let id = sid(index);
        obj.add_relocation(id, Relocation::new(offset, kind, target, addend))
            .unwrap();
    }
}

fn sid(index: usize) -> SectionId {
    SectionId::new(index as u32)
}

fn relocatable() -> impl Strategy<Value = Object> {
    (
        any::<bool>(),
        any::<bool>(),
        prop::collection::vec(section_seed(), 0..7),
        prop::collection::vec(symbol_seed(), 0..10),
        prop::collection::vec(reloc_seed(), 0..12),
    )
        .prop_map(|(arm, stack, sections, symbols, relocs)| {
            let mut obj = Object::relocatable(arch(arm));
            obj.set_executable_stack(stack);
            for seed in &sections {
                obj.add_section(build_section(seed, None));
            }
            for seed in &symbols {
                let symbol = build_symbol(seed, &obj);
                obj.add_symbol(symbol);
            }
            add_relocations(&mut obj, &relocs);
            obj
        })
}

/// Random executables laid out as a linker would: runs of sections with one set of
/// permissions, each run starting on a fresh page, gaps inside a run under a page,
/// BSS only at the end of a run, an entry point inside code.
fn executable() -> impl Strategy<Value = Object> {
    let group = (
        0u8..3,
        prop::collection::vec(
            (0u8..6, prop::collection::vec(any::<u8>(), 1..40), 0u64..64),
            1..4,
        ),
        prop::option::of(1u64..100_000),
        0u64..3,
    );
    (
        any::<bool>(),
        any::<bool>(),
        prop::collection::vec(group, 1..4),
        prop::collection::vec(symbol_seed(), 0..6),
        0u8..3,
        any::<u64>(),
    )
        .prop_map(
            |(arm, stack, groups, symbols, extra_non_alloc, entry_seed)| {
                let arch = arch(arm);
                let page = arch.page_size();
                let mut obj = Object::executable(arch);
                obj.set_executable_stack(stack);
                let mut addr = 0x40_0000 + page * 4;
                let mut code: Vec<(u64, u64)> = Vec::new();
                // Make sure one group is code.
                let mut groups = groups;
                groups[0].0 = 2;
                for (perm, sections, bss, gap_pages) in groups {
                    addr = (addr + page - 1) & !(page - 1);
                    addr += gap_pages * page;
                    let kind = [
                        SectionKind::ReadOnlyData,
                        SectionKind::Data,
                        SectionKind::Text,
                    ][perm as usize];
                    for (align_log, data, gap) in sections {
                        let align = 1u64 << align_log;
                        addr = (addr + gap + align - 1) & !(align - 1);
                        let len = data.len() as u64;
                        obj.add_section(
                            Section::new(format!("s{addr:x}"), kind)
                                .with_align(align)
                                .with_address(addr)
                                .with_data(data),
                        );
                        if kind == SectionKind::Text {
                            code.push((addr, len));
                        }
                        addr += len;
                    }
                    if let (Some(size), SectionKind::Data) = (bss, kind) {
                        addr = (addr + 7) & !7;
                        obj.add_section(
                            Section::new(".bss", SectionKind::Bss)
                                .with_align(8)
                                .with_address(addr)
                                .with_bss_size(size),
                        );
                        addr += size;
                    }
                    addr += page;
                }
                for i in 0..extra_non_alloc {
                    obj.add_section(
                        Section::new(format!(".comment{i}"), SectionKind::Other)
                            .with_data(vec![i; 3]),
                    );
                }
                let (start, len) = code[entry_seed as usize % code.len()];
                obj.set_entry(start + entry_seed % len);
                for seed in &symbols {
                    let symbol = build_symbol(seed, &obj);
                    obj.add_symbol(symbol);
                }
                obj
            },
        )
}

// ---------------------------------------------------------------------------
// Reference: the object as it must read back
// ---------------------------------------------------------------------------

/// `obj` with its symbols in ELF order (locals first, otherwise as given), and every
/// relocation's symbol target renumbered to match.
fn elf_order(obj: &Object) -> Object {
    let symbols = obj.symbols();
    let order: Vec<usize> = (0..symbols.len())
        .filter(|&i| symbols[i].binding == Binding::Local)
        .chain((0..symbols.len()).filter(|&i| symbols[i].binding != Binding::Local))
        .collect();
    let mut new_index = vec![0usize; symbols.len()];
    for (new, &old) in order.iter().enumerate() {
        new_index[old] = new;
    }

    let mut out = Object::new(obj.kind(), obj.architecture());
    out.set_entry(obj.entry());
    out.set_executable_stack(obj.executable_stack());
    let mut ids = Vec::new();
    for &old in &order {
        ids.push(out.add_symbol(symbols[old].clone()));
    }
    for s in obj.sections() {
        let mut copy = Section::new(s.name(), s.kind())
            .with_flags(s.flags())
            .with_align(s.align())
            .with_entry_size(s.entry_size())
            .with_address(s.address());
        copy = if s.kind().is_uninitialized() {
            copy.with_bss_size(s.size())
        } else {
            copy.with_data(s.data().to_vec())
        };
        for r in s.relocations() {
            let mut r = *r;
            if let RelocationTarget::Symbol(id) = r.target {
                r.target = RelocationTarget::Symbol(ids[new_index[id.index()]]);
            }
            copy.add_relocation(r);
        }
        out.add_section(copy);
    }
    out
}

fn elf_section_type(kind: SectionKind) -> u32 {
    match kind {
        SectionKind::Bss => 8,
        SectionKind::Note => 7,
        SectionKind::InitArray => 14,
        SectionKind::FiniArray => 15,
        SectionKind::PreinitArray => 16,
        _ => 1,
    }
}

fn elf_flags(flags: SectionFlags) -> u64 {
    let mut out = 0;
    for (flag, bit) in [
        (SectionFlags::WRITE, 1),
        (SectionFlags::ALLOC, 2),
        (SectionFlags::EXEC, 4),
        (SectionFlags::MERGE, 0x10),
        (SectionFlags::STRINGS, 0x20),
        (SectionFlags::TLS, 0x400),
    ] {
        if flags.contains(flag) {
            out |= bit;
        }
    }
    out
}

/// ELF relocation numbers, from the psABI documents (not from the crate).
fn elf_reloc_type(kind: RelocationKind) -> u32 {
    match kind {
        RelocationKind::X86_64(k) => match k {
            X86_64Reloc::Abs64 => 1,
            X86_64Reloc::Pc32 => 2,
            X86_64Reloc::Plt32 => 4,
            X86_64Reloc::GotPcRel => 9,
            X86_64Reloc::Abs32 => 10,
            X86_64Reloc::Abs32Signed => 11,
            X86_64Reloc::Pc64 => 24,
            X86_64Reloc::GotPcRelX => 41,
            X86_64Reloc::RexGotPcRelX => 42,
            _ => unreachable!(),
        },
        RelocationKind::Aarch64(k) => match k {
            Aarch64Reloc::Abs64 => 257,
            Aarch64Reloc::Abs32 => 258,
            Aarch64Reloc::Prel64 => 260,
            Aarch64Reloc::Prel32 => 261,
            Aarch64Reloc::AdrPrelLo21 => 274,
            Aarch64Reloc::AdrPrelPgHi21 => 275,
            Aarch64Reloc::AddAbsLo12Nc => 277,
            Aarch64Reloc::Ldst8AbsLo12Nc => 278,
            Aarch64Reloc::TstBr14 => 279,
            Aarch64Reloc::CondBr19 => 280,
            Aarch64Reloc::Jump26 => 282,
            Aarch64Reloc::Call26 => 283,
            Aarch64Reloc::Ldst16AbsLo12Nc => 284,
            Aarch64Reloc::Ldst32AbsLo12Nc => 285,
            Aarch64Reloc::Ldst64AbsLo12Nc => 286,
            Aarch64Reloc::Ldst128AbsLo12Nc => 299,
            Aarch64Reloc::AdrGotPage => 311,
            Aarch64Reloc::Ld64GotLo12Nc => 312,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

/// Checks the bytes against the model with the reference decoder.
fn check_against_reference(obj: &Object, bytes: &[u8]) -> Result<(), TestCaseError> {
    let elf = common::parse(bytes);
    let relocatable = obj.kind() == FileKind::Relocatable;
    prop_assert_eq!(elf.e_type, if relocatable { 1 } else { 2 });
    prop_assert_eq!(
        elf.machine,
        if obj.architecture() == Architecture::X86_64 {
            62
        } else {
            183
        }
    );
    prop_assert_eq!(elf.entry, obj.entry());
    prop_assert_eq!(&elf.sections[elf.shstrndx].name, ".shstrtab");

    // Content sections are headers 1..=n, in model order.
    for (i, s) in obj.sections().iter().enumerate() {
        let h = &elf.sections[i + 1];
        prop_assert_eq!(&h.name, s.name());
        prop_assert_eq!(h.ty, elf_section_type(s.kind()));
        prop_assert_eq!(h.flags, elf_flags(s.flags()));
        prop_assert_eq!(h.addr, s.address());
        prop_assert_eq!(h.size, s.size());
        prop_assert_eq!(h.align, s.align());
        prop_assert_eq!(h.entsize, s.entry_size());
        prop_assert_eq!((h.link, h.info), (0, 0));
        if !s.kind().is_uninitialized() {
            prop_assert_eq!(elf.contents(bytes, i + 1), s.data());
        }
    }

    // Symbol table: null, section symbols, locals, then the rest; sh_info = first
    // non-local.
    let symtab = elf.sections.iter().find(|h| h.ty == 2).unwrap();
    let raw = elf.symbols(bytes);
    prop_assert_eq!(
        &raw[0],
        &common::RawSymbol {
            name: String::new(),
            info: 0,
            other: 0,
            shndx: 0,
            value: 0,
            size: 0
        }
    );
    let first_global = symtab.info as usize;
    prop_assert!(raw[1..first_global].iter().all(|s| s.info >> 4 == 0));
    prop_assert!(raw[first_global..].iter().all(|s| s.info >> 4 != 0));
    let named: Vec<&common::RawSymbol> = raw[1..].iter().filter(|s| s.info & 0xf != 3).collect();
    let expected = elf_order(obj);
    prop_assert_eq!(named.len(), expected.symbols().len());
    for (got, want) in named.iter().zip(expected.symbols()) {
        prop_assert_eq!(&got.name, &want.name);
        prop_assert_eq!(got.value, want.value);
        prop_assert_eq!(got.size, want.size);
        let bind = match want.binding {
            Binding::Local => 0,
            Binding::Global => 1,
            _ => 2,
        };
        let ty = match want.kind {
            SymbolKind::NoType => 0,
            SymbolKind::Data => 1,
            SymbolKind::Function => 2,
            SymbolKind::File => 4,
            _ => 6,
        };
        prop_assert_eq!(got.info, (bind << 4) | ty);
        let vis = match want.visibility {
            Visibility::Default => 0,
            Visibility::Hidden => 2,
            _ => 3,
        };
        prop_assert_eq!(got.other, vis);
        let shndx = match want.section {
            SymbolSection::Undefined => 0,
            SymbolSection::Absolute => 0xfff1,
            SymbolSection::Common => 0xfff2,
            SymbolSection::Section(id) => id.index() as u32 + 1,
            _ => unreachable!(),
        };
        prop_assert_eq!(got.shndx, shndx);
    }

    // Relocations: one SHT_RELA per patched section, linked to .symtab, entries in
    // order, symbols resolving to the right target.
    let relocations = elf.relocations(bytes);
    let symtab_index = elf.sections.iter().position(|h| h.ty == 2).unwrap() as u32;
    for h in elf.sections.iter().filter(|h| h.ty == 4) {
        prop_assert_eq!(h.link, symtab_index);
        prop_assert_eq!(h.flags, 0x40);
        prop_assert_eq!(
            &h.name,
            &format!(".rela{}", elf.sections[h.info as usize].name)
        );
    }
    for (i, s) in obj.sections().iter().enumerate() {
        let got = relocations.get(&(i + 1)).cloned().unwrap_or_default();
        prop_assert_eq!(got.len(), s.relocations().len());
        for (raw_reloc, want) in got.iter().zip(s.relocations()) {
            prop_assert_eq!(raw_reloc.offset, want.offset);
            prop_assert_eq!(raw_reloc.addend, want.addend);
            prop_assert_eq!(raw_reloc.ty, elf_reloc_type(want.kind));
            let sym = &raw[raw_reloc.sym as usize];
            match want.target {
                RelocationTarget::Symbol(id) => {
                    prop_assert_eq!(&sym.name, &obj.symbols()[id.index()].name);
                    prop_assert!(sym.info & 0xf != 3);
                }
                RelocationTarget::Section(id) => {
                    prop_assert_eq!(sym.info, 3);
                    prop_assert_eq!(sym.shndx, id.index() as u32 + 1);
                }
                _ => unreachable!(),
            }
        }
    }

    if relocatable {
        let note = elf.index_of(".note.GNU-stack").unwrap();
        prop_assert_eq!(
            elf.sections[note].flags,
            if obj.executable_stack() { 4 } else { 0 }
        );
        prop_assert_eq!(elf.sections[note].size, 0);
        prop_assert!(elf.segments.is_empty());
    } else {
        check_loaded(obj, bytes, &elf)?;
    }
    Ok(())
}

/// Simulates the loader and checks every section is where the model says.
fn check_loaded(obj: &Object, bytes: &[u8], elf: &common::RawElf) -> Result<(), TestCaseError> {
    let page = obj.architecture().page_size();
    let memory = common::Memory::load(bytes, elf, page);
    let stack = elf.segments.iter().find(|s| s.ty == 0x6474_e551).unwrap();
    prop_assert_eq!(stack.flags & 1 != 0, obj.executable_stack());
    // The headers are mapped, so AT_PHDR is valid.
    let phdr_in_memory = memory.read(
        elf.segments.iter().find(|s| s.ty == 1).unwrap().vaddr + 64,
        56 * elf.segments.len() as u64,
    );
    prop_assert!(phdr_in_memory.is_some());

    for s in obj.sections() {
        if !s.flags().contains(SectionFlags::ALLOC) || s.size() == 0 {
            continue;
        }
        let (got, flags) = memory.read(s.address(), s.size()).unwrap();
        if s.kind().is_uninitialized() {
            prop_assert!(got.iter().all(|&b| b == 0));
        } else {
            prop_assert_eq!(&got[..], s.data());
        }
        let want =
            4 | if s.flags().contains(SectionFlags::WRITE) {
                2
            } else {
                0
            } | if s.flags().contains(SectionFlags::EXEC) {
                1
            } else {
                0
            };
        prop_assert!(
            flags.iter().all(|&f| f == want),
            "permissions of {}",
            s.name()
        );
    }
    let (_, entry_flags) = memory.read(obj.entry(), 1).unwrap();
    prop_assert_eq!(entry_flags[0] & 1, 1);
    Ok(())
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

fn mutate(bytes: &mut Vec<u8>, edits: &[(usize, u8)], cut: Option<usize>) {
    if bytes.is_empty() {
        return;
    }
    for &(at, value) in edits {
        let at = at % bytes.len();
        bytes[at] = value;
    }
    if let Some(cut) = cut {
        bytes.truncate(cut % (bytes.len() + 1));
    }
}

proptest! {
    // 1024 cases by default; PROPTEST_CASES overrides for deeper one-off runs.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(1024),
        ..ProptestConfig::default()
    })]

    #[test]
    fn relocatable_objects_round_trip(obj in relocatable()) {
        obj.validate().unwrap();
        let bytes = object_lang::elf::write(&obj).unwrap();
        let back = object_lang::elf::read(&bytes).unwrap();
        prop_assert_eq!(&back, &elf_order(&obj));
        // Deterministic, and a read/write cycle is the identity on bytes.
        prop_assert_eq!(object_lang::elf::write(&obj.clone()).unwrap(), bytes.clone());
        prop_assert_eq!(object_lang::elf::write(&back).unwrap(), bytes);
    }

    #[test]
    fn relocatable_output_matches_the_reference_decoder(obj in relocatable()) {
        let bytes = object_lang::elf::write(&obj).unwrap();
        check_against_reference(&obj, &bytes)?;
    }

    #[test]
    fn executables_round_trip_and_load_as_modelled(obj in executable()) {
        obj.validate().unwrap();
        let bytes = object_lang::elf::write(&obj).unwrap();
        check_against_reference(&obj, &bytes)?;
        let back = object_lang::elf::read(&bytes).unwrap();
        prop_assert_eq!(&back, &elf_order(&obj));
        prop_assert_eq!(object_lang::elf::write(&back).unwrap(), bytes);
    }

    #[test]
    fn reader_never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = object_lang::elf::read(&bytes);
        // And with a valid identification, so the header checks are passed more often.
        let mut elf = b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        elf.extend_from_slice(&bytes);
        let _ = object_lang::elf::read(&elf);
    }

    #[test]
    fn reader_survives_mutation_and_accepts_only_what_it_can_write_back(
        obj in prop_oneof![relocatable(), executable()],
        edits in prop::collection::vec((any::<usize>(), any::<u8>()), 1..6),
        cut in prop::option::of(any::<usize>()),
    ) {
        let mut bytes = object_lang::elf::write(&obj).unwrap();
        mutate(&mut bytes, &edits, cut);
        if let Ok(read) = object_lang::elf::read(&bytes) {
            // Whatever is accepted is a valid model...
            prop_assert!(read.validate().is_ok());
            // ...that (for relocatable objects, whose layout is unconstrained) the
            // writer writes, and that reads back unchanged.
            match object_lang::elf::write(&read) {
                Ok(again) => {
                    prop_assert_eq!(object_lang::elf::read(&again).unwrap(), read);
                }
                Err(err) => prop_assert!(
                    read.kind() == FileKind::Executable,
                    "relocatable object read but not writable: {err}"
                ),
            }
        }
    }
}
