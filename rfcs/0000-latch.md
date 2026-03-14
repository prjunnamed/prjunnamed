- Feature Name: `latch`
- Start Date: 2026-03-01
- RFC PR: [prjunnamed/prjunnamed#0000](https://github.com/prjunnamed/prjunnamed/pull/0000)

# Latch support
[summary]: #summary

An "asynchronous load" capability is added to the flip-flop cell, allowing it to represent more types of register primitives, including latches.

# Motivation
[motivation]: #motivation

The usual modern recommendation for synchronous logic design is to solely use D flip-flop cells with a single constant-value synchronous or asynchronous reset, organized into well-defined domains.  The current `dff` cell type perfectly matches this paradigm.  However, there is also a need to represent more exotic cell types such as latches, flip-flops with more than one reset input, or flip-flops with non-constant reset input.

First, not all digital logic design follows this recommendation.  There are various reasons for doing so:

- following a design methodology involving multi-phase clocks
- special I/O interface requirements
- using latches for power-saving means by cutting off signals from inactive parts of the design
- utilizing legacy code that was not designed with modern methodologies at all

Second, even if the user design is pure synchronous logic, latches may appear as part of the lowering process:

- at a low level, a flip-flop is usually realized as a pair of latches; while most FPGAs and ASIC cell libraries directly provide a flip-flop as a primitive, there are (mostly obscure) targets where this lowering needs to be performed
- on some targets, FFs with an asynchronous reset and an initial value different from the reset value cannot be directly realized in hardware, and if used, must be lowered to an emulation circuit involving a latch
- inserting a latch on a path during P&R is one of the possible ways to fix a hold timing violation


# Guide-level explanation
[guide-level-explanation]: #guide-level-explanation

In addition to its current capabilities, a `dff` cell gets two new inputs:

- asynchronous load enable (a control net)
- asynchronous load data (a value of the same width as the cell output)

Both are specified by `load=` in the text IR format.  Furthermore, all existing inputs are made optional in the text IR (`clk=` defaulting to const 0, and the data input defaulting to `X`).

A simple latch would be represented as such:

```verilog
reg [3:0] q;
always @(*)
    if (en)
        q <= d;
```

```
%0:4 = <d>
%1:1 = <en>
%2:4 = dff load=%1,%0:4
```

`load=` can be combined with all other capabilities of the `dff` cell.  It can be used to describe a flip-flop with a dynamically variable asynchronous reset value:

```verilog
input [3:0] arval;
// NOTE: this Verilog does *NOT* correctly reflect the semantics of the flip-flop we want; such is the pain of describing hardware in a language woefully unsuited for it, including mandating a synthesis-simulation mismatch in the spec.
always @(posedge arst, posedge clk)
    if (arst)
        q <= arval;
    else
        q <= d;
```

```
%0:4 = <d>
%1:1 = <clk>
%2:4 = <arval>
%3:1 = <arst>
%4:4 = dff %0:4 clk=%1:1 load=%3,%2:4
```

The asynchronous load takes precedence over any synchronous operations, but the asynchronous reset takes precedence over asynchronous load.  A latch with a reset can be described as such:

```verilog
always @(*)
    if (rst)
        q <= 0;
    else if (en)
        q <= d;
```

```
%0:4 = <d>
%1:1 = rst
%2:1 = en
%3:4 = dff load=%2,%0:4 clr=%1,0000
```

As a special case, this can be used to describe the occasionally seen flip-flop with both asynchronous set and reset inputs:

```verilog
always @(posedge clr, posedge pre, posedge clk)
    if (clr)
        q <= 0;
    else if (pre)
        q <= 4'b1111;
    else
        q <= d;
```

```
%0:4 = <d>
%1:1 = clr
%2:1 = pre
%3:1 = clk
%3:4 = dff %0:4 clk=%3 load=%2,1111 clr=%1,0000
```

A target may place restrictions on the allowed configurations of a `dff` cell, to match the hardware capabilities (eg. ice40 cannot implement a latch).  The target documentation will specify the exact restrictions, and the flow will reject a netlist violating these restrictions with an error.  As an example:

> The `siliconblue` target only supports `dff` cells that don't use the `load` input (ie. have the `load` enable tied to a constant 0).

> The `virtex2` target only supports `dff` cells that meet at least one of the following conditions:
>
> 1. `clk` input is tied to a constant (a latch with reset)
> 2. `load` enable is tied to constant 0 (a flip-flop, possibly with async reset)
> 3. `load` data is tied to an all-1 constant, while `clr` value is an all-0 constant (a flip-flop with async reset and async set)


# Reference-level explanation
[reference-level-explanation]: #reference-level-explanation

The full specification of a flip-flop cell becomes:

```rust
    // defaults to X
    data: Value,
    // defaults to 0
    clock: ControlNet,
    // defaults to 0
    clear: ControlNet,
    // defaults to 0
    load: ControlNet,
    // defaults to X
    load_data: Value,
    // defaults to 0
    reset: ControlNet,
    // defaults to 1
    enable: ControlNet,
    reset_over_enable: bool,
    // defaults to X
    clear_value: Const,
    // defaults to X
    reset_value: Const,
    // defaults to X
    init_value: Const,
```

The behavior of the flip-flop is defined by the transfer function:

```rust
fn init(&self) -> Const { self.init_value }
fn next(&self) -> Const {
    if self.clear {
        self.clear_value
    } else if self.load {
        self.load_data
    } else if self.clock && !prev(self.clock) {
        if self.reset_over_enable && self.reset {
            self.reset_value
        } else if !self.enable {
            prev(self.q)
        } else if self.reset {
            self.reset_value
        } else {
            self.data
        }
    }
}
```


# Drawbacks
[drawbacks]: #drawbacks

Latches are often maligned, as Verilog makes it easy to accidentally introduce them to a design, breaking it in interesting ways.  However, we consider this to be a defect in Verilog, and mitigating it should be done by linting in the frontend (ensuring any use of latches is intentional), not by entirely erasing the concept of latches from existence.

Not all targets can meaningfully support latches (`siliconblue` being the obvious example).  Furthermore, the proposed addition makes it possible to describe even more complex flip-flop types that cannot be realized even on some targets that do support latches.  As such, we need a concept of a generic cell with possibly complex target-dependent validity rules.  However, this is not a unique concept: the same problems are involved in lowering memory cells (with much more complex rules).

All code operaitng on `dff` cells must remember to look at the new `load` input and skip processing if it cannot hanle such a cell.

The `dff` cell becomes somewhat misnamed.

There are *still* some flip-flops that the proposed addition cannot describe (`$dlatchsr`, which could be natively supported on eg. `virtex2`).


# Rationale and alternatives
[rationale-and-alternatives]: #rationale-and-alternatives

First, we must note there are a myriad variants of register types that we may or may not decide to represent.  As a point of reference, let's consider the following types:

1. A simple latch (`$dlatch` in yosys taxonomy)
2. A latch with async reset (`$adlatch`)
3. A latch with async reset and async set (`$dlatchsr`)
4. A latch with async reset and async set, but no data input (`$sr`)
5. A flip-flop with asynchronous load (`$aldff`)
6. A flip-flop with asynchronous load and asynchronous reset (no corresponding yosys cell)
7. A flip-flop with async reset and async set (`$dffsr`)
8. A flip-flop with asynchronous load, asynchronous reset, and asynchronous set (no corresponding yosys cell)

While the core premise of this RFC is that we want to support latches, it is not obvious just how many variants of them should be included in the IR.

The question of hardware support must be considered. The following register types can be natively realized on interesting targets:

1. `siliconblue`: no latches at all; only simple D flip-flop with synchronous or asynchronous reeset
2. `spartan6` / `virtex6` and up: can do latches with async reset (types 1, 2, 4 from the list above)
3. `virtex2`, `spartan3`, `virtex4`, `virtex5`: D flip-flops or D-latches with both async set and reset (types 1-4, 7 from the list above; 5,6,8 can be emulated)
4. `ecp5`: native type 5 support; types 1, 2, 4 supported as special cases

The core (perhaps surprising) choice made by this RFC is representing latches by adding a new input to the existing `dff` cell.  The obvious alternative would be to add a whole new `latch` cell instead, which would look something like this:

```rust
    // Equivalent to `load_data` in this RFC.
    data: Value,
    // Asynchronous reset.
    clear: ControlNet,
    // Equivalent to `load` in this RFC.
    enable: ControlNet,

    // Must have the same width as `data`.
    clear_value: Const,
    // Must have the same width as `data`.
    init_value: Const,
```

While such alternative would simplify some aspects of the design (one wouldn't have to worry about handling `dff` cells using the full set of inputs), it would also require some code duplication, and would only cover the first two of the motivating cases.

Based on experiences with the yosys flip-flop type explosion, we believe that we should keep using a single unified cell type.  This RFC takes a fairly minimal action of adding one new input path to the `dff` cell.  This addition makes the IR capable of describing cell types 1, 2, 4, 5, 6, 7 from the above list.

Notably missing is support for three distinct asynchronous inputs (set, reset, and load), which would be needed to describe cell types 3 (`$dlatchsr`) and 8.  This effectively makes a native `virtex2` cell (`$dlatchsr`) non-representable.  An alternative would be to add such third input, though it is unclear what its exact semantics would be (perhaps asynchronously setting the register to the *inverse* of the reset value?).  However, it is unclear how to fit the extra input into the IR, and the benefit from it is marginal (it is easily possible to emulate cell 3 via cell 2, and cell 8 via cell 6 or 7).

Another possibility would be to make the `dff` cell completely open-ended, having it contain an arbitrary *list* of asynchronous or synchronous triggers and their behavior.  This is likewise rejected as having little benefit while complicating the code a lot.

Technically, we currently have two FF cells: the `dff` cell, and the read port embedded in a `memory` cell.  This RFC proposes no addition to the `memory` read port, as there don't seem to be any interesting targets that would be able to make use of that.  This has a minor drawback of introducing a difference betwen it and the `dff` cell.

# Prior art
[prior-art]: #prior-art

yosys describes a great variety of FF types via a combinational explosion of cells.  It has a long history of new cell types being added ad-hoc for current needs, leaving existing ones in place.  As a result, there are currently 16 distinct families of FF cell types, and hundreds of individual cell types.  Directly dealing with this many cell types is intractable when writing IR transformations, so in practice all passes use an abstraction layer that translates all FF types to a unified representation strucutre, which is quite similar to our `dff` cell.

This proposal makes the `dff` cell capable of directly representing all yosys FF types except for `$dlatchsr` (as detailed above) and `$ff` (which is a cell specific to formal verification, and as such out of scope here).

# Unresolved questions
[unresolved-questions]: #unresolved-questions

Is there any need for more FF types?

# Future possibilities
[future-possibilities]: #future-possibilities

This RFC makes the major omission of not specifying an event sequencing model describing how a design with several `dff` cells behaves eg. when one `dff` cell drives an asynchronous control input of another.  Having such a model is crucial for being able to reason about transformations and formally validate them.  However, this is not a *new* problem: while latches are the obvious tool to create asynchronous clock-driving contraptions, it is already possible to create strange circuits with just normal flip-flops with async resets.  Furthermore, defining such a model would have a huge scope.  As such, defining a proper event sequence model is left for a future RFC.

Some kind of whitebox model description will be needed for target cells for simulation and verification purposes.  Since target cells may have more complex behavior than the `dff` cell extended by this RFC, a new mechanism may be needed to describe them.

We may want to rename the `dff` cell to eg. `register`, as it is now capable of describing storage elements that are not technically FFs.
