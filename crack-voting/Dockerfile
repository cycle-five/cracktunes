# STAGE1: Build the binary
FROM rust:alpine AS builder

# Install build dependencies
RUN apk add --no-cache build-base musl-dev openssl-dev openssl

# Create a new empty shell project
WORKDIR /app

# Copy over the Cargo.toml files to the shell project
COPY . .
# COPY Cargo.toml ./Cargo.lock crack-voting ./

# # Build and cache the dependencies
# RUN mkdir -p crack-voting/src && echo "fn main() {}" > crack-voting/src/main.rs
# RUN cargo fetch
# RUN cargo build -p crack-voting --release
# RUN rm crack-voting/src/main.rs

# Copy the actual code files and build the application
# COPY ./crack-voting/src ./crack-voting/
# Update the file date
RUN touch ./crack-voting/src/main.rs
RUN cargo build -p crack-voting --release

# STAGE2: create a slim image with the compiled binary
FROM alpine AS runner

# Copy the binary from the builder stage
WORKDIR /app
COPY --from=builder /app/target/release/crack-voting /app/app
COPY --from=builder /app/.env /app/.env

RUN . "/app/.env"
ENV APP_ENVIRONMENT=production
ENV DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres

CMD ["/app/app"]