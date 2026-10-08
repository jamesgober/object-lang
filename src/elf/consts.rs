//! ELF64 constants (System V gABI, the x86-64 psABI, and the AArch64 ELF ABI), and the
//! mapping between the model's relocation kinds and ELF relocation types.

use crate::model::Architecture;
use crate::reloc::{Aarch64Reloc, RelocationKind, X86_64Reloc};

pub(crate) const EHDR_SIZE: u64 = 64;
pub(crate) const SHDR_SIZE: u64 = 64;
pub(crate) const PHDR_SIZE: u64 = 56;
pub(crate) const SYM_SIZE: u64 = 24;
pub(crate) const RELA_SIZE: u64 = 24;

pub(crate) const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
pub(crate) const ELFCLASS64: u8 = 2;
pub(crate) const ELFDATA2LSB: u8 = 1;
pub(crate) const EV_CURRENT: u8 = 1;
pub(crate) const ELFOSABI_NONE: u8 = 0;
pub(crate) const ELFOSABI_GNU: u8 = 3;

pub(crate) const ET_REL: u16 = 1;
pub(crate) const ET_EXEC: u16 = 2;

pub(crate) const EM_X86_64: u16 = 62;
pub(crate) const EM_AARCH64: u16 = 183;

pub(crate) const SHT_NULL: u32 = 0;
pub(crate) const SHT_PROGBITS: u32 = 1;
pub(crate) const SHT_SYMTAB: u32 = 2;
pub(crate) const SHT_STRTAB: u32 = 3;
pub(crate) const SHT_RELA: u32 = 4;
pub(crate) const SHT_NOTE: u32 = 7;
pub(crate) const SHT_NOBITS: u32 = 8;
pub(crate) const SHT_INIT_ARRAY: u32 = 14;
pub(crate) const SHT_FINI_ARRAY: u32 = 15;
pub(crate) const SHT_PREINIT_ARRAY: u32 = 16;
pub(crate) const SHT_SYMTAB_SHNDX: u32 = 18;
/// LLVM's address-significance table: an optimization hint for identical-code folding
/// that readers may drop without changing what the object means.
pub(crate) const SHT_LLVM_ADDRSIG: u32 = 0x6fff_4c03;
/// `.eh_frame` as the x86-64 psABI types it (GNU as uses `SHT_PROGBITS` instead).
pub(crate) const SHT_X86_64_UNWIND: u32 = 0x7000_0001;

pub(crate) const SHF_WRITE: u64 = 0x1;
pub(crate) const SHF_ALLOC: u64 = 0x2;
pub(crate) const SHF_EXECINSTR: u64 = 0x4;
pub(crate) const SHF_MERGE: u64 = 0x10;
pub(crate) const SHF_STRINGS: u64 = 0x20;
pub(crate) const SHF_INFO_LINK: u64 = 0x40;
pub(crate) const SHF_TLS: u64 = 0x400;

pub(crate) const SHN_UNDEF: u16 = 0;
pub(crate) const SHN_LORESERVE: u32 = 0xff00;
pub(crate) const SHN_ABS: u16 = 0xfff1;
pub(crate) const SHN_COMMON: u16 = 0xfff2;
pub(crate) const SHN_XINDEX: u16 = 0xffff;

pub(crate) const STB_LOCAL: u8 = 0;
pub(crate) const STB_GLOBAL: u8 = 1;
pub(crate) const STB_WEAK: u8 = 2;

pub(crate) const STT_NOTYPE: u8 = 0;
pub(crate) const STT_OBJECT: u8 = 1;
pub(crate) const STT_FUNC: u8 = 2;
pub(crate) const STT_SECTION: u8 = 3;
pub(crate) const STT_FILE: u8 = 4;
pub(crate) const STT_TLS: u8 = 6;

pub(crate) const STV_DEFAULT: u8 = 0;
pub(crate) const STV_HIDDEN: u8 = 2;
pub(crate) const STV_PROTECTED: u8 = 3;

pub(crate) const PT_NULL: u32 = 0;
pub(crate) const PT_LOAD: u32 = 1;
pub(crate) const PT_PHDR: u32 = 6;
pub(crate) const PT_GNU_STACK: u32 = 0x6474_e551;
pub(crate) const PN_XNUM: u16 = 0xffff;

pub(crate) const PF_X: u32 = 0x1;
pub(crate) const PF_W: u32 = 0x2;
pub(crate) const PF_R: u32 = 0x4;

/// `p_align` of the `PT_GNU_STACK` header, as GNU ld writes it.
pub(crate) const GNU_STACK_ALIGN: u64 = 16;

/// The name of the section that records, in a relocatable object, whether the code
/// needs an executable stack.
pub(crate) const GNU_STACK_NAME: &str = ".note.GNU-stack";

/// The `e_machine` value for an architecture.
pub(crate) const fn machine(arch: Architecture) -> u16 {
    match arch {
        Architecture::X86_64 => EM_X86_64,
        Architecture::Aarch64 => EM_AARCH64,
    }
}

/// The ELF relocation type for a kind.
pub(crate) const fn reloc_type(kind: RelocationKind) -> u32 {
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
        },
    }
}

/// The kind for an ELF relocation type, or `None` if the model has no such kind.
pub(crate) const fn reloc_kind(arch: Architecture, r_type: u32) -> Option<RelocationKind> {
    let kind = match arch {
        Architecture::X86_64 => RelocationKind::X86_64(match r_type {
            1 => X86_64Reloc::Abs64,
            2 => X86_64Reloc::Pc32,
            4 => X86_64Reloc::Plt32,
            9 => X86_64Reloc::GotPcRel,
            10 => X86_64Reloc::Abs32,
            11 => X86_64Reloc::Abs32Signed,
            24 => X86_64Reloc::Pc64,
            41 => X86_64Reloc::GotPcRelX,
            42 => X86_64Reloc::RexGotPcRelX,
            _ => return None,
        }),
        Architecture::Aarch64 => RelocationKind::Aarch64(match r_type {
            257 => Aarch64Reloc::Abs64,
            258 => Aarch64Reloc::Abs32,
            260 => Aarch64Reloc::Prel64,
            261 => Aarch64Reloc::Prel32,
            274 => Aarch64Reloc::AdrPrelLo21,
            275 => Aarch64Reloc::AdrPrelPgHi21,
            277 => Aarch64Reloc::AddAbsLo12Nc,
            278 => Aarch64Reloc::Ldst8AbsLo12Nc,
            279 => Aarch64Reloc::TstBr14,
            280 => Aarch64Reloc::CondBr19,
            282 => Aarch64Reloc::Jump26,
            283 => Aarch64Reloc::Call26,
            284 => Aarch64Reloc::Ldst16AbsLo12Nc,
            285 => Aarch64Reloc::Ldst32AbsLo12Nc,
            286 => Aarch64Reloc::Ldst64AbsLo12Nc,
            299 => Aarch64Reloc::Ldst128AbsLo12Nc,
            311 => Aarch64Reloc::AdrGotPage,
            312 => Aarch64Reloc::Ld64GotLo12Nc,
            _ => return None,
        }),
    };
    Some(kind)
}
