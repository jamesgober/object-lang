//! A deliberately naive ELF64 decoder and a page-level loader simulation, written
//! straight from the ELF specification for use as test references. It shares no code
//! with the crate: it indexes with `[]` and panics on anything unexpected, which in a
//! test is a failure, as it should be.

#![allow(dead_code, reason = "each test binary uses a different part")]
#![allow(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::collections::BTreeMap;

pub fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}
pub fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
pub fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawSection {
    pub name: String,
    pub ty: u32,
    pub flags: u64,
    pub addr: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub info: u32,
    pub align: u64,
    pub entsize: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawSymbol {
    pub name: String,
    pub info: u8,
    pub other: u8,
    /// The full section index (after resolving SHN_XINDEX), or the reserved value.
    pub shndx: u32,
    pub value: u64,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawRela {
    pub offset: u64,
    pub sym: u32,
    pub ty: u32,
    pub addend: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawSegment {
    pub ty: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub paddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

#[derive(Clone, Debug)]
pub struct RawElf {
    pub e_type: u16,
    pub machine: u16,
    pub entry: u64,
    pub sections: Vec<RawSection>,
    pub shstrndx: usize,
    pub segments: Vec<RawSegment>,
}

pub fn cstr(table: &[u8], offset: usize) -> String {
    let end = table[offset..].iter().position(|&b| b == 0).unwrap();
    String::from_utf8(table[offset..offset + end].to_vec()).unwrap()
}

pub fn parse(b: &[u8]) -> RawElf {
    assert_eq!(&b[..4], b"\x7fELF");
    assert_eq!(b[4], 2, "ELFCLASS64");
    assert_eq!(b[5], 1, "little-endian");
    let shoff = u64_at(b, 0x28) as usize;
    let mut shnum = u16_at(b, 0x3c) as usize;
    let mut shstrndx = u16_at(b, 0x3e) as usize;
    let sh0 = &b[shoff..shoff + 64];
    if shnum == 0 {
        shnum = u64_at(sh0, 32) as usize;
    }
    if shstrndx == 0xffff {
        shstrndx = u32_at(sh0, 40) as usize;
    }
    let mut sections: Vec<RawSection> = (0..shnum)
        .map(|i| {
            let h = &b[shoff + i * 64..shoff + i * 64 + 64];
            RawSection {
                name: String::new(),
                ty: u32_at(h, 4),
                flags: u64_at(h, 8),
                addr: u64_at(h, 16),
                offset: u64_at(h, 24),
                size: u64_at(h, 32),
                link: u32_at(h, 40),
                info: u32_at(h, 44),
                align: u64_at(h, 48),
                entsize: u64_at(h, 56),
            }
        })
        .collect();
    let shstr = &sections[shstrndx];
    let table = b[shstr.offset as usize..(shstr.offset + shstr.size) as usize].to_vec();
    for i in 0..shnum {
        let h = &b[shoff + i * 64..];
        sections[i].name = cstr(&table, u32_at(h, 0) as usize);
    }
    let phoff = u64_at(b, 0x20) as usize;
    let phnum = u16_at(b, 0x38) as usize;
    let segments = (0..phnum)
        .map(|i| {
            let p = &b[phoff + i * 56..phoff + i * 56 + 56];
            RawSegment {
                ty: u32_at(p, 0),
                flags: u32_at(p, 4),
                offset: u64_at(p, 8),
                vaddr: u64_at(p, 16),
                paddr: u64_at(p, 24),
                filesz: u64_at(p, 32),
                memsz: u64_at(p, 40),
                align: u64_at(p, 48),
            }
        })
        .collect();
    RawElf {
        e_type: u16_at(b, 16),
        machine: u16_at(b, 18),
        entry: u64_at(b, 24),
        sections,
        shstrndx,
        segments,
    }
}

impl RawElf {
    pub fn contents<'a>(&self, b: &'a [u8], index: usize) -> &'a [u8] {
        let s = &self.sections[index];
        if s.ty == 8 {
            return &[];
        }
        &b[s.offset as usize..(s.offset + s.size) as usize]
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.sections.iter().position(|s| s.name == name)
    }

    pub fn symbols(&self, b: &[u8]) -> Vec<RawSymbol> {
        let Some(symtab) = self.sections.iter().position(|s| s.ty == 2) else {
            return Vec::new();
        };
        let sh = &self.sections[symtab];
        let strtab = self.contents(b, sh.link as usize).to_vec();
        let shndx = self
            .sections
            .iter()
            .position(|s| s.ty == 18)
            .map(|i| self.contents(b, i).to_vec());
        let data = self.contents(b, symtab);
        data.chunks_exact(24)
            .enumerate()
            .map(|(i, r)| {
                let raw = u16_at(r, 6);
                RawSymbol {
                    name: cstr(&strtab, u32_at(r, 0) as usize),
                    info: r[4],
                    other: r[5],
                    shndx: if raw == 0xffff {
                        u32_at(shndx.as_ref().unwrap(), i * 4)
                    } else {
                        u32::from(raw)
                    },
                    value: u64_at(r, 8),
                    size: u64_at(r, 16),
                }
            })
            .collect()
    }

    /// Relocations by the header index of the section they patch.
    pub fn relocations(&self, b: &[u8]) -> BTreeMap<usize, Vec<RawRela>> {
        let mut out = BTreeMap::new();
        for (i, s) in self.sections.iter().enumerate() {
            if s.ty != 4 {
                continue;
            }
            let list = self
                .contents(b, i)
                .chunks_exact(24)
                .map(|r| RawRela {
                    offset: u64_at(r, 0),
                    sym: (u64_at(r, 8) >> 32) as u32,
                    ty: u64_at(r, 8) as u32,
                    addend: u64_at(r, 16) as i64,
                })
                .collect();
            assert!(out.insert(s.info as usize, list).is_none());
        }
        out
    }
}

/// What the Linux loader would map: page-granular memory with permissions, built from
/// the PT_LOAD headers exactly as `load_elf_binary` does (file bytes up to `filesz`,
/// zeros up to `memsz`). Panics if two segments map the same page, or if a segment's
/// offset and address disagree within a page.
pub struct Memory {
    pub page: u64,
    /// page address -> (bytes, PF_* flags)
    pub pages: BTreeMap<u64, (Vec<u8>, u32)>,
}

impl Memory {
    pub fn load(b: &[u8], elf: &RawElf, page: u64) -> Memory {
        let mut pages: BTreeMap<u64, (Vec<u8>, u32)> = BTreeMap::new();
        let mut last_vaddr = 0;
        for seg in elf.segments.iter().filter(|s| s.ty == 1) {
            assert!(
                seg.vaddr >= last_vaddr,
                "PT_LOAD headers are not sorted by address"
            );
            last_vaddr = seg.vaddr;
            assert_eq!(seg.align, page, "PT_LOAD alignment");
            assert_eq!(
                seg.offset % page,
                seg.vaddr % page,
                "offset/address congruence"
            );
            assert!(seg.filesz <= seg.memsz);
            let first = seg.vaddr & !(page - 1);
            let end = seg.vaddr + seg.memsz;
            let mut p = first;
            while p < end {
                assert!(
                    pages
                        .insert(p, (vec![0; page as usize], seg.flags))
                        .is_none(),
                    "two segments map page {p:#x}"
                );
                p += page;
            }
            for i in 0..seg.filesz {
                let addr = seg.vaddr + i;
                let entry = pages.get_mut(&(addr & !(page - 1))).unwrap();
                entry.0[(addr & (page - 1)) as usize] = b[(seg.offset + i) as usize];
            }
        }
        Memory { page, pages }
    }

    pub fn read(&self, addr: u64, len: u64) -> Option<(Vec<u8>, Vec<u32>)> {
        let mut bytes = Vec::new();
        let mut flags = Vec::new();
        for a in addr..addr + len {
            let (page, f) = self.pages.get(&(a & !(self.page - 1)))?;
            bytes.push(page[(a & (self.page - 1)) as usize]);
            flags.push(*f);
        }
        Some((bytes, flags))
    }
}
