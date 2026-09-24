---
version: alpha
name: Arbiter
description: A compact developer workbench for supervising coding agents.
colors:
  background: "oklch(0.16 0.005 260)"
  panel: "oklch(0.19 0.006 260)"
  raised: "oklch(0.23 0.008 260)"
  border: "oklch(0.29 0.008 260)"
  text: "oklch(0.93 0.005 260)"
  secondary: "oklch(0.68 0.01 260)"
  muted: "oklch(0.5 0.01 260)"
  primary: "oklch(0.72 0.14 265)"
  success: "oklch(0.74 0.15 155)"
  warning: "oklch(0.8 0.14 80)"
  danger: "oklch(0.68 0.19 25)"
typography:
  sans:
    fontFamily: 'Inter, ui-sans-serif, system-ui, "Segoe UI", sans-serif'
    fontSize: "13px"
  mono:
    fontFamily: '"JetBrains Mono", "Cascadia Code", ui-monospace, Consolas, monospace'
rounded:
  DEFAULT: "6px"
  panel: "8px"
spacing:
  toolbar: "12px"
  panel: "16px"
  reading-width: "48rem"
components:
  Button:
    height: "28px"
  Input:
    height: "32px"
---

# Arbiter design system

## Overview

Arbiter is a product workbench for a developer supervising local agents on a laptop. The reference is an IDE's activity and changes panels: compact toolbars, restrained borders, legible code excerpts, and space reserved for the app being built. Product requirements are in `docs/PLAN.md`.

The existing English desktop UI is the visual baseline. No market-specific locale behavior is implied. Its signature is the connection between a selected preview element, a small text descriptor, and the agent's message composer. Keep controls familiar; avoid marketing heroes, decorative dashboards, gradients, or new typography for individual features.

## Colors

`apps/desktop/src/index.css` is the canonical runtime token owner. This document mirrors its existing values: background→bg, panel→panel, raised→raised, border→line, text→fg, secondary→dim, muted→faint, primary→accent, success→ok, warning→warn, danger→bad. Tailwind consumes the `@theme` variables directly; components use semantic utilities rather than copied color values.

Dark surfaces distinguish layers without shadows. Accent marks focus, primary actions, and the selected inspection box. Every status also has a text label. Body/help text uses fg/dim; faint is reserved for secondary metadata. Forced colors retain platform scrollbar behavior.

## Typography

Keep the existing sans stack for controls and prose, and the mono stack for paths, selectors, and descriptors. Fonts fall back to installed system faces; no new font downloads. Headlines are restrained 16–18px; controls are 12–13px, with technical metadata at 11px.

## Layout

Local documentation setup follows the existing single-column assistance step: native model selector, explicit consent, shared buttons and an optional disclosure for test queries. Findings and errors wrap within the panel; they do not create a new app-wide toolbar. No new visual tokens.

The existing application owns a viewport shell. Each active activity/diff/preview panel owns its vertical scrolling through `min-h-0` and flex layout. Toolbars wrap at narrow widths. The preview uses a bounded scrollable image canvas, while attachment rows wrap and retain removal controls. Activity stays mounted when switching tabs so unsent messages and files survive inspection.

The plan screen keeps review/approval actions and progress above the workspace. A dependency map, step list, event timeline and combined changes share one selected-step inspector. Below 1024 CSS pixels, the step list opens by default and selecting a step switches directly to its details; the map scrolls within its own boundary. Status always has text as well as color. The plan editor uses the shared focus-contained dialog; the sidebar indents child agents beneath their root task.

## Elevation & Depth

Use borders and tonal surfaces for panels. Shadows and a dimmed backdrop belong to modal dialogs only. Loading, errors and empty states stay within their owning panel.

## Shapes

Controls use the established 6px radius; content panels use 8px. Selected preview geometry follows the element's actual box and does not redefine app shapes.

## Components

Shared Button, Input and Dialog live in `components/ui.tsx`. Attachments owns the upload queue, validation, cancellation, retry and file links used by both composers. Preview owns the rendered page and inspection selection. Busy mutations disable repeat actions; errors preserve user input and appear inline with a recovery action.

Controls use sentence case and explicit verbs. Focus has an accent outline; disabled controls lose their pointer cursor. Scrollbar styling is global. Reduced motion suppresses animation and transitions. Screenshots are displayed only in the preview; the default agent handoff is text.

## Do's and Don'ts

- Reuse shared controls and runtime tokens.
- Keep inspected source locations honest: report unavailable metadata rather than guessing.
- Preserve drafts on tab switches and failed requests.
- Do not send preview pixels to an agent without an explicit screenshot request.
- Do not introduce screen-local native prompt/confirm dialogs.

## Phase E canonical components

- `ScreenshotViewer.tsx` owns fit, actual size, zoom, pointer/keyboard pan, inspection overlays, and the fullscreen viewer. Geometry comes from returned viewport metadata and image natural dimensions.
- `Dialog` in `ui.tsx` owns default, viewer (96vw), and navigation (left drawer) variants. All use native modal focus containment and bounded internal scrolling.
- `App.tsx` owns collapsed navigation and the narrow-window drawer. The selected task survives opening and closing the drawer.
- `Vault.tsx` owns project notes, graph, proposed replacement comparison, learning evidence and routing preferences. Shared buttons/inputs/dialogs and platform selects remain canonical. No new component library or palette.
- Notes render as plain text; wikilinks become explicit navigation controls. The graph has keyboard-operable nodes and an internal scroll boundary. Agent text never becomes executable HTML.
- New viewport and memory help text uses at least 12px. Long identifiers wrap; image, graph and outcome tables may scroll within their panels.

## Phase F1 guided setup

`Setup.tsx` owns installation-level preferences and account readiness through the daemon API. It uses the existing shared Input/Button/Dialog and native radio/select owners. A restrained four-step progress row and one reading column replace a settings dashboard. Each step scrolls inside the existing viewport shell; no new colors, fonts or tokens.

The default workspace prioritizes project, request, progress and preview/review. Composer routing/access options are under Advanced; per-task model, memory and cost diagnostics are under Task details. These are durable progressive-disclosure decisions, not removal of power-user capabilities.
