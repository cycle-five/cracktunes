# STAGE1: Build the binary
FROM rust:alpine AS builder

# Install build dependencies
RUN apk add --no-cache build-base musl-dev openssl-dev openssl cmake

# Create a new empty shell project
WORKDIR /app


# RUN mkdir -p /app/scripts
# COPY ./scripts/start.sh /app/scripts/start.sh

# COPY ./Cargo.toml ./
# RUN mkdir -p /app/crack-{voting,bf,core,gpt,osint}
# RUN mkdir -p /app/cracktunes
# COPY ./crack-voting/Cargo.toml ./crack-voting/
# COPY ./crack-bf/Cargo.toml ./crack-bf/
# COPY ./crack-core/Cargo.toml ./crack-core/
# COPY ./crack-gpt/Cargo.toml ./crack-gpt/
# COPY ./crack-osint/Cargo.toml ./crack-osint/
# COPY ./cracktunes/Cargo.toml ./cracktunes/

# # # Build and cache the dependencies
# RUN mkdir -p crack-voting/src && echo "fn main() {}" > crack-voting/src/main.rs
# RUN mkdir -p cracktunes/src && echo "fn main() {}" > cracktunes/src/main.rs
# RUN echo "fn main() {}" > cracktunes/build.rs
# RUN mkdir -p crack-bf/src && echo "" > crack-bf/src/lib.rs
# RUN mkdir -p crack-core/src && echo "" > crack-core/src/lib.rs
# RUN mkdir -p crack-gpt/src && echo "" > crack-gpt/src/lib.rs
# RUN mkdir -p crack-osint/src && echo "" > crack-osint/src/lib.rs
# RUN cargo fetch
# # RUN cargo build --profile=release-with-performance --locked --features crack-bf,crack-gpt,crack-osint
# RUN cargo build
# RUN rm crack-voting/src/main.rs
# RUN rm cracktunes/src/main.rs
COPY . .

# Copy the actual code files and build the application
# COPY ./crack-voting/src ./crack-voting/
# # Update the file date
# #RUN touch ./crack-voting/src/main.rs
# RUN touch ./cracktunes/src/main.rs
# # RUN cargo build -p crack-voting --release
RUN cargo build

# STAGE2: create a slim image with the compiled binary
FROM alpine AS runner
# Copy the binary from the builder stage
WORKDIR /app

# RUN apk add --no-cache ffmpeg curl
RUN apk update && apk add --no-cache ffmpeg curl

ADD ./data /data
RUN curl -sSL --output /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/download/2024.08.06/yt-dlp_linux \
    && chmod +x /usr/local/bin/yt-dlp
COPY --from=builder /app/target/debug/cracktunes /app/app
COPY --from=builder /app/.env /app/.env
COPY --from=builder /app/scripts/start.sh /app/start.sh

# RUN . "/app/.env"
ENV APP_ENVIRONMENT=production
ENV DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres

CMD ["/app/start.sh"]