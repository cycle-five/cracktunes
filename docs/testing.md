# Testing

```bash
cargo test --workspace          # everything that needs no database
```

That is the default, and it passes on a clean checkout with nothing running.

## Tests that need postgres

Roughly twenty tests exercise the database layer through `#[sqlx::test]`, which
creates a scratch database per test and runs the migrations into it. They are
marked `#[ignore]` unless the `db-tests` feature is on:

```rust
#[sqlx::test(migrator = "MIGRATOR")]
#[cfg_attr(
    not(feature = "db-tests"),
    ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
)]
async fn test_insert_user(pool: PgPool) { ... }
```

To run them, start a postgres and turn the feature on. The feature lives in
each package that has such tests, so a workspace run names all three:

```bash
docker run -d --name crack-pg -p 5432:5432 \
  -e POSTGRES_PASSWORD=mysecretpassword postgres:latest

cargo test --workspace \
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests
```

`DATABASE_URL` defaults to `postgresql://postgres:mysecretpassword@localhost:5432/postgres`,
set by a `#[ctor]` in the test modules; export your own to point elsewhere.

CI provisions postgres and passes those features, so the database tests do run
on every push — the gate is about the default local experience, not coverage.

## Why they are off by default

The bot runs without a database. `crack-core/src/config.rs` skips the pool
entirely when `DATABASE_URL` is unset, logs which features that disables, and
carries on; the deployed instance is running that way right now. A test suite
that hard-fails without postgres contradicts the thing it is testing, and in
practice it meant twenty red tests on every checkout that said nothing about
the change under review — noise that trains you to ignore a red suite.

`#[cfg_attr(..., ignore)]` rather than `#[cfg(...)]` on purpose: the tests are
still **compiled** either way, so they cannot rot against a changed query or
schema without someone noticing at build time.

## Tests that reach the network

Some tests in `crack-testing` and `crack-osint` make live calls (YouTube,
various OSINT endpoints). Those are marked `#[ignore]` on their own and are not
covered by `db-tests`. They fail for reasons unrelated to whatever you changed;
run them deliberately, by name.
