FROM rust:1.87-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates maven openjdk-17-jdk-headless \
    && rm -rf /var/lib/apt/lists/*

COPY verification/bacnet/harness/bacnet4j/pom.xml /tmp/bacnet4j-pom.xml
RUN mkdir -p /tmp/bacnet4j-src \
    && mvn -q -f /tmp/bacnet4j-pom.xml dependency:go-offline

WORKDIR /workspace
