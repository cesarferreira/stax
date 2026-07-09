FROM rust:1.96.1-trixie

ARG CARGO_NEXTEST_VERSION=0.9.114

RUN rustup component add clippy rustfmt \
	&& apt-get update \
	&& apt-get install -y --no-install-recommends mold ca-certificates \
	&& rm -rf /var/lib/apt/lists/* \
	&& cargo install cargo-nextest --version "${CARGO_NEXTEST_VERSION}" --locked

ENV RUSTFLAGS="-C link-arg=-fuse-ld=mold"
WORKDIR /work
