use std::str::FromStr;

use prjunnamed_generic::{LowerEq, Normalize, SimpleAigOpt};
use prjunnamed_netlist::{assert_isomorphic, Design};

#[test]
fn test_lower_eq() {
    let mut design = Design::from_str(concat!(
        "%0:5 = input \"a\"\n",
        "%1:5 = input \"b\"\n",
        "%2:1 = eq %0:5 %1:5\n",
        "%4:0 = output \"d\" %2\n",
    ))
    .unwrap();
    design.rewrite(&[&LowerEq, &SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:5 = input \"a\"\n",
        "%1:5 = input \"b\"\n",
        "%10:1 = xor %0+0 %1+0\n",
        "%11:1 = xor %0+1 %1+1\n",
        "%12:1 = xor %0+2 %1+2\n",
        "%13:1 = xor %0+3 %1+3\n",
        "%14:1 = xor %0+4 %1+4\n",
        "%20:1 = aig !%10 !%11\n",
        "%21:1 = aig !%12 !%13\n",
        "%30:1 = aig %20 %21\n",
        "%40:1 = aig %30 !%14\n",
        "%4:0 = output \"d\" %40\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
