//! Relocations: places in a section that a linker must patch with an address once it
//! is known.

use crate::model::{Architecture, SectionId, SymbolId};

/// An x86-64 relocation. Each variant names its ELF counterpart; in the formulas, `S` is
/// the target's address, `A` the addend, `P` the address of the patched bytes, `L` the
/// address of the target's PLT entry (or the target itself in a static link), and `G`
/// the address of the target's GOT entry.
///
/// # Examples
///
/// ```
/// use object_lang::{RelocationKind, X86_64Reloc};
///
/// let kind = RelocationKind::from(X86_64Reloc::Pc32);
/// assert_eq!(kind.size(), 4);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum X86_64Reloc {
    /// `S + A`, 64 bits (`R_X86_64_64`).
    Abs64,
    /// `S + A`, 32 bits, zero-extended: must fit in `u32` (`R_X86_64_32`).
    Abs32,
    /// `S + A`, 32 bits, sign-extended: must fit in `i32` (`R_X86_64_32S`).
    Abs32Signed,
    /// `S + A - P`, 32 bits (`R_X86_64_PC32`).
    Pc32,
    /// `S + A - P`, 64 bits (`R_X86_64_PC64`).
    Pc64,
    /// `L + A - P`, 32 bits: a call or jump to a function (`R_X86_64_PLT32`).
    Plt32,
    /// `G + A - P`, 32 bits: a load of the target's GOT entry (`R_X86_64_GOTPCREL`).
    GotPcRel,
    /// As [`GotPcRel`](X86_64Reloc::GotPcRel), and the linker may relax the instruction
    /// to avoid the GOT (`R_X86_64_GOTPCRELX`).
    GotPcRelX,
    /// As [`GotPcRelX`](X86_64Reloc::GotPcRelX) for an instruction with a REX prefix
    /// (`R_X86_64_REX_GOTPCRELX`).
    RexGotPcRelX,
}

/// An AArch64 relocation. Each variant names its ELF counterpart; in the formulas, `S`
/// is the target's address, `A` the addend, `P` the address of the patched bytes,
/// `Page(x)` is `x` with its low 12 bits cleared, and `G` the address of the target's
/// GOT entry. Instruction relocations patch an immediate field of a 4-byte instruction,
/// so their offset must be a multiple of 4.
///
/// # Examples
///
/// ```
/// use object_lang::{Aarch64Reloc, RelocationKind};
///
/// assert!(RelocationKind::from(Aarch64Reloc::Call26).is_instruction());
/// assert!(!RelocationKind::from(Aarch64Reloc::Abs64).is_instruction());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum Aarch64Reloc {
    /// `S + A`, 64 bits of data (`R_AARCH64_ABS64`).
    Abs64,
    /// `S + A`, 32 bits of data (`R_AARCH64_ABS32`).
    Abs32,
    /// `S + A - P`, 64 bits of data (`R_AARCH64_PREL64`).
    Prel64,
    /// `S + A - P`, 32 bits of data (`R_AARCH64_PREL32`).
    Prel32,
    /// `S + A - P` into a `BL` (`R_AARCH64_CALL26`): ±128 MiB.
    Call26,
    /// `S + A - P` into a `B` (`R_AARCH64_JUMP26`): ±128 MiB.
    Jump26,
    /// `S + A - P` into a `B.cond`, `CBZ`, or `CBNZ` (`R_AARCH64_CONDBR19`): ±1 MiB.
    CondBr19,
    /// `S + A - P` into a `TBZ` or `TBNZ` (`R_AARCH64_TSTBR14`): ±32 KiB.
    TstBr14,
    /// `S + A - P` into an `ADR` (`R_AARCH64_ADR_PREL_LO21`): ±1 MiB.
    AdrPrelLo21,
    /// `Page(S + A) - Page(P)` into an `ADRP` (`R_AARCH64_ADR_PREL_PG_HI21`): ±4 GiB.
    AdrPrelPgHi21,
    /// Low 12 bits of `S + A` into an `ADD` immediate (`R_AARCH64_ADD_ABS_LO12_NC`).
    AddAbsLo12Nc,
    /// Low 12 bits of `S + A` into an 8-bit `LDR`/`STR` offset
    /// (`R_AARCH64_LDST8_ABS_LO12_NC`).
    Ldst8AbsLo12Nc,
    /// Bits 11:1 of `S + A` into a 16-bit `LDR`/`STR` offset
    /// (`R_AARCH64_LDST16_ABS_LO12_NC`).
    Ldst16AbsLo12Nc,
    /// Bits 11:2 of `S + A` into a 32-bit `LDR`/`STR` offset
    /// (`R_AARCH64_LDST32_ABS_LO12_NC`).
    Ldst32AbsLo12Nc,
    /// Bits 11:3 of `S + A` into a 64-bit `LDR`/`STR` offset
    /// (`R_AARCH64_LDST64_ABS_LO12_NC`).
    Ldst64AbsLo12Nc,
    /// Bits 11:4 of `S + A` into a 128-bit `LDR`/`STR` offset
    /// (`R_AARCH64_LDST128_ABS_LO12_NC`).
    Ldst128AbsLo12Nc,
    /// `Page(G) - Page(P)` into an `ADRP` (`R_AARCH64_ADR_GOT_PAGE`).
    AdrGotPage,
    /// Bits 11:3 of `G` into an `LDR` (`R_AARCH64_LD64_GOT_LO12_NC`).
    Ld64GotLo12Nc,
}

/// How a relocation patches its bytes: an architecture-specific kind.
///
/// Kinds are named for what they compute, not for one format's numbering, so each
/// writer maps them to its own relocation types. Build one from the architecture enum
/// with `From`.
///
/// # Examples
///
/// ```
/// use object_lang::{Aarch64Reloc, Architecture, RelocationKind, X86_64Reloc};
///
/// let x = RelocationKind::from(X86_64Reloc::Abs64);
/// let a = RelocationKind::from(Aarch64Reloc::AdrPrelPgHi21);
/// assert_eq!(x.architecture(), Architecture::X86_64);
/// assert_eq!(a.architecture(), Architecture::Aarch64);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum RelocationKind {
    /// An x86-64 relocation.
    X86_64(X86_64Reloc),
    /// An AArch64 relocation.
    Aarch64(Aarch64Reloc),
}

impl RelocationKind {
    /// The architecture this kind belongs to. An object may only carry relocations of
    /// its own architecture.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, RelocationKind, X86_64Reloc};
    ///
    /// assert_eq!(RelocationKind::from(X86_64Reloc::Plt32).architecture(), Architecture::X86_64);
    /// ```
    #[must_use]
    pub const fn architecture(self) -> Architecture {
        match self {
            RelocationKind::X86_64(_) => Architecture::X86_64,
            RelocationKind::Aarch64(_) => Architecture::Aarch64,
        }
    }

    /// The number of bytes the relocation patches, starting at its offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Aarch64Reloc, RelocationKind, X86_64Reloc};
    ///
    /// assert_eq!(RelocationKind::from(X86_64Reloc::Abs64).size(), 8);
    /// assert_eq!(RelocationKind::from(X86_64Reloc::Plt32).size(), 4);
    /// assert_eq!(RelocationKind::from(Aarch64Reloc::Call26).size(), 4);
    /// ```
    #[must_use]
    pub const fn size(self) -> u64 {
        match self {
            RelocationKind::X86_64(X86_64Reloc::Abs64 | X86_64Reloc::Pc64)
            | RelocationKind::Aarch64(Aarch64Reloc::Abs64 | Aarch64Reloc::Prel64) => 8,
            _ => 4,
        }
    }

    /// Whether the relocation patches an instruction's immediate field rather than a
    /// plain data word. AArch64 instructions are 4-byte aligned, so such a relocation's
    /// offset must be a multiple of 4.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Aarch64Reloc, RelocationKind, X86_64Reloc};
    ///
    /// assert!(RelocationKind::from(Aarch64Reloc::AddAbsLo12Nc).is_instruction());
    /// assert!(!RelocationKind::from(Aarch64Reloc::Prel32).is_instruction());
    /// assert!(!RelocationKind::from(X86_64Reloc::Pc32).is_instruction());
    /// ```
    #[must_use]
    pub const fn is_instruction(self) -> bool {
        match self {
            RelocationKind::X86_64(_) => false,
            RelocationKind::Aarch64(kind) => !matches!(
                kind,
                Aarch64Reloc::Abs64
                    | Aarch64Reloc::Abs32
                    | Aarch64Reloc::Prel64
                    | Aarch64Reloc::Prel32
            ),
        }
    }
}

impl From<X86_64Reloc> for RelocationKind {
    fn from(kind: X86_64Reloc) -> Self {
        RelocationKind::X86_64(kind)
    }
}

impl From<Aarch64Reloc> for RelocationKind {
    fn from(kind: Aarch64Reloc) -> Self {
        RelocationKind::Aarch64(kind)
    }
}

/// What a relocation's address comes from.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, Object, RelocationTarget, Section, SectionKind};
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// let rodata = obj.add_section(Section::new(".rodata", SectionKind::ReadOnlyData));
/// // An address inside .rodata, without a named symbol for it.
/// let target = RelocationTarget::Section(rodata);
/// assert_eq!(target, RelocationTarget::Section(rodata));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum RelocationTarget {
    /// The address of a symbol of the object.
    Symbol(SymbolId),
    /// The start address of a section of the object; the addend picks the place
    /// inside it. This is how compilers refer to unnamed data such as string literals.
    Section(SectionId),
}

/// A place in a section to patch: the bytes at `offset` receive the value `kind`
/// computes from the address of `target` plus `addend`.
///
/// The addend is always explicit, whatever format the relocation is written in.
///
/// # Examples
///
/// ```
/// use object_lang::{Relocation, RelocationKind, RelocationTarget, SymbolId, X86_64Reloc};
/// # use object_lang::{Architecture, Object, Symbol};
/// # let mut obj = Object::relocatable(Architecture::X86_64);
/// # let target = obj.add_symbol(Symbol::undefined("callee"));
///
/// // `call callee` is e8 followed by a 32-bit displacement from the next instruction.
/// let reloc = Relocation::new(1, X86_64Reloc::Plt32, RelocationTarget::Symbol(target), -4);
/// assert_eq!(reloc.kind, RelocationKind::X86_64(X86_64Reloc::Plt32));
/// assert_eq!(reloc.addend, -4);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub struct Relocation {
    /// Where the patched bytes start, as an offset within the section.
    pub offset: u64,
    /// How the bytes are computed and how many are patched.
    pub kind: RelocationKind,
    /// The symbol or section whose address is used.
    pub target: RelocationTarget,
    /// A constant added to the target's address.
    pub addend: i64,
}

impl Relocation {
    /// A relocation at `offset` of `kind`, against `target`, with `addend`.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Aarch64Reloc, Relocation, RelocationTarget};
    /// # use object_lang::{Architecture, Object, Section, SectionKind};
    /// # let mut obj = Object::relocatable(Architecture::Aarch64);
    /// # let data = obj.add_section(Section::new(".data", SectionKind::Data));
    ///
    /// let r = Relocation::new(8, Aarch64Reloc::Abs64, RelocationTarget::Section(data), 16);
    /// assert_eq!((r.offset, r.addend), (8, 16));
    /// ```
    #[must_use]
    pub fn new(
        offset: u64,
        kind: impl Into<RelocationKind>,
        target: RelocationTarget,
        addend: i64,
    ) -> Self {
        Relocation {
            offset,
            kind: kind.into(),
            target,
            addend,
        }
    }
}
