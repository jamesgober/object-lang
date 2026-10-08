//! Errors: what is wrong with an object model, why a file could not be written, and why
//! bytes could not be read.

use core::fmt;

use crate::model::{SectionId, SymbolId};

/// What is wrong with one section. Reported inside [`ModelError::Section`].
///
/// # Examples
///
/// ```
/// use object_lang::SectionProblem;
///
/// assert_eq!(SectionProblem::BadAlignment.to_string(), "alignment is not a power of two");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum SectionProblem {
    /// The name contains a NUL byte, which no format can store.
    NameContainsNul,
    /// The alignment is zero or not a power of two.
    BadAlignment,
    /// The flags are not allowed for the section's kind (see
    /// [`SectionKind`](crate::SectionKind)).
    FlagsDoNotMatchKind,
    /// [`MERGE`](crate::SectionFlags::MERGE) is set but the entry size is zero.
    MergeWithoutEntrySize,
    /// A [`Bss`](crate::SectionKind::Bss) section has bytes, or another kind has a BSS
    /// size.
    ContentsDoNotMatchKind,
    /// The address is not a multiple of the alignment.
    MisalignedAddress,
    /// The address plus the size overflows the 64-bit address space.
    AddressOverflow,
}

impl fmt::Display for SectionProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SectionProblem::NameContainsNul => "name contains a NUL byte",
            SectionProblem::BadAlignment => "alignment is not a power of two",
            SectionProblem::FlagsDoNotMatchKind => "flags are not allowed for the section's kind",
            SectionProblem::MergeWithoutEntrySize => "mergeable section has an entry size of zero",
            SectionProblem::ContentsDoNotMatchKind => {
                "contents do not match the kind (bytes in a BSS section, or a BSS size on another kind)"
            }
            SectionProblem::MisalignedAddress => "address is not a multiple of the alignment",
            SectionProblem::AddressOverflow => "address plus size overflows the address space",
        })
    }
}

/// What is wrong with one symbol. Reported inside [`ModelError::Symbol`].
///
/// # Examples
///
/// ```
/// use object_lang::SymbolProblem;
///
/// assert_eq!(SymbolProblem::LocalUndefined.to_string(), "a local symbol must be defined");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum SymbolProblem {
    /// The name contains a NUL byte.
    NameContainsNul,
    /// The symbol is defined in a section the object does not have.
    UnknownSection,
    /// The symbol (its value, or value plus size) lies outside its section.
    OutOfSection,
    /// A local symbol is undefined or common: nothing outside the object could define it.
    LocalUndefined,
    /// A [`File`](crate::SymbolKind::File) symbol is not local and absolute with value
    /// and size zero.
    BadFileSymbol,
    /// A [`Common`](crate::SymbolSection::Common) symbol is not global, or its alignment
    /// (its value) is not a power of two.
    BadCommonSymbol,
    /// A [`Common`](crate::SymbolSection::Common) symbol in an executable: an
    /// executable's storage is already allocated.
    CommonInExecutable,
    /// A non-weak undefined symbol in an executable: nothing is left to define it.
    UndefinedInExecutable,
    /// A [`Tls`](crate::SymbolKind::Tls) symbol defined in a section without the
    /// [`TLS`](crate::SectionFlags::TLS) flag.
    TlsOutsideTlsSection,
}

impl fmt::Display for SymbolProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SymbolProblem::NameContainsNul => "name contains a NUL byte",
            SymbolProblem::UnknownSection => "defined in a section the object does not have",
            SymbolProblem::OutOfSection => "lies outside its section",
            SymbolProblem::LocalUndefined => "a local symbol must be defined",
            SymbolProblem::BadFileSymbol => {
                "a file symbol must be local and absolute, with value and size zero"
            }
            SymbolProblem::BadCommonSymbol => {
                "a common symbol must be global, with a power-of-two alignment as its value"
            }
            SymbolProblem::CommonInExecutable => "an executable cannot hold a common symbol",
            SymbolProblem::UndefinedInExecutable => {
                "an executable cannot hold a non-weak undefined symbol"
            }
            SymbolProblem::TlsOutsideTlsSection => {
                "a thread-local symbol is defined outside a thread-local section"
            }
        })
    }
}

/// What is wrong with one relocation. Reported inside [`ModelError::Relocation`].
///
/// # Examples
///
/// ```
/// use object_lang::RelocationProblem;
///
/// assert_eq!(RelocationProblem::OutOfSection.to_string(), "patches bytes outside its section");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum RelocationProblem {
    /// The relocation kind belongs to another architecture.
    WrongArchitecture,
    /// The patched bytes run past the end of the section.
    OutOfSection,
    /// An instruction relocation whose offset is not a multiple of 4.
    Misaligned,
    /// The target symbol or section does not exist.
    UnknownTarget,
    /// The section is [`Bss`](crate::SectionKind::Bss): there are no bytes to patch.
    InUninitializedSection,
    /// The object is an executable, which is fully linked and carries no relocations.
    InExecutable,
}

impl fmt::Display for RelocationProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RelocationProblem::WrongArchitecture => "kind belongs to another architecture",
            RelocationProblem::OutOfSection => "patches bytes outside its section",
            RelocationProblem::Misaligned => "instruction relocation is not 4-byte aligned",
            RelocationProblem::UnknownTarget => "target symbol or section does not exist",
            RelocationProblem::InUninitializedSection => {
                "patches a BSS section, which has no bytes"
            }
            RelocationProblem::InExecutable => "executables carry no relocations",
        })
    }
}

/// A rule of the object model that an [`Object`](crate::Object) breaks, from
/// [`Object::validate`](crate::Object::validate), from the builders on
/// [`Section`](crate::Section), or from a reader whose input decodes to an invalid model.
///
/// The enum is `#[non_exhaustive]`: match it with a wildcard arm.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, ModelError, Object, Symbol, SymbolProblem, Binding};
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// let sym = obj.add_symbol(Symbol::undefined("x").with_binding(Binding::Local));
/// assert_eq!(
///     obj.validate(),
///     Err(ModelError::Symbol { symbol: sym, problem: SymbolProblem::LocalUndefined }),
/// );
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum ModelError {
    /// A section breaks a rule.
    Section {
        /// The section.
        section: SectionId,
        /// The rule it breaks.
        problem: SectionProblem,
    },
    /// A symbol breaks a rule.
    Symbol {
        /// The symbol.
        symbol: SymbolId,
        /// The rule it breaks.
        problem: SymbolProblem,
    },
    /// A relocation breaks a rule.
    Relocation {
        /// The section the relocation patches.
        section: SectionId,
        /// The relocation's position in that section's
        /// [`relocations`](crate::Section::relocations).
        index: usize,
        /// The rule it breaks.
        problem: RelocationProblem,
    },
    /// Two loadable sections of an executable overlap in memory.
    SectionsOverlap {
        /// The section at the lower address.
        first: SectionId,
        /// The section that starts inside it.
        second: SectionId,
    },
    /// An executable's entry point is not inside an executable section.
    EntryNotExecutable {
        /// The entry address.
        entry: u64,
    },
    /// A section id that the object does not have, passed to
    /// [`Object::add_relocation`](crate::Object::add_relocation).
    UnknownSection {
        /// The id.
        section: SectionId,
    },
    /// An alignment that is zero or not a power of two, passed to
    /// [`Section::append`](crate::Section::append) or
    /// [`Section::reserve`](crate::Section::reserve).
    InvalidAlignment {
        /// The alignment.
        align: u64,
    },
    /// [`Section::append`](crate::Section::append) on a
    /// [`Bss`](crate::SectionKind::Bss) section, which holds no bytes; use
    /// [`reserve`](crate::Section::reserve).
    AppendToBss,
    /// A section would grow past the 64-bit size range or past available memory.
    SizeOverflow,
    /// The object holds more sections or symbols than ids can number (2³¹ − 1).
    TooManyItems,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::Section { section, problem } => {
                write!(f, "section #{}: {problem}", section.index())
            }
            ModelError::Symbol { symbol, problem } => {
                write!(f, "symbol #{}: {problem}", symbol.index())
            }
            ModelError::Relocation {
                section,
                index,
                problem,
            } => write!(
                f,
                "relocation #{index} of section #{}: {problem}",
                section.index()
            ),
            ModelError::SectionsOverlap { first, second } => write!(
                f,
                "sections #{} and #{} overlap in memory",
                first.index(),
                second.index()
            ),
            ModelError::EntryNotExecutable { entry } => {
                write!(
                    f,
                    "entry point {entry:#x} is not inside an executable section"
                )
            }
            ModelError::UnknownSection { section } => {
                write!(f, "the object has no section #{}", section.index())
            }
            ModelError::InvalidAlignment { align } => {
                write!(f, "alignment {align} is not a power of two")
            }
            ModelError::AppendToBss => {
                f.write_str("cannot append bytes to a BSS section; reserve space instead")
            }
            ModelError::SizeOverflow => f.write_str("section size overflows"),
            ModelError::TooManyItems => f.write_str("too many sections or symbols to number"),
        }
    }
}

impl core::error::Error for ModelError {}

/// Why an object could not be written.
///
/// The enum is `#[non_exhaustive]`: match it with a wildcard arm.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, Object, Section, SectionKind, WriteError};
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// let note = obj.add_section(Section::new(".note.GNU-stack", SectionKind::Other));
/// assert_eq!(
///     object_lang::elf::write(&obj),
///     Err(WriteError::ReservedSectionName { section: note }),
/// );
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum WriteError {
    /// The object breaks a rule of the model.
    Invalid(ModelError),
    /// A section uses a name the writer generates itself (ELF: `.note.GNU-stack` in a
    /// relocatable object, which records [`executable_stack`](crate::Object::executable_stack)).
    ReservedSectionName {
        /// The section.
        section: SectionId,
    },
    /// Two loadable sections of an executable with different permissions, or a BSS
    /// section followed by initialized data, share a page. Each permission change must
    /// start on a fresh [page](crate::Architecture::page_size).
    SegmentsSharePage {
        /// The last section of the earlier segment.
        first: SectionId,
        /// The first section of the later one.
        second: SectionId,
    },
    /// An executable's lowest section is too close to address zero to map the file
    /// headers on the pages below it.
    NoRoomForHeaders,
    /// The object uses something this writer does not support yet; the message says
    /// what.
    Unsupported {
        /// What is unsupported.
        what: &'static str,
    },
    /// The file would exceed the format's size limits or the address space.
    TooLarge,
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Invalid(err) => write!(f, "invalid object: {err}"),
            WriteError::ReservedSectionName { section } => write!(
                f,
                "section #{} uses a name the writer reserves for itself",
                section.index()
            ),
            WriteError::SegmentsSharePage { first, second } => write!(
                f,
                "sections #{} and #{} need different page permissions but share a page",
                first.index(),
                second.index()
            ),
            WriteError::NoRoomForHeaders => f.write_str(
                "the lowest section is too close to address zero to map the headers below it",
            ),
            WriteError::Unsupported { what } => write!(f, "unsupported: {what}"),
            WriteError::TooLarge => f.write_str("the file would exceed the format's size limits"),
        }
    }
}

impl core::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            WriteError::Invalid(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ModelError> for WriteError {
    fn from(err: ModelError) -> Self {
        WriteError::Invalid(err)
    }
}

/// Why bytes could not be read as an object.
///
/// Readers are strict: anything the model cannot represent exactly, or that a careful
/// writer would never produce, is refused rather than guessed at. The enum is
/// `#[non_exhaustive]`: match it with a wildcard arm.
///
/// # Examples
///
/// ```
/// use object_lang::ReadError;
///
/// assert_eq!(object_lang::elf::read(b"MZ\x90\x00"), Err(ReadError::Truncated { what: "file header" }));
/// assert_eq!(object_lang::elf::read(&[0u8; 64]), Err(ReadError::BadMagic));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum ReadError {
    /// The bytes do not start with the format's magic number.
    BadMagic,
    /// A structure runs past the end of the input.
    Truncated {
        /// The structure.
        what: &'static str,
    },
    /// A field holds a value the format does not allow, or structures contradict each
    /// other.
    Malformed {
        /// What is wrong.
        what: &'static str,
    },
    /// The file is valid but uses something this reader does not support.
    Unsupported {
        /// What is unsupported.
        what: &'static str,
        /// The raw value of the unsupported field (a type, flag set, or class).
        value: u64,
    },
    /// A [`Limits`](crate::Limits) budget was exceeded.
    LimitExceeded {
        /// Which limit.
        limit: &'static str,
    },
    /// The file decodes to an object that breaks a rule of the model.
    Invalid(ModelError),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::BadMagic => {
                f.write_str("not an object file of this format (bad magic number)")
            }
            ReadError::Truncated { what } => {
                write!(f, "truncated: {what} runs past the end of the input")
            }
            ReadError::Malformed { what } => write!(f, "malformed: {what}"),
            ReadError::Unsupported { what, value } => write!(f, "unsupported {what} ({value:#x})"),
            ReadError::LimitExceeded { limit } => write!(f, "limit exceeded: {limit}"),
            ReadError::Invalid(err) => write!(f, "invalid object: {err}"),
        }
    }
}

impl core::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            ReadError::Invalid(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ModelError> for ReadError {
    fn from(err: ModelError) -> Self {
        ReadError::Invalid(err)
    }
}
