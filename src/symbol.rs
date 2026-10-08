//! Symbols: named addresses that objects define for others and refer to from others.

use alloc::string::String;

use crate::model::SectionId;

/// What a symbol names.
///
/// # Examples
///
/// ```
/// use object_lang::{SectionId, Symbol, SymbolKind};
///
/// assert_eq!(Symbol::undefined("puts").kind, SymbolKind::NoType);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum SymbolKind {
    /// Unspecified: a label, or a reference whose kind the object does not know.
    NoType,
    /// A function or other code entry point.
    Function,
    /// A data object: a variable, a constant, a table.
    Data,
    /// A thread-local variable; its value is an offset in the thread-local template.
    Tls,
    /// The name of the source file the object came from. Always local and
    /// [`Absolute`](SymbolSection::Absolute), with value and size zero.
    File,
}

/// Who can see a symbol: this object only, or every object in the link.
///
/// # Examples
///
/// ```
/// use object_lang::{Binding, Symbol};
///
/// assert_eq!(Symbol::undefined("f").binding, Binding::Global);
/// assert_eq!(Symbol::file("main.c").binding, Binding::Local);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum Binding {
    /// Visible only inside this object.
    Local,
    /// Visible to every object in the link; two global definitions of one name clash.
    Global,
    /// Like global, but yields to a global definition of the same name, and an
    /// undefined weak reference resolves to zero instead of failing the link.
    Weak,
}

/// How far a global symbol is exported beyond the module it is linked into.
///
/// # Examples
///
/// ```
/// use object_lang::{Symbol, Visibility};
///
/// let hidden = Symbol::undefined("helper").with_visibility(Visibility::Hidden);
/// assert_eq!(hidden.visibility, Visibility::Hidden);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum Visibility {
    /// Visibility follows the binding.
    Default,
    /// Not exported from the linked module (executable or shared library).
    Hidden,
    /// Exported, but references from inside the module always bind to this definition.
    Protected,
}

/// Where a symbol is defined.
///
/// # Examples
///
/// ```
/// use object_lang::{Symbol, SymbolSection};
///
/// assert_eq!(Symbol::undefined("x").section, SymbolSection::Undefined);
/// assert_eq!(Symbol::absolute("PAGE", 4096).section, SymbolSection::Absolute);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum SymbolSection {
    /// Not defined here: another object must define it (or, if weak, it may stay
    /// undefined and resolve to zero).
    Undefined,
    /// A constant value that relocation does not change.
    Absolute,
    /// A tentative definition (a C "common" symbol): the linker allocates `size` bytes
    /// of zeroed storage, aligned to the symbol's `value`, unless an object defines the
    /// name properly. Only in relocatable objects.
    Common,
    /// Defined in this section of the object.
    Section(SectionId),
}

/// One symbol: a name, what it names, where it is defined, and who can see it.
///
/// `value` is an offset within the symbol's section in a relocatable object, and an
/// address in an executable. For a [`Common`](SymbolSection::Common) symbol it is the
/// required alignment. `size` is the size in bytes of what the symbol names, or zero
/// when unknown.
///
/// The fields are public; the constructors cover the common shapes. The struct is
/// `#[non_exhaustive]`, so build one with a constructor and adjust fields or chain the
/// `with_*` methods.
///
/// # Examples
///
/// ```
/// use object_lang::{
///     Architecture, Binding, Object, Section, SectionKind, Symbol, SymbolKind, Visibility,
/// };
///
/// let mut obj = Object::relocatable(Architecture::X86_64);
/// let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0xc3; 8]));
/// let helper = Symbol::function("helper", text, 4, 4)
///     .with_binding(Binding::Local);
/// let api = Symbol::function("api", text, 0, 4).with_visibility(Visibility::Protected);
/// obj.add_symbol(helper);
/// obj.add_symbol(api);
/// assert!(obj.validate().is_ok());
/// assert_eq!(obj.symbols()[0].kind, SymbolKind::Function);
/// ```
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub struct Symbol {
    /// The symbol's name. Must not contain a NUL byte.
    pub name: String,
    /// What the symbol names.
    pub kind: SymbolKind,
    /// Who can see the symbol.
    pub binding: Binding,
    /// How far a non-local symbol is exported.
    pub visibility: Visibility,
    /// Where the symbol is defined.
    pub section: SymbolSection,
    /// The offset in the section (relocatable), the address (executable), or the
    /// alignment ([`Common`](SymbolSection::Common)).
    pub value: u64,
    /// The size in bytes of what the symbol names, or zero.
    pub size: u64,
}

impl Symbol {
    /// A symbol with every property given; the other constructors are shorthands.
    /// Visibility is [`Default`](Visibility::Default).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Binding, Symbol, SymbolKind, SymbolSection};
    ///
    /// let sym = Symbol::new("x", SymbolKind::Data, Binding::Weak, SymbolSection::Undefined, 0, 0);
    /// assert_eq!(sym.binding, Binding::Weak);
    /// ```
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        kind: SymbolKind,
        binding: Binding,
        section: SymbolSection,
        value: u64,
        size: u64,
    ) -> Self {
        Symbol {
            name: name.into(),
            kind,
            binding,
            visibility: Visibility::Default,
            section,
            value,
            size,
        }
    }

    /// A global function defined at `value` in `section`, `size` bytes long.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind, Symbol, SymbolKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let text = obj.add_section(Section::new(".text", SectionKind::Text).with_data(vec![0xc3]));
    /// let main = Symbol::function("main", text, 0, 1);
    /// assert_eq!(main.kind, SymbolKind::Function);
    /// ```
    #[must_use]
    pub fn function(name: impl Into<String>, section: SectionId, value: u64, size: u64) -> Self {
        Symbol::new(
            name,
            SymbolKind::Function,
            Binding::Global,
            SymbolSection::Section(section),
            value,
            size,
        )
    }

    /// A global data object defined at `value` in `section`, `size` bytes long.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Architecture, Object, Section, SectionKind, Symbol, SymbolKind};
    ///
    /// let mut obj = Object::relocatable(Architecture::X86_64);
    /// let data = obj.add_section(Section::new(".data", SectionKind::Data).with_data(vec![0; 4]));
    /// assert_eq!(Symbol::data("counter", data, 0, 4).kind, SymbolKind::Data);
    /// ```
    #[must_use]
    pub fn data(name: impl Into<String>, section: SectionId, value: u64, size: u64) -> Self {
        Symbol::new(
            name,
            SymbolKind::Data,
            Binding::Global,
            SymbolSection::Section(section),
            value,
            size,
        )
    }

    /// A global reference to a symbol another object defines.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Symbol, SymbolSection};
    ///
    /// assert_eq!(Symbol::undefined("malloc").section, SymbolSection::Undefined);
    /// ```
    #[must_use]
    pub fn undefined(name: impl Into<String>) -> Self {
        Symbol::new(
            name,
            SymbolKind::NoType,
            Binding::Global,
            SymbolSection::Undefined,
            0,
            0,
        )
    }

    /// A global constant `value` that relocation does not change.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::Symbol;
    ///
    /// assert_eq!(Symbol::absolute("STACK_SIZE", 0x10_0000).value, 0x10_0000);
    /// ```
    #[must_use]
    pub fn absolute(name: impl Into<String>, value: u64) -> Self {
        Symbol::new(
            name,
            SymbolKind::NoType,
            Binding::Global,
            SymbolSection::Absolute,
            value,
            0,
        )
    }

    /// A global [common](SymbolSection::Common) data symbol of `size` bytes aligned to
    /// `align` (a power of two).
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Symbol, SymbolSection};
    ///
    /// let buffer = Symbol::common("buffer", 256, 16);
    /// assert_eq!((buffer.section, buffer.value, buffer.size), (SymbolSection::Common, 16, 256));
    /// ```
    #[must_use]
    pub fn common(name: impl Into<String>, size: u64, align: u64) -> Self {
        Symbol::new(
            name,
            SymbolKind::Data,
            Binding::Global,
            SymbolSection::Common,
            align,
            size,
        )
    }

    /// A [`File`](SymbolKind::File) symbol naming the source file.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Binding, Symbol, SymbolKind};
    ///
    /// let file = Symbol::file("main.rs");
    /// assert_eq!((file.kind, file.binding), (SymbolKind::File, Binding::Local));
    /// ```
    #[must_use]
    pub fn file(name: impl Into<String>) -> Self {
        Symbol::new(
            name,
            SymbolKind::File,
            Binding::Local,
            SymbolSection::Absolute,
            0,
            0,
        )
    }

    /// `self` with a different binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Binding, Symbol};
    ///
    /// assert_eq!(Symbol::undefined("f").with_binding(Binding::Weak).binding, Binding::Weak);
    /// ```
    #[must_use]
    pub fn with_binding(mut self, binding: Binding) -> Self {
        self.binding = binding;
        self
    }

    /// `self` with a different visibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Symbol, Visibility};
    ///
    /// let s = Symbol::undefined("f").with_visibility(Visibility::Hidden);
    /// assert_eq!(s.visibility, Visibility::Hidden);
    /// ```
    #[must_use]
    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// `self` with a different kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use object_lang::{Symbol, SymbolKind};
    ///
    /// assert_eq!(Symbol::undefined("f").with_kind(SymbolKind::Function).kind, SymbolKind::Function);
    /// ```
    #[must_use]
    pub fn with_kind(mut self, kind: SymbolKind) -> Self {
        self.kind = kind;
        self
    }
}
