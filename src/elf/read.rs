//! The ELF64 reader: strict, budgeted, and panic-free on arbitrary bytes.
//!
//! Every offset and size from the file is checked with overflow-safe arithmetic before
//! it is used, every slice is taken with `get`, every count is checked against the
//! input length and the caller's [`Limits`] before anything is allocated for it, and
//! nothing recurses. What the reader accepts it turns into an [`Object`] and runs the
//! model's own validation on, so a file is accepted exactly when it decodes to an
//! object every writer could produce again.

use alloc::string::String;
use alloc::vec::Vec;

use super::consts::{
    EHDR_SIZE, ELFCLASS64, ELFDATA2LSB, ELFMAG, ELFOSABI_GNU, ELFOSABI_NONE, EM_AARCH64, EM_X86_64,
    ET_EXEC, ET_REL, EV_CURRENT, GNU_STACK_NAME, PF_W, PF_X, PHDR_SIZE, PN_XNUM, PT_GNU_STACK,
    PT_LOAD, PT_NULL, PT_PHDR, RELA_SIZE, SHDR_SIZE, SHF_ALLOC, SHF_EXECINSTR, SHF_INFO_LINK,
    SHF_MERGE, SHF_STRINGS, SHF_TLS, SHF_WRITE, SHN_ABS, SHN_COMMON, SHN_LORESERVE, SHN_UNDEF,
    SHN_XINDEX, SHT_FINI_ARRAY, SHT_INIT_ARRAY, SHT_LLVM_ADDRSIG, SHT_NOBITS, SHT_NOTE, SHT_NULL,
    SHT_PREINIT_ARRAY, SHT_PROGBITS, SHT_RELA, SHT_STRTAB, SHT_SYMTAB, SHT_SYMTAB_SHNDX,
    SHT_X86_64_UNWIND, STB_GLOBAL, STB_LOCAL, STB_WEAK, STT_FILE, STT_FUNC, STT_NOTYPE, STT_OBJECT,
    STT_SECTION, STT_TLS, STV_DEFAULT, STV_HIDDEN, STV_PROTECTED, SYM_SIZE, reloc_kind,
};
use crate::error::ReadError;
use crate::limits::Limits;
use crate::model::{Architecture, FileKind, Object, SectionId, SymbolId};
use crate::reloc::{Relocation, RelocationTarget};
use crate::section::{Section, SectionFlags, SectionKind};
use crate::symbol::{Binding, Symbol, SymbolKind, SymbolSection, Visibility};

// ---------------------------------------------------------------------------
// Byte access
// ---------------------------------------------------------------------------

const fn truncated(what: &'static str) -> ReadError {
    ReadError::Truncated { what }
}

const fn malformed(what: &'static str) -> ReadError {
    ReadError::Malformed { what }
}

const fn unsupported(what: &'static str, value: u64) -> ReadError {
    ReadError::Unsupported { what, value }
}

/// `size` bytes at `offset`, or `Truncated` if they run past the input.
fn slice<'a>(
    bytes: &'a [u8],
    offset: u64,
    size: u64,
    what: &'static str,
) -> Result<&'a [u8], ReadError> {
    let start = usize::try_from(offset).map_err(|_| truncated(what))?;
    let len = usize::try_from(size).map_err(|_| truncated(what))?;
    let end = start.checked_add(len).ok_or(truncated(what))?;
    bytes.get(start..end).ok_or(truncated(what))
}

/// Little-endian field readers over a record already bounds-checked to its full size;
/// they still return zero rather than panic if a caller slices short.
fn u16_at(b: &[u8], at: usize) -> u16 {
    let mut v = [0u8; 2];
    if let Some(src) = b.get(at..at + 2) {
        v.copy_from_slice(src);
    }
    u16::from_le_bytes(v)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    let mut v = [0u8; 4];
    if let Some(src) = b.get(at..at + 4) {
        v.copy_from_slice(src);
    }
    u32::from_le_bytes(v)
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut v = [0u8; 8];
    if let Some(src) = b.get(at..at + 8) {
        v.copy_from_slice(src);
    }
    u64::from_le_bytes(v)
}

/// Copies names out of string tables, enforcing the name budgets.
struct Names<'l> {
    limits: &'l Limits,
    used: u64,
}

impl Names<'_> {
    fn get<'a>(&mut self, table: &'a [u8], offset: u32) -> Result<&'a str, ReadError> {
        let rest = usize::try_from(offset)
            .ok()
            .and_then(|o| table.get(o..))
            .ok_or(malformed("name offset is past the end of its string table"))?;
        // Scan at most one byte past the longest allowed name.
        let max = usize::try_from(self.limits.max_name_len).unwrap_or(usize::MAX);
        let window = rest.get(..max.saturating_add(1)).unwrap_or(rest);
        let Some(len) = window.iter().position(|&b| b == 0) else {
            return Err(if window.len() > max {
                ReadError::LimitExceeded {
                    limit: "max_name_len",
                }
            } else {
                malformed("name is not NUL-terminated")
            });
        };
        self.used = self.used.saturating_add(len as u64);
        if self.used > self.limits.max_name_bytes {
            return Err(ReadError::LimitExceeded {
                limit: "max_name_bytes",
            });
        }
        let name = window.get(..len).unwrap_or(&[]);
        core::str::from_utf8(name).map_err(|_| malformed("name is not valid UTF-8"))
    }
}

// ---------------------------------------------------------------------------
// Headers
// ---------------------------------------------------------------------------

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

impl Shdr {
    fn parse(b: &[u8]) -> Self {
        Shdr {
            name: u32_at(b, 0),
            ty: u32_at(b, 4),
            flags: u64_at(b, 8),
            addr: u64_at(b, 16),
            offset: u64_at(b, 24),
            size: u64_at(b, 32),
            link: u32_at(b, 40),
            info: u32_at(b, 44),
            align: u64_at(b, 48),
            entsize: u64_at(b, 56),
        }
    }

    /// The section's bytes in the file.
    fn data<'a>(&self, bytes: &'a [u8], what: &'static str) -> Result<&'a [u8], ReadError> {
        slice(bytes, self.offset, self.size, what)
    }
}

#[derive(Clone, Copy)]
struct Phdr {
    ty: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

/// What each section header turned out to be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Null,
    /// A content section: index into the model's sections.
    User(u32),
    Symtab,
    Strtab,
    Rela,
    SymtabShndx,
    /// `.note.GNU-stack`, folded into `Object::executable_stack`.
    GnuStack,
    /// An advisory section dropped without changing the object's meaning.
    Dropped,
}

const KNOWN_SECTION_FLAGS: u64 =
    SHF_WRITE | SHF_ALLOC | SHF_EXECINSTR | SHF_MERGE | SHF_STRINGS | SHF_TLS;

fn model_flags(flags: u64) -> SectionFlags {
    let mut out = SectionFlags::empty();
    for (bit, flag) in [
        (SHF_ALLOC, SectionFlags::ALLOC),
        (SHF_WRITE, SectionFlags::WRITE),
        (SHF_EXECINSTR, SectionFlags::EXEC),
        (SHF_TLS, SectionFlags::TLS),
        (SHF_MERGE, SectionFlags::MERGE),
        (SHF_STRINGS, SectionFlags::STRINGS),
    ] {
        if flags & bit != 0 {
            out |= flag;
        }
    }
    out
}

/// The model kind of a content section, from its type and flags. For `SHT_PROGBITS`
/// the flags decide (see [`SectionKind`]); this is the inverse of the writer's mapping.
fn section_kind(ty: u32, flags: u64, arch: Architecture) -> Option<SectionKind> {
    Some(match ty {
        SHT_PROGBITS => progbits_kind(flags),
        // x86-64 `.eh_frame` is read-only data that the psABI gives its own type; it
        // is written back as SHT_PROGBITS, as GNU as does.
        SHT_X86_64_UNWIND if arch == Architecture::X86_64 => progbits_kind(flags),
        SHT_NOBITS => SectionKind::Bss,
        SHT_NOTE => SectionKind::Note,
        SHT_INIT_ARRAY => SectionKind::InitArray,
        SHT_FINI_ARRAY => SectionKind::FiniArray,
        SHT_PREINIT_ARRAY => SectionKind::PreinitArray,
        _ => return None,
    })
}

fn progbits_kind(flags: u64) -> SectionKind {
    if flags & SHF_ALLOC == 0 {
        SectionKind::Other
    } else if flags & SHF_EXECINSTR != 0 {
        SectionKind::Text
    } else if flags & SHF_WRITE != 0 {
        SectionKind::Data
    } else {
        SectionKind::ReadOnlyData
    }
}

// ---------------------------------------------------------------------------
// The reader
// ---------------------------------------------------------------------------

struct Header {
    kind: FileKind,
    arch: Architecture,
    entry: u64,
    phoff: u64,
    shoff: u64,
    phnum: u16,
    shnum: u16,
    shstrndx: u16,
}

fn parse_header(bytes: &[u8]) -> Result<Header, ReadError> {
    let h = slice(bytes, 0, EHDR_SIZE, "file header")?;
    if h.get(..4) != Some(&ELFMAG[..]) {
        return Err(ReadError::BadMagic);
    }
    let ident = |i: usize| h.get(i).copied().unwrap_or(0);
    if ident(4) != ELFCLASS64 {
        return Err(unsupported(
            "ELF class (only 64-bit is supported)",
            u64::from(ident(4)),
        ));
    }
    if ident(5) != ELFDATA2LSB {
        return Err(unsupported(
            "data encoding (only little-endian is supported)",
            u64::from(ident(5)),
        ));
    }
    if ident(6) != EV_CURRENT || u32_at(h, 20) != u32::from(EV_CURRENT) {
        return Err(malformed("ELF version is not 1"));
    }
    if ident(7) != ELFOSABI_NONE && ident(7) != ELFOSABI_GNU {
        return Err(unsupported("OS ABI", u64::from(ident(7))));
    }
    if ident(8) != 0 {
        return Err(unsupported("ABI version", u64::from(ident(8))));
    }
    let kind = match u16_at(h, 16) {
        ET_REL => FileKind::Relocatable,
        ET_EXEC => FileKind::Executable,
        other => return Err(unsupported("file type", u64::from(other))),
    };
    let arch = match u16_at(h, 18) {
        EM_X86_64 => Architecture::X86_64,
        EM_AARCH64 => Architecture::Aarch64,
        other => return Err(unsupported("machine", u64::from(other))),
    };
    let flags = u32_at(h, 48);
    if flags != 0 {
        return Err(unsupported(
            "processor-specific file flags",
            u64::from(flags),
        ));
    }
    if u64::from(u16_at(h, 52)) != EHDR_SIZE {
        return Err(malformed("e_ehsize is not 64"));
    }
    let header = Header {
        kind,
        arch,
        entry: u64_at(h, 24),
        phoff: u64_at(h, 32),
        shoff: u64_at(h, 40),
        phnum: u16_at(h, 56),
        shnum: u16_at(h, 60),
        shstrndx: u16_at(h, 62),
    };
    if header.phnum != 0 && u64::from(u16_at(h, 54)) != PHDR_SIZE {
        return Err(malformed("e_phentsize is not 56"));
    }
    if header.shoff != 0 && u64::from(u16_at(h, 58)) != SHDR_SIZE {
        return Err(malformed("e_shentsize is not 64"));
    }
    Ok(header)
}

/// Reads the section header table, resolving extended section numbering. Returns the
/// headers and the index of the section-name string table.
fn parse_section_headers(
    bytes: &[u8],
    header: &Header,
    limits: &Limits,
) -> Result<(Vec<Shdr>, usize), ReadError> {
    if header.shoff == 0 {
        return Err(malformed("the file has no section header table"));
    }
    let null = Shdr::parse(slice(
        bytes,
        header.shoff,
        SHDR_SIZE,
        "section header table",
    )?);
    let count = if header.shnum == 0 {
        // Extended numbering: the real count is in the null header's sh_size.
        null.size
    } else {
        if null.size != 0 {
            return Err(malformed("null section header has a size"));
        }
        u64::from(header.shnum)
    };
    let shstrndx = if header.shstrndx == SHN_XINDEX {
        u64::from(null.link)
    } else {
        if null.link != 0 {
            return Err(malformed("null section header has a link"));
        }
        if u32::from(header.shstrndx) >= SHN_LORESERVE {
            return Err(malformed("e_shstrndx is a reserved index"));
        }
        u64::from(header.shstrndx)
    };
    if null.ty != SHT_NULL
        || null.name != 0
        || null.flags != 0
        || null.addr != 0
        || null.offset != 0
        || null.info != 0
        || null.align != 0
        || null.entsize != 0
    {
        return Err(malformed("section header 0 is not a null header"));
    }
    if count == 0 {
        return Err(malformed("section header count is zero"));
    }
    if count > u64::from(limits.max_sections) {
        return Err(ReadError::LimitExceeded {
            limit: "max_sections",
        });
    }
    let table_size = count
        .checked_mul(SHDR_SIZE)
        .ok_or(truncated("section header table"))?;
    let table = slice(bytes, header.shoff, table_size, "section header table")?;
    let headers: Vec<Shdr> = table
        .chunks_exact(SHDR_SIZE as usize)
        .map(Shdr::parse)
        .collect();
    if shstrndx == 0 || shstrndx >= count {
        return Err(malformed("section name string table index is out of range"));
    }
    let shstrndx = usize::try_from(shstrndx).map_err(|_| malformed("section index overflow"))?;
    Ok((headers, shstrndx))
}

pub(crate) fn read(bytes: &[u8], limits: &Limits) -> Result<Object, ReadError> {
    let header = parse_header(bytes)?;
    let (headers, shstrndx) = parse_section_headers(bytes, &header, limits)?;
    let relocatable = header.kind == FileKind::Relocatable;
    if relocatable && header.phnum != 0 {
        return Err(malformed("a relocatable object has program headers"));
    }

    let shstrtab_header = headers.get(shstrndx).copied().unwrap_or_default();
    if shstrtab_header.ty != SHT_STRTAB {
        return Err(malformed("section name table is not a string table"));
    }
    let shstrtab = shstrtab_header.data(bytes, "section name string table")?;
    let mut names = Names { limits, used: 0 };

    // Pass 1: decide what every section header is, and build the content sections.
    let mut roles = alloc::vec![Role::Null; headers.len()];
    let mut sections: Vec<Section> = Vec::new();
    let mut file_offsets: Vec<u64> = Vec::new();
    let mut symtab: Option<usize> = None;
    let mut shndx: Option<usize> = None;
    let mut gnu_stack: Option<bool> = None;
    let mut data_budget = bytes.len() as u64;

    for (index, sh) in headers.iter().enumerate().skip(1) {
        let name = names.get(shstrtab, sh.name)?;
        let role = match sh.ty {
            SHT_NULL => return Err(malformed("an inactive section header follows the first")),
            SHT_SYMTAB => {
                if symtab.replace(index).is_some() {
                    return Err(malformed("more than one symbol table"));
                }
                Role::Symtab
            }
            SHT_STRTAB => Role::Strtab,
            SHT_RELA if !relocatable => {
                return Err(unsupported(
                    "relocation section in an executable",
                    index as u64,
                ));
            }
            SHT_RELA => Role::Rela,
            SHT_SYMTAB_SHNDX => {
                if shndx.replace(index).is_some() {
                    return Err(malformed("more than one extended section index table"));
                }
                Role::SymtabShndx
            }
            SHT_LLVM_ADDRSIG => Role::Dropped,
            ty if relocatable && name == GNU_STACK_NAME => {
                if ty != SHT_PROGBITS || sh.size != 0 || sh.flags & !SHF_EXECINSTR != 0 {
                    return Err(malformed(".note.GNU-stack is not an empty marker section"));
                }
                if gnu_stack.replace(sh.flags & SHF_EXECINSTR != 0).is_some() {
                    return Err(malformed("more than one .note.GNU-stack section"));
                }
                Role::GnuStack
            }
            ty => {
                let Some(kind) = section_kind(ty, sh.flags, header.arch) else {
                    return Err(unsupported("section type", u64::from(ty)));
                };
                if sh.flags & !KNOWN_SECTION_FLAGS != 0 {
                    return Err(unsupported("section flags", sh.flags));
                }
                if !relocatable && sh.flags & (SHF_ALLOC | SHF_TLS) == SHF_ALLOC | SHF_TLS {
                    return Err(unsupported(
                        "thread-local section in an executable",
                        index as u64,
                    ));
                }
                if sh.link != 0 || sh.info != 0 {
                    return Err(malformed("a content section has sh_link or sh_info set"));
                }
                let mut section = Section::new(String::from(name), kind)
                    .with_flags(model_flags(sh.flags))
                    .with_align(if sh.align == 0 { 1 } else { sh.align })
                    .with_entry_size(sh.entsize)
                    .with_address(sh.addr);
                if kind.is_uninitialized() {
                    section = section.with_bss_size(sh.size);
                } else {
                    // Section data may overlap in a hostile file; copying more than the
                    // whole input would let headers multiply memory use.
                    data_budget =
                        data_budget
                            .checked_sub(sh.size)
                            .ok_or(ReadError::LimitExceeded {
                                limit: "section data exceeds the input size",
                            })?;
                    section = section.with_data(sh.data(bytes, "section contents")?.to_vec());
                }
                let id = u32::try_from(sections.len()).map_err(|_| ReadError::LimitExceeded {
                    limit: "max_sections",
                })?;
                sections.push(section);
                file_offsets.push(sh.offset);
                Role::User(id)
            }
        };
        if let Some(slot) = roles.get_mut(index) {
            *slot = role;
        }
    }

    // Every string table must be the section-name table or the symbol table's names;
    // a string table nothing refers to has no place in the model.
    let symtab_header = symtab.and_then(|i| headers.get(i).copied());
    let strtab_index = symtab_header.map(|s| s.link as usize);
    for (index, role) in roles.iter().enumerate() {
        if *role == Role::Strtab && index != shstrndx && Some(index) != strtab_index {
            return Err(unsupported(
                "string table that nothing refers to",
                index as u64,
            ));
        }
    }

    // Pass 2: symbols.
    let mut symbols: Vec<Symbol> = Vec::new();
    // What each ELF symbol index is to relocations.
    let mut targets: Vec<Option<RelocationTarget>> = Vec::new();
    if let (Some(index), Some(sh)) = (symtab, symtab_header) {
        read_symbols(
            bytes,
            &headers,
            &roles,
            index,
            sh,
            shndx,
            &mut names,
            limits,
            &mut symbols,
            &mut targets,
        )?;
    } else if shndx.is_some() {
        return Err(malformed(
            "an extended section index table without a symbol table",
        ));
    }

    // Pass 3: relocations.
    let mut relocations: Vec<Vec<Relocation>> = alloc::vec![Vec::new(); sections.len()];
    let mut has_relocations = alloc::vec![false; sections.len()];
    let mut relocation_count: u64 = 0;
    for (index, sh) in headers.iter().enumerate() {
        if roles.get(index) != Some(&Role::Rela) {
            continue;
        }
        if Some(sh.link as usize) != symtab {
            return Err(malformed(
                "a relocation section is not linked to the symbol table",
            ));
        }
        if sh.flags & !SHF_INFO_LINK != 0 {
            return Err(unsupported("relocation section flags", sh.flags));
        }
        if sh.entsize != RELA_SIZE || sh.size % RELA_SIZE != 0 {
            return Err(malformed("relocation entry size is not 24"));
        }
        let target = match roles.get(sh.info as usize) {
            Some(Role::User(id)) => *id as usize,
            _ => {
                return Err(malformed(
                    "a relocation section patches a section with no contents",
                ));
            }
        };
        match has_relocations.get_mut(target) {
            Some(seen) if !*seen => *seen = true,
            _ => return Err(malformed("two relocation sections patch the same section")),
        }
        let count = sh.size / RELA_SIZE;
        relocation_count = relocation_count.saturating_add(count);
        if relocation_count > limits.max_relocations {
            return Err(ReadError::LimitExceeded {
                limit: "max_relocations",
            });
        }
        let records = sh.data(bytes, "relocation table")?;
        let list = relocations
            .get_mut(target)
            .ok_or(malformed("relocation target out of range"))?;
        list.reserve(usize::try_from(count).unwrap_or(0));
        for record in records.chunks_exact(RELA_SIZE as usize) {
            let offset = u64_at(record, 0);
            let info = u64_at(record, 8);
            let addend = u64_at(record, 16) as i64;
            let r_type = (info & 0xffff_ffff) as u32;
            let sym = (info >> 32) as usize;
            let kind = reloc_kind(header.arch, r_type)
                .ok_or(unsupported("relocation type", u64::from(r_type)))?;
            if sym == 0 {
                return Err(unsupported(
                    "relocation against no symbol",
                    u64::from(r_type),
                ));
            }
            let target = targets.get(sym).copied().flatten().ok_or(malformed(
                "relocation refers to a symbol that does not exist",
            ))?;
            list.push(Relocation::new(offset, kind, target, addend));
        }
    }
    for (section, list) in sections.iter_mut().zip(relocations) {
        section.set_relocations(list);
    }

    // Executables: the program headers must load every section where its header says.
    let executable_stack = if relocatable {
        gnu_stack.unwrap_or(true)
    } else {
        check_segments(bytes, &header, &sections, &file_offsets)?
    };

    let object = Object::from_parts(
        header.kind,
        header.arch,
        header.entry,
        executable_stack,
        sections,
        symbols,
    );
    object.validate()?;
    Ok(object)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one call site; the pieces are the reader's working state"
)]
fn read_symbols(
    bytes: &[u8],
    headers: &[Shdr],
    roles: &[Role],
    symtab_index: usize,
    sh: Shdr,
    shndx_index: Option<usize>,
    names: &mut Names<'_>,
    limits: &Limits,
    symbols: &mut Vec<Symbol>,
    targets: &mut Vec<Option<RelocationTarget>>,
) -> Result<(), ReadError> {
    if sh.entsize != SYM_SIZE || sh.size % SYM_SIZE != 0 {
        return Err(malformed("symbol entry size is not 24"));
    }
    let count = sh.size / SYM_SIZE;
    if count > u64::from(limits.max_symbols) {
        return Err(ReadError::LimitExceeded {
            limit: "max_symbols",
        });
    }
    let table = sh.data(bytes, "symbol table")?;
    let strtab_header = headers
        .get(sh.link as usize)
        .copied()
        .filter(|h| h.ty == SHT_STRTAB)
        .ok_or(malformed(
            "the symbol table's string table is not a string table",
        ))?;
    let strtab = strtab_header.data(bytes, "symbol string table")?;
    if u64::from(sh.info) > count || (count > 0 && sh.info == 0) {
        return Err(malformed(
            "the symbol table's first non-local index is out of range",
        ));
    }

    let extended: Option<&[u8]> = match shndx_index {
        Some(i) => {
            let h = headers.get(i).copied().unwrap_or_default();
            if h.link as usize != symtab_index {
                return Err(malformed(
                    "the extended section index table is not linked to the symbol table",
                ));
            }
            if h.entsize != 4 || Some(h.size) != count.checked_mul(4) {
                return Err(malformed(
                    "the extended section index table does not match the symbol table",
                ));
            }
            Some(h.data(bytes, "extended section index table")?)
        }
        None => None,
    };

    let mut records = table.chunks_exact(SYM_SIZE as usize).enumerate();
    if let Some((_, null)) = records.next() {
        if null.iter().any(|&b| b != 0) {
            return Err(malformed("symbol 0 is not the null symbol"));
        }
    }
    targets.reserve(usize::try_from(count).unwrap_or(0));
    targets.push(None);
    let first_global = sh.info as usize;

    for (index, record) in records {
        let st_name = u32_at(record, 0);
        let info = record.get(4).copied().unwrap_or(0);
        let other = record.get(5).copied().unwrap_or(0);
        let st_shndx = u16_at(record, 6);
        let value = u64_at(record, 8);
        let size = u64_at(record, 16);
        let bind = info >> 4;
        let ty = info & 0xf;

        if (bind == STB_LOCAL) != (index < first_global) {
            return Err(malformed("local and non-local symbols are interleaved"));
        }

        let section = match st_shndx {
            SHN_UNDEF => SymbolSection::Undefined,
            SHN_ABS => SymbolSection::Absolute,
            SHN_COMMON => SymbolSection::Common,
            raw => {
                let full = if raw == SHN_XINDEX {
                    let table = extended.ok_or(malformed(
                        "a symbol uses an extended section index but there is no index table",
                    ))?;
                    u32_at(table, index * 4)
                } else if u32::from(raw) >= SHN_LORESERVE {
                    return Err(unsupported("reserved symbol section index", u64::from(raw)));
                } else {
                    u32::from(raw)
                };
                match roles.get(full as usize) {
                    Some(Role::User(id)) => SymbolSection::Section(SectionId(*id)),
                    _ => {
                        return Err(malformed(
                            "a symbol is defined in a section with no contents",
                        ));
                    }
                }
            }
        };

        if ty == STT_SECTION {
            let SymbolSection::Section(id) = section else {
                return Err(malformed("a section symbol does not name a section"));
            };
            if bind != STB_LOCAL || value != 0 {
                return Err(malformed("a section symbol is not local with value zero"));
            }
            targets.push(Some(RelocationTarget::Section(id)));
            continue;
        }

        let kind = match ty {
            STT_NOTYPE => SymbolKind::NoType,
            STT_OBJECT => SymbolKind::Data,
            STT_FUNC => SymbolKind::Function,
            STT_FILE => SymbolKind::File,
            STT_TLS => SymbolKind::Tls,
            other => return Err(unsupported("symbol type", u64::from(other))),
        };
        let binding = match bind {
            STB_LOCAL => Binding::Local,
            STB_GLOBAL => Binding::Global,
            STB_WEAK => Binding::Weak,
            other => return Err(unsupported("symbol binding", u64::from(other))),
        };
        let visibility = match other {
            STV_DEFAULT => Visibility::Default,
            STV_HIDDEN => Visibility::Hidden,
            STV_PROTECTED => Visibility::Protected,
            other => {
                return Err(unsupported(
                    "symbol visibility or st_other flags",
                    u64::from(other),
                ));
            }
        };
        let name = names.get(strtab, st_name)?;
        let id = u32::try_from(symbols.len()).map_err(|_| ReadError::LimitExceeded {
            limit: "max_symbols",
        })?;
        let mut symbol = Symbol::new(String::from(name), kind, binding, section, value, size);
        symbol.visibility = visibility;
        symbols.push(symbol);
        targets.push(Some(RelocationTarget::Symbol(SymbolId(id))));
    }
    Ok(())
}

/// Checks an executable's program headers against its sections and returns whether it
/// asks for an executable stack.
fn check_segments(
    bytes: &[u8],
    header: &Header,
    sections: &[Section],
    file_offsets: &[u64],
) -> Result<bool, ReadError> {
    if header.phnum == PN_XNUM {
        return Err(unsupported(
            "extended program header count",
            u64::from(PN_XNUM),
        ));
    }
    let table_size = u64::from(header.phnum) * PHDR_SIZE;
    let table = if header.phnum == 0 {
        &[][..]
    } else {
        slice(bytes, header.phoff, table_size, "program header table")?
    };

    let mut loads: Vec<Phdr> = Vec::new();
    let mut stack: Option<bool> = None;
    for record in table.chunks_exact(PHDR_SIZE as usize) {
        let p = Phdr {
            ty: u32_at(record, 0),
            flags: u32_at(record, 4),
            offset: u64_at(record, 8),
            vaddr: u64_at(record, 16),
            filesz: u64_at(record, 32),
            memsz: u64_at(record, 40),
            align: u64_at(record, 48),
        };
        match p.ty {
            // PT_PHDR only says where the program headers are mapped, which the
            // writer derives again; it carries nothing the model needs.
            PT_NULL | PT_PHDR => {}
            PT_LOAD => {
                if p.filesz > p.memsz {
                    return Err(malformed("a segment's file size exceeds its memory size"));
                }
                if slice(bytes, p.offset, p.filesz, "segment contents").is_err() {
                    return Err(truncated("segment contents"));
                }
                if p.align > 1 {
                    if !p.align.is_power_of_two() {
                        return Err(malformed("segment alignment is not a power of two"));
                    }
                    if p.offset % p.align != p.vaddr % p.align {
                        return Err(malformed(
                            "segment offset and address disagree modulo the alignment",
                        ));
                    }
                }
                if p.vaddr.checked_add(p.memsz).is_none() {
                    return Err(malformed("segment overflows the address space"));
                }
                if let Some(prev) = loads.last() {
                    if p.vaddr < prev.vaddr.saturating_add(prev.memsz) {
                        return Err(malformed("loadable segments overlap or are out of order"));
                    }
                }
                loads.push(p);
            }
            PT_GNU_STACK => {
                if stack.replace(p.flags & PF_X != 0).is_some() {
                    return Err(malformed("more than one PT_GNU_STACK header"));
                }
            }
            other => return Err(unsupported("program header type", u64::from(other))),
        }
    }

    for (section, &offset) in sections.iter().zip(file_offsets) {
        let flags = section.flags();
        if !flags.contains(SectionFlags::ALLOC) || section.size() == 0 {
            continue;
        }
        let addr = section.address();
        let end = addr
            .checked_add(section.size())
            .ok_or(malformed("a section overflows the address space"))?;
        let position = loads.partition_point(|l| l.vaddr <= addr);
        let load = position
            .checked_sub(1)
            .and_then(|p| loads.get(p))
            .ok_or(malformed("a loadable section is outside every segment"))?;
        // `vaddr + memsz` cannot overflow: checked when the segment was read.
        if end > load.vaddr + load.memsz {
            return Err(malformed("a loadable section is outside every segment"));
        }
        if !section.kind().is_uninitialized() {
            let in_file = end <= load.vaddr + load.filesz
                && Some(offset) == load.offset.checked_add(addr - load.vaddr);
            if !in_file {
                return Err(malformed(
                    "a section's file offset does not match its segment",
                ));
            }
        }
        if (flags.contains(SectionFlags::WRITE) && load.flags & PF_W == 0)
            || (flags.contains(SectionFlags::EXEC) && load.flags & PF_X == 0)
        {
            return Err(malformed(
                "a section's flags exceed its segment's permissions",
            ));
        }
    }

    let entry = header.entry;
    let entry_loaded = loads
        .iter()
        .any(|l| l.flags & PF_X != 0 && entry >= l.vaddr && entry - l.vaddr < l.memsz);
    if !entry_loaded {
        return Err(malformed("the entry point is not in an executable segment"));
    }
    Ok(stack.unwrap_or(true))
}
