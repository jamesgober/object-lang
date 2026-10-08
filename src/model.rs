//! The format-neutral object model: an [`Object`] is a target, a file kind, sections,
//! and symbols. Every writer takes one and every reader returns one.

use alloc::vec::Vec;

use crate::error::ModelError;
use crate::reloc::Relocation;
use crate::section::Section;
use crate::symbol::Symbol;

/// The instruction set an object is built for.
///
/// The architecture decides which [relocation kinds](crate::RelocationKind) an object may
/// carry and the page size an executable is laid out with.
///
/// # Examples
///
/// ```
/// use object_lang::Architecture;
///
/// assert_eq!(Architecture::X86_64.page_size(), 0x1000);
/// assert_eq!(Architecture::Aarch64.page_size(), 0x1_0000);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum Architecture {
    /// 64-bit x86 (AMD64, Intel 64).
    X86_64,
    /// 64-bit Arm (AArch64, ARMv8-A and later).
    Aarch64,
}

impl Architecture {
    /// The page size an executable for this architecture is laid out with: the boundary
    /// at which loadable segments start, and the alignment of each segment.
    ///
    /// AArch64 kernels run with 4 KiB, 16 KiB, or 64 KiB pages, so AArch64 uses 64 KiB,
    /// which is correct on all three (and is what GNU ld and LLVM lld default to). x86-64
    /// uses 4 KiB. A linker that places sections for an executable must start each
    /// change of permissions on a fresh page of this size.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::Architecture;
    ///
    /// let page = Architecture::X86_64.page_size();
    /// assert!(page.is_power_of_two());
    /// ```
    #[must_use]
    pub const fn page_size(self) -> u64 {
        match self {
            Architecture::X86_64 => 0x1000,
            Architecture::Aarch64 => 0x1_0000,
        }
    }
}

/// What a file is for: input to a linker, or a program the OS can run.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, FileKind, Object};
///
/// let obj = Object::new(FileKind::Relocatable, Architecture::X86_64);
/// assert_eq!(obj.kind(), FileKind::Relocatable);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum FileKind {
    /// A relocatable object (`.o`, `.obj`): sections at no fixed address, symbol values
    /// as offsets within their section, and relocations still to be applied by a linker.
    Relocatable,
    /// A statically linked executable: every loadable section at its final address,
    /// symbol values as addresses, no relocations, and an entry point.
    Executable,
}

/// The identity of a section within its [`Object`]: its position in
/// [`Object::sections`].
///
/// Ids are dense, starting at zero in the order sections were added, so they index
/// straight into the section list. An id is only meaningful for the object that issued it.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, Object, Section, SectionKind};
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// let text = obj.add_section(Section::new(".text", SectionKind::Text));
/// let data = obj.add_section(Section::new(".data", SectionKind::Data));
/// assert_eq!((text.index(), data.index()), (0, 1));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct SectionId(pub(crate) u32);

impl SectionId {
    /// The id of the section at position `index` in [`Object::sections`].
    ///
    /// An id is only an index: one that names no section of the object it is used with
    /// is caught by [`Object::validate`] (or refused by [`Object::section`]), never
    /// trusted.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionId, SectionKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let text = obj.add_section(Section::new(".text", SectionKind::Text));
    /// assert_eq!(SectionId::new(0), text);
    /// assert!(obj.section(SectionId::new(1)).is_none());
    /// ```
    #[must_use]
    pub const fn new(index: u32) -> Self {
        SectionId(index)
    }

    /// The position of the section in [`Object::sections`].
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::Aarch64);
    /// let id = obj.add_section(Section::new(".text", SectionKind::Text));
    /// assert_eq!(obj.sections()[id.index()].name(), ".text");
    /// ```
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The identity of a symbol within its [`Object`]: its position in [`Object::symbols`].
///
/// Ids are dense, starting at zero in the order symbols were added.
///
/// # Examples
///
/// ```
/// use object_lang::{Architecture, Object, Symbol};
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// let puts = obj.add_symbol(Symbol::undefined("puts"));
/// assert_eq!(obj.symbols()[puts.index()].name, "puts");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct SymbolId(pub(crate) u32);

impl SymbolId {
    /// The id of the symbol at position `index` in [`Object::symbols`]. As with
    /// [`SectionId::new`], an id that names no symbol is caught by [`Object::validate`].
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Symbol, SymbolId};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let exit = obj.add_symbol(Symbol::undefined("exit"));
    /// assert_eq!(SymbolId::new(0), exit);
    /// ```
    #[must_use]
    pub const fn new(index: u32) -> Self {
        SymbolId(index)
    }

    /// The position of the symbol in [`Object::symbols`].
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Symbol};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let first = obj.add_symbol(Symbol::undefined("a"));
    /// assert_eq!(first.index(), 0);
    /// ```
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The largest number of sections, or of symbols, one object may hold.
///
/// Ids are `u32`, and every format needs a few indices of its own on top of the
/// object's, so the model stops well short of `u32::MAX`.
pub(crate) const MAX_ITEMS: usize = 0x7fff_ffff;

/// One object file or executable, independent of the format it is written in.
///
/// An object is a [target](Architecture), a [kind](FileKind), a list of [`Section`]s
/// (each carrying its own [`Relocation`]s), and a list of [`Symbol`]s. Executables also
/// have an entry point. The same object can be written in any format the crate supports;
/// reading a file returns an object of the same shape.
///
/// Nothing is checked while an object is built. [`validate`](Object::validate) checks
/// every rule the model has, and every writer runs it first, so a malformed object is an
/// error value, never a malformed file.
///
/// # Examples
///
/// A relocatable object with a function that calls an external one:
///
/// ```
/// use object_lang::{
///     Architecture, Object, Relocation, RelocationTarget, Section, SectionKind, Symbol,
///     X86_64Reloc,
/// };
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// // call puts; ret
/// let text = obj.add_section(
///     Section::new(".text", SectionKind::Text).with_data(vec![0xe8, 0, 0, 0, 0, 0xc3]),
/// );
/// obj.add_symbol(Symbol::function("main", text, 0, 6));
/// let puts = obj.add_symbol(Symbol::undefined("puts"));
/// obj.add_relocation(
///     text,
///     Relocation::new(1, X86_64Reloc::Plt32, RelocationTarget::Symbol(puts), -4),
/// )?;
///
/// let bytes = object_lang::elf::write(&obj)?;
/// let back = object_lang::elf::read(&bytes)?;
/// assert_eq!(back.symbols().len(), 2);
/// assert_eq!(back.sections()[0].relocations().len(), 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Object {
    kind: FileKind,
    arch: Architecture,
    entry: u64,
    executable_stack: bool,
    sections: Vec<Section>,
    symbols: Vec<Symbol>,
}

impl Object {
    /// Creates an empty object of the given kind for the given architecture.
    ///
    /// The entry point starts at zero and the stack is not executable.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, FileKind, Object};
    ///
    /// let exe = Object::new(FileKind::Executable, Architecture::Aarch64);
    /// assert_eq!(exe.architecture(), Architecture::Aarch64);
    /// assert!(exe.sections().is_empty());
    /// ```
    #[must_use]
    pub const fn new(kind: FileKind, arch: Architecture) -> Self {
        Object {
            kind,
            arch,
            entry: 0,
            executable_stack: false,
            sections: Vec::new(),
            symbols: Vec::new(),
        }
    }

    /// Creates an empty relocatable object: `Object::new(FileKind::Relocatable, arch)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, FileKind, Object};
    ///
    /// assert_eq!(Object::relocatable(Architecture::X86_64).kind(), FileKind::Relocatable);
    /// ```
    #[must_use]
    pub const fn relocatable(arch: Architecture) -> Self {
        Object::new(FileKind::Relocatable, arch)
    }

    /// Creates an empty executable: `Object::new(FileKind::Executable, arch)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, FileKind, Object};
    ///
    /// assert_eq!(Object::executable(Architecture::X86_64).kind(), FileKind::Executable);
    /// ```
    #[must_use]
    pub const fn executable(arch: Architecture) -> Self {
        Object::new(FileKind::Executable, arch)
    }

    /// What the file is for.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, FileKind, Object};
    ///
    /// assert_eq!(Object::executable(Architecture::X86_64).kind(), FileKind::Executable);
    /// ```
    #[must_use]
    pub const fn kind(&self) -> FileKind {
        self.kind
    }

    /// The instruction set the object is built for.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object};
    ///
    /// assert_eq!(Object::relocatable(Architecture::Aarch64).architecture(), Architecture::Aarch64);
    /// ```
    #[must_use]
    pub const fn architecture(&self) -> Architecture {
        self.arch
    }

    /// The address execution starts at. Meaningful for executables, where it must lie
    /// inside an executable section; relocatable objects normally leave it at zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object};
    ///
    /// let mut exe = Object::executable(Architecture::X86_64);
    /// exe.set_entry(0x40_1000);
    /// assert_eq!(exe.entry(), 0x40_1000);
    /// ```
    #[must_use]
    pub const fn entry(&self) -> u64 {
        self.entry
    }

    /// Sets the entry point address. See [`entry`](Object::entry).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object};
    ///
    /// let mut exe = Object::executable(Architecture::Aarch64);
    /// exe.set_entry(0x41_0000);
    /// assert_eq!(exe.entry(), 0x41_0000);
    /// ```
    pub fn set_entry(&mut self, address: u64) {
        self.entry = address;
    }

    /// Whether the program asks for an executable stack.
    ///
    /// Almost nothing should: it is off by default. ELF records the request as a
    /// `.note.GNU-stack` section in relocatable objects and a `PT_GNU_STACK` program
    /// header in executables; when a file read in carries neither, it is reported as
    /// `true`, because that is how the GNU toolchain and the Linux kernel treat a file
    /// that does not say.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object};
    ///
    /// let obj = Object::relocatable(Architecture::X86_64);
    /// assert!(!obj.executable_stack());
    /// ```
    #[must_use]
    pub const fn executable_stack(&self) -> bool {
        self.executable_stack
    }

    /// Sets whether the program asks for an executable stack. See
    /// [`executable_stack`](Object::executable_stack).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// obj.set_executable_stack(true);
    /// assert!(obj.executable_stack());
    /// ```
    pub fn set_executable_stack(&mut self, executable: bool) {
        self.executable_stack = executable;
    }

    /// Adds a section and returns its id. Sections keep the order they were added in.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let bss = obj.add_section(Section::new(".bss", SectionKind::Bss).with_bss_size(64));
    /// assert_eq!(obj.section(bss).map(|s| s.size()), Some(64));
    /// ```
    pub fn add_section(&mut self, section: Section) -> SectionId {
        // Ids beyond MAX_ITEMS saturate here and are refused by `validate`, which
        // checks the count; adding is infallible so building reads as a plain sequence.
        let id = SectionId(u32::try_from(self.sections.len()).unwrap_or(u32::MAX));
        self.sections.push(section);
        id
    }

    /// Every section, in the order added; a [`SectionId`] indexes this slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// obj.add_section(Section::new(".text", SectionKind::Text));
    /// assert_eq!(obj.sections()[0].name(), ".text");
    /// ```
    #[must_use]
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The section with this id, or `None` if the object has no such section.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let id = obj.add_section(Section::new(".rodata", SectionKind::ReadOnlyData));
    /// assert_eq!(obj.section(id).map(Section::name), Some(".rodata"));
    /// ```
    #[must_use]
    pub fn section(&self, id: SectionId) -> Option<&Section> {
        self.sections.get(id.index())
    }

    /// The section with this id, mutably, or `None` if the object has no such section.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let text = obj.add_section(Section::new(".text", SectionKind::Text));
    /// let offset = obj.section_mut(text).map(|s| s.append(&[0xc3], 16));
    /// assert_eq!(offset, Some(Ok(0)));
    /// ```
    #[must_use]
    pub fn section_mut(&mut self, id: SectionId) -> Option<&mut Section> {
        self.sections.get_mut(id.index())
    }

    /// The first section with this name, or `None`. A linear search: formats allow
    /// several sections to share a name, and this returns the earliest.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let data = obj.add_section(Section::new(".data", SectionKind::Data));
    /// assert_eq!(obj.section_by_name(".data"), Some(data));
    /// assert_eq!(obj.section_by_name(".bss"), None);
    /// ```
    #[must_use]
    pub fn section_by_name(&self, name: &str) -> Option<SectionId> {
        self.sections
            .iter()
            .position(|s| s.name() == name)
            .and_then(|i| u32::try_from(i).ok())
            .map(SectionId)
    }

    /// Adds a symbol and returns its id. Symbols keep the order they were added in;
    /// formats that need a different order (ELF puts local symbols first) reorder on
    /// write, so a file read back lists them in the format's order.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind, Symbol};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0xc3]));
    /// let f = obj.add_symbol(Symbol::function("f", text, 0, 1));
    /// assert_eq!(obj.symbol(f).map(|s| s.name.as_str()), Some("f"));
    /// ```
    pub fn add_symbol(&mut self, symbol: Symbol) -> SymbolId {
        let id = SymbolId(u32::try_from(self.symbols.len()).unwrap_or(u32::MAX));
        self.symbols.push(symbol);
        id
    }

    /// Every symbol, in the order added; a [`SymbolId`] indexes this slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Symbol};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// obj.add_symbol(Symbol::undefined("exit"));
    /// assert_eq!(obj.symbols()[0].name, "exit");
    /// ```
    #[must_use]
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// The symbol with this id, or `None` if the object has no such symbol.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Symbol};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let id = obj.add_symbol(Symbol::undefined("exit"));
    /// assert!(obj.symbol(id).is_some());
    /// ```
    #[must_use]
    pub fn symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id.index())
    }

    /// The symbol with this id, mutably, or `None` if the object has no such symbol.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Binding, Object, Symbol};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let id = obj.add_symbol(Symbol::undefined("hook"));
    /// if let Some(sym) = obj.symbol_mut(id) {
    ///     sym.binding = Binding::Weak;
    /// }
    /// assert_eq!(obj.symbols()[0].binding, Binding::Weak);
    /// ```
    #[must_use]
    pub fn symbol_mut(&mut self, id: SymbolId) -> Option<&mut Symbol> {
        self.symbols.get_mut(id.index())
    }

    /// The first symbol with this name, or `None`. A linear search.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Symbol};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let id = obj.add_symbol(Symbol::undefined("memcpy"));
    /// assert_eq!(obj.symbol_by_name("memcpy"), Some(id));
    /// ```
    #[must_use]
    pub fn symbol_by_name(&self, name: &str) -> Option<SymbolId> {
        self.symbols
            .iter()
            .position(|s| s.name == name)
            .and_then(|i| u32::try_from(i).ok())
            .map(SymbolId)
    }

    /// Adds a relocation to a section: shorthand for
    /// `object.section_mut(section)?.add_relocation(relocation)`.
    ///
    /// The relocation itself is checked by [`validate`](Object::validate), like
    /// everything else.
    ///
    /// # Errors
    ///
    /// [`ModelError::UnknownSection`] if the object has no section `section`.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{
    ///     Aarch64Reloc, Architecture, Object, Relocation, RelocationTarget, Section,
    ///     SectionKind, Symbol,
    /// };
    ///
    /// let mut obj = Object::relocatable(Architecture::Aarch64);
    /// // bl <callee>
    /// let text = obj.add_section(
    ///     Section::new(".text", SectionKind::Text).with_data(vec![0x00, 0x00, 0x00, 0x94]),
    /// );
    /// let callee = obj.add_symbol(Symbol::undefined("callee"));
    /// obj.add_relocation(
    ///     text,
    ///     Relocation::new(0, Aarch64Reloc::Call26, RelocationTarget::Symbol(callee), 0),
    /// )?;
    /// assert_eq!(obj.sections()[0].relocations().len(), 1);
    /// # Ok::<(), object_lang::ModelError>(())
    /// ```
    pub fn add_relocation(
        &mut self,
        section: SectionId,
        relocation: Relocation,
    ) -> Result<(), ModelError> {
        match self.sections.get_mut(section.index()) {
            Some(s) => {
                s.add_relocation(relocation);
                Ok(())
            }
            None => Err(ModelError::UnknownSection { section }),
        }
    }

    /// Checks every rule of the model, returning the first violation.
    ///
    /// Writers call this before writing anything, and readers call it on what they read,
    /// so an object that passes is one every writer can represent (barring
    /// format-specific limits, reported as [`WriteError`](crate::WriteError)). The rules
    /// are listed under [`ModelError`], [`SectionProblem`](crate::SectionProblem),
    /// [`SymbolProblem`](crate::SymbolProblem), and
    /// [`RelocationProblem`](crate::RelocationProblem).
    ///
    /// # Errors
    ///
    /// The first rule the object breaks, as a [`ModelError`].
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{
    ///     Architecture, ModelError, Object, Section, SectionKind, SectionProblem,
    /// };
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let text = obj.add_section(Section::new(".text", SectionKind::Text).with_align(3));
    /// assert_eq!(
    ///     obj.validate(),
    ///     Err(ModelError::Section { section: text, problem: SectionProblem::BadAlignment }),
    /// );
    /// ```
    pub fn validate(&self) -> Result<(), ModelError> {
        crate::validate::validate(self)
    }

    /// Builds an object from parts a reader has already checked structurally.
    pub(crate) fn from_parts(
        kind: FileKind,
        arch: Architecture,
        entry: u64,
        executable_stack: bool,
        sections: Vec<Section>,
        symbols: Vec<Symbol>,
    ) -> Self {
        Object {
            kind,
            arch,
            entry,
            executable_stack,
            sections,
            symbols,
        }
    }
}
