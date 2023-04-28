pub mod annotations;
pub mod bits;
pub mod cells;
pub mod float;

use std::collections::BTreeSet;

use annotations::{CellAnnotation, DesignAnnotation, ModuleAnnotation};

use delegate::delegate;
use enumflags2::{bitflags, BitFlags};
use prjunnamed_entity::{entity_id, EntityIds, EntitySet, EntityVec};
use thin_vec::ThinVec;

use self::cells::{CellKind, CellValSlot};

entity_id! {
    pub id ModuleId u32, reserve 1;
    pub id CellId u32, reserve 1;
    pub id StrId u32, reserve 1;
    pub id ParamId u32, reserve 1;
    pub id PortInId u32, reserve 1;
    pub id PortOutId u32, reserve 1;
    pub id PortBusId u32, reserve 1;
}

/// The top level structure of the IR, contains modules and top-level annotations.
#[derive(Debug, Clone, Default)]
pub struct Design {
    /// Modules of the design.  A [`None`] in this list is a tombstone, ie. a placeholder left in place of a deleted module.
    /// Tombstones are used so that [`ModuleId`] values stay stable, and are removed only by the GC pass.
    /// Module order has no semantic meaning.
    modules: EntityVec<ModuleId, Option<Module>>,
    /// Annotations regarding the whole design.  The order has no semantic meaning.
    annotations: ThinVec<DesignAnnotation>,
    /// An interning pool for all strings used in the IR.
    strings: EntitySet<StrId, String>,
}

impl Design {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn annotations(&self) -> &[DesignAnnotation] {
        &self.annotations
    }

    pub fn set_annotations(&mut self, anns: Vec<DesignAnnotation>) {
        self.annotations = anns.into();
    }

    pub fn add_annotation(&mut self, ann: DesignAnnotation) {
        self.annotations.push(ann);
    }

    pub fn module_ids(&self) -> EntityIds<ModuleId> {
        self.modules.ids()
    }

    pub fn module(&self, id: ModuleId) -> Option<ModuleRef> {
        self.modules.get(id)?.as_ref()?;
        Some(ModuleRef { design: self, id })
    }

    pub fn module_mut(&mut self, id: ModuleId) -> Option<ModuleRefMut> {
        self.modules.get(id)?.as_ref()?;
        Some(ModuleRefMut { design: self, id })
    }

    pub fn string(&self, id: StrId) -> &str {
        &self.strings[id]
    }

    pub fn intern(&mut self, s: &str) -> StrId {
        self.strings.get_or_insert(s)
    }

    pub fn add_module(&mut self) -> ModuleRefMut {
        let id = self.modules.push(Some(Module::default()));
        ModuleRefMut { design: self, id }
    }

    pub fn remove_module(&mut self, id: ModuleId) {
        if let Some(module) = self.modules[id].take() {
            for (cid, cell) in module.cells {
                if let CellKind::Instance(ref inst) = cell.contents {
                    if let Some(ref mut module) = self.modules[inst.module] {
                        module.uses.remove(&(id, cid));
                    }
                }
            }
        }
    }
}

/// A design module.  Is identified by the index in the [`Design`]'s `modules` attribute.
#[derive(Debug, Clone, Default)]
struct Module {
    /// Module flags (conceptually like annotations, but without parameters and stored as a bitfield).
    flags: BitFlags<ModuleFlag>,
    /// Module annotations.  The order has no semantic meaning.
    annotations: ThinVec<ModuleAnnotation>,
    /// Module cells.  The order has no semantic meaning.  Indexes in this table ([`CellId`] values) stay stable except
    /// for the GC pass.
    cells: EntityVec<CellId, Cell>,
    /// List of externally-supplied parameters that this module takes.  The parameters are represented as [`CellKind::Param`]
    /// and live in the main cell array, this array is just an index for quick access.  The order is important, as it matches
    /// the order in which parameters are supplied in module instantiation.
    params: EntityVec<ParamId, CellId>,
    /// List of input ports, similar to `params`.  The referenced cells must be [`CellKind::PortIn`].
    ports_in: EntityVec<PortInId, CellId>,
    /// List of output ports, similar to `params`.  The referenced cells must be [`CellKind::PortOut`].
    ports_out: EntityVec<PortOutId, CellId>,
    /// List of bus ports, similar to `params`.  The referenced cells must be [`CellKind::PortBus`].
    ports_bus: EntityVec<PortBusId, CellId>,
    uses: BTreeSet<(ModuleId, CellId)>,
}

#[derive(Copy, Clone, Debug)]
pub struct ModuleRef<'a> {
    design: &'a Design,
    id: ModuleId,
}

#[derive(Debug)]
pub struct ModuleRefMut<'a> {
    design: &'a mut Design,
    id: ModuleId,
}

macro_rules! impl_module_flag {
    ($get: ident, $flag: ident) => {
        pub fn $get(&self) -> bool {
            self.module().flags.contains(ModuleFlag::$flag)
        }
    };
}

macro_rules! impl_module_ports {
    ($get: ident, $typ: ident) => {
        pub fn $get(&self) -> &'a EntityVec<$typ, CellId> {
            &self.module().$get
        }
    };
}

impl<'a> ModuleRef<'a> {
    fn module(self) -> &'a Module {
        unsafe {
            self.design
                .modules
                .get_unchecked(self.id)
                .as_ref()
                .unwrap_unchecked()
        }
    }

    pub fn design(self) -> &'a Design {
        self.design
    }

    pub fn id(self) -> ModuleId {
        self.id
    }

    impl_module_flag!(keep, Keep);
    impl_module_flag!(no_merge, NoMerge);
    impl_module_flag!(no_flatten, NoFlatten);
    impl_module_flag!(inline, Inline);
    impl_module_flag!(blackbox, Blackbox);
    impl_module_flag!(top, Top);

    impl_module_ports!(params, ParamId);
    impl_module_ports!(ports_in, PortInId);
    impl_module_ports!(ports_out, PortOutId);
    impl_module_ports!(ports_bus, PortBusId);

    pub fn annotations(self) -> &'a [ModuleAnnotation] {
        &self.module().annotations
    }

    pub fn cell_ids(self) -> EntityIds<CellId> {
        self.module().cells.ids()
    }

    pub fn cells(self) -> impl Iterator<Item = CellRef<'a>> {
        self.module()
            .cells
            .ids()
            .map(move |id| CellRef { module: self, id })
    }

    pub fn cell(self, id: CellId) -> CellRef<'a> {
        self.module().cells.get(id).unwrap();
        CellRef { module: self, id }
    }

    pub fn uses(self) -> impl Iterator<Item = (ModuleId, CellId)> + 'a {
        self.module().uses.iter().copied()
    }

    delegate! {
        to self.design {
            pub fn string(self, id: StrId) -> &'a str;
        }
    }
}

macro_rules! impl_module_flag_mut {
    ($get: ident, $set: ident, $flag: ident) => {
        pub fn $get(&self) -> bool {
            self.as_ref().$get()
        }

        pub fn $set(&mut self, val: bool) {
            if val {
                self.module().flags.insert(ModuleFlag::$flag);
            } else {
                self.module().flags.remove(ModuleFlag::$flag);
            }
        }
    };
}

macro_rules! impl_module_ports_mut {
    ($get: ident, $set: ident, $typ: ident) => {
        pub fn $get(&self) -> &EntityVec<$typ, CellId> {
            self.as_ref().$get()
        }

        pub fn $set(&mut self, val: EntityVec<$typ, CellId>) {
            self.module().$get = val;
        }
    };
}

impl<'a> ModuleRefMut<'a> {
    fn module(&mut self) -> &mut Module {
        unsafe {
            self.design
                .modules
                .get_unchecked_mut(self.id)
                .as_mut()
                .unwrap_unchecked()
        }
    }

    pub fn as_ref(&self) -> ModuleRef {
        ModuleRef {
            design: self.design,
            id: self.id,
        }
    }

    pub fn reborrow(&mut self) -> ModuleRefMut {
        ModuleRefMut {
            design: self.design,
            id: self.id,
        }
    }

    pub fn design(&self) -> &Design {
        self.design
    }

    pub fn id(&self) -> ModuleId {
        self.id
    }

    impl_module_flag_mut!(keep, set_keep, Keep);
    impl_module_flag_mut!(no_merge, set_no_merge, NoMerge);
    impl_module_flag_mut!(no_flatten, set_no_flatten, NoFlatten);
    impl_module_flag_mut!(inline, set_inline, Inline);
    impl_module_flag_mut!(blackbox, set_blackbox, Blackbox);
    impl_module_flag_mut!(top, set_top, Top);

    pub fn set_annotations(&mut self, anns: Vec<ModuleAnnotation>) {
        self.module().annotations = anns.into()
    }

    pub fn add_annotation(&mut self, ann: ModuleAnnotation) {
        self.module().annotations.push(ann);
    }

    impl_module_ports_mut!(params, set_params, ParamId);
    impl_module_ports_mut!(ports_in, set_ports_in, PortInId);
    impl_module_ports_mut!(ports_out, set_ports_out, PortOutId);
    impl_module_ports_mut!(ports_bus, set_ports_bus, PortBusId);

    pub fn into_cell_mut(self, id: CellId) -> CellRefMut<'a> {
        self.as_ref().cell(id);
        CellRefMut { module: self, id }
    }

    pub fn cell_mut(&mut self, id: CellId) -> CellRefMut {
        self.reborrow().into_cell_mut(id)
    }

    pub fn add_void(&mut self) -> CellRefMut {
        let id = self.module().cells.push(Default::default());
        CellRefMut {
            module: ModuleRefMut {
                design: self.design,
                id: self.id,
            },
            id,
        }
    }

    delegate! {
        to self.as_ref() {
            pub fn annotations(&self) -> &[ModuleAnnotation];
            pub fn cell_ids(&self) -> EntityIds<CellId>;
            pub fn cell(&self, id: CellId) -> CellRef;
            pub fn cells(&self) -> impl Iterator<Item = CellRef>;
            pub fn uses(&self) -> impl Iterator<Item = (ModuleId, CellId)> + '_;
        }

        to self.design {
            pub fn string(&self, id: StrId) -> &str;
            pub fn intern(&mut self, s: &str) -> StrId;
        }
    }
}

/// A cell in a [`Module`].  Is indentified by the index in the `cells` attribute, of type [`CellId`].
///
/// Depending on the kind, a cell can be usable as a value and referenced in other cells.  If so, a cell has
/// a type and a plane it belongs to.  The planes (from the lowest) are:
///
/// - constant plane: the value of this cell is a constant determined at some (potentially later) stage of synthesis
/// - main (synthesizable) plane: this cell is to be synthesized, and it produces a value at runtime
/// - debug plane: this cell is not to be synthesized, and the value it produces is for debug info purposes only
///
/// The cells on any given plane can only refer to values on the same and lower planes.
///
/// The value types are:
///
/// - bitvec of known width (made of `0`, `1`, `x` bits); the width of a bitvec must fit in a 32-bit unsigned integer;
///   0-width bitvecs are valid (though they will likely be optimized out at the first opportunity)
/// - bitvec of unknown width
/// - integer (32-bit signed)
/// - float (64-bit IEEE 754)
/// - string (UTF-8)
///
/// Value types other than bitvec exist only on the constant plane.
#[derive(Debug, Clone, Default)]
struct Cell {
    /// Cell flags (like annotations, but without parameters and stored as a bitfield).  The set of valid flags
    /// depends on the cell kind.
    flags: BitFlags<CellFlag>,
    /// Cell annotations.  The set of valid annotation types depends on the cell kind.
    annotations: ThinVec<CellAnnotation>,
    /// The cell kind and kind-specific fields.
    contents: CellKind,
    uses: BTreeSet<(CellId, CellValSlot)>,
}

#[derive(Copy, Clone, Debug)]
pub struct CellRef<'a> {
    module: ModuleRef<'a>,
    id: CellId,
}

#[derive(Debug)]
pub struct CellRefMut<'a> {
    module: ModuleRefMut<'a>,
    id: CellId,
}

macro_rules! impl_cell_flag {
    ($get: ident, $flag: ident) => {
        pub fn $get(&self) -> bool {
            self.cell().flags.contains(CellFlag::$flag)
        }
    };
}

impl<'a> CellRef<'a> {
    fn cell(self) -> &'a Cell {
        let module = self.module.module();
        unsafe { module.cells.get_unchecked(self.id) }
    }

    pub fn design(self) -> &'a Design {
        self.module.design()
    }

    pub fn id(self) -> CellId {
        self.id
    }

    pub fn module(self) -> ModuleRef<'a> {
        self.module
    }

    impl_cell_flag!(keep, Keep);
    impl_cell_flag!(no_merge, NoMerge);
    impl_cell_flag!(no_flatten, NoFlatten);
    impl_cell_flag!(async_, Async);
    impl_cell_flag!(lax_x, LaxX);

    pub fn flags_plane(self) -> CellPlane {
        if self.cell().flags.contains(CellFlag::Param) {
            CellPlane::Param
        } else if self.cell().flags.contains(CellFlag::Debug) {
            CellPlane::Debug
        } else {
            CellPlane::Main
        }
    }

    pub fn annotations(self) -> &'a [CellAnnotation] {
        &self.cell().annotations
    }

    pub fn contents(self) -> &'a CellKind {
        &self.cell().contents
    }

    pub fn sibling(&self, cell: CellId) -> CellRef<'a> {
        self.module().cell(cell)
    }

    pub fn uses(self) -> impl Iterator<Item = (CellId, CellValSlot)> + 'a {
        self.cell().uses.iter().copied()
    }

    delegate! {
        to self.module.design {
            pub fn string(self, id: StrId) -> &'a str;
        }
    }
}

macro_rules! impl_cell_flag_mut {
    ($get: ident, $set: ident, $flag: ident) => {
        pub fn $get(&self) -> bool {
            self.as_ref().$get()
        }

        pub fn $set(&mut self, val: bool) {
            if val {
                self.cell().flags.insert(CellFlag::$flag);
            } else {
                self.cell().flags.remove(CellFlag::$flag);
            }
        }
    };
}

impl CellRefMut<'_> {
    fn cell(&mut self) -> &mut Cell {
        let module = self.module.module();
        unsafe { module.cells.get_unchecked_mut(self.id) }
    }

    pub fn design(&self) -> &Design {
        self.module.design()
    }

    pub fn id(&self) -> CellId {
        self.id
    }

    pub fn module(&self) -> ModuleRef {
        self.module.as_ref()
    }

    pub fn module_mut(&mut self) -> ModuleRefMut {
        self.module.reborrow()
    }

    pub fn as_ref(&self) -> CellRef {
        CellRef {
            module: self.module.as_ref(),
            id: self.id,
        }
    }

    pub fn reborrow(&mut self) -> CellRefMut {
        CellRefMut {
            module: ModuleRefMut {
                design: self.module.design,
                id: self.module.id,
            },
            id: self.id,
        }
    }

    pub fn sibling_mut(&mut self, cell: CellId) -> CellRefMut {
        self.module.cell(cell);
        CellRefMut {
            module: ModuleRefMut {
                design: self.module.design,
                id: self.module.id,
            },
            id: cell,
        }
    }

    impl_cell_flag_mut!(keep, set_keep, Keep);
    impl_cell_flag_mut!(no_merge, set_no_merge, NoMerge);
    impl_cell_flag_mut!(no_flatten, set_no_flatten, NoFlatten);
    impl_cell_flag_mut!(async_, set_async, Async);
    impl_cell_flag_mut!(lax_x, set_lax_x, LaxX);

    pub fn set_flags_plane(&mut self, val: CellPlane) {
        match val {
            CellPlane::Param => {
                self.cell().flags.insert(CellFlag::Param);
                self.cell().flags.remove(CellFlag::Debug);
            }
            CellPlane::Main => {
                self.cell().flags.remove(CellFlag::Param);
                self.cell().flags.remove(CellFlag::Debug);
            }
            CellPlane::Debug => {
                self.cell().flags.remove(CellFlag::Param);
                self.cell().flags.insert(CellFlag::Debug);
            }
        }
    }

    pub fn clear_flags(&mut self) {
        self.cell().flags = Default::default();
    }

    pub fn set_annnotations(&mut self, val: Vec<CellAnnotation>) {
        self.cell().annotations = val.into();
    }

    pub fn add_annotation(&mut self, val: CellAnnotation) {
        self.cell().annotations.push(val);
    }

    pub fn set_contents(&mut self, val: impl Into<CellKind>) {
        let old = core::mem::take(&mut self.cell().contents);
        let id = self.id;
        if let CellKind::Instance(ref inst) = old {
            if let Some(ref mut module) = self.module.design.modules[inst.module] {
                module.uses.remove(&(self.module.id, id));
            }
        }
        old.for_each_val(|cid, slot| {
            self.sibling_mut(cid).cell().uses.remove(&(id, slot));
        });
        let val = val.into();
        if let CellKind::Instance(ref inst) = val {
            if let Some(ref mut module) = self.module.design.modules[inst.module] {
                module.uses.insert((self.module.id, id));
            }
        }
        val.for_each_val(|cid, slot| {
            self.sibling_mut(cid).cell().uses.insert((id, slot));
        });
        self.cell().contents = val;
    }

    pub fn replace_val(&mut self, slot: CellValSlot, val: CellId) {
        let old = self.cell().contents.replace_val(slot, val);
        let id = self.id;
        self.sibling_mut(old).cell().uses.remove(&(id, slot));
        self.sibling_mut(val).cell().uses.insert((id, slot));
    }

    delegate! {
        to self.as_ref() {
            pub fn sibling(&self, cell: CellId) -> CellRef;
            pub fn flags_plane(&self) -> CellPlane;
            pub fn annotations(&self) -> &[CellAnnotation];
            pub fn contents(&self) -> &CellKind;
            pub fn uses(&self) -> impl Iterator<Item = (CellId, CellValSlot)> + '_;
        }

        to self.module.design {
            pub fn string(&self, id: StrId) -> &str;
            pub fn intern(&mut self, s: &str) -> StrId;
        }
    }
}
/// A type of a cell.
pub enum CellType {
    /// No generally usable type.
    Void,
    /// A bitvec of known width.  The second field is true iff the cell is
    /// one of the bus cells (ie. can be driven).
    BitVec(u32, bool),
    /// A bitvec of unknown width.
    BitVecAny,
    /// An output port of known width.
    Out(u32),
    /// An output port of unknown width.
    OutAny,
    /// A string.
    String,
    /// An int.
    Int,
    /// A float.
    Float,
}

/// The plane on which a cell lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum CellPlane {
    Param,
    Main,
    Debug,
}

/// Type for [`Module::flags`].
#[bitflags]
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ModuleFlag {
    /// Instances of this module are not to be optimized out even if seemingly unused.
    /// It's likely that the [`ModuleFlag::NoMerge`] and [`ModuleFlag::NoFlatten`] flags need to be set along with this one.
    Keep,
    /// Instances of this module are not to be merged together even when their inputs and parameters are identical.
    NoMerge,
    /// Instances of this module are not to be flattened into their parent module (commonly known as `keep_hierarchy`).
    NoFlatten,
    /// Instances of this module are to be flattened at the first available opportunity (used for small wrapper modules).
    Inline,
    /// This module is a blackbox — a placeholder for logic that is defined elsewhere.  All module contents are irrelevant,
    /// except for cells that define the module interface (ports, parameters, and blackbox annotations).  It has several uses:
    ///
    /// - defines an interface to a target-defined cell
    /// - defines an interface of a user module that is synthesized separately, and will be linked together in later stage of synthesis
    /// - defines an interface to a partial reconfiguration area
    Blackbox,
    /// This is the top module of the design (the root of the module hierarchy).  For normal synthesis flow, there must be exactly one
    /// of those.  However, module libraries in partial synthesis flows can have multiple top modules.
    Top,
}

/// Type for [`Cell::flags`].
#[bitflags]
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CellFlag {
    /// This cell is not to be removed even if seemingly unused.
    ///
    /// While nominally valid on most cell kinds, using it on unmapped cells is effectively meaningless and should be
    /// avoided, as such cells will generally be transformed during the synthesis process, losing the annotation.
    /// Cells where this annotation can be used effectively include:
    ///
    /// - [`Instance`] and [`UnresolvedInstance`] that are also [`CellFlag::NoFlatten`] or reference a blackbox
    /// - [`Wire`]
    /// - `Memory`
    /// - cells that are directly mapped to target primitives
    ///
    /// If you want to keep the result of some combinatorial logic or a register, create a [`Wire`] referencing it and
    /// put the flag on the wire instead.
    Keep,
    /// This cell is not to be merged with another cell even if identical.
    ///
    /// This flag is subject to the same warning as the [`CellFlag::Keep`] flag.  Effectively supported cells include:
    ///
    /// - [`Instance`] and [`UnresolvedInstance`] that are also [`CellFlag::NoFlatten`] or reference a blackbox
    /// - [`Register`]
    /// - `Memory`
    /// - cells that are directly mapped to target primitives
    NoMerge,
    /// This cell (which must be an instance) is not to be flattened into the containing module.  This flag is effectively
    /// ORed together with the target [`Module`]'s.
    NoFlatten,
    /// On combinatorial cells: this cell must be synthesized in a glitchless way, as it implements asynchronous logic.
    ///
    /// The precise semantics are: for a non-async cell, it is permissible for the output to temporarily become `x` whenever
    /// an input of the cell changes value.  For an async cell, if the output value would be the same both before and after
    /// an input change, the output must stay stable.
    ///
    /// On register cells: this cell is part of a synchronizer and must not participate in retiming.
    Async,
    /// Selects alternate, laxer x-propagation semantics for a cell (ie. the output of the cell becomes `x` in more cases).
    /// The exact definition of this flag depends on the cell kind.
    LaxX,
    /// Marks the cell as belonging to the constant plane.  Valid on swizzles and combinatorial cells.
    Param,
    /// Marks the cell as belonging to the debug plane.  Valid on swizzles and combinatorial cells.
    Debug,
}
