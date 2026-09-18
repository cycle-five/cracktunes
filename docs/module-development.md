# Developing against an extracted module

Some optional cargo features used to be workspace members (a path dependency
living in this repo). They are being pulled out one at a time into their own
public repositories, consumed as optional git dependencies pinned to a tag.
This keeps their build, CI and history independent of cracktunes while the
feature itself — the `--features <name>` flag anyone builds with — stays
exactly as it was.

## Extracted modules

| Feature      | Repository                                    | Pinned tag |
| ------------ | ---------------------------------------------- | ---------- |
| `crack-bf`   | `https://github.com/cycle-five/crack-bf`       | `v0.1.0`   |
| `crack-osint`| `https://github.com/cycle-five/crack-osint`    | `v0.1.0`   |

More modules will be added to this table as they are extracted. Each row's
pin is the `tag = "..."` value in that dependency's entry in
`crack-core/Cargo.toml`.

## A change spanning crack-core and a module is two commits

Because the module is a separate repository, a change that touches both sides
cannot land as one commit. It is:

1. A commit (or several) in the module's own repository, pushed and tagged
   with a new version (e.g. `v0.1.1`).
2. A commit in cracktunes that bumps the `tag = ` value in
   `crack-core/Cargo.toml` to that new tag, plus the resulting `Cargo.lock`
   update.

The module repo's CI and cracktunes' CI are independent gates; both must be
green, in that order, before the feature as a whole is considered done.

## Local development across both repos

Editing both repos at once without publishing a tag for every intermediate
change would otherwise mean either committing untagged code (which nothing
else can reliably depend on) or losing edit/rebuild flicker every time you
want to test a local change against crack-core.

Cargo's `[patch]` mechanism solves this, but it must not go in a *file* here:
`[patch]` in `Cargo.toml` is a tracked file, and so is `.cargo/config.toml` in
this repo (it carries the workspace's `rustflags` and wasm target settings) —
so writing the override into either one puts a local-only, machine-specific
path into version control, ready to be committed by accident and break every
other clone that doesn't have that sibling directory.

Instead, use a `--config` flag on the command line. It changes nothing on
disk, so there is nothing to accidentally commit and nothing to clean up
afterwards.

Clone the module beside the cracktunes checkout:

```bash
git clone https://github.com/cycle-five/crack-bf /home/lothrop/projects/crack-bf
git clone https://github.com/cycle-five/crack-osint /home/lothrop/projects/crack-osint
```

Then, from the cracktunes checkout, point cargo at the local clone for a
single invocation:

```bash
cargo check -p crack-core --features crack-bf \
  --config 'patch."https://github.com/cycle-five/crack-bf".crack-bf.path="/home/lothrop/projects/crack-bf"'
```

```bash
cargo check -p crack-core --features crack-osint \
  --config 'patch."https://github.com/cycle-five/crack-osint".crack-osint.path="/home/lothrop/projects/crack-osint"'
```

This was confirmed with `cargo tree`, which shows the dependency resolved
from the local path instead of the git URL:

```bash
cargo tree -p crack-core --features crack-bf -e normal \
  --config 'patch."https://github.com/cycle-five/crack-bf".crack-bf.path="/home/lothrop/projects/crack-bf"' \
  | grep crack-bf
# crack-bf v0.1.0 (/home/lothrop/projects/crack-bf)
```

Without the flag, the same `cargo tree` command shows the git source instead:

```bash
cargo tree -p crack-core --features crack-bf -e normal | grep crack-bf
# crack-bf v0.1.0 (https://github.com/cycle-five/crack-bf?tag=v0.1.0#f1bd11d5)
```

Edit both repos with the `--config` override in place, get things working,
then follow the release sequence above: commit and push in the module repo,
tag it, and only then bump the pin in `crack-core/Cargo.toml`. The override
never needs to be undone — it lives only in the command you typed, not in any
file — so the very next plain `cargo build` automatically goes back to
resolving the real, tagged git dependency.
