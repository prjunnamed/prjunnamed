use std::str::FromStr;

use prjunnamed_generic::{LowerShift, Normalize};
use prjunnamed_netlist::{assert_isomorphic, Design};

#[test]
fn test_lower_shift_short() {
    let mut design = Design::from_str(concat!(
        "%0:9 = input \"a\"\n",
        "%1:3 = input \"b\"\n",
        "%10:9 = shl %0:9 %1:3 #1\n",
        "%11:0 = output \"c\" %10:9\n",
        "%20:9 = ushr %0:9 %1:3 #1\n",
        "%21:0 = output \"d\" %20:9\n",
        "%30:9 = sshr %0:9 %1:3 #1\n",
        "%31:0 = output \"e\" %30:9\n",
        "%40:9 = xshr %0:9 %1:3 #1\n",
        "%41:0 = output \"f\" %40:9\n",
    ))
    .unwrap();
    design.rewrite(&[&LowerShift, &Normalize]);
    let mut gold = Design::from_str(concat!(
        "%0:9 = input \"a\"\n",
        "%1:3 = input \"b\"\n",
        "%10:9 = mux %1+0 [%0:8 0] %0:9\n",
        "%11:9 = mux %1+1 [%10:7 00] %10:9\n",
        "%12:9 = mux %1+2 [%11:5 0000] %11:9\n",
        "%13:0 = output \"c\" %12:9\n",
        "%20:9 = mux %1+0 [0 %0+1:8] %0:9\n",
        "%21:9 = mux %1+1 [00 %20+2:7] %20:9\n",
        "%22:9 = mux %1+2 [0000 %21+4:5] %21:9\n",
        "%23:0 = output \"d\" %22:9\n",
        "%30:9 = mux %1+0 [%0+8 %0+1:8] %0:9\n",
        "%31:9 = mux %1+1 [%30+8*2 %30+2:7] %30:9\n",
        "%32:9 = mux %1+2 [%31+8*4 %31+4:5] %31:9\n",
        "%33:0 = output \"e\" %32:9\n",
        "%40:9 = mux %1+0 [X %0+1:8] %0:9\n",
        "%41:9 = mux %1+1 [XX %40+2:7] %40:9\n",
        "%42:9 = mux %1+2 [XXXX %41+4:5] %41:9\n",
        "%43:0 = output \"f\" %42:9\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
