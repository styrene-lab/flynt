+++
# Style guide for [project / company name].
# This file is read by Flynt's design extension at canvas-design time.
# Place at <vault>/.flynt/style-guide.md (project-level, version-controlled)
# or ~/.flynt/style-guide.md (user-level default across all projects).
# Project-level overrides user-level when both exist.

[brand]
name = "Acme Corp"
tagline = "Industrial precision, every time."

[colors]
# Use these hex values via CSS variables in cell.css; the canvas's theme
# tokens (--primary, --background) are still the day-to-day defaults.
# Brand colors are for moments that require exact match.
primary = "#1A3A52"
accent  = "#F59E0B"
ink     = "#0C0C0C"
paper   = "#FAFAF9"

[typography]
display = "Söhne, system-ui, sans-serif"
body    = "Söhne, system-ui, sans-serif"
mono    = "Berkeley Mono, ui-monospace, monospace"
+++

# Voice

We write like a senior engineer briefing a peer: terse, technical when it earns
its keep, never breathless. No exclamation marks. No "We're excited to..." No
"unlock" or "empower" or "leverage." If a sentence reads like a press release,
delete it.

# Visual rules

- **Density over delight.** Show numbers, charts, real data. Reserve illustration
  for moments that genuinely benefit from a visual metaphor.
- **One brand color per surface.** Don't accent on accent on accent. The accent
  is amber; use it for one thing per cell, not three.
- **Black on white, white on black.** Avoid mid-tones for body text. We use
  `text-foreground` and `text-muted-foreground` only — no custom greys.
- **Typography ramp.** Hero: `text-5xl` `font-bold`. Section: `text-2xl`
  `font-semibold`. Body: `text-sm`. No `text-base` — it's a sign you didn't
  decide.

# Don't

- Marketing-fluff buttons ("Get Started Now!" → just "Get started")
- Faux-3D effects, gradient backgrounds, sparkle icons
- Drop shadows except on cards at exactly `shadow-md`
- Emoji in product copy (UI labels, errors, microcopy)

# Do

- Use brand amber (`#F59E0B`) for the single most important action on a page,
  nothing else
- Use `border-border` over `shadow` for separation when both work
- Keep CTAs verbal-imperative ("Save", "Cancel", "Continue") not noun-phrase
  ("Submission", "Cancellation")
