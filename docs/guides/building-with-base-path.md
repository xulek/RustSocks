# Building RustSocks with URL Base Path

Complete guide for building and configuring RustSocks with a custom URL prefix (base path) for the API and dashboard.

## 📋 Table of Contents

- [How It Works](#how-it-works)
- [Configuration](#configuration)
- [Building the Application](#building-the-application)
- [Development Mode](#development-mode)
- [Production Deployment](#production-deployment)
- [Examples](#examples)

---

## 🔧 How It Works

RustSocks supports deployment under any URL path through intelligent frontend-backend integration:

### Backend (Rust)

1. **Config**: `sessions.base_path` defines the URL prefix (e.g., `/rustsocks`, `/proxy`, or `/`)
2. **Router nesting**: If `base_path != "/"`, the entire application (API + dashboard) is mounted under this prefix
3. **HTML rewriting**: Automatic `index.html` rewriting:
   - Injects `<script>window.__RUSTSOCKS_BASE_PATH__ = '/rustsocks';</script>` before `</head>`
   - Rewrites asset paths: `./assets/` → `/rustsocks/assets/`
4. **Static files**: Serves `dashboard/dist/` with automatic SPA routing fallback

### Frontend (React)

1. **Auto-detection**: `src/lib/basePath.js` automatically detects base path from:
   - `window.__RUSTSOCKS_BASE_PATH__` (injected by backend)
   - Or from script location (parses `/assets/index-*.js` URL)
   - Or from `window.location.pathname` (fallback)
2. **React Router**: `<BrowserRouter basename={ROUTER_BASENAME}>` for routing
3. **API calls**: `getApiUrl(path)` adds prefix to all fetch() calls
4. **Vite build**: Builds with `base: './'` (relative paths)

---

## ⚙️ Configuration

### 1. Backend Config (`config/rustsocks.toml`)

```toml
[sessions]
stats_api_enabled = true
dashboard_enabled = true
swagger_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090

# Base URL path prefix
base_path = "/rustsocks"  # Options: "/", "/rustsocks", "/proxy", etc.
```

**Important:**
- `base_path = "/"` - dashboard at `http://host/`
- `base_path = "/rustsocks"` - dashboard at `http://host/rustsocks`
- `base_path = "/rustsocks/"` - trailing slash is automatically removed

### 2. Frontend Config (`dashboard/vite.config.js`)

**No changes required!** Vite is configured with `base: './'` (relative paths), which works with any base path.

```js
export default defineConfig({
  base: './',  // ✅ MUST be './' for automatic functionality
  plugins: [react()],
  server: {
    port: 3000,
    proxy: {
      '/api': 'http://127.0.0.1:9090',
      '/health': 'http://127.0.0.1:9090',
      '/metrics': 'http://127.0.0.1:9090',
    }
  }
})
```

---

## 🏗️ Building the Application

### Step 1: Build Backend (Rust)

```bash
# Development build
cargo build

# Production build (optimized)
cargo build --release
```

### Step 2: Build Frontend (React Dashboard)

```bash
cd dashboard

# Install dependencies (first time only)
npm install

# Production build
npm run build
```

This creates `dashboard/dist/` with:
- `index.html`
- `assets/index-*.js`
- `assets/index-*.css`
- `favicon.png`

### Step 3: Run

```bash
# From project root
./target/release/rustsocks --config config/rustsocks.toml
```

Backend automatically:
1. Loads static files from `dashboard/dist/`
2. Rewrites `index.html` adding base path script
3. Serves dashboard under `/rustsocks` (or other base_path)

---

## 🚀 Development Mode

### 1. Run Backend

```bash
cargo run -- --config config/rustsocks.toml
```

API available at: `http://127.0.0.1:9090/api/`

### 2. Run Frontend Dev Server

```bash
cd dashboard
npm run dev
```

Dashboard available at: `http://localhost:3000`

**In dev mode:**
- Vite proxy forwards `/api`, `/health`, `/metrics` to backend `:9090`
- Hot reload for React code changes
- Base path is **NOT** used (always `/`)
- Perfect for development

---

## 🌐 Production Deployment

### Scenario 1: Dashboard under Root Path `/`

**Config:**
```toml
[sessions]
base_path = "/"
```

**Build:**
```bash
npm run build
cargo build --release
```

**Access:**
- Dashboard: `http://server:9090/`
- API: `http://server:9090/api/`
- Swagger: `http://server:9090/swagger-ui/`

### Scenario 2: Dashboard under `/rustsocks`

**Config:**
```toml
[sessions]
base_path = "/rustsocks"
```

**Build:**
```bash
npm run build
cargo build --release
```

**Access:**
- Dashboard: `http://server:9090/rustsocks`
- API: `http://server:9090/rustsocks/api/`
- Swagger: `http://server:9090/rustsocks/swagger-ui/`

### Scenario 3: Nginx Reverse Proxy

**Nginx config:**
```nginx
location /socks/ {
    proxy_pass http://127.0.0.1:9090/socks/;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

**RustSocks config:**
```toml
[sessions]
base_path = "/socks"
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
```

**Access:**
- Dashboard: `http://yourserver.com/socks/`
- API: `http://yourserver.com/socks/api/`

---

## 📝 Examples

### Example 1: Rebuild after changing base_path

```bash
# 1. Change config
sed -i 's|base_path = "/"|base_path = "/rustsocks"|' config/rustsocks.toml

# 2. Rebuild frontend (REQUIRED!)
cd dashboard
npm run build

# 3. Rebuild backend (if code changes)
cd ..
cargo build --release

# 4. Restart server
./target/release/rustsocks --config config/rustsocks.toml
```

**⚠️ IMPORTANT:** Changing `base_path` requires **frontend rebuild**!

### Example 2: Test different base paths

```bash
# Test 1: Root path
echo 'base_path = "/"' >> config/test.toml
cd dashboard && npm run build && cd ..
cargo run -- --config config/test.toml
# Check: http://127.0.0.1:9090/

# Test 2: Subpath
echo 'base_path = "/myproxy"' >> config/test.toml
cd dashboard && npm run build && cd ..
cargo run -- --config config/test.toml
# Check: http://127.0.0.1:9090/myproxy
```

### Example 3: Docker deployment with base path

**Dockerfile:**
```dockerfile
FROM rust:1.90-alpine AS rust-builder
WORKDIR /build
RUN apk add --no-cache build-base musl-dev linux-pam-dev openssl-dev pkgconfig
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
RUN cargo build --release --all-features

FROM node:18-alpine AS dashboard-builder
WORKDIR /build/dashboard
COPY dashboard/package*.json ./
RUN npm ci
COPY dashboard/ ./
RUN npm run build

FROM alpine:3.19
RUN apk add --no-cache linux-pam libgcc ca-certificates libssl3
WORKDIR /app
COPY --from=rust-builder /build/target/release/rustsocks .
COPY --from=dashboard-builder /build/dashboard/dist ./dashboard/dist
COPY config/ ./config/

ENV BASE_PATH=/socks
CMD ["./rustsocks", "--config", "config/rustsocks.toml"]
```

---

## 🐛 Troubleshooting

### Problem: Dashboard shows "Cannot GET /rustsocks"

**Cause:** Frontend was not rebuilt after changing `base_path`

**Solution:**
```bash
cd dashboard
rm -rf dist/
npm run build
```

### Problem: Assets (CSS/JS) not loading (404)

**Cause:** Incorrect Vite configuration

**Solution:** Ensure `vite.config.js` has `base: './'`:
```js
export default defineConfig({
  base: './',  // ✅ Must be relative path
  // ...
})
```

### Problem: API calls fail with 404

**Cause:** Frontend using wrong API path

**Solution:** Verify that:
1. `getApiUrl()` is used in all fetch() calls:
   ```js
   // ✅ Correct
   fetch(getApiUrl('/api/sessions/stats'))

   // ❌ Wrong
   fetch('/api/sessions/stats')
   ```
2. Backend correctly injects `window.__RUSTSOCKS_BASE_PATH__`

### Problem: React Router not working (blank page)

**Cause:** Incorrect `basename` in React Router

**Solution:** Ensure `App.jsx` uses `ROUTER_BASENAME`:
```jsx
import { ROUTER_BASENAME } from './lib/basePath'

<BrowserRouter basename={ROUTER_BASENAME}>
  <Routes>
    {/* ... */}
  </Routes>
</BrowserRouter>
```

### Problem: Works on localhost, but not on server

**Cause:** Reverse proxy not forwarding correct headers

**Solution:** Add to nginx/apache:
```nginx
proxy_set_header Host $host;
proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
proxy_set_header X-Forwarded-Proto $scheme;
```

---

## ✅ Pre-Deployment Checklist

- [ ] `base_path` set in `config/rustsocks.toml`
- [ ] Frontend built: `cd dashboard && npm run build`
- [ ] Backend built: `cargo build --release`
- [ ] `dashboard/dist/` exists and contains `index.html`
- [ ] Test in browser:
  - [ ] Dashboard loads
  - [ ] Routing works (page switching)
  - [ ] API calls work (Sessions, ACL, Stats)
  - [ ] Assets (CSS/JS) load correctly
- [ ] Check browser console (F12) - no 404 errors

---

## 📚 Additional Resources

- [CLAUDE.md](../../CLAUDE.md) - Developer guide
- [README.md](../../README.md) - Project overview
- [dashboard/README.md](../../dashboard/README.md) - Dashboard documentation
- [API Documentation](http://127.0.0.1:9090/swagger-ui/) - Swagger UI (when running)

---

**Last Updated:** 2025-11-02
**Version:** 0.9
**Status:** ✅ Production Ready
