//! Static executables built from hand-assembled machine code.
//!
//! On every platform the files are checked structurally: read back by the crate, and
//! mapped by the reference loader in `tests/common`. On Linux, for the matching
//! architecture, they are also run, and their exit status and output checked. Nothing
//! here uses a system assembler or linker.

#![allow(clippy::unwrap_used, reason = "test failures should be loud")]

mod common;

use object_lang::{Architecture, FileKind, Object, Section, SectionKind, Symbol};

/// `mov edi, 42; mov eax, 60; syscall`: exit(42) on Linux x86-64.
const EXIT42_X86_64: [u8; 12] = [
    0xbf, 0x2a, 0x00, 0x00, 0x00, // mov edi, 42
    0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60
    0x0f, 0x05, //                   syscall
];

/// `mov x0, #42; mov x8, #93; svc #0`: exit(42) on Linux AArch64.
const EXIT42_AARCH64: [u8; 12] = [
    0x40, 0x05, 0x80, 0xd2, // mov x0, #42
    0xa8, 0x0b, 0x80, 0xd2, // mov x8, #93
    0x01, 0x00, 0x00, 0xd4, // svc #0
];

/// Checks an executable against the reference decoder and loader: the code is mapped
/// read+execute at its address, the headers read-only below it, and the entry point
/// is the first instruction.
fn check_structure(bytes: &[u8], arch: Architecture, code: &[u8]) {
    let elf = common::parse(bytes);
    assert_eq!(elf.e_type, 2, "ET_EXEC");
    let page = arch.page_size();
    let address = object_lang::elf::code_address(arch);
    assert_eq!(elf.entry, address);
    // No interpreter, no dynamic section: only PT_LOADs and PT_GNU_STACK.
    let types: Vec<u32> = elf.segments.iter().map(|s| s.ty).collect();
    assert_eq!(types, [1, 1, 0x6474_e551]);
    assert_eq!(elf.segments[0].flags, 4, "headers are read-only");
    assert_eq!(elf.segments[1].flags, 5, "code is read+execute");
    assert_eq!(elf.segments[2].flags, 6, "the stack is not executable");

    let memory = common::Memory::load(bytes, &elf, page);
    let (loaded, flags) = memory.read(address, code.len() as u64).unwrap();
    assert_eq!(loaded, code);
    assert!(flags.iter().all(|&f| f == 5));
    // AT_PHDR = first segment + e_phoff must be readable memory.
    assert!(memory.read(elf.segments[0].vaddr + 64, 56 * 3).is_some());

    let obj = object_lang::elf::read(bytes).unwrap();
    assert_eq!(obj.kind(), FileKind::Executable);
    assert_eq!(obj.architecture(), arch);
    assert_eq!(obj.entry(), address);
    assert_eq!(obj.sections()[0].data(), code);
    assert_eq!(obj.symbols()[0].name, "_start");
}

#[cfg(target_os = "linux")]
fn run(bytes: &[u8], name: &str) -> std::process::Output {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("object-lang-exec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Tests run on parallel threads. A sibling thread that forks while this file's write
    // descriptor is still open hands the child a copy of it, and exec then fails with
    // ETXTBSY (errno 26) until that child execs and drops it. The window is short, so
    // retry rather than flake.
    const ETXTBSY: i32 = 26;
    let mut attempts = 0;
    let output = loop {
        match std::process::Command::new(&path).output() {
            Err(e) if e.raw_os_error() == Some(ETXTBSY) && attempts < 100 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            result => break result.unwrap(),
        }
    };
    let _ = std::fs::remove_file(&path);
    output
}

#[test]
fn exit42_x86_64() {
    let bytes = object_lang::elf::executable(Architecture::X86_64, &EXIT42_X86_64).unwrap();
    check_structure(&bytes, Architecture::X86_64, &EXIT42_X86_64);

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let output = run(&bytes, "exit42");
        assert_eq!(output.status.code(), Some(42), "{output:?}");
    }
}

#[test]
fn exit42_aarch64() {
    let bytes = object_lang::elf::executable(Architecture::Aarch64, &EXIT42_AARCH64).unwrap();
    check_structure(&bytes, Architecture::Aarch64, &EXIT42_AARCH64);

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        let output = run(&bytes, "exit42");
        assert_eq!(output.status.code(), Some(42), "{output:?}");
    }
}

/// Addresses of the multi-segment programs below.
struct Layout {
    text: u64,
    rodata: u64,
    data: u64,
    bss: u64,
}

impl Layout {
    fn new(arch: Architecture) -> Layout {
        let page = arch.page_size();
        let text = object_lang::elf::code_address(arch);
        Layout {
            text,
            rodata: text + page,
            data: text + 2 * page,
            bss: text + 2 * page + 4,
        }
    }
}

/// Builds the four-section program: `.text` (RX), `.rodata` (R), `.data` and `.bss`
/// (RW, one segment with the BSS at its tail).
fn program(arch: Architecture, code: Vec<u8>) -> Object {
    let at = Layout::new(arch);
    let mut obj = Object::executable(arch);
    let len = code.len() as u64;
    let text = obj.add_section(
        Section::new(".text", SectionKind::Text)
            .with_align(16)
            .with_address(at.text)
            .with_data(code),
    );
    obj.add_section(
        Section::new(".rodata", SectionKind::ReadOnlyData)
            .with_address(at.rodata)
            .with_data(b"hello\n".to_vec()),
    );
    obj.add_section(
        Section::new(".data", SectionKind::Data)
            .with_align(4)
            .with_address(at.data)
            .with_data(40u32.to_le_bytes().to_vec()),
    );
    obj.add_section(
        Section::new(".bss", SectionKind::Bss)
            .with_align(4)
            .with_address(at.bss)
            .with_bss_size(4096),
    );
    obj.add_symbol(Symbol::function("_start", text, at.text, len));
    obj.set_entry(at.text);
    obj
}

/// x86-64: write(1, "hello\n", 6); bss = 2; exit(data + bss) — exit status 42 only if
/// `.data` was loaded with its bytes and `.bss` was mapped writable and zeroed.
fn x86_64_hello() -> Vec<u8> {
    let at = Layout::new(Architecture::X86_64);
    let mut code: Vec<u8> = Vec::new();
    // Appends an instruction whose RIP-relative displacement (at `disp_at` within it)
    // points at `target`; `tail` is what follows the displacement.
    let rip = |code: &mut Vec<u8>, head: &[u8], target: u64, tail: &[u8]| {
        let end = at.text + (code.len() + head.len() + 4 + tail.len()) as u64;
        let disp = (target as i64 - end as i64) as i32;
        code.extend_from_slice(head);
        code.extend_from_slice(&disp.to_le_bytes());
        code.extend_from_slice(tail);
    };
    code.extend_from_slice(&[0xb8, 1, 0, 0, 0]); // mov eax, 1 (write)
    code.extend_from_slice(&[0xbf, 1, 0, 0, 0]); // mov edi, 1
    rip(&mut code, &[0x48, 0x8d, 0x35], at.rodata, &[]); // lea rsi, [rip+msg]
    code.extend_from_slice(&[0xba, 6, 0, 0, 0]); // mov edx, 6
    code.extend_from_slice(&[0x0f, 0x05]); // syscall
    rip(&mut code, &[0xc7, 0x05], at.bss + 4092, &[2, 0, 0, 0]); // mov dword [bss_end-4], 2
    rip(&mut code, &[0x8b, 0x3d], at.data, &[]); // mov edi, [data]
    rip(&mut code, &[0x03, 0x3d], at.bss + 4092, &[]); // add edi, [bss_end-4]
    rip(&mut code, &[0x03, 0x3d], at.bss, &[]); // add edi, [bss] (zero)
    code.extend_from_slice(&[0xb8, 0x3c, 0, 0, 0]); // mov eax, 60 (exit)
    code.extend_from_slice(&[0x0f, 0x05]); // syscall
    code
}

/// AArch64: the same program.
fn aarch64_hello() -> Vec<u8> {
    let at = Layout::new(Architecture::Aarch64);
    let mut insns: Vec<u32> = Vec::new();
    let pc = |insns: &Vec<u32>| at.text + 4 * insns.len() as u64;
    let adrp = |pc: u64, rd: u32, target: u64| {
        let imm = ((target >> 12) as i64 - (pc >> 12) as i64) as u32;
        0x9000_0000 | ((imm & 3) << 29) | (((imm >> 2) & 0x7ffff) << 5) | rd
    };
    let add_lo12 = |rd: u32, rn: u32, target: u64| {
        0x9100_0000 | (((target & 0xfff) as u32) << 10) | (rn << 5) | rd
    };
    insns.push(0xd280_0020); // mov x0, #1
    let p = pc(&insns);
    insns.push(adrp(p, 1, at.rodata)); // adrp x1, msg
    insns.push(add_lo12(1, 1, at.rodata)); // add x1, x1, :lo12:msg
    insns.push(0xd280_00c2); // mov x2, #6
    insns.push(0xd280_0808); // mov x8, #64 (write)
    insns.push(0xd400_0001); // svc #0
    let p = pc(&insns);
    insns.push(adrp(p, 3, at.data)); // adrp x3, data
    insns.push(add_lo12(3, 3, at.data)); // add x3, x3, :lo12:data
    insns.push(0xb940_0060); // ldr w0, [x3]
    let p = pc(&insns);
    insns.push(adrp(p, 4, at.bss + 4092)); // adrp x4, bss_end-4
    insns.push(add_lo12(4, 4, at.bss + 4092)); // add x4, x4, :lo12:bss_end-4
    insns.push(0x5280_0045); // mov w5, #2
    insns.push(0xb900_0085); // str w5, [x4]
    insns.push(0xb940_0086); // ldr w6, [x4]
    insns.push(0x0b06_0000); // add w0, w0, w6
    insns.push(0xd280_0ba8); // mov x8, #93 (exit)
    insns.push(0xd400_0001); // svc #0
    insns.iter().flat_map(|i| i.to_le_bytes()).collect()
}

fn check_program(obj: &Object) -> Vec<u8> {
    let bytes = object_lang::elf::write(obj).unwrap();
    let elf = common::parse(&bytes);
    let loads: Vec<u32> = elf
        .segments
        .iter()
        .filter(|s| s.ty == 1)
        .map(|s| s.flags)
        .collect();
    // headers (R), .text (RX), .rodata (R), .data + .bss (RW)
    assert_eq!(loads, [4, 5, 4, 6]);
    let rw = elf.segments.iter().filter(|s| s.ty == 1).nth(3).unwrap();
    assert_eq!((rw.filesz, rw.memsz), (4, 4 + 4096));
    let memory = common::Memory::load(&bytes, &elf, obj.architecture().page_size());
    let at = Layout::new(obj.architecture());
    assert_eq!(memory.read(at.rodata, 6).unwrap().0, b"hello\n");
    assert_eq!(memory.read(at.data, 4).unwrap().0, 40u32.to_le_bytes());
    assert!(memory.read(at.bss, 4096).unwrap().0.iter().all(|&b| b == 0));
    assert_eq!(&object_lang::elf::read(&bytes).unwrap(), obj);
    bytes
}

#[test]
fn hello_x86_64_uses_every_kind_of_segment() {
    let bytes = check_program(&program(Architecture::X86_64, x86_64_hello()));

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let output = run(&bytes, "hello");
        assert_eq!(output.stdout, b"hello\n");
        assert_eq!(output.status.code(), Some(42), "{output:?}");
    }
    let _ = bytes;
}

#[test]
fn hello_aarch64_uses_every_kind_of_segment() {
    let bytes = check_program(&program(Architecture::Aarch64, aarch64_hello()));

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        let output = run(&bytes, "hello");
        assert_eq!(output.stdout, b"hello\n");
        assert_eq!(output.status.code(), Some(42), "{output:?}");
    }
    let _ = bytes;
}

#[test]
fn executable_tier_one_refuses_empty_code() {
    assert!(object_lang::elf::executable(Architecture::X86_64, &[]).is_err());
}
