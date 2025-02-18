//! This library provides the in-memory form of the Project Unnamed IR.
//!
//! A [`Design`] is represented as a sea of [`Cell`]s identified by a contiguous range of indices,
//! connected by [`Net`]s and [`Value`]s that refer back to cells by their index. This representation
//! is equally suited for bit-level and word-level netlists, including bit-level cells with multiple
//! outputs.

mod logic;
mod value;
mod param;
mod io;
mod cell;
mod metadata;
mod design;
mod print;
mod parse;
mod target;

mod isomorphic;
mod smt;

pub use logic::{Trit, Const};
pub use value::{Net, ControlNet, Value};
pub use param::ParamValue;
pub use io::{IoNet, IoValue};
pub use cell::{
    Cell, MatchCell, AssignCell, FlipFlop, IoBuffer, Memory, MemoryWritePort, MemoryReadPort, MemoryReadFlipFlop,
    MemoryPortRelation, TargetCell, Instance,
};
pub use metadata::{MetaStringRef, MetaItem, MetaItemRef};
pub use design::{Design, CellRef};
pub use parse::{parse, ParseError};
pub use target::{
    Target, TargetParamKind, TargetParam, TargetInput, TargetOutput, TargetIo, TargetCellPurity, TargetPrototype,
    TargetCellImportError, TargetImportError, register_target, create_target,
};

pub use isomorphic::{isomorphic, NotIsomorphic};
pub use smt::{SmtEngine, SmtResponse};
#[cfg(feature = "easy-smt")]
pub use smt::easy_smt::EasySmtEngine;
