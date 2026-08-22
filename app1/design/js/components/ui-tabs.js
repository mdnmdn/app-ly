// js/components/ui-tabs.js — reactive tabs. Replaces hand-wiring an
// `activeTab` string plus manual `.tab`/`.tab-panel` classes per page (see
// payments.html for what that looked like before this existed). See
// COMPONENTS.md and storybook.html's "Tabs" section for the real usage
// example, including nesting one inside UiModal.
//
// `.tab-panel` defaults to `display: none` in common.css — only `.active`
// flips it to `display: block`. This component always binds BOTH the
// `active` class and `v-show`: v-show alone clears its own inline style
// when true and silently falls back to that CSS default, which looks like
// the panel is stuck hidden. Don't drop either binding.
//
// A plain component with no fixed positioning of its own, so it works
// nested inside UiModal/UiDrawer exactly like it does standalone.

export const UiTabs = {
  props: ['tabs', 'modelValue'],
  emits: ['update:modelValue'],
  template: `
    <div class="ui-tabs">
      <div class="tabs">
        <button
          v-for="t in tabs"
          :key="t.value"
          type="button"
          class="tab"
          :class="{ active: modelValue === t.value }"
          @click="$emit('update:modelValue', t.value)"
        >{{ t.label }}</button>
      </div>
      <div
        v-for="t in tabs"
        :key="t.value"
        class="tab-panel"
        :class="{ active: modelValue === t.value }"
        v-show="modelValue === t.value"
      >
        <slot :name="t.value" />
      </div>
    </div>
  `,
};
