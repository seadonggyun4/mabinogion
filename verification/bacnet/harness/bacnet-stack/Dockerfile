FROM rust:1.87-bookworm

ARG BACNET_STACK_TAG=bacnet-stack-1.4.3

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates gcc git make python3 \
    && rm -rf /var/lib/apt/lists/*

RUN git clone --depth 1 --branch "${BACNET_STACK_TAG}" https://github.com/bacnet-stack/bacnet-stack.git /opt/bacnet-stack \
    && make -C /opt/bacnet-stack clean all

WORKDIR /workspace
