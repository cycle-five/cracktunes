# Build image
# Necessary dependencies to build CrackTunes
FROM debian:bookworm-slim as build
ARG SQLX_OFFLINE=true

RUN apt-get update && apt-get install -y \
       autoconf \
       automake \
       cmake \
       libtool \
       libssl-dev \
       pkg-config \
       libopus-dev \
       curl \
       git

# Get Rust
RUN curl -proto '=https' -tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
       && . "$HOME/.cargo/env" \
       && rustup default stable

WORKDIR "/app"

COPY . .
RUN . "$HOME/.cargo/env" && cargo build --release --locked

# Release image
# Necessary dependencies to run CrackTunes
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
       # && apt-get upgrade -y \
       && apt-get install -y ffmpeg curl \
       # Clean up
       && apt-get autoremove -y \
       && apt-get clean -y \
       && rm -rf /var/lib/apt/lists/*

RUN curl -sSL --output /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp-master-builds/releases/download/2024.02.13.195934/yt-dlp_linux \ 
       && chmod +x /usr/local/bin/yt-dlp



RUN yt-dlp -v -h

# USER 1000
WORKDIR "/app"

COPY --from=build /app/target/release/cracktunes /app/cracktunes
COPY --from=build /app/data  /app/data
COPY --from=build /app/.env.example /app/.env
COPY --from=build /app/cracktunes.toml /app/cracktunes.toml
# RUN ls -al / && ls -al /data

ENV APP_ENVIRONMENT production
RUN . "/app/.env"
ENV DATABASE_URL postgresql://postgres:mysecretpassword@localhost:5432/postgres
CMD ["/app/cracktunes"]
