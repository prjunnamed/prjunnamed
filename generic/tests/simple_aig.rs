use std::str::FromStr;

use prjunnamed_generic::{Normalize, SimpleAigOpt};
use prjunnamed_netlist::{assert_isomorphic, Design};

#[test]
fn test_lower_aig() {
    let mut design = Design::from_str(concat!(
        "%0:3 = input \"a\"\n",
        "%1:3 = input \"b\"\n",
        "%2:3 = and %0:3 %1:3\n",
        "%3:3 = or %0:3 %1:3\n",
        "%4:3 = xor %0:3 %1:3\n",
        "%5:0 = output \"c\" %2:3\n",
        "%6:0 = output \"d\" %3:3\n",
        "%7:0 = output \"e\" %4:3\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:3 = input \"a\"\n",
        "%1:3 = input \"b\"\n",
        "%20:1 = aig %0+0 %1+0\n",
        "%21:1 = aig %0+1 %1+1\n",
        "%22:1 = aig %0+2 %1+2\n",
        "%30:1 = aig !%0+0 !%1+0\n",
        "%31:1 = aig !%0+1 !%1+1\n",
        "%32:1 = aig !%0+2 !%1+2\n",
        "%40:1 = not %30\n",
        "%41:1 = not %31\n",
        "%42:1 = not %32\n",
        "%50:1 = xor %0+0 %1+0\n",
        "%51:1 = xor %0+1 %1+1\n",
        "%52:1 = xor %0+2 %1+2\n",
        "%60:0 = output \"c\" [%22 %21 %20]\n",
        "%61:0 = output \"d\" [%42 %41 %40]\n",
        "%62:0 = output \"e\" [%52 %51 %50]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_or_chain() {
    let mut design = Design::from_str(concat!(
        "%0:3 = input \"a\"\n",
        "%1:1 = or 0 %0+0\n",
        "%2:1 = or %1 %0+1\n",
        "%3:1 = or %2 %0+2\n",
        "%4:0 = output \"c\" %3\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:3 = input \"a\"\n",
        "%1:1 = aig !%0+0 !%0+1\n",
        "%2:1 = aig %1 !%0+2\n",
        "%3:1 = not %2\n",
        "%4:0 = output \"c\" %3\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_const() {
    let mut design = Design::from_str(concat!(
        "%0:3 = input \"a\"\n",
        "%10:3 = not 01X\n",
        "%11:0 = output \"c\" %10:3\n",
        "%20:3 = and %0:3 01X\n",
        "%21:0 = output \"d\" %20:3\n",
        "%30:3 = and 01X %0:3\n",
        "%31:0 = output \"e\" %30:3\n",
        "%40:3 = xor %0:3 01X\n",
        "%41:0 = output \"f\" %40:3\n",
        "%50:3 = xor 01X %0:3\n",
        "%51:0 = output \"g\" %50:3\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:3 = input \"a\"\n",
        "%11:0 = output \"c\" 10X\n",
        "%20:1 = aig %0+0 X\n",
        "%21:0 = output \"d\" [0 %0+1 %20]\n",
        "%22:0 = output \"e\" [0 %0+1 %20]\n",
        "%40:1 = not %0+1\n",
        "%41:0 = output \"f\" [%0+2 %40 X]\n",
        "%42:0 = output \"g\" [%0+2 %40 X]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_idempotent() {
    let mut design = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:1 = aig %0 %0\n",
        "%11:0 = output \"d\" %10\n",
        "%20:1 = aig %0 %1\n",
        "%21:1 = aig %20 %1\n",
        "%22:0 = output \"e\" %21\n",
        "%30:1 = aig %0 %2\n",
        "%31:1 = aig %20 %30\n",
        "%32:0 = output \"f\" %31\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:0 = output \"d\" %0\n",
        "%20:1 = aig %0 %1\n",
        "%22:0 = output \"e\" %20\n",
        "%30:1 = aig %2 %20\n",
        "%32:0 = output \"f\" %30\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_contradiction() {
    let mut design = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:1 = aig %0 !%0\n",
        "%11:0 = output \"d\" %10\n",
        "%20:1 = aig %0 %1\n",
        "%21:1 = aig %20 !%1\n",
        "%22:0 = output \"e\" %21\n",
        "%30:1 = aig !%0 %2\n",
        "%31:1 = aig %20 %30\n",
        "%32:0 = output \"f\" %31\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:0 = output \"d\" 0\n",
        "%22:0 = output \"e\" 0\n",
        "%32:0 = output \"f\" 0\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_subsumption() {
    let mut design = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:1 = or %0 %1\n",
        "%11:1 = and %10 %0\n",
        "%12:0 = output \"d\" %11\n",
        "%20:1 = and %0 %2\n",
        "%21:1 = and %10 %20\n",
        "%22:0 = output \"e\" %21\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%12:0 = output \"d\" %0\n",
        "%20:1 = aig %0 %2\n",
        "%22:0 = output \"e\" %20\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_resolution() {
    let mut design = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%10:1 = not %0\n",
        "%20:1 = or %0 %1\n",
        "%21:1 = or %1 %10\n",
        "%22:1 = and %20 %21\n",
        "%23:0 = output \"d\" %22\n",
        "%30:1 = and %1 %0\n",
        "%31:1 = and %1 %10\n",
        "%32:1 = or %30 %31\n",
        "%33:0 = output \"e\" %32\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%23:0 = output \"d\" %1\n",
        "%33:0 = output \"e\" %1\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_substitution() {
    let mut design = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:1 = not %1\n",
        "%11:1 = and %10 %2\n",
        "%12:1 = and %1 %2\n",
        "%20:1 = or %0 %1\n",
        "%21:1 = and %20 %10\n",
        "%22:1 = and %20 %11\n",
        "%23:0 = output \"d\" [%22 %21]\n",
        "%30:1 = xor %1 %0\n",
        "%31:1 = not %30\n",
        "%40:1 = and %30 %10\n",
        "%41:1 = and %30 %1\n",
        "%42:1 = and %31 %10\n",
        "%43:1 = and %31 %1\n",
        "%44:0 = output \"e\" [%43 %42 %41 %40]\n",
        "%50:1 = and %30 %11\n",
        "%51:1 = and %30 %12\n",
        "%52:1 = and %31 %11\n",
        "%53:1 = and %31 %12\n",
        "%54:0 = output \"f\" [%53 %52 %51 %50]\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%11:1 = aig %2 !%1\n",
        "%12:1 = aig %1 %2\n",
        "%21:1 = aig %0 !%1\n",
        "%22:1 = aig %0 %11\n",
        "%23:0 = output \"d\" [%22 %21]\n",
        "%40:1 = aig %0 !%1\n",
        "%41:1 = aig %1 !%0\n",
        "%42:1 = aig !%0 !%1\n",
        "%43:1 = aig %0 %1\n",
        "%44:0 = output \"e\" [%43 %42 %41 %40]\n",
        "%50:1 = aig %0 %11\n",
        "%51:1 = aig %12 !%0\n",
        "%52:1 = aig %11 !%0\n",
        "%53:1 = aig %0 %12\n",
        "%54:0 = output \"f\" [%53 %52 %51 %50]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_xor() {
    let mut design = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:1 = xor %0 %0\n",
        "%11:0 = output \"d\" %10\n",
        "%20:1 = xor %0 %1\n",
        "%21:1 = xor %20 %1\n",
        "%22:0 = output \"e\" %21\n",
        "%30:1 = xor %0 %2\n",
        "%31:1 = xor %20 %30\n",
        "%32:0 = output \"f\" %31\n",
        "%40:1 = not %0\n",
        "%41:1 = xor %40 %1\n",
        "%42:0 = output \"g\" %41\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:0 = output \"d\" 0\n",
        "%22:0 = output \"e\" %0\n",
        "%30:1 = xor %1 %2\n",
        "%32:0 = output \"f\" %30\n",
        "%40:1 = xor %0 %1\n",
        "%41:1 = not %40\n",
        "%42:0 = output \"g\" %41\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_and_xor() {
    let mut design = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%10:1 = not %0\n",
        "%11:1 = and %0 %1\n",
        "%20:1 = xor %11 %0\n",
        "%23:0 = output \"d\" %20\n",
        "%30:1 = xor %11 %10\n",
        "%33:0 = output \"e\" %30\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%20:1 = aig %0 !%1\n",
        "%23:0 = output \"d\" %20\n",
        "%30:1 = not %20\n",
        "%33:0 = output \"e\" %30\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_match_xor() {
    let mut design = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%10:1 = not %0\n",
        "%11:1 = not %1\n",
        "%20:1 = and %0 %1\n",
        "%21:1 = and %10 %11\n",
        "%22:1 = or %20 %21\n",
        "%23:0 = output \"d\" %22\n",
        "%30:1 = and %0 %11\n",
        "%31:1 = and %1 %10\n",
        "%32:1 = or %30 %31\n",
        "%33:0 = output \"f\" %32\n",
    ))
    .unwrap();
    design.rewrite(&[&SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:1 = input \"a\"\n",
        "%1:1 = input \"b\"\n",
        "%10:1 = xor %0 %1\n",
        "%11:1 = not %10\n",
        "%23:0 = output \"d\" %11\n",
        "%32:0 = output \"f\" %10\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
