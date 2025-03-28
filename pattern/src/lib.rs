pub trait Pattern<Target> {
    type Capture;

    fn execute(&self, design: &dyn DesignDyn, target: &Target) -> Option<Self::Capture>;
}

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
                            tracing::trace!("match {} => {}",
                                stringify!([ $($pat)* ] $( if $gexpr )?).replace("\n", " "),
                                $design.inner().display_value(&*$target)
                            );
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
                            tracing::trace!("match {} => {}",
                                stringify!([ $($pat)* ] if let $gpat = $gexpr).replace("\n", " "),
                                $design.inner().display_value(&*$target)
                            );
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
            tracing::trace!("replace => {}",
                $design.inner().display_value(&prjunnamed_netlist::Value::from(replace.clone()))
            );
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
