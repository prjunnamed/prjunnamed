use super::{bits::Bits, float::F64BitEq, StrId};

#[cfg(doc)]
use super::{
    cells::{Instance, Param, PortBus, PortIn, PortOut, UnresolvedInstance, Wire},
    CellRef, ModuleRef,
};

#[derive(Debug, Clone)]
pub enum DesignAnnotation {
    /// A user-defined attribute with unknown semantics.
    Attribute(Attribute),
    // TargetToolchain(StringId),
    // TargetFamily(StringId),
    // TargetDevice(StringId),
    // TargetPackage(StringId),
    // TargetGrade(StringId),
}

/// A [`ModuleRef`] annotation.
#[derive(Debug, Clone, PartialEq)]
pub enum ModuleAnnotation {
    /// The module name, directly from the source HDL.  It is not required to be unique, and often won't be — instantiations
    /// of the same module with distinct parameter values will share a name.
    ///
    /// Module instances are not bound by name, so this value is mostly unused in synthesis flows.  The exception is partial
    /// synthesis flows, where the top modules in a library will be bound by the upper-level design by name.
    Name(HierName),
    /// A user-defined attribute with unknown semantics.
    Attribute(Attribute),
    // /// Records a parameter value that has been used in elaboration to instantiate this module.  Such parameters can not
    // /// be changed post elaboration, and thus they do not have a corresponding [`Param`] cell.
    // BakedParameter(StrId, AttributeValue),
}

/// A [`CellRef`] annotation.
#[derive(Debug, Clone, PartialEq)]
pub enum CellAnnotation {
    /// The name of this cell, from the HDL.
    ///
    /// While nominally valid on most cell kinds, using it on unmapped cells is effectively meaningless and should be
    /// avoided, as such cells will generally be transformed during the synthesis process, losing the annotation.
    /// Cells where this annotation can be used effectively include:
    ///
    /// - [`Instance`] and [`UnresolvedInstance`] (if flattened, the name will be preserved by prepending it to the names of any named children)
    /// - [`Param`], [`PortIn`], [`PortOut`], [`PortBus`]
    /// - [`Wire`]
    /// - `Memory`
    /// - cells that are directly mapped to target primitives
    ///
    /// If you want to name the result of some combinatorial logic or a register, create a [`Wire`] referencing it
    /// and name it instead.
    Name(HierName),
    /// The position of this parameter or port cell in the HDL-level parameter or port list.
    ///
    /// This is used for positionally-bound parameters and ports.  Parameters are considered to be in a separate namespace than ports.
    /// All ports are in the same namespace, regardless of kind.
    Position(u32),
    /// A user-defined attribute with unknown semantics.
    Attribute(Attribute),
    /// Describes bit indexing used.  The first field determines whether bit indices go up from MSB to LSB ([`BitIndexingKind::Upto`])
    /// or down from MSB to LSB ([`BitIndexingKind::Downto`]).  The second field is the bit index of the LSB.
    ///
    /// The user-facing index of bit position `i` (counting from 0 = LSB) is:
    ///
    /// - `i + lsb_index` for `Downto`
    /// - `i - lsb_index` for `Upto`
    ///
    /// There must be at most one such annotation on a cell.
    BitIndexing(BitIndexingKind, i32),
}

/// A user-defined attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub key: StrId,
    pub val: AttributeValue,
}

/// The value of a user-defined attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeValue {
    String(StrId),
    Bits(Bits),
    Int(i32),
    Float(F64BitEq),
}

/// A hierarchical name.  Made from identifier and index components.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct HierName {
    pub chunks: Vec<HierNameChunk>,
}

/// A single component of [`HierName`].
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum HierNameChunk {
    String(StrId),
    Index(i32),
}

/// The bit indexing mode for [`CellAnnotation::BitIndexing`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BitIndexingKind {
    /// Indices go down from MSB to LSB (the usual convention).
    Downto,
    /// Indices go up from MSB to LSB.
    Upto,
}
