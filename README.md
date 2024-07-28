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

- Create a bot account
- Copy the **token** and **application id** to a `.env` with the `DISCORD_TOKEN` and `DISCORD_APP_ID` environment variables respectively.
- Define `DATABASE_URL`, `PG_USER`, `PG_PASSWORD` for the Postgres database.
- _Optional_ define `SPOTIFY_CLIENT_ID` and `SPOTIFY_CLIENT_SECRET` for Spotify support.
- _Optional_ define `OPENAI_API_KEY` for chatgpt support.
- _Optional_ define `VIRUSTOTAL_API_KEY` for osint URL checking.
- Use [.env.example](https://github.com/cycle-five/cracktunes/blob/master/.env.example) as a starting point.

### Docker **FIXME**

```shell
docker run -d --env-file .env --restart unless-stopped --name cracktunes ghcr.io/cycle-five/cracktunes:latest
```

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

### Docker Compose

Within the project folder, simply run the following:

```shell
docker build -t cracktunes .
docker compose up -d
```

<p align="center">
<sub><sup>Originally forked from <a href="https://github.com/aquelemiguel/parrot">Parrot</a></sup></sub>
<p>
