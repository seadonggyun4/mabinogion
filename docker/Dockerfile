# =============================================================================
# trap-simulator Dockerfile
# =============================================================================
# Multi-stage build for optimal image size and security
#
# Stages:
# 1. builder  - Compile the application
# 2. runtime  - Minimal runtime image
#
# Usage:
#   docker build -t trap-simulator .
#   docker run -p 8080:8080 -p 9090:9090 trap-simulator
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Builder
# -----------------------------------------------------------------------------
FROM rust:1.75-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock ./
COPY crates/trap-sim-core/Cargo.toml crates/trap-sim-core/
COPY crates/trap-sim-modbus/Cargo.toml crates/trap-sim-modbus/
COPY crates/trap-sim-opcua/Cargo.toml crates/trap-sim-opcua/
COPY crates/trap-sim-bacnet/Cargo.toml crates/trap-sim-bacnet/
COPY crates/trap-sim-knx/Cargo.toml crates/trap-sim-knx/
COPY crates/trap-sim-scenario/Cargo.toml crates/trap-sim-scenario/
COPY crates/trap-sim-chaos/Cargo.toml crates/trap-sim-chaos/
COPY crates/trap-sim-web/Cargo.toml crates/trap-sim-web/

# Create dummy source files for dependency caching
RUN mkdir -p crates/trap-sim-core/src && echo "pub fn dummy() {}" > crates/trap-sim-core/src/lib.rs
RUN mkdir -p crates/trap-sim-modbus/src && echo "pub fn dummy() {}" > crates/trap-sim-modbus/src/lib.rs
RUN mkdir -p crates/trap-sim-opcua/src && echo "pub fn dummy() {}" > crates/trap-sim-opcua/src/lib.rs
RUN mkdir -p crates/trap-sim-bacnet/src && echo "pub fn dummy() {}" > crates/trap-sim-bacnet/src/lib.rs
RUN mkdir -p crates/trap-sim-knx/src && echo "pub fn dummy() {}" > crates/trap-sim-knx/src/lib.rs
RUN mkdir -p crates/trap-sim-scenario/src && echo "pub fn dummy() {}" > crates/trap-sim-scenario/src/lib.rs
RUN mkdir -p crates/trap-sim-chaos/src && echo "pub fn dummy() {}" > crates/trap-sim-chaos/src/lib.rs
RUN mkdir -p crates/trap-sim-web/src && echo "pub fn dummy() {}" > crates/trap-sim-web/src/lib.rs

# Build dependencies only (this layer is cached)
RUN cargo build --release 2>/dev/null || true

# Copy actual source code
COPY crates/ crates/
COPY scenarios/ scenarios/

# Build the application
RUN cargo build --release --workspace

# -----------------------------------------------------------------------------
# Stage 2: Runtime
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -r -s /bin/false -u 1000 simulator

# Create directories
RUN mkdir -p /app/config /app/logs /app/scenarios /app/data \
    && chown -R simulator:simulator /app

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /app/target/release/trap-sim-* /app/

# Copy default configurations and scenarios
COPY --from=builder /app/scenarios/ /app/scenarios/

# Set permissions
RUN chmod +x /app/trap-sim-*

# Switch to non-root user
USER simulator

# Environment variables
ENV RUST_LOG=info \
    TRAP_SIM_CONFIG=/app/config/simulator.yaml \
    TRAP_SIM_LOG_DIR=/app/logs \
    TRAP_SIM_DATA_DIR=/app/data

# Expose ports
# 502   - Modbus TCP
# 4840  - OPC UA
# 47808 - BACnet/IP
# 3671  - KNXnet/IP
# 8080  - Web UI / REST API
# 9090  - Prometheus metrics
EXPOSE 502 4840 47808 3671 8080 9090

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Default command (can be overridden)
CMD ["./trap-sim-server"]

# Labels
LABEL org.opencontainers.image.title="TRAP Simulator" \
      org.opencontainers.image.description="Enterprise-grade protocol simulator for TRAP ecosystem" \
      org.opencontainers.image.vendor="Sylvex" \
      org.opencontainers.image.version="0.1.0" \
      org.opencontainers.image.source="https://github.com/sylvex/trap-simulator"
