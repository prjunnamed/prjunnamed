use std::collections::BTreeSet;

use prjunnamed_netlist::{Cell, Const, Design, FlipFlop, Net, Value};

pub fn split(design: &mut Design) -> bool {
    let mut live_nets = BTreeSet::<Net>::new();
    let mut queue = BTreeSet::<Net>::new();

    // Find roots.
    for cell_ref in design.iter_cells() {
        let cell = cell_ref.get();
        if cell.has_effects(design) {
            cell.visit(|net| {
                queue.insert(net);
            })
        }
    }

    // Mark all live nets.
    while let Some(net) = queue.pop_first() {
        if live_nets.contains(&net) {
            continue;
        }
        live_nets.insert(net);
        if let Ok((cell_ref, offset)) = design.find_cell(net) {
            match &*cell_ref.get() {
                Cell::Buf(arg) | Cell::Not(arg) => {
                    queue.insert(arg[offset]);
                }
                Cell::And(arg1, arg2) | Cell::Or(arg1, arg2) | Cell::Xor(arg1, arg2) => {
                    queue.insert(arg1[offset]);
                    queue.insert(arg2[offset]);
                }
                Cell::Mux(arg1, arg2, arg3) => {
                    queue.insert(*arg1);
                    queue.insert(arg2[offset]);
                    queue.insert(arg3[offset]);
                }
                Cell::Adc(arg1, arg2, arg3) => {
                    queue.insert(*arg3);
                    for index in 0..(offset + 1).min(arg1.len()) {
                        queue.insert(arg1[index]);
                        queue.insert(arg2[index]);
                    }
                }

                Cell::Shl(arg1, arg2, _) => {
                    for index in 0..(offset + 1) {
                        queue.insert(arg1[index]);
                    }
                    for net in arg2 {
                        queue.insert(net);
                    }
                }
                Cell::UShr(arg1, arg2, _) | Cell::SShr(arg1, arg2, _) | Cell::XShr(arg1, arg2, _) => {
                    for index in offset..(arg1.len()) {
                        queue.insert(arg1[index]);
                    }
                    for net in arg2 {
                        queue.insert(net);
                    }
                }

                Cell::Mul(arg1, arg2) => {
                    for index in 0..(offset + 1) {
                        queue.insert(arg1[index]);
                        queue.insert(arg2[index]);
                    }
                }

                Cell::Dff(ff) => {
                    queue.insert(ff.data[offset]);
                    queue.insert(ff.clock.net());
                    queue.insert(ff.reset.net());
                    queue.insert(ff.clear.net());
                    queue.insert(ff.enable.net());
                }

                Cell::Debug(..) => (),

                cell => cell.visit(|net| {
                    queue.insert(net);
                }),
            }
        }
    }

    // Split partially live cells.
    for cell_ref in design.iter_cells() {
        let _guard = design.use_metadata_from(&[cell_ref]);
        let cell = cell_ref.get();
        let cell_output = cell_ref.output();
        let count_live = cell_output.iter().filter(|net| live_nets.contains(&net)).count();
        if cell.has_effects(design) {
            continue; // root
        } else if count_live == cell_ref.output_len() {
            continue; // fully live
        } else if count_live == 0 {
            continue; // fully dead; removed by `design.compact()`
        }
        // might be partially live; candidate for splitting
        let out_low_dead_count = cell_output.iter().position(|net| live_nets.contains(&net)).unwrap();
        let out_high_dead_count = cell_output.iter().rev().position(|net| live_nets.contains(&net)).unwrap();
        let out_live_nets = Value::from_iter(cell_output.iter().filter(|net| live_nets.contains(net)));
        let arg_live_nets = |arg: &Value| {
            Value::from_iter(
                cell_output
                    .iter()
                    .enumerate()
                    .filter(|&(_offset, out_net)| live_nets.contains(&out_net))
                    .map(|(offset, _out_net)| arg[offset]),
            )
        };
        let arg_live_trits = |arg: &Const| {
            Const::from_iter(
                cell_output
                    .iter()
                    .enumerate()
                    .filter(|&(_offset, out_net)| live_nets.contains(&out_net))
                    .map(|(offset, _out_net)| arg[offset]),
            )
        };
        let (from_live_nets, to_live_nets) = match &*cell {
            Cell::Buf(arg) => (out_live_nets, design.add_buf(arg_live_nets(arg))),
            Cell::Not(arg) => (out_live_nets, design.add_not(arg_live_nets(arg))),
            Cell::And(arg1, arg2) => (out_live_nets, design.add_and(arg_live_nets(arg1), arg_live_nets(arg2))),
            Cell::Or(arg1, arg2) => (out_live_nets, design.add_or(arg_live_nets(arg1), arg_live_nets(arg2))),
            Cell::Xor(arg1, arg2) => (out_live_nets, design.add_xor(arg_live_nets(arg1), arg_live_nets(arg2))),
            Cell::Mux(arg1, arg2, arg3) => {
                (out_live_nets, design.add_mux(*arg1, arg_live_nets(arg2), arg_live_nets(arg3)))
            }
            Cell::Adc(arg1, arg2, arg3) if out_high_dead_count > 1 => {
                let new_width = arg1.len() - (out_high_dead_count - 1);
                (
                    cell_output.slice(..new_width),
                    design.add_adc(arg1.slice(..new_width), arg2.slice(..new_width), *arg3).slice(..new_width),
                )
            }
            Cell::Shl(arg1, arg2, stride) if out_high_dead_count > 0 => {
                let new_width = arg1.len() - out_high_dead_count;
                (cell_output.slice(..new_width), design.add_shl(arg1.slice(..new_width), arg2, *stride))
            }
            Cell::UShr(arg1, arg2, stride) if out_low_dead_count > 0 => (
                cell_output.slice(out_low_dead_count..),
                design.add_ushr(arg1.slice(out_low_dead_count..), arg2, *stride),
            ),
            Cell::SShr(arg1, arg2, stride) if out_low_dead_count > 0 => (
                cell_output.slice(out_low_dead_count..),
                design.add_sshr(arg1.slice(out_low_dead_count..), arg2, *stride),
            ),
            Cell::XShr(arg1, arg2, stride) if out_low_dead_count > 0 => (
                cell_output.slice(out_low_dead_count..),
                design.add_xshr(arg1.slice(out_low_dead_count..), arg2, *stride),
            ),
            Cell::Mul(arg1, arg2) if out_high_dead_count > 0 => {
                let new_width = arg1.len() - out_high_dead_count;
                (cell_output.slice(..new_width), design.add_mul(arg1.slice(..new_width), arg2.slice(..new_width)))
            }
            Cell::Dff(flip_flop) => (
                out_live_nets,
                design.add_dff(FlipFlop {
                    data: arg_live_nets(&flip_flop.data),
                    clock: flip_flop.clock,
                    clear: flip_flop.clear,
                    load: flip_flop.load,
                    reset: flip_flop.reset,
                    enable: flip_flop.enable,
                    reset_over_enable: flip_flop.reset_over_enable,
                    clear_value: arg_live_trits(&flip_flop.clear_value),
                    load_data: arg_live_nets(&flip_flop.load_data),
                    reset_value: arg_live_trits(&flip_flop.reset_value),
                    init_value: arg_live_trits(&flip_flop.init_value),
                }),
            ),
            _ => continue,
        };
        if cfg!(feature = "trace") {
            eprintln!(">split {} => {}", design.display_value(&from_live_nets), design.display_value(&to_live_nets));
        }
        design.replace_value(from_live_nets, to_live_nets);
    }

    design.compact()
}
