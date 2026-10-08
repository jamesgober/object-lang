//! The ELF64 writer: relocatable objects (`ET_REL`) and static executables (`ET_EXEC`).
//!
//! Both kinds are written in one forward pass over the output buffer. Everything whose
//! position must be known before it is written (the program headers' segment offsets)
//! is computed first by pure arithmetic; everything recorded only at the end (the
//! section header table, and the header fields pointing at it) is patched in last.

use alloc::string::String;
use alloc::vec::Vec;

use super::consts::{
    EHDR_SIZE, ELFCLASS64, ELFDATA2LSB, ELFMAG, ELFOSABI_NONE, ET_EXEC, ET_REL, EV_CURRENT,
    GNU_STACK_ALIGN, GNU_STACK_NAME, PF_R, PF_W, PF_X, PHDR_SIZE, PT_GNU_STACK, PT_LOAD, RELA_SIZE,
    SHDR_SIZE, SHF_ALLOC, SHF_EXECINSTR, SHF_INFO_LINK, SHF_MERGE, SHF_STRINGS, SHF_TLS, SHF_WRITE,
    SHN_ABS, SHN_COMMON, SHN_LORESERVE, SHN_UNDEF, SHN_XINDEX, SHT_FINI_ARRAY, SHT_INIT_ARRAY,
    SHT_NOBITS, SHT_NOTE, SHT_PREINIT_ARRAY, SHT_PROGBITS, SHT_RELA, SHT_STRTAB, SHT_SYMTAB,
    SHT_SYMTAB_SHNDX, STB_GLOBAL, STB_LOCAL, STB_WEAK, STT_FILE, STT_FUNC, STT_NOTYPE, STT_OBJECT,
    STT_SECTION, STT_TLS, STV_DEFAULT, STV_HIDDEN, STV_PROTECTED, SYM_SIZE, machine, reloc_type,
};
use super::strtab;
use crate::error::WriteError;
use crate::model::{FileKind, Object};
use crate::reloc::RelocationTarget;
use crate::section::{Section, SectionFlags, SectionKind, align_up};
use crate::symbol::{Binding, SymbolKind, SymbolSection, Visibility};

/// File offsets of section contents are aligned to the section's alignment, capped
/// here: beyond a page, aligning the file offset helps no reader and only pads the file.
const MAX_FILE_ALIGN: u64 = 4096;

/// A section header, as written.
#[derive(Clone, Copy, Default)]
struct Shdr {
    name: u32,
    ty: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
}

pub(crate) fn write_into(obj: &Object, out: &mut Vec<u8>) -> Result<(), WriteError> {
    obj.validate()?;
    let base = out.len();
    let result = match obj.kind() {
        FileKind::Relocatable => write_relocatable(obj, out, base),
        FileKind::Executable => write_executable(obj, out, base),
    };
    if result.is_err() {
        // Leave the caller's buffer as it was.
        out.truncate(base);
    }
    result
}

// ---------------------------------------------------------------------------
// Output buffer
// ---------------------------------------------------------------------------

/// Appends to the caller's buffer, measuring positions from where the file starts.
struct Out<'a> {
    buf: &'a mut Vec<u8>,
    base: usize,
}

impl Out<'_> {
    fn pos(&self) -> u64 {
        (self.buf.len() - self.base) as u64
    }

    /// Pads with zeros up to file offset `offset`, which layout guarantees is not
    /// behind the current position.
    fn pad_to(&mut self, offset: u64) -> Result<(), WriteError> {
        let target = usize::try_from(offset)
            .ok()
            .and_then(|o| o.checked_add(self.base))
            .ok_or(WriteError::TooLarge)?;
        if target < self.buf.len() {
            return Err(WriteError::TooLarge);
        }
        self.buf
            .try_reserve(target - self.buf.len())
            .map_err(|_| WriteError::TooLarge)?;
        self.buf.resize(target, 0);
        Ok(())
    }

    fn align(&mut self, align: u64) -> Result<u64, WriteError> {
        let offset = align_up(self.pos(), align).ok_or(WriteError::TooLarge)?;
        self.pad_to(offset)?;
        Ok(offset)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    /// Overwrites bytes already written at file offset `offset`.
    fn patch(&mut self, offset: u64, bytes: &[u8]) -> Result<(), WriteError> {
        let start = usize::try_from(offset)
            .ok()
            .and_then(|o| o.checked_add(self.base))
            .ok_or(WriteError::TooLarge)?;
        let end = start.checked_add(bytes.len()).ok_or(WriteError::TooLarge)?;
        let slot = self.buf.get_mut(start..end).ok_or(WriteError::TooLarge)?;
        slot.copy_from_slice(bytes);
        Ok(())
    }

    fn shdr(&mut self, h: &Shdr) {
        self.u32(h.name);
        self.u32(h.ty);
        self.u64(h.flags);
        self.u64(h.addr);
        self.u64(h.offset);
        self.u64(h.size);
        self.u32(h.link);
        self.u32(h.info);
        self.u64(h.align);
        self.u64(h.entsize);
    }
}

// ---------------------------------------------------------------------------
// Pieces shared by both file kinds
// ---------------------------------------------------------------------------

fn u32_of(value: usize) -> Result<u32, WriteError> {
    u32::try_from(value).map_err(|_| WriteError::TooLarge)
}

fn u64_of(value: usize) -> u64 {
    // usize is at most 64 bits on every supported target.
    value as u64
}

/// The ELF section header index of the model's section `index`: user sections come
/// first, right after the null header.
fn header_index(index: usize) -> Result<u32, WriteError> {
    index
        .checked_add(1)
        .and_then(|i| u32::try_from(i).ok())
        .ok_or(WriteError::TooLarge)
}

fn section_type(kind: SectionKind) -> u32 {
    match kind {
        SectionKind::Text | SectionKind::Data | SectionKind::ReadOnlyData | SectionKind::Other => {
            SHT_PROGBITS
        }
        SectionKind::Bss => SHT_NOBITS,
        SectionKind::Note => SHT_NOTE,
        SectionKind::InitArray => SHT_INIT_ARRAY,
        SectionKind::FiniArray => SHT_FINI_ARRAY,
        SectionKind::PreinitArray => SHT_PREINIT_ARRAY,
    }
}

pub(crate) fn section_flags(flags: SectionFlags) -> u64 {
    let mut out = 0;
    for (flag, bit) in [
        (SectionFlags::ALLOC, SHF_ALLOC),
        (SectionFlags::WRITE, SHF_WRITE),
        (SectionFlags::EXEC, SHF_EXECINSTR),
        (SectionFlags::TLS, SHF_TLS),
        (SectionFlags::MERGE, SHF_MERGE),
        (SectionFlags::STRINGS, SHF_STRINGS),
    ] {
        if flags.contains(flag) {
            out |= bit;
        }
    }
    out
}

/// A symbol's `st_shndx`: a reserved value, or a real section header index (which
/// becomes `SHN_XINDEX` plus an `SHT_SYMTAB_SHNDX` entry when it does not fit 16 bits).
#[derive(Clone, Copy)]
enum Shndx {
    Special(u16),
    Index(u32),
}

/// One symbol table entry, as written (the name is resolved separately).
#[derive(Clone, Copy)]
struct Sym {
    info: u8,
    other: u8,
    shndx: Shndx,
    value: u64,
    size: u64,
}

/// The ELF symbol table for an object: entries in ELF order, names, and the maps from
/// model ids to ELF indices.
struct SymbolTable<'a> {
    /// Entries after the null symbol, in ELF order.
    entries: Vec<Sym>,
    /// Names, parallel to the full table (index 0 is the null symbol's empty name).
    names: Vec<&'a str>,
    /// Index of the first non-local symbol (`sh_info` of `.symtab`).
    first_global: u32,
    /// ELF index of each model symbol.
    of_symbol: Vec<u32>,
    /// ELF index of each section's `STT_SECTION` symbol, or 0 if it has none.
    of_section: Vec<u32>,
    /// Whether some symbol needs `SHN_XINDEX`, and so a `.symtab_shndx` section.
    needs_shndx: bool,
}

impl<'a> SymbolTable<'a> {
    /// Orders the symbol table: the null symbol, then a section symbol for every
    /// section a relocation targets directly (in section order), then the model's
    /// local symbols, then the non-local ones, each group keeping model order. ELF
    /// requires every local symbol to precede every non-local one.
    fn build(obj: &'a Object) -> Result<Self, WriteError> {
        let sections = obj.sections();
        let symbols = obj.symbols();

        let mut targeted = alloc::vec![false; sections.len()];
        for section in sections {
            for reloc in section.relocations() {
                if let RelocationTarget::Section(id) = reloc.target {
                    if let Some(slot) = targeted.get_mut(id.index()) {
                        *slot = true;
                    }
                }
            }
        }

        let capacity = symbols.len() + targeted.iter().filter(|&&t| t).count() + 1;
        let mut table = SymbolTable {
            entries: Vec::with_capacity(capacity),
            names: Vec::with_capacity(capacity),
            first_global: 0,
            of_symbol: alloc::vec![0; symbols.len()],
            of_section: alloc::vec![0; sections.len()],
            needs_shndx: false,
        };
        table.names.push("");

        for (index, &is_target) in targeted.iter().enumerate() {
            if !is_target {
                continue;
            }
            let shndx = header_index(index)?;
            let elf_index = table.push(
                "",
                Sym {
                    info: (STB_LOCAL << 4) | STT_SECTION,
                    other: STV_DEFAULT,
                    shndx: Shndx::Index(shndx),
                    value: 0,
                    size: 0,
                },
            )?;
            if let Some(slot) = table.of_section.get_mut(index) {
                *slot = elf_index;
            }
        }

        for pass_locals in [true, false] {
            if !pass_locals {
                table.first_global = u32_of(table.names.len())?;
            }
            for (index, symbol) in symbols.iter().enumerate() {
                if (symbol.binding == Binding::Local) != pass_locals {
                    continue;
                }
                let bind = match symbol.binding {
                    Binding::Local => STB_LOCAL,
                    Binding::Global => STB_GLOBAL,
                    Binding::Weak => STB_WEAK,
                };
                let ty = match symbol.kind {
                    SymbolKind::NoType => STT_NOTYPE,
                    SymbolKind::Function => STT_FUNC,
                    SymbolKind::Data => STT_OBJECT,
                    SymbolKind::Tls => STT_TLS,
                    SymbolKind::File => STT_FILE,
                };
                let other = match symbol.visibility {
                    Visibility::Default => STV_DEFAULT,
                    Visibility::Hidden => STV_HIDDEN,
                    Visibility::Protected => STV_PROTECTED,
                };
                let shndx = match symbol.section {
                    SymbolSection::Undefined => Shndx::Special(SHN_UNDEF),
                    SymbolSection::Absolute => Shndx::Special(SHN_ABS),
                    SymbolSection::Common => Shndx::Special(SHN_COMMON),
                    SymbolSection::Section(id) => Shndx::Index(header_index(id.index())?),
                };
                let elf_index = table.push(
                    &symbol.name,
                    Sym {
                        info: (bind << 4) | ty,
                        other,
                        shndx,
                        value: symbol.value,
                        size: symbol.size,
                    },
                )?;
                if let Some(slot) = table.of_symbol.get_mut(index) {
                    *slot = elf_index;
                }
            }
        }
        Ok(table)
    }

    fn push(&mut self, name: &'a str, sym: Sym) -> Result<u32, WriteError> {
        let index = u32_of(self.names.len())?;
        if let Shndx::Index(i) = sym.shndx {
            if i >= SHN_LORESERVE {
                self.needs_shndx = true;
            }
        }
        self.names.push(name);
        self.entries.push(sym);
        Ok(index)
    }

    fn count(&self) -> u64 {
        u64_of(self.names.len())
    }

    /// Writes `.symtab` at the current (8-aligned) position.
    fn write_symtab(&self, out: &mut Out<'_>, name_offsets: &[u32]) {
        out.bytes(&[0; SYM_SIZE as usize]);
        for (sym, &name) in self.entries.iter().zip(name_offsets.iter().skip(1)) {
            out.u32(name);
            out.bytes(&[sym.info, sym.other]);
            out.u16(match sym.shndx {
                Shndx::Special(v) => v,
                Shndx::Index(i) if i >= SHN_LORESERVE => SHN_XINDEX,
                Shndx::Index(i) => i as u16,
            });
            out.u64(sym.value);
            out.u64(sym.size);
        }
    }

    /// Writes `.symtab_shndx`: the full section index of every `SHN_XINDEX` symbol,
    /// zero for every other.
    fn write_shndx(&self, out: &mut Out<'_>) {
        out.u32(0);
        for sym in &self.entries {
            out.u32(match sym.shndx {
                Shndx::Index(i) if i >= SHN_LORESERVE => i,
                _ => 0,
            });
        }
    }
}

/// Writes the trailing tables every file has (`.symtab`, `.symtab_shndx` if needed,
/// `.strtab`, `.shstrtab`), then the section header table, then patches the ELF
/// header's section fields. `headers` holds the null header and every header before
/// `.symtab`, with names not yet resolved; `names` holds their names in order.
fn finish(
    out: &mut Out<'_>,
    symbols: &SymbolTable<'_>,
    mut headers: Vec<Shdr>,
    mut names: Vec<&str>,
) -> Result<(), WriteError> {
    let (strtab, name_offsets) = strtab::build(&symbols.names).ok_or(WriteError::TooLarge)?;

    let symtab_index = u32_of(headers.len())?;
    let strtab_index = symtab_index
        .checked_add(if symbols.needs_shndx { 2 } else { 1 })
        .ok_or(WriteError::TooLarge)?;
    let shstrtab_index = strtab_index.checked_add(1).ok_or(WriteError::TooLarge)?;

    let symtab_size = symbols
        .count()
        .checked_mul(SYM_SIZE)
        .ok_or(WriteError::TooLarge)?;
    let symtab_offset = out.align(8)?;
    symbols.write_symtab(out, &name_offsets);
    headers.push(Shdr {
        ty: SHT_SYMTAB,
        offset: symtab_offset,
        size: symtab_size,
        link: strtab_index,
        info: symbols.first_global,
        align: 8,
        entsize: SYM_SIZE,
        ..Shdr::default()
    });
    names.push(".symtab");

    if symbols.needs_shndx {
        let offset = out.align(4)?;
        symbols.write_shndx(out);
        headers.push(Shdr {
            ty: SHT_SYMTAB_SHNDX,
            offset,
            size: symbols.count().checked_mul(4).ok_or(WriteError::TooLarge)?,
            link: symtab_index,
            align: 4,
            entsize: 4,
            ..Shdr::default()
        });
        names.push(".symtab_shndx");
    }

    let strtab_offset = out.pos();
    out.bytes(&strtab);
    headers.push(Shdr {
        ty: SHT_STRTAB,
        offset: strtab_offset,
        size: u64_of(strtab.len()),
        align: 1,
        ..Shdr::default()
    });
    names.push(".strtab");

    names.push(".shstrtab");
    let (shstrtab, section_name_offsets) = strtab::build(&names).ok_or(WriteError::TooLarge)?;
    let shstrtab_offset = out.pos();
    out.bytes(&shstrtab);
    headers.push(Shdr {
        ty: SHT_STRTAB,
        offset: shstrtab_offset,
        size: u64_of(shstrtab.len()),
        align: 1,
        ..Shdr::default()
    });

    for (header, &name) in headers.iter_mut().zip(&section_name_offsets).skip(1) {
        header.name = name;
    }

    let shnum = u32_of(headers.len())?;
    // Extended numbering: counts and indices that do not fit 16 bits go in the null
    // section header, with the ELF header fields set to the escape values.
    let (e_shnum, sh0_size) = if shnum >= SHN_LORESERVE {
        (0u16, u64::from(shnum))
    } else {
        (shnum as u16, 0)
    };
    let (e_shstrndx, sh0_link) = if shstrtab_index >= SHN_LORESERVE {
        (SHN_XINDEX, shstrtab_index)
    } else {
        (shstrtab_index as u16, 0)
    };
    if let Some(null) = headers.first_mut() {
        null.size = sh0_size;
        null.link = sh0_link;
    }

    let shoff = out.align(8)?;
    for header in &headers {
        out.shdr(header);
    }

    // e_shoff at 0x28, e_shnum at 0x3c, e_shstrndx at 0x3e.
    out.patch(0x28, &shoff.to_le_bytes())?;
    out.patch(0x3c, &e_shnum.to_le_bytes())?;
    out.patch(0x3e, &e_shstrndx.to_le_bytes())?;
    Ok(())
}

/// Writes the ELF header with the section fields zeroed; [`finish`] patches them.
fn write_ehdr(out: &mut Out<'_>, obj: &Object, ty: u16, phnum: u16) {
    out.bytes(&ELFMAG);
    out.bytes(&[ELFCLASS64, ELFDATA2LSB, EV_CURRENT, ELFOSABI_NONE, 0]);
    out.bytes(&[0; 7]);
    out.u16(ty);
    out.u16(machine(obj.architecture()));
    out.u32(u32::from(EV_CURRENT));
    out.u64(obj.entry());
    out.u64(if phnum > 0 { EHDR_SIZE } else { 0 }); // e_phoff
    out.u64(0); // e_shoff, patched
    out.u32(0); // e_flags
    out.u16(EHDR_SIZE as u16);
    out.u16(if phnum > 0 { PHDR_SIZE as u16 } else { 0 });
    out.u16(phnum);
    out.u16(SHDR_SIZE as u16);
    out.u16(0); // e_shnum, patched
    out.u16(0); // e_shstrndx, patched
}

/// The section header for a model section, without name or file offset.
fn user_header(section: &Section) -> Shdr {
    Shdr {
        ty: section_type(section.kind()),
        flags: section_flags(section.flags()),
        addr: section.address(),
        size: section.size(),
        align: section.align(),
        entsize: section.entry_size(),
        ..Shdr::default()
    }
}

// ---------------------------------------------------------------------------
// Relocatable objects
// ---------------------------------------------------------------------------

fn write_relocatable(obj: &Object, buf: &mut Vec<u8>, base: usize) -> Result<(), WriteError> {
    let sections = obj.sections();
    for (index, section) in sections.iter().enumerate() {
        if section.name() == GNU_STACK_NAME {
            return Err(WriteError::ReservedSectionName {
                section: crate::model::SectionId(u32_of(index)?),
            });
        }
    }
    let symbols = SymbolTable::build(obj)?;

    buf.try_reserve(estimate_size(obj))
        .map_err(|_| WriteError::TooLarge)?;
    let mut out = Out { buf, base };
    write_ehdr(&mut out, obj, ET_REL, 0);

    let mut headers: Vec<Shdr> = Vec::with_capacity(sections.len() * 2 + 6);
    let mut names: Vec<&str> = Vec::with_capacity(sections.len() * 2 + 6);
    headers.push(Shdr::default());
    names.push("");

    for section in sections {
        let mut header = user_header(section);
        header.offset = if section.kind().is_uninitialized() {
            out.pos()
        } else {
            let offset = out.align(section.align().min(MAX_FILE_ALIGN))?;
            out.bytes(section.data());
            offset
        };
        headers.push(header);
        names.push(section.name());
    }

    // The relocation sections' names must outlive `names`.
    let rela_names: Vec<String> = sections
        .iter()
        .filter(|s| !s.relocations().is_empty())
        .map(|s| {
            let mut name = String::with_capacity(5 + s.name().len());
            name.push_str(".rela");
            name.push_str(s.name());
            name
        })
        .collect();

    // Index the symbol table will have: after the user sections, the relocation
    // sections, and `.note.GNU-stack`.
    let symtab_index = u32_of(sections.len() + rela_names.len() + 2)?;
    let mut rela_name = rela_names.iter();
    for (index, section) in sections.iter().enumerate() {
        if section.relocations().is_empty() {
            continue;
        }
        let offset = out.align(8)?;
        for reloc in section.relocations() {
            let sym = match reloc.target {
                RelocationTarget::Symbol(id) => symbols.of_symbol.get(id.index()),
                RelocationTarget::Section(id) => symbols.of_section.get(id.index()),
            };
            let sym = u64::from(*sym.ok_or(WriteError::TooLarge)?);
            out.u64(reloc.offset);
            out.u64((sym << 32) | u64::from(reloc_type(reloc.kind)));
            out.bytes(&reloc.addend.to_le_bytes());
        }
        headers.push(Shdr {
            ty: SHT_RELA,
            flags: SHF_INFO_LINK,
            offset,
            size: u64_of(section.relocations().len())
                .checked_mul(RELA_SIZE)
                .ok_or(WriteError::TooLarge)?,
            link: symtab_index,
            info: header_index(index)?,
            align: 8,
            entsize: RELA_SIZE,
            ..Shdr::default()
        });
        names.push(rela_name.next().map_or("", String::as_str));
    }

    headers.push(Shdr {
        ty: SHT_PROGBITS,
        flags: if obj.executable_stack() {
            SHF_EXECINSTR
        } else {
            0
        },
        offset: out.pos(),
        align: 1,
        ..Shdr::default()
    });
    names.push(GNU_STACK_NAME);

    finish(&mut out, &symbols, headers, names)
}

/// A capacity hint: the bytes of every section and table, plus some padding.
fn estimate_size(obj: &Object) -> usize {
    let mut total: usize = 1024;
    for section in obj.sections() {
        total = total
            .saturating_add(section.data().len())
            .saturating_add(
                section
                    .relocations()
                    .len()
                    .saturating_mul(RELA_SIZE as usize),
            )
            .saturating_add(section.name().len().saturating_mul(2))
            .saturating_add(SHDR_SIZE as usize * 2 + 16);
    }
    for symbol in obj.symbols() {
        total = total
            .saturating_add(SYM_SIZE as usize + 1)
            .saturating_add(symbol.name.len());
    }
    // Do not trust the hint with more than the data could need; padding is bounded.
    total.min(isize::MAX as usize / 2)
}

// ---------------------------------------------------------------------------
// Executables
// ---------------------------------------------------------------------------

/// One `PT_LOAD` segment: a run of loaded sections with one set of permissions.
struct Segment {
    /// Range of `loaded` (sections sorted by address) this segment covers.
    first: usize,
    end: usize,
    vaddr: u64,
    /// Bytes backed by the file, from `vaddr`.
    filesz: u64,
    /// Bytes in memory, from `vaddr`.
    memsz: u64,
    flags: u32,
    /// Whether the last section is BSS (only BSS may follow it in this segment).
    ends_in_bss: bool,
    offset: u64,
}

fn permissions(section: &Section) -> u32 {
    let flags = section.flags();
    let mut p = PF_R;
    if flags.contains(SectionFlags::WRITE) {
        p |= PF_W;
    }
    if flags.contains(SectionFlags::EXEC) {
        p |= PF_X;
    }
    p
}

/// Groups the loaded sections (sorted by address) into segments. A new segment starts
/// where permissions change, where initialized data follows BSS (BSS has no file bytes
/// and must end its segment), or where a gap of a page or more opens (to keep the file
/// from filling the gap with zeros). Each new segment must start on a page the previous
/// one does not touch.
fn plan_segments(
    sections: &[Section],
    loaded: &[u32],
    page: u64,
) -> Result<Vec<Segment>, WriteError> {
    let mut segments: Vec<Segment> = Vec::new();
    for (position, &index) in loaded.iter().enumerate() {
        let Some(section) = sections.get(index as usize) else {
            continue;
        };
        let addr = section.address();
        let end = addr
            .checked_add(section.size())
            .ok_or(WriteError::TooLarge)?;
        let bss = section.kind().is_uninitialized();
        let flags = permissions(section);

        let extend = match segments.last() {
            Some(seg) => {
                let seg_end = seg.vaddr + seg.memsz;
                let gap = addr.saturating_sub(seg_end);
                let starts_new = seg.flags != flags || (seg.ends_in_bss && !bss) || gap >= page;
                if starts_new {
                    let seg_end_page = align_up(seg_end, page).ok_or(WriteError::TooLarge)?;
                    if addr & !(page - 1) < seg_end_page {
                        let previous = position
                            .checked_sub(1)
                            .and_then(|p| loaded.get(p))
                            .copied()
                            .unwrap_or(index);
                        return Err(WriteError::SegmentsSharePage {
                            first: crate::model::SectionId(previous),
                            second: crate::model::SectionId(index),
                        });
                    }
                }
                !starts_new
            }
            None => false,
        };

        if !extend {
            segments.push(Segment {
                first: position,
                end: position,
                vaddr: addr,
                filesz: 0,
                memsz: 0,
                flags,
                ends_in_bss: false,
                offset: 0,
            });
        }
        if let Some(seg) = segments.last_mut() {
            seg.end = position + 1;
            seg.memsz = end - seg.vaddr;
            if !bss {
                seg.filesz = end - seg.vaddr;
            }
            seg.ends_in_bss = bss;
        }
    }
    Ok(segments)
}

fn write_executable(obj: &Object, buf: &mut Vec<u8>, base: usize) -> Result<(), WriteError> {
    let sections = obj.sections();
    let page = obj.architecture().page_size();

    let mut loaded: Vec<u32> = Vec::new();
    let mut empty_loaded: Vec<u32> = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        let flags = section.flags();
        if !flags.contains(SectionFlags::ALLOC) {
            continue;
        }
        if flags.contains(SectionFlags::TLS) {
            return Err(WriteError::Unsupported {
                what: "thread-local sections in executables",
            });
        }
        if section.size() == 0 {
            empty_loaded.push(u32_of(index)?);
        } else {
            loaded.push(u32_of(index)?);
        }
    }
    loaded.sort_unstable_by_key(|&i| (sections.get(i as usize).map_or(0, Section::address), i));

    let mut segments = plan_segments(sections, &loaded, page)?;
    // The headers' own segment, the content segments, and PT_GNU_STACK.
    let phnum = segments.len().checked_add(2).ok_or(WriteError::TooLarge)?;
    let phnum = u16::try_from(phnum)
        .ok()
        .filter(|&n| n < super::consts::PN_XNUM)
        .ok_or(WriteError::Unsupported {
            what: "more than 65534 program headers",
        })?;
    let header_size = EHDR_SIZE + PHDR_SIZE * u64::from(phnum);

    // The ELF header and program headers are mapped read-only on the page(s) just
    // below the first segment, so the loader's AT_PHDR points at real memory.
    let first_vaddr = segments.first().map_or(0, |s| s.vaddr);
    let header_span = align_up(header_size, page).ok_or(WriteError::TooLarge)?;
    let header_vaddr = (first_vaddr & !(page - 1))
        .checked_sub(header_span)
        .ok_or(WriteError::NoRoomForHeaders)?;

    // File offsets: each segment's offset is congruent to its address modulo the page
    // size, as the loader's mmap needs.
    let mut cursor = header_size;
    for seg in &mut segments {
        let delta = (seg.vaddr % page + page - cursor % page) % page;
        seg.offset = cursor.checked_add(delta).ok_or(WriteError::TooLarge)?;
        cursor = seg
            .offset
            .checked_add(seg.filesz)
            .ok_or(WriteError::TooLarge)?;
    }

    let symbols = SymbolTable::build(obj)?;
    buf.try_reserve(estimate_size(obj))
        .map_err(|_| WriteError::TooLarge)?;
    let mut out = Out { buf, base };
    write_ehdr(&mut out, obj, ET_EXEC, phnum);

    let mut phdr =
        |ty: u32, flags: u32, offset: u64, vaddr: u64, filesz: u64, memsz: u64, align: u64| {
            out.u32(ty);
            out.u32(flags);
            out.u64(offset);
            out.u64(vaddr);
            out.u64(vaddr);
            out.u64(filesz);
            out.u64(memsz);
            out.u64(align);
        };
    phdr(
        PT_LOAD,
        PF_R,
        0,
        header_vaddr,
        header_size,
        header_size,
        page,
    );
    for seg in &segments {
        phdr(
            PT_LOAD, seg.flags, seg.offset, seg.vaddr, seg.filesz, seg.memsz, page,
        );
    }
    let stack_flags = if obj.executable_stack() {
        PF_R | PF_W | PF_X
    } else {
        PF_R | PF_W
    };
    phdr(PT_GNU_STACK, stack_flags, 0, 0, 0, 0, GNU_STACK_ALIGN);

    let mut headers: Vec<Shdr> = alloc::vec![Shdr::default(); sections.len() + 1];
    let mut names: Vec<&str> = Vec::with_capacity(sections.len() + 5);
    names.push("");
    for (header, section) in headers.iter_mut().skip(1).zip(sections) {
        *header = user_header(section);
        names.push(section.name());
    }

    for seg in &segments {
        out.pad_to(seg.offset)?;
        for &index in loaded.get(seg.first..seg.end).unwrap_or(&[]) {
            let Some(section) = sections.get(index as usize) else {
                continue;
            };
            // Where the section sits in the segment's file image. For BSS past the
            // file bytes this is only nominal (`SHT_NOBITS` has no file contents), and
            // a huge BSS may put it past 2^64, so it saturates; initialized data lies
            // within the segment's file bytes, so its offset is exact.
            let offset = seg
                .offset
                .saturating_add(section.address().saturating_sub(seg.vaddr));
            if !section.kind().is_uninitialized() {
                out.pad_to(offset)?;
                out.bytes(section.data());
            }
            if let Some(header) = headers.get_mut(index as usize + 1) {
                header.offset = offset;
            }
        }
    }

    let after_segments = out.pos();
    for &index in &empty_loaded {
        if let Some(header) = headers.get_mut(index as usize + 1) {
            header.offset = after_segments;
        }
    }
    for (index, section) in sections.iter().enumerate() {
        if section.flags().contains(SectionFlags::ALLOC) {
            continue;
        }
        let offset = out.align(section.align().min(MAX_FILE_ALIGN))?;
        out.bytes(section.data());
        if let Some(header) = headers.get_mut(index + 1) {
            header.offset = offset;
        }
    }

    finish(&mut out, &symbols, headers, names)
}
