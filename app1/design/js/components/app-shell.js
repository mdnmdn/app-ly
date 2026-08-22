// js/components/app-shell.js — sidebar nav + topbar, one file, used by every view.
// Options API only (no <script setup> — see COMPONENTS.md). Import into a
// view's own Vue app via `components: { AppShell }`, don't edit this file
// to build a new page — see COMPONENTS.md + views/invoices.html instead.

export const NAV = [
  {
    section: 'Main',
    items: [
      { slug: 'dashboard', label: 'Dashboard', migrated: true,
        icon: '<rect x="2" y="2" width="6" height="6" rx="1"/><rect x="10" y="2" width="6" height="6" rx="1"/><rect x="2" y="10" width="6" height="6" rx="1"/><rect x="10" y="10" width="6" height="6" rx="1"/>' },
      { slug: 'invoices', label: 'Invoices', migrated: true,
        icon: '<path d="M4 2h10l2 2v12H4V2z"/><path d="M6 6h6M6 9h6M6 12h4"/>' },
      { slug: 'customers', label: 'Customers', migrated: true,
        icon: '<circle cx="9" cy="6" r="3"/><path d="M3 16c0-3.3 2.7-6 6-6s6 2.7 6 6"/>' },
      { slug: 'ai-lab', label: 'AI Lab', migrated: true,
        icon: '<path d="M9 2l1.6 4.4L15 8l-4.4 1.6L9 14l-1.6-4.4L3 8l4.4-1.6L9 2z"/><path d="M14 12l.7 1.8L16.5 14.5l-1.8.7L14 17l-.7-1.8-1.8-.7 1.8-.7L14 12z"/>' },
    ],
  },
  {
    section: 'Tax',
    items: [
      { slug: 'tax-regimes', label: 'Tax Regimes', migrated: true,
        icon: '<path d="M3 4h12M3 8h12M3 12h8"/>' },
      { slug: 'fiscal-years', label: 'Fiscal Years', migrated: true,
        icon: '<rect x="3" y="4" width="12" height="11" rx="1"/><path d="M3 8h12M6 2v4M12 2v4"/>' },
      { slug: 'tax-appointments', label: 'Tax Appointments', migrated: true,
        icon: '<rect x="3" y="4" width="12" height="11" rx="1"/><path d="M3 8h12M7 11h2M11 11h2"/>' },
      { slug: 'payments', label: 'Payments', migrated: true,
        icon: '<path d="M2 6h14v8H2z"/><path d="M2 9h14M5 12h4"/>' },
      { slug: 'reports', label: 'Reports', migrated: true, disabled: true,
        icon: '<path d="M3 14V8M7 14V4M11 14V10M15 14V6"/>' },
    ],
  },
];

const COLLAPSED_KEY = 'fiscodesk-nav-collapsed';

function loadCollapsed() {
  try {
    return JSON.parse(localStorage.getItem(COLLAPSED_KEY)) || {};
  } catch {
    return {};
  }
}

export const AppShell = {
  props: ['page', 'title'],
  data() {
    // Collapsed state is keyed by section name, not page — it's chrome the
    // user sets up once, so it's persisted (same idea as theme/palette in
    // theme.js) and stays put across navigation instead of resetting on
    // every page load.
    return { nav: NAV, collapsed: loadCollapsed() };
  },
  methods: {
    // Converted views live at views/<slug>.html; anything not migrated yet
    // still only exists in the pre-Vue snapshot under _original/.
    href(item) {
      return item.migrated ? item.slug + '.html' : '../_original/views/' + item.slug + '.html';
    },
    toggleSection(section) {
      this.collapsed = { ...this.collapsed, [section]: !this.collapsed[section] };
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify(this.collapsed));
    },
  },
  template: `
    <aside class="sidebar">
      <div class="sidebar-brand">
        <div class="sidebar-logo">FD</div>
        <div><div class="sidebar-title">FiscoDesk</div><div class="sidebar-sub">Invoice & Tax</div></div>
      </div>
      <template v-for="s in nav" :key="s.section">
        <button
          type="button"
          class="nav-section"
          :aria-expanded="!collapsed[s.section]"
          @click="toggleSection(s.section)"
        >
          <span>{{ s.section }}</span>
          <svg class="nav-section-chevron" :class="{ collapsed: collapsed[s.section] }" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M3 4.5 6 7.5 9 4.5"/></svg>
        </button>
        <ul class="nav-list" v-show="!collapsed[s.section]">
          <li v-for="item in s.items" :key="item.slug">
            <a class="nav-link" :class="{ active: item.slug === page, disabled: item.disabled }" :href="href(item)">
              <svg class="nav-icon" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.5" v-html="item.icon"></svg>{{ item.label }}
            </a>
          </li>
        </ul>
      </template>
    </aside>
    <header class="topbar">
      <div class="topbar-left"><h1>{{ title }}</h1></div>
      <div class="topbar-right"><slot name="topbar-right" /></div>
    </header>
    <main class="main"><slot /></main>
  `,
};
