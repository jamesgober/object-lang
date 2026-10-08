//! Budgets that bound the work and memory a reader spends on untrusted input.

/// Budgets for reading untrusted files.
///
/// Every reader checks its input against these before allocating for it. Section data,
/// symbol records, and relocation records are also bounded by the input itself (a
/// reader copies at most as many bytes of section data as the file holds, however its
/// headers overlap), so the budgets that matter most are the ones on names: a hostile
/// file can point many symbols at one long string, and only `max_name_bytes` stops that
/// from multiplying.
///
/// The defaults sit far above what compilers produce and far below what would exhaust
/// a machine. The struct is `#[non_exhaustive]`: start from [`Limits::default`] and
/// change fields.
///
/// | Field | Default | Bounds |
/// |---|---|---|
/// | `max_sections` | 1,048,576 | section headers |
/// | `max_symbols` | 16,777,216 | symbol table entries |
/// | `max_relocations` | 67,108,864 | relocation records, all sections together |
/// | `max_name_len` | 65,536 bytes | one section or symbol name |
/// | `max_name_bytes` | 268,435,456 bytes (256 MiB) | all names together, as copied out |
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, Limits, Object, ReadError, Section, SectionKind};
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// obj.add_section(Section::new(".text", SectionKind::Text));
/// obj.add_section(Section::new(".data", SectionKind::Data));
/// let bytes = object_lang::elf::write(&obj)?;
///
/// let mut limits = Limits::default();
/// limits.max_sections = 3;
/// assert_eq!(
///     object_lang::elf::read_with_limits(&bytes, &limits),
///     Err(ReadError::LimitExceeded { limit: "max_sections" }),
/// );
/// # Ok::<(), object_lang::WriteError>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub struct Limits {
    /// The most section headers a file may have.
    pub max_sections: u32,
    /// The most entries a symbol table may have.
    pub max_symbols: u32,
    /// The most relocation records, over all sections.
    pub max_relocations: u64,
    /// The longest a single name may be, in bytes.
    pub max_name_len: u32,
    /// The most name bytes copied out of the file, over all names.
    pub max_name_bytes: u64,
}

impl Limits {
    /// The default budgets, as a constant.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::Limits;
    ///
    /// assert_eq!(Limits::DEFAULT, Limits::default());
    /// ```
    pub const DEFAULT: Limits = Limits {
        max_sections: 1 << 20,
        max_symbols: 1 << 24,
        max_relocations: 1 << 26,
        max_name_len: 1 << 16,
        max_name_bytes: 1 << 28,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}
