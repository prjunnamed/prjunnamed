# Elaboration process

Elaboration is the process of converting source HDL to IR.
The unnamed core is agnostic about HDLs used, and the core elaboration
driver is concerned only with calling into actual language frontends,
passing module instantiation requests between them, and linking the
resulting IR fragments together.

The inputs of the elaboration driver are:

1. Target selection
2. Elaboration options:
   - "error on unknown module" flag: whether unknown modules should be considered to be an error, or result in unresolved instance cells
3. A bunch of frontends; there are two kinds of frontends:
   - built-in: is passed to the elaboration process as a dyn ref to an object implementing a `Frontend` trait and will communicate with elaboration through direct function calls; has direct access to the design being created (ie. creates its elaborated modules in-place)
   - remote: runs in an external process, communicates via RPC
4. For every frontend, frontend-specific input files and options
5. Top module selection, one of:
   - module-based selection: the name of the top module, optionally with parameters
   - frontend-based selection: one of the frontends is marked as providing the top module
   - automatic selection: all frontends are asked in turn to provide the top module

The output of the elaboration driver is a single design in the unnamed IR.

The elaboration driver communicates with frontends via a small set of requests and responses, acting essentially as an RPC router.  The communication is bidirectional and recursive: a frontend can reply to elaboration request with its own request and block on it before providing its own response.  In turn, the elaboration driver may call back into the frontend with another request while it's waiting on a response — this process directly follows the recursive nature of module instantiation.

The main difference from the yosys RPC protocol is the recursive nature of elaboration, where frontends can block their elaboration on submodule elaboration.  The motivation for this approach is allowing the frontend to make decisions based on elaborated port widths and other parts of the elaborated module's interface:

1. Verilog frontend needs to know the port widths and signedness in several cases:
   - to perform sign/zero extension or truncation in the event of width mismatch
   - when creating an instance array, to decide whether the connected value should be broadcast to all instances, or split across them
   - to properly expand an unsized constant (such as `'1`)
2. Implementing the SystemVerilog `.*` wildcard connection requires knowing the submodule's port names.
3. RTLIL frontend benefits from knowing the instantiated module's port directionality to generate less horrible IR without buses (since cell port directions are not specified in RTLIL)

## Target-provided frontend

If a known target is selected for elaboration, the driver will instantiate a special target-provided frontend and use it in the elaboration process in addition to user-specified frontends.  This frontend is expected to elaborate target-specific cells.  It will be queried last for every elaboration request, so that user-provided modules can shadow target cells.

## Passthru frontend

Including pre-elaborated IR in elaboration is done by means of the "passthru" frontend.  It can be instantiated multiple times, to include multiple pre-elaborated designs.  Each frontend instance takes the following parameters:

1. The input design IR
2. The "top" flag (if set, all top modules in the input will be considered to be top modules in the output)

The operation of this frontend is quite simple:

1. If the "top" flag is set:
   1. When the "elaborate top modules" request is processed, all modules of the input design will be emitted into the output, and the list of modules with the top flag will be returned
   2. The "list exported modules" will return an empty list
   3. The "elaborate specific module" will always return "module not available" response
2. If the "top" flag is not set:
   1. The "elaborate top modules" request will return an empty list
   2. The "list exported modules" request will return a list of names of all input modules that have the "top" flag set
   3. The "elaborate specific module" request will be handled as follows:
      1. All modules with "top" flag set and matching name will be considered for compatibility; if there are none, "module not available" response is returned
      2. A module is compatible if, for every parameter name in the request:
         1. There is a "baked-in parameter" annotation on the module with matching parameter name and value, or
         2. There is a parameter cell on the module with matching parameter name, and the value in the request (if any) is compatible with its type.
      3. If exactly one module is compatible, and all parameter cells have provided (or default) values, the module is inserted into the output IR (along with transitive dependencies) and returned.  Such modules are subject to unresolved instance processing.
      4. Otherwise, an invalid parameter error is returned.
3. Whenever an instance cell that refers to a blackbox is present in an input module that is to be emitted, an elaboration request is sent as if the instance cell were converted to an unresolved instance cell in-place and following the rules defined below.  If the request returns a module, the rules for unresolved instance cells are followed further to replace this cell with an instantiation of the returned module.  Otherwise, the referenced blackbox module is emitted into the input, and the original instance cell is emitted unchanged.

# General elaboration rules

The driver-frontend interface described here is designed to be used for cross-language elaboration only.  It is expected that frontends will not use it for elaborating instances that do not cross a language barrier — such instantiation should be elaborated locally.  This is important, as the elaboration protocol makes no effort to support the full semantics of Verilog / VHDL / etc. module instantiation, opting for a rather minimalistic set of supported types and features.  However, the driver will loop back elaboration requests to the requesting frontend as appropriate, and this fact can be used for frontends that have no complex instantiation needs.

Examples of (System)Verilog instantiation features that cannot be supported by this protocol, but can be supported by intra-frontend elaboration:

- hierarchical references across instantiation boundary
- parameter types other than string, int, real, bitvec
- ports of unpacked aggregate types
- interface ports

An exception to this rule is blackbox module: frontends are expected to always send out elaboration requests for modules for which they only have a blackbox definition, as another frontend may have a proper definition.  This request may end up bouncing back into the requesting frontend if there is no proper definition.

## Names and case sensitivity

Every name used in elaboration is actually a (string, case sensitive flag) pair.  When requesting a module or emitting a module, the frontend sets this flag according to its language's case sensitivity rules.

A module, port, or parameter name in the instantiation matches a corresponding name in the target module iff:

- both names are tagged as case sensitive and the names are exactly equal (including case), or
- either or both names are tagged as case insensitive and the names are equal ignoring case

It may be the case that a case insensitive name on one side matches multiple case sensitive names on the other side.  This is an error.

## Submodules

There are two ways for a frontend to request elaborating a submodule:

- emit an "unresolved instance" cell in its output IR
- send an elaboration request to the driver, receive elaborated submodule's interface in response, use it to directly emit an instance cell

The first way is easier to implement, but has its limitations:

- the instantiating frontend must know the correct width of all connected ports beforehand
- the instantiating frontend will not have feedback on which parameters have been immediately used up, and which are still variable
- the frontend doesn't get to emit its diagnostics in case of invalid parameter error

The "unresolved instance" cell effectively offloads the responsibility for matching up parameters and ports to the driver, which will error out in case of major mismatches.  The second way allows the frontend to make all connection decisions itself, based on the module interface.

### Unresolved cell handling

The driver will, if the frontend so requests, go through all unresolved instance cells in the elaborated modules and attempt to resolve them.  This involves the following steps:

1. An elaboration request is created based on the cell and sent to frontends as appropriate:
   - the module name is taken directly from the cell
   - parameter names/positions are taken directly from the cell; if parameter is connected directly to a "const" cell, its value is included in the request; otherwise, parameter value is marked as unavailable
   - connected port names/positions are taken directly from the cell
   - the top flag is not set
2. If the module is unknown (all frontends reply with "no such module"), the unresolved cell is left alone.  If "error on unknown module" is enabled, an error is emitted.
3. If invalid parameter error happens, a diagnostic is emitted pointing to the unresolved cell.
4. If submodule elaboration is not successful, stop now.
5. The cell is converted into a plain "instance" cell referencing the elaborated module.
6. Parameter inputs are connected to the cell for all submodule parameters as follows:
   - if a given parameter matches a parameter of the unresolved cell (by position or name):
     - if more than one cell parameter matches the module parameter, it is an error
     - if the value of the parameter is a const, the type-converted value from the response is used
     - otherwise, if the type of the parameter matches the type of value, the value is used
     - otherwise, it is a type mismatch error
   - otherwise, if the parameter has a default value, it is used as the parameter value
   - otherwise, it is an error
7. Extra parameters on the cell are ignored (they are assumed to have been baked into the submodule).
8. Submodule's ports are connected as follows:
   - if the submodule's input port matches a port on the unresolved cell (by position or name):
     - if more then one port matches, it is an error
     - if the width of the unresolved cell's port doesn't match the width of the submodule's port, it is an error
     - if the submodule's port is an input:
       - if the unresolved cell's port is input or bus, the value is passed as-is to the port
       - otherwise (unresolved cell's port is output), it is an error
     - if the submodule's port is a bus:
       - if the cell's port is an input:
         - create a bus in the instantiating submodule of appropriate width
         - create an always-enabled bus driver that drives the cell's input value onto it
         - connect the bus to the port
       - if the cell's port is a bus: use the value directly
       - if the cell's port is an output: replace the "instance output" cell with a "bus" cell and connect it to the port
     - if the submodule's port is an output:
       - if the cell's port is an input, it is an error
       - if the cell's port is a bus: create an "instance output" cell for this port, create a bus driver cell that drives the output onto the previously connected bus
       - if the cell's port is an output: reuse the already existing instance output cell
   - otherwise (submodule port is unconnected):
     - if the submodule's port is an input:
       - if it has a default value, connect it to a const driver with this value
       - otherwise, connect it to a const driver of an all-x value
     - if the submodule's port is a bus: connect it to a newly-created dummy bus cell
     - if the submodule's port is an output: create a dummy "instance out" cell for it
9.  If there's a cell port that doesn't correspond to exactly one submodule port, it is an error.

### Parameter handling

The elaboration process tries to avoid baking parameter values into the IR, aiming for elaborated module reuse where possible.  The process works roughly as follows (for a Verilog-like language frontend):

1. A request comes into the frontend, with as many parameters filled with actual values as possible.
2. When elaborating hierarchy, parameter values are operated on symbolically, until their value actually has to be materialized, eg. for:
   - wire/port/... width computation
   - generate-if condition, generate-for loop condition
   - control flow in constant function evaluation
   - ...
3. If materialization of a constant for one of the above purposes requires the value of a parameter, the parameter is marked as structurally used
4. When instantiating a submodule and a submodule parameter depends on parameter value in a way that can be represented in unnamed IR, the value is speculatively materialized:
   - the submodule parameter value is computed into a const, based on parameter values
   - the computed const value is passed to elaboration request
   - if the resulting submodule has the parameter as a cell (ie. it wasn't baked in by the submodule elaboration), and the parameter type matches the computed value (ie. no type conversion is necessary), the computed const value is discarded, and the symbolically computed value is passed into the submodule
   - otherwise, the const value is used, and all parameters involved are marked as structurally used
5. At the end, all structurally used parameters are replaced by constants in whatever symbolic computations remain, and are converted to "baked-in parameter" annotations on the module
6. All other parameters become parameter cells of the elaborated module, and the symbolic computations (if any) remain.

## Blackbox cell processing

# Request types

The elaboration driver can perform the following requests to a frontend:

1. Initialize frontend
2. Elaborate top modules
3. List exported modules
4. Elaborate specified module

The frontend can perform the following requests to the driver:

1. Insert specified IR (remote frontends only)
2. Mark modules for unresolved instance processing (builtin frontends only)
3. Elaborate specified module

## Initialize frontend

This request from the driver provides the frontend with general information about the elaboration process.

The parameters are:

- target information
- elaboration options (for now, the "error on unknown module" flag)

This request has no response.

## Elaborate top modules

This request is sent from the driver to the frontend if it is (one of) the frontends expected to provide the top module(s) of the design.

This request has no parameters.

In response to this request, the frontend should elaborate whatever
it considers to be its top-level module(s) into the design.  This may involve sending sub-module elaboration requests to the driver before sending the final response.

The response to this request has the following fields:

- a list of elaborated top modules (possibly empty); the modules are specified as module IDs within the driver's IR

## List exported modules

This request is sent from the driver to the frontend at the beginning of elaboration.  If the frontend can know a-priori the list of module names it is able to elaborate, it should respond with such a list.

The request has no parameters.  There are two responses to this request:

1. Success: a (possibly empty) list of exported module names is returned.
2. List unavailable: this frontend is not able to provide such a list up front, and the driver should query it for all otherwise unknown module names as needed.

## Elaborate specified module (to frontend)

This is the core request of the elaboration process.  When sent from the driver to the frontend, it has the following parameters:

- elaboration mode:
  - "top module" mode: used by the driver only if module-based top selection was used and this is the request for that top module; the frontend should set the "top" flag on the elaborated module iff this mode is set in the request
  - "proper module only" mode: frontend should only reply to this request with a module if it can provide a proper non-blackbox module; if it can only provide a blackbox, it should return the "requested name not provided" response instead
  - "any module"" mode
- the module name to elaborate
- the requested module parameters:
  - a list of positional parameters, with their values (or information that the given parameter value is not immediately available)
  - a list of named parameters, with their names and values (or information that the given parameter value is not immediately available)
- list of module ports connected

The frontend should elaborate the module with requested parameters, possibly sending recursive elaboration requests to the driver.  Once done, it should send one of the following responses:

1. Requested module name not provided by this frontend.
2. Invalid parameter error.  This will eventually abort the elaboration process (though it will continue as long as possible, to collect more diagnostics).  The frontend should emit its own diagnostics for this error, explaining the problem.  The frontend (or driver) orignally requesting this module instantiation should also emit a diagnostic at the point of use of this module.
3. Elaboration error.  Likewise, this will eventually abort the elaboration.  The frontend should emit its diagnostics for the error, unless the only cause of this error was an error in resursively-invoked elaboration.
4. Success.  The frontend should have created the IR for the module.  The returned information is:
   - the module ID of the elaborated module (within the driver)
   - possibly type-converted parameter values to be used when instantiating the module; a list of values is returned, one for every parameter cell present on the elaborated module:
     - if the parameter was provided in the request with a value, the value must be converted to parameter type according to target language rules and returned here; if the value provided by requestor already matches the parameter type, the value must be returned unchanged
     - if the parameter was provided in the request, but without a value, the value is assumed to be a variable; the value returned in this list is null (and the requestor must connect whatever value it has directly, or raise a type error)
     - if the parameter was not provided in the request, the default parameter value must be returned here

It is the frontend's responsibility to convert the incoming parameter values to its own internal types, as appropriate.

The incoming parameter values can be used up during elaboration (in which case the frontend should store them on the elaborated module as baked-in parameter annotations), or considered not structurally significant (in which case the frontend should create corresponding parameter cells, and the value in the request should not be used in this elaboration in any way).  In the second case, it will be the responsibility of the requesting frontend to notice the elaborated parameter cell and provide the (possibly type-converted) value again in instantiation.

It is the frontend's responsibility to remember previously-elaborated modules and reuse them as appropriate when the same (or similar enough) elaboration request is received more than once.  The driver may not have enough information to perform such deduplication, as it does not know how the parameter values will be normalized.

However, the driver (and other frontends) may also want to perform their own deduplication.  Specifically, they may make the following assumptions:

- it is safe to reuse an elaborated module when one would otherwise request elaboration with the same request parameters
- if the elaborated module contains a parameter cell for a given parameter (ie. the parameter isn't of the baked-in kind), the value of that parameter can be ignored when doing the above comparison

The frontend doesn't return any metadata about the module directly in the response — it should be extracted by the driver from the IR module instead.

## Elaborate specified module (from frontend)

The frontend can send this request to the driver whenever it is busy elaborating a module (ie. while processing an "elaborate top modules" or "elaborate specified module" request).  The parameters to the request are the same as for the version that is sent to a frontend, minus the mode.  The responses are:

1. Unknown module.  Returned when no frontend provides a module of this name.  The frontend may consider this to be an error (and emit diagnostics), or choose to emit an unresolved instance.
2. Invalid parameter error.  Passed through directly from the responding frontend.  The requesting frontend is expected to emit a diagnostic pointing to the location of the instantiation that caused this error.  The requesting frontend may return an elaboration error in turn, or provide a (possibly broken) IR module anyway so that more diagnostics can be generated.  The elaboration process will be eventually aborted either way.
3. Elaboration error.  The requesting frontend is not required to emit a diagnostic.  As above, it can ignore the error or pass it on — the elaboration process will be aborted either way.
4. Success.  The returned information includes:
   - the elaborated module's id in driver IR
   - (for remote protocol only) the elaborated module's interface, which is basically a serialized subset of its IR:
     - the parameter cells
     - the input/output/bus port cells (minus the source value for output ports)

Builtin frontends are expected to just use the returned module id to inspect the IR directly.

This request will be routed by the driver to frontends as follows:

1. The request will be sent to all frontends that have a name matching the requested module name in their exported module list, or haven't provided such a list.
2. In the first round, the request is sent with the "proper module only" mode.
3. If more than one frontend replies with a response other than "module not provided", it is an error.
4. If exactly one response other than "module not provided" is received, it is routed back to the requesting frontend.
5. If all responses were "module not provided", the same request is sent again to each frontend in turn, and the first response other than "module not provided" is passed to the requesting frontend.
6. If all responses were "module not provided" again, an "unknown module" response is passed to the requesting frontend.  If the "error on unknown module" option is in use, an error is emitted.

## Insert specified IR

This request is used by remote frontends to send their elaborated IR to the driver.  It has the following parameters:

1. A design in serialized unnamed IR format.  This design is expected to contain two kinds of modules:
   - blackbox modules corresponding to previously-elaborated modules already present in the driver (their contents are irrelevant, and they can be completely empty; the normal IR validity rules do not apply to instantiations of these modules — validity will be checked wrt actual modules present in the driver IR)
   - elaborated modules to be inserted into final IR; the frontend is allowed to emit multiple modules per elaboration request, elaborating a subtree of hierarchy on its own
2. A mapping of (request design's module id -> driver design's module id) pairs, identifying which modules in the above design are actually blackboxes and should be replaced with references to modules already present in the design.  This can be used by the frontend to both refer to foreign modules as returned by "elaborate specific module" request and its own previously-inserted modules.
3. A flag whether the inserted modules should be queued for unresolved instance processing.  If the frontend performs its own elaboration requests for submodules, this flag should be false.  If the frontend doesn't elaborate submodules and wishes to offload this responsibility onto the driver, this flag should be true.

The response to this request, in turn, contains a mapping of (request design's module id -> driver design's module id) pairs, so that the frontend knows the actual ids of the modules it inserted, and can use it in elaboration replies and further insertion requests.

## Mark modules for unresolved instance processing

Builtin frontends skip the abouve request and instead insert their modules directly into the IR.  If they do not perform their own submodule elaboration, they can instead use this much simpler request to tag the inserted modules for unresolved instance processing.

This request has a single parameter: the list of inserted module ids to be queued for processing.  There is no response to this request.