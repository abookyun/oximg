"""Read back each platform's own billing metrics for the windows that
bench/platforms/platforms.sh loaded, and print per-request billable usage.

  python3 bench/platforms/platforms_metrics.py <out-dir> [gap-seconds]

Each size's usage is summed from its window's start minute through the
end of the following idle gap (Cloudflare's per-minute usage buckets
arrive lumpy), minus the container's idle CPU over the same minutes,
taken from the last gap.

Cloudflare: GraphQL Analytics (Workers invocations, Durable Objects,
Containers usage) with $CLOUDFLARE_API_TOKEN, else the cf CLI's OAuth
token. Cloud Run: Cloud Monitoring via `gcloud auth print-access-token`.

Deployment identity comes from the environment (all required except
the last two):
  CF_ACCOUNT_ID     Cloudflare account
  WORKERS_VERSION   deployed version id of oximg-workers
  FRONT_VERSION     deployed version id of oximg-containers
  CONTAINER_APP_ID  Containers application id (cf deploy prints it)
  DO_NAMESPACE_ID   Durable Object namespace id of OximgContainer
  GCP_PROJECT       Cloud Run project
  RUN_SERVICE       Cloud Run service (default oximg)
  CF_CONFIG         cf CLI config (default: its macOS location)
Workers analytics reports cf-deployed scripts as scriptName
"__unknown__", so the two Workers are told apart by version id.
"""
import json, os, subprocess, sys, urllib.request, urllib.parse, datetime as dt

E = os.environ
CF_ACCOUNT = E.get("CF_ACCOUNT_ID", "")
WASM_VERSION = E.get("WORKERS_VERSION", "")
FRONT_VERSION = E.get("FRONT_VERSION", "")
CONTAINER_APP = E.get("CONTAINER_APP_ID", "")
DO_NAMESPACE = E.get("DO_NAMESPACE_ID", "")
GCP_PROJECT = E.get("GCP_PROJECT", "")
RUN_SERVICE = E.get("RUN_SERVICE", "oximg")
CF_CONFIG = os.path.expanduser(E.get("CF_CONFIG", "~/Library/Preferences/cloudflare/config/default.json"))


def cf_gql(query, variables):
    tok = os.environ.get("CLOUDFLARE_API_TOKEN") or json.load(open(CF_CONFIG))["oauth_token"]
    req = urllib.request.Request(
        "https://api.cloudflare.com/client/v4/graphql",
        data=json.dumps({"query": query, "variables": variables}).encode(),
        headers={"Authorization": f"Bearer {tok}", "Content-Type": "application/json"},
    )
    d = json.load(urllib.request.urlopen(req, timeout=60))
    if d.get("errors"):
        raise SystemExit(f"GraphQL: {d['errors']}")
    return d["data"]["viewer"]["accounts"][0]


Q = """query ($a: String!, $from: Time!, $to: Time!, $app: String!, $ns: String!) {
  viewer { accounts(filter: { accountTag: $a }) {
    workers: workersInvocationsAdaptive(limit: 10000,
        filter: { datetimeMinute_geq: $from, datetimeMinute_lt: $to }) {
      dimensions { scriptVersion datetimeMinute }
      sum { requests cpuTimeUs }
    }
    containers: containersUsageAdaptiveGroups(limit: 1000,
        filter: { applicationId: $app, datetimeMinute_geq: $from, datetimeMinute_lt: $to }) {
      dimensions { datetimeMinute }
      sum { cpuTimeSec allocatedMemory allocatedDisk }
    }
    dos: durableObjectsPeriodicGroups(limit: 1000,
        filter: { namespaceId: $ns, datetimeMinute_geq: $from, datetimeMinute_lt: $to }) {
      dimensions { datetimeMinute }
      sum { activeTime cpuTime duration }
    }
  } }
}"""


def cloud_run(start, end):
    tok = subprocess.run(["gcloud", "auth", "print-access-token"], capture_output=True, text=True).stdout.strip()
    out = {}
    for metric, key in [("container/billable_instance_time", "billable_s"), ("request_count", "requests")]:
        params = {
            "filter": f'metric.type="run.googleapis.com/{metric}" AND resource.labels.service_name="{RUN_SERVICE}"',
            "interval.startTime": start, "interval.endTime": end,
            "aggregation.alignmentPeriod": "60s", "aggregation.perSeriesAligner": "ALIGN_SUM",
            "aggregation.crossSeriesReducer": "REDUCE_SUM",
        }
        url = f"https://monitoring.googleapis.com/v3/projects/{GCP_PROJECT}/timeSeries?" + urllib.parse.urlencode(params)
        d = json.load(urllib.request.urlopen(urllib.request.Request(url, headers={"Authorization": f"Bearer {tok}"}), timeout=60))
        for ts in d.get("timeSeries", []):
            for p in ts["points"]:
                # A point's endTime closes the minute it sums.
                end_t = dt.datetime.fromisoformat(p["interval"]["endTime"].replace("Z", "+00:00"))
                minute = (end_t - dt.timedelta(seconds=1)).strftime("%Y-%m-%dT%H:%M:00Z")
                v = p["value"]
                out.setdefault(minute, {}).setdefault(key, 0.0)
                out[minute][key] += float(v.get("doubleValue", v.get("int64Value", 0)))
    return out


def oximg_delta(out_dir, name, size):
    """Pipeline runs and mean phase times inside the window, from the
    native server's own counters (scraped before/after by platforms.sh).
    Runs < responses means coalescing merged identical requests."""

    def read(path):
        c = {}
        try:
            for line in open(path):
                k, v = line.rsplit(" ", 1)
                c[k] = float(v)
        except FileNotFoundError:
            pass
        return c

    a, b = read(f"{out_dir}/{name}-{size}.before"), read(f"{out_dir}/{name}-{size}.after")
    if not b:
        return None
    d = lambda k: b.get(k, 0) - a.get(k, 0)
    runs = d('oximg_request_duration_seconds_count{phase="process"}')
    return {
        "responses_2xx": sum(d(k) for k in b if k.startswith('oximg_requests_total{class="2xx"')),
        "pipeline_runs": runs,
        "process_ms_mean": 1e3 * d('oximg_request_duration_seconds_sum{phase="process"}') / max(runs, 1),
        "fetch_ms_mean": 1e3 * d('oximg_request_duration_seconds_sum{phase="fetch"}') / max(runs, 1),
        "queue_ms_mean": 1e3 * d('oximg_request_duration_seconds_sum{phase="queue"}') / max(runs, 1),
    }


def iso(t):
    return t.strftime("%Y-%m-%dT%H:%M:00Z")


def minutes(a, b):
    """Minute keys from a (floored) up to but excluding b (floored)."""
    t = a.replace(second=0)
    while t < b.replace(second=0):
        yield iso(t)
        t += dt.timedelta(minutes=1)


def main(out_dir, gap):
    need = {"CF_ACCOUNT_ID": CF_ACCOUNT, "WORKERS_VERSION": WASM_VERSION, "FRONT_VERSION": FRONT_VERSION,
            "CONTAINER_APP_ID": CONTAINER_APP, "DO_NAMESPACE_ID": DO_NAMESPACE, "GCP_PROJECT": GCP_PROJECT}
    missing = [k for k, v in need.items() if not v]
    if missing:
        raise SystemExit(f"set {', '.join(missing)} (see the module docstring)")
    parse = lambda s: dt.datetime.fromisoformat(s.replace("Z", "+00:00"))
    lines = [l.split("\t") for l in open(f"{out_dir}/windows.tsv").read().split("\n")[1:] if l]
    windows = [(size, parse(a), parse(b)) for size, a, b in lines]
    first, last = windows[0][1], windows[-1][2] + dt.timedelta(seconds=gap)
    q_from, q_to = iso(first), iso(last + dt.timedelta(minutes=1))
    cf = cf_gql(Q, {"a": CF_ACCOUNT, "from": q_from, "to": q_to, "app": CONTAINER_APP, "ns": DO_NAMESPACE})
    run = cloud_run(q_from, q_to)

    per = {}  # minute -> metrics
    for r in cf["workers"]:
        k = {WASM_VERSION: "wasm", FRONT_VERSION: "front"}.get(r["dimensions"]["scriptVersion"])
        if k:
            e = per.setdefault(r["dimensions"]["datetimeMinute"], {})
            e[f"{k}_req"] = e.get(f"{k}_req", 0) + r["sum"]["requests"]
            e[f"{k}_cpu_s"] = e.get(f"{k}_cpu_s", 0) + r["sum"]["cpuTimeUs"] / 1e6
    for r in cf["containers"]:
        e = per.setdefault(r["dimensions"]["datetimeMinute"], {})
        e["c_cpu_s"] = e.get("c_cpu_s", 0) + r["sum"]["cpuTimeSec"]
        e["c_mem_gibs"] = e.get("c_mem_gibs", 0) + r["sum"]["allocatedMemory"] / 2**30
        e["c_disk_gbs"] = e.get("c_disk_gbs", 0) + r["sum"]["allocatedDisk"] / 1e9
    for r in cf["dos"]:
        e = per.setdefault(r["dimensions"]["datetimeMinute"], {})
        e["do_gbs"] = e.get("do_gbs", 0) + r["sum"]["duration"]
    for m, v in run.items():
        e = per.setdefault(m, {})
        e["run_billable_s"] = e.get("run_billable_s", 0) + v.get("billable_s", 0)
        e["run_req"] = e.get("run_req", 0) + v.get("requests", 0)

    def total(keys, key):
        return sum(per.get(m, {}).get(key, 0) for m in keys)

    # Idle baseline: the minutes fully inside the last gap.
    idle_keys = list(minutes(windows[-1][2] + dt.timedelta(seconds=60), last))
    idle_cpu_per_min = total(idle_keys, "c_cpu_s") / max(len(idle_keys), 1)

    rows = []
    for i, (size, a, b) in enumerate(windows):
        # Through the gap, but stop before the next window starts.
        stop = windows[i + 1][1] if i + 1 < len(windows) else b + dt.timedelta(seconds=gap)
        keys = list(minutes(a, stop))
        secs = (b - a).total_seconds()
        oha = {}
        for name in ("workers", "containers", "cloudrun"):
            try:
                o = json.load(open(f"{out_dir}/{name}-{size}.json"))
                oha[name] = {"ok": o["statusCodeDistribution"].get("200", 0),
                             "non200": {k: v for k, v in o["statusCodeDistribution"].items() if k != "200"},
                             "rps": o["statusCodeDistribution"].get("200", 0) / secs,
                             "p50_s": o["latencyPercentiles"]["p50"], "p99_s": o["latencyPercentiles"]["p99"]}
            except FileNotFoundError:
                oha[name] = None
        n_w = max(total(keys, "wasm_req"), 1)
        n_c = max(total(keys, "front_req"), 1)
        n_r = max(total(keys, "run_req"), 1)
        c_cpu = total(keys, "c_cpu_s") - idle_cpu_per_min * len(keys)
        row = {
            "size": size, "window_s": secs, "minutes": [k[11:16] for k in keys],
            "workers": {"requests": n_w, "cpu_ms_per_req": 1e3 * total(keys, "wasm_cpu_s") / n_w, "client": oha["workers"]},
            "containers": {
                "requests": n_c,
                "cpu_ms_per_req": 1e3 * c_cpu / n_c,
                "awake_s_per_req": secs / n_c,  # one instance, awake throughout
                "front_cpu_ms_per_req": 1e3 * total(keys, "front_cpu_s") / n_c,
                "do_gbs_per_req": total(keys, "do_gbs") / n_c,
                "client": oha["containers"], "oximg": oximg_delta(out_dir, "containers", size),
            },
            "cloudrun": {
                "requests": n_r,
                "billable_ms_per_req": 1e3 * total(keys, "run_billable_s") / n_r,
                "client": oha["cloudrun"], "oximg": oximg_delta(out_dir, "cloudrun", size),
            },
        }
        rows.append(row)
        print(json.dumps(row))
    json.dump({"idle_container_cpu_s_per_min": idle_cpu_per_min, "rows": rows},
              open(f"{out_dir}/metrics.json", "w"), indent=1)
    print(f"container idle CPU: {idle_cpu_per_min:.2f} s/min (subtracted)")


if __name__ == "__main__":
    main(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 60)
