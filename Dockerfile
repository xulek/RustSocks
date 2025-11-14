# ==============================================================================
# Stage 1: Dashboard Builder (Node.js + Vite)
# ==============================================================================
FROM node:18-alpine AS dashboard-builder
WORKDIR /build/dashboard
COPY dashboard/package*.json ./
RUN npm ci
COPY dashboard/ ./
RUN npm run build

# ==============================================================================
# Stage 2: Rust Builder (musl, dynamically PAM)
# ==============================================================================
FROM rust:1.90-alpine AS rust-builder
WORKDIR /build

# Build deps
RUN apk add --no-cache \
    build-base \
    musl-dev \
    linux-pam-dev \
    openssl-dev \
    pkgconfig \
    curl

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
# Quick cache warmup: empty src
RUN mkdir -p src && echo "fn main(){}" > src/main.rs && cargo build --release || true
RUN rm -rf src

# Copy source files
COPY src/ ./src/
COPY migrations/ ./migrations/

# Build release with all the features
RUN cargo build --release --all-features && \
    strip /build/target/release/rustsocks

# (linker debugging) show what is dynamically linked
RUN ldd /build/target/release/rustsocks || true

# ==============================================================================
# Stage 3: Runtime (Alpine with PAM and .so)
# ==============================================================================
FROM alpine:3.19

RUN apk add --no-cache \
    linux-pam \
    libgcc \
    ca-certificates \
    libssl3 \
    libcrypto3 \
    libstdc++

# Non-root user
RUN addgroup -g 1000 rustsocks && \
    adduser -D -u 1000 -G rustsocks rustsocks

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
COPY docker/configs/rustsocks.docker.toml /etc/rustsocks/rustsocks.toml.example
COPY docker/configs/acl.docker.toml /etc/rustsocks/acl.toml.example

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
