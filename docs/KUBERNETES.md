# Kubernetes (Helm)

Rampart ships a production-grade Helm chart, published as an OCI artifact to
GitHub Container Registry. Rampart is a **stateless** Deployment — bring your
own Postgres.

> Full chart reference (every value, more examples):
> [`charts/rampart/README.md`](https://github.com/pen-pal/rampart/blob/main/charts/rampart/README.md)
> · [`values.yaml`](https://github.com/pen-pal/rampart/blob/main/charts/rampart/values.yaml)

## Install

```bash
# 1. Put your Postgres DSN in a Secret (key: DATABASE_URL)
kubectl create secret generic rampart-db \
  --from-literal=DATABASE_URL='postgres://user:pass@pg-host:5432/rampart'

# 2. Install from GHCR (OCI)
helm install rampart oci://ghcr.io/pen-pal/charts/rampart \
  --version 0.2.0 \
  --set externalDatabase.existingSecret=rampart-db
```

Migrations run automatically on first boot; the chart's `startupProbe` waits
them out before liveness begins. The chart targets Rampart **v0.8.0**.

## Database

All state lives in Postgres — use a managed/operator-run instance (RDS, Cloud
SQL, Neon, CloudNativePG, …).

| How | Set |
| :-- | :-- |
| Existing Secret (recommended) | `externalDatabase.existingSecret=<name>` (key `DATABASE_URL`) |
| Inline DSN (eval) | `externalDatabase.url=postgres://…` |
| Throwaway embedded PG (eval) | `postgres.embedded=true` |

## Production capabilities

All off by default, all documented in `values.yaml`:

- **HPA** — `autoscaling.enabled` (the API tier is stateless; replicas are safe)
- **PodDisruptionBudget** — `podDisruptionBudget.enabled`
- **Topology spread** — `topologySpreadConstraints`
- **Ingress** (+ cert-manager) — `ingress.enabled`
- **Service mesh** — Istio Gateway + VirtualService via `istio.enabled` (for clusters that don't use a Kubernetes Ingress)
- **NetworkPolicy** — `networkPolicy.enabled`
- **Prometheus ServiceMonitor** — `serviceMonitor.enabled` (scrapes `/metrics`)
- **Persistence + extra volumes** — `persistence.enabled`, `extraVolumes`
- Non-root + read-only-rootfs security context and startup/liveness/readiness probes are **on** by default.

### Example: HA + mesh + Prometheus

```yaml
autoscaling: { enabled: true, minReplicas: 2, maxReplicas: 6 }
podDisruptionBudget: { enabled: true, minAvailable: 1 }
ingress: { enabled: false }
istio:
  enabled: true
  gateway:
    selector: { istio: ingressgateway }
    hosts: [rampart.example.com]
    tls: { enabled: true, mode: SIMPLE, credentialName: rampart-tls }
  virtualService: { hosts: [rampart.example.com] }
serviceMonitor:
  enabled: true
  labels: { release: kube-prometheus-stack }
```

## NetworkPolicy — read before enabling egress

!!! warning "Egress is off by default on purpose"
    Rampart is a **monitor**: it makes outbound probes to arbitrary
    user-defined targets (HTTP, databases, TCP, …) and posts to arbitrary
    notification webhooks, plus it needs DNS and Postgres. A restrictive
    **egress** policy will silently break monitoring and alerting. Lock down
    **ingress** freely; only constrain egress if you enumerate every
    destination (DNS + Postgres + each probe/webhook target).

```yaml
networkPolicy:
  enabled: true
  ingress:
    enabled: true
    from:
      - namespaceSelector:
          matchLabels: { kubernetes.io/metadata.name: ingress-nginx }
```

## Publishing

CI (`helm.yml`) lints the chart on every change and, on a `v*` / `chart-v*`
tag, packages and pushes it to `oci://ghcr.io/pen-pal/charts/rampart`. The chart
version comes from `charts/rampart/Chart.yaml`.
