use prjunnamed_netlist::{Design, Cell, Value, Net};

fn add_horiz_or(design: &Design, value: Value) -> Net {
    let mut nets = Vec::from_iter(value.iter());
    while nets.len() > 1 {
        for chunk in std::mem::take(&mut nets).chunks(2) {
            if chunk.len() == 2 {
                nets.push(design.add_or1(chunk[0], chunk[1]))
            } else {
                nets.push(chunk[0]);
            }
        }
    }
    *nets.first().unwrap_or(&Net::ZERO)
}

fn lower_shift(
    design: &Design,
    value: &Value,
    amount: &Value,
    stride: u32,
    shift: impl Fn(&Value, usize) -> Value,
    overflow: Value,
) -> Cell {
    let mut stride = stride as usize;
    let mut value = value.clone();
    for (index, bit) in amount.iter().enumerate() {
        if stride < value.len() {
            let shifted = shift(&value, stride);
            value = design.add_mux(bit, shifted, value);
            stride *= 2;
        } else {
            let did_overflow = add_horiz_or(design, amount.slice(index + 1..));
            value = design.add_mux(did_overflow, overflow, value);
            break;
        }
    }
    Cell::Buf(value)
}

pub fn lower_arith(design: &mut Design) {
    for cell_ref in design.iter_cells() {
        let _guard = design.with_metadata_from(&[cell_ref]);
        let new_cell = match &*cell_ref.get() {
            Cell::Eq(a, b) => {
                if a.is_empty() {
                    Cell::Buf(Value::ones(1))
                } else {
                    let xor = design.add_xor(a, b);
                    Cell::Not(add_horiz_or(design, xor).into())
                }
            }
            Cell::ULt(a, b) => {
                let b_inv = design.add_not(b);
                let sub = design.add_adc(a, b_inv, Net::ONE);
                Cell::Not(sub.msb().into())
            }
            Cell::SLt(a, b) => {
                let a_inv = a.slice(..a.len() - 1).concat(design.add_not(a.msb()));
                let b_inv = design.add_not(b.slice(..b.len() - 1)).concat(b.msb());
                let sub = design.add_adc(a_inv, b_inv, Net::ONE);
                Cell::Not(sub.msb().into())
            }
            Cell::Shl(a, b, stride) => {
                let shift = |value: &Value, amount| Value::zero(amount).concat(value.slice(..value.len() - amount));
                lower_shift(design, a, b, *stride, shift, Value::zero(a.len()))
            }
            Cell::UShr(a, b, stride) => {
                let shift = |value: &Value, amount| value.slice(amount..).zext(a.len());
                lower_shift(design, a, b, *stride, shift, Value::zero(a.len()))
            }
            Cell::SShr(a, b, stride) => {
                let shift = |value: &Value, amount| value.slice(amount..).sext(a.len());
                lower_shift(design, a, b, *stride, shift, Value::from(a.msb()).sext(a.len()))
            }
            Cell::XShr(a, b, stride) => {
                let shift = |value: &Value, amount| value.slice(amount..).concat(Value::undef(amount));
                lower_shift(design, a, b, *stride, shift, Value::undef(a.len()))
            }
            Cell::Mul(a, b) => {
                let mut value = Value::zero(a.len());
                for (index, bit) in b.iter().enumerate() {
                    value = design.add_adc(
                        value,
                        Value::zero(index).concat(design.add_mux(bit, a, Value::zero(a.len()))),
                        Net::ZERO,
                    );
                }
                Cell::Buf(value.slice(..a.len()))
            }
            Cell::UDiv(..)
            | Cell::UMod(..)
            | Cell::SDivTrunc(..)
            | Cell::SModTrunc(..)
            | Cell::SDivFloor(..)
            | Cell::SModFloor(..) => {
                todo!()
            }
            _ => continue,
        };
        if cfg!(feature = "trace") {
            eprintln!(">lower {}", design.display_cell(cell_ref));
        }
        cell_ref.replace(new_cell);
    }
    design.compact();
}
