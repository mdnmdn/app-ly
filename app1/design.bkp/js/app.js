/* FiscoDesk — shared interactions */

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
    if (toggle) {
      const next = getTheme() === 'dark' ? 'light' : 'dark';
      setTheme(next);
    }
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
    if (swatch) {
      setPalette(swatch.dataset.paletteSwatch);
    }
  });

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initPaletteSelector);
  } else {
    initPaletteSelector();
  }

  /* Modals */
  function openModal(id) {
    const el = document.getElementById(id);
    if (el) {
      el.classList.add('open');
      el.setAttribute('aria-hidden', 'false');
    }
  }

  function closeModal(id) {
    const el = document.getElementById(id);
    if (el) {
      el.classList.remove('open');
      el.setAttribute('aria-hidden', 'true');
    }
  }

  document.addEventListener('click', (e) => {
    const openBtn = e.target.closest('[data-modal-open]');
    if (openBtn) {
      openModal(openBtn.dataset.modalOpen);
      return;
    }
    const closeBtn = e.target.closest('[data-modal-close]');
    if (closeBtn) {
      closeModal(closeBtn.dataset.modalClose);
      return;
    }
    if (e.target.classList.contains('overlay') && e.target.classList.contains('open')) {
      closeModal(e.target.id);
    }
  });

  /* Drawer */
  function openDrawer(id) {
    const overlay = document.getElementById(id + '-overlay');
    const drawer = document.getElementById(id);
    if (overlay) overlay.classList.add('open');
    if (drawer) drawer.classList.add('open');
  }

  function closeDrawer(id) {
    const overlay = document.getElementById(id + '-overlay');
    const drawer = document.getElementById(id);
    if (overlay) overlay.classList.remove('open');
    if (drawer) drawer.classList.remove('open');
  }

  document.addEventListener('click', (e) => {
    const openBtn = e.target.closest('[data-drawer-open]');
    if (openBtn) {
      openDrawer(openBtn.dataset.drawerOpen);
      return;
    }
    const closeBtn = e.target.closest('[data-drawer-close]');
    if (closeBtn) {
      closeDrawer(closeBtn.dataset.drawerClose);
      return;
    }
    if (e.target.classList.contains('drawer-overlay') && e.target.classList.contains('open')) {
      const id = e.target.id.replace('-overlay', '');
      closeDrawer(id);
    }
  });

  /* Tabs */
  document.addEventListener('click', (e) => {
    const tab = e.target.closest('[data-tab]');
    if (!tab) return;
    const group = tab.closest('[data-tab-group]');
    if (!group) return;
    const target = tab.dataset.tab;
    group.querySelectorAll('[data-tab]').forEach((t) => t.classList.toggle('active', t === tab));
    group.querySelectorAll('[data-tab-panel]').forEach((p) => {
      p.classList.toggle('active', p.dataset.tabPanel === target);
    });
  });

  /* Confirm dialog */
  document.addEventListener('click', (e) => {
    const btn = e.target.closest('[data-confirm]');
    if (btn) {
      const msg = btn.dataset.confirm || 'Are you sure?';
      if (!confirm(msg)) e.preventDefault();
    }
  });

  /* Keyboard shortcuts */
  document.addEventListener('keydown', (e) => {
    if (e.target.matches('input, textarea, select')) return;
    if (e.key === 'n' || e.key === 'N') {
      const newBtn = document.querySelector('[data-shortcut-n]');
      if (newBtn) {
        e.preventDefault();
        newBtn.click();
      }
    }
    if (e.key === 'Escape') {
      document.querySelectorAll('.overlay.open').forEach((o) => closeModal(o.id));
      document.querySelectorAll('.drawer.open').forEach((d) => closeDrawer(d.id));
    }
  });

  /* Calendar navigation */
  document.addEventListener('click', (e) => {
    const prev = e.target.closest('[data-cal-prev]');
    const next = e.target.closest('[data-cal-next]');
    const label = document.querySelector('[data-cal-label]');
    if (!label) return;
    const months = ['January','February','March','April','May','June','July','August','September','October','November','December'];
    let idx = parseInt(label.dataset.monthIdx || '5', 10);
    let year = parseInt(label.dataset.year || '2025', 10);
    if (prev) {
      idx--;
      if (idx < 0) { idx = 11; year--; }
    }
    if (next) {
      idx++;
      if (idx > 11) { idx = 0; year++; }
    }
    if (prev || next) {
      label.textContent = months[idx] + ' ' + year;
      label.dataset.monthIdx = idx;
      label.dataset.year = year;
    }
  });

  /* Format currency cells on load */
  document.querySelectorAll('[data-eur]').forEach((el) => {
    const val = parseFloat(el.dataset.eur);
    if (!isNaN(val)) el.textContent = formatEUR(val);
  });

  document.querySelectorAll('[data-pct]').forEach((el) => {
    const val = parseFloat(el.dataset.pct);
    if (!isNaN(val)) el.textContent = formatPct(val);
  });

  /* Tax appointments — fiscal year scope */
  const fyFilter = document.querySelector('[data-fy-filter]');
  const fyIncludeNext = document.querySelector('[data-fy-include-next]');
  const fyLabel = document.querySelector('[data-fy-label]');

  function applyFyFilter() {
    if (!fyFilter) return;
    const fy = fyFilter.value;
    const includeNext = fyIncludeNext && fyIncludeNext.checked;
    const nextFy = String(parseInt(fy, 10) + 1);

    if (fyLabel) fyLabel.textContent = 'FY ' + fy;
    document.body.classList.toggle('show-next-fy', includeNext);

    document.querySelectorAll('[data-fy]').forEach((row) => {
      const rowFy = row.dataset.fy;
      const visible = rowFy === fy || (includeNext && rowFy === nextFy);
      row.style.display = visible ? '' : 'none';
    });
  }

  if (fyFilter) {
    fyFilter.addEventListener('change', applyFyFilter);
    if (fyIncludeNext) fyIncludeNext.addEventListener('change', applyFyFilter);
    applyFyFilter();
  }

  /* Inline list editors */
  const BADGE_MAP = {
    Confirmed: 'badge-confirmed',
    Planned: 'badge-planned',
    Excluded: 'badge-excluded',
    Current: 'badge-current',
    Open: 'badge-open',
    Closed: 'badge-closed',
  };

  function getColumnDefs(table) {
    return [...table.querySelectorAll('thead th[data-field]')].map((th) => ({
      field: th.dataset.field,
      type: th.dataset.type || 'text',
      required: th.classList.contains('col-required'),
      options: th.dataset.options ? th.dataset.options.split('|') : [],
      readonly: th.dataset.readonly === 'true',
    }));
  }

  function cellValue(td) {
    if (!td) return '';
    if (td.dataset.value !== undefined) return td.dataset.value;
    return td.textContent.trim();
  }

  function createInlineControl(col, value) {
    if (col.readonly) {
      const span = document.createElement('span');
      span.className = 'text-muted';
      span.textContent = value || '—';
      return span;
    }

    if (col.type === 'select') {
      const sel = document.createElement('select');
      sel.className = 'select select-sm select-inline';
      if (col.required) {
        sel.required = true;
        sel.classList.add('select-required');
      }
      col.options.forEach((opt) => {
        const option = document.createElement('option');
        option.value = opt;
        option.textContent = opt;
        if (opt === value) option.selected = true;
        sel.appendChild(option);
      });
      return sel;
    }

    if (col.type === 'textarea') {
      const ta = document.createElement('textarea');
      ta.className = 'textarea textarea-inline';
      ta.value = value || '';
      if (col.required) {
        ta.required = true;
        ta.classList.add('input-required');
      }
      return ta;
    }

    const input = document.createElement('input');
    input.className = 'input input-sm input-inline';
    if (col.type === 'date') input.type = 'date';
    else if (col.type === 'number') input.type = 'number';
    else input.type = 'text';
    input.value = value || '';
    if (col.required) {
      input.required = true;
      input.classList.add('input-required');
    }
    if (col.type === 'mono') input.classList.add('text-mono');
    return input;
  }

  function renderCellDisplay(td, col, value) {
    td.dataset.value = value;
    td.className = td.className.replace(/\b(num|text-mono|text-muted)\b/g, '').trim();

    if (col.field === 'actions') return;

    if (col.field === 'status' || col.type === 'badge') {
      const cls = BADGE_MAP[value] || 'badge-planned';
      td.innerHTML = `<span class="badge ${cls}">${value}</span>`;
      return;
    }

    if (col.field === 'fy' || col.type === 'fy-badge') {
      td.innerHTML = `<span class="badge badge-fy">FY ${value}</span>`;
      return;
    }

    if (col.type === 'date') {
      if (value && /^\d{4}-\d{2}-\d{2}$/.test(value)) {
        const [y, m, d] = value.split('-');
        td.textContent = `${d}/${m}/${y}`;
      } else {
        td.textContent = value || '—';
      }
      return;
    }

    if (col.type === 'eur') {
      td.classList.add('num');
      td.dataset.eur = value;
      td.textContent = '—';
      const num = parseFloat(value);
      if (!isNaN(num)) td.textContent = formatEUR(num);
      return;
    }

    if (col.type === 'pct') {
      td.classList.add('num');
      td.dataset.pct = value;
      td.textContent = '—';
      const num = parseFloat(value);
      if (!isNaN(num)) td.textContent = formatPct(num);
      return;
    }

    if (col.type === 'mono') {
      td.classList.add('text-mono');
      td.textContent = value || '—';
      return;
    }

    if (col.field === 'name') {
      td.innerHTML = `<strong>${value}</strong>`;
      return;
    }

    if (col.type === 'textarea') {
      td.classList.add('text-muted');
      td.textContent = value;
      return;
    }

    td.textContent = value;
  }

  function toggleRowActions(row, editing) {
    const view = row.querySelector('.row-actions-view');
    const edit = row.querySelector('.row-actions-edit');
    if (view) view.hidden = editing;
    if (edit) edit.hidden = !editing;
  }

  function startRowEdit(row, cols) {
    if (row.classList.contains('is-editing')) return;
    row.classList.add('is-editing');
    row.classList.remove('has-errors');
    toggleRowActions(row, true);

    cols.forEach((col) => {
      if (col.field === 'actions' || col.readonly) return;
      const td = row.querySelector(`[data-field="${col.field}"]`);
      if (!td) return;
      const value = cellValue(td);
      td.innerHTML = '';
      td.appendChild(createInlineControl(col, value));
    });

    const first = row.querySelector('.input-inline, .select-inline, .textarea-inline');
    if (first) first.focus();
  }

  function cancelRowEdit(row, cols, table) {
    if (row.classList.contains('is-new')) {
      row.remove();
      return;
    }
    row.classList.remove('is-editing', 'has-errors');
    toggleRowActions(row, false);
    cols.forEach((col) => {
      if (col.field === 'actions' || col.readonly) return;
      const td = row.querySelector(`[data-field="${col.field}"]`);
      if (!td) return;
      renderCellDisplay(td, col, td.dataset.value ?? '');
    });
  }

  function saveRowEdit(row, cols) {
    const controls = row.querySelectorAll('.input-inline, .select-inline, .textarea-inline');
    let valid = true;
    controls.forEach((ctrl) => {
      if (!ctrl.checkValidity()) valid = false;
    });
    if (!valid) {
      row.classList.add('has-errors');
      const firstInvalid = row.querySelector(':invalid');
      if (firstInvalid) firstInvalid.focus();
      return;
    }

    row.classList.remove('is-editing', 'is-new', 'has-errors');
    toggleRowActions(row, false);

    cols.forEach((col) => {
      if (col.field === 'actions' || col.readonly) return;
      const td = row.querySelector(`[data-field="${col.field}"]`);
      if (!td) return;
      const ctrl = td.querySelector('input, select, textarea');
      const value = ctrl ? ctrl.value : cellValue(td);
      renderCellDisplay(td, col, value);
    });

    if (row.dataset.fy !== undefined) {
      const fyTd = row.querySelector('[data-field="fy"]');
      if (fyTd) row.dataset.fy = cellValue(fyTd).replace(/\D/g, '') || row.dataset.fy;
    }
  }

  function defaultViewActions(table) {
    const sample = table.querySelector('tbody tr:not(.is-new) .row-actions-view');
    if (sample) return sample.innerHTML;
    return '<button type="button" class="btn btn-sm btn-ghost" data-inline-edit>Edit</button><button type="button" class="btn btn-sm btn-danger" data-confirm="Delete?">Delete</button>';
  }

  function createNewRow(table, cols) {
    const row = document.createElement('tr');
    row.classList.add('is-new', 'is-editing');
    if (table.dataset.rowFy) row.dataset.fy = table.dataset.rowFy;

    cols.forEach((col) => {
      const td = document.createElement('td');
      td.dataset.field = col.field;
      if (col.type === 'eur' || col.type === 'pct' || col.type === 'number') td.classList.add('num');
      if (col.field === 'actions') {
        td.classList.add('actions');
        td.innerHTML = `
          <span class="row-actions-view" hidden>${defaultViewActions(table)}</span>
          <span class="row-actions-edit">
            <button type="button" class="btn btn-sm btn-primary" data-inline-save>Save</button>
            <button type="button" class="btn btn-sm" data-inline-cancel>Cancel</button>
          </span>`;
        row.appendChild(td);
        return;
      }
      if (col.readonly) {
        td.innerHTML = '<span class="text-muted">—</span>';
      } else {
        td.appendChild(createInlineControl(col, ''));
      }
      row.appendChild(td);
    });

    return row;
  }

  function showNewRow(table, cols) {
    const existing = table.querySelector('tbody tr.is-new');
    if (existing) {
      existing.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      existing.querySelector('input, select, textarea')?.focus();
      return;
    }
    const tbody = table.querySelector('tbody');
    const row = createNewRow(table, cols);
    tbody.insertBefore(row, tbody.firstChild);
    row.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    row.querySelector('input, select, textarea')?.focus();
  }

  document.querySelectorAll('[data-inline-table]').forEach((table) => {
    const cols = getColumnDefs(table);
    const tableId = table.id;

    document.querySelectorAll(`[data-inline-new="${tableId}"]`).forEach((btn) => {
      btn.addEventListener('click', (e) => {
        e.preventDefault();
        showNewRow(table, cols);
      });
    });

    table.addEventListener('click', (e) => {
      const row = e.target.closest('tbody tr');
      if (!row || !table.contains(row)) return;

      if (e.target.closest('[data-inline-edit]')) {
        e.preventDefault();
        startRowEdit(row, cols);
        return;
      }
      if (e.target.closest('[data-inline-save]')) {
        e.preventDefault();
        saveRowEdit(row, cols);
        if (table.dataset.fyFilter) applyFyFilter();
        return;
      }
      if (e.target.closest('[data-inline-cancel]')) {
        e.preventDefault();
        cancelRowEdit(row, cols, table);
      }
    });
  });

  /* Inline card editor — fiscal years */
  function toggleCardMode(card, editing) {
    const view = card.querySelector('.card-view');
    const edit = card.querySelector('.card-edit');
    if (view) view.hidden = editing;
    if (edit) edit.hidden = !editing;
    card.classList.toggle('is-editing', editing);
  }

  function saveFyCard(card) {
    const controls = card.querySelectorAll('.card-edit input, .card-edit select');
    let valid = true;
    controls.forEach((ctrl) => {
      if (!ctrl.checkValidity()) valid = false;
    });
    if (!valid) {
      card.classList.add('has-errors');
      card.querySelector(':invalid')?.focus();
      return;
    }
    card.classList.remove('is-new', 'has-errors');
    const year = card.querySelector('[data-fy-field="year"]')?.value || '';
    const regime = card.querySelector('[data-fy-field="regime"]')?.value || '';
    const start = card.querySelector('[data-fy-field="start"]')?.value || '';
    const end = card.querySelector('[data-fy-field="end"]')?.value || '';
    const status = card.querySelector('[data-fy-field="status"]')?.value || 'Open';

    if (card.classList.contains('is-new')) {
      const startFmt = start ? start.split('-').reverse().join('/') : '—';
      const endFmt = end ? end.split('-').reverse().join('/') : '—';
      card.classList.remove('is-new');
      card.innerHTML = `
        <div class="card-view">
          <div style="display:flex;justify-content:space-between;align-items:start">
            <div class="fy-year" data-fy-display="year">${year}</div>
            <span class="badge ${BADGE_MAP[status] || 'badge-open'}" data-fy-display="status">${status}</span>
          </div>
          <div class="text-sm text-muted" data-fy-display="meta">${regime} · ${startFmt} – ${endFmt}</div>
          <dl class="fy-meta">
            <dt>Taxable</dt><dd>—</dd>
            <dt>VAT</dt><dd>—</dd>
            <dt>Inarcassa</dt><dd>—</dd>
            <dt>Gross Invoiced</dt><dd>—</dd>
            <dt>Appointments</dt><dd>0 scheduled</dd>
            <dt>Next due</dt><dd>—</dd>
          </dl>
          <div class="fy-card-actions">
            <button type="button" class="btn btn-sm btn-ghost" data-inline-card-edit>Edit</button>
            <a class="btn btn-sm" href="invoices.html">Invoices</a>
            <a class="btn btn-sm btn-primary" href="tax-appointments.html">Appointments</a>
          </div>
        </div>
        <div class="card-edit" hidden>
          <div class="form-grid">
            <div class="field"><label class="required">Year</label><input class="input" data-fy-field="year" type="number" value="${year}" required /></div>
            <div class="field"><label class="required">Status</label><select class="select" data-fy-field="status" required><option${status === 'Open' ? ' selected' : ''}>Open</option><option${status === 'Closed' ? ' selected' : ''}>Closed</option></select></div>
            <div class="field span-2"><label class="required">Tax Regime</label><select class="select" data-fy-field="regime" required><option selected>${regime}</option><option>Forfettario 2025</option><option>Ordinario 2025</option></select></div>
            <div class="field"><label class="required">Start Date</label><input class="input" data-fy-field="start" type="date" value="${start}" required /></div>
            <div class="field"><label class="required">End Date</label><input class="input" data-fy-field="end" type="date" value="${end}" required /></div>
          </div>
          <div class="card-edit-actions">
            <button type="button" class="btn btn-sm btn-primary" data-inline-card-save>Save</button>
            <button type="button" class="btn btn-sm" data-inline-card-cancel>Cancel</button>
          </div>
        </div>`;
      return;
    }

    card.querySelector('[data-fy-display="year"]').textContent = year;
    const meta = card.querySelector('[data-fy-display="meta"]');
    if (meta) {
      const startFmt = start ? start.split('-').reverse().join('/') : '—';
      const endFmt = end ? end.split('-').reverse().join('/') : '—';
      meta.textContent = `${regime} · ${startFmt} – ${endFmt}`;
    }
    const badge = card.querySelector('[data-fy-display="status"]');
    if (badge) {
      badge.textContent = status;
      badge.className = `badge ${BADGE_MAP[status] || 'badge-open'}`;
    }
    toggleCardMode(card, false);
  }

  document.querySelectorAll('[data-inline-cards]').forEach((grid) => {
    const gridId = grid.id;

    document.querySelectorAll(`[data-inline-new="${gridId}"]`).forEach((btn) => {
      btn.addEventListener('click', (e) => {
        e.preventDefault();
        let draft = grid.querySelector('.fy-card.is-new');
        if (draft) {
          draft.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
          draft.querySelector('input')?.focus();
          return;
        }
        draft = document.createElement('article');
        draft.className = 'fy-card is-new is-editing';
        draft.innerHTML = `
          <div class="card-edit">
            <div class="form-grid">
              <div class="field"><label class="required">Year</label><input class="input" data-fy-field="year" type="number" value="2026" required /></div>
              <div class="field"><label class="required">Status</label><select class="select" data-fy-field="status" required><option>Open</option><option>Closed</option></select></div>
              <div class="field span-2"><label class="required">Tax Regime</label><select class="select" data-fy-field="regime" required><option>Forfettario 2025</option><option>Ordinario 2025</option></select></div>
              <div class="field"><label class="required">Start Date</label><input class="input" data-fy-field="start" type="date" value="2026-01-01" required /></div>
              <div class="field"><label class="required">End Date</label><input class="input" data-fy-field="end" type="date" value="2026-12-31" required /></div>
            </div>
            <div class="card-edit-actions">
              <button type="button" class="btn btn-sm btn-primary" data-inline-card-save>Save</button>
              <button type="button" class="btn btn-sm" data-inline-card-cancel>Cancel</button>
            </div>
          </div>`;
        grid.insertBefore(draft, grid.firstChild);
        draft.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
        draft.querySelector('input')?.focus();
      });
    });

    grid.addEventListener('click', (e) => {
      const card = e.target.closest('.fy-card');
      if (!card || !grid.contains(card)) return;

      if (e.target.closest('[data-inline-card-edit]')) {
        e.preventDefault();
        toggleCardMode(card, true);
        card.querySelector('.card-edit input, .card-edit select')?.focus();
        return;
      }
      if (e.target.closest('[data-inline-card-save]')) {
        e.preventDefault();
        saveFyCard(card);
        return;
      }
      if (e.target.closest('[data-inline-card-cancel]')) {
        e.preventDefault();
        if (card.classList.contains('is-new')) {
          card.remove();
        } else {
          card.classList.remove('has-errors');
          toggleCardMode(card, false);
        }
      }
    });
  });
})();