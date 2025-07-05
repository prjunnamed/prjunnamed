use std::str::FromStr;

use prjunnamed_generic::{LowerMul, Normalize};
use prjunnamed_netlist::{assert_isomorphic, Design};

#[test]
fn test_lower_mul() {
    let mut design = Design::from_str(concat!(
        "%0:5 = input \"a\"\n",
        "%1:5 = input \"b\"\n",
        "%2:5 = mul %0:5 %1:5\n",
        "%4:0 = output \"c\" %2:5\n",
    ))
    .unwrap();
    design.rewrite(&[&LowerMul, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:5 = input \"a\"\n",
        "%1:5 = input \"b\"\n",
        "%10:5 = mux %1+0 %0:5 00000\n",
        "%11:5 = mux %1+1 %0:5 00000\n",
        "%12:5 = mux %1+2 %0:5 00000\n",
        "%13:5 = mux %1+3 %0:5 00000\n",
        "%14:5 = mux %1+4 %0:5 00000\n",
        "%20:6 = adc 00000 %10:5 0\n",
        "%21:7 = adc [%11:5 0] %20:6 0\n",
        "%22:8 = adc [%12:5 00] %21:7 0\n",
        "%23:9 = adc [%13:5 000] %22:8 0\n",
        "%24:10 = adc [%14:5 0000] %23:9 0\n",
        "%4:0 = output \"c\" %24:5\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
