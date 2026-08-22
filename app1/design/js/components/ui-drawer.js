// js/components/ui-drawer.js — v-model:open replaces app.js's
// data-drawer-open/data-drawer-close delegation. See COMPONENTS.md.

export const UiDrawer = {
  props: ['open', 'title'],
  emits: ['update:open'],
  template: `
    <div class="drawer-overlay" :class="{ open }" @click="$emit('update:open', false)"></div>
    <aside class="drawer" :class="{ open }">
      <div class="drawer-header">
        <h2>{{ title }}</h2>
        <button class="btn btn-ghost btn-icon" @click="$emit('update:open', false)">✕</button>
      </div>
      <div class="drawer-body"><slot /></div>
      <div class="drawer-footer">
        <slot name="footer">
          <button class="btn" @click="$emit('update:open', false)">Cancel</button>
          <button class="btn btn-primary" @click="$emit('update:open', false)">Save</button>
        </slot>
      </div>
    </aside>
  `,
};
