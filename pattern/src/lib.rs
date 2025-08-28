//! This crate implements a macro-based DSL for matching netlists.
//!
//! We use a custom DSL because the in-memory representation of our IR is not
//! at all suited to being pattern-matched with Rust's `match` statement.
//!
//! The general netlist match syntax looks as follows:
//!
//! ```no_run
//! # use prjunnamed_netlist::{Design, Net, Value};
//! # use prjunnamed_pattern::netlist_match;
//! # use prjunnamed_pattern::patterns::*;
//! let do_match = netlist_match! {
//!     [PAdc@y [PAny@a] [PZero] [PInput@c ("cin")]] => (y, a, c);
//! };
//! # let design: Design = todo!();
//! # let target: Value = todo!();
//! # let result: Option<(Value, Value, Net)> = do_match(&design, &target);
//! ```
//!
//! You will notice that the patterns don't directly refer to the variants of
//! the [`Cell`] enum. Instead, they are built out of *matchers*, such as
//! [`PAdc`] or [`PZero`]. By convention, the names of matchers are prefixed
//! with `P`.
//!
//! While some matchers, such as [`PAdc`], directly correspond to cells, a
//! matcher can evaluate arbitrary conditions. For example:
//!
//! - [`PZExt`] matches zero extensions, even though they don't correspond
//!   to a cell at all, and are achieved by appropriately mixing [`Net`]s
//!   within a [`Value`],
//! - patterns which correspond to bitwise cells, such as [`PAnd`], can match
//!   a [`Value`] even if it is made up from [`Net`]s driven by many distinct
//!   [`And`] cells.
//!
//! If a matcher succeeds, it *captures* a result, which can then be bound with
//! the `@var` syntax. This result is often the [`Value`] it matched against,
//! but it doesn't have to be. For example, [`PPow2@a`] matches constants that
//! are a power of two, and captures the *exponent* into `a: u32`.
//!
//! [`Cell`]: prjunnamed_netlist::Cell
//! [`And`]: prjunnamed_netlist::Cell::And
//! [`Net`]: prjunnamed_netlist::Net
//! [`Value`]: prjunnamed_netlist::Value
//! [`PAnd`]: patterns::PAnd
//! [`PAdc`]: patterns::PAdc
//! [`PPow2@a`]: patterns::PPow2
//! [`PZExt`]: patterns::PZExt
//! [`PZero`]: patterns::PZero
//!
//! ## Pattern syntax
//!
//! In the example above, `[PAdc@y [PAny@a] [PZero] [PInput@c ("cin")]]` is a
//! pattern. A pattern is delimited with square brackets and consists of the
//! following parts:
//! - the name of the matcher being called, e.g. `PAdc`
//! - a result capture, e.g. `@y`. This is optional, and if it is not provided,
//!   the value captured by the pattern is discarded.
//! - a number of expression arguments, e.g. `("cin")`. Expression arguments
//!   are evaluated according to normal Rust rules, and the results are passed
//!   to the pattern. Each pattern has a fixed number of expression arguments
//!   it expects.
//!
//!   If multiple expression arguments are being passed to the pattern, each
//!   requires its own pair of parentheses, e.g. `PFeline@cat ("meow") (42)`
//! - a number of pattern arguments, e.g. `[PZero]`. Each pattern argument
//!   is, recursively, a pattern in itself.
//!
//!   Each pattern has a fixed number of pattern arguments it expects.
//!   Most commonly, they correspond to the inputs of the cell being matched.
//!
//!   The pattern arguments are mandatory. If you do not wish to perform further
//!   matching, use `[PAny]`.
//!
//! ## Guards
//!
//! Much like `match`, [`netlist_match!`] supports `if` and `if let` guards.
//! For example:
//!
//! ```no_run
//! # use prjunnamed_netlist::{Design, Net, Value};
//! # use prjunnamed_pattern::netlist_match;
//! # use prjunnamed_pattern::patterns::*;
//! let do_match = netlist_match! {
//!     [PXor [PAny@a] [PAny@b]] if a == b => Value::zero(a.len());
//! };
//! # let design: Design = todo!();
//! # let target: Value = todo!();
//! # let result: Option<Value> = do_match(&design, &target);
//! ```
//!
//! ## Implementing matchers
//! 
//! Each matcher is a struct that contains all the arguments of the matcher.
//! The matcher must have an associated function called `new` that will
//! instantiate it when provided with, in order, the values of the expression
//! arguments, followed by all the pattern arguments.
//!
//! Each particular instance of a matcher is a pattern. Thus the matcher
//! should implement the [`Pattern`] trait as appropriate.

/// A pattern that can be matched against a `Target`.
///
/// Most commonly, `Target` will be [`Value`] or [`Net`], but it could also be
/// something like `u32`. However, only matchers implementing `Pattern<Value>`
/// can occur at the top-level of a pattern.
///
/// [`Net`]: prjunnamed_netlist::Net
/// [`Value`]: prjunnamed_netlist::Value
pub trait Pattern<Target> {
    /// The capture type resulting from matching this pattern, including all the
    /// patterns passed to the matcher as arguments.
    ///
    /// This should be a tuple, where the first element is the result captured
    /// by the top-level matcher, and the following elements correspond to the
    /// `Capture`s of all the child patterns.
    type Capture;

    fn execute(&self, design: &dyn DesignDyn, target: &Target) -> Option<Self::Capture>;
}

/// Matches a [`Value`] against a list of patterns and runs the body
/// corresponding to the first matching pattern.
///
/// Note that this means that the order in which you list the patterns
/// is significant.
///
/// `netlist_match!` returns an `impl Fn(&dyn DesignDyn, &Value) -> Option<T>`.
/// If no pattern matches the value provided, the function will return `None`.
/// Otherwise, it will call `.into()` on the result of the matching arm and
/// return `Some`.
///
/// [`Value`]: prjunnamed_netlist::Value
#[macro_export]
macro_rules! netlist_match {
    { [ $($rule:tt)* ] $($rest:tt)* } => {
        |design: &dyn $crate::DesignDyn, target: &prjunnamed_netlist::Value| {
            $crate::netlist_match! { @TOP@ design target [ $($rule)* ] $($rest)* }
        }
    };
    { @TOP@ $design:ident $target:ident $($rest:tt)* } => {
        {
            if $target.len() > 0 {
                use $crate::{Pattern, DesignDyn};
                let design = $crate::CellCollector::new($design);
                $crate::netlist_match! { @RULE@ design $target $($rest)* }
            } else {
                None
            }
        }
    };
    { @RULE@ $design:ident $target:ident } => { None };
    { @RULE@ $design:ident $target:ident [ $($pat:tt)+ ] $( if $gexpr:expr )? => $result:expr; $($rest:tt)* } => {
        {
            'block: {
                $design.clear();
                let pattern = $crate::netlist_match!( @NEW@ [ $($pat)+ ] );
                match pattern.execute(&$design, $target) {
                    Some($crate::netlist_match!( @PAT@ [ $($pat)+ ] )) => {
                        let _guard = $design.inner().use_metadata_from(&$design.cells());
                        $( if $gexpr )? {
                            if cfg!(feature = "trace") {
                                eprintln!(">match {} => {}",
                                    stringify!([ $($pat)* ] $( if $gexpr )?).replace("\n", " "),
                                    $design.inner().display_value(&*$target)
                                );
                            }
                            break 'block Some($result.into())
                        }
                    }
                    _ => ()
                }
                $crate::netlist_match! { @RULE@ $design $target $($rest)* }
            }
        }
    };
    { @RULE@ $design:ident $target:ident [ $($pat:tt)+ ] if let $gpat:pat = $gexpr:expr => $result:expr; $($rest:tt)* } => {
        {
            'block: {
                $design.clear();
                let pattern = $crate::netlist_match!( @NEW@ [ $($pat)+ ] );
                match pattern.execute(&$design, $target) {
                    Some($crate::netlist_match!( @PAT@ [ $($pat)+ ] )) => {
                        let _guard = $design.inner().use_metadata_from(&$design.cells());
                        if let $gpat = $gexpr {
                            if cfg!(feature = "trace") {
                                eprintln!(">match {} => {}",
                                    stringify!([ $($pat)* ] if let $gpat = $gexpr).replace("\n", " "),
                                    $design.inner().display_value(&*$target)
                                );
                            }
                            break 'block Some($result.into())
                        }
                    }
                    _ => ()
                }
                $crate::netlist_match! { @RULE@ $design $target $($rest)* }
            }
        }
    };
    ( @NEW@ [ $pat:ident $( @ $cap:ident )? $( ( $($exparg:tt)+ ) )* $( [ $($patarg:tt)+ ] )* ] ) => {
        $pat::new( $( $($exparg)+, )* $( $crate::netlist_match!( @NEW@ [ $($patarg)+ ] ) ),*)
    };
    ( @PAT@ [ $pat:ident $( ( $($exparg:tt)+ ) )* $( [ $($patarg:tt)+ ] )* ] ) => {
        (_, $( $crate::netlist_match!( @PAT@ [ $($patarg)+ ] ) ),*)
    };
    ( @PAT@ [ $pat:ident @ $cap:ident $( ( $($exparg:tt)+ ) )* $( [ $($patarg:tt)+ ] )* ] ) => {
        ($cap, $( $crate::netlist_match!( @PAT@ [ $($patarg)+ ] ) ),*)
    };
}

/// Like [`netlist_match!`], but when a match is found, the value is [replaced]
/// by the value returned by the body of the rule.
///
/// `netlist_replace!` returns an `impl Fn(&dyn DesignDyn, &Value) -> bool`.
/// The function returns a boolean that indicates whether an applicable pattern
/// has been found.
///
/// [replaced]: prjunnamed_netlist::Design::replace_value
#[macro_export]
macro_rules! netlist_replace {
    { [ $($rule:tt)* ] $($rest:tt)* } => {
        |design: &dyn $crate::DesignDyn, target: &prjunnamed_netlist::Value| -> bool {
            $crate::netlist_replace! { @TOP@ design target [ $($rule)* ] $($rest)* }
        }
    };
    { @TOP@ $design:ident $target:ident $($rest:tt)* } => {
        let result: Option<Value> = $crate::netlist_match! { @TOP@ $design $target $($rest)* };
        if let Some(replace) = result {
            #[allow(unexpected_cfgs)]
            if cfg!(feature = "trace") {
                eprintln!(">replace => {}",
                    $design.inner().display_value(&prjunnamed_netlist::Value::from(replace.clone()))
                );
            }
            $design.inner().replace_value($target, &replace);
            true
        } else {
            false
        }
    };
}

#[macro_export]
macro_rules! assert_netlist {
    ( $design:expr , $check:expr $( , $( $assertarg:tt)+ )? ) => {
        {
            $design.apply();
            let check = $check;
            let mut matches = $design.iter_cells().all(|cell_ref| {
                if let prjunnamed_netlist::Cell::Output(_name, value) = &*cell_ref.get() {
                    check(&$design, value).unwrap_or(false)
                } else {
                    true
                }
            });
            if !matches {
                eprintln!("{}", $design);
            }
            assert!(matches $( , $( $assertarg )+ )?);
        }
    };
}

mod traits;
mod simple;
mod bitwise;
mod shift;
mod arithmetic;

pub use traits::{NetOrValue, DesignDyn, CellCollector};

pub mod patterns {
    pub use crate::simple::*;
    pub use crate::bitwise::*;
    pub use crate::shift::*;
    pub use crate::arithmetic::*;
}
