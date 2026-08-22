// js/storybook/story-nav.js — storybook-only sticky table-of-contents.
// Reads its list from the actual rendered `.story-section` elements (see
// storybook.html's `mounted()`) instead of a hand-maintained array, so the
// nav can never drift out of sync with the sections that really exist.

export const StoryNav = {
  props: ['sections'],
  template: `
    <nav class="story-nav" aria-label="Storybook sections">
      <a v-for="s in sections" :key="s.id" :href="'#' + s.id" class="story-nav-link">{{ s.title }}</a>
    </nav>
  `,
};
