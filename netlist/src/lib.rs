//! This crate defines the IR of Project Unnamed.
//!
//! A [`Design`] is represented as a sea of [`Cell`]s. Each cell is identified
//! only by a range of indices; neither cells nor the nets connecting them have
//! names.
//!
//! No distinction is made between coarse and fine netlists — a single IR is
//! used for both.

mod logic;
mod value;
mod param;
mod io;
mod cell;
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
