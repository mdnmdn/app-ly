// js/storybook/story-section.js — storybook-only wrapper, NOT part of the
// design system (don't import this into a real view). Exists purely to
// avoid repeating the same heading/anchor/description markup ~20 times in
// storybook.html. Every section gets an id (deep-linkable), a title, and a
// `desc` slot explaining what the pattern is for and when to reach for it —
// the demo + code sample go in the default slot.

export const StorySection = {
  props: ['id', 'title'],
  template: `
    <section :id="id" class="story-section">
      <div class="story-heading">
        <h2>{{ title }}</h2>
        <a class="story-anchor" :href="'#' + id" aria-label="Link to this section">#</a>
      </div>
      <p class="story-desc text-sm text-muted"><slot name="desc" /></p>
      <slot />
    </section>
  `,
};
