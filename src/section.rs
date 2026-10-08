//! Sections: named runs of bytes (or reserved zero-filled space) with a kind, flags,
//! an alignment, and the relocations that patch them.

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::{BitOr, BitOrAssign};

use crate::error::ModelError;
use crate::reloc::Relocation;

/// What a section holds. The kind decides how a format stores the section (an ELF
/// section type, a COFF characteristic, a Mach-O section type) and which
/// [`SectionFlags`] it may carry.
///
/// | Kind | Contents | Required flags | Forbidden flags |
/// |---|---|---|---|
/// | `Text` | machine code | `ALLOC`, `EXEC` | `TLS` |
/// | `Data` | writable initialized data | `ALLOC`, `WRITE` | `EXEC` |
/// | `ReadOnlyData` | constants | `ALLOC` | `WRITE`, `EXEC`, `TLS` |
/// | `Bss` | zero-filled space, no bytes in the file | `ALLOC` | `EXEC`, `MERGE`, `STRINGS` |
/// | `Note` | vendor notes | | `WRITE`, `EXEC`, `TLS`, `MERGE`, `STRINGS` |
/// | `InitArray`, `FiniArray`, `PreinitArray` | pointers to constructors or destructors | `ALLOC`, `WRITE` | `EXEC`, `TLS`, `MERGE`, `STRINGS` |
/// | `Other` | data not loaded at run time (comments, metadata) | | `ALLOC`, `WRITE`, `EXEC`, `TLS` |
///
/// The rule for the first four rows is what makes a kind recoverable from a file:
/// with `ALLOC`, `EXEC` means code, else `WRITE` means data, else read-only data, and
/// without `ALLOC` the section is `Other`.
///
/// # Examples
///
/// ```
/// use object_lang::{SectionFlags, SectionKind};
///
/// assert_eq!(SectionKind::Text.default_flags(), SectionFlags::ALLOC | SectionFlags::EXEC);
/// assert!(SectionKind::Bss.is_uninitialized());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum SectionKind {
    /// Executable machine code (`.text`).
    Text,
    /// Writable, initialized data (`.data`; with [`TLS`](SectionFlags::TLS), `.tdata`).
    Data,
    /// Read-only data (`.rodata`).
    ReadOnlyData,
    /// Zero-initialized space that takes no room in the file (`.bss`; with
    /// [`TLS`](SectionFlags::TLS), `.tbss`). Its size is set with
    /// [`with_bss_size`](Section::with_bss_size) or [`reserve`](Section::reserve).
    Bss,
    /// Notes for the loader or tools (`SHT_NOTE` in ELF).
    Note,
    /// Pointers to functions run before `main` (`.init_array`).
    InitArray,
    /// Pointers to functions run at exit (`.fini_array`).
    FiniArray,
    /// Pointers to functions run before shared-library initializers (`.preinit_array`).
    PreinitArray,
    /// Data that is not loaded at run time: comments, tool metadata, debug information.
    Other,
}

impl SectionKind {
    /// The flags [`Section::new`] gives a section of this kind: the required flags in
    /// the table above.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{SectionFlags, SectionKind};
    ///
    /// assert_eq!(SectionKind::Data.default_flags(), SectionFlags::ALLOC | SectionFlags::WRITE);
    /// assert_eq!(SectionKind::Other.default_flags(), SectionFlags::empty());
    /// ```
    #[must_use]
    pub const fn default_flags(self) -> SectionFlags {
        match self {
            SectionKind::Text => SectionFlags(SectionFlags::ALLOC.0 | SectionFlags::EXEC.0),
            SectionKind::Data
            | SectionKind::Bss
            | SectionKind::InitArray
            | SectionKind::FiniArray
            | SectionKind::PreinitArray => {
                SectionFlags(SectionFlags::ALLOC.0 | SectionFlags::WRITE.0)
            }
            SectionKind::ReadOnlyData => SectionFlags::ALLOC,
            SectionKind::Note | SectionKind::Other => SectionFlags::empty(),
        }
    }

    /// Whether sections of this kind hold no bytes, only a size ([`Bss`](SectionKind::Bss)).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::SectionKind;
    ///
    /// assert!(SectionKind::Bss.is_uninitialized());
    /// assert!(!SectionKind::Data.is_uninitialized());
    /// ```
    #[must_use]
    pub const fn is_uninitialized(self) -> bool {
        matches!(self, SectionKind::Bss)
    }

    /// Whether `flags` are allowed on a section of this kind (the table on
    /// [`SectionKind`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{SectionFlags, SectionKind};
    ///
    /// assert!(SectionKind::Data.allows(SectionFlags::ALLOC | SectionFlags::WRITE | SectionFlags::TLS));
    /// assert!(!SectionKind::ReadOnlyData.allows(SectionFlags::ALLOC | SectionFlags::WRITE));
    /// ```
    #[must_use]
    pub const fn allows(self, flags: SectionFlags) -> bool {
        let (required, forbidden) = match self {
            SectionKind::Text => (
                SectionFlags::ALLOC.0 | SectionFlags::EXEC.0,
                SectionFlags::TLS.0,
            ),
            SectionKind::Data => (
                SectionFlags::ALLOC.0 | SectionFlags::WRITE.0,
                SectionFlags::EXEC.0,
            ),
            SectionKind::ReadOnlyData => (
                SectionFlags::ALLOC.0,
                SectionFlags::WRITE.0 | SectionFlags::EXEC.0 | SectionFlags::TLS.0,
            ),
            SectionKind::Bss => (
                SectionFlags::ALLOC.0,
                SectionFlags::EXEC.0 | SectionFlags::MERGE.0 | SectionFlags::STRINGS.0,
            ),
            SectionKind::Note => (
                0,
                SectionFlags::WRITE.0
                    | SectionFlags::EXEC.0
                    | SectionFlags::TLS.0
                    | SectionFlags::MERGE.0
                    | SectionFlags::STRINGS.0,
            ),
            SectionKind::InitArray | SectionKind::FiniArray | SectionKind::PreinitArray => (
                SectionFlags::ALLOC.0 | SectionFlags::WRITE.0,
                SectionFlags::EXEC.0
                    | SectionFlags::TLS.0
                    | SectionFlags::MERGE.0
                    | SectionFlags::STRINGS.0,
            ),
            SectionKind::Other => (
                0,
                SectionFlags::ALLOC.0
                    | SectionFlags::WRITE.0
                    | SectionFlags::EXEC.0
                    | SectionFlags::TLS.0,
            ),
        };
        flags.0 & required == required && flags.0 & forbidden == 0
    }
}

/// Attributes of a section, independent of the file format.
///
/// A small bit set: combine flags with `|`, test them with
/// [`contains`](SectionFlags::contains). Which combinations a section may carry depends
/// on its [`SectionKind`].
///
/// # Examples
///
/// ```
/// use object_lang::SectionFlags;
///
/// let flags = SectionFlags::ALLOC | SectionFlags::MERGE | SectionFlags::STRINGS;
/// assert!(flags.contains(SectionFlags::MERGE));
/// assert!(!flags.contains(SectionFlags::WRITE));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SectionFlags(u8);

impl SectionFlags {
    /// The section occupies memory when the program runs.
    pub const ALLOC: SectionFlags = SectionFlags(1);
    /// The section is writable at run time.
    pub const WRITE: SectionFlags = SectionFlags(1 << 1);
    /// The section holds executable instructions.
    pub const EXEC: SectionFlags = SectionFlags(1 << 2);
    /// The section is a thread-local storage template (`.tdata`, `.tbss`).
    pub const TLS: SectionFlags = SectionFlags(1 << 3);
    /// Equal entries of [`entry_size`](Section::entry_size) bytes may be merged by the
    /// linker. Requires a non-zero entry size.
    pub const MERGE: SectionFlags = SectionFlags(1 << 4);
    /// The entries are NUL-terminated strings (with [`MERGE`](SectionFlags::MERGE),
    /// equal strings may be merged).
    pub const STRINGS: SectionFlags = SectionFlags(1 << 5);

    const ALL: u8 = (1 << 6) - 1;

    /// No flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::SectionFlags;
    ///
    /// assert!(SectionFlags::empty().is_empty());
    /// ```
    #[must_use]
    pub const fn empty() -> Self {
        SectionFlags(0)
    }

    /// Whether every flag in `other` is set in `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::SectionFlags;
    ///
    /// let rw = SectionFlags::ALLOC | SectionFlags::WRITE;
    /// assert!(rw.contains(SectionFlags::WRITE));
    /// assert!(!rw.contains(SectionFlags::WRITE | SectionFlags::EXEC));
    /// ```
    #[must_use]
    pub const fn contains(self, other: SectionFlags) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no flag is set.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::SectionFlags;
    ///
    /// assert!(!SectionFlags::ALLOC.is_empty());
    /// ```
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `self` with every flag of `other` added.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::SectionFlags;
    ///
    /// assert_eq!(SectionFlags::ALLOC.union(SectionFlags::WRITE), SectionFlags::ALLOC | SectionFlags::WRITE);
    /// ```
    #[must_use]
    pub const fn union(self, other: SectionFlags) -> Self {
        SectionFlags(self.0 | other.0)
    }

    /// `self` with every flag of `other` removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::SectionFlags;
    ///
    /// let rw = SectionFlags::ALLOC | SectionFlags::WRITE;
    /// assert_eq!(rw.difference(SectionFlags::WRITE), SectionFlags::ALLOC);
    /// ```
    #[must_use]
    pub const fn difference(self, other: SectionFlags) -> Self {
        SectionFlags(self.0 & !other.0)
    }

    /// The raw bits, one per flag constant, in declaration order from bit 0.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::SectionFlags;
    ///
    /// assert_eq!(SectionFlags::ALLOC.bits(), 1);
    /// assert_eq!(SectionFlags::from_bits(0b11), Some(SectionFlags::ALLOC | SectionFlags::WRITE));
    /// ```
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// The flags with these raw bits, or `None` if a bit names no flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::SectionFlags;
    ///
    /// assert_eq!(SectionFlags::from_bits(0b100), Some(SectionFlags::EXEC));
    /// assert_eq!(SectionFlags::from_bits(0x80), None);
    /// ```
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !SectionFlags::ALL == 0 {
            Some(SectionFlags(bits))
        } else {
            None
        }
    }
}

impl BitOr for SectionFlags {
    type Output = SectionFlags;

    fn bitor(self, rhs: SectionFlags) -> SectionFlags {
        self.union(rhs)
    }
}

impl BitOrAssign for SectionFlags {
    fn bitor_assign(&mut self, rhs: SectionFlags) {
        *self = self.union(rhs);
    }
}

impl core::fmt::Debug for SectionFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        const NAMES: [&str; 6] = ["ALLOC", "WRITE", "EXEC", "TLS", "MERGE", "STRINGS"];
        if self.0 == 0 {
            return f.write_str("SectionFlags(empty)");
        }
        f.write_str("SectionFlags(")?;
        let mut first = true;
        for (bit, name) in NAMES.iter().enumerate() {
            if self.0 & (1 << bit) != 0 {
                if !first {
                    f.write_str(" | ")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        f.write_str(")")
    }
}

/// One section: a name, a [kind](SectionKind), [flags](SectionFlags), an alignment, its
/// contents (bytes, or for [`Bss`](SectionKind::Bss) only a size), an address, and the
/// [`Relocation`]s that patch it.
///
/// Build one with [`new`](Section::new) and the `with_*` methods, or grow its contents
/// in place with [`append`](Section::append) and [`reserve`](Section::reserve), which
/// handle padding and alignment.
///
/// In a relocatable object the address is normally zero and offsets within the section
/// are what symbols and relocations refer to. In an executable, every loadable section
/// has its final address.
///
/// # Examples
///
/// ```
/// use object_lang::{Section, SectionKind};
///
/// let mut rodata = Section::new(".rodata", SectionKind::ReadOnlyData);
/// let hello = rodata.append(b"hello\0", 1)?;
/// let table = rodata.append(&[1, 0, 0, 0, 2, 0, 0, 0], 8)?;
/// assert_eq!((hello, table), (0, 8)); // padded up to the table's alignment
/// assert_eq!(rodata.align(), 8);
/// assert_eq!(rodata.size(), 16);
/// # Ok::<(), object_lang::ModelError>(())
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section {
    name: String,
    kind: SectionKind,
    flags: SectionFlags,
    align: u64,
    entry_size: u64,
    address: u64,
    data: Vec<u8>,
    bss_size: u64,
    relocations: Vec<Relocation>,
}

impl Section {
    /// An empty section of this kind, with the kind's [default
    /// flags](SectionKind::default_flags), alignment 1, address 0, and no relocations.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// let text = Section::new(".text", SectionKind::Text);
    /// assert_eq!(text.flags(), SectionKind::Text.default_flags());
    /// assert_eq!((text.align(), text.size()), (1, 0));
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>, kind: SectionKind) -> Self {
        Section {
            name: name.into(),
            kind,
            flags: kind.default_flags(),
            align: 1,
            entry_size: 0,
            address: 0,
            data: Vec::new(),
            bss_size: 0,
            relocations: Vec::new(),
        }
    }

    /// Replaces the flags. [`validate`](crate::Object::validate) checks that the kind
    /// allows them.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionFlags, SectionKind};
    ///
    /// let tdata = Section::new(".tdata", SectionKind::Data)
    ///     .with_flags(SectionKind::Data.default_flags() | SectionFlags::TLS);
    /// assert!(tdata.flags().contains(SectionFlags::TLS));
    /// ```
    #[must_use]
    pub fn with_flags(mut self, flags: SectionFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Sets the alignment in bytes: a power of two, checked by
    /// [`validate`](crate::Object::validate).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// assert_eq!(Section::new(".text", SectionKind::Text).with_align(16).align(), 16);
    /// ```
    #[must_use]
    pub fn with_align(mut self, align: u64) -> Self {
        self.align = align;
        self
    }

    /// Sets the size of each entry, for sections of fixed-size entries such as
    /// [`MERGE`](SectionFlags::MERGE) sections (zero otherwise).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionFlags, SectionKind};
    ///
    /// let strings = Section::new(".rodata.str1.1", SectionKind::ReadOnlyData)
    ///     .with_flags(SectionFlags::ALLOC | SectionFlags::MERGE | SectionFlags::STRINGS)
    ///     .with_entry_size(1);
    /// assert_eq!(strings.entry_size(), 1);
    /// ```
    #[must_use]
    pub fn with_entry_size(mut self, entry_size: u64) -> Self {
        self.entry_size = entry_size;
        self
    }

    /// Sets the address: the final load address in an executable.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// let text = Section::new(".text", SectionKind::Text).with_address(0x40_1000);
    /// assert_eq!(text.address(), 0x40_1000);
    /// ```
    #[must_use]
    pub fn with_address(mut self, address: u64) -> Self {
        self.address = address;
        self
    }

    /// Replaces the contents with `data`. Only for kinds that hold bytes;
    /// [`validate`](crate::Object::validate) refuses data on a [`Bss`](SectionKind::Bss)
    /// section.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// let data = Section::new(".data", SectionKind::Data).with_data(vec![1, 2, 3]);
    /// assert_eq!(data.data(), &[1, 2, 3]);
    /// ```
    #[must_use]
    pub fn with_data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }

    /// Sets the size of a [`Bss`](SectionKind::Bss) section; [`validate`](crate::Object::validate)
    /// refuses a size on other kinds, whose size is their data's length.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// let bss = Section::new(".bss", SectionKind::Bss).with_bss_size(4096);
    /// assert_eq!(bss.size(), 4096);
    /// assert!(bss.data().is_empty());
    /// ```
    #[must_use]
    pub fn with_bss_size(mut self, size: u64) -> Self {
        self.bss_size = size;
        self
    }

    /// Appends `bytes`, first padding with zeros to a multiple of `align`, and returns
    /// the offset the bytes start at. Raises the section's alignment to `align` if lower.
    ///
    /// # Errors
    ///
    /// - [`ModelError::InvalidAlignment`] if `align` is not a power of two.
    /// - [`ModelError::AppendToBss`] if the section is [`Bss`](SectionKind::Bss), which
    ///   holds no bytes; use [`reserve`](Section::reserve).
    /// - [`ModelError::SizeOverflow`] if the section would outgrow memory.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// let mut text = Section::new(".text", SectionKind::Text);
    /// assert_eq!(text.append(&[0x90], 1)?, 0);
    /// assert_eq!(text.append(&[0xc3], 16)?, 16);
    /// assert_eq!(text.size(), 17);
    /// # Ok::<(), object_lang::ModelError>(())
    /// ```
    pub fn append(&mut self, bytes: &[u8], align: u64) -> Result<u64, ModelError> {
        if self.kind.is_uninitialized() {
            return Err(ModelError::AppendToBss);
        }
        let offset = self.next_offset(align)?;
        let len = u64::try_from(bytes.len()).map_err(|_| ModelError::SizeOverflow)?;
        let end = offset.checked_add(len).ok_or(ModelError::SizeOverflow)?;
        self.grow_to(offset, end)?;
        self.data.extend_from_slice(bytes);
        self.align = self.align.max(align);
        Ok(offset)
    }

    /// Reserves `size` zero bytes at the next multiple of `align` and returns their
    /// offset. A [`Bss`](SectionKind::Bss) section only grows its size; any other kind
    /// appends zero bytes. Raises the section's alignment to `align` if lower.
    ///
    /// # Errors
    ///
    /// - [`ModelError::InvalidAlignment`] if `align` is not a power of two.
    /// - [`ModelError::SizeOverflow`] if the size would overflow (or, for a section that
    ///   holds bytes, not fit in memory).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// let mut bss = Section::new(".bss", SectionKind::Bss);
    /// assert_eq!(bss.reserve(4, 4)?, 0);
    /// assert_eq!(bss.reserve(8, 8)?, 8);
    /// assert_eq!(bss.size(), 16);
    /// # Ok::<(), object_lang::ModelError>(())
    /// ```
    pub fn reserve(&mut self, size: u64, align: u64) -> Result<u64, ModelError> {
        if self.kind.is_uninitialized() {
            if !align.is_power_of_two() {
                return Err(ModelError::InvalidAlignment { align });
            }
            let offset = align_up(self.bss_size, align).ok_or(ModelError::SizeOverflow)?;
            self.bss_size = offset.checked_add(size).ok_or(ModelError::SizeOverflow)?;
            self.align = self.align.max(align);
            return Ok(offset);
        }
        let offset = self.next_offset(align)?;
        let end = offset.checked_add(size).ok_or(ModelError::SizeOverflow)?;
        self.grow_to(end, end)?;
        self.align = self.align.max(align);
        Ok(offset)
    }

    /// The offset of the next byte at a multiple of `align` (a power of two).
    fn next_offset(&self, align: u64) -> Result<u64, ModelError> {
        if !align.is_power_of_two() {
            return Err(ModelError::InvalidAlignment { align });
        }
        let len = u64::try_from(self.data.len()).map_err(|_| ModelError::SizeOverflow)?;
        align_up(len, align).ok_or(ModelError::SizeOverflow)
    }

    /// Zero-fills the data to `fill` bytes, after making room for `end` bytes in all.
    /// Allocation failure is an error here rather than an abort, so a request for an
    /// absurd size cannot take the process down; nothing changes on error.
    fn grow_to(&mut self, fill: u64, end: u64) -> Result<(), ModelError> {
        let fill = usize::try_from(fill).map_err(|_| ModelError::SizeOverflow)?;
        let end = usize::try_from(end).map_err(|_| ModelError::SizeOverflow)?;
        let additional = end.saturating_sub(self.data.len());
        self.data
            .try_reserve(additional)
            .map_err(|_| ModelError::SizeOverflow)?;
        self.data.resize(fill, 0);
        Ok(())
    }

    /// Adds a relocation that patches this section. Checked by
    /// [`validate`](crate::Object::validate).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{
    ///     Architecture, Object, Relocation, RelocationTarget, Section, SectionKind, Symbol,
    ///     X86_64Reloc,
    /// };
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let target = obj.add_symbol(Symbol::undefined("table"));
    /// let mut data = Section::new(".data", SectionKind::Data).with_data(vec![0; 8]);
    /// data.add_relocation(Relocation::new(0, X86_64Reloc::Abs64, RelocationTarget::Symbol(target), 0));
    /// obj.add_section(data);
    /// assert!(obj.validate().is_ok());
    /// ```
    pub fn add_relocation(&mut self, relocation: Relocation) {
        self.relocations.push(relocation);
    }

    /// The section's name.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// assert_eq!(Section::new(".text", SectionKind::Text).name(), ".text");
    /// ```
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the section holds.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// assert_eq!(Section::new(".bss", SectionKind::Bss).kind(), SectionKind::Bss);
    /// ```
    #[must_use]
    pub const fn kind(&self) -> SectionKind {
        self.kind
    }

    /// The section's flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionFlags, SectionKind};
    ///
    /// assert!(Section::new(".text", SectionKind::Text).flags().contains(SectionFlags::EXEC));
    /// ```
    #[must_use]
    pub const fn flags(&self) -> SectionFlags {
        self.flags
    }

    /// The alignment in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// assert_eq!(Section::new(".data", SectionKind::Data).with_align(8).align(), 8);
    /// ```
    #[must_use]
    pub const fn align(&self) -> u64 {
        self.align
    }

    /// The size of each fixed-size entry, or zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// assert_eq!(Section::new(".data", SectionKind::Data).entry_size(), 0);
    /// ```
    #[must_use]
    pub const fn entry_size(&self) -> u64 {
        self.entry_size
    }

    /// The address: the load address in an executable, normally zero in a relocatable
    /// object.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// assert_eq!(Section::new(".text", SectionKind::Text).address(), 0);
    /// ```
    #[must_use]
    pub const fn address(&self) -> u64 {
        self.address
    }

    /// The section's bytes; empty for [`Bss`](SectionKind::Bss).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// let s = Section::new(".data", SectionKind::Data).with_data(vec![7]);
    /// assert_eq!(s.data(), &[7]);
    /// ```
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The size in memory: the length of the data, or a [`Bss`](SectionKind::Bss)
    /// section's reserved size.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// assert_eq!(Section::new(".data", SectionKind::Data).with_data(vec![0; 3]).size(), 3);
    /// assert_eq!(Section::new(".bss", SectionKind::Bss).with_bss_size(9).size(), 9);
    /// ```
    #[must_use]
    pub fn size(&self) -> u64 {
        if self.kind.is_uninitialized() {
            self.bss_size
        } else {
            // A Vec's length always fits in u64 on supported targets.
            u64::try_from(self.data.len()).unwrap_or(u64::MAX)
        }
    }

    /// The relocations that patch this section, in the order added.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Section, SectionKind};
    ///
    /// assert!(Section::new(".text", SectionKind::Text).relocations().is_empty());
    /// ```
    #[must_use]
    pub fn relocations(&self) -> &[Relocation] {
        &self.relocations
    }

    /// The size a [`Bss`](SectionKind::Bss) section was given, whatever its kind (for
    /// validation, which refuses it on kinds that hold bytes).
    pub(crate) const fn bss_size(&self) -> u64 {
        self.bss_size
    }

    /// Replaces the relocations wholesale (readers attach them after the symbol table
    /// is known).
    pub(crate) fn set_relocations(&mut self, relocations: Vec<Relocation>) {
        self.relocations = relocations;
    }
}

/// `value` rounded up to a multiple of `align` (a power of two), or `None` on overflow.
pub(crate) const fn align_up(value: u64, align: u64) -> Option<u64> {
    let mask = align.wrapping_sub(1);
    match value.checked_add(mask) {
        Some(v) => Some(v & !mask),
        None => None,
    }
}
