# Phase 1 — Deployment Guide

## Local Development (WSL2 / Linux / macOS)

### Prerequisites
- Docker Engine 24+
- Docker Compose v2
- Git

### Setup

```bash
# Clone
git clone https://github.com/pruthvirajdgit/oneclick-ai.git
cd oneclick-ai

# Configure
cp .env.example .env
# Edit .env — add GROQ_API_KEY, OPENROUTER_API_KEY

# Build agent image
docker build -t oneclick-agent:latest agent-runtime/

# Build and start
docker compose up -d --build

# Check status
docker compose ps

# View logs
docker compose logs -f backend
```

### Access
- Frontend: http://localhost:3000
- Swagger UI: http://localhost:8080/swagger-ui/
- Backend API: http://localhost:8080/api/
- PostgreSQL: localhost:5432 (user: oneclick, db: oneclick)
- Redis: localhost:6379

### Development workflow
```bash
# Rebuild backend after code changes
docker compose up -d --build backend

# Rebuild frontend after code changes
docker compose up -d --build frontend

# Build agent image after changes to agent-runtime/
docker build -t oneclick-agent:latest agent-runtime/

# Run database migrations
docker compose exec backend ./oneclick-backend migrate

# View agent container logs
docker logs agent-{user-id}

# Connect to database
docker compose exec postgres psql -U oneclick
```

---

## Production (Azure VM)

### Server Requirements
- **Minimum**: 4 vCPU, 8GB RAM, 80GB SSD (Azure B4ms ~$120/month)
- **Recommended**: 4 vCPU, 16GB RAM, 128GB SSD
- OS: Ubuntu 24.04 LTS
- Docker Engine installed

### DNS Setup
Point your domain to the server IP:
```
A   api.oneclick.ai    →  <server-ip>
A   oneclick.ai        →  <server-ip>  (for future frontend)
```

### Deploy

```bash
# SSH to server
ssh user@<server-ip>

# Install Docker (if not present)
curl -fsSL https://get.docker.com | sh

# Clone and configure
git clone https://github.com/pruthvirajdgit/oneclick-ai.git
cd oneclick-ai
cp .env.example .env
# Edit .env with production values

# Start with production config
docker compose -f docker-compose.yml -f docker-compose.prod.yml up -d

# Verify
curl https://api.oneclick.ai/health
```

### Production .env

```bash
# Database (use a strong password)
DATABASE_URL=postgres://oneclick:$(openssl rand -hex 32)@postgres:5432/oneclick

# Auth (generate a random secret)
JWT_SECRET=$(openssl rand -hex 32)

# Domain
DOMAIN=api.oneclick.ai
ACME_EMAIL=admin@oneclick.ai

# LLM
GROQ_API_KEY=gsk_...
OPENROUTER_API_KEY=sk-or-v1-...

# Limits
MAX_AGENTS=100
FREE_TIER_DAILY_LIMIT=50
IDLE_TIMEOUT_MINUTES=15
```

### docker-compose.prod.yml (overrides)

```yaml
services:
  frontend:
    restart: always

  backend:
    restart: always

  postgres:
    restart: always

  redis:
    restart: always
```

> Note: Traefik was removed in Phase 2. For production TLS, add a reverse proxy (Caddy, nginx, or cloud load balancer) in front of the frontend container.

### Backups

```bash
# Database backup (daily cron)
0 3 * * * docker compose exec -T postgres pg_dump -U oneclick oneclick | gzip > /backups/db-$(date +\%Y\%m\%d).sql.gz

# Agent data backup
0 4 * * * tar czf /backups/agents-$(date +\%Y\%m\%d).tar.gz /data/agents/
```

### Monitoring

```bash
# Health check
curl https://api.oneclick.ai/health

# Agent count
curl -H "Authorization: Bearer <admin-token>" https://api.oneclick.ai/api/admin/stats

# Resource usage
docker stats --no-stream

# Logs
docker compose logs -f --tail 100 backend
```

---

## CI/CD (GitHub Actions)

```yaml
# .github/workflows/deploy.yml
name: Deploy
on:
  push:
    branches: [main]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Build Docker image
        run: docker build -t ghcr.io/pruthvirajdgit/oneclick-backend:latest ./backend

      - name: Push to registry
        run: |
          echo "${{ secrets.GHCR_TOKEN }}" | docker login ghcr.io -u pruthvirajdgit --password-stdin
          docker push ghcr.io/pruthvirajdgit/oneclick-backend:latest

      - name: Deploy to server
        uses: appleboy/ssh-action@v1
        with:
          host: ${{ secrets.SERVER_IP }}
          username: deploy
          key: ${{ secrets.SSH_KEY }}
          script: |
            cd /opt/oneclick-ai
            docker compose pull backend
            docker compose up -d backend
```
