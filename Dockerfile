# ==============================================================================
# Stage 1: Dashboard Builder (Node.js + Vite)
# ==============================================================================
FROM node:20-alpine AS dashboard-builder
WORKDIR /build/dashboard
COPY dashboard/package*.json ./
RUN npm ci
COPY dashboard/ ./
RUN npm run build

# ==============================================================================
# Stage 2: Rust Builder (musl, dynamically PAM)
# ==============================================================================
FROM rust:1.90 AS rust-builder
WORKDIR /build

# Build deps
RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential \
      musl-dev \
      libpam0g-dev \
      libkrb5-dev \
      clang \
      llvm-dev \
      libclang-dev \
      libssl-dev \
      pkg-config \
      curl && \
    rm -rf /var/lib/apt/lists/*

# Key: disable the static CRT and force dynamic linking of system .so
# -crt-static -> musl will link dynamically
# -Bdynamic  -> prefer shared objects during linking
ENV RUSTFLAGS="-C target-feature=-crt-static -C link-args=-Wl,-Bdynamic"
# Helps pkg-config in a cross compilation environment (safe here)
ENV PKG_CONFIG_ALLOW_CROSS=1

# (optional) when using crates that require pkg-config:
# ENV PKG_CONFIG_PATH="/usr/lib/pkgconfig:/usr/local/lib/pkgconfig"

# Copy Cargo metadata first for cache
COPY Cargo.toml Cargo.lock ./
COPY benches/ ./benches/
# Quick cache warmup: empty src
RUN mkdir -p src && echo "fn main(){}" > src/main.rs && \
    export LIBCLANG_PATH=$(llvm-config --libdir) && \
    cargo build --release || true
RUN rm -rf src

# Copy source files
COPY src/ ./src/
COPY migrations/ ./migrations/

# Build release with all the features
RUN export LIBCLANG_PATH=$(llvm-config --libdir) && \
    cargo build --release --all-features && \
    strip /build/target/release/rustsocks

# (linker debugging) show what is dynamically linked
RUN ldd /build/target/release/rustsocks || true

# ==============================================================================
# Stage 3: Runtime (Debian trixie slim with matching glibc)
# ==============================================================================
FROM debian:trixie-slim

RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      libpam0g \
      libpam-modules \
      libkrb5-3 \
      libgssapi-krb5-2 \
      libgcc-s1 \
      libstdc++6 \
      libssl3 \
      ca-certificates \
      wget && \
    rm -rf /var/lib/apt/lists/*

# Non-root user
RUN groupadd -g 1000 rustsocks && \
    useradd -u 1000 -g rustsocks -m -s /bin/bash rustsocks

# Directory layout
RUN mkdir -p \
    /etc/rustsocks \
    /data \
    /var/log/rustsocks \
    /app/dashboard && \
    chown -R rustsocks:rustsocks \
      /etc/rustsocks /data /var/log/rustsocks /app

# Binary
COPY --from=rust-builder /build/target/release/rustsocks /usr/local/bin/rustsocks

# Dashboard
COPY --from=dashboard-builder /build/dashboard/dist /app/dashboard/dist

# Entrypoint and sample configurations
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
COPY docker/configs/rustsocks.toml /etc/rustsocks/rustsocks.toml.example
COPY docker/configs/acl.toml /etc/rustsocks/acl.toml.example

WORKDIR /app
USER rustsocks

EXPOSE 1080 9090

HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD wget --no-verbose --tries=1 --spider http://localhost:9090/health || exit 1

ENTRYPOINT ["/entrypoint.sh"]
CMD ["rustsocks", "--config", "/etc/rustsocks/rustsocks.toml"]

LABEL org.opencontainers.image.title="RustSocks" \
      org.opencontainers.image.description="High-performance SOCKS5 proxy with ACL, PAM auth, and web dashboard" \
      org.opencontainers.image.version="0.9.0" \
      org.opencontainers.image.vendor="RustSocks Contributors" \
      org.opencontainers.image.licenses="MIT"
