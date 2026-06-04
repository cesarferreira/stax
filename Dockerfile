FROM rust:1.95

RUN cargo install cargo-nextest --locked

WORKDIR /work
