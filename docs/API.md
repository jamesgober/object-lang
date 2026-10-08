# object-lang &mdash; API Reference

> Complete reference for every public item in `object-lang` 0.2, with examples.
> **Status: pre-1.0.** The surface is designed across the 0.x series and frozen at
> `1.0`, after a real consumer has driven it end to end. See
> [`../dev/ROADMAP.md`](../dev/ROADMAP.md).

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>

## Table of contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Concepts](#concepts)
  - [Relocatable objects and executables](#relocatable-objects-and-executables)
  - [Sections: kinds and flags](#sections-kinds-and-flags)
  - [Symbols and values](#symbols-and-values)
  - [Relocations](#relocations)
  - [Validation](#validation)
- [The `elf` module](#the-elf-module)
  - [`elf::write`](#elfwrite)
  - [`elf::write_into`](#elfwrite_into)
  - [`elf::read`](#elfread)
  - [`elf::read_with_limits`](#elfread_with_limits)
  - [`elf::executable`](#elfexecutable)
  - [`elf::code_address`](#elfcode_address)
  - [ELF layout](#elf-layout)
  - [What the reader accepts](#what-the-reader-accepts)
- [`Object`](#object)
  - [`Object::validate`](#objectvalidate)
- [`Architecture`](#architecture)
- [`FileKind`](#filekind)
- [`SectionId`, `SymbolId`](#sectionid-symbolid)
- [`Section`](#section)
- [`SectionKind`](#sectionkind)
- [`SectionFlags`](#sectionflags)
- [`Symbol`](#symbol)
- [`SymbolKind`](#symbolkind)
- [`Binding`](#binding)
- [`Visibility`](#visibility)
- [`SymbolSection`](#symbolsection)
- [`Relocation`](#relocation)
- [`RelocationKind`](#relocationkind)
- [`X86_64Reloc`](#x86_64reloc)
- [`Aarch64Reloc`](#aarch64reloc)
- [`RelocationTarget`](#relocationtarget)
- [`Limits`](#limits)
- [`ModelError`](#modelerror)
- [`SectionProblem`](#sectionproblem)
- [`SymbolProblem`](#symbolproblem)
- [`RelocationProblem`](#relocationproblem)
- [`WriteError`](#writeerror)
- [`ReadError`](#readerror)
- [Feature flags](#feature-flags)
- [Not supported yet](#not-supported-yet)

## Overview

`object-lang` turns a format-neutral description of an object file into the bytes of a
real file format, and back. Version 0.2 writes and reads **ELF64, little-endian, for
x86-64 and AArch64**: relocatable objects (`ET_REL`) for a linker, and static
executables (`ET_EXEC`) for the Linux kernel.

| Item | Kind | Purpose |
|---|---|---|
| [`elf`](#the-elf-module) | module | ELF writer, reader, and the one-call executable builder. |
| [`Object`](#object) | struct | One object file or executable. |
| [`Architecture`](#architecture) | enum | x86-64 or AArch64. |
| [`FileKind`](#filekind) | enum | Relocatable object or executable. |
| [`SectionId`, `SymbolId`](#sectionid-symbolid) | structs | Dense ids: positions in the object's lists. |
| [`Section`](#section) | struct | Bytes (or a BSS size), kind, flags, alignment, address, relocations. |
| [`SectionKind`](#sectionkind) | enum | What a section holds. |
| [`SectionFlags`](#sectionflags) | struct | How a section is used (a bit set). |
| [`Symbol`](#symbol) | struct | A named address. |
| [`SymbolKind`](#symbolkind), [`Binding`](#binding), [`Visibility`](#visibility), [`SymbolSection`](#symbolsection) | enums | A symbol's properties. |
| [`Relocation`](#relocation) | struct | A place to patch. |
| [`RelocationKind`](#relocationkind), [`X86_64Reloc`](#x86_64reloc), [`Aarch64Reloc`](#aarch64reloc) | enums | How to patch it. |
| [`RelocationTarget`](#relocationtarget) | enum | Whose address to patch in. |
| [`Limits`](#limits) | struct | Budgets for reading untrusted files. |
| [`ModelError`](#modelerror), [`SectionProblem`](#sectionproblem), [`SymbolProblem`](#symbolproblem), [`RelocationProblem`](#relocationproblem) | enums | Rules an object breaks. |
| [`WriteError`](#writeerror) | enum | Why a file could not be written. |
| [`ReadError`](#readerror) | enum | Why bytes could not be read. |

## Installation

```toml
[dependencies]
object-lang = "0.2"
```

For `no_std` targets (the crate needs only `alloc`):

```toml
[dependencies]
object-lang = { version = "0.2", default-features = false }
```

## Quick start

```rust
use object_lang::{
    Architecture, Object, Relocation, RelocationTarget, Section, SectionKind, Symbol,
    X86_64Reloc,
};

// The one-call path: machine code in, a runnable static executable out.
let exit42 = [0xbf, 0x2a, 0, 0, 0, 0xb8, 0x3c, 0, 0, 0, 0x0f, 0x05];
let program = object_lang::elf::executable(Architecture::X86_64, &exit42)?;
assert_eq!(object_lang::elf::read(&program)?.entry(), 0x40_1000);

// The general path: describe an object, write it, read it back.
let mut obj = Object::relocatable(Architecture::X86_64);
let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0xe8, 0, 0, 0, 0, 0xc3]));
obj.add_symbol(Symbol::function("main", text, 0, 6));
let exit = obj.add_symbol(Symbol::undefined("exit"));
obj.add_relocation(text, Relocation::new(1, X86_64Reloc::Plt32, RelocationTarget::Symbol(exit), -4))?;

let bytes = object_lang::elf::write(&obj)?;
assert_eq!(object_lang::elf::read(&bytes)?, obj);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Concepts

### Relocatable objects and executables

A **relocatable object** is what a compiler produces for a linker: sections at no fixed
address (normally zero), symbol values that are offsets within their section, and
relocations still to be applied. An **executable** is fully linked: every loadable
section has its final address, symbol values are addresses, nothing is left to
relocate, and execution starts at the entry point, which must lie inside an executable
section.

| | Relocatable | Executable |
|---|---|---|
| Section address | as given (normally 0) | final load address |
| Symbol value | offset in its section | address |
| Relocations | allowed | refused |
| Undefined symbols | allowed | weak only |
| Common symbols | allowed | refused |
| Entry point | as given (normally 0) | must be inside an executable section |
| Loadable sections | may overlap (they are not placed yet) | must not overlap |

### Sections: kinds and flags

A section's [`SectionKind`](#sectionkind) says what it holds; its [`SectionFlags`](#sectionflags) say
how it is used. [`Section::new`](#section) gives each kind its required flags; extra flags
(`TLS` on data, `MERGE`/`STRINGS` on constants, `ALLOC` on a note) are added with
`with_flags`. For the four kinds stored as plain bytes, the flags alone determine the
kind &mdash; with `ALLOC`, `EXEC` means code, else `WRITE` means data, else read-only data,
and without `ALLOC` the section is `Other` &mdash; so a file read back gives the exact kind
written.

```rust
use object_lang::{Section, SectionFlags, SectionKind};

let strings = Section::new(".rodata.str1.1", SectionKind::ReadOnlyData)
    .with_flags(SectionFlags::ALLOC | SectionFlags::MERGE | SectionFlags::STRINGS)
    .with_entry_size(1);
assert!(SectionKind::ReadOnlyData.allows(strings.flags()));
// Writable flags on read-only data would read back as data, so they are refused.
assert!(!SectionKind::ReadOnlyData.allows(SectionFlags::ALLOC | SectionFlags::WRITE));
```

### Symbols and values

A symbol is defined in a section, or is [undefined](#symbolsection) (another object
defines it), [absolute](#symbolsection) (a constant), or [common](#symbolsection) (a
tentative definition the linker allocates). Its value is an offset (relocatable), an
address (executable), or an alignment (common). A defined symbol must lie inside its
section: `value + size` may reach the section's end but not pass it.

Writers may reorder symbols: ELF puts every local symbol before every other. A file read
back lists them in the format's order, so ids issued while building may differ from ids
after reading; names and properties are preserved.

```rust
use object_lang::{Architecture, Binding, Object, Section, SectionKind, Symbol};

let mut obj = Object::relocatable(Architecture::X86_64);
let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0xc3; 2]));
obj.add_symbol(Symbol::function("api", text, 0, 1));
obj.add_symbol(Symbol::function("helper", text, 1, 1).with_binding(Binding::Local));

let back = object_lang::elf::read(&object_lang::elf::write(&obj)?)?;
let names: Vec<&str> = back.symbols().iter().map(|s| s.name.as_str()).collect();
assert_eq!(names, ["helper", "api"]); // locals first
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Relocations

A relocation lives in the section it patches. It names an offset, a
[kind](#relocationkind) (which says how many bytes are patched and with what formula),
a [target](#relocationtarget) (a symbol, or the start of a section), and an explicit
addend. Relocation kinds belong to one architecture; an object may only carry its own.
Instruction relocations on AArch64 must be 4-byte aligned.

```rust
use object_lang::{Aarch64Reloc, Architecture, Object, Relocation, RelocationTarget, Section, SectionKind, Symbol};

let mut obj = Object::relocatable(Architecture::Aarch64);
let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0; 8]));
let data = obj.add_section(Section::new(".data", SectionKind::Data).with_data(vec![0; 16]));
// adrp x0, data+8 ; add x0, x0, :lo12:data+8
let at = RelocationTarget::Section(data);
obj.add_relocation(text, Relocation::new(0, Aarch64Reloc::AdrPrelPgHi21, at, 8))?;
obj.add_relocation(text, Relocation::new(4, Aarch64Reloc::AddAbsLo12Nc, at, 8))?;
assert!(obj.validate().is_ok());
# Ok::<(), object_lang::ModelError>(())
```

### Validation

Building never fails (except for the few [`Section`](#section) builders that compute
offsets); [`Object::validate`](#objectvalidate) checks every rule at once and returns the
first violation as a [`ModelError`](#modelerror). Writers validate before writing, and
readers validate what they decode, so an invalid object is never written and an invalid
file is never accepted.

## The `elf` module

ELF64, little-endian, for x86-64 (`EM_X86_64`) and AArch64 (`EM_AARCH64`).

### `elf::write`

```rust,ignore
pub fn write(object: &Object) -> Result<Vec<u8>, WriteError>
```

Writes an object as an ELF file. The object is validated first; the output is
deterministic (the same object always gives the same bytes).

**Errors:** [`WriteError::Invalid`](#writeerror) for a model rule; for relocatable
objects, [`ReservedSectionName`](#writeerror) if a section is named `.note.GNU-stack`; for
executables, [`SegmentsSharePage`](#writeerror), [`NoRoomForHeaders`](#writeerror), and
[`Unsupported`](#writeerror) for thread-local sections; [`TooLarge`](#writeerror) if a table
outgrows ELF's 32-bit fields.

```rust
use object_lang::{Architecture, Object, Section, SectionKind, Symbol};

let mut obj = Object::relocatable(Architecture::Aarch64);
let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0xc0, 0x03, 0x5f, 0xd6]));
obj.add_symbol(Symbol::function("ret", text, 0, 4));
let bytes = object_lang::elf::write(&obj)?;
assert_eq!(&bytes[..4], b"\x7fELF");
assert_eq!(object_lang::elf::write(&obj)?, bytes); // deterministic
# Ok::<(), object_lang::WriteError>(())
```

### `elf::write_into`

```rust,ignore
pub fn write_into(object: &Object, out: &mut Vec<u8>) -> Result<(), WriteError>
```

As [`write`](#elfwrite), appending to `out` so a buffer can be reused. On error `out` is
left exactly as it was.

```rust
use object_lang::{Architecture, Object};

let mut buffer = Vec::with_capacity(4096);
object_lang::elf::write_into(&Object::relocatable(Architecture::X86_64), &mut buffer)?;
let first = buffer.len();
buffer.clear();
object_lang::elf::write_into(&Object::relocatable(Architecture::X86_64), &mut buffer)?;
assert_eq!(buffer.len(), first);
# Ok::<(), object_lang::WriteError>(())
```

### `elf::read`

```rust,ignore
pub fn read(bytes: &[u8]) -> Result<Object, ReadError>
```

Reads an ELF file with the default [`Limits`](#limits). Never panics, whatever the bytes.

**Errors:** a [`ReadError`](#readerror) naming the first problem.

```rust
use object_lang::{Architecture, FileKind, ReadError};

let exe = object_lang::elf::executable(Architecture::Aarch64, &[0x00, 0x00, 0x00, 0x14])?;
let obj = object_lang::elf::read(&exe)?;
assert_eq!((obj.kind(), obj.architecture()), (FileKind::Executable, Architecture::Aarch64));

assert_eq!(object_lang::elf::read(b"\x7fELF"), Err(ReadError::Truncated { what: "file header" }));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `elf::read_with_limits`

```rust,ignore
pub fn read_with_limits(bytes: &[u8], limits: &Limits) -> Result<Object, ReadError>
```

As [`read`](#elfread), with the caller's budgets.

```rust
use object_lang::{Architecture, Limits, Object, ReadError, Symbol};

let mut obj = Object::relocatable(Architecture::X86_64);
for i in 0..10 {
    obj.add_symbol(Symbol::undefined(format!("s{i}")));
}
let bytes = object_lang::elf::write(&obj)?;
let mut limits = Limits::default();
limits.max_symbols = 5;
assert_eq!(
    object_lang::elf::read_with_limits(&bytes, &limits),
    Err(ReadError::LimitExceeded { limit: "max_symbols" }),
);
# Ok::<(), object_lang::WriteError>(())
```

### `elf::executable`

```rust,ignore
pub fn executable(arch: Architecture, code: &[u8]) -> Result<Vec<u8>, WriteError>
```

The one-call path: a static executable whose `.text` (16-byte aligned) holds `code` at
[`code_address(arch)`](#elfcode_address), with execution starting at its first byte,
marked by a global `_start`. The code must be position-dependent machine code that
needs no relocation.

**Errors:** [`WriteError::Invalid`](#writeerror) wrapping
[`EntryNotExecutable`](#modelerror) if `code` is empty.

```rust
use object_lang::Architecture;

// exit(42) on Linux AArch64: mov x0, #42; mov x8, #93; svc #0
let code = [0x40, 0x05, 0x80, 0xd2, 0xa8, 0x0b, 0x80, 0xd2, 0x01, 0x00, 0x00, 0xd4];
let program = object_lang::elf::executable(Architecture::Aarch64, &code)?;
let obj = object_lang::elf::read(&program)?;
assert_eq!(obj.symbols()[0].name, "_start");
assert!(object_lang::elf::executable(Architecture::Aarch64, &[]).is_err());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `elf::code_address`

```rust,ignore
pub const fn code_address(arch: Architecture) -> u64
```

Where [`executable`](#elfexecutable) loads its code: `0x40_0000 + arch.page_size()`, so
the file headers are mapped on the page(s) at `0x40_0000` just below.

```rust
use object_lang::{Architecture, elf};

assert_eq!(elf::code_address(Architecture::X86_64), 0x40_1000);
assert_eq!(elf::code_address(Architecture::Aarch64), 0x41_0000);
```

### ELF layout

**Relocatable objects.** Section header `i + 1` is the model's section `i`. After the
content sections come one `.rela<name>` (`SHT_RELA`, `SHF_INFO_LINK`, entry size 24) per
section with relocations, an empty `.note.GNU-stack` (`SHF_EXECINSTR` when
[`executable_stack`](#object) is set), `.symtab`, `.symtab_shndx` when a symbol's section
index exceeds 16 bits, `.strtab`, and `.shstrtab`. File offsets of contents are aligned to
the section's alignment, capped at 4 KiB.

The symbol table holds the null symbol, then one local `STT_SECTION` symbol for each
section a relocation targets directly (in section order), then the object's local
symbols, then the rest, each group in model order; `sh_info` is the first non-local
index. Names share bytes when one is a suffix of another. More than 65,279 sections use
ELF's extended numbering: `e_shnum = 0` and `e_shstrndx = SHN_XINDEX`, with the real values
in section header 0, and `SHN_XINDEX` symbols resolved through `.symtab_shndx`.

**Executables.** The loadable sections, sorted by address, are grouped into `PT_LOAD`
segments: a new segment starts where permissions change (`R`, `RW`, `RX`, `RWX`), where
initialized data follows BSS, or where a gap of a page or more opens. A new segment must
start on a page the previous one does not touch, or the write fails with
[`SegmentsSharePage`](#writeerror). Segments are aligned to the
[page size](#architecture) with file offsets congruent to their addresses. The ELF header
and program headers are mapped read-only on the page(s) directly below the lowest
section (so `AT_PHDR` is valid). A `PT_GNU_STACK` header (`RW`, or `RWX` with
`executable_stack`) follows. There is no `PT_INTERP` and no `PT_DYNAMIC`. Section
headers and the symbol table are written as for relocatable objects, so tools can name
what they show.

### What the reader accepts

ELF64, little-endian, `EV_CURRENT`, OS ABI `NONE` or `GNU`, ABI version 0, `e_flags`
0, type `ET_REL` or `ET_EXEC`, machine `EM_X86_64` or `EM_AARCH64`, with a section
header table. Section types `PROGBITS`, `NOBITS`, `NOTE`, `INIT_ARRAY`, `FINI_ARRAY`,
`PREINIT_ARRAY`, `SYMTAB` (one), `STRTAB` (each referenced), `RELA` (relocatable only),
`SYMTAB_SHNDX`, and on x86-64 `X86_64_UNWIND` (read as read-only data). Section flags
`WRITE`, `ALLOC`, `EXECINSTR`, `MERGE`, `STRINGS`, `TLS`. Symbol types `NOTYPE`,
`OBJECT`, `FUNC`, `SECTION`, `FILE`, `TLS`; bindings `LOCAL`, `GLOBAL`, `WEAK`;
visibilities `DEFAULT`, `HIDDEN`, `PROTECTED`. Relocation types: exactly the
[x86-64](#x86_64reloc) and [AArch64](#aarch64reloc) kinds. Program header types
`PT_NULL`, `PT_LOAD`, `PT_GNU_STACK`, and `PT_PHDR` (ignored).

Dropped by rule, because the model derives them: `STT_SECTION` symbols (relocations
against them become [`RelocationTarget::Section`](#relocationtarget)),
`.note.GNU-stack` in relocatable objects (it becomes `executable_stack`; when absent,
`executable_stack` is `true`, as GNU ld assumes), `PT_GNU_STACK` (likewise for
executables), `PT_PHDR`, and LLVM's `.llvm_addrsig` hint.

Checked for executables: every loadable section lies inside one `PT_LOAD` whose
permissions cover its flags, at the file offset the segment implies; segments are sorted,
do not overlap, and have offsets congruent to addresses; the entry point is in an
executable segment.

```rust
use object_lang::{Architecture, Object, ReadError};

let mut bytes = object_lang::elf::write(&Object::relocatable(Architecture::X86_64))?;
bytes[0x12] = 40; // e_machine = EM_ARM
assert_eq!(
    object_lang::elf::read(&bytes),
    Err(ReadError::Unsupported { what: "machine", value: 40 }),
);
# Ok::<(), object_lang::WriteError>(())
```

## `Object`

```rust,ignore
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Object { /* private */ }
```

One object file or executable: a [`FileKind`](#filekind), an
[`Architecture`](#architecture), [`Section`](#section)s, [`Symbol`](#symbol)s, an entry
point, and whether the program asks for an executable stack.

| Method | Description |
|---|---|
| `new(kind, arch)` | An empty object; entry 0, stack not executable. |
| `relocatable(arch)`, `executable(arch)` | Shorthands for `new`. |
| `kind()`, `architecture()` | What it is and what it targets. |
| `entry()`, `set_entry(address)` | The entry point. |
| `executable_stack()`, `set_executable_stack(bool)` | Whether the program asks for an executable stack (default `false`). |
| `add_section(section) -> SectionId` | Appends a section. |
| `sections() -> &[Section]` | All sections; a `SectionId` indexes this slice. |
| `section(id)`, `section_mut(id)` | One section, or `None`. |
| `section_by_name(name)` | The first section with this name (linear search). |
| `add_symbol(symbol) -> SymbolId` | Appends a symbol. |
| `symbols() -> &[Symbol]` | All symbols; a `SymbolId` indexes this slice. |
| `symbol(id)`, `symbol_mut(id)` | One symbol, or `None`. |
| `symbol_by_name(name)` | The first symbol with this name (linear search). |
| `add_relocation(section, relocation)` | Adds a relocation to a section; `Err(ModelError::UnknownSection)` if there is no such section. |
| `validate()` | Checks every rule; see below. |

```rust
use object_lang::{Architecture, Binding, Object, Section, SectionKind, Symbol};

let mut obj = Object::relocatable(Architecture::X86_64);
let data = obj.add_section(Section::new(".data", SectionKind::Data).with_data(vec![0; 8]));
let counter = obj.add_symbol(Symbol::data("counter", data, 0, 8));
if let Some(sym) = obj.symbol_mut(counter) {
    sym.binding = Binding::Weak;
}
assert_eq!(obj.symbol_by_name("counter"), Some(counter));
assert_eq!(obj.section(data).map(|s| s.size()), Some(8));
assert!(!obj.executable_stack());
```

### `Object::validate`

```rust,ignore
pub fn validate(&self) -> Result<(), ModelError>
```

Checks every rule of the model and returns the first violation. The rules are the
problems listed under [`SectionProblem`](#sectionproblem),
[`SymbolProblem`](#symbolproblem), and [`RelocationProblem`](#relocationproblem), plus
two for executables (loadable sections may not overlap; the entry point must be inside an
executable section) and a size limit (at most 2³¹ − 1 sections and symbols). Cost: one
pass, plus a sort of the loadable sections for executables.

```rust
use object_lang::{Architecture, ModelError, Object, Section, SectionKind};

let mut exe = Object::executable(Architecture::X86_64);
exe.add_section(Section::new(".text", SectionKind::Text).with_address(0x40_1000).with_data(vec![0xc3]));
exe.set_entry(0x40_2000);
assert_eq!(exe.validate(), Err(ModelError::EntryNotExecutable { entry: 0x40_2000 }));
exe.set_entry(0x40_1000);
assert!(exe.validate().is_ok());
```

## `Architecture`

```rust,ignore
#[non_exhaustive]
pub enum Architecture { X86_64, Aarch64 }
```

The instruction set. `page_size()` is the page size executables are laid out with:
4 KiB on x86-64, 64 KiB on AArch64 (correct for 4, 16, and 64 KiB kernels, and what GNU
ld and LLVM lld default to). A linker placing sections for an executable must start each
change of permissions on a fresh page of this size.

```rust
use object_lang::Architecture;

assert_eq!(Architecture::X86_64.page_size(), 4096);
assert_eq!(Architecture::Aarch64.page_size(), 65536);
```

## `FileKind`

```rust,ignore
#[non_exhaustive]
pub enum FileKind { Relocatable, Executable }
```

What a file is for. See [Relocatable objects and executables](#relocatable-objects-and-executables).

```rust
use object_lang::{Architecture, FileKind, Object};

assert_eq!(Object::executable(Architecture::X86_64).kind(), FileKind::Executable);
```

## `SectionId`, `SymbolId`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct SectionId(/* private */);
pub struct SymbolId(/* private */);
```

Dense ids: a section's or symbol's position in [`Object::sections`](#object) or
[`Object::symbols`](#object). `index()` gives the position; `new(index)` makes an id from
one. An id that names nothing in the object it is used with is caught by
[`validate`](#objectvalidate) (or refused by `section`/`symbol`), never trusted.

```rust
use object_lang::{Architecture, Object, Section, SectionId, SectionKind, Symbol, SymbolId};

let mut obj = Object::relocatable(Architecture::X86_64);
let text = obj.add_section(Section::new(".text", SectionKind::Text));
let f = obj.add_symbol(Symbol::undefined("f"));
assert_eq!((text, f), (SectionId::new(0), SymbolId::new(0)));
assert_eq!(text.index(), 0);
```

## `Section`

```rust,ignore
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section { /* private */ }
```

| Method | Description |
|---|---|
| `new(name, kind)` | An empty section with the kind's [default flags](#sectionkind), alignment 1, address 0. |
| `with_flags(flags)` | Replaces the flags. |
| `with_align(align)` | Sets the alignment (a power of two, checked by `validate`). |
| `with_entry_size(size)` | Size of fixed-size entries; required (non-zero) with `MERGE`. |
| `with_address(address)` | Sets the address (an executable's load address; must be a multiple of the alignment). |
| `with_data(bytes)` | Replaces the contents (not for BSS). |
| `with_bss_size(size)` | Sets a BSS section's size (only for BSS). |
| `append(bytes, align) -> Result<u64, ModelError>` | Pads to `align`, appends, returns the offset; raises the section's alignment. Errors: `InvalidAlignment`, `AppendToBss`, `SizeOverflow`. |
| `reserve(size, align) -> Result<u64, ModelError>` | Reserves `size` zero bytes at `align` (grows a BSS size, or appends zeros); returns the offset. Errors: `InvalidAlignment`, `SizeOverflow`. Nothing changes on error. |
| `add_relocation(relocation)` | Adds a relocation that patches this section. |
| `name()`, `kind()`, `flags()`, `align()`, `entry_size()`, `address()` | Accessors. |
| `data() -> &[u8]` | The bytes (empty for BSS). |
| `size() -> u64` | The size in memory: the data length, or the BSS size. |
| `relocations() -> &[Relocation]` | In the order added. |

```rust
use object_lang::{ModelError, Section, SectionKind};

let mut text = Section::new(".text", SectionKind::Text);
assert_eq!(text.append(&[0x90], 1)?, 0);
assert_eq!(text.append(&[0xc3], 16)?, 16); // padded to the requested alignment
assert_eq!((text.size(), text.align()), (17, 16));

let mut bss = Section::new(".bss", SectionKind::Bss);
assert_eq!(bss.reserve(100, 8)?, 0);
assert_eq!(bss.reserve(8, 64)?, 128);
assert_eq!(bss.size(), 136);
assert_eq!(bss.append(&[1], 1), Err(ModelError::AppendToBss));
# Ok::<(), ModelError>(())
```

## `SectionKind`

```rust,ignore
#[non_exhaustive]
pub enum SectionKind {
    Text, Data, ReadOnlyData, Bss, Note, InitArray, FiniArray, PreinitArray, Other,
}
```

| Kind | Contents | ELF type | Required flags | Forbidden flags |
|---|---|---|---|---|
| `Text` | machine code | `PROGBITS` | `ALLOC`, `EXEC` | `TLS` |
| `Data` | writable data (`TLS`: `.tdata`) | `PROGBITS` | `ALLOC`, `WRITE` | `EXEC` |
| `ReadOnlyData` | constants | `PROGBITS` | `ALLOC` | `WRITE`, `EXEC`, `TLS` |
| `Bss` | zero-filled, no file bytes (`TLS`: `.tbss`) | `NOBITS` | `ALLOC` | `EXEC`, `MERGE`, `STRINGS` |
| `Note` | notes | `NOTE` | | `WRITE`, `EXEC`, `TLS`, `MERGE`, `STRINGS` |
| `InitArray`, `FiniArray`, `PreinitArray` | constructor/destructor pointers | `INIT_ARRAY`, `FINI_ARRAY`, `PREINIT_ARRAY` | `ALLOC`, `WRITE` | `EXEC`, `TLS`, `MERGE`, `STRINGS` |
| `Other` | not loaded (comments, metadata, debug info) | `PROGBITS` | | `ALLOC`, `WRITE`, `EXEC`, `TLS` |

`default_flags()` returns the required flags; `allows(flags)` applies the table;
`is_uninitialized()` is true for `Bss`.

```rust
use object_lang::{SectionFlags, SectionKind};

assert_eq!(SectionKind::Bss.default_flags(), SectionFlags::ALLOC | SectionFlags::WRITE);
assert!(SectionKind::Bss.allows(SectionFlags::ALLOC | SectionFlags::WRITE | SectionFlags::TLS));
assert!(!SectionKind::Text.allows(SectionFlags::ALLOC));
```

## `SectionFlags`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SectionFlags(/* private */);
```

A bit set: `ALLOC`, `WRITE`, `EXEC`, `TLS`, `MERGE`, `STRINGS`. Combine with `|` (and
`|=`); query with `contains`, `is_empty`; build with `empty`, `union`, `difference`,
`from_bits` (`None` for unknown bits); `bits` gives the raw value. `Debug` prints the
flag names.

```rust
use object_lang::SectionFlags;

let mut flags = SectionFlags::ALLOC;
flags |= SectionFlags::WRITE;
assert!(flags.contains(SectionFlags::ALLOC | SectionFlags::WRITE));
assert_eq!(flags.difference(SectionFlags::WRITE), SectionFlags::ALLOC);
assert_eq!(format!("{flags:?}"), "SectionFlags(ALLOC | WRITE)");
assert_eq!(SectionFlags::from_bits(0xff), None);
```

## `Symbol`

```rust,ignore
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub binding: Binding,
    pub visibility: Visibility,
    pub section: SymbolSection,
    pub value: u64,
    pub size: u64,
}
```

The fields are public; build with a constructor, then adjust fields or chain `with_*`.

| Constructor | Kind | Binding | Section | Value |
|---|---|---|---|---|
| `new(name, kind, binding, section, value, size)` | as given | as given | as given | as given |
| `function(name, section, value, size)` | `Function` | `Global` | the section | offset/address |
| `data(name, section, value, size)` | `Data` | `Global` | the section | offset/address |
| `undefined(name)` | `NoType` | `Global` | `Undefined` | 0 |
| `absolute(name, value)` | `NoType` | `Global` | `Absolute` | the value |
| `common(name, size, align)` | `Data` | `Global` | `Common` | the alignment |
| `file(name)` | `File` | `Local` | `Absolute` | 0 |

`with_binding`, `with_visibility`, and `with_kind` return a changed copy. Visibility is
`Default` unless set.

```rust
use object_lang::{Binding, Symbol, SymbolKind, SymbolSection, Visibility};

let s = Symbol::undefined("hook").with_binding(Binding::Weak).with_kind(SymbolKind::Function);
assert_eq!((s.binding, s.kind, s.section), (Binding::Weak, SymbolKind::Function, SymbolSection::Undefined));
let hidden = Symbol::absolute("PAGE", 4096).with_visibility(Visibility::Hidden);
assert_eq!(hidden.value, 4096);
```

## `SymbolKind`

```rust,ignore
#[non_exhaustive]
pub enum SymbolKind { NoType, Function, Data, Tls, File }
```

`NoType` (a label, or an unknown reference), `Function`, `Data`, `Tls` (a thread-local
variable; if defined, in a `TLS` section), `File` (the source file name; local, absolute,
value and size zero).

```rust
use object_lang::{Symbol, SymbolKind};

assert_eq!(Symbol::file("lib.rs").kind, SymbolKind::File);
```

## `Binding`

```rust,ignore
#[non_exhaustive]
pub enum Binding { Local, Global, Weak }
```

`Local` symbols are visible only inside the object and must be defined. `Global` symbols
are visible to the whole link. `Weak` symbols yield to a global definition, and an
undefined weak reference resolves to zero.

```rust
use object_lang::{Binding, Symbol};

assert_eq!(Symbol::undefined("x").with_binding(Binding::Weak).binding, Binding::Weak);
```

## `Visibility`

```rust,ignore
#[non_exhaustive]
pub enum Visibility { Default, Hidden, Protected }
```

How far a non-local symbol is exported beyond the module it is linked into: `Default`
follows the binding, `Hidden` is not exported, `Protected` is exported but always binds
locally.

```rust
use object_lang::{Symbol, Visibility};

assert_eq!(Symbol::undefined("x").visibility, Visibility::Default);
```

## `SymbolSection`

```rust,ignore
#[non_exhaustive]
pub enum SymbolSection { Undefined, Absolute, Common, Section(SectionId) }
```

Where a symbol is defined: nowhere (another object defines it), as a constant, as a
common block (relocatable only; the value is the alignment, a power of two; the binding
must be global), or in a section of this object.

```rust
use object_lang::{Symbol, SymbolSection};

assert_eq!(Symbol::common("buf", 64, 8).section, SymbolSection::Common);
```

## `Relocation`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub struct Relocation {
    pub offset: u64,
    pub kind: RelocationKind,
    pub target: RelocationTarget,
    pub addend: i64,
}
```

`Relocation::new(offset, kind, target, addend)` takes the kind as anything convertible to
[`RelocationKind`](#relocationkind) (an [`X86_64Reloc`](#x86_64reloc) or
[`Aarch64Reloc`](#aarch64reloc)). The patched bytes, `offset .. offset + kind.size()`,
must lie inside the section.

```rust
use object_lang::{Relocation, RelocationTarget, SymbolId, X86_64Reloc};

let r = Relocation::new(16, X86_64Reloc::Abs64, RelocationTarget::Symbol(SymbolId::new(0)), 8);
assert_eq!((r.offset, r.kind.size(), r.addend), (16, 8, 8));
```

## `RelocationKind`

```rust,ignore
#[non_exhaustive]
pub enum RelocationKind { X86_64(X86_64Reloc), Aarch64(Aarch64Reloc) }
```

`architecture()` names the architecture the kind belongs to; `size()` the number of bytes
patched (8 for the 64-bit data kinds, otherwise 4); `is_instruction()` whether it patches
an instruction field (AArch64 instruction relocations; their offset must be a multiple of
4). `From<X86_64Reloc>` and `From<Aarch64Reloc>` are implemented.

```rust
use object_lang::{Aarch64Reloc, Architecture, RelocationKind};

let k = RelocationKind::from(Aarch64Reloc::Prel64);
assert_eq!((k.architecture(), k.size(), k.is_instruction()), (Architecture::Aarch64, 8, false));
```

## `X86_64Reloc`

```rust,ignore
#[non_exhaustive]
pub enum X86_64Reloc { Abs64, Abs32, Abs32Signed, Pc32, Pc64, Plt32, GotPcRel, GotPcRelX, RexGotPcRelX }
```

`S` is the target address, `A` the addend, `P` the patched address, `L` the PLT entry (the
target itself in a static link), `G` the GOT entry.

| Variant | ELF | Value | Bytes |
|---|---|---|---:|
| `Abs64` | `R_X86_64_64` (1) | `S + A` | 8 |
| `Pc32` | `R_X86_64_PC32` (2) | `S + A - P` | 4 |
| `Plt32` | `R_X86_64_PLT32` (4) | `L + A - P` | 4 |
| `GotPcRel` | `R_X86_64_GOTPCREL` (9) | `G + A - P` | 4 |
| `Abs32` | `R_X86_64_32` (10) | `S + A`, zero-extended | 4 |
| `Abs32Signed` | `R_X86_64_32S` (11) | `S + A`, sign-extended | 4 |
| `Pc64` | `R_X86_64_PC64` (24) | `S + A - P` | 8 |
| `GotPcRelX` | `R_X86_64_GOTPCRELX` (41) | `G + A - P`, relaxable | 4 |
| `RexGotPcRelX` | `R_X86_64_REX_GOTPCRELX` (42) | `G + A - P`, relaxable, REX prefix | 4 |

```rust
use object_lang::{RelocationKind, X86_64Reloc};

assert_eq!(RelocationKind::from(X86_64Reloc::Pc64).size(), 8);
```

## `Aarch64Reloc`

```rust,ignore
#[non_exhaustive]
pub enum Aarch64Reloc {
    Abs64, Abs32, Prel64, Prel32, Call26, Jump26, CondBr19, TstBr14, AdrPrelLo21,
    AdrPrelPgHi21, AddAbsLo12Nc, Ldst8AbsLo12Nc, Ldst16AbsLo12Nc, Ldst32AbsLo12Nc,
    Ldst64AbsLo12Nc, Ldst128AbsLo12Nc, AdrGotPage, Ld64GotLo12Nc,
}
```

`Page(x)` is `x` with its low 12 bits cleared.

| Variant | ELF | Value | Patches |
|---|---|---|---|
| `Abs64` | `R_AARCH64_ABS64` (257) | `S + A` | 8 data bytes |
| `Abs32` | `R_AARCH64_ABS32` (258) | `S + A` | 4 data bytes |
| `Prel64` | `R_AARCH64_PREL64` (260) | `S + A - P` | 8 data bytes |
| `Prel32` | `R_AARCH64_PREL32` (261) | `S + A - P` | 4 data bytes |
| `AdrPrelLo21` | `R_AARCH64_ADR_PREL_LO21` (274) | `S + A - P` | `ADR` |
| `AdrPrelPgHi21` | `R_AARCH64_ADR_PREL_PG_HI21` (275) | `Page(S + A) - Page(P)` | `ADRP` |
| `AddAbsLo12Nc` | `R_AARCH64_ADD_ABS_LO12_NC` (277) | `S + A`, bits 11:0 | `ADD` |
| `Ldst8AbsLo12Nc` | `R_AARCH64_LDST8_ABS_LO12_NC` (278) | `S + A`, bits 11:0 | `LDRB`/`STRB` |
| `TstBr14` | `R_AARCH64_TSTBR14` (279) | `S + A - P` | `TBZ`/`TBNZ` |
| `CondBr19` | `R_AARCH64_CONDBR19` (280) | `S + A - P` | `B.cond`/`CBZ`/`CBNZ` |
| `Jump26` | `R_AARCH64_JUMP26` (282) | `S + A - P` | `B` |
| `Call26` | `R_AARCH64_CALL26` (283) | `S + A - P` | `BL` |
| `Ldst16AbsLo12Nc` | `R_AARCH64_LDST16_ABS_LO12_NC` (284) | `S + A`, bits 11:1 | 16-bit `LDR`/`STR` |
| `Ldst32AbsLo12Nc` | `R_AARCH64_LDST32_ABS_LO12_NC` (285) | `S + A`, bits 11:2 | 32-bit `LDR`/`STR` |
| `Ldst64AbsLo12Nc` | `R_AARCH64_LDST64_ABS_LO12_NC` (286) | `S + A`, bits 11:3 | 64-bit `LDR`/`STR` |
| `Ldst128AbsLo12Nc` | `R_AARCH64_LDST128_ABS_LO12_NC` (299) | `S + A`, bits 11:4 | 128-bit `LDR`/`STR` |
| `AdrGotPage` | `R_AARCH64_ADR_GOT_PAGE` (311) | `Page(G) - Page(P)` | `ADRP` |
| `Ld64GotLo12Nc` | `R_AARCH64_LD64_GOT_LO12_NC` (312) | `G`, bits 11:3 | `LDR` |

```rust
use object_lang::{Aarch64Reloc, RelocationKind};

assert!(RelocationKind::from(Aarch64Reloc::Ldst64AbsLo12Nc).is_instruction());
```

## `RelocationTarget`

```rust,ignore
#[non_exhaustive]
pub enum RelocationTarget { Symbol(SymbolId), Section(SectionId) }
```

Whose address a relocation uses: a symbol's, or a section's start (the addend picks the
place inside it, which is how compilers refer to unnamed data such as string literals).
ELF writes a section target as a relocation against a local `STT_SECTION` symbol, which
the reader turns back into a section target.

```rust
use object_lang::{RelocationTarget, SectionId};

let start_of_rodata = RelocationTarget::Section(SectionId::new(2));
assert!(matches!(start_of_rodata, RelocationTarget::Section(_)));
```

## `Limits`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub struct Limits {
    pub max_sections: u32,
    pub max_symbols: u32,
    pub max_relocations: u64,
    pub max_name_len: u32,
    pub max_name_bytes: u64,
}
```

Budgets for reading untrusted files; exceeding one is
[`ReadError::LimitExceeded`](#readerror) naming the field. Start from
`Limits::default()` (or `Limits::DEFAULT`) and change fields.

| Field | Default | Bounds |
|---|---:|---|
| `max_sections` | 1,048,576 | section headers |
| `max_symbols` | 16,777,216 | symbol table entries |
| `max_relocations` | 67,108,864 | relocation records, all sections |
| `max_name_len` | 65,536 | bytes in one name |
| `max_name_bytes` | 268,435,456 (256 MiB) | name bytes copied out, all names |

Independently of these, the reader never copies more section data than the input holds,
however the section headers overlap (`LimitExceeded { limit: "section data exceeds the
input size" }`), and checks every count against the input length before allocating.

```rust
use object_lang::Limits;

let mut strict = Limits::default();
strict.max_name_len = 256;
assert_ne!(strict, Limits::DEFAULT);
```

## `ModelError`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum ModelError { /* variants below */ }
impl Display for ModelError {}
impl core::error::Error for ModelError {}
```

A rule an object breaks. Match with a wildcard arm.

| Variant | Meaning | Message |
|---|---|---|
| `Section { section, problem }` | A [section rule](#sectionproblem). | `section #2: alignment is not a power of two` |
| `Symbol { symbol, problem }` | A [symbol rule](#symbolproblem). | `symbol #0: a local symbol must be defined` |
| `Relocation { section, index, problem }` | A [relocation rule](#relocationproblem). | `relocation #1 of section #0: patches bytes outside its section` |
| `SectionsOverlap { first, second }` | Two loadable sections of an executable overlap. | `sections #0 and #1 overlap in memory` |
| `EntryNotExecutable { entry }` | An executable's entry is not in an executable section. | `entry point 0x401000 is not inside an executable section` |
| `UnknownSection { section }` | `Object::add_relocation` with a section the object lacks. | `the object has no section #3` |
| `InvalidAlignment { align }` | `Section::append`/`reserve` with a bad alignment. | `alignment 3 is not a power of two` |
| `AppendToBss` | `Section::append` on BSS. | `cannot append bytes to a BSS section; reserve space instead` |
| `SizeOverflow` | A section would outgrow the size range or memory. | `section size overflows` |
| `TooManyItems` | More than 2³¹ − 1 sections or symbols. | `too many sections or symbols to number` |

```rust
use object_lang::{Architecture, ModelError, Object, Section, SectionKind, SectionProblem};

let mut obj = Object::relocatable(Architecture::X86_64);
let s = obj.add_section(Section::new("bad\0name", SectionKind::Data));
let err = obj.validate().unwrap_err();
assert_eq!(err, ModelError::Section { section: s, problem: SectionProblem::NameContainsNul });
assert_eq!(err.to_string(), "section #0: name contains a NUL byte");
```

## `SectionProblem`

`#[non_exhaustive]`. Each variant is a rule every section must keep.

| Variant | Rule |
|---|---|
| `NameContainsNul` | The name has no NUL byte. |
| `BadAlignment` | The alignment is a non-zero power of two. |
| `FlagsDoNotMatchKind` | The flags are [allowed for the kind](#sectionkind). |
| `MergeWithoutEntrySize` | `MERGE` needs a non-zero entry size. |
| `ContentsDoNotMatchKind` | BSS has no bytes; other kinds have no BSS size. |
| `MisalignedAddress` | The address is a multiple of the alignment. |
| `AddressOverflow` | Address plus size fits in 64 bits. |

```rust
use object_lang::{Architecture, ModelError, Object, Section, SectionKind, SectionProblem};

let mut obj = Object::relocatable(Architecture::X86_64);
obj.add_section(Section::new(".bss", SectionKind::Bss).with_data(vec![1]));
assert!(matches!(
    obj.validate(),
    Err(ModelError::Section { problem: SectionProblem::ContentsDoNotMatchKind, .. })
));
```

## `SymbolProblem`

`#[non_exhaustive]`. Each variant is a rule every symbol must keep.

| Variant | Rule |
|---|---|
| `NameContainsNul` | The name has no NUL byte. |
| `UnknownSection` | A section-defined symbol's section exists. |
| `OutOfSection` | `value .. value + size` lies within the section (offsets in a relocatable object, addresses in an executable). |
| `LocalUndefined` | Local symbols are defined (not undefined, not common). |
| `BadFileSymbol` | `File` symbols are local and absolute, with value and size zero. |
| `BadCommonSymbol` | Common symbols are global with a power-of-two alignment. |
| `CommonInExecutable` | Executables have no common symbols. |
| `UndefinedInExecutable` | Executables have no non-weak undefined symbols. |
| `TlsOutsideTlsSection` | A defined `Tls` symbol is in a `TLS` section. |

```rust
use object_lang::{Architecture, ModelError, Object, Symbol, SymbolProblem};

let mut exe = Object::executable(Architecture::X86_64);
exe.add_symbol(Symbol::undefined("printf"));
assert!(matches!(
    exe.validate(),
    Err(ModelError::Symbol { problem: SymbolProblem::UndefinedInExecutable, .. })
));
```

## `RelocationProblem`

`#[non_exhaustive]`. Each variant is a rule every relocation must keep.

| Variant | Rule |
|---|---|
| `WrongArchitecture` | The kind belongs to the object's architecture. |
| `OutOfSection` | The patched bytes lie inside the section. |
| `Misaligned` | Instruction relocations are 4-byte aligned. |
| `UnknownTarget` | The target symbol or section exists. |
| `InUninitializedSection` | BSS sections have no relocations. |
| `InExecutable` | Executables have no relocations. |

```rust
use object_lang::{
    Architecture, ModelError, Object, Relocation, RelocationProblem, RelocationTarget, Section,
    SectionKind, SymbolId, X86_64Reloc,
};

let mut obj = Object::relocatable(Architecture::X86_64);
let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0; 4]));
obj.add_relocation(text, Relocation::new(0, X86_64Reloc::Pc32, RelocationTarget::Symbol(SymbolId::new(9)), 0))?;
assert!(matches!(
    obj.validate(),
    Err(ModelError::Relocation { problem: RelocationProblem::UnknownTarget, .. })
));
# Ok::<(), ModelError>(())
```

## `WriteError`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum WriteError { /* variants below */ }
impl Display for WriteError {}
impl core::error::Error for WriteError {}  // source() is the ModelError for Invalid
impl From<ModelError> for WriteError {}
```

| Variant | Meaning |
|---|---|
| `Invalid(ModelError)` | The object breaks a model rule. |
| `ReservedSectionName { section }` | A relocatable object has a section named `.note.GNU-stack`, which the writer emits itself. |
| `SegmentsSharePage { first, second }` | Two loadable sections of an executable need different segments (different permissions, or data after BSS) but share a page. |
| `NoRoomForHeaders` | The lowest section of an executable is too close to address zero to map the headers below it. |
| `Unsupported { what }` | Something this writer does not support yet (thread-local sections in executables; more than 65,534 program headers). |
| `TooLarge` | The file would exceed ELF's 32-bit table fields or the address space. |

```rust
use object_lang::{Architecture, Object, Section, SectionId, SectionKind, WriteError};

let mut exe = Object::executable(Architecture::X86_64);
exe.add_section(Section::new(".text", SectionKind::Text).with_address(0x40_1000).with_data(vec![0xc3]));
exe.add_section(Section::new(".data", SectionKind::Data).with_address(0x40_1100).with_data(vec![0]));
exe.set_entry(0x40_1000);
assert_eq!(
    object_lang::elf::write(&exe),
    Err(WriteError::SegmentsSharePage { first: SectionId::new(0), second: SectionId::new(1) }),
);
```

## `ReadError`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum ReadError { /* variants below */ }
impl Display for ReadError {}
impl core::error::Error for ReadError {}  // source() is the ModelError for Invalid
impl From<ModelError> for ReadError {}
```

| Variant | Meaning |
|---|---|
| `BadMagic` | Not ELF. |
| `Truncated { what }` | A structure runs past the end of the input. |
| `Malformed { what }` | A field breaks the format, or structures contradict each other. |
| `Unsupported { what, value }` | Valid ELF the model cannot represent; `value` is the raw field (type, flags, class). |
| `LimitExceeded { limit }` | A [`Limits`](#limits) budget (named by its field), or section data larger than the input. |
| `Invalid(ModelError)` | The decoded object breaks a model rule. |

```rust
use object_lang::{Architecture, Object, ReadError};

let bytes = object_lang::elf::write(&Object::relocatable(Architecture::X86_64))?;
let mut big_endian = bytes.clone();
big_endian[5] = 2;
assert!(matches!(object_lang::elf::read(&big_endian), Err(ReadError::Unsupported { value: 2, .. })));
assert_eq!(object_lang::elf::read(&bytes[..100]), Err(ReadError::Truncated { what: "section header table" }));
# Ok::<(), object_lang::WriteError>(())
```

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `std` | yes | Nothing beyond `alloc` is used. Without it the crate is `no_std`; the API is identical. |

## Not supported yet

Each of these is scheduled in [`../dev/ROADMAP.md`](../dev/ROADMAP.md); until then the
writer or reader refuses it with an error naming it.

- **Thread-local storage relocations** (`R_X86_64_TPOFF32`, `R_AARCH64_TLSLE_*`, …) and
  `PT_TLS` in executables. TLS *sections* and *symbols* in relocatable objects are
  supported.
- **COMDAT groups** (`SHT_GROUP`), `SHF_LINK_ORDER`, `SHF_GNU_RETAIN`, compressed
  sections, and IFUNC / `STB_GNU_UNIQUE` symbols.
- **Dynamic linking**: shared objects, `PT_INTERP`, `PT_DYNAMIC`, PIE.
- **Other formats**: COFF/PE and Mach-O (v0.5), with import/export tables.
- **Debug information** and unwind tables (DWARF, `.eh_frame` generation), in
  `debuginfo`-tier work; existing `.eh_frame` and `.debug_*` sections are carried as
  plain data.
