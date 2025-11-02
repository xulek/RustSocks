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
# Stage 2: Rust Builder (musl, dynamiczne PAM)
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

# Klucz: wyłącz statyczny CRT i wymuś dynamiczne linkowanie systemowych .so
# -crt-static -> musl będzie linkowany dynamicznie
# -Bdynamic  -> preferencja dla .so podczas linkowania
ENV RUSTFLAGS="-C target-feature=-crt-static -C link-args=-Wl,-Bdynamic"
# Ułatwia pkg-config w środowisku cross (tu bezpieczne)
ENV PKG_CONFIG_ALLOW_CROSS=1

# (opcjonalnie) gdy używasz crate'ów wymagających pkg-config:
# ENV PKG_CONFIG_PATH="/usr/lib/pkgconfig:/usr/local/lib/pkgconfig"

# Copy Cargo metadata first for cache
COPY Cargo.toml Cargo.lock ./
# Szybkie zapośredniczenie cache: puste src
RUN mkdir -p src && echo "fn main(){}" > src/main.rs && cargo build --release || true
RUN rm -rf src

# Copy źródeł
COPY src/ ./src/
COPY migrations/ ./migrations/

# Build release z wszystkimi feature'ami
RUN cargo build --release --all-features && \
    strip /build/target/release/rustsocks

# (debug linkera) pokaż co jest dynamiczne
RUN ldd /build/target/release/rustsocks || true

# ==============================================================================
# Stage 3: Runtime (Alpine z PAM i .so)
# ==============================================================================
FROM alpine:3.19

RUN apk add --no-cache \
    linux-pam \
    libgcc \
    ca-certificates \
    libssl3 \
    libcrypto3 \
    libstdc++

# Użytkownik nie-root
RUN addgroup -g 1000 rustsocks && \
    adduser -D -u 1000 -G rustsocks rustsocks

# Struktura katalogów
RUN mkdir -p \
    /etc/rustsocks \
    /data \
    /var/log/rustsocks \
    /app/dashboard && \
    chown -R rustsocks:rustsocks \
      /etc/rustsocks /data /var/log/rustsocks /app

# Binarka
COPY --from=rust-builder /build/target/release/rustsocks /usr/local/bin/rustsocks

# Dashboard
COPY --from=dashboard-builder /build/dashboard/dist /app/dashboard/dist

# Entrypoint i konfiguracje przykładowe
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
