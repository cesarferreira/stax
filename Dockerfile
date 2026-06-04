FROM rust:latest

RUN cargo install cargo-nextest --locked

WORKDIR /work
