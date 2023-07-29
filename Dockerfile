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

#RUN useradd -m cracktunes
#USER cracktunes
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
FROM debian:bookworm-slim

RUN apt-get update -y && apt-get install -y ffmpeg curl
RUN curl -o /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/download/2023.07.06/yt-dlp_linux && chmod +x /usr/local/bin/yt-dlp

#RUN useradd -m cracktunes
#USER cracktunes
RUN yt-dlp -v -h

COPY --from=build /cracktunes/target/release/cracktunes .

CMD ["/cracktunes/cracktunes"]
