# STAGE1: Build the binary
FROM rust:1.98.0-alpine3.22 AS builder

# Install build dependencies
# RUN apk add --no-cache build-base musl-dev openssl-dev openssl cmake
# Versions are deliberately unpinned: the previous pins were exact
# alpine 3.20 package revisions and do not resolve on newer bases.
RUN apk add --no-cache \
  build-base \
  musl-dev \
  cmake \
  git

# Default directory
WORKDIR /app

#
# Create a new empty shell project
# Build and cache the dependencies

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
