<p align="center">
  <img alt="Light" src="./docs/logo.png" width="50%">
</p>

<p align="center">
  A hassle-free, highly performant, host-it-yourself, cracking smoking Discord music bot
</p>

<p align="center">
  <a href="https://github.com/cycle-five/cracktunes/actions/workflows/ci_workflow.yml"><img src="https://github.com/cycle-five/cracktunes/actions/workflows/ci_workflow.yml/badge.svg"></a>
  <a href="https://deps.rs/repo/github/cycle-five/cracktunes"><img src="https://deps.rs/repo/github/cycle-five/cracktunes/status.svg"></a>
  <a href="https://github.com/cycle-five/cracktunes/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <a href="https://github.com/cycle-five/cracktunes/"><img src="https://img.shields.io/badge/rustc-1.65-blue.svg"></a>
</p>

## Deployment

### Usage

* Create a bot account
* Copy the **token** and **application id** to a `.env` with the `DISCORD_TOKEN` and `DISCORD_APP_ID` environment variables respectively.
* *Optional* define `SPOTIFY_CLIENT_ID` and `SPOTIFY_CLIENT_SECRET` for Spotify support.
* *Optional* define `OPENAI_KEY` for chatgpt support.
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
- [] Reminders
- [] Notes

## 0.2.6
- [] Flesh out admin commands

## 0.3.0
- [] Database Schema

...and more!
## 1.0.0
- [] RTChris' Demuxer


<p align="center">
<small>
Originally forked from <a href="https://github.com/aquelemiguel/parrot">Parrot</a>
</small>
<p>