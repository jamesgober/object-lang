<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>object-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

---

## [0.2.0] - 2026-10-08

The foundation: a format-neutral object model, and an ELF64 writer and reader for
x86-64 and AArch64 that produce relocatable objects any ELF linker accepts and static
executables the Linux kernel runs, with no system linker, assembler, or third-party
crate involved.

### Added

- The object model: `Object` (a `FileKind`, an `Architecture`, sections, symbols, an
  entry point, and the executable-stack request), `Section` with `SectionKind` and
  `SectionFlags` (alignment, address, entry size, bytes or a BSS size, and its
  relocations; `append` and `reserve` builders that pad and align), `Symbol` with
  `SymbolKind`, `Binding`, `Visibility`, and `SymbolSection`, and `Relocation` with
  `RelocationKind` (`X86_64Reloc`, `Aarch64Reloc`) and `RelocationTarget` (a symbol, or
  a section). Dense `SectionId` and `SymbolId`.
- `Object::validate`: every model rule in one pass, reported as `ModelError` with
  `SectionProblem`, `SymbolProblem`, and `RelocationProblem`.
- `elf::write` and `elf::write_into`: ELF64 relocatable objects (`ET_REL`) with
  `.rela` sections, local-first symbol tables (`sh_info`), `STT_SECTION` symbols for
  section targets, suffix-sharing string tables, `.note.GNU-stack`, and extended section
  numbering (`SHN_XINDEX`, `.symtab_shndx`) past 65,279 sections; and static executables
  (`ET_EXEC`) with page-aligned `PT_LOAD` segments grouped by permission, the headers
  mapped read-only, and `PT_GNU_STACK`. Deterministic output.
- x86-64 relocations `R_X86_64_64`, `32`, `32S`, `PC32`, `PC64`, `PLT32`, `GOTPCREL`,
  `GOTPCRELX`, `REX_GOTPCRELX`; AArch64 relocations `ABS64`, `ABS32`, `PREL64`,
  `PREL32`, `CALL26`, `JUMP26`, `CONDBR19`, `TSTBR14`, `ADR_PREL_LO21`,
  `ADR_PREL_PG_HI21`, `ADD_ABS_LO12_NC`, `LDST{8,16,32,64,128}_ABS_LO12_NC`,
  `ADR_GOT_PAGE`, `LD64_GOT_LO12_NC`.
- `elf::read` and `elf::read_with_limits`: a strict reader returning the same model,
  with `Limits` budgets (sections, symbols, relocations, name length, total name bytes),
  overflow-checked offsets, section data never copied beyond the input size, and no
  panics on any input. Reads clang's and ld.lld's output.
- `elf::executable` and `elf::code_address`: the one-call path from machine code to a
  runnable static executable.
- `WriteError`, `ReadError` (`#[non_exhaustive]`, `core::error::Error`).
- Examples `exit42`, `relocatable`, and `inspect`.
- Tests: unit, integration, known-answer, hostile-input, and property tests (round trip
  over random models; the writer against an independent decoder and a loader
  simulation; reader robustness on arbitrary and mutated bytes; determinism); executables
  run on Linux; clang-built fixtures; LLVM oracle tests (`--ignored`) linking every
  relocation kind with `ld.lld`. Criterion benchmarks at 100,000 symbols and 200,000
  relocations. `README.md` and `docs/API.md` examples run as doctests.

### Changed

- `Cargo.toml` description and keywords describe the crate as built.
- CI runs the LLVM oracle tests on Linux.

## [0.1.0] - 2026-10-08

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/object-lang/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/jamesgober/object-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/object-lang/releases/tag/v0.1.0
