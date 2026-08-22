# FiscoDesk design system — component reference

**Start at `storybook.html`, not here.** It's a live, running showcase of
every CSS pattern and every component below (tables, cards, tabs, modal,
drawer, the AppShell chrome, design tokens) with a comment on each section
explaining what it's for and when to reach for it — open it in a browser and
copy markup straight out of it. This file is the prose backup for when the
storybook's comments aren't enough; the source under `js/components/` is the
backup for when this file isn't enough.

**Convention: Options API, global build, no `<script setup>`, no SFCs.**
There is no build step. `vendor/vue.global.js` is a plain `<script>` that
defines a global `Vue`; every view calls `Vue.createApp({...}).mount('#app')`
with a `data()/methods/computed` options object. `<script setup>` and `.vue`
files need a compiler that isn't part of this setup — don't reach for that
syntax here.

## Page skeleton

Every view (except the storyboard, which has no app chrome) follows this
shape. Copy `views/invoices.html` as the starting point for a new page, not
this snippet — it's the real, running example.

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <link rel="stylesheet" href="../css/common.css" />
  <style>#app { display: contents; }</style>
  <title>{Page} — FiscoDesk</title>
</head>
<body class="app-shell">
  <div id="app">
    <app-shell page="{slug}" title="{Page}">
      <template #topbar-right>
        <!-- FY select, theme toggle, whatever this page's topbar needs -->
      </template>
      <!-- page content: v-for tables, cards, forms -->
    </app-shell>
  </div>

  <script src="../vendor/vue.global.js"></script>
  <script src="../js/theme.js"></script>
  <script type="module">
    import { AppShell } from '../js/components/app-shell.js';
    const { createApp } = Vue;
    createApp({
      components: { AppShell },
      data() { return { /* page state, plain JS/JSON */ } },
      methods: { formatEUR: window.FiscoDesk.formatEUR, formatPct: window.FiscoDesk.formatPct },
    }).mount('#app');
  </script>
</body>
</html>
```

Script order matters for one reason: `js/theme.js` is a classic script that
reads/writes `window.FiscoDesk` and the palette selector immediately; the
Vue app is a `type="module"` script, which the HTML spec always defers until
after parsing, so `window.FiscoDesk` is guaranteed to exist by the time a
view's `methods` object references it, and the palette selector (which
anchors into `.topbar-right`) finds it already rendered by Vue. Keep
`vue.global.js` and `theme.js` as plain scripts, keep the app in a `module`
script, and don't reorder them.

The `<style>#app { display: contents; }</style>` line matters too:
`body.app-shell` is a CSS grid in `common.css` that expects `.sidebar`,
`.topbar`, `.main` as *direct* children of `<body>`. Vue mounts them one
level deeper, inside `#app`; `display: contents` makes that wrapper div
transparent to the grid so its children still count as direct grid items.
Drop it and the layout collapses to a narrow column. Not needed on pages
that don't use `<app-shell>` (e.g. the storyboard).

## `AppShell`

Sidebar nav + topbar + `<main>`. Renders the entire chrome from one `NAV`
config array (`js/components/app-shell.js`) — there is nothing view-specific
in the component itself.

- **Props:** `page` (slug used to highlight the active nav link, e.g.
  `"invoices"`), `title` (topbar `<h1>`).
- **Slots:** `topbar-right` (named — FY selector, theme toggle, per-page
  topbar controls), default (the page body, goes inside `<main>`).

```html
<app-shell page="invoices" title="Invoices">
  <template #topbar-right>
    <select class="select select-sm">...</select>
    <button class="btn btn-ghost btn-icon" data-theme-toggle>...</button>
  </template>
  <div class="toolbar">...</div>
  <div class="table-wrap">...</div>
</app-shell>
```

To add a nav entry or reorder sections, edit the `NAV` array in
`js/components/app-shell.js` — every view picks it up automatically, nothing
per-view to touch.

Each `NAV` section (`Main`, `Tax`) is collapsible — clicking the section
label toggles it, and the collapsed/expanded state per section is persisted
to `localStorage` (`fiscodesk-nav-collapsed`), the same pattern `theme.js`
uses for theme/palette. It's chrome the user sets up once, so it survives
navigation instead of resetting on every page load. Nothing to configure —
`AppShell` owns this entirely.

## `UiModal`

Centered overlay dialog. Replaces the old `data-modal-open`/`data-modal-close`
click-delegation — open/close state is just a boolean the page owns.

- **Props:** `open` (boolean), `title`.
- **Emits:** `update:open` — always pair with `v-model:open`.
- **Slots:** default (body), `footer` (optional — override the default
  Cancel/Save buttons).

```html
<ui-modal v-model:open="showAppointmentModal" title="New Tax Appointment">
  <div class="form-grid">...</div>
</ui-modal>
```

```js
data() { return { showAppointmentModal: false } }
// <button class="fab" @click="showAppointmentModal = true">New Tax Appointment</button>
```

## `UiDrawer`

Same contract as `UiModal` (props `open`/`title`, emits `update:open`,
default + `footer` slots), rendered as a right-side sliding panel instead of
a centered overlay. Use for the "create/edit a record" pattern (new invoice,
register a payment); use `UiModal` for shorter confirmations/forms.

```html
<ui-drawer v-model:open="showInvoiceDrawer" title="New Invoice">
  <div class="form-grid">...</div>
</ui-drawer>
```

## `UiTabs`

Reactive tabs. Replaces hand-wiring an `activeTab` string plus manual
`.tab`/`.tab-panel` classes per page (that's what `payments.html` did before
this existed, and still does — it predates this component).

- **Props:** `tabs` (array of `{ value, label }`), `modelValue` (the active
  tab's `value`).
- **Emits:** `update:modelValue` — pair with plain `v-model`, not
  `v-model:open`.
- **Slots:** one dynamically-named slot per tab, keyed by `value`.

```html
<ui-tabs :tabs="[{ value: 'inbound', label: 'Inbound' }, { value: 'outbound', label: 'Outbound' }]" v-model="activeTab">
  <template #inbound>...</template>
  <template #outbound>...</template>
</ui-tabs>
```

It's a plain component with no fixed positioning, so it works nested inside
`UiModal`/`UiDrawer` exactly like it does standalone — give the nested
instance its own `v-model` state, separate from any tabs on the page behind
the modal. See storybook.html's "Tabs" section for both cases live.

## Tables — `v-for`, not `data-inline-table`

Old pattern: `<td data-field="x" data-value="1">1</td>` plus ~150 lines of
`app.js` DOM-state-sync to make a row editable. New pattern: a plain JS array
in `data()`, a `v-for` row, and an `editing` field holding the key of the row
currently being edited.

```html
<tr v-for="inv in invoices" :key="inv.number">
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
```

Delete confirmation is a one-line `confirm()` in a method, not a
`data-confirm` attribute:

```js
removeInvoice(number) {
  if (confirm('Delete invoice ' + number + '?')) {
    this.invoices = this.invoices.filter(i => i.number !== number);
  }
}
```

## What's still vanilla (not Vue's job)

- **Theme + palette** (`js/theme.js`) — sets `data-theme`/`data-palette` on
  `<html>` before Vue mounts (avoids a flash of the wrong theme), and injects
  the palette-swatch selector into `.topbar-right` on `DOMContentLoaded`.
  Nothing to import; it's a plain `<script src="../js/theme.js">` on every
  page.
- **`window.FiscoDesk.formatEUR` / `.formatPct`** — also set by `theme.js`.
  Wire into a Vue app's `methods` as `formatEUR: window.FiscoDesk.formatEUR`.

## Migration status

All 9 views are converted. `AppShell`'s `href()` still supports a
`migrated: false` fallback to `_original/views/<slug>.html` (the pre-Vue
snapshot) for any future view that isn't ready yet — see
`js/components/app-shell.js`'s `NAV` array.

| View | Status |
|---|---|
| `dashboard.html` | converted — `AppShell` + `UiModal` + `UiDrawer` |
| `invoices.html` | converted — `AppShell` + `v-for` table |
| `customers.html` | converted — `AppShell` + `v-for` table |
| `ai-lab.html` | converted — `AppShell` + reactive state for `shell.ai` calls (generate/generateObject/stream/tools), replaces ~230 lines of imperative DOM updates |
| `tax-regimes.html` | converted — `AppShell` + `v-for` table, all columns editable |
| `fiscal-years.html` | converted — `AppShell` + `v-for` **card grid** (not a table — `.fy-card` with `.card-view`/`.card-edit` toggled by `v-if`/`v-else`) |
| `tax-appointments.html` | converted — `AppShell` + `v-for` table with a `computed` fiscal-year filter (replaces `data-fy`/`show-next-fy` class toggling); the calendar's prev/next only relabel the month (cosmetic, same as the original — it never regenerated the day grid) |
| `payments.html` | converted — `AppShell` + reactive tabs (`activeTab`) + two independent `v-for` tables (inbound/outbound) + one `UiModal`. Two dead modal overlays from the original (`#inbound-modal`, `#outbound-modal` — defined but never triggered by any button) were dropped rather than ported forward |
| `reports.html` | converted — `AppShell` only, no reactive state (static placeholder) |

## Examples to read, not guess from

- `storybook.html` — every component and CSS pattern, live and commented.
  Read this first.
- `index.html` — storyboard, `v-for` over a `SCREENS` array, live `<iframe>`
  previews instead of hand-drawn ones.
- `views/invoices.html` — data-heavy table, inline edit, the pattern to copy
  for any new list view.
- `views/dashboard.html` — `UiModal` + `UiDrawer` usage, FAB-triggered forms.
- `views/customers.html` — smallest possible converted view; start here for
  a simple new page.
