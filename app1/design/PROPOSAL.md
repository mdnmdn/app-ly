# Proposal: token-efficient component layer for the FiscoDesk design system

Discussion doc. Nothing implemented yet. Revised: consolidated on Vue 3
(no build) for both layout and logic — see "Decision" below for why the
original native-custom-elements draft was dropped in favor of one system.

## The problem, with numbers

Every view repeats the same ~30 lines of sidebar nav + topbar markup, byte-for-byte
except for one `active` class and a title:

```html
<aside class="sidebar">
  <div class="sidebar-brand">...</div>
  <div class="nav-section">Main</div>
  <ul class="nav-list">
    <li><a class="nav-link" href="dashboard.html"><svg class="nav-icon" viewBox="0 0 18 18" ...>...</svg>Dashboard</a></li>
    <li><a class="nav-link active" href="invoices.html"><svg ...>...</svg>Invoices</a></li>
    <li><a class="nav-link" href="customers.html"><svg ...>...</svg>Customers</a></li>
  </ul>
  <div class="nav-section">Tax</div>
  <ul class="nav-list">... 5 more entries, same shape ...</ul>
</aside>
<header class="topbar">...</header>
```

That block is copy-pasted in all 8 views (`views/*.html`), ~230 duplicated
lines app-wide. It's also a correctness trap: nothing enforces that the
`active` class matches the filename, so it *will* drift.

It already has: `index.html` and `fiscodesk-storyboard.html` are two independent
copies of the same landing page. They've already diverged — one says "8 screens"
and mentions 3 color palettes, the other doesn't, and neither lists `ai-lab.html`.
This is the redundancy problem you're describing, caught in the act.

Same pattern, smaller scale, on modals/drawers (`dashboard.html` has 3 overlays,
each hand-rolling the same header/footer wrapper) and on the storyboard's fake
preview thumbnails (~15 lines of nested `div`s per card, per screen, that just
approximate what the real view already looks like).

Table rows are boilerplate too, but of a different kind — each `<td data-field
data-value="X">X</td>` repeats the value once as an attribute and once as
rendered content, and the ~150 lines of `app.js` that turn a row into an
editable form (`createInlineControl`, `renderCellDisplay`, `startRowEdit`,
`saveRowEdit`, `cancelRowEdit`) exist only because that state has to be
hand-synced with the DOM. This is the part the Vue decision below targets
directly.

## Goal

Cut the boilerplate an LLM has to write per view, in one coherent system —
not two systems glued together with a hand-off rule — without a build step,
bundler, or framework the LLM has to learn from source rather than from
already-known conventions.

## Decision: Vue 3, no-build, one app per page — layout *and* logic

The first draft of this doc split the problem in two: native custom elements
(zero-dependency, zero-`eval`) for the static chrome, petite-vue for the
data-shaped parts (tables, forms). That works, but it means an LLM writing a
new view has to hold two APIs in its head — a bespoke
`<app-shell page="...">` attribute/slot contract that only exists in this
repo, *and* Vue's directives for everything else — plus a DOM-ownership rule
("custom elements own the chrome, Vue owns `<main>`, don't let them fight
over the same subtree").

Collapsing to Vue for both removes all of that: one vocabulary, and it's
Vue's own — `v-for`, `v-if`, `v-bind`, `slots` — which is far more
represented in LLM training data than a repo-invented custom-element
contract would ever be. That's a genuine token-economy win, not just a
simplification for its own sake.

Costs you're accepting by making this call, worth being explicit about:

- **Every page now has a hard Vue dependency**, including ones that don't
  need reactivity at all (`reports.html`'s placeholder, `ai-lab.html`'s test
  harness) — they still mount Vue just to render the shared sidebar. Tens of
  kb, not huge, but it's no longer "zero cost for static pages" the way
  native custom elements were.
- **The CSP/`eval` caveat applies app-wide now, not just to 2-3 data views.**
  Vue's in-DOM template compiler needs `new Function(...)` at runtime, and
  every page uses it now (via `AppShell`), not just the ones with reactive
  tables. `src-tauri/tauri.conf.json` currently has `"csp": null`, so nothing
  is enforced today — but if that ever gets locked down, this is an app-wide
  `unsafe-eval` requirement, not a scoped one.
- **No `<script setup>`.** No-build means the global CDN build compiling
  in-DOM templates via the **Options API** (`data()/methods/computed`) — the
  SFC syntax that dominates Vue's docs and most LLM defaults needs a compile
  step and isn't available here. Worth one convention line somewhere visible
  so generations don't drift toward syntax that can't run.
- **This is "the whole page is a Vue app" now, not "static HTML with a
  sprinkle."** More idiomatic Vue, but a bigger architectural commitment than
  mounting Vue only where a table loops over data.

None of these are blockers — they're the trade for one system instead of
two. Proceeding on that basis.

## What it looks like

### Shared layout component — replaces the hand-copied sidebar/topbar

```js
// js/components/app-shell.js — one file, imported by every view
export const NAV = [
  { section: 'Main', items: [
    { slug: 'dashboard', label: 'Dashboard', icon: 'grid' },
    { slug: 'invoices',  label: 'Invoices',  icon: 'doc' },
    { slug: 'customers', label: 'Customers', icon: 'users' },
  ]},
  { section: 'Tax', items: [ /* ...5 more, same shape... */ ] },
];

export const AppShell = {
  props: ['page', 'title'],
  data() { return { nav: NAV } },
  template: `
    <aside class="sidebar">
      <div class="sidebar-brand">...</div>
      <template v-for="s in nav" :key="s.section">
        <div class="nav-section">{{ s.section }}</div>
        <ul class="nav-list">
          <li v-for="item in s.items" :key="item.slug">
            <a class="nav-link" :class="{ active: item.slug === page, disabled: item.disabled }"
               :href="item.slug + '.html'">{{ item.label }}</a>
          </li>
        </ul>
      </template>
    </aside>
    <header class="topbar">
      <div class="topbar-left"><h1>{{ title }}</h1></div>
      <div class="topbar-right"><slot name="topbar-right" /></div>
    </header>
    <main class="main"><slot /></main>
  `
};
```

```html
<!-- views/invoices.html -->
<body class="app-shell">
  <div id="app">
    <app-shell page="invoices" title="Invoices">
      <template #topbar-right>
        <select class="select select-sm">...</select>
      </template>

      <div class="toolbar">...</div>
      <div class="table-wrap">
        <table class="data-table">
          <tbody>
            <tr v-for="inv in invoices" :key="inv.number">
              <td class="text-mono">{{ inv.number }}</td>
              <td><span :class="'badge badge-' + inv.status.toLowerCase()">{{ inv.status }}</span></td>
              <td class="num">{{ formatEUR(inv.taxable) }}</td>
              <td>
                <template v-if="editing === inv.number">
                  <input class="input input-sm" v-model="inv.description" />
                  <button class="btn btn-sm btn-primary" @click="editing = null">Save</button>
                </template>
                <template v-else>
                  <button class="btn btn-sm btn-ghost" @click="editing = inv.number">Edit</button>
                </template>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </app-shell>
  </div>

  <script type="module">
    import { AppShell } from '../js/components/app-shell.js';
    const { createApp } = Vue;
    createApp({
      components: { AppShell },
      data() { return { editing: null, invoices: [ /* JSON rows */ ] } },
      methods: { formatEUR: window.FiscoDesk.formatEUR }
    }).mount('#app');
  </script>
</body>
```

~30 lines of sidebar/topbar collapse to a 3-line `<app-shell>` tag with a
named slot; ~10 lines of `<td data-field data-value>` per row collapse to a
`v-for` over a JSON array; the ~150 lines of imperative row-editing state in
today's `app.js` collapse to `editing === inv.number` and `v-model`. That
last part is the biggest win — not just shorter, but a whole class of
DOM-state-tracking bugs (values living in `dataset.value` that have to be
manually torn down and rebuilt) stops existing.

### Modals/drawers — `v-if`/`:class` instead of `data-modal-open` delegation

```html
<ui-modal v-model:open="showAppointmentModal" title="New Tax Appointment">
  <div class="form-grid">...</div>
</ui-modal>
```

```js
export const UiModal = {
  props: ['open', 'title'],
  emits: ['update:open'],
  template: `
    <div class="overlay" :class="{ open }">
      <div class="modal">
        <div class="modal-header"><h2>{{ title }}</h2>
          <button class="btn btn-ghost btn-icon" @click="$emit('update:open', false)">✕</button></div>
        <div class="modal-body"><slot /></div>
        <div class="modal-footer">
          <button class="btn" @click="$emit('update:open', false)">Cancel</button>
          <button class="btn btn-primary" @click="$emit('update:open', false)">Save</button>
        </div>
      </div>
    </div>`
};
```

This replaces `app.js`'s `data-modal-open`/`data-modal-close` click-delegation
entirely — open/close *is* the reactive `showAppointmentModal` boolean, no
separate event-wiring layer to keep in sync. Same pattern retires the
drawer, tab, and (mostly) theme/palette logic in `app.js` too, if you want to
go all the way — that file is 745 lines today, and a large share of it is
exactly the kind of imperative DOM-state-sync Vue exists to delete.

## Storyboard

Same fixes as before, now expressed the same way as everything else:

1. **One file, not two** — fold `fiscodesk-storyboard.html` into `index.html`.
2. **Live previews instead of hand-drawn ones** — scaled-down iframe of the
   real view instead of ~15 lines of fake `.screen-preview-inner` div soup:
   ```html
   <iframe class="screen-preview-frame" src="views/dashboard.html" tabindex="-1" loading="lazy"></iframe>
   ```
   Can't drift from the real view because it *is* the real view.
3. **`v-for` over a `SCREENS` array**, consistent with everything else, instead
   of a hand-copied card block per screen:
   ```js
   const SCREENS = [
     { slug: 'dashboard', title: 'Dashboard', desc: 'Stats, charts, upcoming appointments, payment alerts', badges: ['Home'] },
     // ...
   ];
   ```

## What this still deliberately does not touch

- **`css/common.css` stays as-is** — already a compact, sensible
  utility+component system.
- **No bundler, no npm install, no `.vue` SFC files.** `vue.global.js` via a
  single vendored file or CDN `<script>`; views stay openable as plain files.
- **Table data stays a plain JS array per view** — no backend, no store, no
  routing library. Don't reach for Vuex/Pinia/vue-router here; nothing in
  this system needs cross-page state or client-side routing yet.

## LLM-facing reference

`design/COMPONENTS.md`: one section per shared Vue component (`AppShell`,
`UiModal`, `UiDrawer`) — props, slots, a minimal copy-paste example, ~8 lines
each — plus one convention line at the top: *"Options API, global build, no
`<script setup>`, no SFCs."* That's what an LLM building a new view should
read instead of `js/components/*.js`.

## Appendix: the native-custom-elements split (not proceeding)

Kept for reference in case the app-wide Vue dependency or the CSP/`eval`
constraint ever becomes a real blocker — e.g. CSP gets locked down in
`tauri.conf.json` and `unsafe-eval` turns out to be unacceptable. The
fallback: swap `AppShell`'s Vue component definition for a
`customElements.define` class with the same props-via-attributes,
slots-via-`<slot>` shape; per-view markup barely changes, since the whole
point of that draft was the same "one shared file, short tag per view" shape
Vue gives you here. Also considered and still rejected either way: Alpine.js
(same eval caveat as Vue, less capable), Lit (no eval, but a dependency for a
problem this scale doesn't need), Declarative Shadow DOM (fights the "one
shared stylesheet" model `common.css` already gives us), SSI-style includes
(needs a server or build step).

## Suggested order of work

1. `js/components/app-shell.js` (Vue component + `NAV` config) — touches all 8 views, biggest win.
2. `UiModal` / `UiDrawer` Vue components — replace `app.js`'s modal/drawer delegation.
3. Convert one data-heavy view (invoices or tax-appointments) to JSON-array `v-for` rows + `editing` state, as the pattern to copy for the rest.
4. Storyboard rebuild (merge files, iframe previews, `v-for` card grid) + `COMPONENTS.md`.
5. Once the pattern's proven, decide whether to also fold theme/palette/tabs out of `app.js` into the same Vue app, or leave those as-is (they're small and not broken).

## Questions before building anything

- OK to delete one of `index.html` / `fiscodesk-storyboard.html`?
- OK with every page (including `reports.html`, `ai-lab.html`) carrying the Vue runtime, even where nothing on that page is reactive?
- Vendor `vue.global.js` locally (`js/vendor/vue.global.js`) or load from a CDN? Local avoids an external network dependency for what's meant to become a desktop app — worth doing regardless of CDN convenience.
- Any near-term plan to lock down `tauri.conf.json`'s CSP? That's the one thing that would push this back toward the Appendix.
