# STAGE1: Build the binary, in three parts so the dependencies cache (#424).
#
# 🔑 WHY cargo-chef. A single `COPY . .` followed by `cargo build` meant every
# commit changed the copy, so the compile layer never cached: every master build
# was a from-scratch ~17-minute compile of all 563 crates, and mode=max then
# exported that 1.3 GB layer to the GHA cache, where nothing ever read it again.
#
# Now `planner` reduces the workspace to a recipe (manifests, lockfile and
# rust-toolchain.toml, with our own crates' versions masked, so a release bump
# does not change it). `builder` compiles the dependencies from that recipe
# ALONE, then copies the source and compiles only our crates. The dependency
# layer caches until Cargo.lock, a manifest's dependencies or the toolchain file
# changes.
FROM rust:1.98.0-alpine3.22 AS chef

# Install build dependencies
# RUN apk add --no-cache build-base musl-dev openssl-dev openssl cmake
# Versions are deliberately unpinned: the previous pins were exact
# alpine 3.20 package revisions and do not resolve on newer bases.
RUN apk add --no-cache \
  build-base \
  musl-dev \
  cmake \
  git

# 🔑 BEFORE `COPY . .`, deliberately. sqlx-cli takes minutes to build and never
# changes with our source, so keeping it above the copy means the layer caches
# across every source change and only rebuilds when this base image moves.
#
# Pinned to ~0.8 to match the sqlx 0.8.2 that writes the _sqlx_migrations
# ledger; a mismatched CLI can disagree with the library about that table.
RUN cargo install sqlx-cli --version '~0.8' --no-default-features --features rustls,postgres

# Pinned exactly, for the same reason as any build tool: a new release can change
# the recipe format, and with it what counts as a cache hit.
RUN cargo install cargo-chef --version '=0.1.78' --locked

# Default directory
WORKDIR /app

FROM chef AS planner
COPY . .
# 🪤 RUSTUP_TOOLCHAIN, for this one command. rust-toolchain.toml says `stable`,
# which is not the toolchain this image ships (it ships $RUST_VERSION), so any
# cargo invocation here would first download the current stable. This stage
# reruns on every commit, and `prepare` only reads metadata, so any toolchain
# will do. The compile in `builder` still uses rust-toolchain.toml.
RUN RUSTUP_TOOLCHAIN="$RUST_VERSION" cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
# 🪤 BEFORE the cook, and this file alone. `cargo chef cook` is a cargo
# subcommand, so rustup resolves the toolchain when cook is INVOKED -- before
# cook writes the recipe's copy of this file into /app. Without this line the
# dependencies compile under the image's pinned toolchain while the build below,
# which sees the real file, compiles under `stable`. Different rustc, so cargo
# discards every cooked artifact: measured on the first run of this PR as a 616s
# cook followed by a full 1328s build, slower than the single build it replaced.
#
# Its own layer, so it is invalidated only when the toolchain file changes. The
# `stable` it resolves is installed inside the cook layer and reused by the build
# below, which is what makes the two agree.
COPY rust-toolchain.toml .
COPY --from=planner /app/recipe.json recipe.json
# The dependencies only. The package and profile must match the build below
# exactly, or cargo treats these artifacts as a different build and recompiles
# them. This is also where rust-toolchain.toml (carried in the recipe) installs
# the current stable, so the toolchain is cached along with the dependencies.
RUN cargo chef cook -p cracktunes --profile=dist --recipe-path recipe.json

# Copy all the files
COPY . .

RUN cargo build -p cracktunes --profile=dist

# STAGE2: create a slim image with the compiled binary
FROM alpine:3.22 AS runner

# Default directory
WORKDIR /app

# RUN apk add --no-cache ffmpeg curl
# `deno` is a JavaScript runtime for yt-dlp, not for us: YouTube extraction
# without one is deprecated upstream and silently drops formats ("No supported
# JavaScript runtime could be found"). Alpine ships a musl-native build, so it
# needs no special handling -- unlike deno's own releases, which are glibc.
# It is ~89 MB, which is most of this image.
RUN apk add --no-cache \
  ffmpeg \
  curl \
  deno

ADD ./data /data
# 🪤 `yt-dlp_musllinux`, NOT `yt-dlp_linux`. This base is Alpine, so it is musl,
# and the glibc build cannot execute at all: it fails with "No such file or
# directory" -- pointing at the missing ELF interpreter, not at a missing file,
# which is a genuinely confusing way to be told you picked the wrong build. It
# shipped that way and yt-dlp was dead in every image until 2026-09-05.
#
# The `--version` is a build-time assertion, not a nicety: it is what turns
# picking the wrong build back into a failed build rather than a bot that
# cannot play anything.
RUN curl -sSL --fail --output /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_musllinux \
  && chmod +x /usr/local/bin/yt-dlp \
  && /usr/local/bin/yt-dlp --version
# Copy the binary from the builder stage
COPY --from=builder /app/target/dist/cracktunes /app/app
# Copy the start script from the builder stage
COPY --from=builder /app/scripts/start.sh /app/start.sh

CMD ["/app/start.sh"]

# STAGE3: the migration runner, used as a one-shot before the bot starts.
#
# 🪤 This stage is LAST, which makes it the default build target. The bot image
# must therefore be built with an explicit `--target runner`; the Docker
# workflow does this. Building with no target gets you this image, which is
# emphatically not the bot.
FROM alpine:3.22 AS migrate
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
COPY --from=builder /app/migrations /migrations
# Needs DATABASE_URL in the environment and nothing else.
#
# No ca-certificates here. Harmless today -- compose points this at a
# Postgres on the compose network with no sslmode, so rustls never needs a
# root store -- but the first `sslmode=require` or managed Postgres target
# will fail a TLS handshake from a one-shot container with no other context
# to explain why.
ENTRYPOINT ["sqlx", "migrate", "run", "--source", "/migrations"]
