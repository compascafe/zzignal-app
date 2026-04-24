# ZZignal App - Setup & Deployment Guide

## Overview

Polymarket BTC Dashboard con backend Rust (axum + WebSocket) y frontend React.

**URLs:**
- Landing: `https://example.com`
- App: `https://app.example.com`
- Blog: `https://blog.example.com`

**Stack:**
- Backend: Rust (axum 0.8, sqlx, polymarket-client-sdk 0.4, alloy 1.8)
- Frontend: React + Vite + Tailwind CSS
- WebSocket: Tiempo real para order book, trades, balance
- Database: PostgreSQL
- Server: AWS EC2 Ubuntu con nginx

---

## Estructura del Proyecto

```
zzignal_app/
├── .github/workflows/deploy.yml    # CI/CD pipeline
├── backend_rust/                      # Código Rust
│   ├── Cargo.toml                    # Dependencias (Rust 1.91)
│   ├── src/
│   │   ├── main.rs                   # Entry point axum server :8080
│   │   ├── api.rs                    # REST + WebSocket handlers
│   │   ├── worker.rs                # Polymarket worker
│   │   ├── db.rs                     # PostgreSQL queries
│   │   └── state.rs                  # AppState compartido
│   └── migrations/                  # SQL migrations
├── frontend_react/                   # Código React
│   ├── src/hooks/useBackend.js        # WebSocket hook (importante!)
│   └── dist/                          # Build output (subido al server)
├── .env                              # Credenciales (NO commitear)
└── zzignal-app.service              # systemd service
```

---

## Deploy desde Cero

### 1. Repositorio GitHub

```bash
cd zzignal_app
git init
git add .
git commit -m "initial commit"
git remote add origin https://github.com/compascafe/zzignal-app.git
git branch -M main
git push -u origin main
```

### 2. Secrets en GitHub Actions

Ve a `Settings → Secrets and variables → Actions`:

| Secret | Valor |
|--------|-------|
| SERVER_IP | IP del servidor (ej: 172.26.0.18) |
| SERVER_SSH_KEY | Clave privada SSH (sin passphrase) |
| ENV_FILE | `base64 -i .env` output |

### 3. Servicio systemd

En el servidor (`/etc/systemd/system/zzignal-app.service`):

```ini
[Unit]
Description=ZZignal Polymarket App
After=network.target postgresql.service

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/home/ubuntu/zzignal-app
EnvironmentFile=/home/ubuntu/zzignal-app/.env
ExecStart=/home/ubuntu/zzignal-app/polymarket-backend
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Habilitar:

```bash
sudo cp zzignal-app.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable zzignal-app
```

### 4. Nginx Config

En `/etc/nginx/sites-enabled/zzignal`:

```nginx
# app.example.com - Frontend + Backend
server {
    listen 443 ssl;
    server_name app.example.com;

    root /var/www/zzignal-app;
    index index.html;

    ssl_certificate /etc/letsencrypt/live/example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/example.com/privkey.pem;
    include /etc/letsencrypt/options-ssl-nginx.conf;

    location / {
        try_files $uri $uri/ /index.html;
    }

    location /api/ {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
    }

    location /ws {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_redirect off;
    }
}
```

Reload nginx:

```bash
sudo nginx -t && sudo systemctl reload nginx
```

### 5. Directorios

```bash
sudo mkdir -p /var/www/zzignal-app
sudo mkdir -p /home/ubuntu/zzignal-app
sudo chown -R ubuntu:ubuntu /var/www/zzignal-app
sudo chown -R ubuntu:zzignal-app /home/ubuntu/zzignal-app
```

### 6. AWS Security Group

Asegurate de abrir:
- 443 (HTTPS)
- 8080 (backend)

---

## Desarrollo Local

### Frontend

```bash
cd frontend_react
npm install
npm run dev
```

**Importante:** La URL del WebSocket está en `src/hooks/useBackend.js`:

```javascript
const WS_URL = import.meta.env.VITE_WS_URL || 'wss://app.example.com/ws'
```

Para desarrollo local, crear `.env`:

```env
VITE_WS_URL=ws://localhost:8080/ws
```

### Backend

```bash
cd backend_rust
cargo build --release
cargo run
```

El backend corre en `http://0.0.0.0:8080`.

---

## Errores Comunes y Soluciones

### Binance 451 Unavailable

```
WARN Binance WS connect falló: HTTP error: 451 Unavailable For Legal Reasons
```

AWS bloquea conexiones a Binance. Soluciones:
1. Usar VPN/proxy
2. Cambiar a API REST en vez de WebSocket
3. Ignorar si no necesitás datos de Binance

### WebSocket "Disconnected" en el browser

El frontend busca `ws://localhost:8080/ws` -> cambiar a `wss://app.example.com/ws`.

Fix en `frontend_react/src/hooks/useBackend.js`.

### Pipeline falla por permisos

Asegurar que los directorios existen y tienen owner correcto:

```bash
sudo chown -R ubuntu:ubuntu /home/ubuntu/zzignal-app
sudo chown -R ubuntu:ubuntu /var/www/zzignal-app
```

### Rust 1.91 required

El pipeline instala Rust 1.91 porque `polymarket-client-sdk` lo requiere.

---

## Pipeline CI/CD (.github/workflows/deploy.yml)

El pipeline hace:

1. **Frontend job:**
   - Instala dependencias npm
   - Build con Vite
   - Deploy a `/var/www/zzignal-app`

2. **Backend job:**
   - Instala Rust 1.91
   - Compila con `cargo build --release`
   - Deploy a `/home/ubuntu/zzignal-app`
   - Extrae tar.gz
   - Crea .env desde secrets
   - Reinicia servicio systemd

Optimizaciones incluidas:
- Cache de Cargo y npm
- Build solo si cambió el código relevante
- jobs paralelos

---

## Comandos Útiles

```bash
# Ver logs del backend
sudo journalctl -u zzignal-app -n 50 --no-pager

# Reiniciar servicio
sudo systemctl restart zzignal-app

# Ver estado
sudo systemctl status zzignal-app

# Ver puertos escuchando
sudo ss -tlnp | grep 8080

# Probar API
curl https://app.example.com/api/status

# Probar WebSocket
curl -i -N -H "Upgrade: websocket" -H "Connection: upgrade" https://app.example.com/ws
```

---

## Estado (Abril 2026)

- [x] Backend compilando y corriendo
- [x] WebSocket funcionando
- [x] Frontend deployado
- [x] CI/CD optimizado
- [ ] Binance streaming (bloqueado por AWS)
- [ ] Fallback a otro provider de precio BTC (opcional)