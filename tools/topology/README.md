# topology

Maps a Cargo workspace into facts (crates, modules, items, references) from
rust-analyzer's SCIP export plus `syn`. Standalone: its own `[workspace]` and
`Cargo.lock`, never part of the cracktunes root workspace.

Run through the wrapper from the repo root:

```shell
scripts/topology.sh extract
```

Config is `topology.toml` (every field optional). Output goes to `tools/topology/out/`.
