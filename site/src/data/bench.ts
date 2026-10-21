// Benchmark figures shown on the site. Every number here is copied from
// BENCH.md or README.md in this repo — never typed in fresh — and each
// dataset carries enough provenance to find its source table.

const REPO = "https://github.com/oximg/oximg/blob/main";

export type Format = "JPEG" | "PNG" | "WebP" | "AVIF";
export const FORMATS: Format[] = ["JPEG", "PNG", "WebP", "AVIF"];

export interface Cell {
  rps: number;
  p95ms: number;
}

export interface ServerRow {
  server: string;
  ours: boolean;
  cells: Record<Format, Cell>;
}

export interface HarnessRun {
  id: string;
  instance: string;
  arch: string;
  measured: string;
  rows: ServerRow[];
}

export const harness = {
  title: "imgproxy's official benchmark harness",
  workload: "DIV2K corpus over nginx, fit into 512×512, k6 2 VUs × 5 min, all defaults",
  source: `${REPO}/BENCH.md#official-harness-on-real-aws-hardware-c7ilarge-and-c7glarge`,
  // Every cell is a single 5-minute run. JPEG was also run in 3 rotated
  // rounds at 2 and 8 VUs (BENCH.md, same section).
  caveat:
    "One run per cell; over 3 rounds, JPEG is 1.24–1.26× imgproxy on c7i.large and 1.51–1.53× on c7g.large",
  caveatSource: `${REPO}/BENCH.md#official-harness-on-real-aws-hardware-c7ilarge-and-c7glarge`,
  runs: [
    {
      id: "c7i",
      instance: "c7i.large",
      arch: "x86-64 · 2 vCPU",
      measured: "2026-10-21 on 0.15.0",
      rows: [
        {
          server: "oximg",
          ours: true,
          cells: {
            JPEG: { rps: 88.5, p95ms: 29 },
            PNG: { rps: 34.6, p95ms: 75 },
            WebP: { rps: 32.5, p95ms: 88 },
            AVIF: { rps: 16.5, p95ms: 174 },
          },
        },
        {
          server: "imgproxy 4.0.17",
          ours: false,
          cells: {
            JPEG: { rps: 71.9, p95ms: 38 },
            PNG: { rps: 15.1, p95ms: 176 },
            WebP: { rps: 21.5, p95ms: 129 },
            AVIF: { rps: 16.2, p95ms: 179 },
          },
        },
        {
          server: "imagor 1.9.13",
          ours: false,
          cells: {
            JPEG: { rps: 63.7, p95ms: 41 },
            PNG: { rps: 16.3, p95ms: 165 },
            WebP: { rps: 18.6, p95ms: 143 },
            AVIF: { rps: 10.7, p95ms: 270 },
          },
        },
        {
          server: "thumbor 7.8.0",
          ours: false,
          cells: {
            JPEG: { rps: 53.2, p95ms: 47 },
            PNG: { rps: 9.1, p95ms: 293 },
            WebP: { rps: 14.5, p95ms: 180 },
            AVIF: { rps: 12.5, p95ms: 219 },
          },
        },
      ],
    },
    {
      id: "c7g",
      instance: "c7g.large",
      arch: "Graviton3 · 2 cores",
      measured: "2026-10-21 on 0.15.0",
      rows: [
        {
          server: "oximg",
          ours: true,
          cells: {
            JPEG: { rps: 101.0, p95ms: 25 },
            PNG: { rps: 40.8, p95ms: 63 },
            WebP: { rps: 42.5, p95ms: 68 },
            AVIF: { rps: 24.8, p95ms: 119 },
          },
        },
        {
          server: "imgproxy 4.0.17",
          ours: false,
          cells: {
            JPEG: { rps: 65.7, p95ms: 40 },
            PNG: { rps: 20.9, p95ms: 125 },
            WebP: { rps: 25.4, p95ms: 111 },
            AVIF: { rps: 19.9, p95ms: 143 },
          },
        },
        {
          server: "imagor 1.9.13",
          ours: false,
          cells: {
            JPEG: { rps: 55.8, p95ms: 45 },
            PNG: { rps: 21.9, p95ms: 117 },
            WebP: { rps: 19.3, p95ms: 133 },
            AVIF: { rps: 13.4, p95ms: 209 },
          },
        },
        {
          server: "thumbor 7.8.0",
          ours: false,
          cells: {
            JPEG: { rps: 59.4, p95ms: 43 },
            PNG: { rps: 12.3, p95ms: 212 },
            WebP: { rps: 20.1, p95ms: 130 },
            AVIF: { rps: 14.6, p95ms: 198 },
          },
        },
      ],
    },
  ] satisfies HarnessRun[],
};

export interface Proof {
  value: string;
  unit: string;
  label: string;
  versus: string;
  context: string;
  source: string;
}

// The headline strip. Each claim is chosen to be a like-for-like
// comparison: the memory figure is the library one, not the server
// same-URL one, because request coalescing flatters the latter.
export const proofs: Proof[] = [
  {
    value: "1.52",
    unit: "×",
    label: "JPEG throughput",
    versus: "101.0 vs imgproxy 65.7 req/s",
    context: "c7g.large, official harness, 0.15.0, 3 rounds",
    source: harness.source,
  },
  {
    value: "2.1",
    unit: "×",
    label: "JPEG → WebP",
    versus: "79.3 vs 37.0 req/s",
    context: "c7g.large, cross-format cell, pre-0.11 build",
    source: `${REPO}/README.md#benchmarks`,
  },
  {
    value: "+6.3",
    unit: "SSIMULACRA2",
    label: "sharper at the same q80",
    versus: "77.5 vs imgproxy 71.2",
    context: "Kodak corpus, end-to-end JPEG",
    source: `${REPO}/bench/quality/QUALITY.md`,
  },
  {
    value: "37.5",
    unit: "MB",
    label: "peak RSS as a library",
    versus: "ruby-vips 373.3 MB",
    context: "4000×2667 → 750×500, M2 Max",
    source: `${REPO}/BENCH.md#ruby-as-a-library-against-the-image-processing-gems`,
  },
];

export interface QualityPair {
  label: string;
  detail: string;
  ours: number;
  theirs: number;
  theirsName: string;
}

export const quality: QualityPair[] = [
  {
    label: "End-to-end JPEG, q80",
    detail: "Kodak corpus; the gap widens with source size",
    ours: 77.5,
    theirs: 71.2,
    theirsName: "imgproxy",
  },
  {
    label: "Pure resize (lossless PNG path)",
    detail: "isolates the resampler from the encoder",
    ours: 97.6,
    theirs: 81.9,
    theirsName: "imgproxy",
  },
];
