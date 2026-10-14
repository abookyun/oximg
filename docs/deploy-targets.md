# Deployment targets

oximg's primary hosted targets are **Google Cloud Run**, **Cloudflare
Containers** and **Cloudflare Workers**. The first two run the released
image unchanged; Workers runs the pipeline compiled to wasm and is
experimental. Render, Railway and Fly.io come next. Each target gets a
guide, a deploy script under [`deploy/`](../deploy/), and a measured row
in the table below.

| Target | Guide | Deploy from | Status |
|---|---|---|---|
| Cloud Run | [deploy-cloud-run.md](deploy-cloud-run.md) | `deploy/cloud-run/deploy.sh` | supported |
| Cloudflare Containers | [deploy-cloudflare.md](deploy-cloudflare.md) | `deploy/cloudflare-containers/` | supported |
| Cloudflare Workers | [deploy-cloudflare.md](deploy-cloudflare.md#workers-experimental) | `deploy/cloudflare-workers/` | experimental (wasm) |
| Render, Railway, Fly.io | — | — | planned |
| Docker, Kubernetes (self-hosted) | [deploy-docker.md](deploy-docker.md), [deploy-kubernetes.md](deploy-kubernetes.md) | — | supported |

## Choosing

Measured 2026-10-14 (method below). One round only, so read the
differences between the cheap two as indicative.

- **Mostly cache misses at scale, egress paid by the origin:**
  Cloudflare Containers. It had the lowest cost per transform with
  egress counted, and native code, AVIF and threads all work.
- **A CDN in front absorbs egress, or traffic is small:** Cloud Run. It
  is cheapest before egress, has a free tier (2 M requests, 180 k
  vCPU-s per month) and the fastest cold start (0.5 s).
- **Small volume, edge latency, no container:** Workers. Under ~1 M
  transforms a month the $5 plan minimum dominates. Above that it costs
  the most of the three, because wasm takes 3–6× native CPU, and it has
  no AVIF, no URL signing and a 128 MB isolate.
- **Cloudflare Images** costs $0.50 per 1 000 unique transformations,
  100× the others at volume.

## Measured cost per transform

Three deployments ran under the same load at the same time, each
reading sources stored next to it:

- **Workers:** `deploy/cloudflare-workers/wasm-worker`, sources over a
  service binding.
- **Containers:** `docker.io/oximg/oximg:0.14.0` on standard-2 (1 vCPU,
  6 GiB), one instance, sources from a Cloudflare-hosted origin.
- **Cloud Run:** the same image in us-south1, 1 vCPU, 1 GiB,
  concurrency 16, sources from a private GCS bucket in-region.

[`bench/platforms/`](../bench/platforms/) saturated each one for 150 s
per source size with 16 connections from one US client, then read every
platform's own billing metrics. Sources went to 800 px webp (~24 KB
out).

| source | Workers CPU | Containers CPU / awake time | Cloud Run billable instance time |
|---|---|---|---|
| 1 MP JPEG | 129 ms | 39 ms / 27 ms | 40 ms |
| 3 MP JPEG | 256 ms | 45 ms / 45 ms | 70 ms |
| 12 MP JPEG | 685 ms | 169 ms / 119 ms | 157 ms |

One saturated instance did 37 / 22 / 8.5 transforms per second on
Containers and 25 / 14 / 5.4 on Cloud Run. Containers bills more CPU
than its awake time, by up to 1.45×: the "1 vCPU" instance is billed
for more than one core's time.

Monthly USD for 100 M cache-miss transforms, egress to North America
included (`bench/platforms/cost.py`, prices verified 2026-10-14):

| source | Workers | Containers | Cloud Run, request-billed | Cloud Run, instance-billed | Cloud Run without egress |
|---|---|---|---|---|---|
| 1 MP | 289 | **208** | 409 | 344 | 140 |
| 3 MP | 543 | **252** | 479 | 394 | 221 |
| 12 MP | 1,401 | 622 | 695 | **553** | 451 |

Egress decides the ranking. ~2.4 TB a month leaves Cloud Run at
$0.08–0.12 per GiB (Premium Tier); Workers egress is free; Containers
include 1 TB and then charge $0.025/GB. Memory is ~30% of the
Containers bill at standard-2. Smaller instance types (`basic`,
`standard-1`) were not measured and should cost less.

## Cold start

| | cold | warm |
|---|---|---|
| Cloud Run, from zero instances | 0.51 s (container startup 56–145 ms) | 0.13–0.17 s |
| Containers, after sleeping | 0.91–1.24 s | 0.24–0.26 s |
| Containers, first start after deploy (image pull) | 9.9–18.6 s | — |
| Workers (wasm instantiation) | not measured separately | — |

## How it was measured, and what to avoid

- **Distinct URLs.** With one fixed URL, the server's request
  coalescing turned 3965 responses into 1113 pipeline runs. The bench
  draws widths from 700–799 per request. At 16 connections that still
  coalesced ~12% on Containers, so its usage is divided by pipeline
  runs read from `/metrics`. See rule 2 in
  [bench/METHODOLOGY.md](../bench/METHODOLOGY.md).
- **Sources next to compute.** Each platform read from storage on its
  own network, as production would. The bucket's location did not move
  the result: Cloud Run reading from a Cloudflare-hosted origin, then
  from GCS in-region, served 272 and 276 12 MP transforms in the same
  50 s at 4 connections. That was the 1 vCPU quota, not the network.
- **Saturation.** Request-based billing charges instance wall time
  while any request is in flight, so an under-loaded instance bills
  idle CPU. The table is from 16 connections. At 4 connections, billed
  time per transform was 2% higher at 1 MP, the same at 3 MP and 17%
  higher at 12 MP.
- **Lumpy usage buckets.** Cloudflare's per-minute Containers usage
  arrives in uneven buckets (60–810 GiB·s for a constant 6 GiB). The
  bench sums each window plus a 60 s gap instead of reading single
  minutes.
- **Limits.** One round per platform in one 12-minute window, one
  client, one region pair, no interleaving, one instance size each.
