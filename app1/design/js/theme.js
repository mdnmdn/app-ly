/* FiscoDesk — theme/palette + format helpers. Vanilla, loaded on every page
   (Vue or not). Not Vue's job: this runs before Vue mounts, to avoid a
   flash of the wrong theme. See design/COMPONENTS.md. */

(function () {
  'use strict';

  const STORAGE_THEME = 'fiscodesk-theme';
  const STORAGE_PALETTE = 'fiscodesk-palette';

  const PALETTES = [
    { id: 'default', label: 'Default', color: 'oklch(58% 0.16 145)' },
    { id: 'purple', label: 'Purple', color: 'oklch(52% 0.22 305)' },
    { id: 'cobalt', label: 'Cobalt', color: 'oklch(48% 0.17 264)' },
  ];

  const formatEUR = (value) =>
    new Intl.NumberFormat('it-IT', {
      style: 'currency',
      currency: 'EUR',
    }).format(value);

  const formatPct = (value) =>
    new Intl.NumberFormat('it-IT', {
      style: 'percent',
      minimumFractionDigits: 0,
      maximumFractionDigits: 1,
    }).format(value / 100);

  window.FiscoDesk = { formatEUR, formatPct };

  /* Theme */
  function getTheme() {
    return localStorage.getItem(STORAGE_THEME) ||
      (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  }

  function setTheme(theme) {
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem(STORAGE_THEME, theme);
    document.querySelectorAll('[data-theme-toggle]').forEach((btn) => {
      btn.setAttribute('aria-pressed', theme === 'dark');
      const label = btn.querySelector('.theme-label');
      if (label) label.textContent = theme === 'dark' ? 'Light' : 'Dark';
    });
  }

  document.documentElement.setAttribute('data-theme', getTheme());

  document.addEventListener('click', (e) => {
    const toggle = e.target.closest('[data-theme-toggle]');
    if (toggle) setTheme(getTheme() === 'dark' ? 'light' : 'dark');
  });

  /* Palette */
  function getPalette() {
    return localStorage.getItem(STORAGE_PALETTE) || 'default';
  }

  function setPalette(palette) {
    const valid = PALETTES.some((p) => p.id === palette);
    const next = valid ? palette : 'default';
    if (next === 'default') {
      document.documentElement.removeAttribute('data-palette');
    } else {
      document.documentElement.setAttribute('data-palette', next);
    }
    localStorage.setItem(STORAGE_PALETTE, next);
    document.querySelectorAll('[data-palette-swatch]').forEach((btn) => {
      btn.setAttribute('aria-pressed', btn.dataset.paletteSwatch === next);
    });
  }

  function buildPaletteSelector() {
    const el = document.createElement('div');
    el.className = 'palette-selector';
    el.setAttribute('role', 'group');
    el.setAttribute('aria-label', 'Color theme');

    const label = document.createElement('span');
    label.className = 'palette-selector-label';
    label.textContent = 'Theme';
    el.appendChild(label);

    PALETTES.forEach((palette) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'palette-swatch';
      btn.dataset.paletteSwatch = palette.id;
      btn.title = palette.label;
      btn.setAttribute('aria-label', palette.label);
      btn.setAttribute('aria-pressed', palette.id === getPalette());

      const dot = document.createElement('span');
      dot.className = 'palette-swatch-dot';
      dot.style.setProperty('--swatch-color', palette.color);
      btn.appendChild(dot);
      el.appendChild(btn);
    });

    return el;
  }

  function initPaletteSelector() {
    const current = getPalette();
    if (current !== 'default') {
      document.documentElement.setAttribute('data-palette', current);
    }

    const anchors = [
      ...document.querySelectorAll('.topbar-right'),
      ...document.querySelectorAll('.storyboard-header > div > div:last-child'),
    ].filter((node) => !node.querySelector('.palette-selector'));

    anchors.forEach((container) => {
      const toggle = container.querySelector('[data-theme-toggle]');
      const selector = buildPaletteSelector();
      if (toggle) {
        container.insertBefore(selector, toggle);
      } else {
        container.appendChild(selector);
      }
    });
  }

  document.addEventListener('click', (e) => {
    const swatch = e.target.closest('[data-palette-swatch]');
    if (swatch) setPalette(swatch.dataset.paletteSwatch);
  });

  /* Runs on DOMContentLoaded, which (per spec) fires after deferred/module
     scripts have executed — so on Vue pages the sidebar/topbar are already
     rendered by the time this looks for `.topbar-right` to anchor into. */
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initPaletteSelector);
  } else {
    initPaletteSelector();
  }

  /* Keyboard: "N" focuses/clicks the page's primary "new" action, if any */
  document.addEventListener('keydown', (e) => {
    if (e.target.matches('input, textarea, select')) return;
    if (e.key === 'n' || e.key === 'N') {
      const newBtn = document.querySelector('[data-shortcut-n]');
      if (newBtn) {
        e.preventDefault();
        newBtn.click();
      }
    }
  });
})();
