# Sidekick Tab Style Unification Design

Date: 2026-04-03
Repo: `<repo-root>`
Status: Draft approved for spec write-up

## Summary

Unify the visual language of `Sidekick` navigation-like UI so the left rail tabs, pane headers, grouped section headers, and action-adjacent navigation surfaces all read as one system instead of several unrelated styles. The user selected the restrained list-style direction anchored on the existing left sidebar tabs rather than a new pill-tab treatment. The implementation should preserve the current app structure and behavior while consolidating styling tokens, shared header primitives, and active/hover states across `Jobs`, `MCP`, `Skills`, `Settings`, and `Users`.

## Goals

- Make navigation-like UI across the frontend feel visually consistent.
- Use the existing left rail `.tab-btn` language as the primary reference.
- Apply the approved `brand-guidelines` palette and hierarchy:
  - `#141413` dark ink
  - `#faf9f5` light paper
  - `#e8e6dc` subtle surfaces and separators
  - `#b0aea5` muted text and borders
  - `#d97757` primary emphasis
- Reduce duplicated one-off styling for headers, small action buttons, and section wrappers.
- Keep the current interaction model, DOM hierarchy, and tab switching behavior low-risk.
- Ensure light theme, dark theme, collapsed sidebar, and smaller screens remain coherent after the visual cleanup.

## Non-Goals

- Do not redesign the information architecture or add new tabs.
- Do not convert every content card into a tab-styled component.
- Do not change `switchTab()` behavior or introduce nested tab state.
- Do not rewrite pane layouts such as jobs lists, admin user cards, or forms unless a small class/markup adjustment is needed to share styles.
- Do not introduce a heavy, bright pill-tab visual system.
- Do not perform a broad typography redesign beyond limited heading/nav harmonization needed for consistency.

## Current State

The frontend already uses a warm editorial palette, but navigation-adjacent UI is fragmented:

- The left rail `.tab-btn` elements use a compact, low-chroma list treatment with subtle hover and active states.
- `Jobs` and `MCP` use a different top header language driven by `.jobs-header` and icon-only buttons.
- `Skills`, `Settings`, and `Users` use multiple independent header/group patterns:
  - `.control-panel-header`
  - `.skills-group-header`
  - `.skills-detail-header`
  - `.settings-section-header`
  - `.settings-channel-group-header`
  - `.users-pane-header`
- Action buttons in these areas do not share a single radius, border, hover fill, or emphasis model.
- Some section containers look like muted cards, while others read like independent panels or pseudo-tabs.
- Theme overrides repeat similar intent in several places instead of drawing from a smaller shared token set.

The result is that moving between panes feels like switching between several mini-products rather than one coherent interface.

## Desired State

### Visual Principle

The application should use one navigation family with two hierarchy levels:

- **Primary navigation**: the left sidebar tabs
- **Secondary navigation/section framing**: pane headers, subsection headers, grouped navigation-like surfaces inside panes

Both levels should look related, but the primary sidebar remains visually stronger. Secondary headers should feel like calm structural signposts, not like another top-level tab bar competing for attention.

### Chosen Direction

The approved direction is the restrained list-style system derived from the current left rail, not a pill-tab redesign. That means:

- emphasis comes from spacing, border contrast, and sparse accent usage
- active states use low-intensity fills and text emphasis rather than saturated blocks
- small controls remain quiet until hover/focus/active
- orange remains the single primary emphasis color for current state and important actions

### Tokenized Navigation System

Shared styling should be expressed through a small set of navigation-oriented tokens layered on top of the existing palette, such as:

- item radius
- item horizontal and vertical padding
- neutral hover background
- active background
- active text color
- soft border color
- stronger border color
- quiet icon color
- quiet section surface

Exact variable names can follow current stylesheet conventions, but the design requires navigation-adjacent components to draw from the same tokens rather than local hard-coded values.

### Typography and Hierarchy

- Major section titles should align more closely with the brand-guideline heading family where practical, using `Poppins` with fallback to `Arial`.
- Body copy and form content should preserve current readability.
- Navigation labels and metadata-like labels may keep a restrained utility/mono feel where already established, but size, letter spacing, and weight should be normalized so they feel intentional rather than incidental.
- Kicker + title groupings should follow one hierarchy pattern across panes.

### Scope of Unification

The following should be unified into the shared navigation/section family:

- left rail `.tab-btn`
- `.jobs-header`
- `.control-panel-header`
- `.skills-group-header`
- `.skills-detail-header`
- `.settings-section-header`
- `.settings-channel-group-header`
- `.users-pane-header`
- adjacent small header actions such as refresh/create/toggle buttons when they visually belong to the section frame

The following should **not** be forcibly restyled into tab-like elements:

- `job-item`
- `admin-user-card`
- general content cards
- regular forms and input controls
- transcript/messages

These remain content surfaces, not navigation primitives.

## Design Approach

### 1. Establish Shared Section Primitives

Introduce a small set of semantic utility classes in `frontend/index.html` so multiple panes can share a consistent frame:

- `section-header`
- `section-title-group`
- `section-eyebrow`
- `section-action`
- `section-surface`

These classes should complement, not replace, existing pane-specific classes. Pane-specific selectors can remain where needed for spacing or layout, but the visual language should come primarily from the shared primitives.

### 2. Normalize Navigation States

Unify hover, focus, and active behavior across primary and secondary navigation-like controls:

- hover uses a faint neutral/orange wash
- active uses stronger text emphasis and a paper-like inset background
- focus remains clearly visible and theme-safe
- icon treatment follows the same color/intensity model as labels

The left rail active state remains the reference. Secondary section controls should inherit the same family while staying slightly quieter.

### 3. Align Header Composition

Pane and subsection headers should consistently use:

- a small kicker/eyebrow line
- a clear title
- optional trailing action area

This structure should apply to `Jobs`, `MCP`, `Skills`, `Settings`, and `Users` wherever the header is acting as a section anchor. A user moving from pane to pane should see the same grammar even when the content beneath differs.

### 4. Reconcile Surface Treatments

Some panes currently use full panel cards, some use lighter grouped boxes, and some use nearly transparent containers. These should be rationalized into a small number of surfaces:

- app shell / pane background
- section surface
- inner grouped surface
- content card

The navigation unification should mainly affect the first three layers so section framing reads consistently before the user evaluates business content.

### 5. Keep Behavior Stable

This pass should avoid JavaScript behavior changes unless a very small accessibility or state hook is needed. In particular:

- keep existing tab switching logic
- keep current hidden/display pane behavior
- keep admin-only `Users` tab logic
- keep collapsed sidebar behavior
- keep current responsive flow and mobile menu interactions

## Files and Change Scope

Primary files:

- `<repo-root>/frontend/src/style.css`
- `<repo-root>/frontend/index.html`

Expected work inside `frontend/src/style.css`:

- add or expand shared navigation/section tokens
- update `.tab-btn` to reference shared values
- align header/button treatments for pane headers
- normalize section/group wrappers for `Skills`, `Settings`, and `Users`
- reduce duplicated light/dark overrides where the same intent can be tokenized

Expected work inside `frontend/index.html`:

- add shared semantic classes to pane headers and grouped section wrappers
- keep existing DOM structure mostly intact
- avoid structural rewrites unless needed to expose a consistent title/action grouping

JavaScript impact:

- none expected beyond incidental selector safety if a shared class is introduced for styling only

## Theme Behavior

The unified system must hold in both themes:

- light theme should preserve the paper-and-ink feel with subtle separators
- dark theme should preserve the same hierarchy instead of reverting to a separate styling language
- active states should remain identifiable in icon-only collapsed sidebar mode
- the same token intent should exist across base, explicit theme, and fallback dark-mode styling

## Risks

- Over-unifying could make content cards look like navigation controls if the scope boundary is not respected.
- Some secondary headers may become too subtle if active/hover values are made quieter without compensating text contrast.
- Dark theme may regress if legacy overrides remain outside the new token path.
- Adding shared semantic classes without discipline could create duplicate styling paths instead of eliminating them.
- Collapsed sidebar and narrow-screen states may expose padding/alignment issues if the shared token set assumes full-width labels.

## Validation

The unification is complete when all of the following are true:

- The left rail tabs and pane-level section headers clearly belong to the same design family.
- `Jobs`, `MCP`, `Skills`, `Settings`, and `Users` no longer feel like they use separate header systems.
- Active, hover, and focus states are consistent across navigation-like controls.
- Light and dark theme both preserve the same structural hierarchy.
- Collapsed sidebar still communicates active state clearly.
- No business logic or navigation behavior changes are required to support the visual unification.

## Verification Plan

- Run the frontend test suite for `frontend` if covered components exist.
- Run the frontend production build for `frontend`.
- Manually inspect:
  - sidebar expanded
  - sidebar collapsed
  - `Jobs`
  - `MCP`
  - `Skills`
  - `Settings`
  - admin `Users` pane
  - light theme
  - dark theme

## Open Questions

- None for the design itself. The user selected the restrained sidebar-derived direction and approved the unification boundary of navigation-like surfaces rather than all cards.
