# Build image
# Necessary dependencies to build CrackTunes
FROM rust:slim-bookworm as build

#build-essential \
RUN apt-get update -y && apt-get install -y \
       autoconf \
       automake \
       cmake \
       libtool \
       libssl-dev \
       pkg-config

WORKDIR "/app"

COPY . .
RUN ls -al . && ls -al data
ENV DATABASE_URL sqlite:///app/data/crackedmusic.db
RUN cargo build --release --locked

# Release image
# Necessary dependencies to run CrackTunes
FROM debian:bookworm-slim AS runtime

RUN apt-get update -y \
       && apt-get install -y ffmpeg curl \
       # Clean up
       && apt-get autoremove -y \
       && apt-get clean -y \
       && rm -rf /var/lib/apt/lists/*

RUN curl -o /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/download/2023.10.13/yt-dlp_linux && chmod +x /usr/local/bin/yt-dlp

RUN yt-dlp -v -h

COPY --from=build /app/target/release/cracktunes .
COPY --from=build /app/data  /data
RUN ls -al / && ls -al /data

ENV APP_ENVIRONMENT production
ENV DATABASE_URL sqlite:///data/crackedmusic.db
ENV RUST_BACKTRACE 1
CMD ["/app/cracktunes"]
