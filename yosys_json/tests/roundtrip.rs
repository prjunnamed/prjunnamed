use prjunnamed_netlist::{Design, parse, assert_isomorphic, MetaItem};
use prjunnamed_yosys_json::{import, export};
use std::collections::BTreeMap;
use std::io;

fn roundtrip(design: Design) -> Design {
    let mut buffer = Vec::<u8>::new();
    let designs = BTreeMap::from([("top".to_owned(), design)]);
    export(&mut buffer, designs).ok().unwrap();
    let mut cursor = io::Cursor::new(&buffer);
    let designs2 = import(None, &mut cursor).unwrap();
    designs2.into_values().next().unwrap()
}

#[test]
fn test_roundtrip() {
    let mut design = parse(
        None,
        r#"
		!0 = source "top.v" (#2 #13) (#2 #15)
		%0:1 = input "a"
		; source file://top.v#3
		%4:1 = eq 0 %0 !0
		%1:0 = output "y" %4
		%2:0 = name "a" %0
		%3:0 = name "y" %4
	"#,
    )
    .unwrap();

    let mut design2 = roundtrip(design.clone());
    let cell1 = design.find_cell(design.find_output("y").unwrap().all_inputs().unwrap_net()).ok().unwrap().0;
    let cell2 = design2.find_cell(design2.find_output("y").unwrap().all_inputs().unwrap_net()).ok().unwrap().0;

    let MetaItem::Source { start: start1, end: end1, .. } = cell1.metadata().get() else {
        panic!("unexpected metadata");
    };
    let MetaItem::Source { start: start2, end: end2, .. } = cell2.metadata().get() else {
        panic!("unexpected metadata");
    };
    assert_eq!(start1, start2);
    assert_eq!(end1, end2);
    assert_isomorphic!(design, design2);
}
