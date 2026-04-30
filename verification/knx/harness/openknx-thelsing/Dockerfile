FROM rust:1.87-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates python3 \
    && rm -rf /var/lib/apt/lists/*

ENV OPENKNX_THELSING_MODE=fixture-replay
