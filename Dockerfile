# STAGE1: Build the binary
FROM rust:1.81.0-alpine3.20 AS builder

# Install build dependencies
# RUN apk add --no-cache build-base musl-dev openssl-dev openssl cmake
RUN apk add --no-cache \
  build-base=0.5-r3 \
  musl-dev=1.2.5-r1 \
  cmake=3.29.3-r0 \
  git=2.45.3-r0

# Default directory
WORKDIR /app

#
# Create a new empty shell project
# Build and cache the dependencies

# Copy all the files
COPY . .

RUN cargo build -p cracktunes --profile=dist

# STAGE2: create a slim image with the compiled binary
FROM alpine:3.20 AS runner

# Default directory
WORKDIR /app

# RUN apk add --no-cache ffmpeg curl
RUN apk add --no-cache \
  ffmpeg=6.1.1-r8 \
  curl

ADD ./data /data
RUN curl -sSL --output /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux \
  && chmod +x /usr/local/bin/yt-dlp
# Copy the binary from the builder stage
COPY --from=builder /app/target/dist/cracktunes /app/app
# Copy the start script from the builder stage
COPY --from=builder /app/scripts/start.sh /app/start.sh

CMD ["/app/start.sh"]
