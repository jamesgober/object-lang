//! Cross-checks against LLVM's tools, used as test oracles only (LexerSketch decision
//! D3: external tools may check our output in tests; nothing ships with them).
//!
//! These tests are `#[ignore]`d because they need LLVM installed. Run them with
//!
//! ```text
//! OBJECT_LANG_LLVM_BIN=/path/to/llvm/bin cargo test --test llvm_oracle -- --ignored
//! ```
//!
//! (without the variable the tools are looked up on `PATH`). They fail, not skip, when
//! a tool is missing.
//!
//! - `ld.lld` links relocatable objects this crate writes, for both architectures and
//!   every relocation kind; the linked bytes are then checked against each kind's
//!   formula, which proves the relocation numbering and addend handling match LLVM.
//!   The executable lld writes is read back by this crate's reader.
//! - `llvm-readobj` parses every file the writer produces without a warning.

#![allow(clippy::unwrap_used, reason = "test failures should be loud")]

use std::path::{Path, PathBuf};
use std::process::Command;

use object_lang::{
    Aarch64Reloc, Architecture, Object, Relocation, RelocationTarget, Section, SectionKind, Symbol,
    X86_64Reloc,
};

fn tool(name: &str) -> PathBuf {
    match std::env::var_os("OBJECT_LANG_LLVM_BIN") {
        Some(dir) => Path::new(&dir).join(name),
        None => PathBuf::from(name),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("object-lang-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn run(cmd: &mut Command) -> String {
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("cannot run {cmd:?}: {e}"));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{cmd:?} failed:\n{stderr}");
    assert!(stderr.trim().is_empty(), "{cmd:?} warned:\n{stderr}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Links `obj` with ld.lld into a static executable and reads it back.
fn link(obj: &Object, name: &str) -> (Vec<u8>, Object) {
    let input = scratch(&format!("{name}.o"));
    let output = scratch(name);
    std::fs::write(&input, object_lang::elf::write(obj).unwrap()).unwrap();
    let _ = run(Command::new(tool("llvm-readobj"))
        .args(["--all", "--elf-output-style=GNU"])
        .arg(&input));
    let _ = run(Command::new(tool("ld.lld"))
        .args([
            "-static",
            "-z",
            "norelro",
            "--no-relax",
            "-e",
            "_start",
            "-o",
        ])
        .arg(&output)
        .arg(&input));
    let bytes = std::fs::read(&output).unwrap();
    let linked = object_lang::elf::read(&bytes)
        .unwrap_or_else(|e| panic!("our reader refused lld's executable: {e}"));
    (bytes, linked)
}

fn address_of(exe: &Object, name: &str) -> u64 {
    let id = exe
        .symbol_by_name(name)
        .unwrap_or_else(|| panic!("no symbol {name}"));
    exe.symbols()[id.index()].value
}

fn section_of<'a>(exe: &'a Object, name: &str) -> &'a Section {
    &exe.sections()[exe.section_by_name(name).unwrap().index()]
}

/// Reads `N` bytes at virtual address `addr` from the linked executable's sections.
fn load<const N: usize>(exe: &Object, addr: u64) -> [u8; N] {
    for s in exe.sections() {
        if addr >= s.address() && addr + N as u64 <= s.address() + s.size() && !s.data().is_empty()
        {
            let at = (addr - s.address()) as usize;
            return s.data()[at..at + N].try_into().unwrap();
        }
    }
    panic!("address {addr:#x} is not in any section");
}

fn i32_at(exe: &Object, addr: u64) -> i64 {
    i64::from(i32::from_le_bytes(load(exe, addr)))
}

fn u32_at(exe: &Object, addr: u64) -> u32 {
    u32::from_le_bytes(load(exe, addr))
}

fn u64_at(exe: &Object, addr: u64) -> u64 {
    u64::from_le_bytes(load(exe, addr))
}

fn sign_extend(value: u64, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((value << shift) as i64) >> shift
}

#[test]
#[ignore = "needs LLVM (ld.lld, llvm-readobj); see the module docs"]
fn lld_applies_every_x86_64_relocation_as_documented() {
    let mut obj = Object::relocatable(Architecture::X86_64);
    #[rustfmt::skip]
    let code = vec![
        0xe8, 0, 0, 0, 0,                   // 0x00 call func          Plt32 @1
        0x48, 0x8d, 0x05, 0, 0, 0, 0,       // 0x05 lea rax,[rip+..]   Pc32 @8 -> .rodata+2
        0x48, 0xc7, 0xc0, 0, 0, 0, 0,       // 0x0c mov rax, imm32     Abs32Signed @15 -> data_word
        0xb8, 0, 0, 0, 0,                   // 0x13 mov eax, imm32     Abs32 @20 -> data_word+4
        0x48, 0x8b, 0x05, 0, 0, 0, 0,       // 0x18 mov rax,[rip+GOT]  GotPcRel @27 -> data_word
        0x48, 0x8b, 0x05, 0, 0, 0, 0,       // 0x1f mov rax,[rip+GOT]  RexGotPcRelX @34 -> data_word
        0x8b, 0x05, 0, 0, 0, 0,             // 0x26 mov eax,[rip+GOT]  GotPcRelX @40 -> data_word
        0xc3,                               // 0x2c ret
        0xc3,                               // 0x2d func: ret
    ];
    let text = obj.add_section(
        Section::new(".text", SectionKind::Text)
            .with_align(16)
            .with_data(code),
    );
    let rodata = obj.add_section(
        Section::new(".rodata", SectionKind::ReadOnlyData).with_data(b"xxhello\0".to_vec()),
    );
    let data = obj.add_section(
        Section::new(".data", SectionKind::Data)
            .with_align(8)
            .with_data(vec![0; 24]),
    );
    obj.add_symbol(Symbol::function("_start", text, 0, 0x2d));
    let func = obj.add_symbol(Symbol::function("func", text, 0x2d, 1));
    let word = obj.add_symbol(Symbol::data("data_word", data, 0, 8));
    let sym = RelocationTarget::Symbol;
    for (section, offset, kind, target, addend) in [
        (text, 1, X86_64Reloc::Plt32, sym(func), -4),
        (
            text,
            8,
            X86_64Reloc::Pc32,
            RelocationTarget::Section(rodata),
            2 - 4,
        ),
        (text, 15, X86_64Reloc::Abs32Signed, sym(word), 0),
        (text, 20, X86_64Reloc::Abs32, sym(word), 4),
        (text, 27, X86_64Reloc::GotPcRel, sym(word), -4),
        (text, 34, X86_64Reloc::RexGotPcRelX, sym(word), -4),
        (text, 40, X86_64Reloc::GotPcRelX, sym(word), -4),
        (data, 8, X86_64Reloc::Abs64, sym(func), 1),
        (data, 16, X86_64Reloc::Pc64, sym(func), 0),
    ] {
        obj.add_relocation(section, Relocation::new(offset, kind, target, addend))
            .unwrap();
    }

    let (_, exe) = link(&obj, "x86_64-relocs");
    let start = address_of(&exe, "_start");
    let func = address_of(&exe, "func");
    let word = address_of(&exe, "data_word");
    let rodata = section_of(&exe, ".rodata").address();

    let pc_rel = |field: u64| (start + field + 4).wrapping_add(i32_at(&exe, start + field) as u64);
    assert_eq!(pc_rel(1), func, "Plt32");
    assert_eq!(pc_rel(8), rodata + 2, "Pc32 against a section");
    assert_eq!(i32_at(&exe, start + 15) as u64, word, "Abs32Signed");
    assert_eq!(u64::from(u32_at(&exe, start + 20)), word + 4, "Abs32");
    for field in [27, 34, 40] {
        // With relaxation off, each GOT load reads a GOT slot holding the address.
        assert_eq!(u64_at(&exe, pc_rel(field)), word, "GOT load at {field}");
    }
    assert_eq!(u64_at(&exe, word + 8), func + 1, "Abs64");
    assert_eq!(
        u64_at(&exe, word + 16),
        func.wrapping_sub(word + 16),
        "Pc64"
    );
}

#[test]
#[ignore = "needs LLVM (ld.lld, llvm-readobj); see the module docs"]
fn lld_applies_every_aarch64_relocation_as_documented() {
    let mut obj = Object::relocatable(Architecture::Aarch64);
    let insns: [u32; 13] = [
        0x9400_0000, // 0x00 bl func                     Call26
        0x1400_0000, // 0x04 b func                      Jump26
        0x5400_0000, // 0x08 b.eq func                   CondBr19
        0x3600_0000, // 0x0c tbz w0, #0, func            TstBr14
        0x9000_0001, // 0x10 adrp x1, data_word          AdrPrelPgHi21
        0x9100_0021, // 0x14 add x1, x1, :lo12:data_word AddAbsLo12Nc
        0xf940_0022, // 0x18 ldr x2, [x1, :lo12:..]      Ldst64AbsLo12Nc -> data_word+8
        0x1000_0003, // 0x1c adr x3, func                AdrPrelLo21
        0x9000_0004, // 0x20 adrp x4, :got:data_word     AdrGotPage
        0xf940_0084, // 0x24 ldr x4, [x4, :got_lo12:..]  Ld64GotLo12Nc
        0x3940_0025, // 0x28 ldrb w5, [x1, :lo12:..]     Ldst8AbsLo12Nc -> data_word+3
        0xd65f_03c0, // 0x2c ret
        0xd65f_03c0, // 0x30 func: ret
    ];
    let code: Vec<u8> = insns.iter().flat_map(|i| i.to_le_bytes()).collect();
    let text = obj.add_section(
        Section::new(".text", SectionKind::Text)
            .with_align(4)
            .with_data(code),
    );
    let data = obj.add_section(
        Section::new(".data", SectionKind::Data)
            .with_align(8)
            .with_data(vec![0; 32]),
    );
    obj.add_symbol(Symbol::function("_start", text, 0, 0x30));
    let func = obj.add_symbol(Symbol::function("func", text, 0x30, 4));
    let word = obj.add_symbol(Symbol::data("data_word", data, 0, 8));
    let sym = RelocationTarget::Symbol;
    for (section, offset, kind, target, addend) in [
        (text, 0x00, Aarch64Reloc::Call26, sym(func), 0),
        (text, 0x04, Aarch64Reloc::Jump26, sym(func), 0),
        (text, 0x08, Aarch64Reloc::CondBr19, sym(func), 0),
        (text, 0x0c, Aarch64Reloc::TstBr14, sym(func), 0),
        (text, 0x10, Aarch64Reloc::AdrPrelPgHi21, sym(word), 0),
        (text, 0x14, Aarch64Reloc::AddAbsLo12Nc, sym(word), 0),
        (text, 0x18, Aarch64Reloc::Ldst64AbsLo12Nc, sym(word), 8),
        (text, 0x1c, Aarch64Reloc::AdrPrelLo21, sym(func), 0),
        (text, 0x20, Aarch64Reloc::AdrGotPage, sym(word), 0),
        (text, 0x24, Aarch64Reloc::Ld64GotLo12Nc, sym(word), 0),
        (text, 0x28, Aarch64Reloc::Ldst8AbsLo12Nc, sym(word), 3),
        (data, 8, Aarch64Reloc::Abs64, sym(func), 4),
        (data, 16, Aarch64Reloc::Prel64, sym(func), 0),
        (data, 24, Aarch64Reloc::Prel32, sym(func), 0),
        (data, 28, Aarch64Reloc::Abs32, sym(word), 0),
    ] {
        obj.add_relocation(section, Relocation::new(offset, kind, target, addend))
            .unwrap();
    }

    let (_, exe) = link(&obj, "aarch64-relocs");
    let start = address_of(&exe, "_start");
    let func = address_of(&exe, "func");
    let word = address_of(&exe, "data_word");
    let insn = |at: u64| u64::from(u32_at(&exe, start + at));
    let page = |x: u64| x & !0xfff;
    let adr_imm = |i: u64| sign_extend((((i >> 5) & 0x7ffff) << 2) | ((i >> 29) & 3), 21);

    assert_eq!(
        start + (sign_extend(insn(0x00) & 0x3ff_ffff, 26) << 2) as u64,
        func,
        "Call26"
    );
    assert_eq!(
        start + 4 + (sign_extend(insn(0x04) & 0x3ff_ffff, 26) << 2) as u64,
        func,
        "Jump26"
    );
    assert_eq!(
        start + 8 + (sign_extend((insn(0x08) >> 5) & 0x7ffff, 19) << 2) as u64,
        func,
        "CondBr19"
    );
    assert_eq!(
        start + 12 + (sign_extend((insn(0x0c) >> 5) & 0x3fff, 14) << 2) as u64,
        func,
        "TstBr14"
    );
    assert_eq!(
        page(start + 0x10).wrapping_add((adr_imm(insn(0x10)) << 12) as u64),
        page(word),
        "AdrPrelPgHi21"
    );
    assert_eq!((insn(0x14) >> 10) & 0xfff, word & 0xfff, "AddAbsLo12Nc");
    assert_eq!(
        ((insn(0x18) >> 10) & 0xfff) << 3,
        (word + 8) & 0xfff,
        "Ldst64AbsLo12Nc"
    );
    assert_eq!(
        (start + 0x1c).wrapping_add(adr_imm(insn(0x1c)) as u64),
        func,
        "AdrPrelLo21"
    );
    let got_page = page(start + 0x20).wrapping_add((adr_imm(insn(0x20)) << 12) as u64);
    let got_slot = got_page + (((insn(0x24) >> 10) & 0xfff) << 3);
    assert_eq!(u64_at(&exe, got_slot), word, "AdrGotPage + Ld64GotLo12Nc");
    assert_eq!(
        (insn(0x28) >> 10) & 0xfff,
        (word + 3) & 0xfff,
        "Ldst8AbsLo12Nc"
    );
    assert_eq!(u64_at(&exe, word + 8), func + 4, "Abs64");
    assert_eq!(
        u64_at(&exe, word + 16),
        func.wrapping_sub(word + 16),
        "Prel64"
    );
    assert_eq!(
        i32_at(&exe, word + 24),
        func as i64 - (word + 24) as i64,
        "Prel32"
    );
    assert_eq!(u64::from(u32_at(&exe, word + 28)), word, "Abs32");
}

#[test]
#[ignore = "needs LLVM (llvm-readobj); see the module docs"]
fn llvm_readobj_accepts_our_executables_without_warnings() {
    for (arch, code) in [
        (Architecture::X86_64, &[0xeb, 0xfe][..]),
        (Architecture::Aarch64, &[0x00, 0x00, 0x00, 0x14][..]),
    ] {
        let path = scratch(&format!("readobj-{arch:?}"));
        std::fs::write(&path, object_lang::elf::executable(arch, code).unwrap()).unwrap();
        let out = run(Command::new(tool("llvm-readobj"))
            .args(["--all", "--elf-output-style=GNU"])
            .arg(&path));
        assert!(out.contains("EXEC (Executable file)"), "{out}");
        assert!(out.contains("GNU_STACK"), "{out}");
    }
}
