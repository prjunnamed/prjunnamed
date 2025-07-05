#[cfg(not(feature = "verify"))]
use std::str::FromStr;

#[cfg(not(feature = "verify"))]
use prjunnamed_generic::{LowerMux, Normalize, SimpleAigOpt};
#[cfg(not(feature = "verify"))]
use prjunnamed_netlist::{assert_isomorphic, Design};

#[cfg(not(feature = "verify"))]
#[test]
fn test_lower_mux() {
    let mut design = Design::from_str(concat!(
        "%0:4 = input \"a\"\n",
        "%1:4 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%3:4 = mux %2 %0:4 %1:4\n",
        "%4:0 = output \"d\" %3:4\n",
    ))
    .unwrap();
    design.rewrite(&[&LowerMux, &SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"a\"\n",
        "%1:4 = input \"b\"\n",
        "%2:1 = input \"c\"\n",
        "%10:1 = aig %0+0 %2\n",
        "%11:1 = aig %0+1 %2\n",
        "%12:1 = aig %0+2 %2\n",
        "%13:1 = aig %0+3 %2\n",
        "%20:1 = aig %1+0 !%2\n",
        "%21:1 = aig %1+1 !%2\n",
        "%22:1 = aig %1+2 !%2\n",
        "%23:1 = aig %1+3 !%2\n",
        "%30:1 = aig !%10 !%20\n",
        "%31:1 = aig !%11 !%21\n",
        "%32:1 = aig !%12 !%22\n",
        "%33:1 = aig !%13 !%23\n",
        "%40:1 = not %30\n",
        "%41:1 = not %31\n",
        "%42:1 = not %32\n",
        "%43:1 = not %33\n",
        "%4:0 = output \"d\" [%43 %42 %41 %40]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
