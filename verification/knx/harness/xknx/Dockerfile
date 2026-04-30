FROM rust:1.87-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 python3-pip python3-venv ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY verification/knx/harness/xknx/requirements.txt /tmp/xknx-requirements.txt

RUN python3 -m venv /opt/xknx-venv \
    && /opt/xknx-venv/bin/pip install --no-cache-dir --upgrade pip \
    && /opt/xknx-venv/bin/pip install --no-cache-dir -r /tmp/xknx-requirements.txt

ENV PATH="/opt/xknx-venv/bin:${PATH}"
WORKDIR /workspace
