//! The rules of the object model, checked in one linear pass (plus one sort for
//! executables), shared by every writer and every reader.

use alloc::vec::Vec;

use crate::error::{ModelError, RelocationProblem, SectionProblem, SymbolProblem};
use crate::model::{FileKind, MAX_ITEMS, Object, SectionId, SymbolId};
use crate::reloc::RelocationTarget;
use crate::section::{Section, SectionFlags};
use crate::symbol::{Binding, SymbolKind, SymbolSection};

pub(crate) fn validate(obj: &Object) -> Result<(), ModelError> {
    let sections = obj.sections();
    let symbols = obj.symbols();
    if sections.len() > MAX_ITEMS || symbols.len() > MAX_ITEMS {
        return Err(ModelError::TooManyItems);
    }
    let executable = obj.kind() == FileKind::Executable;

    for (index, section) in sections.iter().enumerate() {
        let id = SectionId(index_u32(index));
        check_section(section).map_err(|problem| ModelError::Section {
            section: id,
            problem,
        })?;
        for (r, reloc) in section.relocations().iter().enumerate() {
            let problem = if executable {
                Some(RelocationProblem::InExecutable)
            } else {
                check_relocation(obj, section, reloc)
            };
            if let Some(problem) = problem {
                return Err(ModelError::Relocation {
                    section: id,
                    index: r,
                    problem,
                });
            }
        }
    }

    for (index, symbol) in symbols.iter().enumerate() {
        let id = SymbolId(index_u32(index));
        check_symbol(obj, symbol).map_err(|problem| ModelError::Symbol {
            symbol: id,
            problem,
        })?;
    }

    if executable {
        check_executable_layout(obj)?;
    }
    Ok(())
}

/// Index conversion for counts already checked against `MAX_ITEMS`.
fn index_u32(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

fn check_section(section: &Section) -> Result<(), SectionProblem> {
    if section.name().as_bytes().contains(&0) {
        return Err(SectionProblem::NameContainsNul);
    }
    if !section.align().is_power_of_two() {
        return Err(SectionProblem::BadAlignment);
    }
    let flags = section.flags();
    if !section.kind().allows(flags) {
        return Err(SectionProblem::FlagsDoNotMatchKind);
    }
    if flags.contains(SectionFlags::MERGE) && section.entry_size() == 0 {
        return Err(SectionProblem::MergeWithoutEntrySize);
    }
    let contents_ok = if section.kind().is_uninitialized() {
        section.data().is_empty()
    } else {
        section.bss_size() == 0
    };
    if !contents_ok {
        return Err(SectionProblem::ContentsDoNotMatchKind);
    }
    if section.address() & (section.align() - 1) != 0 {
        return Err(SectionProblem::MisalignedAddress);
    }
    if section.address().checked_add(section.size()).is_none() {
        return Err(SectionProblem::AddressOverflow);
    }
    Ok(())
}

fn check_relocation(
    obj: &Object,
    section: &Section,
    reloc: &crate::reloc::Relocation,
) -> Option<RelocationProblem> {
    if reloc.kind.architecture() != obj.architecture() {
        return Some(RelocationProblem::WrongArchitecture);
    }
    if section.kind().is_uninitialized() {
        return Some(RelocationProblem::InUninitializedSection);
    }
    match reloc.offset.checked_add(reloc.kind.size()) {
        Some(end) if end <= section.size() => {}
        _ => return Some(RelocationProblem::OutOfSection),
    }
    if reloc.kind.is_instruction() && reloc.offset % 4 != 0 {
        return Some(RelocationProblem::Misaligned);
    }
    let target_exists = match reloc.target {
        RelocationTarget::Symbol(id) => id.index() < obj.symbols().len(),
        RelocationTarget::Section(id) => id.index() < obj.sections().len(),
    };
    if !target_exists {
        return Some(RelocationProblem::UnknownTarget);
    }
    None
}

fn check_symbol(obj: &Object, symbol: &crate::symbol::Symbol) -> Result<(), SymbolProblem> {
    if symbol.name.as_bytes().contains(&0) {
        return Err(SymbolProblem::NameContainsNul);
    }
    let executable = obj.kind() == FileKind::Executable;
    if symbol.kind == SymbolKind::File
        && (symbol.binding != Binding::Local
            || symbol.section != SymbolSection::Absolute
            || symbol.value != 0
            || symbol.size != 0)
    {
        return Err(SymbolProblem::BadFileSymbol);
    }
    match symbol.section {
        SymbolSection::Undefined => {
            if symbol.binding == Binding::Local {
                return Err(SymbolProblem::LocalUndefined);
            }
            if executable && symbol.binding != Binding::Weak {
                return Err(SymbolProblem::UndefinedInExecutable);
            }
        }
        SymbolSection::Common => {
            if symbol.binding == Binding::Local {
                return Err(SymbolProblem::LocalUndefined);
            }
            if executable {
                return Err(SymbolProblem::CommonInExecutable);
            }
            if symbol.binding != Binding::Global || !symbol.value.is_power_of_two() {
                return Err(SymbolProblem::BadCommonSymbol);
            }
        }
        SymbolSection::Absolute => {}
        SymbolSection::Section(id) => {
            let section = obj
                .sections()
                .get(id.index())
                .ok_or(SymbolProblem::UnknownSection)?;
            // Relocatable objects measure symbols from the section start; executables
            // give addresses, so the section occupies [address, address + size).
            let start = if executable { section.address() } else { 0 };
            let end = start
                .checked_add(section.size())
                .ok_or(SymbolProblem::OutOfSection)?;
            let symbol_end = symbol
                .value
                .checked_add(symbol.size)
                .ok_or(SymbolProblem::OutOfSection)?;
            if symbol.value < start || symbol_end > end {
                return Err(SymbolProblem::OutOfSection);
            }
            if symbol.kind == SymbolKind::Tls && !section.flags().contains(SectionFlags::TLS) {
                return Err(SymbolProblem::TlsOutsideTlsSection);
            }
        }
    }
    Ok(())
}

/// Executables: loadable sections may not overlap, and the entry point must be inside
/// an executable one.
fn check_executable_layout(obj: &Object) -> Result<(), ModelError> {
    let sections = obj.sections();
    let mut loaded: Vec<u32> = (0..sections.len())
        .filter(|&i| {
            sections
                .get(i)
                .is_some_and(|s| s.flags().contains(SectionFlags::ALLOC) && s.size() > 0)
        })
        .map(index_u32)
        .collect();
    loaded.sort_unstable_by_key(|&i| (sections.get(i as usize).map_or(0, Section::address), i));

    let mut previous: Option<(u32, u64)> = None;
    for &i in &loaded {
        let Some(section) = sections.get(i as usize) else {
            continue;
        };
        if let Some((prev, prev_end)) = previous {
            if section.address() < prev_end {
                return Err(ModelError::SectionsOverlap {
                    first: SectionId(prev),
                    second: SectionId(i),
                });
            }
        }
        // `check_section` has already ruled out overflow here.
        let end = section.address().saturating_add(section.size());
        previous = Some((i, end));
    }

    let entry = obj.entry();
    let in_code = sections.iter().any(|s| {
        s.flags().contains(SectionFlags::ALLOC | SectionFlags::EXEC)
            && entry >= s.address()
            && entry - s.address() < s.size()
    });
    if !in_code {
        return Err(ModelError::EntryNotExecutable { entry });
    }
    Ok(())
}
