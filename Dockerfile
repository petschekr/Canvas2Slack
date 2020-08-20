FROM rust:1.45-stretch AS builder

WORKDIR /usr/src

# Create a dummy project and build the app's dependencies.
# If the Cargo.toml or Cargo.lock files have not changed,
# we can use the docker build cache and skip these (typically slow) steps.
RUN USER=root cargo new canvas2slack
WORKDIR /usr/src/canvas2slack
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release

# Copy the source and build the application.
COPY src ./src
RUN cargo install --path .

##################################################################
FROM debian:stretch-slim

WORKDIR /usr/bin/canvas2slack
VOLUME [ "/usr/bin/canvas2slack" ]

RUN apt-get update && apt-get install -y openssl libssl1.1 libssl-dev ca-certificates

COPY --from=builder /usr/local/cargo/bin/canvas2slack .
COPY config.json .
CMD ["./canvas2slack"]
