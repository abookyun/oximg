"""Monthly cost of N cache-miss transforms, from measured usage.

Usage per transform: bench/platforms/platforms.sh + platforms_metrics.py, run3,
2026-10-14 21:39-21:51 UTC. Each platform was saturated for 150 s per
size (oha -c 16, widths 700-799 so requests are distinct), sources
-> 800 px webp, read from the platform's own billing metrics:
- Workers (wasm, route B): workersInvocationsAdaptive cpuTimeUs.
- Containers (oximg:0.14.0, standard-2 = 1 vCPU / 6 GiB / 12 GB, one
  instance): containersUsageAdaptiveGroups cpuTimeSec; awake time =
  window / pipeline runs; plus the front Worker and its Durable Object.
  Divided by pipeline runs (the server coalesced ~12% of responses).
- Cloud Run (oximg:0.14.0, us-south1, 1 vCPU / 1 GiB, concurrency 16,
  sources from GCS in-region): billable_instance_time / requests.
Output bytes per transform: ~24 KB (oha sizePerRequest, all three agree).

Prices: USD, from the vendors' pricing pages, verified 2026-10-14.
"""

SIZES = ("1MP", "3MP", "12MP")
OUT_BYTES = {"1MP": 25_300, "3MP": 24_200, "12MP": 22_800}

# Measured, per transform.
WORKERS_CPU_MS = {"1MP": 128.8, "3MP": 255.8, "12MP": 684.7}
CONT_CPU_MS = {"1MP": 39.1, "3MP": 45.4, "12MP": 169.1}
CONT_AWAKE_MS = {"1MP": 26.7, "3MP": 45.4, "12MP": 118.5}
CONT_FRONT_CPU_MS = {"1MP": 0.82, "3MP": 0.88, "12MP": 0.87}
CONT_DO_GBS = {"1MP": 0.0054, "3MP": 0.0092, "12MP": 0.0177}
RUN_BILLABLE_MS = {"1MP": 40.0, "3MP": 70.4, "12MP": 157.3}

CONT_MEM_GIB, CONT_DISK_GB = 6, 12  # standard-2
RUN_VCPU, RUN_MEM_GIB = 1, 1


def gcp_egress_na(gib):
    """Premium Tier, us-south1 -> North America, tiered per month."""
    tiers = [(1, 0.0), (1024, 0.12), (10240, 0.11), (float("inf"), 0.08)]
    cost, prev = 0.0, 0
    for top, price in tiers:
        cost += max(0, min(gib, top) - prev) * price
        prev = top
    return cost


def workers(n, s, egress=True):
    cpu_ms = n * WORKERS_CPU_MS[s]
    return 5 + max(0, n - 10e6) * 0.30e-6 + max(0, cpu_ms - 30e6) * 0.02e-6


def containers(n, s, egress=True):
    cpu_s = n * CONT_CPU_MS[s] / 1e3
    awake_s = n * CONT_AWAKE_MS[s] / 1e3
    c = 5
    c += max(0, cpu_s - 375 * 60) * 0.000020
    c += max(0, awake_s * CONT_MEM_GIB - 25 * 3600) * 0.0000025
    c += max(0, awake_s * CONT_DISK_GB - 200 * 3600) * 0.00000007
    c += max(0, n - 10e6) * 0.30e-6  # front Worker requests
    c += max(0, n * CONT_FRONT_CPU_MS[s] - 30e6) * 0.02e-6  # front Worker CPU
    c += max(0, n - 1e6) * 0.15e-6  # Durable Object requests
    c += max(0, n * CONT_DO_GBS[s] - 400e3) * 12.50e-6  # DO duration
    if egress:  # North America: 1 TB included, then $0.025/GB
        c += max(0, n * OUT_BYTES[s] / 1e9 - 1000) * 0.025
    return c


def cloud_run(n, s, egress=True, instance_based=False):
    inst_s = n * RUN_BILLABLE_MS[s] / 1e3
    if instance_based:  # saturated always-on instances; no request fee
        c = max(0, inst_s * RUN_VCPU - 240e3) * 0.000018
        c += max(0, inst_s * RUN_MEM_GIB - 450e3) * 0.000002
    else:
        c = max(0, inst_s * RUN_VCPU - 180e3) * 0.000024
        c += max(0, inst_s * RUN_MEM_GIB - 360e3) * 0.0000025
        c += max(0, n - 2e6) * 0.40e-6
    if egress:
        c += gcp_egress_na(n * OUT_BYTES[s] / 2**30)
    return c


def cf_images(n, s=None, egress=True):
    return 5 + max(0, n - 5000) * 0.50e-3


if __name__ == "__main__":
    cols = [
        ("Workers", workers, {}),
        ("Containers", containers, {}),
        ("CRun req", cloud_run, {}),
        ("CRun inst", cloud_run, {"instance_based": True}),
        ("CRun req, no egress", cloud_run, {"egress": False}),
        ("CF Images", cf_images, {}),
    ]
    for s in SIZES:
        print(f"\n{s} -> 800px webp, USD/month (egress to North America included unless noted)")
        print(f"{'transforms':>14}" + "".join(f"{c[0]:>21}" for c in cols))
        for n in [1e6, 10e6, 100e6, 1e9]:
            print(f"{n:>14,.0f}" + "".join(f"{f(n, s, **kw):>21,.0f}" for _, f, kw in cols))
