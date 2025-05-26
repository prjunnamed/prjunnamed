use std::{
    borrow::Cow,
    collections::{hash_map, HashMap},
};

use prjunnamed_netlist::{Cell, Const, ControlNet, Net, Trit, Value};

#[derive(Debug, Clone)]
enum SwizzleInput {
    Zero,
    One,
    NewInput(Vec<usize>),
}

fn swizzle(old_table: &Const, old_inputs: &[SwizzleInput], new_inputs_len: usize) -> Const {
    assert_eq!(old_table.len(), 1 << old_inputs.len());
    let mut result = Const::new();
    'bits: for index in 0..(1 << new_inputs_len) {
        let mut old_index = 0;
        for (old_bit_index, old_input) in old_inputs.iter().enumerate() {
            let input_value = match old_input {
                SwizzleInput::Zero => false,
                SwizzleInput::One => true,
                SwizzleInput::NewInput(new_inputs) => {
                    let input_value1 = ((index >> new_inputs[0]) & 1) != 0;
                    for &new_input in new_inputs {
                        assert!(new_input < new_inputs_len);
                        let input_value2 = ((index >> new_input) & 1) != 0;
                        if input_value2 != input_value1 {
                            // impossible combination, go eat an X
                            result.extend([Trit::Undef]);
                            continue 'bits;
                        }
                    }
                    input_value1
                }
            };
            if input_value {
                old_index |= 1 << old_bit_index;
            }
        }
        result.extend([old_table[old_index]]);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lut {
    inputs: Value,
    table: Const,
}

impl Lut {
    pub fn new(inputs: impl Into<Value>, table: impl Into<Const>) -> Lut {
        let (inputs, table) = (inputs.into(), table.into());
        assert_eq!(1 << inputs.len(), table.len());
        let mut lut = Lut { inputs, table };
        lut.simplify();
        lut
    }

    pub fn new_fixed(inputs: impl Into<Value>, table: impl Into<Const>) -> Lut {
        let (inputs, table) = (inputs.into(), table.into());
        assert_eq!(1 << inputs.len(), table.len());
        Lut { inputs, table }
    }

    pub fn lut1(in0: impl Into<Net>, table: impl Into<Const>) -> Lut {
        Self::new([in0.into()].as_ref(), table)
    }

    pub fn lut2(in0: impl Into<Net>, in1: impl Into<Net>, table: impl Into<Const>) -> Lut {
        Self::new([in0.into(), in1.into()].as_ref(), table)
    }

    pub fn lut3(in0: impl Into<Net>, in1: impl Into<Net>, in2: impl Into<Net>, table: impl Into<Const>) -> Lut {
        Self::new([in0.into(), in1.into(), in2.into()].as_ref(), table)
    }

    pub fn lut4(
        in0: impl Into<Net>,
        in1: impl Into<Net>,
        in2: impl Into<Net>,
        in3: impl Into<Net>,
        table: impl Into<Const>,
    ) -> Lut {
        Self::new([in0.into(), in1.into(), in2.into(), in3.into()].as_ref(), table)
    }

    pub fn inputs(&self) -> &Value {
        &self.inputs
    }

    pub fn table(&self) -> &Const {
        &self.table
    }

    pub fn from_cell<'a>(cell: impl Into<Cow<'a, Cell>>) -> Option<Lut> {
        let cell = cell.into();
        assert_eq!(cell.output_len(), 1);
        Some(match &*cell {
            Cell::Buf(arg) => Self::lut1(arg[0], Const::lit("10")),
            Cell::Not(arg) => Self::lut1(arg[0], Const::lit("01")),
            Cell::And(arg1, arg2) => Self::lut2(arg1[0], arg2[0], Const::lit("1000")),
            Cell::Or(arg1, arg2) => Self::lut2(arg1[0], arg2[0], Const::lit("1110")),
            Cell::Xor(arg1, arg2) => Self::lut2(arg1[0], arg2[0], Const::lit("0110")),
            Cell::Mux(arg1, arg2, arg3) => Self::lut3(arg3[0], arg2[0], *arg1, Const::lit("11001010")),
            Cell::Aig(arg1, arg2) => Self::lut2(
                arg1.net(),
                arg2.net(),
                Const::lit(match (arg1, arg2) {
                    (ControlNet::Neg(_), ControlNet::Neg(_)) => "0001",
                    (ControlNet::Neg(_), ControlNet::Pos(_)) => "0010",
                    (ControlNet::Pos(_), ControlNet::Neg(_)) => "0100",
                    (ControlNet::Pos(_), ControlNet::Pos(_)) => "1000",
                }),
            ),
            _ => return None,
        })
    }

    fn split(&self, input_index: usize) -> (Value, Const, Const) {
        let mut old_inputs = Vec::from_iter((0..self.inputs.len()).map(|index| {
            if index == input_index {
                SwizzleInput::Zero
            } else if index < input_index {
                SwizzleInput::NewInput(vec![index])
            } else {
                SwizzleInput::NewInput(vec![index - 1])
            }
        }));
        let table0 = swizzle(&self.table, &old_inputs, self.inputs.len() - 1);
        old_inputs[input_index] = SwizzleInput::One;
        let table1 = swizzle(&self.table, &old_inputs, self.inputs.len() - 1);
        let inputs = self.inputs.slice(..input_index).concat(self.inputs.slice(input_index + 1..));
        (inputs, table0, table1)
    }

    pub fn simplify(&mut self) {
        let mut old_inputs = vec![];
        let mut new_inputs = Value::new();
        let mut new_inputs_index = HashMap::new();
        for input in &self.inputs {
            if input == Net::ZERO {
                old_inputs.push(SwizzleInput::Zero);
            } else if input == Net::ONE {
                old_inputs.push(SwizzleInput::One);
            } else {
                match new_inputs_index.entry(input) {
                    hash_map::Entry::Occupied(entry) => {
                        old_inputs.push(SwizzleInput::NewInput(vec![*entry.get()]));
                    }
                    hash_map::Entry::Vacant(entry) => {
                        let index = new_inputs.len();
                        old_inputs.push(SwizzleInput::NewInput(vec![index]));
                        entry.insert(index);
                        new_inputs.extend([input]);
                    }
                }
            }
        }
        self.table = swizzle(&self.table, &old_inputs, new_inputs.len());
        self.inputs = new_inputs;
        for input_index in (0..self.inputs.len()).rev() {
            let (inputs, table0, table1) = self.split(input_index);
            let input = self.inputs[input_index];
            let table = if input == Net::UNDEF {
                (Trit::Undef).mux(table0, table1)
            } else if let Some(table) = Const::combine(table0, table1) {
                table
            } else {
                continue;
            };
            self.inputs = inputs;
            self.table = table;
        }
    }

    pub fn expand_with_fixed(&self, inputs: &[Option<Net>]) -> Option<Lut> {
        let mut inputs = inputs.to_vec();
        let mut input_to_index: HashMap<Net, Vec<usize>> = HashMap::new();
        for (index, &input) in inputs.iter().enumerate() {
            if let Some(input) = input {
                input_to_index.entry(input).or_default().push(index);
            }
        }
        let mut old_inputs = vec![];
        'inputs: for input in &self.inputs {
            if let Some(indices) = input_to_index.get(&input) {
                old_inputs.push(SwizzleInput::NewInput(indices.clone()));
                continue;
            }
            for (index, cur_input) in inputs.iter().enumerate() {
                if cur_input.is_none() {
                    inputs[index] = Some(input);
                    old_inputs.push(SwizzleInput::NewInput(vec![index]));
                    continue 'inputs;
                }
            }
            // out of space.
            return None;
        }
        let table = swizzle(&self.table, &old_inputs, inputs.len());
        Some(Lut { inputs: Value::from_iter(inputs.into_iter().map(|x| x.unwrap_or(Net::UNDEF))), table })
    }

    pub fn expand_to(&self, width: usize) -> Option<Lut> {
        self.expand_with_fixed(&vec![None; width])
    }

    pub fn merge(&self, input_index: usize, other: &Lut) -> Lut {
        let (mut inputs, table0, table1) = self.split(input_index);
        inputs.extend(&other.inputs);
        let tablex = (Trit::Undef).mux(&table0, &table1);
        let table = Const::from_iter(other.table.iter().flat_map(|bit| match bit {
            Trit::Undef => &tablex,
            Trit::Zero => &table0,
            Trit::One => &table1,
        }));
        Lut::new(inputs, table)
    }
}
