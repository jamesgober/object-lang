//! The Tier-1 path: machine code in, a runnable static executable out.
//!
//! ```text
//! cargo run --example exit42 -- exit42          # x86-64 (default)
//! cargo run --example exit42 -- exit42 aarch64  # AArch64
//! ```
//!
//! On Linux, `chmod +x exit42 && ./exit42; echo $?` prints `42`.

use std::process::ExitCode;

use object_lang::Architecture;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| String::from("exit42"));
    let (arch, code): (Architecture, &[u8]) = match args.next().as_deref() {
        Some("aarch64") => (
            Architecture::Aarch64,
            &[
                0x40, 0x05, 0x80, 0xd2, // mov x0, #42
                0xa8, 0x0b, 0x80, 0xd2, // mov x8, #93 (exit)
                0x01, 0x00, 0x00, 0xd4, // svc #0
            ],
        ),
        _ => (
            Architecture::X86_64,
            &[
                0xbf, 0x2a, 0x00, 0x00, 0x00, // mov edi, 42
                0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60 (exit)
                0x0f, 0x05, //                   syscall
            ],
        ),
    };

    let program = match object_lang::elf::executable(arch, code) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = std::fs::write(&path, &program) {
        eprintln!("error: cannot write {path}: {err}");
        return ExitCode::FAILURE;
    }
    println!("wrote {} bytes to {path} ({arch:?})", program.len());
    ExitCode::SUCCESS
}
