//! ELF64, little-endian, for x86-64 and AArch64: relocatable objects (`ET_REL`) and
//! static executables (`ET_EXEC`).
//!
//! - [`write()`] and [`write_into`] turn an [`Object`] into ELF bytes.
//! - [`read()`] and [`read_with_limits`] turn ELF bytes back into an [`Object`].
//! - [`executable`] is the one-call path from machine code to a runnable program.
//!
//! ## What the writer produces
//!
//! A **relocatable object** holds the object's sections in order (section header `i + 1`
//! is the model's section `i`), then one `.rela<name>` section per section with
//! relocations, an empty `.note.GNU-stack` marking whether the stack must be executable,
//! `.symtab` (with `.symtab_shndx` when a section index exceeds 16 bits), `.strtab`, and
//! `.shstrtab`. The symbol table starts with the null symbol, then one `STT_SECTION`
//! symbol for each section a relocation targets directly, then the object's local
//! symbols, then the rest, each group in model order; `sh_info` is the index of the
//! first non-local symbol. String tables share the bytes of names that are suffixes of
//! other names. Section counts and indices past 0xff00 use ELF's extended numbering.
//!
//! An **executable** maps its file headers read-only on the page(s) just below its
//! lowest section, then one `PT_LOAD` per run of sections with the same permissions,
//! each aligned to the [page size](crate::Architecture::page_size) with its file offset
//! congruent to its address. A `PT_GNU_STACK` header marks the stack non-executable
//! unless [`executable_stack`](Object::executable_stack) is set. There is no
//! interpreter and no dynamic section: the program is fully static. Section headers and
//! a symbol table are written too, so tools can name what they disassemble.
//!
//! The output depends only on the object: the same object always gives the same bytes.
//!
//! ## What the reader accepts
//!
//! Exactly the files the model can represent: ELF64 little-endian `ET_REL` or `ET_EXEC`
//! for x86-64 or AArch64, with `SHT_RELA` relocations of the kinds in
//! [`X86_64Reloc`](crate::X86_64Reloc) and [`Aarch64Reloc`](crate::Aarch64Reloc). It
//! drops what carries no meaning in the model (`STT_SECTION` symbols, which become
//! [`RelocationTarget::Section`](crate::RelocationTarget::Section); LLVM's
//! `.llvm_addrsig` hint) and refuses everything else it cannot represent exactly
//! (COMDAT groups, `SHT_REL`, dynamic sections, unknown section, symbol, or
//! relocation types) with a [`ReadError`].

mod consts;
mod read;
mod strtab;
mod write;

use alloc::vec::Vec;

use crate::error::{ReadError, WriteError};
use crate::limits::Limits;
use crate::model::{Architecture, Object};
use crate::section::{Section, SectionKind};
use crate::symbol::Symbol;

/// Writes an object as an ELF file and returns the bytes.
///
/// The object is [validated](Object::validate) first. The output is deterministic.
///
/// # Errors
///
/// - [`WriteError::Invalid`] if the object breaks a rule of the model.
/// - [`WriteError::ReservedSectionName`] for a relocatable object with a section named
///   `.note.GNU-stack`, which the writer emits itself.
/// - For executables: [`WriteError::SegmentsSharePage`],
///   [`WriteError::NoRoomForHeaders`], and [`WriteError::Unsupported`] for
///   thread-local sections.
/// - [`WriteError::TooLarge`] if a table outgrows ELF's 32-bit fields.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, Object, Section, SectionKind, Symbol};
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0xc3]));
/// obj.add_symbol(Symbol::function("ret", text, 0, 1));
///
/// let bytes = object_lang::elf::write(&obj)?;
/// assert_eq!(&bytes[..4], b"\x7fELF");
/// assert_eq!(object_lang::elf::read(&bytes)?, obj);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn write(object: &Object) -> Result<Vec<u8>, WriteError> {
    let mut out = Vec::new();
    write::write_into(object, &mut out)?;
    Ok(out)
}

/// Writes an object as an ELF file, appending the bytes to `out`, so one buffer can be
/// reused across many objects. On error, `out` is left as it was.
///
/// # Errors
///
/// As [`write()`].
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, Object};
///
/// let mut buffer = Vec::new();
/// for arch in [Architecture::X86_64, Architecture::Aarch64] {
///     buffer.clear();
///     object_lang::elf::write_into(&Object::relocatable(arch), &mut buffer)?;
///     assert_eq!(object_lang::elf::read(&buffer)?.architecture(), arch);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn write_into(object: &Object, out: &mut Vec<u8>) -> Result<(), WriteError> {
    write::write_into(object, out)
}

/// Reads an ELF file into an object, with the default [`Limits`].
///
/// # Errors
///
/// A [`ReadError`] saying what is wrong: [`BadMagic`](ReadError::BadMagic) for bytes
/// that are not ELF, [`Truncated`](ReadError::Truncated) when a structure runs past the
/// input, [`Malformed`](ReadError::Malformed) when fields contradict the format or each
/// other, [`Unsupported`](ReadError::Unsupported) for valid ELF the model cannot
/// represent, [`LimitExceeded`](ReadError::LimitExceeded) when a budget is exceeded,
/// and [`Invalid`](ReadError::Invalid) when the decoded object breaks a model rule. It
/// never panics, whatever the bytes.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, FileKind};
///
/// let exe = object_lang::elf::executable(Architecture::X86_64, &[0xeb, 0xfe])?; // jmp .
/// let obj = object_lang::elf::read(&exe)?;
/// assert_eq!(obj.kind(), FileKind::Executable);
/// assert_eq!(obj.sections()[0].data(), &[0xeb, 0xfe]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn read(bytes: &[u8]) -> Result<Object, ReadError> {
    read::read(bytes, &Limits::DEFAULT)
}

/// Reads an ELF file into an object, with the given [`Limits`].
///
/// # Errors
///
/// As [`read()`], with [`ReadError::LimitExceeded`] naming the first budget exceeded.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, Limits, Object, ReadError, Symbol};
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// obj.add_symbol(Symbol::undefined("a_rather_long_symbol_name"));
/// let bytes = object_lang::elf::write(&obj)?;
///
/// let mut limits = Limits::default();
/// limits.max_name_len = 8;
/// assert_eq!(
///     object_lang::elf::read_with_limits(&bytes, &limits),
///     Err(ReadError::LimitExceeded { limit: "max_name_len" }),
/// );
/// # Ok::<(), object_lang::WriteError>(())
/// ```
pub fn read_with_limits(bytes: &[u8], limits: &Limits) -> Result<Object, ReadError> {
    read::read(bytes, limits)
}

/// The address [`executable`] loads its code at: one page above 4 MiB (`0x40_0000`), the
/// traditional base of a static Linux executable, with the file headers on the page(s)
/// below it.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, elf};
///
/// assert_eq!(elf::code_address(Architecture::X86_64), 0x40_1000);
/// assert_eq!(elf::code_address(Architecture::Aarch64), 0x41_0000);
/// ```
#[must_use]
pub const fn code_address(arch: Architecture) -> u64 {
    0x40_0000 + arch.page_size()
}

/// Builds a static executable from machine code: the one-call path to a program the OS
/// can run.
///
/// `code` becomes a `.text` section at [`code_address`], 16-byte aligned, and execution
/// starts at its first byte, where a global `_start` symbol marks it. The code must not
/// need relocations or data of its own beyond what it carries; for anything more,
/// build an executable [`Object`] and [`write()`] it.
///
/// # Errors
///
/// [`WriteError::Invalid`] with [`EntryNotExecutable`](crate::ModelError::EntryNotExecutable)
/// if `code` is empty.
///
/// # Examples
///
/// `exit(42)` on Linux x86-64, hand-assembled:
///
/// ```
/// use object_lang::Architecture;
///
/// let code = [
///     0xbf, 0x2a, 0x00, 0x00, 0x00, // mov edi, 42
///     0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60 (exit)
///     0x0f, 0x05, //                   syscall
/// ];
/// let program = object_lang::elf::executable(Architecture::X86_64, &code)?;
/// // Write `program` to a file, mark it executable, run it: it exits with status 42.
/// assert_eq!(&program[..4], b"\x7fELF");
/// # Ok::<(), object_lang::WriteError>(())
/// ```
pub fn executable(arch: Architecture, code: &[u8]) -> Result<Vec<u8>, WriteError> {
    let address = code_address(arch);
    let mut object = Object::executable(arch);
    let text = object.add_section(
        Section::new(".text", SectionKind::Text)
            .with_align(16)
            .with_address(address)
            .with_data(code.to_vec()),
    );
    let size = u64::try_from(code.len()).map_err(|_| WriteError::TooLarge)?;
    let _start = object.add_symbol(Symbol::function("_start", text, address, size));
    object.set_entry(address);
    write(&object)
}
