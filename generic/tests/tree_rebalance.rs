use std::str::FromStr;

use prjunnamed_generic::{tree_rebalance, Normalize, SimpleAigOpt};
use prjunnamed_netlist::{assert_isomorphic, Design};

#[test]
fn test_tree_rebalance_and() {
    let mut design = Design::from_str(concat!(
        "%0:16 = input \"a\"\n",
        "%1:16 = and %0:16 [%1:15 1]\n",
        "%2:0 = output \"y\" %1+15\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt]);
    tree_rebalance(&mut design);
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
fn test_tree_rebalance_xor() {
    let mut design = Design::from_str(concat!(
        "%0:16 = input \"a\"\n",
        "%1:16 = xor %0:16 [%1:15 0]\n",
        "%2:0 = output \"y\" %1+15\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt]);
    tree_rebalance(&mut design);
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
        "%21:1 = xor %10 %11\n",
        "%23:1 = xor %12 %13\n",
        "%25:1 = xor %14 %15\n",
        "%27:1 = xor %16 %17\n",
        "%33:1 = xor %21 %23\n",
        "%37:1 = xor %25 %27\n",
        "%47:1 = xor %33 %37\n",
        "%50:0 = output \"y\" %47\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_tree_rebalance_skip() {
    let mut design = Design::from_str(concat!(
        "%0:4 = input \"a\"\n",
        "%1:4 = and %0:4 [%1:3 1]\n",
        "%2:0 = output \"y\" %1:4\n",
        "%3:4 = xor %0:4 [%3:3 0]\n",
        "%4:0 = output \"z\" %3:4\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt]);
    tree_rebalance(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"a\"\n",
        "%10:1 = aig %0+0 %0+1\n",
        "%11:1 = aig %0+2 %10\n",
        "%12:1 = aig %0+3 %11\n",
        "%20:1 = xor %0+0 %0+1\n",
        "%21:1 = xor %0+2 %20\n",
        "%22:1 = xor %0+3 %21\n",
        "%30:0 = output \"y\" [%12 %11 %10 %0+0]\n",
        "%31:0 = output \"z\" [%22 %21 %20 %0+0]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_tree_rebalance_and_xor() {
    let mut design = Design::from_str(concat!(
        "%0:11 = input \"a\"\n",
        "%11:4 = input \"b\"\n",
        "%16:1 = aig %0+0 %0+1\n",
        "%17:1 = aig %0+2 %16\n",
        "%18:1 = aig %0+3 %17\n",
        "%19:1 = aig %0+4 %18\n",
        "%20:1 = aig %0+5 %19\n",
        "%21:1 = aig %0+6 %20\n",
        "%22:1 = aig %0+7 %21\n",
        "%23:1 = aig %0+8 %22\n",
        "%24:1 = aig %0+9 %23\n",
        "%25:1 = aig %0+10 %24\n",
        "%26:1 = xor %11+0 %25\n",
        "%27:1 = xor %11+1 %26\n",
        "%28:1 = xor %11+2 %27\n",
        "%29:1 = xor %11+3 %28\n",
        "%15:0 = output \"y\" %29\n",
    ))
    .unwrap();
    design.rewrite(&[&Normalize, &SimpleAigOpt]);
    tree_rebalance(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:11 = input \"a\"\n",
        "%11:4 = input \"b\"\n",
        "%20:1 = aig %0+0 %0+1\n",
        "%21:1 = aig %0+2 %0+3\n",
        "%22:1 = aig %0+4 %0+5\n",
        "%23:1 = aig %0+6 %0+7\n",
        "%24:1 = aig %0+8 %0+9\n",
        "%30:1 = aig %0+10 %20\n",
        "%31:1 = aig %21 %22\n",
        "%32:1 = aig %23 %24\n",
        "%40:1 = aig %30 %31\n",
        "%41:1 = aig %32 %40\n",
        "%50:1 = xor %11+0 %11+1\n",
        "%51:1 = xor %11+2 %11+3\n",
        "%52:1 = xor %50 %51\n",
        "%53:1 = xor %41 %52\n",
        "%60:0 = output \"y\" %53\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
