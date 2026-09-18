# crack-bf

A Brainfuck interpreter, used by [cracktunes](https://github.com/cycle-five/cracktunes)'
`/bf` command.

## The command wrapper lives elsewhere

The poise command wrapper for `/bf` lives in cracktunes itself, at
`crack-core/src/commands/bf.rs`, and must never move here. A poise command
needs `crack_core::Context`, and crack-core depends on this crate — importing
the wrapper back into `crack-bf` would create a dependency cycle that cargo
forbids. This crate stays a plain interpreter library with no dependency on
crack-core or any Discord/poise types.

## Build and test

```bash
cargo build
cargo test
```

## Known limitation

This interpreter executes arbitrary user-submitted programs with **no step
limit, memory cap, or timeout**. It has no sandboxing of its own; callers that
expose it to untrusted input are responsible for bounding execution.
