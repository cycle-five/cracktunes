# crack-osint

OSINT (open-source intelligence) helpers for [cracktunes](https://github.com/cycle-five/cracktunes).

## What ships

- `check_password_pwned` — checks a password against the Have I Been Pwned
  range API, behind the `checkpass` feature.
- `VirusTotalClient` — submits and fetches VirusTotal scan results, behind
  the `virustotal` feature.
- `scan_url` and `get_scan_result` — thin wrappers around a URL scanning
  flow, behind the `scan` feature.

## The poise wrappers live in cracktunes

The poise command wrappers for these functions live in cracktunes itself, at
`crack-core/src/commands/osint.rs`, and must never move here. A poise command
needs `crack_core::Context`, and crack-core depends on this crate — importing
the wrapper back into `crack-osint` would create a dependency cycle that
cargo forbids. This is rule 2 of the extraction contract, and ignoring it is
what left `phcode.rs` and `phlookup.rs` uncompilable for years (see below).

## Known state: most of this crate is commented out

Roughly 474 lines across `ip.rs`, `ipv.rs`, `paywall.rs`, `phcode.rs`,
`phlookup.rs`, `socialmedia.rs`, `wayback.rs` and `whois.rs` (source plus
their test counterparts) are commented out of `lib.rs` and have not been
built in years. Triaging that backlog — deciding what still makes sense,
fixing what's salvageable, and deleting what isn't — is the first job in
this repository.

**`phlookup.rs` must not be revived as written.** See
[cracktunes#549](https://github.com/cycle-five/cracktunes/issues/549) for
why.

## Known defect: two of the three default features gate nothing

`default = ["checkpass", "virustotal", "scan"]`, but only `checkpass` is
actually conditional in `lib.rs` — `pub mod scan;` and `pub mod virustotal;`
are unconditional, so disabling either feature changes nothing. This is a
known defect carried over from cracktunes, recorded here rather than fixed:
fixing it is a behaviour change, and this repository's CI currently only
exercises one meaningful feature combination as a result. Once the gating is
real, a `--no-default-features` CI leg would be worth adding.

## Build and test

```bash
cargo build
cargo test
```
