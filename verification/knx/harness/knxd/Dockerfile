FROM rust:1.87-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates knxd knxd-tools python3 \
    && rm -rf /var/lib/apt/lists/*

ENV KNXD_EXPECTED_VERSION=0.14.54.1
