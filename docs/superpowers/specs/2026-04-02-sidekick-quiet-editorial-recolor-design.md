# Sidekick Quiet Editorial Recolor Design

Date: 2026-04-02
Repo: `<repo-root>`
Status: Draft approved for spec write-up

## Summary

Recolor the `Sidekick` frontend to follow an Anthropic-inspired "Quiet Editorial" direction. The UI should shift from its current warm-orange product styling toward a restrained neutral system built on Anthropic's dark ink, light paper, and soft gray surfaces, with orange reserved for primary emphasis and blue/green used only for semantic support. The change applies across the login page, main app shell, light theme, dark theme, syntax highlighting, and hard-coded accent usages, while preserving the existing layout, information architecture, and interactions.

## Goals

- Align the frontend palette with the Anthropic brand colors already documented for `brand-guidelines`.
- Implement the user-selected "Quiet Editorial" variant:
  - deep neutral surfaces and text
  - minimal accent usage
  - subdued gradients and hover states
- Unify light theme, dark theme, and code highlighting under one coherent token system.
- Replace scattered hard-coded colors with reusable design tokens where practical.
- Remove visually off-brand accents such as the existing purple assistant avatar treatment and overly bright status colors.
- Keep the current structure and behavior intact so the recolor remains low-risk.

## Non-Goals

- Do not redesign the layout or change navigation structure.
- Do not alter application behavior, data flow, or JavaScript interaction logic.
- Do not add a new theme system or runtime theme customization.
- Do not replace typography families in this pass unless a small heading or login-page text tweak is needed to match the new visual tone.
- Do not attempt a full accessibility audit beyond preserving or improving the current contrast and focus visibility.

## Current State

- The frontend styling lives primarily in `<repo-root>/frontend/src/style.css`.
- The current palette is already warm and editorial-adjacent, but it mixes:
  - custom warm neutrals
  - orange emphasis values
  - GitHub-like success greens
  - bright blues
  - purple gradients
  - independent highlight.js token colors
- Theme handling is split between:
  - base `:root` values
  - `:root[data-theme="light"]`
  - `:root[data-theme="dark"]`
  - `@media (prefers-color-scheme: dark)` fallback styling
- Many components still use inline hard-coded `rgba(...)` and hex values instead of drawing from a complete token system.

## Desired Visual Direction

### Core Mood

The application should feel like a calm private workspace rather than a colorful dashboard. Surfaces should read as paper, stone, and ink, with emphasis conveyed through spacing, border contrast, and sparse accent use rather than large blocks of saturated color.

### Brand Palette

Primary neutrals:

- `#141413` for primary text and deepest dark surfaces
- `#faf9f5` for the main paper background
- `#e8e6dc` for subtle backgrounds and separators
- `#b0aea5` for muted borders and secondary metadata

Controlled accents:

- `#d97757` as the primary emphasis color
- `#6a9bcc` as a restrained informational secondary color
- `#788c5d` as a restrained success/positive color

Supporting semantic colors may derive from those anchors, but the palette should avoid introducing new unrelated hues unless required for error visibility.

## Design Approach

### Token Expansion

The current root variables are not sufficient to normalize all the hard-coded styling. Expand the token set so the stylesheet can consistently express:

- page background
- panel background
- softer inset panel background
- sidebar background
- input background
- default border
- stronger border
- primary text
- secondary text
- muted text
- primary accent
- stronger accent state
- info
- success
- warning
- danger
- code background

Representative token naming can remain simple, but the final design must make it easy to trace any repeated UI color back to a shared variable instead of local one-off values.

### Light Theme

- Keep the page background close to `#faf9f5`, with only a very subtle neutral gradient if needed.
- Use softer gray-beige surface shifts instead of the current warmer peach-tinted emphasis.
- Reduce colored hover fills to faint neutral or very light orange washes.
- Ensure active tabs, selected items, and focus rings are visible, but quieter than the current implementation.

### Dark Theme

- Treat dark mode as the same product in lower light, not as a separate reddish-brown skin.
- Use a deep ink/brown-black family anchored closer to Anthropic's dark color than to the current orange-heavy dark theme.
- Preserve sufficient contrast for text and borders while keeping accents sparse.
- Mirror the same hierarchy as light mode: neutral surfaces first, accent only for important actions and current state.

### Login Page

- Keep the login page distinct enough to feel intentional, but tone down decorative energy.
- Retain a subtle atmospheric gradient or radial wash so the page does not become flat.
- Reserve the orange accent for the kicker, primary action, and focus states.
- If the current login headline or supporting copy feels too "control plane marketing" for the quieter tone, lightly edit the wording in `frontend/index.html` without changing the structure.

### App Shell and Panels

- Recolor the sidebar, session list, settings cards, skills editor, channel panels, MCP cards, composer, and chat transcript to share the same neutral hierarchy.
- Use surface layering and border contrast to separate regions rather than highly tinted backgrounds.
- Active and selected states should be distinguishable by a mix of text color, border emphasis, and faint background tint rather than bold colored slabs.

### Message and Trace Surfaces

- Remove the current purple assistant avatar gradient and replace it with a treatment aligned to the neutral/quiet editorial system.
- Normalize chat bubbles, trace blocks, tool output sections, badges, and code containers so they look like part of the same system.
- Keep tool and status distinctions readable, but reduce ornamental color usage.

### Syntax Highlighting

Syntax highlighting should align with the Anthropic palette rather than a generic IDE preset:

- keywords: orange-led emphasis
- strings and names: green-led emphasis
- attributes and tags: blue-led emphasis
- comments: muted gray
- numbers and literals: restrained warm tone or subdued complementary variant

The result should remain readable in both light and dark themes while feeling visually quieter than the current highlight set.

### Semantic Status Colors

Semantic colors should be limited and purposeful:

- success uses the green family
- informational or metadata accents use the blue family
- warning uses a softened amber/ochre tied to the neutral system
- destructive states keep a red family, but one that does not overpower the palette

These colors should be applied only to true semantic states such as status dots, badges, destructive actions, warnings, and errors.

## Scope of Code Changes

Primary files:

- `<repo-root>/frontend/src/style.css`

Possible secondary file:

- `<repo-root>/frontend/index.html`

Expected change areas inside the stylesheet:

- root theme variables
- login shell and login card
- sidebar and tab states
- session list and session channel badges
- settings, channels, jobs, MCP, and skills panels
- conversation transcript surfaces
- composer and slash menu
- tool output and trace UI
- avatar treatments
- code and highlight.js colors
- dark theme overrides and system dark fallback

## Implementation Notes

- Prefer replacing repeated hard-coded colors with tokens before tuning individual components.
- Keep selectors and structure intact where possible to minimize regression risk.
- When only one or two isolated elements use a color, convert them to semantic tokens if they represent meaning; otherwise merge them into the neutral hierarchy.
- Avoid introducing a second accent color into primary call-to-action patterns.
- If a color currently encodes real meaning, preserve that meaning even if the hue shifts.

## Risks

- Some low-contrast secondary text or border values may become too faint after the palette is muted.
- Light and dark themes may drift if token substitutions are incomplete and legacy hard-coded colors remain behind.
- Code highlighting may lose clarity if the colors become too subdued, especially in dark mode.
- Small elements such as status badges and delete buttons may look under-emphasized if semantic colors are over-normalized.

## Validation

The recolor is complete when all of the following are true:

- The login page, app shell, settings areas, skills editor, chat transcript, and MCP/channel panels all read as one coherent visual system.
- Light and dark themes feel like tonal variants of the same product.
- No obviously off-brand purple, bright neon green, or unrelated highlight palette remains.
- Primary actions and active states remain easy to identify.
- Error, warning, success, and informational states remain distinguishable.
- Code blocks and tool output remain readable in both themes.

## Verification Plan

- Run the frontend tests for `frontend`.
- Run the frontend production build for `frontend`.
- Manually inspect the main screens in both light and dark theme if the local preview path is available.

## Open Questions

- None for the design itself. The user explicitly selected the "Quiet Editorial" direction and approved the implementation scope of light theme, dark theme, syntax highlighting, and login-page tone adjustments.
