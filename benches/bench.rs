//! Criterion benchmarks: writing and reading ELF at the scale of a large compilation
//! unit.
//!
//! ```text
//! cargo bench --bench bench
//! ```
//!
//! - `relocatable`: 1,000 sections of 1.6 KB, 100,000 symbols (a third local), and
//!   200,000 relocations: write, read, and validate.
//! - `executable`: 3,000 loadable sections in 3 segments plus 100,000 symbols: write
//!   and read.
//! - `tier1`: the one-call `elf::executable` on a 64 KiB code blob.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use object_lang::{
    Aarch64Reloc, Architecture, Binding, Object, Relocation, RelocationTarget, Section,
    SectionKind, Symbol, SymbolId,
};

fn relocatable() -> Object {
    let mut obj = Object::relocatable(Architecture::Aarch64);
    let mut ids = Vec::new();
    for i in 0..1000 {
        ids.push(
            obj.add_section(
                Section::new(
                    format!(".text._ZN6module8function{i:04}E"),
                    SectionKind::Text,
                )
                .with_align(4)
                .with_data(vec![0x1f; 1600]),
            ),
        );
    }
    for i in 0..100_000u32 {
        let binding = if i % 3 == 0 {
            Binding::Local
        } else {
            Binding::Global
        };
        let id = ids[(i % 1000) as usize];
        obj.add_symbol(
            Symbol::function(
                format!("_ZN6module4item{i:06}17h0123456789abcdefE"),
                id,
                u64::from(i % 400) * 4,
                4,
            )
            .with_binding(binding),
        );
    }
    for i in 0..200_000u32 {
        let id = ids[(i % 1000) as usize];
        let target = RelocationTarget::Symbol(SymbolId::new((i * 7) % 100_000));
        let reloc = Relocation::new(
            u64::from((i / 1000) % 400) * 4,
            Aarch64Reloc::Call26,
            target,
            0,
        );
        if obj.add_relocation(id, reloc).is_err() {
            unreachable!("the section exists");
        }
    }
    obj
}

fn executable() -> Object {
    let mut obj = Object::executable(Architecture::X86_64);
    let mut address = 0x40_1000u64;
    let mut first = None;
    for kind in [
        SectionKind::Text,
        SectionKind::ReadOnlyData,
        SectionKind::Data,
    ] {
        for i in 0..1000 {
            let id = obj.add_section(
                Section::new(format!(".s{i}"), kind)
                    .with_align(16)
                    .with_address(address)
                    .with_data(vec![0xcc; 512]),
            );
            first.get_or_insert(id);
            address += 512;
        }
        address = (address + 0x1fff) & !0xfff;
    }
    let text = first.unwrap_or_else(|| unreachable!("sections were added"));
    for i in 0..100_000u64 {
        obj.add_symbol(Symbol::function(
            format!("f{i}"),
            text,
            0x40_1000 + i % 512,
            0,
        ));
    }
    obj.set_entry(0x40_1000);
    obj
}

fn bench_relocatable(c: &mut Criterion) {
    let obj = relocatable();
    let bytes = object_lang::elf::write(&obj).unwrap_or_default();
    assert!(!bytes.is_empty(), "the benchmark object must be valid");
    let mut group = c.benchmark_group("relocatable");
    group.sample_size(20);
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    let mut buffer = Vec::with_capacity(bytes.len());
    group.bench_function("write", |b| {
        b.iter(|| {
            buffer.clear();
            let result = object_lang::elf::write_into(black_box(&obj), &mut buffer);
            black_box(result.is_ok())
        });
    });
    group.bench_function("read", |b| {
        b.iter(|| object_lang::elf::read(black_box(&bytes)))
    });
    group.bench_function("validate", |b| b.iter(|| black_box(&obj).validate()));
    group.finish();
}

fn bench_executable(c: &mut Criterion) {
    let obj = executable();
    let bytes = object_lang::elf::write(&obj).unwrap_or_default();
    assert!(!bytes.is_empty(), "the benchmark executable must be valid");
    let mut group = c.benchmark_group("executable");
    group.sample_size(20);
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("write", |b| {
        b.iter(|| object_lang::elf::write(black_box(&obj)))
    });
    group.bench_function("read", |b| {
        b.iter(|| object_lang::elf::read(black_box(&bytes)))
    });
    group.finish();
}

fn bench_tier1(c: &mut Criterion) {
    let code = vec![0x90u8; 64 * 1024];
    let mut group = c.benchmark_group("tier1");
    group.throughput(Throughput::Bytes(code.len() as u64));
    group.bench_function("executable_64k", |b| {
        b.iter(|| object_lang::elf::executable(Architecture::X86_64, black_box(&code)));
    });
    group.finish();
}

criterion_group!(benches, bench_relocatable, bench_executable, bench_tier1);
criterion_main!(benches);
