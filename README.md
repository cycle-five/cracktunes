![CrackTunes](./docs/logo.png)

  A hassle-free, highly performant, host-it-yourself, cracking smoking Discord music bot

[![builds.sr.ht status](https://builds.sr.ht/~cycle-five.svg)](https://builds.sr.ht/~cycle-five?)
[![GitHub CI workflow status](https://github.com/cycle-five/cracktunes/actions/workflows/ci_workflow.yml/badge.svg)](https://github.com/cycle-five/cracktunes/actions/workflows/ci_workflow.yml)
[![Dependency status](https://deps.rs/repo/github/cycle-five/cracktunes/status.svg)](https://deps.rs/repo/github/cycle-five/cracktunes)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/cycle-five/cracktunes/blob/main/LICENSE)
[![Rust Version](https://img.shields.io/badge/rustc-1.74-blue.svg)](https://github.com/cycle-five/cracktunes/)

## Aknowledgements

Thanks to the guys over at  [alwaysdata](https://www.alwaysdata.com/) for hosting the website, web portal, email, etc for this project for free, in their "Open Source" program.

## Deployment

### Usage

* Create a bot account
* Copy the **token** and **application id** to a `.env` with the `DISCORD_TOKEN` and `DISCORD_APP_ID` environment variables respectively.
* *Optional* define `SPOTIFY_CLIENT_ID` and `SPOTIFY_CLIENT_SECRET` for Spotify support.
* *Optional* define `OPENAI_API_KEY` for chatgpt support.
* Use [.env.example](https://github.com/cycle-five/cracktunes/blob/main/.env.example) as a starting point.

### Docker

```shell
docker run -d --env-file .env --restart unless-stopped --name cracktunes ghcr.io/cycle-five/cracktunes:latest
```

## Development

Make sure you've installed Rust. You can install Rust and its package manager, `cargo` by following the instructions on https://rustup.rs/.
After installing the requirements below, simply run `cargo run`.

### Linux/MacOS

The commands below install a C compiler, GNU autotools and FFmpeg, as well as [yt-dlp](https://github.com/yt-dlp/yt-dlp) through Python's package manager, pip.

#### Linux

```shell
apt install build-essential autoconf automake libtool ffmpeg
pip install -U yt-dlp
```

#### MacOS

```shell
brew install autoconf automake libtool ffmpeg
pip install -U yt-dlp
```

### Windows

If you are using the MSVC toolchain, a prebuilt DLL for Opus is already provided for you.  
You will only need to download [FFmpeg](https://ffmpeg.org/download.html), and install [yt-dlp](https://github.com/yt-dlp/yt-dlp) which can be done through Python's package manager, pip.

```shell
pip install -U yt-dlp
```

If you are using Windows Subsystem for Linux (WSL), you should follow the [Linux/MacOS](#linuxmacos) guide, and, in addition to the other required packages, install pkg-config, which you may do by running:

```shell
apt install pkg-config
```

## Testing

Tests are available inside the `src/tests` folder. They can be run via `cargo test`. It's recommended that you run the tests before submitting your Pull Request.
Increasing the test coverage is also welcome.

### Docker

Within the project folder, simply run the following:

```shell
docker build -t cracktunes .
docker run -d --env-file .env cracktunes
```

### Roadmap

## 0.2.0
- [x] Play music from YouTube
- [x] Play music from Spotify (kind of...)

## 0.2.1 (2023/07/02)
- [x] Play music from local files

## 0.2.2 (2023/07/09 ish)
- [x] Welcome Actions
- [x] Play on multiple servers at once

## 0.2.3
- [x] Bug fixes (volume)
- [x] Shuttle support (still broken)

## 0.2.4 (2023/07/17)
- [x] Bug fixes.
- [x] Remove reliance on slash commands everywhere.
- [x] Remove shuttle for now

## 0.2.5
- ~~[] Shuttle~~
- ~~[] Reminders~~
- ~~[] Notes~~

## ~~0.2.6~~
Didn't really track stuff here...
## ~~0.2.12~~
## 0.2.13
- [] Port to next branch of serenity
- [] Flesh out admin commands

## 0.3.0
- [] Database Schema

...and more!
## 1.0.0
- [] RTChris' Demuxer in C++ (for fun)?


<p align="center">
<sub><sup>Originally forked from <a href="https://github.com/aquelemiguel/parrot">Parrot</a></sup></sub>
<p>