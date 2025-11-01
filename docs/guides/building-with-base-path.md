# Building RustSocks with URL Base Path

Kompletny przewodnik budowania i konfiguracji RustSocks z prefixem URL (base path) dla API i dashboardu.

## 📋 Spis treści

- [Jak to działa](#jak-to-działa)
- [Konfiguracja](#konfiguracja)
- [Budowanie aplikacji](#budowanie-aplikacji)
- [Development mode](#development-mode)
- [Production deployment](#production-deployment)
- [Przykłady](#przykłady)

---

## 🔧 Jak to działa

RustSocks wspiera deployment pod dowolną ścieżką URL dzięki inteligentnej integracji frontend-backend:

### Backend (Rust)

1. **Config**: `sessions.base_path` określa prefix URL (np. `/rustsocks`, `/proxy`, lub `/`)
2. **Router nesting**: Jeśli `base_path != "/"`, cała aplikacja (API + dashboard) jest montowana pod tym prefixem
3. **HTML rewriting**: Automatyczne przepisanie `index.html`:
   - Dodanie `<script>window.__RUSTSOCKS_BASE_PATH__ = '/rustsocks';</script>` przed `</head>`
   - Zmiana ścieżek do assets: `./assets/` → `/rustsocks/assets/`
4. **Static files**: Serwowanie `dashboard/dist/` z automatycznym fallback dla SPA routing

### Frontend (React)

1. **Auto-detection**: `src/lib/basePath.js` automatycznie wykrywa base path:
   - Z `window.__RUSTSOCKS_BASE_PATH__` (injectowane przez backend)
   - Lub z lokalizacji skryptów (analizuje URL `/assets/index-*.js`)
   - Lub z `window.location.pathname` (fallback)
2. **React Router**: `<BrowserRouter basename={ROUTER_BASENAME}>` dla routing
3. **API calls**: `getApiUrl(path)` dodaje prefix do wszystkich fetch()
4. **Vite build**: Buduje z `base: './'` (relative paths)

---

## ⚙️ Konfiguracja

### 1. Backend Config (`config/rustsocks.toml`)

```toml
[sessions]
stats_api_enabled = true
dashboard_enabled = true
swagger_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090

# Base URL path prefix
base_path = "/rustsocks"  # Opcje: "/", "/rustsocks", "/proxy", etc.
```

**Ważne:**
- `base_path = "/"` - dashboard pod `http://host/`
- `base_path = "/rustsocks"` - dashboard pod `http://host/rustsocks`
- `base_path = "/rustsocks/"` - trailing slash jest automatycznie usuwany

### 2. Frontend Config (`dashboard/vite.config.js`)

**Nie wymaga zmian!** Vite jest skonfigurowany z `base: './'` (relative paths), co działa z każdym base path.

```js
export default defineConfig({
  base: './',  // ✅ MUSI być './' dla automatycznego działania
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

## 🏗️ Budowanie aplikacji

### Krok 1: Build Backend (Rust)

```bash
# Development build
cargo build

# Production build (optimized)
cargo build --release
```

### Krok 2: Build Frontend (React Dashboard)

```bash
cd dashboard

# Install dependencies (first time only)
npm install

# Production build
npm run build
```

To tworzy `dashboard/dist/` z:
- `index.html`
- `assets/index-*.js`
- `assets/index-*.css`
- `vite.svg`

### Krok 3: Uruchomienie

```bash
# Z głównego katalogu projektu
./target/release/rustsocks --config config/rustsocks.toml
```

Backend automatycznie:
1. Ładuje static files z `dashboard/dist/`
2. Przepisuje `index.html` dodając base path script
3. Serwuje dashboard pod `/rustsocks` (lub innym base_path)

---

## 🚀 Development Mode

### 1. Uruchom Backend

```bash
cargo run -- --config config/rustsocks.toml
```

API dostępne na: `http://127.0.0.1:9090/api/`

### 2. Uruchom Frontend Dev Server

```bash
cd dashboard
npm run dev
```

Dashboard dostępne na: `http://localhost:3000`

**W dev mode:**
- Vite proxy przekierowuje `/api`, `/health`, `/metrics` na backend `:9090`
- Hot reload dla zmian w kodzie React
- Base path **NIE** jest używany (zawsze `/`)
- Idealne do development

---

## 🌐 Production Deployment

### Scenariusz 1: Dashboard pod root path `/`

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

**Dostęp:**
- Dashboard: `http://server:9090/`
- API: `http://server:9090/api/`
- Swagger: `http://server:9090/swagger-ui/`

### Scenariusz 2: Dashboard pod `/rustsocks`

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

**Dostęp:**
- Dashboard: `http://server:9090/rustsocks`
- API: `http://server:9090/rustsocks/api/`
- Swagger: `http://server:9090/rustsocks/swagger-ui/`

### Scenariusz 3: Nginx Reverse Proxy

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

**Dostęp:**
- Dashboard: `http://yourserver.com/socks/`
- API: `http://yourserver.com/socks/api/`

---

## 📝 Przykłady

### Przykład 1: Rebuild po zmianie base_path

```bash
# 1. Zmień config
sed -i 's|base_path = "/"|base_path = "/rustsocks"|' config/rustsocks.toml

# 2. Rebuild frontend (WYMAGANE!)
cd dashboard
npm run build

# 3. Rebuild backend (jeśli były zmiany w kodzie)
cd ..
cargo build --release

# 4. Restart serwera
./target/release/rustsocks --config config/rustsocks.toml
```

**⚠️ WAŻNE:** Zmiana `base_path` wymaga **rebuildu frontend**!

### Przykład 2: Test różnych base paths

```bash
# Test 1: Root path
echo 'base_path = "/"' >> config/test.toml
cd dashboard && npm run build && cd ..
cargo run -- --config config/test.toml
# Sprawdź: http://127.0.0.1:9090/

# Test 2: Subpath
echo 'base_path = "/myproxy"' >> config/test.toml
cd dashboard && npm run build && cd ..
cargo run -- --config config/test.toml
# Sprawdź: http://127.0.0.1:9090/myproxy
```

### Przykład 3: Docker deployment z base path

**Dockerfile:**
```dockerfile
FROM rust:1.70 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM node:18 AS frontend
WORKDIR /app/dashboard
COPY dashboard/package*.json ./
RUN npm install
COPY dashboard/ ./
RUN npm run build

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/rustsocks .
COPY --from=frontend /app/dashboard/dist ./dashboard/dist
COPY config/ ./config/

ENV BASE_PATH=/socks
CMD ["./rustsocks", "--config", "config/rustsocks.toml"]
```

---

## 🐛 Troubleshooting

### Problem: Dashboard pokazuje "Cannot GET /rustsocks"

**Przyczyna:** Frontend nie został przebudowany po zmianie `base_path`

**Rozwiązanie:**
```bash
cd dashboard
rm -rf dist/
npm run build
```

### Problem: Assets (CSS/JS) nie ładują się (404)

**Przyczyna:** Niepoprawna konfiguracja Vite

**Rozwiązanie:** Upewnij się że `vite.config.js` ma `base: './'`:
```js
export default defineConfig({
  base: './',  // ✅ Musi być relative path
  // ...
})
```

### Problem: API calls fail with 404

**Przyczyna:** Frontend używa złej ścieżki do API

**Rozwiązanie:** Sprawdź czy:
1. `getApiUrl()` jest używany we wszystkich fetch():
   ```js
   // ✅ Correct
   fetch(getApiUrl('/api/sessions/stats'))

   // ❌ Wrong
   fetch('/api/sessions/stats')
   ```
2. Backend prawidłowo injectuje `window.__RUSTSOCKS_BASE_PATH__`

### Problem: React Router nie działa (blank page)

**Przyczyna:** Niepoprawny `basename` w React Router

**Rozwiązanie:** Upewnij się że `App.jsx` używa `ROUTER_BASENAME`:
```jsx
import { ROUTER_BASENAME } from './lib/basePath'

<BrowserRouter basename={ROUTER_BASENAME}>
  <Routes>
    {/* ... */}
  </Routes>
</BrowserRouter>
```

### Problem: Działa na localhost, ale nie na serwerze

**Przyczyna:** Reverse proxy nie przekazuje prawidłowych nagłówków

**Rozwiązanie:** Dodaj do nginx/apache:
```nginx
proxy_set_header Host $host;
proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
proxy_set_header X-Forwarded-Proto $scheme;
```

---

## ✅ Checklist przed deploymentem

- [ ] `base_path` ustawiony w `config/rustsocks.toml`
- [ ] Frontend zbudowany: `cd dashboard && npm run build`
- [ ] Backend zbudowany: `cargo build --release`
- [ ] `dashboard/dist/` istnieje i zawiera `index.html`
- [ ] Test w przeglądarce:
  - [ ] Dashboard się ładuje
  - [ ] Routing działa (przełączanie stron)
  - [ ] API calls działają (Sessions, ACL, Stats)
  - [ ] Assets (CSS/JS) ładują się poprawnie
- [ ] Sprawdź browser console (F12) - brak błędów 404

---

## 📚 Dodatkowe zasoby

- [CLAUDE.md](../../CLAUDE.md) - Developer guide
- [README.md](../../README.md) - Project overview
- [dashboard/README.md](../../dashboard/README.md) - Dashboard docs
- [API Documentation](http://127.0.0.1:9090/swagger-ui/) - Swagger UI (when running)

---

**Ostatnia aktualizacja:** 2025-10-29
**Wersja:** 1.0
**Status:** ✅ Production Ready
