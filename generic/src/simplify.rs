use prjunnamed_netlist::{Cell, CellRef, ControlNet, Design, FlipFlop, IoBuffer, Net, ParamValue, Trit, Value};
use prjunnamed_pattern::{netlist_replace, patterns::*};

pub fn simplify(design: &mut Design) -> bool {
    let rules = netlist_replace! {
        [PBuf [PAny@a]]                 => a;

        [PNot [PConst@a]]               => a.not();
        [PNot [PNot [PAny@a]]]          => a;

        [PAnd [PConst@a] [PConst@b]]    => a.and(b);
        [PAnd [PAny@a]   [POnes]]       => a;
        [PAnd [POnes]    [PAny@a]]      => a;
        [PAnd [PAny@a]   [PZero]]       => Value::zero(a.len());
        [PAnd [PZero]    [PAny@a]]      => Value::zero(a.len());

        [POr  [PConst@a] [PConst@b]]    => a.or(b);
        [POr  [PAny@a]   [PZero]]       => a;
        [POr  [PZero]    [PAny@a]]      => a;
        [POr  [PAny@a]   [POnes]]       => Value::ones(a.len());
        [POr  [POnes]    [PAny@a]]      => Value::ones(a.len());

        [PXor [PConst@a] [PConst@b]]    => a.xor(b);
        [PXor [PAny@a]   [PZero]]       => a;
        [PXor [PZero]    [PAny@a]]      => a;
        [PXor [PAny@a]   [POnes]]       => design.add_not(a);
        [PXor [POnes]    [PAny@a]]      => design.add_not(a);
        [PXor [PAny@a]   [PUndef]]      => Value::undef(a.len());
        [PXor [PUndef]   [PAny@a]]      => Value::undef(a.len());
        [PXor [PAny@a]   [PAny@b]]      if a == b => Value::zero(a.len());

        [PAnd [PAny@a] [PNot [PAny@b]]] if a == b => Value::zero(a.len());
        [PAnd [PNot [PAny@a]] [PAny@b]] if a == b => Value::zero(a.len());
        [POr  [PAny@a] [PNot [PAny@b]]] if a == b => Value::ones(a.len());
        [POr  [PNot [PAny@a]] [PAny@b]] if a == b => Value::ones(a.len());
        [PXor [PAny@a] [PNot [PAny@b]]] if a == b => Value::ones(a.len());
        [PXor [PNot [PAny@a]] [PAny@b]] if a == b => Value::ones(a.len());

        [PMux [POnes]    [PAny@a]   [PAny]]         => a;
        [PMux [PZero]    [PAny]     [PAny@b]]       => b;
        [PMux [PAny]     [PAny@a]   [PUndef]]       => a;
        [PMux [PAny]     [PUndef]   [PAny@b]]       => b;
        [PMux [PAny]     [PAny@a]   [PAny@b]]       if a == b => a;

        [PMux@y [PAny@s] [POnes]    [PZero]]        => Value::from(s).sext(y.len());
        [PMux@y [PAny@s] [PZero]    [POnes]]        => design.add_not(s).sext(y.len());

        [PAnd [PNot [PAny@a]] [PNot [PAny@b]]]      => design.add_not(design.add_or (a, b));
        [POr  [PNot [PAny@a]] [PNot [PAny@b]]]      => design.add_not(design.add_and(a, b));

        [PXor [PNot [PAny@a]] [PNot [PAny@b]]]      => design.add_xor(a, b);
        [PXor [PNot [PAny@a]] [PAny@b]]             => design.add_not(design.add_xor(a, b));
        [PXor [PAny@a]        [PNot [PAny@b]]]      => design.add_not(design.add_xor(a, b));

        [PMux [PNot [PAny@s]] [PAny@a] [PAny@b]]    => design.add_mux(s, b, a);

        [PAdc   [PConst@a] [PConst@b] [PConst@c]]   => a.adc(b, c);
        [PAdc@y [PAny@a]   [PZero]    [PZero]]      => a.zext(y.len());
        [PAdc@y [PZero]    [PAny@b]   [PZero]]      => b.zext(y.len());
        [PAdc@y [PZero]    [PZero]    [PAny@c]]     => Value::from(c).zext(y.len());

        [PAdc   [PAny@a]   [PAny@b]   [PAny@c]]     if let Some(y) = adc_split(design, a, b, c) => y;
        [PAdc   [PAny@a]   [PAny@b]   [PAny@c]]     if let Some(y) = adc_unsext(design, a, b, c) => y;

        [PAdc@y [PAdc [PAny@a] [PAny@b] [PZero]] [PZExt [PAny@c]] [PZero]] if c.len() == 1 =>
            design.add_adc(a, b, c[0]).zext(y.len());

        [PEq  [PConst@a] [PConst@b]]    => a.eq(b);
        [PEq  [PAny@a]   [POnes]]       if a.len() == 1 => a;
        [PEq  [POnes]    [PAny@a]]      if a.len() == 1 => a;
        [PEq  [PAny@a]   [PZero]]       if a.len() == 1 => design.add_not(a);
        [PEq  [PZero]    [PAny@a]]      if a.len() == 1 => design.add_not(a);
        [PEq  [PAny@a]   [PAny@b]]      if a == b => Trit::One;

        [PULt [PConst@a] [PConst@b]]    => a.ult(b);
        [PULt [PAny]     [PZero]]       => Trit::Zero;
        [PULt [POnes]    [PAny]]        => Trit::Zero;
        [PULt [PHasX]    [PAny]]        => Trit::Undef;
        [PULt [PAny]     [PHasX]]       => Trit::Undef;
        [PULt [PAny@a]   [PAny@b]]      if a == b => Trit::Zero;

        [PSLt [PConst@a] [PConst@b]]    => a.slt(b);
        [PSLt [PHasX]    [PAny]]        => Trit::Undef;
        [PSLt [PAny]     [PHasX]]       => Trit::Undef;
        [PSLt [PAny@a]   [PAny@b]]      if a == b => Trit::Zero;

        [PShl    [PAny@a] [PConst@b] [PAny@s]]  => a.shl(b, s);
        [PShl    [PAny@a] [PAny]     [PZero]]   => a;
        [PShl@y  [PZero]  [PAny]     [PAny]]    => Value::zero(y.len());

        [PUShr   [PAny@a] [PConst@b] [PAny@s]]  => a.ushr(b, s);
        [PUShr   [PAny@a] [PAny]     [PZero]]   => a;
        [PUShr@y [PZero]  [PAny]     [PAny]]    => Value::zero(y.len());

        [PSShr   [PAny@a] [PConst@b] [PAny@s]]  => a.sshr(b, s);
        [PSShr   [PAny@a] [PAny]     [PZero]]   => a;
        [PSShr@y [PZero]  [PAny]     [PAny]]    => Value::zero(y.len());
        [PSShr@y [POnes]  [PAny]     [PAny]]    => Value::ones(y.len());

        [PXShr   [PAny@a] [PConst@b] [PAny@s]]  => a.xshr(b, s);
        [PXShr   [PAny@a] [PAny]     [PZero]]   => a;
        [PXShr@y [PZero]  [PAny]     [PAny]]    => Value::zero(y.len());

        [PMul    [PConst@a] [PConst@b]] => a.mul(b);
        [PMul@y  [PAny]     [PHasX]]    => Value::undef(y.len());
        [PMul@y  [PHasX]    [PAny]]     => Value::undef(y.len());
        [PMul@y  [PAny]     [PZero]]    => Value::zero(y.len());
        [PMul@y  [PZero]    [PAny]]     => Value::zero(y.len());
        [PMul    [PAny@a]   [PPow2@b]]  => a.shl(Trit::One, b);
        [PMul    [PPow2@a]  [PAny@b]]   => b.shl(Trit::One, a);

        // [PUDiv   [PConst@a] [PConst@b]] => a.udiv(b);
        [PUDiv@y [PZero]    [PAny]]     => Value::zero(y.len());
        [PUDiv@y [PAny]     [PZero]]    => Value::undef(y.len());
        [PUDiv   [PAny@a]   [PPow2@b]]  => a.ushr(Trit::One, b);

        // [PUMod   [PConst@a] [PConst@b]] => a.umod(b);
        [PUMod@y [PZero]    [PAny]]     => Value::zero(y.len());
        [PUMod@y [PAny]     [PZero]]    => Value::undef(y.len());
        [PUMod   [PAny@a]   [PPow2@b]]  =>
            if (b as usize) < a.len() { Value::from(&a[..(b as usize)]).zext(a.len()) } else { a };

        [PSDivTrunc@y [PZero] [PAny]]   => Value::zero(y.len());
        [PSDivTrunc@y [PAny]  [PZero]]  => Value::undef(y.len());
        [PSModTrunc@y [PZero] [PAny]]   => Value::zero(y.len());
        [PSModTrunc@y [PAny]  [PZero]]  => Value::undef(y.len());

        [PSDivFloor@y [PZero] [PAny]]   => Value::zero(y.len());
        [PSDivFloor@y [PAny]  [PZero]]  => Value::undef(y.len());
        [PSModFloor@y [PZero] [PAny]]   => Value::zero(y.len());
        [PSModFloor@y [PAny]  [PZero]]  => Value::undef(y.len());

        [PDff@ff [PAny]] if ff.clear.is_always(true) => Value::from(ff.clear_value);

        [PDff@ff [PMux [PAny@r] [PConst@rv] [PAny@d]]] if ff.reset.is_always(false) =>
            design.add_dff(ff.clone().with_data(d).with_reset_value(ControlNet::Pos(r), rv));
        [PDff@ff [PMux [PAny@r] [PAny@d] [PConst@rv]]] if ff.reset.is_always(false) =>
            design.add_dff(ff.clone().with_data(d).with_reset_value(ControlNet::Neg(r), rv));

        [PBind@ffq [PDff@ff [PMux [PAny@en] [PAny@d] [PAny@q]]]] if ff.enable.is_always(true) && ffq == q =>
            design.add_dff(ff.clone().with_data(d).with_enable(ControlNet::Pos(en)));
        [PBind@ffq [PDff@ff [PMux [PAny@en] [PAny@q] [PAny@d]]]] if ff.enable.is_always(true) && ffq == q =>
            design.add_dff(ff.clone().with_data(d).with_enable(ControlNet::Neg(en)));
    };

    for cell_ref in design.iter_cells() {
        // Fold inverters into controls first. This only ever replaces the cells themselves.
        fold_controls(design, cell_ref);
        // Rules that match individual bits of a cell are more powerful, but some rules must match an entire cell.
        let value = cell_ref.output();
        if rules(design, &value) {
            continue;
        }
        for net in &value {
            rules(design, &Value::from(net));
        }
    }
    design.compact()
}

/// Finds positions within an `adc` cell where the carry chain up to the position
/// doesn't affect output bits past the position, then splits the cell into smaller
/// `adc` cells at these positions.
///
/// This function recognizes the following cases:
///
/// 1. `a` and `b` have the same net at position `i`:
///
///    ```
///       { a7 a6 a5 ab4 a3 a2 a1 a0 } +
///       { b7 b6 b5 ab4 b3 b2 b1 b0 } + ci =
///    { y8 y7 y6 y5  y4 y3 y2 y1 y0 }
///    ```
///
///    In this case, the carry out of position `i` will always be equal to the value
///    of the common net.  The sum at that position will be equal to carry out of position
///    `i - 1` (or to carry input, for position 0).  The above example gets split into two
///    `adc` cells:
///
///    ```
///    { y8 y7 y6 y5 } =
///       { a7 a6 a5 } +
///       { b7 b6 b5 } + ab4
///
///    { y4 y3 y2 y1 y0 } =
///       { a3 a2 a1 a0 } +
///       { b3 b2 b1 b0 } + ci
///    ```
///
/// 2. The low bit of `a` (or `b`) is the same net as the carry input:
///
///    ```
///    { y4 y3 y2 y1 y0 } =
///       { a3 a2 a1 a0 } +
///       { b3 b2 b1 b0 } + a0
///    ```
///
///    In this case, The carry out of position `0` will always be equal to `a0`, and the sum
///    at this position will be equal to `b0`.  The above example gets split up as follows:
///
///    ```
///    { y4 y3 y2 y1 } =
///       { a3 a2 a1 } +
///       { b3 b2 b1 } + a0
///
///    y0 = b0
///    ```
///
/// This optimization is comically general.  We are mostly interested in it for its useful special
/// cases:
///
/// - `a + a` will get optimized into `{ a 0 }`
/// - `a + a + ci` will get optimized into `{ a ci }`
/// - when both operands have const LSBs, the low part is const-folded
/// - when one operand has const-0 LSBs, and the CI is 0, the low bits of output are replaced with
///   bits of the other operand
/// - when one operand has const-1 LSBs, and the CI is 1, the low bits of output are replaced with
///   bits of the other operand
/// - when the two operands have disjoint sets of non-zero positions (eg. `{ a0 a1 0 0 } + { 0 0 b2 b3 }`,
///   the whole cell is replaced by a buffer
/// - when there's a stretch of zeros in both operands at the same positions, the cell gets split into two `adc`
/// - when both operands have redundant 0 bits at MSBs, the high bits of output are replaced with 0
fn adc_split(design: &Design, a: Value, b: Value, c: Net) -> Option<Value> {
    let mut ci = c;
    let mut result = Value::new();
    for (offset, (a_bit, b_bit)) in a.iter().zip(b.iter()).enumerate() {
        // Note that the following cases are enough to do constant-folding as
        // well: either the two bits are the same, or they are different.
        // If they are different, one of them must be equal to whatever
        // ci currently is.
        if a_bit == b_bit {
            result.extend(make_adc(design, &a[result.len()..offset], &b[result.len()..offset], ci));
            ci = a_bit;
        } else if offset == result.len() {
            if a_bit == ci {
                result.extend([b_bit]);
                // ci unchanged
            } else if b_bit == ci {
                result.extend([a_bit]);
                // ci unchanged
            }
        }
    }
    if result.is_empty() {
        return None;
    }
    result.extend(make_adc(design, &a[result.len()..], &b[result.len()..], ci));
    Some(result)
}

/// Finds runs of three or more repeating pairs of bits in the inputs of an `adc` cell, shortens
/// them to just two pairs.  Mostly useful for shortening over-long `adc` cells with sign-extended
/// inputs.
///
/// Observation: given
///
/// ```
/// { a a } +
/// { b b } + c
/// ```
///
/// the carry out of position 1 will always be the same as carry out of position 0:
///
/// - if `a` and `b` are both `0`, the carry-out is `0` on both positions
/// - if `a` and `b` are both `1`, the carry-out is `1` on both positions
/// - if `a` and `b` are `0` and `1` (or the other way around), the carry-in of `c` is propagated
///
/// Thus, if we have
///
/// ```
/// { y3 y2 y1 y0 } =
///     { a  a  a } +
///     { b  b  b } + c
/// ```
///
/// it follows that carry-in to positions 1 and 2 is the same, and so y1 = y2.  Further,
/// carry out of position 2 is also the same, allowing us to completely remove position 2
/// from the `adc` as redundant:
///
/// ```
/// { y3 y1 y0 } =
///     { a  a } +
///     { b  b } + c
/// y2 = y1
/// ```
///
/// This applies to any repetition of length ≥ 3.  While mostly applicable to trimming MSBs,
/// we recognize such repetitions anywhere within the inputs and split the `adc` cell there.
fn adc_unsext(design: &Design, a: Value, b: Value, c: Net) -> Option<Value> {
    let mut ci = c;
    let mut offset = 0;
    let mut result = Value::new();
    while offset + 2 < a.len() {
        let a_bit = a[offset];
        let b_bit = b[offset];
        let same_count =
            a[offset + 1..].iter().zip(&b[offset + 1..]).take_while(|&pair| pair == (&a_bit, &b_bit)).count() + 1;

        if same_count < 3 {
            offset += same_count;
            continue;
        }

        let start_offset = result.len();
        let end_offset = offset + same_count;
        let chunk = make_adc(design, &a[start_offset..offset + 2], &b[start_offset..offset + 2], ci);
        let chunk_sum = chunk.slice(..chunk.len() - 1); // drop cout
        result.extend(chunk_sum.sext(end_offset - start_offset));
        ci = chunk.msb();
        offset = end_offset;
    }
    if result.is_empty() {
        return None;
    }
    result.extend(make_adc(design, &a[result.len()..], &b[result.len()..], ci));
    Some(result)
}

/// Create an `adc` cell if operands have a non-zero width.
fn make_adc(design: &Design, a: impl Into<Value>, b: impl Into<Value>, ci: impl Into<Net>) -> Value {
    let a = a.into();
    let b = b.into();
    let ci = ci.into();
    if a.is_empty() {
        assert!(b.is_empty());
        ci.into()
    } else {
        design.add_adc(a, b, ci)
    }
}

fn fold_controls(design: &Design, cell_ref: CellRef) {
    let uninvert = |net: Net| -> Option<Net> {
        if let Ok((cell_ref, offset)) = design.find_cell(net) {
            if let Cell::Not(value) = &*cell_ref.get() {
                return Some(value[offset]);
            }
        }
        None
    };

    let fold_control_net = |control_net: ControlNet| {
        if let Some(net) = uninvert(control_net.net()) {
            if control_net.is_positive() {
                ControlNet::Neg(net)
            } else {
                ControlNet::Pos(net)
            }
        } else {
            control_net.canonicalize()
        }
    };

    match &*cell_ref.get() {
        Cell::Dff(flip_flop) => {
            let mut flip_flop = FlipFlop {
                clock: fold_control_net(flip_flop.clock),
                clear: fold_control_net(flip_flop.clear),
                load: fold_control_net(flip_flop.load),
                reset: fold_control_net(flip_flop.reset),
                enable: fold_control_net(flip_flop.enable),
                ..flip_flop.clone()
            };
            if flip_flop.clock.is_const() {
                flip_flop.data = Value::undef(flip_flop.data.len());
                flip_flop.clock = ControlNet::ZERO;
                flip_flop.reset = ControlNet::ZERO;
                flip_flop.enable = ControlNet::ZERO;
            }
            if flip_flop.reset.is_always(true) {
                flip_flop.data = flip_flop.reset_value.clone().into();
                flip_flop.reset = ControlNet::ZERO;
                if flip_flop.reset_over_enable {
                    flip_flop.enable = ControlNet::ONE;
                }
            }
            if flip_flop.enable.is_always(false) {
                if flip_flop.reset_over_enable {
                    flip_flop.data = flip_flop.reset_value.clone().into();
                    flip_flop.enable = flip_flop.reset;
                    flip_flop.reset = ControlNet::ZERO;
                } else {
                    flip_flop.data = Value::undef(flip_flop.data.len());
                    flip_flop.clock = ControlNet::ZERO;
                    flip_flop.reset = ControlNet::ZERO;
                    flip_flop.enable = ControlNet::ZERO;
                }
            }
            cell_ref.replace(Cell::Dff(flip_flop));
        }
        Cell::IoBuf(io_buffer) => {
            cell_ref.replace(Cell::IoBuf(IoBuffer {
                io: io_buffer.io.clone(),
                output: io_buffer.output.clone(),
                enable: fold_control_net(io_buffer.enable),
            }));
        }
        Cell::Target(target_cell) => {
            let mut target_cell = target_cell.clone();
            let prototype = design.target_prototype(&target_cell);
            for input in prototype.inputs.iter() {
                let Some(invert_param_index) = input.invert_param else { continue };
                let ParamValue::Const(ref mut invert_const) = target_cell.params[invert_param_index] else {
                    unreachable!()
                };
                for offset in input.range.clone() {
                    let const_offset = offset - input.range.start;
                    let Some(net) = uninvert(target_cell.inputs[offset]) else { continue };
                    target_cell.inputs[offset] = net;
                    invert_const[const_offset] = !invert_const[const_offset];
                }
            }
            cell_ref.replace(Cell::Target(target_cell));
        }
        _ => (),
    }
}

#[cfg(test)]
mod test {
    use std::{collections::BTreeMap, sync::Arc};

    use prjunnamed_netlist::{
        assert_isomorphic, Const, ControlNet, Design, FlipFlop, IoBuffer, Net, Target, TargetCell, TargetImportError,
        TargetPrototype, Trit, Value,
    };
    use prjunnamed_pattern::{assert_netlist, netlist_match, patterns::*};

    use super::simplify;

    macro_rules! assert_simplify {
        ( $( |$design:ident| $build:expr ),+ ; $( $match:tt )+ ) => {
            let rules = netlist_match! { $( $match )+ };
            $(
                let mut $design = Design::new();
                $design.add_output("y", $build);
                $design.apply();
                let design_before = $design.clone();
                simplify(&mut $design);
                assert_netlist!($design, rules, "simplify failed on:\n{}\nresult:\n{}", design_before, $design);
            )+
        };
    }

    macro_rules! assert_simplify_isomorphic {
        ( $design:ident, $gold:ident ) => {
            let (mut design, mut gold) = ($design, $gold);
            design.apply();
            simplify(&mut design);
            gold.apply();
            assert_isomorphic!(design, gold);
        };
    }

    macro_rules! assert_no_simplify {
        ( $design:ident ) => {
            let mut design = $design;
            design.apply();
            assert!(!simplify(&mut design));
        };
    }

    fn iter_interesting_consts() -> impl Iterator<Item = Const> {
        ["0", "1", "X", "00", "11", "XX", "01", "10"].into_iter().map(Const::lit)
    }

    fn iter_has_undef_consts() -> impl Iterator<Item = Const> {
        ["X", "XX", "0X", "X1"].into_iter().map(Const::lit)
    }

    fn iter_interesting_const_pairs() -> impl Iterator<Item = (Const, Const)> {
        iter_interesting_consts().flat_map(|value1| {
            iter_interesting_consts().filter_map(move |value2| {
                if value1.len() == value2.len() {
                    Some((value1.clone(), value2))
                } else {
                    None
                }
            })
        })
    }

    #[allow(dead_code)]
    #[derive(Debug)]
    struct MockTarget {
        prototypes: BTreeMap<String, TargetPrototype>,
    }

    impl MockTarget {
        #[allow(dead_code)]
        fn new() -> Arc<Self> {
            Arc::new(MockTarget {
                prototypes: BTreeMap::from_iter([
                    (
                        "BUF1".into(),
                        TargetPrototype::new_has_effects()
                            .add_param_bits("INVERT", Const::zero(1))
                            .add_input_invertible("I", Const::zero(1), "INVERT")
                            .add_output("O", 1),
                    ),
                    (
                        "BUF2".into(),
                        TargetPrototype::new_has_effects()
                            .add_param_bits("INVERT", Const::zero(2))
                            .add_input_invertible("I", Const::zero(2), "INVERT")
                            .add_output("O", 2),
                    ),
                ]),
            })
        }
    }

    impl Target for MockTarget {
        fn name(&self) -> &str {
            "mock"
        }

        fn options(&self) -> BTreeMap<String, String> {
            BTreeMap::new()
        }

        fn prototype(&self, name: &str) -> Option<&TargetPrototype> {
            self.prototypes.get(name)
        }

        fn validate(&self, _design: &Design, _cell: &TargetCell) {}

        fn import(&self, _design: &mut Design) -> Result<(), TargetImportError> {
            Ok(())
        }

        fn export(&self, _design: &mut Design) {}

        fn synthesize(&self, _design: &mut Design) -> Result<(), ()> {
            Ok(())
        }
    }

    #[test]
    fn test_not_const_eval() {
        for value in iter_interesting_consts() {
            assert_simplify!(
                |design| design.add_not(&value);
                [PConst@a] => (a == value.not());
            );
        }
    }

    #[test]
    fn test_not_not() {
        assert_simplify!(
            |ds| ds.add_not(ds.add_not(ds.add_input("a", 2)));
            [PInput ("a")] => true;
        );
    }

    #[test]
    fn test_and_const_eval() {
        for (value1, value2) in iter_interesting_const_pairs() {
            assert_simplify!(
                |ds| ds.add_and(&value1, &value2);
                [PConst@a] => (a == value1.and(&value2));
            );
        }
    }

    #[test]
    fn test_and_fold() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_and(Const::zero(size), ds.add_input("a", size)),
                |ds| ds.add_and(ds.add_input("a", size), Const::zero(size));
                [PZero] => true;
            );
            assert_simplify!(
                |ds| ds.add_and(Const::ones(size), ds.add_input("a", size)),
                |ds| ds.add_and(ds.add_input("a", size), Const::ones(size));
                [PInput ("a")] => true;
            );
            assert_simplify!(
                |ds| { let a = ds.add_input("a", size); ds.add_and(&a, ds.add_not(&a)) },
                |ds| { let a = ds.add_input("a", size); ds.add_and(ds.add_not(&a), &a) };
                [PZero] => true;
            );
        }
    }

    #[test]
    fn test_or_const_eval() {
        for (value1, value2) in iter_interesting_const_pairs() {
            assert_simplify!(
                |ds| ds.add_or(&value1, &value2);
                [PConst@a] => a == value1.or(&value2);
            );
        }
    }

    #[test]
    fn test_or_fold() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_or(Const::zero(size), ds.add_input("a", size)),
                |ds| ds.add_or(ds.add_input("a", size), Const::zero(size));
                [PInput ("a")] => true;
            );
            assert_simplify!(
                |ds| ds.add_or(Const::ones(size), ds.add_input("a", size)),
                |ds| ds.add_or(ds.add_input("a", size), Const::ones(size));
                [POnes] => true;
            );
            assert_simplify!(
                |ds| { let a = ds.add_input("a", size); ds.add_or(&a, ds.add_not(&a)) },
                |ds| { let a = ds.add_input("a", size); ds.add_or(ds.add_not(&a), &a) };
                [POnes] => true;
            );
        }
    }

    #[test]
    fn test_xor_const_eval() {
        for (value1, value2) in iter_interesting_const_pairs() {
            assert_simplify!(
                |ds| ds.add_xor(&value1, &value2);
                [PConst@a] => a == value1.xor(&value2);
            );
        }
    }

    #[test]
    fn test_xor_fold() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_xor(Const::zero(size), ds.add_input("a", size)),
                |ds| ds.add_xor(ds.add_input("a", size), Const::zero(size));
                [PInput ("a")] => true;
            );
            assert_simplify!(
                |ds| ds.add_xor(Const::ones(size), ds.add_input("a", size)),
                |ds| ds.add_xor(ds.add_input("a", size), Const::ones(size));
                [PNot [PInput ("a")]] => true;
            );
            assert_simplify!(
                |ds| ds.add_xor(Const::undef(size), ds.add_input("a", size)),
                |ds| ds.add_xor(ds.add_input("a", size), Const::undef(size));
                [PUndef] => true;
            );
            assert_simplify!(
                |ds| { let a = ds.add_input("a", size); ds.add_xor(&a, ds.add_not(&a)) },
                |ds| { let a = ds.add_input("a", size); ds.add_xor(ds.add_not(&a), &a) };
                [POnes] => true;
            );
            assert_simplify!(
                |ds| {let a = ds.add_input("a", size); ds.add_xor(&a, &a)};
                [PZero] => true;
            );
        }
    }

    #[test]
    fn test_mux_fold() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_mux(Net::ZERO, ds.add_input("a", size), ds.add_input("b", size));
                [PInput ("b")] => true;
            );
            assert_simplify!(
                |ds| ds.add_mux(Net::ONE, ds.add_input("a", size), ds.add_input("b", size));
                [PInput ("a")] => true;
            );
            assert_simplify!(
                |ds| ds.add_mux(ds.add_input1("s"), ds.add_input("a", size), Const::undef(size));
                [PInput ("a")] => true;
            );
            assert_simplify!(
                |ds| ds.add_mux(ds.add_input1("s"), Const::undef(size), ds.add_input("b", size));
                [PInput ("b")] => true;
            );
            assert_simplify!(
                |ds| { let a = ds.add_input("a", size); ds.add_mux(ds.add_input1("s"), &a, &a) };
                [PInput ("a")] => true;
            );
            assert_simplify!(
                |ds| ds.add_mux(ds.add_input1("s"), Const::ones(size), Const::zero(size));
                [PSExt [PAny@s]] if s.len() == 1 => true;
            );
        }
    }

    #[test]
    fn test_demorgan() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_and(ds.add_not(ds.add_input("a", size)), ds.add_not(ds.add_input("b", size)));
                [PNot [POr [PInput ("a")] [PInput ("b")]]] => true;
            );
            assert_simplify!(
                |ds| ds.add_or(ds.add_not(ds.add_input("a", size)), ds.add_not(ds.add_input("b", size)));
                [PNot [PAnd [PInput ("a")] [PInput ("b")]]] => true;
            );
        }
    }

    #[test]
    fn test_xor_not_push() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_xor(ds.add_not(ds.add_input("a", size)), ds.add_not(ds.add_input("b", size)));
                [PXor [PInput ("a")] [PInput ("b")]] => true;
            );
            assert_simplify!(
                |ds| ds.add_xor(ds.add_not(ds.add_input("a", size)), ds.add_input("b", size)),
                |ds| ds.add_xor(ds.add_input("a", size), ds.add_not(ds.add_input("b", size)));
                [PNot [PXor [PInput ("a")] [PInput ("b")]]] => true;
            );
        }
    }

    #[test]
    fn test_mux_flip() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_mux(ds.add_not(ds.add_input("s", 1))[0], ds.add_input("a", size), ds.add_input("b", size));
                [PMux [PInput ("s")] [PInput ("b")] [PInput ("a")]] => true;
            );
        }
    }

    #[test]
    fn test_adc_const_eval() {
        for carry in [Trit::Zero, Trit::One, Trit::Undef] {
            for (value1, value2) in iter_interesting_const_pairs() {
                assert_simplify!(
                    |ds| ds.add_adc(&value1, &value2, carry);
                    [PConst@a] => a == value1.adc(&value2, carry);
                );
            }
        }
    }

    #[test]
    fn test_adc_fold() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_adc(Const::zero(size), ds.add_input("a", size), Net::ZERO),
                |ds| ds.add_adc(ds.add_input("a", size), Const::zero(size), Net::ZERO);
                [PZExt [PInput ("a")]] => true;
            );
            assert_simplify!(
                |ds| ds.add_adc(Const::zero(size), Const::zero(size), ds.add_input("c", 1)[0]);
                [PZExt [PInput ("c")]] => true;
            );
        }
    }

    #[test]
    fn test_adc_split_case1() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let b = design.add_input("b", 4);
        let ab = design.add_input("ab", 1);
        let c = design.add_input("c", 1);
        let y = design.add_adc(
            a.slice(..2).concat(&ab).concat(a.slice(2..)),
            b.slice(..2).concat(&ab).concat(b.slice(2..)),
            c.unwrap_net(),
        );
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let b = gold.add_input("b", 4);
        let ab = gold.add_input("ab", 1);
        let c = gold.add_input("c", 1);
        let y0 = gold.add_adc(a.slice(..2), b.slice(..2), c.unwrap_net());
        let y2 = gold.add_adc(a.slice(2..), b.slice(2..), ab.unwrap_net());
        gold.add_output("y", y0.concat(y2));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_case1_top() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let b = design.add_input("b", 4);
        let ab = design.add_input("ab", 1);
        let c = design.add_input("c", 1);
        let y = design.add_adc(a.concat(&ab), b.concat(&ab), c.unwrap_net());
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let b = gold.add_input("b", 4);
        let ab = gold.add_input("ab", 1);
        let c = gold.add_input("c", 1);
        let y = gold.add_adc(a, b, c.unwrap_net());
        gold.add_output("y", y.concat(ab));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_case2() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let b = design.add_input("b", 4);
        let y = design.add_adc(&a, &b, b.lsb());
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let b = gold.add_input("b", 4);
        let y = gold.add_adc(&a[1..], &b[1..], b.lsb());
        gold.add_output("y", Value::from(a.lsb()).concat(y));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_case2_swap() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let b = design.add_input("b", 4);
        let y = design.add_adc(&a, &b, a.lsb());
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let b = gold.add_input("b", 4);
        let y = gold.add_adc(&a[1..], &b[1..], a.lsb());
        gold.add_output("y", Value::from(b.lsb()).concat(y));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_case2_top() {
        let mut design = Design::new();
        let a = design.add_input("a", 1);
        let b = design.add_input("b", 1);
        let y = design.add_adc(&a, &b, b.lsb());
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 1);
        let b = gold.add_input("b", 1);
        gold.add_output("y", a.concat(b));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_a_plus_a() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let y = design.add_adc(&a, &a, Net::ZERO);
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        gold.add_output("y", Value::from(Net::ZERO).concat(a));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_a_plus_a_carry() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let c = design.add_input("c", 1);
        let y = design.add_adc(&a, &a, c.unwrap_net());
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let c = gold.add_input("c", 1);
        gold.add_output("y", c.concat(a));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_const_lsb() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let b = design.add_input("b", 4);
        let y = design.add_adc(
            Value::from(Const::lit("0111")).concat(&a),
            Value::from(Const::lit("1101")).concat(&b),
            Net::ZERO,
        );
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let b = gold.add_input("b", 4);
        let y = gold.add_adc(a, b, Net::ONE);
        gold.add_output("y", Value::from(Const::lit("0100")).concat(&y));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_const_zero_lsb() {
        let mut design = Design::new();
        let a = design.add_input("a", 8);
        let b = design.add_input("b", 4);
        let y = design.add_adc(a, Value::zero(4).concat(&b), Net::ZERO);
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 8);
        let b = gold.add_input("b", 4);
        let y = gold.add_adc(&a[4..], b, Net::ZERO);
        gold.add_output("y", a.slice(..4).concat(&y));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_const_ones_lsb() {
        let mut design = Design::new();
        let a = design.add_input("a", 8);
        let b = design.add_input("b", 4);
        let y = design.add_adc(a, Value::from(Const::lit("1111")).concat(&b), Net::ONE);
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 8);
        let b = gold.add_input("b", 4);
        let y = gold.add_adc(&a[4..], b, Net::ONE);
        gold.add_output("y", a.slice(..4).concat(&y));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_disjoint() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let b = design.add_input("b", 4);
        let y = design.add_adc(a.concat(Value::zero(4)), Value::zero(4).concat(&b), Net::ZERO);
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let b = gold.add_input("b", 4);
        gold.add_output("y", a.concat(&b).concat(Net::ZERO));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_zeros() {
        let mut design = Design::new();
        let al = design.add_input("al", 4);
        let ah = design.add_input("ah", 4);
        let bl = design.add_input("bl", 4);
        let bh = design.add_input("bh", 4);
        let y = design.add_adc(al.concat(Value::zero(4)).concat(ah), bl.concat(Value::zero(4)).concat(bh), Net::ZERO);
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let al = gold.add_input("al", 4);
        let ah = gold.add_input("ah", 4);
        let bl = gold.add_input("bl", 4);
        let bh = gold.add_input("bh", 4);
        let yl = gold.add_adc(al, bl, Net::ZERO);
        let yh = gold.add_adc(ah, bh, Net::ZERO);
        gold.add_output("y", yl.concat(Value::zero(3)).concat(yh));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_split_zext() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let b = design.add_input("b", 4);
        let c = design.add_input("c", 1);
        let y = design.add_adc(a.concat(Value::zero(4)), b.concat(Value::zero(4)), c.unwrap_net());
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let b = gold.add_input("b", 4);
        let c = gold.add_input("c", 1);
        let y = gold.add_adc(a, b, c.unwrap_net());
        gold.add_output("y", y.concat(Value::zero(4)));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_unsext_sext() {
        let mut design = Design::new();
        let a = design.add_input("a", 4);
        let b = design.add_input("b", 4);
        let c = design.add_input("c", 1);
        let y = design.add_adc(a.sext(8), b.sext(8), c.unwrap_net());
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let a = gold.add_input("a", 4);
        let b = gold.add_input("b", 4);
        let c = gold.add_input("c", 1);
        let y = gold.add_adc(a.sext(5), b.sext(5), c.unwrap_net());
        gold.add_output("y", y.slice(..5).sext(8).concat(y.msb()));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_unsext_bi() {
        let mut design = Design::new();
        let al = design.add_input("al", 1);
        let bl = design.add_input("bl", 1);
        let ah = design.add_input("ah", 1);
        let bh = design.add_input("bh", 1);
        let c = design.add_input("c", 1);
        let y = design.add_adc(al.sext(4).concat(ah.sext(4)), bl.sext(4).concat(bh.sext(4)), c.unwrap_net());
        design.add_output("y", y);
        design.apply();
        simplify(&mut design);
        let mut gold = Design::new();
        let al = gold.add_input("al", 1);
        let bl = gold.add_input("bl", 1);
        let ah = gold.add_input("ah", 1);
        let bh = gold.add_input("bh", 1);
        let c = gold.add_input("c", 1);
        let y0 = gold.add_adc(al.sext(2), bl.sext(2), c.unwrap_net());
        let y1 = gold.add_adc(ah.sext(2), bh.sext(2), y0.msb());
        gold.add_output("y", y0.slice(..2).sext(4).concat(y1.slice(..2).sext(4)).concat(y1.msb()));
        gold.apply();
        assert_isomorphic!(design, gold);
    }

    #[test]
    fn test_adc_unsext_only2() {
        for size in [1, 2] {
            let design = Design::new();
            let a = design.add_input("a", size);
            let b = design.add_input("b", size);
            let c = design.add_input("c", 1);
            let y = design.add_adc(a.sext(size + 1), b.sext(size + 1), c.unwrap_net());
            design.add_output("y", y);
            assert_no_simplify!(design);
        }
    }

    #[test]
    fn test_adc_unsext_only3() {
        for size in [1, 2] {
            let mut design = Design::new();
            let a = design.add_input("a", size);
            let b = design.add_input("b", size);
            let c = design.add_input("c", 1);
            let y = design.add_adc(a.sext(size + 2), b.sext(size + 2), c.unwrap_net());
            design.add_output("y", y);
            design.apply();
            simplify(&mut design);
            simplify(&mut design); // clean up zero length adcs

            let mut gold = Design::new();
            let a = gold.add_input("a", size);
            let b = gold.add_input("b", size);
            let c = gold.add_input("c", 1);
            let y = gold.add_adc(a.sext(size + 1), b.sext(size + 1), c.unwrap_net());
            gold.add_output("y", y.slice(..size + 1).sext(size + 2).concat(y.msb()));
            gold.apply();
            assert_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_adc_ci_folding() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| {
                    ds.add_adc(
                        ds.add_adc(ds.add_input("a", size), ds.add_input("b", size), Net::ZERO),
                        ds.add_input("c", 1).zext(size + 1),
                        Net::ZERO
                    )
                };
                [PZExt [PAdc [PInput ("a")] [PInput ("b")] [PInput ("c")]]] => true;
            );
        }
    }

    #[test]
    fn test_eq_const_eval() {
        for (value1, value2) in iter_interesting_const_pairs() {
            assert_simplify!(
                |ds| ds.add_eq(&value1, &value2);
                [PConst@a] => a == Const::from(value1.eq(&value2));
            );
        }
    }

    #[test]
    fn test_eq_fold() {
        assert_simplify!(
            |ds| ds.add_eq(Const::ones(1), ds.add_input("a", 1)),
            |ds| ds.add_eq(ds.add_input("a", 1), Const::ones(1));
            [PInput ("a")] => true;
        );
        assert_simplify!(
            |ds| ds.add_eq(Const::zero(1), ds.add_input("a", 1)),
            |ds| ds.add_eq(ds.add_input("a", 1), Const::zero(1));
            [PNot [PInput ("a")]] => true;
        );
        for size in [1, 2] {
            assert_simplify!(
                |ds| {let a = ds.add_input("a", size); ds.add_eq(&a, &a)};
                [POnes] => true;
            );
        }
    }

    #[test]
    fn test_ult_const_eval() {
        for (value1, value2) in iter_interesting_const_pairs() {
            assert_simplify!(
                |ds| ds.add_ult(&value1, &value2);
                [PConst@a] => a == Const::from(value1.ult(&value2));
            );
        }
    }

    #[test]
    fn test_ult_fold() {
        for size in [1, 2] {
            assert_simplify!(
                |ds| ds.add_ult(ds.add_input("a", size), Value::zero(size));
                [PZero] => true;
            );
            assert_simplify!(
                |ds| ds.add_ult(Value::ones(size), ds.add_input("a", size));
                [PZero] => true;
            );
            for a in iter_has_undef_consts().filter(|c| c.len() == size) {
                assert_simplify!(
                    |ds| ds.add_ult(&a, ds.add_input("a", size)),
                    |ds| ds.add_ult(ds.add_input("a", size), &a);
                    [PUndef] => true;
                );
            }
            assert_simplify!(
                |ds| {let a = ds.add_input("a", size); ds.add_ult(&a, &a)};
                [PZero] => true;
            );
        }
    }

    #[test]
    fn test_slt_const_eval() {
        for (value1, value2) in iter_interesting_const_pairs() {
            assert_simplify!(
                |ds| ds.add_slt(&value1, &value2);
                [PConst@a] => a == Const::from(value1.slt(&value2));
            );
        }
    }

    #[test]
    fn test_slt_fold() {
        for size in [1, 2] {
            for a in iter_has_undef_consts().filter(|c| c.len() == size) {
                assert_simplify!(
                    |ds| ds.add_slt(&a, ds.add_input("a", size)),
                    |ds| ds.add_slt(ds.add_input("a", size), &a);
                    [PUndef] => true;
                );
            }
            assert_simplify!(
                |ds| {let a = ds.add_input("a", size); ds.add_slt(&a, &a)};
                [PZero] => true;
            );
        }
    }

    #[test]
    fn test_shl_fold() {
        for size in [2, 4] {
            for size2 in [2, 4] {
                assert_simplify!(
                    |ds| ds.add_shl(Value::zero(size), ds.add_input("a", size2), 1);
                    [PZero] => true;
                );
                assert_simplify!(
                    |ds| ds.add_shl(ds.add_input("a", size), ds.add_input("b", size2), 0);
                    [PInput ("a")] => true;
                );
            }
        }
    }

    #[test]
    fn test_ff_simplify_clear() {
        for size in [1, 2] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", size), design.add_input1("c"))
                .with_clear(ControlNet::Pos(design.add_not(design.add_input("r", 1))[0]));
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(gold.add_input("d", size), gold.add_input1("c"))
                .with_clear(ControlNet::Neg(gold.add_input1("r")));
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
        for size in [1, 2] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", size), design.add_input1("c"))
                .with_clear(ControlNet::Neg(design.add_not(design.add_input("r", 1))[0]));
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(gold.add_input("d", size), gold.add_input1("c"))
                .with_clear(ControlNet::Pos(gold.add_input1("r")));
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_simplify_reset() {
        for size in [1, 2] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", size), design.add_input1("c"))
                .with_reset(ControlNet::Pos(design.add_not(design.add_input("r", 1))[0]));
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(gold.add_input("d", size), gold.add_input1("c"))
                .with_reset(ControlNet::Neg(gold.add_input1("r")));
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
        for size in [1, 2] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", size), design.add_input1("c"))
                .with_reset(ControlNet::Neg(design.add_not(design.add_input("r", 1))[0]));
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(gold.add_input("d", size), gold.add_input1("c"))
                .with_reset(ControlNet::Pos(gold.add_input1("r")));
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_simplify_enable() {
        for size in [1, 2] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", size), design.add_input1("c"))
                .with_enable(ControlNet::Pos(design.add_not(design.add_input("e", 1))[0]));
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(gold.add_input("d", size), gold.add_input1("c"))
                .with_enable(ControlNet::Neg(gold.add_input1("e")));
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
        for size in [1, 2] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", size), design.add_input1("c"))
                .with_enable(ControlNet::Neg(design.add_not(design.add_input("e", 1))[0]));
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(gold.add_input("d", size), gold.add_input1("c"))
                .with_enable(ControlNet::Pos(gold.add_input1("e")));
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_canonicalize_controls() {
        let design = Design::new();
        let flip_flop = FlipFlop::new(design.add_input("d", 1), ControlNet::Neg(Net::ONE))
            .with_clear(ControlNet::Neg(Net::ONE))
            .with_reset(ControlNet::Neg(Net::ONE))
            .with_enable(ControlNet::Neg(Net::ONE));
        design.add_output("q", design.add_dff(flip_flop));
        let gold = Design::new();
        let flip_flop = FlipFlop::new(Value::undef(1), Net::ZERO).with_enable(ControlNet::Pos(Net::ZERO));
        gold.add_output("q", gold.add_dff(flip_flop));
        assert_simplify_isomorphic!(design, gold);
    }

    #[test]
    fn test_ff_simplify_clock_const() {
        let design = Design::new();
        let flip_flop = FlipFlop::new(design.add_input("d", 1), Net::ZERO)
            .with_reset(design.add_input1("r"))
            .with_enable(design.add_input1("e"));
        design.add_output("q", design.add_dff(flip_flop));
        let gold = Design::new();
        let flip_flop = FlipFlop::new(Value::undef(1), Net::ZERO).with_enable(ControlNet::Pos(Net::ZERO));
        gold.add_output("q", gold.add_dff(flip_flop));
        assert_simplify_isomorphic!(design, gold);
    }

    #[test]
    fn test_ff_simplify_reset_always_enable_prio() {
        for init in &[Const::undef(1), Const::zero(1), Const::ones(1)] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", 1), design.add_input1("c"))
                .with_enable(design.add_input1("e"))
                .with_reset(Net::ONE)
                .with_init(init.clone());
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(init.into(), gold.add_input1("c"))
                .with_enable(gold.add_input1("e"))
                .with_init(init.clone());
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_simplify_reset_always_reset_prio() {
        for init in &[Const::undef(1), Const::zero(1), Const::ones(1)] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", 1), design.add_input1("c"))
                .with_reset(Net::ONE)
                .with_enable(design.add_input1("e"))
                .with_init(init.clone());
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop =
                FlipFlop::new(init.into(), gold.add_input1("c")).with_enable(Net::ONE).with_init(init.clone());
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_simplify_enable_never_enable_prio() {
        for init in &[Const::undef(1), Const::zero(1), Const::ones(1)] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", 1), design.add_input1("c"))
                .with_enable(Net::ZERO)
                .with_reset(design.add_input1("r"))
                .with_init(init.clone());
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(Value::undef(1), Net::ZERO)
                .with_reset(Net::ZERO)
                .with_enable(Net::ZERO)
                .with_init(init.clone());
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_simplify_enable_never_reset_prio() {
        for init in &[Const::undef(1), Const::zero(1), Const::ones(1)] {
            let design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", 1), design.add_input1("c"))
                .with_reset(design.add_input1("r"))
                .with_enable(Net::ZERO)
                .with_init(init.clone());
            design.add_output("q", design.add_dff(flip_flop));
            let gold = Design::new();
            let flip_flop = FlipFlop::new(init.into(), gold.add_input1("c"))
                .with_reset(Net::ZERO)
                .with_enable(gold.add_input1("r"))
                .with_init(init.clone());
            gold.add_output("q", gold.add_dff(flip_flop));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_simplify_always_clear() {
        for init in &[Const::undef(1), Const::zero(1), Const::ones(1)] {
            let mut design = Design::new();
            let flip_flop = FlipFlop::new(design.add_input("d", 1), design.add_input1("c"))
                .with_clear(Net::ONE)
                .with_init(init.clone());
            design.add_output("q", design.add_dff(flip_flop));
            design.apply();
            simplify(&mut design);
            assert_netlist!(design, netlist_match! { [PConst@c] => c == *init; });
        }
    }

    #[test]
    fn test_ff_reset_pos_matching() {
        for init in &[Const::undef(1), Const::zero(1), Const::ones(1)] {
            let design = Design::new();
            let d = design.add_mux(design.add_input1("r"), init, design.add_input("d", 1));
            design.add_output("q", design.add_dff(FlipFlop::new(d, design.add_input1("c"))));
            let gold = Design::new();
            let ff = FlipFlop::new(gold.add_input("d", 1), gold.add_input1("c"))
                .with_reset_value(ControlNet::Pos(gold.add_input1("r")), init.clone());
            gold.add_output("q", gold.add_dff(ff));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_reset_neg_matching() {
        for init in &[Const::undef(1), Const::zero(1), Const::ones(1)] {
            let design = Design::new();
            let d = design.add_mux(design.add_input1("r"), design.add_input("d", 1), init);
            design.add_output("q", design.add_dff(FlipFlop::new(d, design.add_input1("c"))));
            let gold = Design::new();
            let ff = FlipFlop::new(gold.add_input("d", 1), gold.add_input1("c"))
                .with_reset_value(ControlNet::Neg(gold.add_input1("r")), init.clone());
            gold.add_output("q", gold.add_dff(ff));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_ff_enable_pos_matching() {
        let design = Design::new();
        let q = design.add_void(1);
        let d = design.add_mux(design.add_input1("e"), design.add_input("d", 1), q.clone());
        design.replace_value(&q, design.add_dff(FlipFlop::new(d, design.add_input1("c"))));
        design.add_output("q", q);
        let gold = Design::new();
        let ff = FlipFlop::new(gold.add_input("d", 1), gold.add_input1("c"))
            .with_enable(ControlNet::Pos(gold.add_input1("e")));
        gold.add_output("q", gold.add_dff(ff));
        assert_simplify_isomorphic!(design, gold);
    }

    #[test]
    fn test_ff_enable_neg_matching() {
        let design = Design::new();
        let q = design.add_void(1);
        let d = design.add_mux(design.add_input1("e"), q.clone(), design.add_input("d", 1));
        design.replace_value(&q, design.add_dff(FlipFlop::new(d, design.add_input1("c"))));
        design.add_output("q", q);
        let gold = Design::new();
        let ff = FlipFlop::new(gold.add_input("d", 1), gold.add_input1("c"))
            .with_enable(ControlNet::Neg(gold.add_input1("e")));
        gold.add_output("q", gold.add_dff(ff));
        assert_simplify_isomorphic!(design, gold);
    }

    #[test]
    fn test_iobuf_simplify_enable() {
        for size in [1, 2] {
            let design = Design::new();
            let io_buffer = IoBuffer {
                io: design.add_io("io", size),
                output: design.add_input("o", size),
                enable: ControlNet::Pos(design.add_not(design.add_input("e", 1))[0]),
            };
            design.add_output("i", design.add_iobuf(io_buffer));
            let gold = Design::new();
            let io_buffer = IoBuffer {
                io: gold.add_io("io", size),
                output: gold.add_input("o", size),
                enable: ControlNet::Neg(gold.add_input1("e")),
            };
            gold.add_output("i", gold.add_iobuf(io_buffer));
            assert_simplify_isomorphic!(design, gold);
        }
        for size in [1, 2] {
            let design = Design::new();
            let io_buffer = IoBuffer {
                io: design.add_io("io", size),
                output: design.add_input("o", size),
                enable: ControlNet::Neg(design.add_not(design.add_input("e", 1))[0]),
            };
            design.add_output("i", design.add_iobuf(io_buffer));
            let gold = Design::new();
            let io_buffer = IoBuffer {
                io: gold.add_io("io", size),
                output: gold.add_input("o", size),
                enable: ControlNet::Pos(gold.add_input1("e")),
            };
            gold.add_output("i", gold.add_iobuf(io_buffer));
            assert_simplify_isomorphic!(design, gold);
        }
    }

    #[test]
    fn test_iobuf_canonicalize_controls() {
        let design = Design::new();
        let io_buffer = IoBuffer {
            io: design.add_io("io", 1),
            output: design.add_input("o", 1),
            enable: ControlNet::Neg(Net::ONE),
        };
        design.add_output("i", design.add_iobuf(io_buffer));
        let gold = Design::new();
        let io_buffer =
            IoBuffer { io: gold.add_io("io", 1), output: gold.add_input("o", 1), enable: ControlNet::Pos(Net::ZERO) };
        gold.add_output("i", gold.add_iobuf(io_buffer));
        assert_simplify_isomorphic!(design, gold);
    }

    #[cfg(not(feature = "verify"))]
    #[test]
    fn test_target_cell_simplify() {
        let target = MockTarget::new();
        let prototype = target.prototype("BUF1").unwrap();
        let design = Design::with_target(Some(target.clone()));
        let mut target_cell = TargetCell::new("BUF1", prototype);
        prototype.apply_input(&mut target_cell, "I", design.add_not(design.add_input("i", 1)));
        design.add_output("o", design.add_target(target_cell));
        let gold = Design::with_target(Some(target.clone()));
        let mut target_cell = TargetCell::new("BUF1", prototype);
        prototype.apply_input(&mut target_cell, "I", gold.add_input("i", 1));
        prototype.apply_param(&mut target_cell, "INVERT", Const::ones(1));
        gold.add_output("o", gold.add_target(target_cell));
        assert_simplify_isomorphic!(design, gold);
    }
}
