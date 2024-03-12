![CrackTunes](./docs/logo.png)

  A hassle-free, highly performant, host-it-yourself, cracking smoking Discord music bot

[![builds.sr.ht status](https://builds.sr.ht/~cycle-five.svg)](https://builds.sr.ht/~cycle-five?)
[![GitHub CI workflow status](https://github.com/cycle-five/cracktunes/actions/workflows/ci_workflow.yml/badge.svg)](https://github.com/cycle-five/cracktunes/actions/workflows/ci_workflow.yml)
[![Dependency status](https://deps.rs/repo/github/cycle-five/cracktunes/status.svg)](https://deps.rs/repo/github/cycle-five/cracktunes)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/cycle-five/cracktunes/blob/main/LICENSE)
[![Rust Version](https://img.shields.io/badge/rustc-1.76-blue.svg)](https://github.com/cycle-five/cracktunes/)
[![Rust Version](https://img.shields.io/badge/rustc-1.78-blue.svg)](https://github.com/cycle-five/cracktunes/)

## Aknowledgements

Thanks to the guys over at  [alwaysdata](https://www.alwaysdata.com/) for hosting the website, web portal, email, etc for this project for free, in their [Open Source](https://www.alwaysdata.com/en/open-source/) program.

## Deployment

### Usage

* Create a bot account
* Copy the **token** and **application id** to a `.env` with the `DISCORD_TOKEN` and `DISCORD_APP_ID` environment variables respectively.
* Define `DATABASE_URL`, `PG_USER`, `PG_PASSWORD` for the Postgres database.
* *Optional* define `SPOTIFY_CLIENT_ID` and `SPOTIFY_CLIENT_SECRET` for Spotify support.
* *Optional* define `OPENAI_API_KEY` for chatgpt support.
* Use [.env.example](https://github.com/cycle-five/cracktunes/blob/main/.env.example) as a starting point.

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
apt install pkg-config
```

## Testing **FIXME**

Tests are available inside the `src/tests` folder. They can be run via `cargo test`. It's recommended that you run the tests before submitting your Pull Request.
Increasing the test coverage is also welcome.

### Docker **FIXME**

Within the project folder, simply run the following:

```shell
docker build -t cracktunes .
docker run -d --env-file .env cracktunes
```

### ~~Roadmap~~ Change Log

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
## ~~0.2.13~~
- ~~[] Port to next branch of serenity~~
- ~~[] Flesh out admin commands~~

## 0.3.0.rc1
- [x] Dockerized!
- [x] Refactored settings commands.
- [x] Storing and retrieving settings from Postgres.
- [x] Updated dependencies to be in line with current.
## 0.3.0.rc2
- [x] Clean command
- [x] Bug fixes
- ~~[ ] Down vote~~ (not working)
## ~~0.3.0.rc3~~
## 0.3.0.rc4
* fix storing auto role and timeout I think
* download and skip together
* ~~try to finally fix this fucking volume bug~~
* fix loading guild settings
* add pgadmin to docker compose
* ~~fix volume~~ (volume is still broken)
## 0.3.0.rc5 (2024/03/09)
- cargo update
- GuildId checks
- user authorized message
- adding scan command
- add feature for osint
- make admin commands usable by guild members with admin
- add dry run to rename_all
## 0.1.4 (crack-osint) (2024/03/12)
- osint scan command to check urls for malicious content
...
## 1.0.0
- [ ] RTChris' Demuxer in C++ (for fun)?


<p align="center">
<sub><sup>Originally forked from <a href="https://github.com/aquelemiguel/parrot">Parrot</a></sup></sub>
<p>