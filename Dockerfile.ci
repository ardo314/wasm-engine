FROM rust:slim

RUN apt-get update && apt-get install -y \
    git \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install --locked cargo-component
RUN cargo install --locked wasm-tools
RUN cargo install --locked wkg
RUN rustup component add rustfmt