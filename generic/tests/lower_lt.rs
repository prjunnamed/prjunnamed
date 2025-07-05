use std::str::FromStr;

use prjunnamed_generic::{LowerLt, Normalize, SimpleAigOpt};
use prjunnamed_netlist::{assert_isomorphic, Design};

#[test]
fn test_lower_lt() {
    let mut design = Design::from_str(concat!(
        "%0:5 = input \"a\"\n",
        "%1:5 = input \"b\"\n",
        "%2:1 = ult %0:5 %1:5\n",
        "%3:1 = slt %0:5 %1:5\n",
        "%4:0 = output \"c\" %2\n",
        "%5:0 = output \"d\" %3\n",
    ))
    .unwrap();
    design.rewrite(&[&LowerLt, &SimpleAigOpt, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:5 = input \"a\"\n",
        "%1:5 = input \"b\"\n",
        "%10:1 = not %1+0\n",
        "%11:1 = not %1+1\n",
        "%12:1 = not %1+2\n",
        "%13:1 = not %1+3\n",
        "%14:1 = not %1+4\n",
        "%15:1 = not %0+4\n",
        "%20:6 = adc %0:5 [%14 %13 %12 %11 %10] 1\n",
        "%21:6 = adc [%15 %0:4] [%1+4 %13 %12 %11 %10] 1\n",
        "%30:1 = not %20+5\n",
        "%31:1 = not %21+5\n",
        "%4:0 = output \"c\" %30\n",
        "%5:0 = output \"d\" %31\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
