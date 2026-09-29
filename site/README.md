# oximg.dev

The project website: a static [Astro](https://astro.build) site. Only
the home page exists so far; docs, benchmarks, migration guides and a
playground are planned.

```sh
npm install
npm run dev       # http://localhost:4321
npm run build     # static output in dist/
```

## Rules

- **Numbers come from the repo.** Every figure lives in
  `src/data/bench.ts`, copied from `BENCH.md`, `README.md` or
  `bench/quality/QUALITY.md`, with the machine, date and a link to the
  source table. Never type a number into a component.
- **Show the losses.** The benchmark section keeps a "where we don't
  lead" list; update it whenever a remeasure changes a cell.
- **No JavaScript unless it earns it.** The chart switch is CSS; the
  only script on the home page is the copy button.

## Color system: "Oxide"

Tokens are in `src/styles/tokens.css`: primitives first, then a
semantic layer. Components only use the semantic layer.

| Token | Hue | Meaning |
|---|---|---|
| `--brand` | rust `#e8622c` | oximg — its data, its CTAs, its mark |
| `--accent` | patina `#3fb6a8` | secondary: links, states, "correct" |
| `--series-theirs-*` | graphite grays | competitors in charts, always |
| `--bg` / `--surface` | graphite `#0b0d0e` / `#111416` | dark-first surfaces |

Rust is reserved for oximg, so in any chart a reader can tell whose bar
is whose without a legend. Code samples use the matching Shiki theme in
`src/styles/code-theme.ts`. Type is Inter for prose and JetBrains Mono
for code, labels and every number (`.num` sets tabular figures).
