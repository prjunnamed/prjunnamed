//! Metadata storage.
//!
//! This module is intentionally tightly coupled to `crate::design`, and intentionally avoids exporting any APIs that
//! expose indices. The exported APIs are based on [`MetaStringRef`] and [`MetaItemRef`], which parallel [`CellRef`].
//!
//! [`CellRef`]: crate::CellRef
use std::{
    borrow::Cow,
    cell::Ref,
    collections::BTreeSet,
    fmt::{Debug, Display},
    hash::Hash,
};
use indexmap::IndexSet;

use crate::{Design, ParamValue};

/// Position within a source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePosition {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based column number.
    pub column: u32,
}

/// Metadata item.
///
/// Metadata items represent information about cells that does not affect computational semantics. Some metadata items
/// only carry information about the source code, and other metadata items affect how the netlist is transformed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum MetaItem<'a> {
    /// Absence of a metadata item.
    /// The purpose of this variant is to make metadata indices take less memory.
    #[default]
    None,
    /// Multiple metadata items.
    /// The purpose of this variant is to make collections of metadata indices take less memory.
    /// A metadata set cannot contain less than two metadata items.
    Set(BTreeSet<MetaItemRef<'a>>),
    /// Source location.
    Source {
        /// Filename.  Must not be empty.
        file: MetaStringRef<'a>,
        /// Start of the range (inclusive).
        start: SourcePosition,
        /// End of the range (exclusive).
        end: SourcePosition,
    },
    /// Scope identified by a name.
    /// A top-level named scope could be a module declaration. A named scope with a parent could be a block within
    /// a module.
    NamedScope {
        /// Name.  Must not be empty.
        name: MetaStringRef<'a>,
        /// Source location.  Must reference [`MetaItem::None`] or [`MetaItem::Source`].
        source: MetaItemRef<'a>,
        /// Parent scope.  Must reference [`MetaItem::None`], [`MetaItem::NamedScope`], or [`MetaItem::IndexedScope`].
        parent: MetaItemRef<'a>,
    },
    /// Scope identified by an index.
    /// A top-level named scope could be a module declaration. A named scope with a parent could be a named instance
    /// of a module within another module.
    IndexedScope {
        index: i32,
        /// Source location.  Must reference [`MetaItem::None`] or [`MetaItem::Source`].
        source: MetaItemRef<'a>,
        /// Parent scope.  Must reference [`MetaItem::None`], [`MetaItem::NamedScope`], or [`MetaItem::IndexedScope`].
        parent: MetaItemRef<'a>,
    },
    /// Identifier within source code.
    Ident {
        /// Name.  Must not be empty.
        name: MetaStringRef<'a>,
        /// Containing scope.
        /// Must reference a [`MetaItem::NamedScope`], or [`MetaItem::IndexedScope`].
        scope: MetaItemRef<'a>,
    },
    Attr {
        /// Name.  Must not be empty.
        name: MetaStringRef<'a>,
        /// Value.
        value: ParamValue,
    },
    // to be added:
    // - MemoryKind
    // - EnumEncoding
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub(crate) struct MetaStringIndex(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub(crate) struct MetaItemIndex(usize);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
enum MetaItemRepr {
    #[default]
    None,
    Set(Vec<MetaItemIndex>),
    Source {
        file: MetaStringIndex,
        start: SourcePosition,
        end: SourcePosition,
    },
    NamedScope {
        name: MetaStringIndex,
        source: MetaItemIndex,
        parent: MetaItemIndex,
    },
    IndexedScope {
        index: i32,
        source: MetaItemIndex,
        parent: MetaItemIndex,
    },
    Ident {
        name: MetaStringIndex,
        scope: MetaItemIndex,
    },
    Attr {
        name: MetaStringIndex,
        value: ParamValue,
    },
}

#[derive(Clone, Debug)]
pub struct MetadataStore {
    strings: IndexSet<String>,
    items: IndexSet<MetaItemRepr>,
}

#[derive(Clone, Copy)]
pub struct MetaStringRef<'a> {
    design: &'a Design,
    index: MetaStringIndex,
}

#[derive(Clone, Copy)]
pub struct MetaItemRef<'a> {
    design: &'a Design,
    index: MetaItemIndex,
}

pub struct MetaItemIterator<'a> {
    item: MetaItemRef<'a>,
    offset: usize,
}

impl MetaItem<'_> {
    pub fn validate(&self) {
        match self {
            MetaItem::None => (),
            MetaItem::Set(items) => {
                assert!(items.len() > 1, "MetaItem::Set must contain more than one element");
                for item in items {
                    assert!(
                        !matches!(&*item.get_repr(), MetaItemRepr::None | MetaItemRepr::Set(..)),
                        "MetaItem::Set item must not be MetaItem::None or another MetaItem::Set"
                    )
                }
            }
            MetaItem::Source { file, start, end } => {
                assert!(!file.get().is_empty(), "MetaItem::Source must have a file");
                assert!(
                    end.line > start.line || (end.line == start.line && end.column >= start.column),
                    "MetaItem::Source must specify a range in non-descending order"
                );
            }
            MetaItem::NamedScope { name: _, source, parent } | MetaItem::IndexedScope { index: _, source, parent } => {
                if let MetaItem::NamedScope { name, .. } = self {
                    assert!(!name.get().is_empty(), "MetaItem::NamedScope must have a name");
                }
                assert!(
                    matches!(&*source.get_repr(), MetaItemRepr::None | MetaItemRepr::Source { .. }),
                    concat!(
                        "source of a MetaItem::NamedScope or MetaItem::IndexedScope must be ",
                        "MetaItem::None or MetaItem::Source"
                    )
                );
                assert!(
                    matches!(
                        &*parent.get_repr(),
                        MetaItemRepr::None | MetaItemRepr::NamedScope { .. } | MetaItemRepr::IndexedScope { .. }
                    ),
                    concat!(
                        "parent of a MetaItem::NamedScope or MetaItem::IndexedScope must be ",
                        "MetaItem::None, MetaItem::NamedScope, or MetaItem::IndexedScope"
                    )
                );
            }
            MetaItem::Ident { name, scope } => {
                assert!(!name.get().is_empty(), "MetaItem::Ident must have a name");
                assert!(
                    matches!(&*scope.get_repr(), MetaItemRepr::NamedScope { .. } | MetaItemRepr::IndexedScope { .. }),
                    "scope of a MetaItem::Ident must be MetaItem::NamedScope or MetaItem::IndexedScope"
                );
            }
            MetaItem::Attr { name, value: _ } => {
                assert!(!name.get().is_empty(), "MetaItem::Attr must have a name");
            }
        }
    }
}

impl MetaStringIndex {
    pub const EMPTY: MetaStringIndex = MetaStringIndex(0);
}

impl MetaItemIndex {
    pub const NONE: MetaItemIndex = MetaItemIndex(0);
}

impl MetadataStore {
    pub(crate) fn new() -> Self {
        Self { strings: IndexSet::from(["".to_owned()]), items: IndexSet::from([MetaItemRepr::None]) }
    }

    pub(crate) fn add_string<'a>(&mut self, string: impl Into<Cow<'a, str>>) -> MetaStringIndex {
        let string = string.into();
        match self.strings.get_index_of(string.as_ref()) {
            Some(index) => MetaStringIndex(index),
            None => MetaStringIndex(self.strings.insert_full(string.into()).0),
        }
    }

    pub(crate) fn ref_string<'a>(&self, design: &'a Design, index: MetaStringIndex) -> MetaStringRef<'a> {
        MetaStringRef { design, index }
    }

    pub(crate) fn add_item(&mut self, item: &MetaItem) -> MetaItemIndex {
        // The item must be validated before adding; however it cannot be done here because `.validate()` would need
        // to reborrow the store.
        let repr = match item {
            MetaItem::None => MetaItemRepr::None,
            MetaItem::Set(items) => MetaItemRepr::Set(items.iter().map(|item| item.index).collect()),
            MetaItem::Source { file, start, end } => {
                MetaItemRepr::Source { file: file.index, start: *start, end: *end }
            }
            MetaItem::NamedScope { name, source, parent } => {
                MetaItemRepr::NamedScope { parent: parent.index, source: source.index, name: name.index }
            }
            MetaItem::IndexedScope { index, source, parent } => {
                MetaItemRepr::IndexedScope { parent: parent.index, source: source.index, index: *index }
            }
            MetaItem::Ident { name, scope } => MetaItemRepr::Ident { scope: scope.index, name: name.index },
            MetaItem::Attr { name, value } => MetaItemRepr::Attr { name: name.index, value: value.clone() },
        };
        MetaItemIndex(self.items.insert_full(repr).0)
    }

    pub(crate) fn ref_item<'a>(&self, design: &'a Design, index: MetaItemIndex) -> MetaItemRef<'a> {
        MetaItemRef { design, index }
    }

    pub(crate) fn iter_items<'a>(&self, design: &'a Design) -> impl Iterator<Item = MetaItemRef<'a>> + use<'a> {
        (0..self.items.len()).map(|index| MetaItemRef { design, index: MetaItemIndex(index) })
    }
}

impl<'a> MetaStringRef<'a> {
    pub(crate) fn index(&self) -> MetaStringIndex {
        self.index
    }

    pub fn is_empty(&self) -> bool {
        self.index == MetaStringIndex::EMPTY
    }

    pub fn get(&self) -> Ref<'a, str> {
        Ref::map(self.design.metadata(), |store| {
            store.strings.get_index(self.index.0).expect("invalid metadata string reference").as_str()
        })
    }
}

impl<'a> MetaItemRef<'a> {
    pub(crate) fn index(&self) -> MetaItemIndex {
        self.index
    }

    pub fn is_none(&self) -> bool {
        self.index == MetaItemIndex::NONE
    }

    fn get_repr(&self) -> Ref<'a, MetaItemRepr> {
        Ref::map(self.design.metadata(), |store| {
            store.items.get_index(self.index.0).expect("invalid metadata item reference")
        })
    }

    pub fn get(&self) -> MetaItem<'a> {
        let design = self.design;
        match &*self.get_repr() {
            MetaItemRepr::None => MetaItem::None,
            MetaItemRepr::Set(items) => {
                MetaItem::Set(items.iter().map(|&index| MetaItemRef { index, design }).collect())
            }
            MetaItemRepr::Source { file: filename, start, end } => {
                MetaItem::Source { file: MetaStringRef { index: *filename, design }, start: *start, end: *end }
            }
            MetaItemRepr::NamedScope { name, source, parent } => MetaItem::NamedScope {
                name: MetaStringRef { index: *name, design },
                source: MetaItemRef { index: *source, design },
                parent: MetaItemRef { index: *parent, design },
            },
            MetaItemRepr::IndexedScope { index, source, parent } => MetaItem::IndexedScope {
                index: *index,
                source: MetaItemRef { index: *source, design },
                parent: MetaItemRef { index: *parent, design },
            },
            MetaItemRepr::Ident { name, scope } => MetaItem::Ident {
                name: MetaStringRef { index: *name, design },
                scope: MetaItemRef { index: *scope, design },
            },
            MetaItemRepr::Attr { name, value } => {
                MetaItem::Attr { name: MetaStringRef { index: *name, design }, value: value.clone() }
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = MetaItemRef<'a>> + use<'a> {
        MetaItemIterator { item: *self, offset: 0 }
    }

    pub fn from_iter(design: &'a Design, iter: impl IntoIterator<Item = MetaItemRef<'a>>) -> Self {
        let items = BTreeSet::from_iter(iter);
        if items.len() == 0 {
            MetaItemRef { design, index: MetaItemIndex::NONE }
        } else if items.len() == 1 {
            *items.first().unwrap()
        } else {
            MetaItemRef { design, index: design.add_metadata_item(&MetaItem::Set(items)).index() }
        }
    }

    pub fn from_merge(design: &'a Design, iter: impl IntoIterator<Item = MetaItemRef<'a>>) -> Self {
        Self::from_iter(
            design,
            iter.into_iter().flat_map(|item| item.iter()).filter(|item| match &*item.get_repr() {
                MetaItemRepr::Source { .. }
                | MetaItemRepr::NamedScope { .. }
                | MetaItemRepr::IndexedScope { .. }
                | MetaItemRepr::Ident { .. } => true,
                _ => false,
            }),
        )
    }

    pub fn merge(&self, other: MetaItemRef<'a>) -> Self {
        Self::from_merge(&self.design, [*self, other])
    }
}

impl<'a> Iterator for MetaItemIterator<'a> {
    type Item = MetaItemRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let repr = self.item.get_repr();
        let slice = match &*repr {
            MetaItemRepr::None => &[][..],
            MetaItemRepr::Set(items) => &items[..],
            _ => std::slice::from_ref(&self.item.index),
        };
        if self.offset < slice.len() {
            let item = MetaItemRef { design: self.item.design, index: slice[self.offset] };
            self.offset += 1;
            Some(item)
        } else {
            None
        }
    }
}

impl Display for MetaItemIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let index = self.0;
        assert!(index > 0);
        write!(f, "!{}", index - 1)
    }
}

impl Debug for MetaStringRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.get())
    }
}

impl PartialEq<MetaStringRef<'_>> for MetaStringRef<'_> {
    fn eq(&self, other: &MetaStringRef<'_>) -> bool {
        std::ptr::eq(self.design, other.design) && self.index == other.index
    }
}

impl Eq for MetaStringRef<'_> {}

impl PartialOrd<MetaStringRef<'_>> for MetaStringRef<'_> {
    fn partial_cmp(&self, other: &MetaStringRef<'_>) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MetaStringRef<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.design as *const Design).cmp(&(other.design as *const Design)) {
            core::cmp::Ordering::Equal => self.index.cmp(&other.index),
            ord => ord,
        }
    }
}

impl Hash for MetaStringRef<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl Debug for MetaItemRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.get())
    }
}

impl PartialEq<MetaItemRef<'_>> for MetaItemRef<'_> {
    fn eq(&self, other: &MetaItemRef<'_>) -> bool {
        std::ptr::eq(self.design, other.design) && self.index == other.index
    }
}

impl Eq for MetaItemRef<'_> {}

impl PartialOrd<MetaItemRef<'_>> for MetaItemRef<'_> {
    fn partial_cmp(&self, other: &MetaItemRef<'_>) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MetaItemRef<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.design as *const Design).cmp(&(other.design as *const Design)) {
            core::cmp::Ordering::Equal => self.index.cmp(&other.index),
            ord => ord,
        }
    }
}

impl Hash for MetaItemRef<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}
