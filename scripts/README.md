# scripts/

`cracktunes.sh` is the front door. Everything else here is either a development
convenience or a leftover, and three of them do not work.

## Canonical

### `cracktunes.sh`

Every routine deploy and inspect operation. `./scripts/cracktunes.sh help` lists the
subcommands; `preflight` runs every safety check and changes nothing, so it is always
safe to run first.

```shell
./scripts/cracktunes.sh preflight     # check without touching anything
./scripts/cracktunes.sh up            # bring the stack up
./scripts/cracktunes.sh deploy        # pull fresh images + recreate  ← ships a new version
./scripts/cracktunes.sh restart       # recreate only; ships NO new code
./scripts/cracktunes.sh logs cracktunes
./scripts/cracktunes.sh status
```

The `deploy` / `restart` distinction matters. The compose services pin floating `:dev`
tags and set no `pull_policy`, so a plain recreate happily keeps running the cached
image — a "deploy" that changes nothing while looking like it worked.

It refuses rather than warns on four things, each of which otherwise fails silently:
the wrong Docker context, a `POSTGRES_PASSWORD` that disagrees with the password
hardcoded in `docker-compose.yml`, missing external volumes, and a missing
`DISCORD_TOKEN`. The header comment in the script explains each one.

## Development utilities

| script | what it does |
|---|---|
| `lint_test_build.sh` / `.fish` | fmt + clippy + test + build, the local pre-push loop |
| `lint_test_build_crack_voting.sh` | same, scoped to crack-voting |
| `run_one_test.sh` | run a single test by name |
| `reset_db.sh` | drop, recreate and re-migrate the local database, then `cargo sqlx prepare`. Hardcodes the local dev password |
| `install_psql.sh` | install the postgres client |
| `test_curl.sh` | poke the crack-voting webhook endpoint by hand |
| `start.sh` | the container entrypoint — `COPY`d into the image by the Dockerfile, not run directly |

## Broken — do not use

Left in place rather than deleted so the intent behind them is not lost, but none of
these works as written. See #403.

### `build_and_deploy.sh`

Cannot deploy. Its last line is:

```sh
ssh kalevala -c '$HOME/run.sh'
```

`-c` selects an ssh *cipher*; running a remote command is `ssh host cmd`. It exits
immediately with `Unknown cipher type '$HOME/run.sh'`, so the zip is copied to the
host and nothing is ever started. Separately, `kalevala` is documented as being
decommissioned.

### `run.sh`

Starts nothing. It creates one tmux session and sends the start command to a
different one:

```sh
tmux new-session -s cracktunes -d
tmux send-keys -t grafana-agent "$HOME/cracktunes" Enter
```

It also runs `$HOME/cracktunes`, while the zip it unpacks puts the binary at
`target/release/cracktunes`.

### `sync.sh`

rsyncs from a hardcoded `/home/lothrop/dev/cracktunes`, which is not where this
repository lives. The remote destination `${HOME}` is expanded locally, so it only
lands in the right place when the local and remote usernames match. Its server list
predates the current homelab inventory.

### `refresh_service.sh`

Not broken, but superseded: it is `cracktunes.sh deploy crack-voting` without the
project-name pinning or any of the guards.
