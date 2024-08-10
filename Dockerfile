# STAGE1: Build the binary
FROM rust:alpine AS builder

# Install build dependencies
RUN apk add --no-cache build-base musl-dev openssl-dev openssl cmake

# Create a new empty shell project
WORKDIR /app

# Build and cache the dependencies
COPY . .

RUN cargo build --no-default-features --features crack-tracing -p cracktunes --release

# STAGE2: create a slim image with the compiled binary
FROM alpine AS runner
# Copy the binary from the builder stage
WORKDIR /app

# RUN apk add --no-cache ffmpeg curl
RUN apk update && apk add --no-cache ffmpeg curl

ADD ./data /data
RUN curl -sSL --output /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/download/2024.08.06/yt-dlp_linux \
    && chmod +x /usr/local/bin/yt-dlp
COPY --from=builder /app/target/release/cracktunes /app/app
COPY --from=builder /app/.env /app/.env
COPY --from=builder /app/scripts/start.sh /app/start.sh

# RUN . "/app/.env"
ENV APP_ENVIRONMENT=production
ENV DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres

CMD ["/app/start.sh"]