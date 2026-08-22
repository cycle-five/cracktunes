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
RUN apk add --no-cache \
  ffmpeg \
  curl

ADD ./data /data
RUN curl -sSL --output /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux \
  && chmod +x /usr/local/bin/yt-dlp
# Copy the binary from the builder stage
COPY --from=builder /app/target/dist/cracktunes /app/app
# Copy the start script from the builder stage
COPY --from=builder /app/scripts/start.sh /app/start.sh

CMD ["/app/start.sh"]
