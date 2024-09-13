# STAGE1: Build the binary
FROM rust:1.80.1-alpine AS builder

# Install build dependencies
# openssl-dev openssl
RUN apk add --no-cache build-base musl-dev

# Create a new empty shell project
WORKDIR /app

# Copy over the Cargo.toml files to the shell project
COPY ./Cargo.toml ./Cargo.lock ./
RUN mkdir -p /app/crack-{cli,voting,bf,core,gpt,osint}
COPY ./crack-voting/Cargo.toml ./crack-voting/
COPY ./crack-bf/Cargo.toml ./crack-bf/
COPY ./crack-core/Cargo.toml ./crack-core/
COPY ./crack-gpt/Cargo.toml ./crack-gpt/
COPY ./crack-osint/Cargo.toml ./crack-osint/
COPY ./crack-cli/Cargo.toml ./crack-cli/

# # Build and cache the dependencies
RUN mkdir -p crack-voting/src && echo "fn main() {}" > crack-voting/src/main.rs
RUN mkdir -p crack-cli/src && echo "fn main() {}" > crack-cli/src/main.rs
RUN mkdir -p crack-bf/src && echo "" > crack-bf/src/lib.rs
RUN mkdir -p crack-core/src && echo "" > crack-core/src/lib.rs
RUN mkdir -p crack-gpt/src && echo "" > crack-gpt/src/lib.rs
RUN mkdir -p crack-osint/src && echo "" > crack-osint/src/lib.rs
RUN cargo fetch
RUN cargo build -p crack-voting --profile=dist
RUN rm crack-voting/src/main.rs
COPY . .

# Copy the actual code files and build the application
# COPY ./crack-voting/src ./crack-voting/
# Update the file date
RUN touch ./crack-voting/src/main.rs
RUN cargo build -p crack-voting --release

# STAGE2: create a slim image with the compiled binary
FROM alpine AS runner

# Copy the binary from the builder stage
WORKDIR /app
COPY --from=builder /app/target/dist/crack-voting /app/app

CMD ["/app/app"]