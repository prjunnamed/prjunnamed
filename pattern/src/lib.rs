//! This crate implements a macro-based DSL for matching netlists.
//!
//! The DSL features programmable patterns, which enables many features that
//! wouldn't otherwise be possible. For example:
//! - [`PZExt`] can match zero extensions, even though they don't correspond
//!   to a cell at all, and are achieved by appropriately mixing [`Net`]s
//!   within a [`Value`]
//! - patterns which correspond to bitwise cells, such as [`PAnd`], can match
//!   a [`Value`] even if it is made up from [`Net`]s coming from many distinct
//!   [`And`] cells.
//! - specialized patterns can be easily implemented, such as [`PPow2@a`],
//!   which matches a constant power of 2 and captures the exponent into `a`.
//!
//! [`And`]: prjunnamed_netlist::Cell::And
//! [`Net`]: prjunnamed_netlist::Net
//! [`Value`]: prjunnamed_netlist::Value
//! [`PAnd`]: patterns::PAnd
//! [`PPow2@a`]: patterns::PPow2
//! [`PZExt`]: patterns::PZExt
//!
//! By convention, the names of patterns are prefixed with `P`.
//!
//! The general netlist match syntax looks as follows:
//!
//! ```no_run
//! # use prjunnamed_netlist::{Design, Net, Value};
//! # use prjunnamed_pattern::netlist_matches;
//! # use prjunnamed_pattern::patterns::*;
//! let matcher = netlist_matches! {
//!     [PAdc@y [PAny@a] [PZero] [PInput@c ("cin")]] => (y, a, c);
//! };
//! # let design: Design = todo!();
//! # let target: Value = todo!();
//! # let result: Option<(Value, Value, Net)> = matcher(&design, &target);
//! ```
//!
//! A pattern call is delimited with square brackets and consists of the
//! following parts:
//! - the name of the pattern being called, e.g. `PAdc`
//! - a result capture, e.g. `@y`. This is optional, and if it is not provided,
//!   the value captured by the pattern is discarded.
//!
//!   Each pattern captures exactly one value. Often, it is the [`Net`] or
//!   [`Value`] it is being matched against. In rare cases, the captured value
//!   is of the unit type `()`, in which case it is usually discarded by the
//!   user.
//! - a number of expression arguments, e.g. `("cin")`. Expression arguments
//!   are evaluated according to normal Rust rules, and the results are passed
//!   to the pattern. Each pattern has a fixed number of expression arguments
//!   it expects.
//!
//!   If multiple expression arguments are being passed to the pattern, each
//!   requires its own pair of parentheses, e.g. `PFeline@cat ("meow") (42)`
//! - a number of pattern arguments, e.g. `[PZero]`. Pattern arguments
//!   follow the pattern call syntax being described.
//!
//!   Each pattern has a fixed number of pattern arguments it expects.
//!   Most commonly, they correspond to the inputs of the cell being matched.
//!
//!   If you do not wish to perform further matching on the inputs, use `[PAny]`.
//!
//! Much like `match`, [`netlist_matches!`] supports `if` and `if let` guards.
//! For example:
//!
//! ```no_run
//! # use prjunnamed_netlist::{Design, Net, Value};
//! # use prjunnamed_pattern::netlist_matches;
//! # use prjunnamed_pattern::patterns::*;
//! let matcher = netlist_matches! {
//!     [PXor [PAny@a] [PAny@b]] if a == b => Value::zero(a.len());
//! };
//! # let design: Design = todo!();
//! # let target: Value = todo!();
//! # let result: Option<Value> = matcher(&design, &target);
//! ```

/// A pattern that can be matched against a `Target`.
///
/// Most commonly, `Target` will be [`Value`] or [`Net`], but it could also be
/// something like `u32`.
///
/// A pattern must also have a `new` function that will be called by the macro
/// to construct an instance of the pattern. The arguments of the `new` function
/// consist of all the expression arguments, followed by all the pattern
/// arguments.
pub trait Pattern<Target> {
    /// The capture type resulting from matching this pattern *and all the
    /// patterns passed as arguments*.
    ///
    /// The macros expect this to be a tuple, where the first element is
    /// the value captured by the pattern itself, and the following elements
    /// correspond to the `Capture`s of all the child patterns.
    type Capture;

    fn execute(&self, design: &dyn DesignDyn, target: &Target) -> Option<Self::Capture>;
}

/// Matches a [`Value`] against a list of patterns and runs the body
/// corresponding to the first matching pattern.
///
/// To be specific, `netlist_matches!` returns an
/// `impl Fn(&dyn DesignDyn, &Value) -> Option<T>`. If no pattern matches
/// the value provided, the function will return `None`. Otherwise, it will
/// call `.into()` on the result of the matching arm and return `Some`.
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
                use $crate::Pattern;
                $crate::netlist_match! { @RULE@ $design $target $($rest)* }
            } else {
                None
            }
        }
    };
    { @RULE@ $design:ident $target:ident } => { None };
    { @RULE@ $design:ident $target:ident [ $($pat:tt)+ ] $( if $guard:expr )? => $result:expr; $($rest:tt)* } => {
        {
            'block: {
                let pattern = $crate::netlist_match!( @NEW@ [ $($pat)+ ] );
                match pattern.execute($design, $target) {
                    Some($crate::netlist_match!( @PAT@ [ $($pat)+ ] )) $( if $guard )? => {
                        if cfg!(feature = "trace") {
                            eprintln!(">match {} => {}",
                                stringify!([ $($pat)* ] $( if $guard )?).replace("\n", " "),
                                $design.inner().display_value(&*$target)
                            );
                        }
                        break 'block Some($result.into())
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
                let pattern = $crate::netlist_match!( @NEW@ [ $($pat)+ ] );
                match pattern.execute($design, $target) {
                    Some($crate::netlist_match!( @PAT@ [ $($pat)+ ] )) => {
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

/// Like [`netlist_matches!`], but calls
/// [`replace_value`][Design::replace_value] when a match is found.
///
/// The function returned by this macro returns `true` if an applicable rule
/// has been found.
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

pub use traits::{NetOrValue, DesignDyn};

pub mod patterns {
    pub use crate::simple::*;
    pub use crate::bitwise::*;
    pub use crate::shift::*;
    pub use crate::arithmetic::*;
}
