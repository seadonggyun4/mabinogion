FROM rust:1.87-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates nodejs npm \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g knx@2.5.4

ENV NODE_PATH=/usr/local/lib/node_modules
ENV MABI_KNX_NODE_PACKAGE_VERSION=2.5.4
