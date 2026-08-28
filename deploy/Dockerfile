# Sparta State Engine — single minimal image carrying all four binaries
# (gateway, scanner, meter, fixtures). Build from the repo root:
#   docker build -f deploy/Dockerfile -t sparta-engine .

FROM rust:1-slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config make cmake g++ && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release \
    -p sparta-gateway -p sparta-scanner -p sparta-meter -p sparta-testkit

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home sparta
COPY --from=builder /build/target/release/sparta-gateway /usr/local/bin/
COPY --from=builder /build/target/release/sparta-scanner /usr/local/bin/
COPY --from=builder /build/target/release/sparta-meter   /usr/local/bin/
COPY --from=builder /build/target/release/sparta-fixtures /usr/local/bin/
USER sparta
WORKDIR /home/sparta
# Local state (metadata cache, meter log, chunk cache) lives on a volume.
ENV SPARTA_DB_PATH=/data/sparta-gateway.sqlite \
    SPARTA_METER_PATH=/data/sparta-meter.jsonl \
    SPARTA_CACHE_DIR=/data/cache
VOLUME ["/data"]
EXPOSE 8014
ENTRYPOINT ["sparta-gateway"]
CMD ["serve"]
