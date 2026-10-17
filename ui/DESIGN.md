---
name: horon-router Instrument
description: The proxy drawn as a well-made device, after Rams-era Braun.
colors:
  light-background: "#ffffff"
  light-panel: "#f5f5f5"
  light-raised: "#ffffff"
  light-line: "#e5e5e5"
  light-ink: "#0a0a0a"
  light-ink-2: "#737373"
  light-hole: "#e5e5e5"
  dark-background: "#0a0a0a"
  dark-panel: "#262626"
  dark-raised: "#171717"
  dark-line: "#ffffff1a"
  dark-ink: "#fafafa"
  dark-ink-2: "#a1a1a1"
  dark-hole: "#2b2b2b"
  primary: "#171717 / #e5e5e5"
  primary-foreground: "#fafafa / #171717"
  ring: "#a1a1a1 / #737373"
  ok: "#00a63e / #05df72"
  warn: "#e17100 / #ffb900"
  bad: "#e7000b / #ff6467"
  warn-text: "#bb4d00 / #ffb900"
rounded:
  sm: "6px"
  field: "8px"
  window: "12px"
  lg: "16px"
  key: "999px"
typography:
  ui:
    fontFamily: "Host"
  code:
    fontFamily: "ui-monospace, SF Mono, Cascadia Mono, Menlo, Consolas"
---

# Design system: Instrument

## World

The dashboard is drawn as the face of a calm, precise device, after Dieter Rams' Braun work: the ET66 calculator, the T3 radio, the SK4. The device vocabulary is the component vocabulary: a recessed **display window** for readings, round **keys** for actions, small **lamps** for state, and a perforated **grille** for traffic. Everything else is space, alignment and hairlines.

Light theme is white with ink text for daytime. Dark theme is near-black for late-night work. Both use the same structure and the same neutral primary.

## Anti-references

- The KPI-card SaaS dashboard: a row of equal metric cards, a gradient area chart, every section in a bordered card.
- The previous "Observatory" UI of this project: taglines, status said three times, an empty chart dominating first run.
- Terminal cosplay: green-on-black monospace everywhere.
- Grafana-style walls of dark panels; glass, glow and gradients.
- Skeuomorphic hardware: screws, brushed metal, RJ45 ports. Braun is the grammar, not a costume.

## Colour

`src/style.css` is the source of truth. The pen.dev canvas Foundations frame `b67W7z` is the source of the values: the shadcn/ui neutral theme. Strategy: restrained. One neutral grey scale carries every surface, and the primary action is neutral ink.

- **Body** is the page. **Panel** is the muted surface (display windows, segmented tracks, code). **Raised** is a key or field face.
- **Primary** is neutral ink (`#171717` light, `#e5e5e5` dark). It appears in exactly three places: the primary key, the current-page dot, and text selection. There is no second accent colour.
- **Lamps**: green ready, orange cooling, red failing, hollow ring disabled or not available. A lamp always has a word next to it or an accessible label.
- **Grille**: unlit holes use `--hole`; lit holes are ink at four strengths; a hole is red when its bucket had failures.
- Text uses `--ink` or `--ink-2` (5:1 or better on body and panel in both themes, checked with axe-core). `--ink-3` is never text: it draws hole edges, hollow lamps and hover borders.
- Warning text in logs uses `--warn-ink`, a darker orange in the light theme, because the lamp orange is too light for small text.
- **Flat surfaces**: there is no depth cue. `--sunk` and `--lift` are both `none`, so a raised key, a segmented track and a card all sit on one plane and are told apart by fill and a 1 px hairline border only. No inset shadow, no drop shadow, no glow.
- `border` and `input` are solid in the light theme and translucent white in the dark theme, so they sit on any surface.

## Type

One family, Host Grotesk (declared as `font-family: Host`), self-hosted as a 17.5 KB Latin subset with weights 400–650 and falling back to `system-ui`. Its figures are tabular by default, so numbers align without features.

| Role | Size / weight |
| --- | --- |
| Display reading | 52 px / 500, −0.035em (40 px on phones) |
| Page title | 24 px / 600, −0.02em |
| Section title | 17 px / 600 |
| Body, controls | 14 px / 400–500 |
| Legends, meta | 12.5–13 px / 400, `--ink-2` |

Monospace is for data only: keys, file names in detail views, paths, log lines and diffs. Sentence case everywhere; no tracked capitals, no eyebrows above headings.

## Layout

- Desktop: a 216 px key column (wordmark, pages, theme and sign-out keys) and a content column up to 1200 px with 48 px gutters. Connection state and server version appear once, top right.
- Pages stack sections with 40 px between them; a section title sits 16 px above its content.
- Lists are open: hairline rows, no card. Groups (by provider) are a bold label and a count.
- Phones (≤ 860 px): a top bar with wordmark, connection lamp and a menu key that opens the page list full-screen. Rows reflow into two lines, the grille under the name. Touch targets are at least 40 px.

## Components

- **Key**: pill, raised face with a 1 px hairline border; pressing moves it down 1 px. Primary is neutral ink with light text. Quiet keys have no border. Danger is red text, never a red fill.
- **Display window**: 12 px radius, muted fill with a hairline, readings separated by hairlines. Used on Overview and the live Usage view only.
- **Grille**: SVG circles, 20 ten-minute buckets, oldest left. Rows use 7 px holes; the Overview display stacks five holes per bucket like a level meter. Each grille has an accessible summary ("86 requests in 200 minutes, 86 failed").
- **Segmented control**: muted track, the selected option is a raised key face, `aria-pressed`.
- **Fields**: raised face, 8 px radius, label above in `--ink-2`. Focus is a 1 px ink ring plus the border.
- **States**: loading is one breathing skeleton row; empty explains what to do next; not available names the route and status code; errors keep the last good data visible when there is any.
- **Toast**: an ink pill at the bottom centre. Success clears itself after four seconds; errors stay until dismissed and use `role=alert`.
- Icons are 15 hand-drawn 24 px strokes (1.7 px, round caps) in an SVG sprite in `index.html`.

## Motion

Motion only reports state. Keys press (120 ms). Lamps on live things (log tail, OAuth waiting, live usage) breathe on a 2.4 s cycle. Toasts rise 8 px in 200 ms. No page transitions, no entrance choreography. `prefers-reduced-motion` turns all animation and transition off.

## Do and don't

- Do show each fact once, in the place it is used.
- Do disable an action the server lacks and say so in one line. Don't offer it and let it fail.
- Do keep explanatory copy to the actions that are costly to get wrong (deleting, draining the usage queue).
- Don't add taglines, cards around sections, gradients, glows, or a second accent colour.
- Don't store the management key or show a metric the API does not report.
