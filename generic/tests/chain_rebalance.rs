use std::str::FromStr;

use prjunnamed_generic::{chain_rebalance, LowerEq, Normalize, SimpleAigOpt};
use prjunnamed_netlist::{assert_isomorphic, Design};

#[test]
fn test_chain_rebalance_and() {
    let mut design = Design::from_str(concat!(
        "%0:16 = input \"a\"\n",
        "%1:16 = and %0:16 [%1:15 1]\n",
        "%2:0 = output \"y\" %1:16\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt]);
    chain_rebalance(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:16 = input \"a\"\n",
        "%10:1 = aig %0+0 %0+1\n",
        "%11:1 = aig %0+2 %0+3\n",
        "%12:1 = aig %0+4 %0+5\n",
        "%13:1 = aig %0+6 %0+7\n",
        "%14:1 = aig %0+8 %0+9\n",
        "%15:1 = aig %0+10 %0+11\n",
        "%16:1 = aig %0+12 %0+13\n",
        "%17:1 = aig %0+14 %0+15\n",
        "%20:1 = aig %0+2 %10\n",
        "%21:1 = aig %10 %11\n",
        "%22:1 = aig %0+6 %12\n",
        "%23:1 = aig %12 %13\n",
        "%24:1 = aig %0+10 %14\n",
        "%25:1 = aig %14 %15\n",
        "%26:1 = aig %0+14 %16\n",
        "%27:1 = aig %16 %17\n",
        "%30:1 = aig %0+4 %21\n",
        "%31:1 = aig %21 %12\n",
        "%32:1 = aig %21 %22\n",
        "%33:1 = aig %21 %23\n",
        "%34:1 = aig %0+12 %25\n",
        "%35:1 = aig %25 %16\n",
        "%36:1 = aig %25 %26\n",
        "%37:1 = aig %25 %27\n",
        "%40:1 = aig %0+8 %33\n",
        "%41:1 = aig %33 %14\n",
        "%42:1 = aig %33 %24\n",
        "%43:1 = aig %33 %25\n",
        "%44:1 = aig %33 %34\n",
        "%45:1 = aig %33 %35\n",
        "%46:1 = aig %33 %36\n",
        "%47:1 = aig %33 %37\n",
        "%50:0 = output \"y\" [%47 %46 %45 %44 %43 %42 %41 %40 %33 %32 %31 %30 %21 %20 %10 %0+0]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_chain_rebalance_xor() {
    let mut design = Design::from_str(concat!(
        "%0:16 = input \"a\"\n",
        "%1:16 = xor %0:16 [%1:15 0]\n",
        "%2:0 = output \"y\" %1:16\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt]);
    println!("MEOW {design}");
    chain_rebalance(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:16 = input \"a\"\n",
        "%10:1 = xor %0+0 %0+1\n",
        "%11:1 = xor %0+2 %0+3\n",
        "%12:1 = xor %0+4 %0+5\n",
        "%13:1 = xor %0+6 %0+7\n",
        "%14:1 = xor %0+8 %0+9\n",
        "%15:1 = xor %0+10 %0+11\n",
        "%16:1 = xor %0+12 %0+13\n",
        "%17:1 = xor %0+14 %0+15\n",
        "%20:1 = xor %0+2 %10\n",
        "%21:1 = xor %10 %11\n",
        "%22:1 = xor %0+6 %12\n",
        "%23:1 = xor %12 %13\n",
        "%24:1 = xor %0+10 %14\n",
        "%25:1 = xor %14 %15\n",
        "%26:1 = xor %0+14 %16\n",
        "%27:1 = xor %16 %17\n",
        "%30:1 = xor %0+4 %21\n",
        "%31:1 = xor %21 %12\n",
        "%32:1 = xor %21 %22\n",
        "%33:1 = xor %21 %23\n",
        "%34:1 = xor %0+12 %25\n",
        "%35:1 = xor %25 %16\n",
        "%36:1 = xor %25 %26\n",
        "%37:1 = xor %25 %27\n",
        "%40:1 = xor %0+8 %33\n",
        "%41:1 = xor %33 %14\n",
        "%42:1 = xor %33 %24\n",
        "%43:1 = xor %33 %25\n",
        "%44:1 = xor %33 %34\n",
        "%45:1 = xor %33 %35\n",
        "%46:1 = xor %33 %36\n",
        "%47:1 = xor %33 %37\n",
        "%50:0 = output \"y\" [%47 %46 %45 %44 %43 %42 %41 %40 %33 %32 %31 %30 %21 %20 %10 %0+0]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_chain_rebalance_eq() {
    let mut design = Design::from_str(concat!(
        "%0:16 = input \"a\"\n",
        "%1:1 = eq %0:16 0000000000000000\n",
        "%2:0 = output \"y\" %1:1\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt, &LowerEq]);
    chain_rebalance(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:16 = input \"a\"\n",
        "%10:1 = aig !%0+0 !%0+1\n",
        "%11:1 = aig !%0+2 !%0+3\n",
        "%12:1 = aig !%0+4 !%0+5\n",
        "%13:1 = aig !%0+6 !%0+7\n",
        "%14:1 = aig !%0+8 !%0+9\n",
        "%15:1 = aig !%0+10 !%0+11\n",
        "%16:1 = aig !%0+12 !%0+13\n",
        "%17:1 = aig !%0+14 !%0+15\n",
        "%21:1 = aig %10 %11\n",
        "%23:1 = aig %12 %13\n",
        "%25:1 = aig %14 %15\n",
        "%27:1 = aig %16 %17\n",
        "%33:1 = aig %21 %23\n",
        "%37:1 = aig %25 %27\n",
        "%47:1 = aig %33 %37\n",
        "%50:0 = output \"y\" %47\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_chain_rebalance_decoder() {
    let mut design = Design::from_str(concat!(
        "%0:4 = input \"a\"\n",
        "%100:1 = eq %0:4 0000\n",
        "%101:1 = eq %0:4 0001\n",
        "%102:1 = eq %0:4 0010\n",
        "%103:1 = eq %0:4 0011\n",
        "%104:1 = eq %0:4 0100\n",
        "%105:1 = eq %0:4 0101\n",
        "%106:1 = eq %0:4 0110\n",
        "%107:1 = eq %0:4 0111\n",
        "%108:1 = eq %0:4 1000\n",
        "%109:1 = eq %0:4 1001\n",
        "%110:1 = eq %0:4 1010\n",
        "%111:1 = eq %0:4 1011\n",
        "%112:1 = eq %0:4 1100\n",
        "%113:1 = eq %0:4 1101\n",
        "%114:1 = eq %0:4 1110\n",
        "%115:1 = eq %0:4 1111\n",
        "%2:0 = output \"y\" [%115 %114 %113 %112 %111 %110 %109 %108 %107 %106 %105 %104 %103 %102 %101 %100]\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt, &LowerEq]);
    chain_rebalance(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"a\"\n",
        "%10:1 = aig !%0+0 !%0+1\n",
        "%11:1 = aig %0+0 !%0+1\n",
        "%12:1 = aig %0+1 !%0+0\n",
        "%13:1 = aig %0+0 %0+1\n",
        "%20:1 = aig !%0+2 !%0+3\n",
        "%21:1 = aig %0+2 !%0+3\n",
        "%22:1 = aig %0+3 !%0+2\n",
        "%23:1 = aig %0+2 %0+3\n",
        "%100:1 = aig %10 %20\n",
        "%101:1 = aig %11 %20\n",
        "%102:1 = aig %12 %20\n",
        "%103:1 = aig %13 %20\n",
        "%104:1 = aig %10 %21\n",
        "%105:1 = aig %11 %21\n",
        "%106:1 = aig %12 %21\n",
        "%107:1 = aig %13 %21\n",
        "%108:1 = aig %10 %22\n",
        "%109:1 = aig %11 %22\n",
        "%110:1 = aig %12 %22\n",
        "%111:1 = aig %13 %22\n",
        "%112:1 = aig %10 %23\n",
        "%113:1 = aig %11 %23\n",
        "%114:1 = aig %12 %23\n",
        "%115:1 = aig %13 %23\n",
        "%2:0 = output \"y\" [%115 %114 %113 %112 %111 %110 %109 %108 %107 %106 %105 %104 %103 %102 %101 %100]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_chain_rebalance_and_or() {
    let mut design = Design::from_str(concat!(
        "%0:17 = input \"a\"\n",
        "%100:1 = and %0+0 %0+1\n",
        "%101:1 = or %100 %0+2\n",
        "%102:1 = and %101 %0+3\n",
        "%103:1 = or %102 %0+4\n",
        "%104:1 = and %103 %0+5\n",
        "%105:1 = or %104 %0+6\n",
        "%106:1 = and %105 %0+7\n",
        "%107:1 = or %106 %0+8\n",
        "%108:1 = and %107 %0+9\n",
        "%109:1 = or %108 %0+10\n",
        "%110:1 = and %109 %0+11\n",
        "%111:1 = or %110 %0+12\n",
        "%112:1 = and %111 %0+13\n",
        "%113:1 = or %112 %0+14\n",
        "%114:1 = and %113 %0+15\n",
        "%115:1 = or %114 %0+16\n",
        "%2:0 = output \"y\" [%115 %114 %113 %112 %111 %110 %109 %108 %107 %106 %105 %104 %103 %102 %101 %100]\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt]);
    chain_rebalance(&mut design);
    // in SMT we trust.
}
