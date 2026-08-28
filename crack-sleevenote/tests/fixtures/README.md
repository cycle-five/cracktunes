# Wire-contract fixtures

These five files are **real HTTP responses captured from a running sleevenote
v0.1.0 deployment**. They are not hand-written, and they are not to be edited
to make a test pass.

## Provenance

Copied verbatim from `docs/examples/` in [cycle-five/sleevenote][repo] at
v0.1.0, where they are maintained as a contract artifact and checked against
that service's own `src/types.ts` by its `tests/examples.test.ts`. That repo's
README for them says, of this crate:

> They are a **contract artifact**: an out-of-repo Rust client deserializes
> these fixtures in its own CI, so a wire-format regression here fails a build
> over there before it reaches production.

This crate is that client. `tests/wire_contract.rs` is that CI check.

| File            | Response | Contents                                                                    |
|-----------------|----------|-----------------------------------------------------------------------------|
| `track.json`    | 200      | a `Track`                                                                    |
| `album.json`    | 200      | an `Album` (60 tracks, 0 unresolved; every nested track has `album: null`)   |
| `playlist.json` | 200      | a `Playlist` (2 tracks, 2 unresolved -- a podcast episode and a local file)  |
| `notfound.json` | 404      | the error shape, `error: "not_found"`                                        |
| `invalid.json`  | 400      | the error shape, `error: "invalid_id"`                                       |

## Rules

* **Do not edit these to fix a failing test.** A failure here means the wire
  shape and this crate's types disagree, and the answer is to change the types
  (or the service), never the evidence.
* **Update them only by re-copying from the service repo**, in the same change
  that reacts to the drift, and say which sleevenote version they came from.
* `playlist.json` deliberately contains a podcast episode whose `url` is
  `/episode/...` rather than `/track/...`. That is not a defect in the capture;
  it is the case a naive client gets wrong. Keep it.
* The files have no trailing newline, exactly as the service emitted them.
  `wire_contract.rs` asserts a byte-exact re-serialization round trip, so
  adding one will fail the suite.

[repo]: https://github.com/cycle-five/sleevenote
