Building from source
--------------------

In order to build Project Unnamed and run its testsuite, you will need [Rust][] (latest stable release) and [Z3][]. We recommend using [rustup][] or installing Rust from the software repository of your Linux distribution. Once you have these tools, run:

```console
$ cargo test
```

We use [rustfmt][] to ensure consistent formatting of the entire codebase. Prior to sending a pull request, run:

```console
$ cargo fmt
```

In order to build the documentation, you will need [mdbook][] (which can be installed in a number of ways including via [rustup][]). Once you have it, run:

```console
$ mdbook serve docs
```

The documentation will be accessible in a browser at [http://localhost:3000](http://localhost:3000).

[rust]: https://rust-lang.org/
[rustfmt]: https://rust-lang.github.io/rustfmt/
[rustup]: https://rustup.rs/
[z3]: https://github.com/Z3Prover/z3
[mdbook]: https://rust-lang.github.io/mdBook/guide/installation.html
