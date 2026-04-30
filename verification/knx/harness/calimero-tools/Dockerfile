FROM rust:1.87-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl default-jre-headless python3 \
    && rm -rf /var/lib/apt/lists/*

ENV CALIMERO_TOOLS_VERSION=2.6
ENV CALIMERO_TOOLS_JAR=/opt/calimero-tools/calimero-tools-2.6.jar

RUN mkdir -p /opt/calimero-tools \
    && curl -fsSL \
        "https://repo1.maven.org/maven2/com/github/calimero/calimero-tools/${CALIMERO_TOOLS_VERSION}/calimero-tools-${CALIMERO_TOOLS_VERSION}.jar" \
        -o "$CALIMERO_TOOLS_JAR"
