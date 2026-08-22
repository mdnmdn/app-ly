// js/components/ui-modal.js — v-model:open replaces app.js's
// data-modal-open/data-modal-close delegation. See COMPONENTS.md.

export const UiModal = {
  props: ['open', 'title'],
  emits: ['update:open'],
  template: `
    <div class="overlay" :class="{ open }" @click.self="$emit('update:open', false)">
      <div class="modal">
        <div class="modal-header">
          <h2>{{ title }}</h2>
          <button class="btn btn-ghost btn-icon" @click="$emit('update:open', false)">✕</button>
        </div>
        <div class="modal-body"><slot /></div>
        <div class="modal-footer">
          <slot name="footer">
            <button class="btn" @click="$emit('update:open', false)">Cancel</button>
            <button class="btn btn-primary" @click="$emit('update:open', false)">Save</button>
          </slot>
        </div>
      </div>
    </div>
  `,
};
