pub trait Pattern<Target> {
    type Capture;

    fn execute(&self, design: &dyn DesignDyn, target: &Target) -> Option<Self::Capture>;
}

#[macro_export]
macro_rules! netlist_matches {
    { [ $($rule:tt)* ] $($rest:tt)* } => {
        |design: &dyn $crate::DesignDyn, target: &prjunnamed_netlist::Value| {
            $crate::netlist_matches! { @TOP@ design target [ $($rule)* ] $($rest)* }
        }
    };
    { @TOP@ $design:ident $target:ident $($rest:tt)* } => {
        {
            if $target.len() > 0 {
                use $crate::Pattern;
                $crate::netlist_matches! { @RULE@ $design $target $($rest)* }
            } else {
                None
            }
        }
    };
    { @RULE@ $design:ident $target:ident } => { None };
    { @RULE@ $design:ident $target:ident [ $($pat:tt)+ ] $( if $guard:expr )? => $result:expr; $($rest:tt)* } => {
        {
            'block: {
                let pattern = $crate::netlist_matches!( @NEW@ [ $($pat)+ ] );
                match pattern.execute($design, $target) {
                    Some($crate::netlist_matches!( @PAT@ [ $($pat)+ ] )) $( if $guard )? => {
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
                $crate::netlist_matches! { @RULE@ $design $target $($rest)* }
            }
        }
    };
    { @RULE@ $design:ident $target:ident [ $($pat:tt)+ ] if let $gpat:pat = $gexpr:expr => $result:expr; $($rest:tt)* } => {
        {
            'block: {
                let pattern = $crate::netlist_matches!( @NEW@ [ $($pat)+ ] );
                match pattern.execute($design, $target) {
                    Some($crate::netlist_matches!( @PAT@ [ $($pat)+ ] )) => {
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
                $crate::netlist_matches! { @RULE@ $design $target $($rest)* }
            }
        }
    };
    ( @NEW@ [ $pat:ident $( @ $cap:ident )? $( ( $($exparg:tt)+ ) )+ $( [ $($patarg:tt)+ ] )* ] ) => {
        $pat::new( $( $($exparg)+ ),+ , $( $crate::netlist_matches!( @NEW@ [ $($patarg)+ ] ) ),*)
    };
    ( @NEW@ [ $pat:ident $( @ $cap:ident )? $( [ $($patarg:tt)+ ] )* ] ) => {
        $pat::new( $( $crate::netlist_matches!( @NEW@ [ $($patarg)+ ] ) ),*)
    };
    ( @PAT@ [ $pat:ident $( ( $($exparg:tt)+ ) )* $( [ $($patarg:tt)+ ] )* ] ) => {
        (_, $( $crate::netlist_matches!( @PAT@ [ $($patarg)+ ] ) ),*)
    };
    ( @PAT@ [ $pat:ident @ $cap:ident $( ( $($exparg:tt)+ ) )* $( [ $($patarg:tt)+ ] )* ] ) => {
        ($cap, $( $crate::netlist_matches!( @PAT@ [ $($patarg)+ ] ) ),*)
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
        let result: Option<Value> = $crate::netlist_matches! { @TOP@ $design $target $($rest)* };
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
