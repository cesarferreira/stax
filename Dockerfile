FROM rust:1

RUN apt-get update \
	&& apt-get install -y --no-install-recommends mold curl ca-certificates \
	&& rm -rf /var/lib/apt/lists/* \
	&& case "$(uname -m)" in \
		aarch64 | arm64) nextest_platform=linux-arm ;; \
		x86_64 | amd64) nextest_platform=linux ;; \
		*) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;; \
	esac \
	&& curl -LsSf "https://get.nexte.st/latest/${nextest_platform}" | tar zxf - -C /usr/local/cargo/bin

ENV RUSTFLAGS="-C link-arg=-fuse-ld=mold"
WORKDIR /work
