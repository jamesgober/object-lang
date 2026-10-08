//! # object-lang
//!
//! Writes and reads the files operating systems run. A format-neutral [`Object`] —
//! sections, symbols, relocations, and for executables an entry point — goes in, and
//! ELF64 bytes for x86-64 or AArch64 come out: relocatable objects a linker can
//! consume, or static executables the kernel loads directly, with no system linker or
//! assembler involved. Reading turns ELF bytes back into the same model, strictly and
//! within budgets, without panicking on any input.
//!
//! ## Quick start
//!
//! A static Linux x86-64 program that exits with status 42, from hand-assembled bytes:
//!
//! ```
//! use object_lang::Architecture;
//!
//! let code = [
//!     0xbf, 0x2a, 0x00, 0x00, 0x00, // mov edi, 42
//!     0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60 (exit)
//!     0x0f, 0x05, //                   syscall
//! ];
//! let program = object_lang::elf::executable(Architecture::X86_64, &code)?;
//! assert_eq!(&program[..4], b"\x7fELF");
//! # Ok::<(), object_lang::WriteError>(())
//! ```
//!
//! A relocatable object, written and read back:
//!
//! ```
//! use object_lang::{
//!     Architecture, Object, Relocation, RelocationTarget, Section, SectionKind, Symbol,
//!     X86_64Reloc,
//! };
//!
//! let mut obj = Object::relocatable(Architecture::X86_64);
//! // main: call helper; ret
//! let text = obj.add_section(
//!     Section::new(".text", SectionKind::Text)
//!         .with_align(16)
//!         .with_data(vec![0xe8, 0, 0, 0, 0, 0xc3]),
//! );
//! obj.add_symbol(Symbol::function("main", text, 0, 6));
//! let helper = obj.add_symbol(Symbol::undefined("helper"));
//! obj.add_relocation(
//!     text,
//!     Relocation::new(1, X86_64Reloc::Plt32, RelocationTarget::Symbol(helper), -4),
//! )?;
//!
//! let bytes = object_lang::elf::write(&obj)?;
//! assert_eq!(object_lang::elf::read(&bytes)?, obj);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## The model
//!
//! - An [`Object`] is a [`FileKind`] (relocatable or executable), an [`Architecture`],
//!   [`Section`]s, [`Symbol`]s, and an entry point.
//! - A [`Section`] has a [`SectionKind`], [`SectionFlags`], an alignment, an address,
//!   and either bytes or (for BSS) a size, plus the [`Relocation`]s that patch it.
//! - A [`Relocation`] patches `offset` with a [`RelocationKind`] computed from a
//!   [`RelocationTarget`] (a symbol or a section) and an explicit addend.
//! - [`Object::validate`] checks every rule; writers run it first and readers run it on
//!   what they read, so errors are values ([`ModelError`], [`WriteError`],
//!   [`ReadError`]), never malformed files.
//!
//! Nothing in the model is ELF-specific: kinds are named for what they mean, not for
//! one format's numbering, so COFF/PE and Mach-O writers fit the same model.
//!
//! ## Features
//!
//! - `std` (default): nothing beyond `alloc` is used; without it the crate is
//!   `no_std` and the API is identical.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(
    warnings,
    missing_docs,
    unsafe_op_in_unsafe_fn,
    unused_must_use,
    unused_results,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::dbg_macro,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::undocumented_unsafe_blocks
)]

extern crate alloc;

pub mod elf;
mod error;
mod limits;
mod model;
mod reloc;
mod section;
mod symbol;
mod validate;

pub use error::{
    ModelError, ReadError, RelocationProblem, SectionProblem, SymbolProblem, WriteError,
};
pub use limits::Limits;
pub use model::{Architecture, FileKind, Object, SectionId, SymbolId};
pub use reloc::{Aarch64Reloc, Relocation, RelocationKind, RelocationTarget, X86_64Reloc};
pub use section::{Section, SectionFlags, SectionKind};
pub use symbol::{Binding, Symbol, SymbolKind, SymbolSection, Visibility};

/// Compiles and runs the `rust` code blocks in `README.md` and `docs/API.md` as
/// part of `cargo test`, so the published examples cannot drift from the API.
///
/// Present only while collecting doctests (`#[cfg(doctest)]`); it is not part of
/// the public surface and does not appear in the built library or its docs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
#[doc = include_str!("../docs/API.md")]
pub struct MarkdownDocTests;
