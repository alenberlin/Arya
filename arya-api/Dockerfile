# Reproducible, non-root Arya API image. Pinned base digests should be added
# before the first production deploy (see docs/release.md).
FROM rust:1.94-slim-bookworm AS builder
WORKDIR /build
ARG GIT_SHA=unknown
ENV ARYA_SOURCE_COMMIT=$GIT_SHA
# Deterministic codegen for reproducible builds.
ENV CARGO_INCREMENTAL=0 SOURCE_DATE_EPOCH=0
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release --locked && strip target/release/arya-api

FROM debian:bookworm-slim
RUN useradd --uid 10001 --no-create-home --shell /usr/sbin/nologin arya \
    && apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/arya-api /usr/local/bin/arya-api
USER 10001
ENV ARYA_API_BIND=0.0.0.0:8477
EXPOSE 8477
ENTRYPOINT ["/usr/local/bin/arya-api"]
