# Production Dockerfile for Railway Deployment
# True Linux multi-stage build

# STAGE 1: Rust builder (nightly required for edition2024 dependencies)
FROM rustlang/rust:nightly-bookworm AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo files first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build release binary
RUN cargo build --release --bin pramagraph-financial

# STAGE 2: debian:bookworm-slim runtime
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root appuser
RUN useradd -r -s /bin/bash -m appuser

WORKDIR /app

# Copy the Linux release binary FROM the builder stage
COPY --from=builder /build/target/release/pramagraph-financial /app/pramagraph-financial

# Copy required runtime data directories from build context (not from builder)
COPY data ./data
COPY calibration ./calibration

# Create directories for logging
RUN mkdir -p /app/results/runtime && chown -R appuser:appuser /app

# Switch to non-root user
USER appuser

# Expose port (Railway will set PORT env var)
EXPOSE 8080

# Set default environment variables
ENV RUST_LOG=info
ENV PORT=8080

# Run the server
CMD ["/app/pramagraph-financial", "serve", "--bind", "0.0.0.0:8080", "--corpus", "data/corpus", "--calibration", "calibration/profiles"]