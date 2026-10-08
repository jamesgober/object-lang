# object-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../_lexersketch/ROADMAP.md and ../_lexersketch/NEW-LIBS.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt, DIRECTIVES, ROADMAP.

## v0.2.0 - Foundation
- [ ] ELF64 writer and reader: sections, symbols, relocations; round-trip property tests.

## v0.5.0 - Implementation
- [ ] COFF/PE32+ and Mach-O 64; import/export tables; DWARF line tables and unwind (.eh_frame, .pdata/.xdata, compact unwind).

## v0.9.0 - Hardening
- [ ] Fuzzing readers; CodeView/PDB; determinism tests.

## v1.0.0 - Stable
- [ ] Frozen after aot-lang 2.0 produces runnable executables on all three OSes (D18).
