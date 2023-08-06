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

WORKDIR "/cracktunes"

# Cache cargo build dependencies by creating a dummy source
RUN mkdir src
RUN echo "fn main() {}" > src/main.rs
COPY Cargo.toml ./
COPY Cargo.lock ./
RUN cargo build --release --locked

COPY . .
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

RUN curl -o /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/download/2023.07.06/yt-dlp_linux && chmod +x /usr/local/bin/yt-dlp

RUN yt-dlp -v -h

COPY --from=build /cracktunes/target/release/cracktunes .

ENV APP_ENVIRONMENT production
ENV RUST_BACKTRACE 1
CMD ["/cracktunes/cracktunes"]
