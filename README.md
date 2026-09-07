![CrackTunes](./docs/logo.png)

A hassle-free, highly performant, host-it-yourself, cracking smoking Discord music bot

[![builds.sr.ht status](https://builds.sr.ht/~cycle-five.svg)](https://builds.sr.ht/~cycle-five?)
[![GitHub CI workflow status](https://github.com/cycle-five/cracktunes/actions/workflows/ci_workflow.yml/badge.svg)](https://github.com/cycle-five/cracktunes/actions/workflows/ci_workflow.yml)
[![Dependency status](https://deps.rs/repo/github/cycle-five/cracktunes/status.svg)](https://deps.rs/repo/github/cycle-five/cracktunes)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/cycle-five/cracktunes/blob/main/LICENSE)
[![Rust Version](https://img.shields.io/badge/rustc-1.78-blue.svg)](https://github.com/cycle-five/cracktunes/)

## Aknowledgements

Thanks to the guys over at [alwaysdata](https://www.alwaysdata.com/) for hosting the website, web portal, email, etc for this project for free, in their [Open Source](https://www.alwaysdata.com/en/open-source/) program.

## Deployment

### Usage

- Create a bot account.
- Put its **token** in a `.env` as `DISCORD_TOKEN`. **That is the only required
  variable** — the bot starts and plays music with nothing else set.

Everything below is optional, and the bot degrades rather than failing without it:

| variable | what you lose without it |
| --- | --- |
| `DATABASE_URL` | play history, track reactions, playlist storage, and guild settings that survive a restart |
| `OPENAI_API_KEY` | ChatGPT commands |
| `VIRUSTOTAL_API_KEY` | OSINT URL checking |

Spotify needs no credentials. It is served by
[sleevenote](https://github.com/cycle-five/sleevenote) rather than the Spotify
Web API — which matters, because Spotify stopped accepting new Web API app
registrations around December 2025, so `SPOTIFY_CLIENT_ID` and
`SPOTIFY_CLIENT_SECRET` are not obtainable for a new self-hoster even if the old
path were still wanted.

`DATABASE_URL` is the only database variable the bot reads. If you run the
bundled Postgres from `docker-compose-postgres.yml`, `POSTGRES_USER` and
`POSTGRES_PASSWORD` configure *that container* — the official Postgres image
reads them, the bot does not.

Use [.env.example](https://github.com/cycle-five/cracktunes/blob/master/.env.example) as a starting point.

### Prebuilt binaries

No Docker, no Rust toolchain. Each release publishes a static
`x86_64-unknown-linux-gnu` build with an installer script and a `.sha256`
alongside it:

```shell
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/cycle-five/cracktunes/releases/latest/download/cracktunes-installer.sh | sh
```

Or take the tarball directly from the
[latest release](https://github.com/cycle-five/cracktunes/releases/latest) if you
would rather read the script first, or check the `.sha256` before running anything.
`crack-testing` and `crack-voting` ship the same way.

You still need a `.env` with `DISCORD_TOKEN`, and `yt-dlp` on `PATH` for playback.

### Docker

A single image, no database of your own to run:

```shell
docker run -d --env-file .env --restart unless-stopped --name cracktunes ghcr.io/cycle-five/cracktunes:latest
```

Images are published to two registries by CI: `ghcr.io/cycle-five/cracktunes` and
`docker.io/cyclefive/cracktunes`. Tagged releases get version tags; branch builds get
floating ones.

### Docker Compose (the full stack)

Use `scripts/cracktunes.sh` rather than driving `docker compose` directly — it pins
the project name and env file, and refuses on the failure modes that are otherwise
silent (wrong Docker context, database password divergence, missing external volumes,
missing `DISCORD_TOKEN`).

```shell
cp .env.example .env          # then fill it in
./scripts/cracktunes.sh preflight   # verify without changing anything
./scripts/cracktunes.sh up
```

To ship a new version, `deploy` (pull + recreate) rather than `restart` (recreate
only, picks up no new code):

```shell
./scripts/cracktunes.sh deploy
./scripts/cracktunes.sh logs cracktunes
```

`./scripts/cracktunes.sh help` lists everything. See [scripts/README.md](scripts/README.md)
for what the other scripts in that directory are, and which of them are broken.

## Development

Make sure you've installed Rust. You can install Rust and its package manager, `cargo` by following the instructions on https://rustup.rs/.
After installing the requirements below, simply run `cargo run`.

### Linux/MacOS **FIXME**

The commands below install a C compiler, GNU autotools and FFmpeg, as well as [yt-dlp](https://github.com/yt-dlp/yt-dlp) through Python's package manager, pip.

#### Linux **FIXME**

```shell
apt install build-essential autoconf automake libtool ffmpeg
pip install -U yt-dlp
```

#### MacOS **FIXME**

```shell
brew install autoconf automake libtool ffmpeg
pip install -U yt-dlp
```

### Windows **FIXME**

If you are using the MSVC toolchain, a prebuilt DLL for Opus is already provided for you.  
You will only need to download [FFmpeg](https://ffmpeg.org/download.html), and install [yt-dlp](https://github.com/yt-dlp/yt-dlp) which can be done through Python's package manager, pip.

```shell
pip install -U yt-dlp
```

If you are using Windows Subsystem for Linux (WSL), you should follow the [Linux/MacOS](#linuxmacos) guide, and, in addition to the other required packages, install pkg-config, which you may do by running:

```shell
apt install -y pkg-config
```

## Testing

The following command will run all tests:

```shell
cargo +nightly test --all-features --workspace
```

Some tests are available inside the `src/tests` folder, others are in their respective
files. It's recommended that you run the tests before submitting a Pull Request.
Increasing the test coverage is also welcome. Test coverage is tracked using
[tarpaulin]().

```shell
cargo +nightly tarpaulin --all-features --workspace
```

## Linting

```shell
cargo +nightly clippy --profile=release --all-features --workspace -- -D warnings -D clippy:all
```

## Build

```shell
cargo +nightly build --profile=release --features crack-osint,crack-bf,crack-fpt --workspace --locked
```

## Distribution

```shell
cargo dist init --hosting github
# make change `pr-run-mode = "upload"`
git add .
git commit -am "chore: cargo-dist"
cargo dist build --profile=release --features crack-gpt,crack-bf,crack-osint
```

## Release

```shell
git tag vX.X.X
git push --tags

# publish to crates.io (optional)
cargo publish
```

### Building the image locally

```shell
docker build -t cracktunes .
```

Note that `docker-compose.yml` references the published `cyclefive/cracktunes:dev`
image, not a locally built one — so building with the tag above does not change what
`docker compose up` runs. Retag it or edit the compose file if you want your local
build in the stack. Deploying is covered under [Deployment](#deployment) above.

<p align="center">
<sub><sup>Originally forked from <a href="https://github.com/aquelemiguel/parrot">Parrot</a></sup></sub>
<p>
