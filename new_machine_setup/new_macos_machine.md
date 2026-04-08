# macOS Machine Setup

Complete guide to set up the OneClick.ai development environment on macOS.

> **Note:** Firecracker requires Linux with KVM and is **not supported on macOS**. This guide covers Docker runtime only. For Firecracker development, use a Linux VM or cloud instance — see `new_linux_machine.md`.

## Prerequisites

- macOS 13 (Ventura) or later
- Apple Silicon (M1/M2/M3/M4) or Intel Mac
- At least 8 GB RAM, 20 GB free disk space
- [Homebrew](https://brew.sh) installed

If you don't have Homebrew:
```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

## 1. Clone the Repository

```bash
git clone https://github.com/pruthvirajdgit/oneclick-ai.git
cd oneclick-ai
```

## 2. Install Rust Toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup default stable
rustc --version   # 1.80+ required
```

## 3. Install Docker Desktop

Download and install from [docker.com/products/docker-desktop](https://www.docker.com/products/docker-desktop/).

Or via Homebrew:
```bash
brew install --cask docker
```

After installation:
1. Open Docker Desktop from Applications
2. Wait for the Docker engine to start (whale icon in menu bar stops animating)
3. Verify:
```bash
docker --version
docker compose version   # v2.20+ required
```

### Recommended Docker Desktop Settings

- **Resources → Memory:** At least 4 GB (6 GB recommended)
- **Resources → CPU:** At least 4 cores
- **Resources → Disk:** At least 20 GB

## 4. Install Node.js 22 LTS (optional — only if running frontend in dev mode)

```bash
brew install node@22
```

Or via nvm:
```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.0/install.sh | bash
source ~/.zshrc
nvm install 22
nvm use 22
node --version
```

## 5. Install Build Dependencies

macOS needs a few extra tools for compiling native Rust crates:

```bash
# Xcode Command Line Tools (if not already installed)
xcode-select --install

# pkg-config and OpenSSL (needed by some Rust crates)
brew install pkg-config openssl

# Add OpenSSL to environment (add to ~/.zshrc for persistence)
export PKG_CONFIG_PATH="$(brew --prefix openssl)/lib/pkgconfig"
```

Add this to your `~/.zshrc` to persist:
```bash
echo 'export PKG_CONFIG_PATH="$(brew --prefix openssl)/lib/pkgconfig"' >> ~/.zshrc
```

## 6. Start Infrastructure Services

```bash
cd oneclick-ai
docker compose up -d postgres redis
```

Wait for healthy status:
```bash
docker compose ps   # postgres and redis should show "healthy"
```

## 7. Build the Agent Docker Image

```bash
cd agent-runtime
docker build -t oneclick-agent:latest .
cd ..
```

> **Note for Apple Silicon (M1/M2/M3/M4):** If the agent image is built for `linux/amd64`, you may need to build with platform flag:
> ```bash
> docker build --platform linux/amd64 -t oneclick-agent:latest .
> ```
> Or if the Dockerfile supports multi-arch, the default `linux/arm64` build should work.

## 8. Configure Environment

```bash
cp .env.example .env   # or create manually
```

Edit `.env` with at minimum:
```env
DATABASE_URL=postgres://oneclick:devpassword@localhost:5432/oneclick
REDIS_URL=redis://localhost:6379
JWT_SECRET=your-dev-jwt-secret-change-in-prod
INTERNAL_SECRET=your-dev-internal-secret-change-in-prod

# LLM provider (at least one required)
GROQ_API_KEY=gsk_...
# OPENROUTER_API_KEY=sk-or-...

# Runtime selection (Docker only on macOS)
AGENT_RUNTIME=docker

# Docker runtime settings
AGENT_IMAGE=oneclick-agent:latest
AGENT_MEMORY_LIMIT=512m
AGENT_CPU_LIMIT=0.5
DOCKER_NETWORK=oneclick-net

# Limits
MAX_AGENTS=100
IDLE_TIMEOUT_MINUTES=15
CORS_ALLOWED_ORIGINS=http://localhost:3000
```

## 9. Build and Run the Backend

```bash
cd backend
cargo build --release
./target/release/oneclick-backend
# Listening on http://0.0.0.0:8080
# Swagger UI at http://localhost:8080/swagger-ui/
```

> **First build** may take 5–10 minutes as it compiles all dependencies. Subsequent builds are much faster.

## 10. Start the Frontend (optional)

```bash
# Option A: Docker (recommended)
docker compose up -d frontend

# Option B: Dev mode
cd frontend
npm install
npm run dev   # http://localhost:5173
```

## 11. Verify

```bash
curl http://localhost:8080/health   # Should return 200
```

Open [http://localhost:3000](http://localhost:3000) in your browser to access the UI.

---

## macOS-Specific Notes

### Docker Networking

On macOS, Docker runs inside a Linux VM (via Docker Desktop). Key differences from Linux:

- **No `host` network mode** — containers can't directly access the host network
- **`host.docker.internal`** — use this hostname from inside containers to reach the host machine
- **Container-to-host:** Containers reach the backend via the Docker bridge network, not `localhost`

### File System Performance

Docker on macOS uses VirtioFS (or gRPC FUSE) for volume mounts. For best performance:

- Avoid mounting the entire repo into containers
- Use named volumes for database data (already configured in `docker-compose.yml`)
- If builds are slow, ensure VirtioFS is enabled in Docker Desktop → Settings → General

### Port Conflicts

If ports 5432 (postgres), 6379 (redis), or 8080 (backend) are already in use:

```bash
# Check what's using a port
lsof -i :5432

# Override ports in .env
POSTGRES_PORT=5433
REDIS_PORT=6380
```

---

## Cleanup

```bash
# Stop all services
docker compose down

# Stop and remove volumes (resets database)
docker compose down -v

# Remove agent containers
docker ps -a | grep oneclick-agent | awk '{print $1}' | xargs docker rm -f

# Clean up Docker resources
docker system prune -f
```

---

## Differences from Linux Setup

| Feature | Linux | macOS |
|---------|-------|-------|
| Docker | Docker Engine (native) | Docker Desktop (VM) |
| Firecracker | ✅ Supported (KVM) | ❌ Not supported |
| Agent Runtime | Docker or Firecracker | Docker only |
| Snapshot wake | ~116ms (Firecracker) | N/A |
| Docker networking | Native host access | Via `host.docker.internal` |
| Build tools | `apt install build-essential` | `xcode-select --install` |
| Package manager | apt | Homebrew |

For Firecracker development, use a Linux machine or cloud VM (see `new_linux_machine.md`).
