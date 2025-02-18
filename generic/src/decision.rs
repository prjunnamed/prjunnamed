//! Decision tree lowering.
//!
//! The `decision` pass implements decision tree lowering based on the well-known heuristic
//! algorithm developed for ML from the paper "Tree pattern matching for ML" by Marianne Baudinet
//! and David MacQueen (unpublished, 1985) the extended abstract of which is available from:
//! *  https://smlfamily.github.io/history/Baudinet-DM-tree-pat-match-12-85.pdf (scan)
//! *  https://www.classes.cs.uchicago.edu/archive/2011/spring/22620-1/papers/macqueen-baudinet85.pdf (OCR)
//!
//! The algorithm is described in §4 "Decision trees and the dispatching problem". Only two
//! of the heuristics described apply here: the relevance heuristic and the branching factor
//! heuristic.

use std::fmt::Display;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use union_find_rs::{traits::UnionFind, disjoint_sets::DisjointSets};

use prjunnamed_netlist::{AssignCell, Cell, CellRef, Const, Design, MatchCell, Net, Trit, Value};

/// Maps `pattern` (a constant where 0 and 1 match the respective states, and X matches any state)
/// to a set of `rules` (the nets that are asserted if the `pattern` matches the value being tested).
#[derive(Debug, Clone, PartialEq, Eq)]
struct MatchRow {
    pattern: Const,
    rules: BTreeSet<Net>,
}

/// Matches `value` against ordered `rows` of patterns and their corresponding rules, where the `rules`
/// for the first pattern that matches the value being tested are asserted, and all other `rules`
/// are deasserted.
///
/// Invariant: once a matrix is constructed, there is always a case that matches.
/// Note that due to the possiblity of inputs being `X`, this is only
/// satisfiable with a catch-all row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MatchMatrix {
    value: Value,
    rows: Vec<MatchRow>,
}

/// Describes the process of testing individual nets of a value (equivalently, eliminating columns
/// from a match matrix) until a specific row is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Decision {
    /// Drive a set of nets to 1.
    Result { rules: BTreeSet<Net> },
    /// Branch on the value of a particular net.
    Branch { test: Net, if0: Box<Decision>, if1: Box<Decision> },
}

impl MatchRow {
    fn new(pattern: impl Into<Const>, rules: impl IntoIterator<Item = Net>) -> Self {
        Self { pattern: pattern.into(), rules: BTreeSet::from_iter(rules) }
    }

    fn empty(pattern_len: usize) -> Self {
        Self::new(Const::undef(pattern_len), [])
    }

    fn merge(mut self, other: &MatchRow) -> Self {
        self.pattern.extend(&other.pattern);
        self.rules.extend(other.rules.iter().cloned());
        self
    }
}

impl MatchMatrix {
    fn new(value: impl Into<Value>) -> Self {
        Self { value: value.into(), rows: Vec::new() }
    }

    fn add(&mut self, row: MatchRow) -> usize {
        assert_eq!(self.value.len(), row.pattern.len());
        self.rows.push(row);
        self.rows.len() - 1
    }

    fn add_enable(&mut self, enable: Net) {
        if enable != Net::ONE {
            for row in &mut self.rows {
                row.pattern.push(Trit::One);
            }
            self.rows.insert(0, MatchRow::new(Const::undef(self.value.len()).concat(Trit::Zero), []));
            self.value.push(enable);
        }
    }

    /// Merge in a `MatchMatrix` describing a child `match` cell whose `enable`
    /// input is being driven by `at`.
    fn merge(mut self, at: Net, other: &MatchMatrix) -> Self {
        self.value.extend(&other.value);
        for self_row in std::mem::take(&mut self.rows) {
            if self_row.rules.contains(&at) {
                for other_row in &other.rows {
                    self.add(self_row.clone().merge(&other_row));
                }
            } else {
                self.add(self_row.merge(&MatchRow::empty(other.value.len())));
            }
        }
        self
    }

    fn iter_outputs(&self) -> impl Iterator<Item = Net> {
        let mut outputs: Vec<Net> = self.rows.iter()
            .flat_map(|row| row.rules.iter().copied())
            .collect();
        outputs.sort();
        outputs.dedup();
        outputs.into_iter()
    }

    fn assume(mut self, target: Net, value: Trit) -> Self {
        self.value =
            Value::from_iter(self.value.into_iter().map(|net| if net == target { Net::from(value) } else { net }));
        self
    }

    /// Remove redundant rows and columns.
    ///
    /// This function ensures the following normal-form properties:
    /// - `self.value` does not contain any constant nets
    /// - all nets occurs within `self.value` at most once
    /// - a row of all `X` can only occur at the very end
    /// - no two identical rows can occur immediately next to each other
    ///
    /// Note that the last of these properties is relatively weak, in that
    /// stronger properties exist which can still be feasibly checked.
    fn normalize(mut self) -> Self {
        let mut remove_cols = BTreeSet::new();
        let mut remove_rows = BTreeSet::new();

        // Pick columns to remove where the matched value is constant or has repeated nets.

        // For each `Net`, store the index of the first column being driven
        // by that net.
        let mut first_at = BTreeMap::new();
        for (index, net) in self.value.iter().enumerate() {
            if net.is_cell() && !first_at.contains_key(&net) {
                first_at.insert(net, index);
            } else {
                remove_cols.insert(index);
            }
        }

        // Pick rows to remove that:
        // * are redundant with the immediately preceeding row, or
        // * contradict themselves or the constant nets in matched value.
        let mut prev_pattern = None;
        'outer: for (row_index, row) in self.rows.iter_mut().enumerate() {
            // Check if row will never match because of a previous row that:
            // * has the same pattern, or
            // * matches any value.
            if let Some(ref prev_pattern) = prev_pattern {
                if row.pattern == *prev_pattern || prev_pattern.is_undef() {
                    remove_rows.insert(row_index);
                    continue;
                }
            }
            prev_pattern = Some(row.pattern.clone());
            // Check if row is contradictory.
            for (net_index, net) in self.value.iter().enumerate() {
                let mask = row.pattern[net_index];
                // Row contradicts constant in matched value.
                //
                // Note that if we're matching against a constant `X`, removing
                // the row is nevertheless valid, since the row isn't guaranteed
                // to match regardless of the value of the `X`, and so we can
                // refine it into "doesn't match".
                match net.as_const() {
                    Some(trit) if trit != mask && mask != Trit::Undef => {
                        remove_rows.insert(row_index);
                        continue 'outer;
                    }
                    _ => (),
                }
                // Check if the net appears multiple times in the matched value.
                match first_at.get(&net) {
                    // It doesn't.
                    None => (),
                    // It does and this is the first occurrence. Leave it alone.
                    Some(&first_index) if first_index == net_index => (),
                    // It does and this is the second or later occurrence. Check if it is compatible with
                    // the first one. Also, if the first one was a don't-care, move this one into the position
                    // of the first one.
                    Some(&first_index) => {
                        let first_mask = &mut row.pattern[first_index];
                        if *first_mask != Trit::Undef && mask != Trit::Undef && *first_mask != mask {
                            remove_rows.insert(row_index);
                            continue 'outer;
                        }
                        if *first_mask == Trit::Undef {
                            *first_mask = mask;
                        }
                    }
                }
            }
        }

        // Pick columns to remove where all of the patterns match any value.
        let mut all_undef = vec![true; self.value.len()];
        for (row_index, row) in self.rows.iter().enumerate() {
            if remove_rows.contains(&row_index) {
                continue;
            }
            for col_index in 0..self.value.len() {
                if row.pattern[col_index] != Trit::Undef {
                    all_undef[col_index] = false;
                }
            }
        }
        for (col_index, matches_any) in all_undef.into_iter().enumerate() {
            if matches_any {
                remove_cols.insert(col_index);
            }
        }

        // Execute column and row removal.
        fn remove_indices<'a, T>(
            iter: impl IntoIterator<Item = T> + 'a,
            remove_set: &'a BTreeSet<usize>,
        ) -> impl Iterator<Item = T> + 'a {
            iter.into_iter().enumerate().filter_map(|(index, elem)| (!remove_set.contains(&index)).then_some(elem))
        }

        self.value = Value::from_iter(remove_indices(self.value, &remove_cols));
        self.rows = Vec::from_iter(remove_indices(self.rows, &remove_rows));
        for row in &mut self.rows {
            row.pattern = Const::from_iter(remove_indices(row.pattern.iter(), &remove_cols));
        }
        self
    }

    /// Construct a decision tree for the match matrix.
    fn dispatch(mut self) -> Decision {
        self = self.normalize();
        if self.value.is_empty() || self.rows.len() == 1 {
            Decision::Result { rules: self.rows.into_iter().next().map(|r| r.rules).unwrap_or_default() }
        } else {
            // Fanfiction of the heuristics from the 1986 paper that reduces them to: split the matrix on the column
            // with the fewest don't-care's in it.
            let mut undef_count = vec![0; self.value.len()];
            for row in self.rows.iter() {
                for (col_index, mask) in row.pattern.iter().enumerate() {
                    if mask == Trit::Undef {
                        undef_count[col_index] += 1;
                    }
                }
            }
            let test_index = (0..self.value.len()).min_by_key(|&i| undef_count[i]);
            let test = self.value[test_index.unwrap()];

            // Split the matrix into two, where the test net has a value of 0 and 1, and recurse.
            let if0 = self.clone().assume(test, Trit::Zero).dispatch();
            let if1 = self.assume(test, Trit::One).dispatch();
            if if0 == if1 {
                // Skip the branch if the outputs of the decision function are the same. This can happen
                // e.g. in the following matrix:
                //   00 => x
                //   10 => x
                //   XX => y
                // regardless of the column selection order. Even if the column 1 is chosen (as it should be),
                // there are two rows but the `y` rule is still unreachable due to irrefutability.
                if0
            } else {
                Decision::Branch { test, if0: if0.into(), if1: if1.into() }
            }
        }
    }
}

impl Decision {
    /// Union `Net`s if they can both be true at the same time.
    fn disjoint(&self, disjoint_sets: &mut DisjointSets<Net>) {
        match self {
            Decision::Result { rules } => {
                let mut unify_with = None;
                for &rule in rules {
                    let _ = disjoint_sets.make_set(rule);
                    if let Some(other_rule) = unify_with {
                        // work around https://gitlab.com/rustychoi/union_find/-/issues/1
                        if disjoint_sets.find_set(&rule).unwrap() != disjoint_sets.find_set(&other_rule).unwrap() {
                            disjoint_sets.union(&rule, &other_rule).unwrap();
                        }
                    } else {
                        unify_with = Some(rule);
                    }
                }
            }
            Decision::Branch { if0, if1, .. } => {
                if0.disjoint(disjoint_sets);
                if1.disjoint(disjoint_sets);
            }
        }
    }

    /// Emit a mux-tree that outputs `values[x]` when net `x` would be `1`
    /// according to the decision tree.
    ///
    /// Assumes that each case within `values` is mutually exclusive. Panics if
    /// that is not the case.
    fn emit_disjoint_mux(&self, design: &Design, values: &BTreeMap<Net, Value>, default: &Value) -> Value {
        match self {
            Decision::Result { rules } => {
                let mut result = None;
                for rule in rules {
                    if let Some(value) = values.get(&rule) {
                        assert!(result.is_none());
                        result = Some(value.clone());
                    }
                }
                result.unwrap_or(default.clone())
            }
            Decision::Branch { test, if0, if1 } => design.add_mux(
                *test,
                if1.emit_disjoint_mux(design, values, default),
                if0.emit_disjoint_mux(design, values, default),
            ),
        }
    }

    /// Emit a mux-tree that drives the `nets` according to the decision tree.
    fn emit_one_hot_mux(&self, design: &Design, nets: &Value) -> Value {
        match self {
            Decision::Result { rules } => Value::from_iter(
                nets.iter().map(|net| if rules.contains(&net) { Trit::One } else { Trit::Zero }.into()),
            ),
            Decision::Branch { test, if0, if1 } => {
                design.add_mux(*test, if1.emit_one_hot_mux(design, nets), if0.emit_one_hot_mux(design, nets))
            }
        }
    }
}

struct MatchTrees<'a> {
    design: &'a Design,
    /// Set of all `match` cells that aren't children. A `match` cell is a child
    /// of another `match` cell if its `enable` input is being driven from
    /// the output of the parent, and it is the unique such `match` cell for
    /// that particular output bit. That is, if it is possible to merge the
    /// child into the same decision tree.
    roots: BTreeSet<CellRef<'a>>,
    /// Maps a particular output of a `match` cell to the child `match` cell
    /// whose `enable` input it is driving.
    subtrees: BTreeMap<(CellRef<'a>, usize), CellRef<'a>>,
}

impl<'a> MatchTrees<'a> {
    /// Recognize a tree of `match` cells, connected by their enable inputs.
    fn build(design: &'a Design) -> MatchTrees<'a> {
        let mut roots: BTreeSet<CellRef> = BTreeSet::new();
        let mut subtrees: BTreeMap<(CellRef, usize), BTreeSet<CellRef>> = BTreeMap::new();
        for cell_ref in design.iter_cells() {
            let Cell::Match(MatchCell { enable, .. }) = &*cell_ref.get() else { continue };
            if let Ok((enable_cell_ref, offset)) = design.find_cell(*enable) {
                if let Cell::Match(_) = &*enable_cell_ref.get() {
                    // Driven by a match cell; may be a subtree or a root depending on its fanout.
                    subtrees.entry((enable_cell_ref, offset)).or_default().insert(cell_ref);
                    continue;
                }
            }
            // Driven by some other cell or a constant; is a root.
            roots.insert(cell_ref);
        }

        // Whenever multiple subtrees are connected to the same one-hot output, it is not possible
        // to merge all of them into the same matrix. Turn all of these subtrees into roots.
        let subtrees = subtrees.into_iter().filter_map(|(key, subtrees)| {
            if subtrees.len() == 1 {
                Some((key, subtrees.into_iter().next().unwrap()))
            } else {
                roots.extend(subtrees);
                None
            }
        }).collect();

        Self { design, roots, subtrees }
    }

    /// Convert a tree of `match` cells into a matrix.
    ///
    /// Collects a list of all the cells being lifted into the matrix into
    /// `all_cell_refs`.
    ///
    /// Replaces outputs that don't have any patterns at all with `Net::ZERO`,
    /// but otherwise doesn't modify the design.
    fn cell_into_matrix(&self, cell_ref: CellRef<'a>, all_cell_refs: &mut Vec<CellRef<'a>>) -> MatchMatrix {
        let Cell::Match(match_cell) = &*cell_ref.get() else { unreachable!() };
        let output = cell_ref.output();
        all_cell_refs.push(cell_ref);

        // Create matrix for this cell.
        let mut matrix = MatchMatrix::new(&match_cell.value);
        for (output_net, alternates) in output.iter().zip(match_cell.patterns.iter()) {
            for pattern in alternates {
                matrix.add(MatchRow::new(pattern.clone(), [output_net]));
            }
            if alternates.is_empty() {
                self.design.replace_net(output_net, Net::ZERO);
            }
        }
        matrix.add(MatchRow::empty(match_cell.value.len()));

        // Create matrices for subtrees and merge them into the matrix for this cell.
        for (offset, output_net) in output.iter().enumerate() {
            if let Some(&sub_cell_ref) = self.subtrees.get(&(cell_ref, offset)) {
                matrix = matrix.merge(output_net, &self.cell_into_matrix(sub_cell_ref, all_cell_refs));
            }
        }

        matrix
    }

    /// For each tree of `match` cells, return a corresponding `MatchMatrix`
    /// and a list of `match` cells that this matrix implements.
    fn iter_matrices<'b>(&'b self) -> impl Iterator<Item = (MatchMatrix, Vec<CellRef<'b>>)> + 'b {
        self.roots.iter().map(|&cell_ref| {
            let Cell::Match(MatchCell { enable, .. }) = &*cell_ref.get() else { unreachable!() };
            let mut all_cell_refs = Vec::new();
            let mut matrix = self.cell_into_matrix(cell_ref, &mut all_cell_refs);
            matrix.add_enable(*enable);
            (matrix, all_cell_refs)
        })
    }
}

struct AssignChains<'a> {
    chains: Vec<Vec<CellRef<'a>>>,
}

impl<'a> AssignChains<'a> {
    fn build(design: &'a Design) -> AssignChains<'a> {
        let mut roots: BTreeSet<CellRef> = BTreeSet::new();
        let mut links: BTreeMap<CellRef, BTreeSet<CellRef>> = BTreeMap::new();
        for cell_ref in design.iter_cells() {
            let Cell::Assign(AssignCell { value, offset: 0, update, .. }) = &*cell_ref.get() else { continue };
            if update.len() != value.len() { continue }
            if let Ok((value_cell_ref, _offset)) = design.find_cell(value[0]) {
                if value_cell_ref.output() == *value {
                    if let Cell::Assign(_) = &*value_cell_ref.get() {
                        links.entry(value_cell_ref).or_default().insert(cell_ref);
                        continue;
                    }
                }
            }
            roots.insert(cell_ref);
        }

        let mut chains = Vec::new();
        for root in roots {
            let mut chain = vec![root];
            while let Some(links) = links.get(&chain.last().unwrap()) {
                if links.len() == 1 {
                    chain.push(*links.first().unwrap());
                } else {
                    break;
                }
            }
            if chain.len() > 1 {
                chains.push(chain);
            }
        }

        Self { chains }
    }

    fn iter_disjoint<'b>(
        &'b self,
        decisions: &'a BTreeMap<Net, Rc<Decision>>,
        disjoint_sets: &'a DisjointSets<Net>,
    ) -> impl Iterator<Item = (Rc<Decision>, &'b [CellRef<'a>])> {
        fn enable_of(cell_ref: CellRef) -> Net {
            let Cell::Assign(AssignCell { enable, .. }) = &*cell_ref.get() else { unreachable!() };
            *enable
        }

        self.chains.iter().filter_map(|chain| {
            // Check if the enables belong to the same decision tree.
            let decision = decisions.get(&enable_of(chain[0]))?;
            let mut end_index = chain.len();
            'chain: for (index, &other_cell) in chain.iter().enumerate().skip(1) {
                let other_decision = decisions.get(&enable_of(other_cell))?;
                if !Rc::ptr_eq(decision, other_decision) {
                    end_index = index;
                    break 'chain
                }
            }
            let chain = &chain[..end_index];

            // Check if the enables are disjoint (like in a SystemVerilog "unique" or "unique0" statement).
            let enables = BTreeSet::from_iter(chain.iter().map(|&cell_ref| enable_of(cell_ref)));
            let unique_enables =
                BTreeSet::from_iter(enables.iter().filter_map(|enable| disjoint_sets.find_set(enable).ok()));
            if chain.len() != enables.len() || chain.len() != unique_enables.len() {
                return None;
            }

            Some((decision.clone(), chain))
        })
    }
}

pub fn decision(design: &mut Design) {
    // Detect and extract trees of `match` cells present in the netlist.
    let match_trees = MatchTrees::build(design);

    // Detect and extract chains of `assign` statements without slicing.
    let assign_chains = AssignChains::build(design);

    // Combine each tree of `match` cells into a single match matrix.
    // Then build a decision tree for it and use it to drive the output.
    let mut decisions: BTreeMap<Net, Rc<Decision>> = BTreeMap::new();

    // Nets are in the same set if they are driven by the same decision tree
    // and they can be driven at the same time.
    //
    // Note that this is currently conservative. For example, in the following
    // match matrix, %2 and %3 are not considered mutually exclusive:
    //   10 => %1 %2
    //   11 => %1 %3
    let mut disjoint_sets: DisjointSets<Net> = DisjointSets::new();
    for (matrix, matches) in match_trees.iter_matrices() {
        let all_outputs = BTreeSet::from_iter(matrix.iter_outputs());
        if cfg!(feature = "trace") {
            eprint!(">matrix:\n{matrix}");
        }

        let decision = Rc::new(matrix.dispatch());
        if cfg!(feature = "trace") {
            eprint!(">decision tree:\n{decision}")
        }

        decision.disjoint(&mut disjoint_sets);
        for &output in &all_outputs {
            decisions.insert(output, decision.clone());
        }

        let _guard = design.use_metadata_from(&matches[..]);
        let nets = Value::from_iter(all_outputs);
        design.replace_value(&nets, decision.emit_one_hot_mux(design, &nets));
    }

    // Find chains of `assign` cells that are order-independent.
    // Then lower these cells to a `mux` tree without `eq` cells.
    let mut used_assigns = BTreeSet::new();
    for (decision, chain) in assign_chains.iter_disjoint(&decisions, &disjoint_sets) {
        let (first_assign, last_assign) = (chain.first().unwrap(), chain.last().unwrap());
        if cfg!(feature = "trace") {
            eprintln!(">disjoint:");
            for &cell_ref in chain {
                eprintln!("{}", design.display_cell(cell_ref));
            }
        }

        let mut values = BTreeMap::new();
        let Cell::Assign(AssignCell { value: default, .. }) = &*first_assign.get() else { unreachable!() };
        for &cell_ref in chain {
            let Cell::Assign(AssignCell { enable, update, .. }) = &*cell_ref.get() else { unreachable!() };
            values.insert(*enable, update.clone());
        }

        let _guard = design.use_metadata_from(&chain[..]);
        design.replace_value(last_assign.output(), decision.emit_disjoint_mux(design, &values, default));
        used_assigns.insert(*last_assign);
    }

    // Lower other `assign` cells.
    for cell_ref in design.iter_cells().filter(|cell_ref| !used_assigns.contains(cell_ref)) {
        let Cell::Assign(assign_cell) = &*cell_ref.get() else { continue };
        if cfg!(feature = "trace") {
            eprintln!(">chained: {}", design.display_cell(cell_ref));
        }

        let _guard = design.use_metadata_from(&[cell_ref]);
        let mut nets = Vec::from_iter(assign_cell.value.iter());
        let slice = assign_cell.offset..(assign_cell.offset + assign_cell.update.len());
        nets[slice.clone()].copy_from_slice(
            &design.add_mux(assign_cell.enable, &assign_cell.update, assign_cell.value.slice(slice))[..],
        );
        design.replace_value(cell_ref.output(), Value::from(nets));
    }

    design.compact();
}

impl Display for MatchRow {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (index, trit) in self.pattern.iter().rev().enumerate() {
            if index != 0 && index % 4 == 0 {
                write!(f, "_")?;
            }
            write!(f, "{trit}")?;
        }
        write!(f, " =>")?;
        if self.rules.is_empty() {
            return write!(f, " (empty)");
        }
        for rule in &self.rules {
            write!(f, " {rule}")?;
        }
        Ok(())
    }
}

impl Display for MatchMatrix {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:\n", self.value)?;
        for row in &self.rows {
            write!(f, "  {row}\n")?;
        }
        Ok(())
    }
}

impl Decision {
    fn format(&self, f: &mut std::fmt::Formatter, level: usize) -> std::fmt::Result {
        let format_rules = |f: &mut std::fmt::Formatter, rules: &BTreeSet<Net>| {
            if rules.is_empty() {
                write!(f, " (empty)")
            } else {
                for rule in rules {
                    write!(f, " {rule}")?;
                }
                Ok(())
            }
        };

        let format_decision = |f: &mut std::fmt::Formatter, net: Net, value: usize, decision: &Decision| {
            if let Decision::Result { rules } = decision {
                if rules.is_empty() {
                    return Ok(());
                }
            }
            for _ in 0..level {
                write!(f, "  ")?;
            }
            match decision {
                Decision::Result { rules } => {
                    write!(f, "{net} = {value} =>")?;
                    format_rules(f, &rules)?;
                    write!(f, "\n")
                }
                Decision::Branch { .. } => {
                    write!(f, "{net} = {value} =>\n")?;
                    decision.format(f, level + 1)
                }
            }
        };

        match self {
            Decision::Result { rules } => {
                assert_eq!(level, 0);
                write!(f, "=>")?;
                format_rules(f, &rules)?;
                write!(f, "\n")?;
            }
            Decision::Branch { test, if0, if1 } => {
                format_decision(f, *test, 0, if0)?;
                format_decision(f, *test, 1, if1)?;
            }
        }
        Ok(())
    }
}

impl Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.format(f, 0)
    }
}

#[cfg(test)]
mod test {
    #![allow(non_snake_case)]

    use std::collections::{BTreeMap, BTreeSet};

    use prjunnamed_netlist::{assert_isomorphic, AssignCell, Cell, Const, Design, MatchCell, Net, Value};

    use super::{decision, AssignChains, Decision, MatchMatrix, MatchRow, MatchTrees};

    struct Helper(Design);

    impl Helper {
        fn new() -> Self {
            Self(Design::new())
        }

        fn val(&self, width: usize) -> Value {
            self.0.add_void(width)
        }

        fn net(&self) -> Net {
            self.0.add_void(1).unwrap_net()
        }

        fn rs(&self, rule: Net) -> Box<Decision> {
            Box::new(Decision::Result { rules: BTreeSet::from_iter([rule]) })
        }

        fn br(&self, test: Net, if1: Box<Decision>, if0: Box<Decision>) -> Box<Decision> {
            Box::new(Decision::Branch { test, if0, if1 })
        }
    }

    #[test]
    fn test_add_enable() {
        let h = Helper::new();

        let v = h.val(2);
        let (n1, n2, en) = (h.net(), h.net(), h.net());

        let mut ml = MatchMatrix::new(&v);
        ml.add(MatchRow::new(Const::lit("10"), [n1]));
        ml.add(MatchRow::new(Const::lit("01"), [n2]));

        let mut mr = MatchMatrix::new(&v.concat(en));
        mr.add(MatchRow::new(Const::lit("0XX"), []));
        mr.add(MatchRow::new(Const::lit("110"), [n1]));
        mr.add(MatchRow::new(Const::lit("101"), [n2]));

        ml.add_enable(en);
        assert_eq!(ml, mr, "\n{ml} != \n{mr}");
    }

    #[test]
    fn test_add_enable_trivial() {
        let h = Helper::new();

        let v = h.val(2);
        let (n1, n2) = (h.net(), h.net());

        let mut ml = MatchMatrix::new(&v);
        ml.add(MatchRow::new(Const::lit("10"), [n1]));
        ml.add(MatchRow::new(Const::lit("01"), [n2]));

        let mr = ml.clone();

        ml.add_enable(Net::ONE);
        assert_eq!(ml, mr, "\n{ml} != \n{mr}");
    }

    #[test]
    fn test_merge_1() {
        let h = Helper::new();

        let v1 = h.val(2);
        let (n11, n12) = (h.net(), h.net());
        let v2 = h.val(3);
        let (n21, n22) = (h.net(), h.net());
        let mut m1 = MatchMatrix::new(&v1);
        m1.add(MatchRow::new(Const::lit("10"), [n11]));
        m1.add(MatchRow::new(Const::lit("01"), [n12]));
        m1.add(MatchRow::new(Const::lit("XX"), []));

        let mut m2 = MatchMatrix::new(&v2);
        m2.add(MatchRow::new(Const::lit("X00"), [n21]));
        m2.add(MatchRow::new(Const::lit("10X"), [n22]));
        m2.add(MatchRow::new(Const::lit("XXX"), []));

        let ml1 = m1.clone().merge(n12, &m2);

        let mut mr1 = MatchMatrix::new(v1.concat(&v2));
        mr1.add(MatchRow::new(Const::lit("XXX10"), [n11]));
        mr1.add(MatchRow::new(Const::lit("X0001"), [n12, n21]));
        mr1.add(MatchRow::new(Const::lit("10X01"), [n12, n22]));
        mr1.add(MatchRow::new(Const::lit("XXX01"), [n12]));
        mr1.add(MatchRow::new(Const::lit("XXXXX"), []));

        assert_eq!(ml1, mr1, "\n{ml1} != \n{mr1}");
    }

    #[test]
    fn test_merge_2() {
        let h = Helper::new();

        let v1 = h.val(2);
        let (n11, n12) = (h.net(), h.net());
        let v2 = h.val(3);
        let (n21, n22) = (h.net(), h.net());
        let mut m1 = MatchMatrix::new(&v1);
        m1.add(MatchRow::new(Const::lit("10"), [n11]));
        m1.add(MatchRow::new(Const::lit("01"), [n11]));
        m1.add(MatchRow::new(Const::lit("XX"), [n12]));

        let mut m2 = MatchMatrix::new(&v2);
        m2.add(MatchRow::new(Const::lit("X00"), [n21]));
        m2.add(MatchRow::new(Const::lit("10X"), [n22]));
        m2.add(MatchRow::new(Const::lit("XXX"), []));

        let ml1 = m1.clone().merge(n11, &m2);

        let mut mr1 = MatchMatrix::new(v1.concat(&v2));
        mr1.add(MatchRow::new(Const::lit("X0010"), [n11, n21]));
        mr1.add(MatchRow::new(Const::lit("10X10"), [n11, n22]));
        mr1.add(MatchRow::new(Const::lit("XXX10"), [n11]));
        mr1.add(MatchRow::new(Const::lit("X0001"), [n11, n21]));
        mr1.add(MatchRow::new(Const::lit("10X01"), [n11, n22]));
        mr1.add(MatchRow::new(Const::lit("XXX01"), [n11]));
        mr1.add(MatchRow::new(Const::lit("XXXXX"), [n12]));

        assert_eq!(ml1, mr1, "\n{ml1} != \n{mr1}");
    }

    #[test]
    fn test_normalize_vertical() {
        let h = Helper::new();
        let n = h.net();

        let mut m00 = MatchMatrix::new(&Value::from(Const::lit("0")));
        m00.add(MatchRow::new(Const::lit("0"), [n]));

        let mut m01 = MatchMatrix::new(&Value::from(Const::lit("0")));
        m01.add(MatchRow::new(Const::lit("1"), [n]));

        let mut m0X = MatchMatrix::new(&Value::from(Const::lit("0")));
        m0X.add(MatchRow::new(Const::lit("X"), [n]));

        let mut m10 = MatchMatrix::new(&Value::from(Const::lit("1")));
        m10.add(MatchRow::new(Const::lit("0"), [n]));

        let mut m11 = MatchMatrix::new(&Value::from(Const::lit("1")));
        m11.add(MatchRow::new(Const::lit("1"), [n]));

        let mut m1X = MatchMatrix::new(&Value::from(Const::lit("1")));
        m1X.add(MatchRow::new(Const::lit("X"), [n]));

        let mut mX0 = MatchMatrix::new(&Value::from(Const::lit("X")));
        mX0.add(MatchRow::new(Const::lit("0"), [n]));

        let mut mX1 = MatchMatrix::new(&Value::from(Const::lit("X")));
        mX1.add(MatchRow::new(Const::lit("1"), [n]));

        let mut mXX = MatchMatrix::new(&Value::from(Const::lit("X")));
        mXX.add(MatchRow::new(Const::lit("X"), [n]));

        for must_reject in [m01, m10, mX0, mX1] {
            let normalized = must_reject.clone().normalize();
            assert_eq!(normalized.rows.len(), 0, "before:\n{must_reject}\nafter:\n{normalized}");
        }
        for must_accept in [m00, m0X, m11, m1X, mXX] {
            let normalized = must_accept.clone().normalize();
            assert_eq!(normalized.rows.len(), 1, "before:\n{must_accept}\nafter:\n{normalized}");
            assert_eq!(normalized.rows[0].pattern.len(), 0, "before:\n{must_accept}\nafter:\n{normalized}");
        }
    }

    #[test]
    fn test_normalize_horizontal() {
        let h = Helper::new();
        let v = h.val(1);
        let n = h.net();

        let mut m1 = MatchMatrix::new(v.concat(&v));
        m1.add(MatchRow::new(Const::lit("0X"), [n]));
        m1 = m1.normalize();
        assert_eq!(m1.rows[0].pattern, Const::lit("0"));

        let mut m2 = MatchMatrix::new(v.concat(&v));
        m2.add(MatchRow::new(Const::lit("X0"), [n]));
        m2 = m2.normalize();
        assert_eq!(m2.rows[0].pattern, Const::lit("0"));

        let mut m3 = MatchMatrix::new(v.concat(&v));
        m3.add(MatchRow::new(Const::lit("10"), [n]));
        m3 = m3.normalize();
        assert_eq!(m3.rows.len(), 0);
    }

    #[test]
    fn test_normalize_duplicate_row() {
        let h = Helper::new();
        let v = h.val(2);
        let (n1, n2) = (h.net(), h.net());

        let mut m = MatchMatrix::new(v);
        m.add(MatchRow::new(Const::lit("10"), [n1]));
        m.add(MatchRow::new(Const::lit("10"), [n2]));
        m = m.normalize();
        assert_eq!(m.rows.len(), 1);
        assert_eq!(m.rows[0].pattern, Const::lit("10"));
        assert_eq!(m.rows[0].rules, BTreeSet::from_iter([n1]));
    }

    #[test]
    fn test_normalize_irrefitable() {
        let h = Helper::new();
        let v = h.val(2);
        let (n1, n2) = (h.net(), h.net());

        let mut m = MatchMatrix::new(v);
        m.add(MatchRow::new(Const::lit("XX"), [n1]));
        m.add(MatchRow::new(Const::lit("10"), [n2]));
        m = m.normalize();
        assert_eq!(m.rows.len(), 1);
        assert_eq!(m.rows[0].pattern, Const::lit(""));
        assert_eq!(m.rows[0].rules, BTreeSet::from_iter([n1]));
    }

    #[test]
    fn test_normalize_unused_column() {
        let h = Helper::new();
        let v = h.val(2);
        let (n1, n2) = (h.net(), h.net());

        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("X0"), [n1]));
        m.add(MatchRow::new(Const::lit("X1"), [n2]));
        m = m.normalize();
        assert_eq!(m.value, v.slice(0..1));
        assert_eq!(m.rows.len(), 2);
        assert_eq!(m.rows[0], MatchRow::new(Const::lit("0"), [n1]));
        assert_eq!(m.rows[1], MatchRow::new(Const::lit("1"), [n2]));
    }

    #[test]
    fn test_normalize_unused_column_after_elim() {
        let h = Helper::new();
        let v = h.val(2);
        let (n1, n2, n3) = (h.net(), h.net(), h.net());

        let mut m = MatchMatrix::new(&v.concat(&v));
        m.add(MatchRow::new(Const::lit("XXX0"), [n1]));
        m.add(MatchRow::new(Const::lit("XXX1"), [n2]));
        m.add(MatchRow::new(Const::lit("1X0X"), [n3]));
        m = m.normalize();
        assert_eq!(m.value, v.slice(0..1));
        assert_eq!(m.rows.len(), 2);
        assert_eq!(m.rows[0], MatchRow::new(Const::lit("0"), [n1]));
        assert_eq!(m.rows[1], MatchRow::new(Const::lit("1"), [n2]));
    }

    macro_rules! assert_dispatch {
        ($m:expr, $d:expr) => {
            let dl = $m.clone().dispatch();
            let dr = $d;
            assert!(dl == *dr, "\ndispatching {}\n{} != \n{}", $m, dl, dr);
        };
    }

    #[test]
    fn test_decide_0() {
        let h = Helper::new();

        let v = h.val(1);
        let n = h.net();
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("0"), [n]));

        let d = h.rs(n);

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_decide_0_1() {
        let h = Helper::new();

        let v = h.val(1);
        let (n1, n2) = (h.net(), h.net());
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("0"), [n1]));
        m.add(MatchRow::new(Const::lit("1"), [n2]));

        let d = h.br(v[0], h.rs(n2), h.rs(n1));

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_decide_0_X() {
        let h = Helper::new();

        let v = h.val(1);
        let (n1, n2) = (h.net(), h.net());
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("0"), [n1]));
        m.add(MatchRow::new(Const::lit("X"), [n2]));

        let d = h.br(v[0], h.rs(n2), h.rs(n1));

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_decide_1_X() {
        let h = Helper::new();

        let v = h.val(1);
        let (n1, n2) = (h.net(), h.net());
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("1"), [n1]));
        m.add(MatchRow::new(Const::lit("X"), [n2]));

        let d = h.br(v[0], h.rs(n1), h.rs(n2));

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_decide_X_0_1() {
        let h = Helper::new();

        let v = h.val(1);
        let (n1, n2, n3) = (h.net(), h.net(), h.net());
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("X"), [n1]));
        m.add(MatchRow::new(Const::lit("0"), [n2]));
        m.add(MatchRow::new(Const::lit("1"), [n3]));

        let d = h.rs(n1);

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_decide_0_1_X() {
        let h = Helper::new();

        let v = h.val(1);
        let (n1, n2, n3) = (h.net(), h.net(), h.net());
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("0"), [n1]));
        m.add(MatchRow::new(Const::lit("1"), [n2]));
        m.add(MatchRow::new(Const::lit("X"), [n3]));

        let d = h.br(v[0], h.rs(n2), h.rs(n1));

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_decide_0X_1X_XX() {
        let h = Helper::new();

        let v = h.val(2);
        let (n1, n2, n3) = (h.net(), h.net(), h.net());
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("X0"), [n1]));
        m.add(MatchRow::new(Const::lit("X1"), [n2]));
        m.add(MatchRow::new(Const::lit("XX"), [n3]));

        let d = h.br(v[0], h.rs(n2), h.rs(n1));

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_decide_0X_11_XX() {
        let h = Helper::new();

        let v = h.val(2);
        let (n1, n2, n3) = (h.net(), h.net(), h.net());
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("0X"), [n1]));
        m.add(MatchRow::new(Const::lit("11"), [n2]));
        m.add(MatchRow::new(Const::lit("XX"), [n3]));

        let d = h.br(v[1], h.br(v[0], h.rs(n2), h.rs(n3)), h.rs(n1));

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_decide_00_10_XX() {
        let h = Helper::new();

        let v = h.val(2);
        let (n1, n2) = (h.net(), h.net());
        let mut m = MatchMatrix::new(&v);
        m.add(MatchRow::new(Const::lit("00"), [n1]));
        m.add(MatchRow::new(Const::lit("01"), [n1]));
        m.add(MatchRow::new(Const::lit("XX"), [n2]));

        let d = h.br(v[1], h.rs(n2), h.rs(n1));

        assert_dispatch!(m, d);
    }

    #[test]
    fn test_match_tree_build_root_1() {
        let mut design = Design::new();
        let a = design.add_input("a", 1);
        let y = design.add_match(MatchCell { value: a, enable: Net::ONE, patterns: vec![vec![Const::lit("0")]] });
        design.apply();

        let y_cell = design.find_cell(y[0]).unwrap().0;

        let match_trees = MatchTrees::build(&design);
        assert!(match_trees.roots == BTreeSet::from_iter([y_cell]));
        assert!(match_trees.subtrees == BTreeMap::from_iter([]));
    }

    #[test]
    fn test_match_tree_build_root_pi() {
        let mut design = Design::new();
        let a = design.add_input("a", 1);
        let b = design.add_input1("b");
        let y = design.add_match(MatchCell { value: a, enable: b, patterns: vec![vec![Const::lit("0")]] });
        design.apply();

        let y_cell = design.find_cell(y[0]).unwrap().0;

        let match_trees = MatchTrees::build(&design);
        assert!(match_trees.roots == BTreeSet::from_iter([y_cell]));
        assert!(match_trees.subtrees == BTreeMap::from_iter([]));
    }

    #[test]
    fn test_match_tree_build_root_subtree() {
        let mut design = Design::new();
        let a = design.add_input("a", 1);
        let y1 =
            design.add_match(MatchCell { value: a.clone(), enable: Net::ONE, patterns: vec![vec![Const::lit("0")]] });
        let y2 = design.add_match(MatchCell { value: a, enable: y1[0], patterns: vec![vec![Const::lit("0")]] });
        design.apply();

        let y1_cell = design.find_cell(y1[0]).unwrap().0;
        let y2_cell = design.find_cell(y2[0]).unwrap().0;

        let match_trees = MatchTrees::build(&design);
        assert!(match_trees.roots == BTreeSet::from_iter([y1_cell]));
        assert!(match_trees.subtrees == BTreeMap::from_iter([((y1_cell, 0), y2_cell)]));
    }

    #[test]
    fn test_match_tree_build_root_subtrees_disjoint() {
        let mut design = Design::new();
        let a = design.add_input("a", 1);
        let y1 = design.add_match(MatchCell {
            value: a.clone(),
            enable: Net::ONE,
            patterns: vec![vec![Const::lit("0")], vec![Const::lit("1")]],
        });
        let y2 = design.add_match(MatchCell { value: a.clone(), enable: y1[0], patterns: vec![vec![Const::lit("0")]] });
        let y3 = design.add_match(MatchCell { value: a, enable: y1[1], patterns: vec![vec![Const::lit("0")]] });
        design.apply();

        let y1_cell = design.find_cell(y1[0]).unwrap().0;
        let y2_cell = design.find_cell(y2[0]).unwrap().0;
        let y3_cell = design.find_cell(y3[0]).unwrap().0;

        let match_trees = MatchTrees::build(&design);
        assert!(match_trees.roots == BTreeSet::from_iter([y1_cell]));
        assert!(match_trees.subtrees == BTreeMap::from_iter([((y1_cell, 0), y2_cell), ((y1_cell, 1), y3_cell)]));
    }

    #[test]
    fn test_match_tree_build_root_subtrees_rerooted() {
        let mut design = Design::new();
        let a = design.add_input("a", 1);
        let y1 =
            design.add_match(MatchCell { value: a.clone(), enable: Net::ONE, patterns: vec![vec![Const::lit("0")]] });
        let y2 = design.add_match(MatchCell { value: a.clone(), enable: y1[0], patterns: vec![vec![Const::lit("0")]] });
        let y3 = design.add_match(MatchCell { value: a, enable: y1[0], patterns: vec![vec![Const::lit("1")]] });
        design.apply();

        let y1_cell = design.find_cell(y1[0]).unwrap().0;
        let y2_cell = design.find_cell(y2[0]).unwrap().0;
        let y3_cell = design.find_cell(y3[0]).unwrap().0;

        let match_trees = MatchTrees::build(&design);
        assert!(match_trees.roots == BTreeSet::from_iter([y1_cell, y2_cell, y3_cell]));
        assert!(match_trees.subtrees == BTreeMap::from_iter([]));
    }

    #[test]
    fn test_match_cell_into_matrix_flat() {
        let mut design = Design::new();
        let a = design.add_input("a", 3);
        let y = design.add_match(MatchCell {
            value: a.clone(),
            enable: Net::ONE,
            patterns: vec![vec![Const::lit("000"), Const::lit("111")], vec![Const::lit("010")]],
        });
        let yy = design.add_buf(&y);
        design.add_output("y", &yy);
        design.apply();

        let y_cell = design.find_cell(y[0]).unwrap().0;
        let mut match_cells = Vec::new();
        let m = MatchTrees::build(&design).cell_into_matrix(y_cell, &mut match_cells);
        assert_eq!(match_cells.len(), 1);
        assert_eq!(match_cells[0].output(), y);
        design.apply();

        let yy_cell = design.find_cell(yy[0]).unwrap().0;
        let Cell::Buf(y) = &*yy_cell.get() else { unreachable!() };
        assert_eq!(m.value, a);
        assert_eq!(m.rows, vec![
            MatchRow::new(Const::lit("000"), [y[0]]),
            MatchRow::new(Const::lit("111"), [y[0]]),
            MatchRow::new(Const::lit("010"), [y[1]]),
            MatchRow::new(Const::lit("XXX"), []),
        ]);
    }

    #[test]
    fn test_match_cell_into_matrix_nested() {
        let mut design = Design::new();
        let a = design.add_input("a", 3);
        let b = design.add_input("b", 2);
        let ya = design.add_match(MatchCell {
            value: a.clone(),
            enable: Net::ONE,
            patterns: vec![vec![Const::lit("000"), Const::lit("111")], vec![Const::lit("010")]],
        });
        let yb = design.add_match(MatchCell {
            value: b.clone(),
            enable: ya[1],
            patterns: vec![vec![Const::lit("00")], vec![Const::lit("11")]],
        });
        let yya = design.add_buf(&ya);
        let yyb = design.add_buf(&yb);
        design.add_output("ya", &yya);
        design.add_output("yb", &yyb);
        design.apply();

        let ya_cell = design.find_cell(ya[0]).unwrap().0;
        let mut match_cells = Vec::new();
        let ml = MatchTrees::build(&design).cell_into_matrix(ya_cell, &mut match_cells);
        assert_eq!(match_cells.len(), 2);
        assert_eq!(match_cells[0].output(), ya);
        assert_eq!(match_cells[1].output(), yb);
        design.apply();

        let ya_cell = design.find_cell(yya[0]).unwrap().0;
        let yb_cell = design.find_cell(yyb[0]).unwrap().0;

        let Cell::Buf(ya) = &*ya_cell.get() else { unreachable!() };
        let Cell::Buf(yb) = &*yb_cell.get() else { unreachable!() };
        let mut mr = MatchMatrix::new(a.concat(b));
        mr.add(MatchRow::new(Const::lit("XX000"), [ya[0]]));
        mr.add(MatchRow::new(Const::lit("XX111"), [ya[0]]));
        mr.add(MatchRow::new(Const::lit("00010"), [ya[1], yb[0]]));
        mr.add(MatchRow::new(Const::lit("11010"), [ya[1], yb[1]]));
        mr.add(MatchRow::new(Const::lit("XX010"), [ya[1]]));
        mr.add(MatchRow::new(Const::lit("XXXXX"), []));

        assert_eq!(ml, mr, "\n{ml} != \n{mr}");
    }

    fn assign(value: impl Into<Value>, enable: impl Into<Net>, update: impl Into<Value>) -> AssignCell {
        AssignCell { value: value.into(), enable: enable.into(), update: update.into(), offset: 0 }
    }

    #[test]
    fn test_assign_chains_build_1() {
        let mut design = Design::new();
        let x = design.add_input("x", 4);
        let _a1 = design.add_assign(assign(Value::zero(4), Net::ONE, x));
        design.apply();

        let AssignChains { chains } = AssignChains::build(&design);

        assert!(chains.is_empty());
    }

    #[test]
    fn test_assign_chains_build_2() {
        let mut design = Design::new();
        let x = design.add_input("x", 4);
        let a1 = design.add_assign(assign(Value::zero(4), Net::ONE, x));
        let y = design.add_input("y", 4);
        let a2 = design.add_assign(assign(a1.clone(), Net::ONE, y));
        design.apply();

        let a1_cell = design.find_cell(a1[0]).unwrap().0;
        let a2_cell = design.find_cell(a2[0]).unwrap().0;
        let AssignChains { chains } = AssignChains::build(&design);

        assert!(chains == &[vec![a1_cell, a2_cell]]);
    }

    #[test]
    fn test_assign_chains_build_3_fork() {
        let mut design = Design::new();
        let x = design.add_input("x", 4);
        let a1 = design.add_assign(assign(Value::zero(4), Net::ONE, x));
        let y = design.add_input("y", 4);
        let _a2 = design.add_assign(assign(a1.clone(), Net::ONE, y));
        let z = design.add_input("z", 4);
        let _a3 = design.add_assign(assign(a1.clone(), Net::ONE, z));
        design.apply();

        let AssignChains { chains } = AssignChains::build(&design);

        assert!(chains.is_empty());
    }

    #[test]
    fn test_assign_chains_build_partial_update() {
        let mut design = Design::new();
        let x = design.add_input("x", 4);
        let a1 = design.add_assign(assign(Value::zero(4), Net::ONE, x));
        let y = design.add_input("y", 3);
        let _a2 = design.add_assign(AssignCell { value: a1.clone(), enable: Net::ONE, update: y, offset: 1 });
        design.apply();

        let AssignChains { chains } = AssignChains::build(&design);

        assert!(chains.is_empty());
    }

    #[test]
    fn test_assign_chains_build_partial_value() {
        let mut design = Design::new();
        let x = design.add_input("x", 4);
        let a1 = design.add_assign(assign(Value::zero(4), Net::ONE, x));
        let y = design.add_input("y", 3);
        let _a2 = design.add_assign(assign(a1.slice(..3), Net::ONE, y));
        design.apply();

        let AssignChains { chains } = AssignChains::build(&design);

        assert!(chains.is_empty());
    }

    #[test]
    fn test_assign_lower_disjoint() {
        let mut dl = Design::new();
        let c = dl.add_input("c", 2);
        let m = dl.add_match(MatchCell {
            value: c.clone(),
            enable: Net::ONE,
            patterns: vec![
                vec![
                    Const::lit("00"), // x1
                    Const::lit("11"),
                ], // x1
                vec![Const::lit("01")], // x2
                vec![Const::lit("10")], // x3
            ],
        });
        let a1 = dl.add_assign(assign(Value::zero(4), m[0], dl.add_input("x1", 4)));
        let a2 = dl.add_assign(assign(a1, m[1], dl.add_input("x2", 4)));
        let a3 = dl.add_assign(assign(a2, m[2], dl.add_input("x3", 4)));
        dl.add_output("y", a3);
        dl.apply();

        decision(&mut dl);

        let mut dr = Design::new();
        let c = dr.add_input("c", 2);
        let x1 = dr.add_input("x1", 4);
        let x2 = dr.add_input("x2", 4);
        let x3 = dr.add_input("x3", 4);
        let m1 = dr.add_mux(c[1], &x3, &x1);
        let m2 = dr.add_mux(c[1], &x1, &x2);
        let m3 = dr.add_mux(c[0], m2, m1);
        dr.add_output("y", m3);

        assert_isomorphic!(dl, dr);
    }

    #[test]
    fn test_assign_lower_disjoint_partial() {
        let mut dl = Design::new();
        let c = dl.add_input("c", 2);
        let m = dl.add_match(MatchCell {
            value: c.clone(),
            enable: Net::ONE,
            patterns: vec![
                vec![
                    Const::lit("00"), // x1
                    Const::lit("11"),
                ], // x1
                vec![Const::lit("01")], // x2
                vec![Const::lit("10")], // x3
            ],
        });
        let a1 = dl.add_assign(assign(Value::zero(4), m[0], dl.add_input("x1", 4)));
        let a2 = dl.add_assign(assign(a1, m[1], dl.add_input("x2", 4)));
        let a3 = dl.add_assign(assign(a2, m[2], dl.add_input("x3", 3)));
        dl.add_output("y", a3);
        dl.apply();

        decision(&mut dl);
        // the particular output generated here is uninteresting, assert that
        // lowering doesn't panic and is accepted by SMT
    }

    #[test]
    fn test_assign_lower_overlapping() {
        let mut dl = Design::new();
        let c = dl.add_input("c", 1);
        let m = dl.add_match(MatchCell {
            value: c.clone(),
            enable: Net::ONE,
            patterns: vec![vec![Const::lit("0")], vec![Const::lit("1")]],
        });
        let a1 = dl.add_assign(assign(Value::zero(4), m[0], dl.add_input("x1", 4)));
        let a2 = dl.add_assign(assign(a1, m[1], dl.add_input("x2", 4)));
        let a3 = dl.add_assign(assign(a2, m[1], dl.add_input("x3", 4)));
        dl.add_output("y", a3);
        dl.apply();

        decision(&mut dl);

        let mut dr = Design::new();
        let c = dr.add_input1("c");
        let x1 = dr.add_input("x1", 4);
        let x2 = dr.add_input("x2", 4);
        let x3 = dr.add_input("x3", 4);
        let mc = dr.add_mux(c, Const::lit("10"), Const::lit("01"));
        let m1 = dr.add_mux(mc[0], x1, Value::zero(4));
        let m2 = dr.add_mux(mc[1], x2, m1);
        let m3 = dr.add_mux(mc[1], x3, m2);
        dr.add_output("y", m3);

        assert_isomorphic!(dl, dr);
    }

    #[test]
    fn test_assign_lower_different_matches() {
        let mut dl = Design::new();
        let c1 = dl.add_input("c1", 1);
        let c2 = dl.add_input("c2", 1);
        let m1 = dl.add_match(MatchCell { value: c1, enable: Net::ONE, patterns: vec![vec![Const::lit("0")]] });
        let m2 = dl.add_match(MatchCell { value: c2, enable: Net::ONE, patterns: vec![vec![Const::lit("0")]] });
        let a1 = dl.add_assign(assign(Value::zero(4), m1[0], dl.add_input("x1", 4)));
        let a2 = dl.add_assign(assign(a1, m2[0], dl.add_input("x2", 4)));
        dl.add_output("y", a2);
        dl.apply();

        decision(&mut dl);

        let mut dr = Design::new();
        let c1 = dr.add_input1("c1");
        let c2 = dr.add_input1("c2");
        let mc2 = dr.add_mux(c2, Const::lit("0"), Const::lit("1"));
        let m1 = dr.add_mux(c1, Value::zero(4), dr.add_input("x1", 4));
        let m2 = dr.add_mux(mc2[0], dr.add_input("x2", 4), m1);
        dr.add_output("y", m2);

        assert_isomorphic!(dl, dr);
    }

    #[test]
    fn test_assign_lower_partial() {
        let mut dl = Design::new();
        let en = dl.add_input1("en");
        let assign = dl.add_assign(AssignCell {
            value: dl.add_input("value", 6),
            enable: en,
            update: dl.add_input("update", 3),
            offset: 2,
        });
        dl.add_output("assign", assign);
        dl.apply();

        decision(&mut dl);

        let mut dr = Design::new();
        let en = dr.add_input1("en");
        let value = dr.add_input("value", 6);
        let update = dr.add_input("update", 3);
        let mux = dr.add_mux(en, update, value.slice(2..5));
        dr.add_output("assign", value.slice(..2).concat(mux.concat(value.slice(5..))));

        assert_isomorphic!(dl, dr);
    }

    #[test]
    fn test_match_eq_refinement() {
        let mut design = Design::new();
        let a = design.add_input("a", 2);
        let m = design.add_match(MatchCell {
            value: a,
            enable: Net::ONE,
            patterns: vec![vec![Const::lit("00")], vec![Const::lit("XX")]],
        });
        design.add_output("y", m);
        design.apply();

        decision(&mut design);
    }
}
