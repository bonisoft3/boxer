# Boxer

Postgres WAL consumer with **at-least-once** HTTP delivery. A tiny Rust binary (~5MB) that reads logical replication and POSTs change events to any HTTP endpoint, advancing the replication slot only after successful delivery.

## Why Boxer?

- **True at-least-once**: The replication slot's `confirmed_flush_lsn` advances only after your endpoint returns 2xx. If boxer crashes, Postgres replays from the last confirmed position.
- **Tiny**: ~5MB statically linked binary, ~23MB Docker image. Compare with Debezium (~800MB) or Sequin (~215MB).
- **Zero dependencies**: No Kafka, no Redis, no JVM. Just Postgres and an HTTP endpoint.
- **Scale-to-zero friendly**: Designed for Cloud Run and Knative. Sub-second cold start.

## Install

**Docker:**
```bash
docker pull ghcr.io/bonisoft3/boxer:latest
```

**Binary download:**
```bash
curl -fsSL -o boxer \
  "https://github.com/bonisoft3/boxer/releases/latest/download/boxer-linux-x64"
chmod +x boxer
```

**From source:**
```bash
cargo install --git https://github.com/bonisoft3/boxer
```

## Quick Start

```bash
# 1. Create a publication on your Postgres database
psql $DATABASE_URL -c "CREATE PUBLICATION boxer_pub FOR TABLE my_table;"

# 2. Run boxer
export DATABASE_URL="postgresql://user:pass@host:5432/mydb?sslmode=require"
export BOXER_DELIVERY_URL="http://localhost:3000/api/webhook"
boxer
```

Boxer creates the replication slot on first connect and starts streaming changes immediately.

## Configuration

All configuration is via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | *(required)* | Postgres connection string |
| `BOXER_DELIVERY_URL` | *(required)* | HTTP endpoint for event delivery |
| `BOXER_SLOT` | `boxer_slot` | Logical replication slot name |
| `BOXER_PUBLICATION` | `boxer_pub` | Postgres publication name |
| `BOXER_TABLE` | `trackrequest` | Table to watch for INSERTs |
| `BOXER_HEALTH_PORT` | `8080` | Health check HTTP port |
| `RUST_LOG` | `info` | Log level (`debug`, `info`, `warn`, `error`) |

## How It Works

```
Postgres WAL ──→ boxer ──→ HTTP POST ──→ your endpoint
                  │                         │
                  │    only on 2xx ←─────────┘
                  ▼
            advance LSN
```

1. Boxer connects to a Postgres [logical replication slot](https://www.postgresql.org/docs/current/logicaldecoding.html) and receives WAL events via the `pgoutput` protocol.
2. On INSERT for the configured table, boxer extracts column values into a JSON object and POSTs it to `BOXER_DELIVERY_URL`.
3. **Only after a 2xx response** does boxer advance the slot's `confirmed_flush_lsn`.
4. On delivery failure, boxer retries with exponential backoff (2s → 4s → ... → 60s max).
5. After 10 consecutive failures, boxer crashes. Your orchestrator (Cloud Run, K8s, systemd) restarts it, and Postgres replays from the last confirmed LSN.

### Delivery format

```json
{
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "Example Item",
    "createdAt": "2024-01-15T10:30:00+00:00"
  }
}
```

### Health endpoint

```bash
curl http://localhost:8080/health
# {"status":"ok"}
```

## Architecture

Boxer is designed as infrastructure that knows nothing about your application. It only understands Postgres WAL and HTTP.

**Local development** — boxer delivers directly to your API:
```
Postgres → boxer → http://localhost:3000/api/webhook
```

**Production** — boxer delivers through a Dapr sidecar for retry policies, circuit breaking, and observability:
```
Postgres → boxer → http://localhost:3500/v1.0/publish/eventbus/my-topic → Dapr → your API
```

Same binary, one env var change.

## Prerequisites

Your Postgres instance needs `wal_level = logical`. Most managed providers (Neon, Supabase, RDS, Cloud SQL) support this — check your provider's docs.

```sql
-- Create a publication for the tables you want to stream
CREATE PUBLICATION boxer_pub FOR TABLE my_table;

-- Boxer creates the replication slot automatically on first connect.
-- To create it manually:
SELECT pg_create_logical_replication_slot('boxer_slot', 'pgoutput');
```

## Development

Boxer uses [sayt](https://github.com/bonisoft3/sayt) for development lifecycle:

```bash
sayt setup       # Install Rust toolchain via mise
sayt build       # cargo build
sayt test        # cargo test
sayt lint        # cargo clippy + cargo fmt --check
sayt launch      # cargo run (with env vars)
```

Or directly with cargo:

```bash
cargo build
cargo test
cargo clippy -- -D warnings
```

## Contributing

Contributions are welcome. Please open an issue first to discuss what you'd like to change.

```bash
git clone https://github.com/bonisoft3/boxer
cd boxer
sayt setup && sayt test
```

## License

[LGPL-3.0](LICENSE)
