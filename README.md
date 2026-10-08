<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>object-lang</b>
    <br>
    <sub><sup>OBJECT FILES AND EXECUTABLES</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/object-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/object-lang"></a>
    <a href="https://crates.io/crates/object-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/object-lang?color=%230099ff"></a>
    <a href="https://docs.rs/object-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/object-lang"></a>
    <a href="https://github.com/jamesgober/object-lang/actions"><img alt="CI" src="https://github.com/jamesgober/object-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>object-lang</strong> writes and reads the files operating systems run. Describe an object &mdash; sections, symbols, relocations, and for a program an entry point &mdash; and get ELF64 bytes for x86-64 or AArch64: a relocatable object any ELF linker accepts, or a static executable the Linux kernel loads directly. No system linker, no assembler, no C toolchain, and no third-party crate is involved.
    </p>
    <p>
        The model is format-neutral: section kinds, symbol bindings, and relocation kinds are named for what they mean, not for one format's numbering, so the COFF/PE and Mach-O writers planned for v0.5 take the same objects. Reading is the inverse: ELF bytes come back as the same model, checked strictly and within explicit budgets, and the reader never panics on any input. It is the object-file layer of the <code>-lang</code> family's native compiler backend.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition). <code>no_std</code>-compatible (needs only <code>alloc</code>), <code>#![forbid(unsafe_code)]</code>, no dependencies.
    </p>
    <blockquote>
        <strong>Status: v0.2.0, pre-1.0.</strong> ELF64 is complete for what is listed below; COFF/PE, Mach-O, thread-local storage, and debug information come in later 0.x releases (see <a href="./dev/ROADMAP.md"><code>dev/ROADMAP.md</code></a>). The API may change before <code>1.0</code>, which waits until a real consumer has driven it end to end. See <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## The model

- An **[`Object`](./docs/API.md#object)** is a [`FileKind`](./docs/API.md#filekind) (relocatable or executable), an [`Architecture`](./docs/API.md#architecture), sections, symbols, and an entry point. [`validate`](./docs/API.md#objectvalidate) checks every rule; writers run it first, and readers run it on what they read.
- A **[`Section`](./docs/API.md#section)** has a [`SectionKind`](./docs/API.md#sectionkind) (text, data, read-only data, BSS, notes, init/fini arrays, or unloaded metadata), [`SectionFlags`](./docs/API.md#sectionflags), an alignment, an address, its bytes (or, for BSS, only a size), and the relocations that patch it.
- A **[`Symbol`](./docs/API.md#symbol)** has a name, a [kind](./docs/API.md#symbolkind), a [binding](./docs/API.md#binding) (local, global, weak), a [visibility](./docs/API.md#visibility), where it is defined ([`SymbolSection`](./docs/API.md#symbolsection): a section, undefined, absolute, or common), a value, and a size.
- A **[`Relocation`](./docs/API.md#relocation)** patches bytes at an offset with a [`RelocationKind`](./docs/API.md#relocationkind) &mdash; [`X86_64Reloc`](./docs/API.md#x86_64reloc) or [`Aarch64Reloc`](./docs/API.md#aarch64reloc) &mdash; computed from a [target](./docs/API.md#relocationtarget) (a symbol, or a section) and an explicit addend.
- **[`elf`](./docs/API.md#the-elf-module)** writes and reads all of it. Errors are values: [`ModelError`](./docs/API.md#modelerror), [`WriteError`](./docs/API.md#writeerror), [`ReadError`](./docs/API.md#readerror); budgets are [`Limits`](./docs/API.md#limits).

<br>

What it guarantees, and how each guarantee is checked:

| Guarantee | How it is held |
|---|---|
| Every object the writer accepts reads back as the same object. | Property tests over random relocatable objects and executables (every section kind, flag combination, symbol shape, and relocation kind), 1,024 cases each per run; one-off runs of 400,000 cases each found no failure. |
| The bytes are what the ELF specification says. | An independent decoder written in the tests from the specification checks every header, section, symbol, and relocation against the model; known-answer tests pin header fields and whole-file layouts byte for byte. |
| Relocations mean what the psABIs say. | Objects using every relocation kind are linked by LLVM's `ld.lld` for both architectures and the patched bytes are checked against each kind's formula (`tests/llvm_oracle.rs`, run with `--ignored` when LLVM is installed, and in CI). |
| Executables load as modelled. | A page-level simulation of the Linux loader maps every `PT_LOAD`: each section's bytes appear at its address with its permissions, BSS reads as zeros, no page is mapped twice. On Linux the test suite runs the programs it writes: hand-assembled `exit(42)`, and a four-segment program that writes to stdout and uses `.data` and `.bss`. |
| The reader reads real compiler output. | Objects compiled by clang 18 for both architectures are read, checked symbol by symbol and relocation by relocation, and rewritten without change; `ld.lld`'s static executables are read too. |
| The reader never panics and stays within budget. | Property tests feed it arbitrary bytes and mutated and truncated valid files; every single-byte corruption of a sample object is tried; whatever is accepted must write back and read back unchanged. Named budgets bound sections, symbols, relocations, and name bytes, and copied section data never exceeds the input size. |
| Output is deterministic. | Writing is byte-identical across calls and across a read/write cycle, checked on every property-test case. |

<hr>
<br>

## Installation

```toml
[dependencies]
object-lang = "0.2"
```

Without the standard library:

```toml
[dependencies]
object-lang = { version = "0.2", default-features = false }
```

<hr>
<br>

## Quick start

A program the Linux kernel runs, from hand-assembled x86-64 bytes:

```rust
use object_lang::Architecture;

let code = [
    0xbf, 0x2a, 0x00, 0x00, 0x00, // mov edi, 42
    0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60 (exit)
    0x0f, 0x05, //                   syscall
];
let program = object_lang::elf::executable(Architecture::X86_64, &code)?;
// Write it to a file, `chmod +x`, run it: it exits with status 42.
assert_eq!(&program[..4], b"\x7fELF");
# Ok::<(), object_lang::WriteError>(())
```

### A relocatable object

Code that calls a function another object defines; the relocation tells the linker where to patch:

```rust
use object_lang::{
    Architecture, Object, Relocation, RelocationTarget, Section, SectionKind, Symbol,
    X86_64Reloc,
};

let mut obj = Object::relocatable(Architecture::X86_64);
// main: call helper; ret
let text = obj.add_section(
    Section::new(".text", SectionKind::Text)
        .with_align(16)
        .with_data(vec![0xe8, 0, 0, 0, 0, 0xc3]),
);
obj.add_symbol(Symbol::function("main", text, 0, 6));
let helper = obj.add_symbol(Symbol::undefined("helper"));
obj.add_relocation(
    text,
    Relocation::new(1, X86_64Reloc::Plt32, RelocationTarget::Symbol(helper), -4),
)?;

let bytes = object_lang::elf::write(&obj)?;
assert_eq!(object_lang::elf::read(&bytes)?, obj);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### An executable with data

Sections carry their final addresses; the writer groups them into segments by permission:

```rust
use object_lang::{Architecture, Object, Section, SectionKind, Symbol};

let mut exe = Object::executable(Architecture::Aarch64);
let text = exe.add_section(
    Section::new(".text", SectionKind::Text)
        .with_address(0x41_0000)
        .with_data(vec![0x40, 0x05, 0x80, 0xd2, 0xa8, 0x0b, 0x80, 0xd2, 0x01, 0x00, 0x00, 0xd4]),
);
exe.add_section(
    Section::new(".data", SectionKind::Data).with_address(0x42_0000).with_data(vec![1, 2, 3, 4]),
);
exe.add_section(Section::new(".bss", SectionKind::Bss).with_address(0x42_0004).with_bss_size(4096));
exe.add_symbol(Symbol::function("_start", text, 0x41_0000, 12));
exe.set_entry(0x41_0000);

let bytes = object_lang::elf::write(&exe)?;
assert_eq!(object_lang::elf::read(&bytes)?, exe);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Mistakes are values

```rust
use object_lang::{Architecture, ModelError, Object, Section, SectionKind, SectionProblem};

let mut obj = Object::relocatable(Architecture::X86_64);
let bad = obj.add_section(Section::new(".data", SectionKind::Data).with_align(12));
assert_eq!(
    object_lang::elf::write(&obj).unwrap_err().to_string(),
    "invalid object: section #0: alignment is not a power of two",
);
assert_eq!(
    obj.validate(),
    Err(ModelError::Section { section: bad, problem: SectionProblem::BadAlignment }),
);
```

<hr>
<br>

## Examples

Runnable programs in [`examples/`](./examples):

| Example | What it shows |
|---|---|
| [`exit42`](./examples/exit42.rs) | The one-call path: machine code to a runnable static executable, for x86-64 or AArch64. `cargo run --example exit42 -- exit42 aarch64` |
| [`relocatable`](./examples/relocatable.rs) | Building a relocatable object by hand: mergeable strings, a section-relative reference, and a call to an external function. |
| [`inspect`](./examples/inspect.rs) | A small `readelf`: what the reader makes of any ELF file. |

<hr>
<br>

## Performance

Writing is one forward pass: segment layout is computed first by arithmetic, the bytes are appended in file order into one buffer (reusable across calls with `write_into`), and only the few header fields that point at the section table are patched at the end. String tables share the bytes of names that are suffixes of other names, found by an MSD radix sort over reversed names, which is linear in the bytes it inspects. Reading checks each structure once and copies only what the model owns.

Measured with the benchmarks in [`benches/`](./benches), Windows x86_64, Rust stable, release profile. Linux numbers have not been measured yet.

| Benchmark | What it measures | Windows |
|---|---|---:|
| `relocatable/write` | 1,000 sections, 100,000 symbols, 200,000 relocations (12.5 MB of ELF) | ~24 ms (~520 MiB/s) |
| `relocatable/read` | The same file, read back into the model | ~26 ms (~480 MiB/s) |
| `relocatable/validate` | Checking every model rule on that object | ~1.9 ms |
| `executable/write` | 3,000 sections in 3 segments, 100,000 symbols | ~8.3 ms (~555 MiB/s) |
| `executable/read` | The same file, read back | ~8.8 ms (~520 MiB/s) |
| `tier1/executable_64k` | `elf::executable` on 64 KiB of code | ~6 µs |

```bash
cargo bench --bench bench
```

Criterion writes per-benchmark reports to `target/criterion/`. Numbers vary by CPU; use the trend across runs, not a single absolute.

<hr>
<br>

## Design notes

- **One model, many formats.** Section kinds say what a section holds, and the flags say how it is used; for code and data the kind is recoverable from the flags alone (`ALLOC` + `EXEC` is code, else `WRITE` is data, else read-only data), which is what lets a reader rebuild the exact model a writer was given. Relocation kinds are named for their computation (`Pc32`, `Call26`), not their ELF number, so COFF and Mach-O writers can map them to their own types.
- **The writer decides nothing the linker should.** Executables take their section addresses from the model: the writer only checks them (no overlap; each change of permissions on a fresh page) and derives the segments, the file offsets, and the header mapping from them.
- **Strict reading.** Anything the model cannot represent exactly &mdash; COMDAT groups, `SHT_REL`, dynamic sections, thread-local relocations, unknown section, symbol, or relocation types &mdash; is an error that names it, not a silent approximation. What carries no meaning in the model is dropped by rule: `STT_SECTION` symbols become section targets, and LLVM's `.llvm_addrsig` hint and the `PT_PHDR` header are derived data.
- **Budgets, not trust.** Counts are checked against the input length and the caller's [`Limits`](./docs/API.md#limits) before anything is allocated, offsets are added with overflow checks, slices are taken with `get`, nothing recurses, and the total of copied names is budgeted so many symbols cannot share one long name to multiply memory use.

<hr>
<br>

## Testing

The suite runs on Windows, Linux, and macOS through the CI matrix, on stable and the 1.85 MSRV:

```bash
cargo test                       # unit + integration + property + doctests
cargo test --no-default-features # no_std + alloc
cargo clippy --all-targets --all-features -- -D warnings
cargo bench --bench bench
# with LLVM installed (ld.lld, llvm-readobj):
OBJECT_LANG_LLVM_BIN=/path/to/llvm/bin cargo test --test llvm_oracle -- --ignored
```

The property tests in [`tests/properties.rs`](./tests/properties.rs) hold the writer to a decoder and a loader simulation written independently in [`tests/common`](./tests/common/mod.rs); [`tests/executable.rs`](./tests/executable.rs) runs the programs it builds when the host is Linux on the matching architecture. Every `rust` example in this README and in [`docs/API.md`](./docs/API.md) is compiled and run as a doctest.

<hr>
<br>

## Cross-platform support

- Linux (x86_64, aarch64)
- macOS (x86_64, Apple Silicon)
- Windows (x86_64)

The crate uses no operating-system facilities: it produces the same bytes on every host. The files it writes target Linux (ELF, static, direct system calls); on other hosts they are checked structurally, not run.

<hr>
<br>

## Contributing

See [`REPS.md`](./REPS.md) and [`dev/DIRECTIVES.md`](./dev/DIRECTIVES.md) for the engineering standards every change is held to, and [`dev/ROADMAP.md`](./dev/ROADMAP.md) for what comes next. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober.</strong></sup>
</div>
