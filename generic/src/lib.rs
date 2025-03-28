use prjunnamed_netlist::Design;

mod unname;
mod decision;
mod simplify;
mod merge;
mod split;
mod lower_arith;
mod iobuf_insert;

pub use unname::unname;
pub use decision::decision;
pub use lower_arith::lower_arith;
pub use iobuf_insert::iobuf_insert;

pub fn canonicalize(design: &mut Design) {
    for iter in 1.. {
        let _span = tracing::debug_span!("canonicalize", iter).entered();

        let did_simplify = simplify::simplify(design);
        let did_merge = merge::merge(design);
        let did_split = split::split(design);
        if !(did_simplify || did_merge || did_split) {
            break;
        }
    }
}
