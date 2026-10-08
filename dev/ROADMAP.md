# object-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../_lexersketch/ROADMAP.md and ../_lexersketch/NEW-LIBS.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt, DIRECTIVES, ROADMAP.

## v0.2.0 - Foundation (DONE)
- [x] ELF64 writer and reader: sections, symbols, relocations; round-trip property tests.
- [x] Format-neutral object model designed so COFF/PE and Mach-O fit it.
- [x] ELF64 executables (`ET_EXEC`, static, no interpreter) that the Linux kernel runs.
- [x] Every public item has rustdoc and a runnable example; every DIRECTIVES §4 invariant property-tested.

Delivered:
- Model: `Object` (`FileKind`, `Architecture`, entry point, executable-stack request),
  `Section` (`SectionKind`, `SectionFlags`, alignment, entry size, address, bytes or BSS
  size, relocations; `append`/`reserve` builders), `Symbol` (`SymbolKind`, `Binding`,
  `Visibility`, `SymbolSection`), `Relocation` (`RelocationKind` = `X86_64Reloc` |
  `Aarch64Reloc`, `RelocationTarget` = symbol | section, explicit addend), dense
  `SectionId`/`SymbolId`. `Object::validate` with `ModelError` +
  `SectionProblem`/`SymbolProblem`/`RelocationProblem`.
- ELF writer (`elf::write`, `elf::write_into`): `ET_REL` with `.rela*`, local-first
  symbol table and `sh_info`, `STT_SECTION` symbols for section targets, suffix-sharing
  string tables (MSD radix sort over reversed names), `.note.GNU-stack`, extended
  numbering (`SHN_XINDEX`, `.symtab_shndx`); `ET_EXEC` with permission-grouped,
  page-aligned `PT_LOAD`s, headers mapped read-only, `PT_GNU_STACK`. Deterministic.
- Relocations: x86-64 `64/32/32S/PC32/PC64/PLT32/GOTPCREL/GOTPCRELX/REX_GOTPCRELX`;
  AArch64 `ABS64/ABS32/PREL64/PREL32/CALL26/JUMP26/CONDBR19/TSTBR14/ADR_PREL_LO21/
  ADR_PREL_PG_HI21/ADD_ABS_LO12_NC/LDST{8,16,32,64,128}_ABS_LO12_NC/ADR_GOT_PAGE/
  LD64_GOT_LO12_NC`.
- ELF reader (`elf::read`, `elf::read_with_limits`): strict, `Limits`-budgeted,
  overflow-checked, panic-free; reads clang objects and ld.lld static executables.
- Tier 1: `elf::executable(arch, code)` and `elf::code_address`.
- Verification: round-trip, reference-decoder, loader-simulation, determinism, and
  hostile-input property tests; known-answer tests; executables run on Linux; clang
  fixtures; LLVM oracle (ld.lld links every relocation kind on both architectures) in CI.

Dependency wiring (decided here, recorded per the anti-deferral rule):
- **No first-party crate is wired.** object-lang sits below `linker-lang` 2.0 and
  `aot-lang` 2.0, which will depend on it, not the reverse. Machine code arrives as bytes
  from the caller (later `backend-lang`/`isa-lang`). Nothing here needs `span-lang`
  (no source positions), `diag-lang` (errors are plain values a caller can turn into
  diagnostics), or `intern-lang` (names are owned `String`s; an interned-name model is a
  candidate for 0.5 if profiling of linker-lang 2.0 shows name copies matter).
- No third-party runtime dependency. Dev-only: `criterion`, `proptest`; LLVM tools as
  test oracles (D3) in `tests/llvm_oracle.rs` and the CI `oracle` job.

Moved out of v0.2.0, with reasons (anti-deferral rule):
- **Thread-local storage relocations and `PT_TLS`** → v0.5.0. Not in the v0.2.0 task
  scope; TLS sections and symbols are modelled, but TLS relocations (`TPOFF32`,
  `GOTTPOFF`, `TLSGD`/`TLSLD`, AArch64 `TLSLE_*`/`TLSIE_*`/`TLSDESC_*`) and the
  executable's `PT_TLS` template need the linker-lang 2.0 TLS design (which models to
  support, relaxations) to be fixed first, so both sides land together. Today the writer
  refuses TLS sections in executables and the reader refuses TLS relocations, each with
  a named error.
- **COMDAT groups (`SHT_GROUP`)** → v0.5.0 (new item). Needed to read C++ and some Rust
  objects; it adds a group concept to the neutral model that COFF's COMDAT selection
  also needs, so it is designed with COFF. Refused with a named error until then.

## v0.5.0 - Implementation
- [ ] COFF/PE32+ and Mach-O 64; import/export tables; DWARF line tables and unwind (.eh_frame, .pdata/.xdata, compact unwind).
- [ ] Thread-local storage: TLS relocations for both architectures, `PT_TLS` in executables (moved from v0.2.0, see above).
- [ ] COMDAT groups in the model, ELF `SHT_GROUP` and COFF COMDAT (moved from v0.2.0, see above).

## v0.9.0 - Hardening
- [ ] Fuzzing readers; CodeView/PDB; determinism tests.

## v1.0.0 - Stable
- [ ] Frozen after aot-lang 2.0 produces runnable executables on all three OSes (D18).
