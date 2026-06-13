# Rampart Helm chart

Deploy [Rampart](https://github.com/pen-pal/rampart) — self-hosted uptime
monitoring + observability — to Kubernetes. One stateless Deployment backed by
your Postgres.

## TL;DR

```bash
# 1. Put your Postgres DSN in a secret (key: DATABASE_URL)
kubectl create secret generic rampart-db \
  --from-literal=DATABASE_URL='postgres://user:pass@pg-host:5432/rampart'

# 2. Install from the OCI registry
helm install rampart oci://ghcr.io/pen-pal/charts/rampart \
  --version 0.2.0 \
  --set externalDatabase.existingSecret=rampart-db
```

The chart targets Rampart **v0.8.0** (`appVersion`). Browse versions on the
[GHCR package page](https://github.com/pen-pal/rampart/pkgs/container/charts%2Frampart).

## Database

Rampart is **stateless** — all data lives in Postgres — so you bring your own
(RDS, Cloud SQL, Neon, CloudNativePG, Bitnami, …). Two ways to supply the DSN:

| Approach | Values |
| :--- | :--- |
| **Existing secret** (recommended) | `externalDatabase.existingSecret=<name>` (key `DATABASE_URL`, override with `externalDatabase.existingSecretKey`) |
| **Inline** (eval only — ends up in `values`) | `externalDatabase.url=postgres://…` |

A tiny **embedded Postgres** StatefulSet is available for local evaluation only
(`postgres.embedded=true`); never use it for anything you care about.

Migrations run automatically on pod boot — no separate Job. The `startupProbe`
gives the first pod time to migrate before liveness kicks in.

## What's included

| Capability | Value | Default |
| :--- | :--- | :--- |
| Horizontal Pod Autoscaler | `autoscaling.enabled` | off |
| Pod Disruption Budget | `podDisruptionBudget.enabled` | off |
| Topology spread | `topologySpreadConstraints` | `[]` |
| Kubernetes Ingress (+ cert-manager) | `ingress.enabled` | off |
| Istio Gateway + VirtualService (service mesh) | `istio.enabled` | off |
| NetworkPolicy | `networkPolicy.enabled` | off |
| Prometheus ServiceMonitor | `serviceMonitor.enabled` | off |
| Persistence (optional PVC) + extra volumes | `persistence.enabled`, `extraVolumes` | off |
| Non-root + read-only-rootfs security context | `podSecurityContext`, `securityContext` | on |
| Startup/liveness/readiness probes | `*Probe` | on |
| Config-change auto-reload (Stakater) | `configReloader.enabled` | off |

## Common configurations

### Ingress (NGINX / ALB) with cert-manager

```yaml
ingress:
  enabled: true
  className: nginx
  certManager:
    enabled: true
    clusterIssuer: letsencrypt-prod
  hosts:
    - host: rampart.example.com
      paths: [{ path: /, pathType: Prefix }]
```

### Service mesh (Istio) — no Ingress controller

```yaml
ingress: { enabled: false }
istio:
  enabled: true
  gateway:
    selector: { istio: ingressgateway }
    hosts: [rampart.example.com]
    tls: { enabled: true, mode: SIMPLE, credentialName: rampart-tls }
  virtualService:
    hosts: [rampart.example.com]
```

### High availability

```yaml
autoscaling: { enabled: true, minReplicas: 2, maxReplicas: 6 }
podDisruptionBudget: { enabled: true, minAvailable: 1 }
topologySpreadConstraints:
  - maxSkew: 1
    topologyKey: topology.kubernetes.io/zone
    whenUnsatisfiable: ScheduleAnyway
```

The API tier is stateless and the scheduler/notifier are DB-coordinated, so
running multiple replicas is safe.

### Prometheus

```yaml
serviceMonitor:
  enabled: true
  labels: { release: kube-prometheus-stack }  # match your Prometheus selector
```

Scrapes Rampart's built-in `/metrics` endpoint.

### NetworkPolicy

> ⚠️ **Egress is off by default on purpose.** Rampart is a *monitor* — it makes
> outbound probes to arbitrary user-defined targets (HTTP, DBs, TCP, …) and
> posts to arbitrary notification webhooks, and needs DNS + Postgres. A
> restrictive egress policy will silently break monitoring/alerting.

Lock down **ingress** safely (who can reach the dashboard/ingest):

```yaml
networkPolicy:
  enabled: true
  ingress:
    enabled: true
    from:
      - namespaceSelector:
          matchLabels: { kubernetes.io/metadata.name: ingress-nginx }
```

If you must constrain egress, enumerate **every** destination (DNS, Postgres,
and each probe/webhook target):

```yaml
networkPolicy:
  egress:
    enabled: true
    to:
      - to: [{ namespaceSelector: {} }]   # DNS (kube-dns) — required
        ports: [{ port: 53, protocol: UDP }]
      - to: [{ ipBlock: { cidr: 10.0.0.0/8 } }]  # Postgres + internal targets
      # ...plus every external probe/webhook endpoint you monitor.
```

## Values

See [`values.yaml`](./values.yaml) — every key is documented inline. The most
common:

| Key | Description | Default |
| :--- | :--- | :--- |
| `image.repository` / `image.tag` | Image (tag defaults to `appVersion`) | `ghcr.io/pen-pal/rampart` |
| `replicaCount` | Replicas (ignored when `autoscaling.enabled`) | `1` |
| `externalDatabase.existingSecret` | Secret holding `DATABASE_URL` | `""` |
| `config.bindAddr` | `BIND_ADDR` | `0.0.0.0:3000` |
| `config.rustLog` | `RUST_LOG` | `info,rampart=debug` |
| `resources` | CPU/memory requests + limits | 100m/128Mi … 1/512Mi |
| `service.port` | Service + container port | `3000` |
| `sessionKey` | Session signing key (auto-generated + persisted if empty) | `""` |

## Uninstall

```bash
helm uninstall rampart
```

PVCs from `persistence.enabled` (or the embedded Postgres) are retained by
Kubernetes — delete them manually if you want the data gone.
