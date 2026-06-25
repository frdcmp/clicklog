# monitoring

**Shared observability stack — Prometheus + Grafana.**

A single Prometheus that **scrapes** each project's `/metrics` endpoint over the
private overlay, plus Grafana with provisioned dashboards. Kept separate from
any app's compose so it survives app rebuilds/downs and can watch many services
at once.

Metrics are **pulled**, not pushed: each backend just exposes a `/metrics`
endpoint; Prometheus reaches out on a schedule.

## Where it runs

Host-agnostic — deploy wherever. Set the published URL/ports in `.env`:

| | |
|--|--|
| **Grafana** | host port `${GRAFANA_EXT_PORT}` (default `3001`) → container `3000` |
| **Prometheus** | host port `${PROMETHEUS_EXT_PORT}` (default `9090`) → container `9090` |
| **Root URL** | `GRAFANA_ROOT_URL` — how Grafana is reached (e.g. `http://172.25.212.41:3001`) |

```
  app_one /metrics ─┐
  app_two /metrics ─┤── scrape (the private overlay 172.25.x) ──▶  Prometheus ──▶ Grafana
  node_exporter :9100   ─┘
```

## Quick start

```bash
cp .env.example .env          # set GRAFANA_ADMIN_PASSWORD + GRAFANA_ROOT_URL
docker compose up -d
docker compose logs -f
```

Grafana comes up with the Prometheus datasource and dashboards already
provisioned (`grafana/provisioning/`, `grafana/dashboards/`). Log in as `admin`
with `GRAFANA_ADMIN_PASSWORD`.

## Configuration (`.env`)

| Var | Meaning |
|-----|---------|
| `GRAFANA_ADMIN_PASSWORD` | Grafana `admin` login. |
| `GRAFANA_ROOT_URL` | Public URL Grafana advertises (links/redirects). |
| `GRAFANA_EXT_PORT` / `PROMETHEUS_EXT_PORT` | Published host ports. |

## Adding a target

Edit [`prometheus/prometheus.yml`](prometheus/prometheus.yml) — add a
`static_configs` target for the new service's `/metrics` endpoint (use a
`service:` label matching its tenant name), then reload:

```bash
# hot reload (web-lifecycle is enabled), or just: docker compose restart prometheus
curl -X POST http://127.0.0.1:${PROMETHEUS_EXT_PORT:-9090}/-/reload
```

Drop a dashboard JSON into `grafana/dashboards/` (it's auto-provisioned) to
visualise it. Hit each node **directly** (not a round-robin edge) so metrics are
attributed to the right node.

## Operations

```bash
docker compose ps
docker compose logs -f prometheus
docker compose logs -f grafana
docker compose down                 # stop (data persists in named volumes)
docker compose down -v              # full wipe (drops prometheus_data + grafana_data)
```
