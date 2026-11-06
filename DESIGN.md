---
name: SpeakFlow
description: Black ground, one amber signal, and the voice drawn as a waveform
colors:
  primary: "#FFB020"
  primary-light: "#A86200"
  bg-primary: "#000000"
  bg-secondary: "#070707"
  bg-tertiary: "#0E0E0E"
  bg-elevated: "#0A0A0A"
  text-primary: "#FFFFFF"
  text-secondary: "#8A8A8A"
  text-tertiary: "#585858"
  border: "#1A1A1A"
  success: "#4ADE80"
  error: "#FF453A"
  warning: "#FFD60A"
  assistant: "#B98CFF"
  bg-primary-light: "#FFFFFF"
  bg-secondary-light: "#F4F4F1"
  bg-tertiary-light: "#E9E9E4"
  text-primary-light: "#0A0A0A"
  text-secondary-light: "#565656"
  border-light: "#D4D4CE"
typography:
  display:
    fontFamily: '"Bahnschrift", "Segoe UI Variable", "Segoe UI", system-ui, sans-serif'
    fontSize: "8px"
    fontWeight: 600
    letterSpacing: "0.17em"
    textTransform: "uppercase"
  liveLine:
    fontFamily: '"Segoe UI Variable", "Segoe UI", system-ui, sans-serif'
    fontSize: "19px"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "-0.012em"
  row:
    fontFamily: '"Segoe UI Variable", "Segoe UI", system-ui, sans-serif'
    fontSize: "11.5px"
    lineHeight: 1.4
  numeric:
    fontFamily: '"Cascadia Mono", Consolas, monospace'
    fontSize: "15px"
    fontVariantNumeric: "tabular-nums"
  counter:
    fontFamily: '"Cascadia Mono", Consolas, monospace'
    fontSize: "44px"
    letterSpacing: "-0.03em"
rounded:
  control: "0px"
  full: "9999px"
spacing:
  hair: "1px"
  xs: "4px"
  sm: "8px"
  md: "14px"
  lg: "24px"
components:
  title-bar:
    height: "32px"
    backgroundColor: "{colors.bg-primary}"
    borderBottom: "1px solid {colors.border}"
  nav-active:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.bg-primary}"
    rounded: "{rounded.control}"
  wave-panel:
    height: "168px"
    backgroundColor: "{colors.bg-primary}"
    borderBottom: "1px solid {colors.border}"
  status-bar:
    height: "24px"
    borderTop: "1px solid {colors.border}"
---

# Design System: SpeakFlow

## Overview

**Creative North Star: "SIGNAL"**

SpeakFlow is a dictation instrument. The ground is pure black, the structure is
hairline rules, and exactly one colour exists: amber, reserved for the signal —
recording, live state, primary action. Everything else is white, grey, or the
absence of light.

The interface's job is to make the voice visible. The top of the dictation
workspace is a waveform drawn from real microphone levels, scrolling right to
left, white for the history of the take and amber for the live edge. The
transcript sits beneath it. The rundown sits beneath that. A sources rail holds
what the app is using. Nothing else competes.

**Key Characteristics:**

- Pure black ground; no charcoal, no tinted surfaces.
- One chromatic colour, amber, used only for signal.
- Square corners everywhere; hairlines instead of shadows.
- Dense rows and small type; the transcript is the only large text.
- The waveform is real data, never decoration.

## Colors

### Primary

Amber `#FFB020` in dark, `#A86200` in light. It marks the record control, the
live caption tag, the active navigation item, the live waveform tail, the
recording status dot, and primary links. It is never used for large surfaces
except the active nav item and the live tag, which are small.

**The Signal Rule.** If it is amber, it is live, primary, or the thing to press.
Nothing decorative is amber.

### Neutral

The dark theme is `#000000` at the base, `#070707` for the rundown and rail, and
`#0E0E0E` for hover. Text runs white, `#8A8A8A` secondary, `#585858` tertiary.
Borders are `#1A1A1A` — visible enough to structure, quiet enough to disappear.

The light theme is a paper translation of the same system: `#FFFFFF` ground,
`#F4F4F1` secondary, `#D4D4CE` borders, and a darkened amber `#A86200` that holds
contrast on white. Dark is the fresh-install default.

### Semantic

Success, error, warning, and assistant keep their own hues and are never
recoloured to amber. Warning is a pale yellow `#FFD60A` so it does not read as
the signal.

## Typography

Three voices, all installed on Windows. No runtime font download.

- **Bahnschrift** for every micro-label: 8px, 600 weight, 0.17em tracking,
  uppercase. This is the instrument's engraved voice and carries most of the
  interface's character.
- **Segoe UI Variable** for reading: the transcript at 19px with -0.012em
  tracking, rundown rows at 11.5px, rail values at 11px.
- **Cascadia Mono** for anything that counts: the take counter at 44px, the
  duration at 15px, timestamps, character counts, and latency. Always tabular.

The scale is deliberately narrow. The largest text in the app is the 44px take
counter in the rail; the transcript at 19px is the only large reading size.
Everything else is 8–12px.

## Layout

The shell is three rows: a 32px title bar, the workspace, and a 24px status bar.
The title bar carries the brand, the document name, the four navigation
destinations, and the window controls. There is no sidebar.

Dictation is a two-column split with a draggable divider: the main column
(1fr) and the sources rail (252px). The main column is four rows: the 168px
waveform panel, the transcript, a 4px splitter, and the 176px rundown.

At 1100px the rail narrows to 220px. At 900px the rail is dropped and the
splitter with it. At 640px the waveform panel shortens to 132px, the rundown to
140px, the transcript head wraps, and the live line drops to 16px.

History, Settings, and Updates share the shell and the square, hairline
language. Their content areas scroll independently.

Settings is a two-column grid: a 186px section index and the open form column,
with a full-width footer beneath. The index numbers each destination 01–10 in
mono, tracks the section in view as the form scrolls, and marks the active row
with a 2px amber left bar. Sections are separated by hairlines rather than
cards; each carries a tracked-caps title over a rule. Rows are 30px minimum with
the label left and the control right. Below 720px the index becomes a horizontal
strip above the form.

## Elevation & Depth

There is no elevation. Surfaces are separated by tone (`#000000` against
`#070707`) and by 1px rules. Shadows are removed from the token layer so no
component can reintroduce them. Depth is communicated by position and rule
weight, never by blur.

The one exception is the desktop overlay windows — subtitle and selection —
which keep their own restrained shadow because they float over other
applications.

## Shapes

Square. Every control, panel, and input has 0 radius. The only circles are the
record button, status dots, and the waveform bars' 0.5px rounding. Keycaps are
square with an amber hairline border.

## Components

### Waveform panel

The signature surface. A 42px head holds the record control, the model status
chips, the status line, and the elapsed duration. Below it, 78 bars span the
full width; the rightmost 18 carry the amber live treatment. The panel ends with
a hairline baseline and a 26px foot carrying the input label and the shortcut.

Bars are painted imperatively inside a single animation frame from the native
`waveform` event, which emits nine RMS values roughly eighteen times a second.
A React render per event would re-render the whole page at that rate, so the
buffer is written straight to the bar elements. When idle the bars rest at a
3% hairline so the panel never looks broken, and no idle motion is simulated.

### Transcript

The live line is an editable textarea at 19px with no border and no card. Above
it, a 20px header carries the result label, the original-transcript toggle, and
copy. Below it, a mono stats line carries language, pace, characters, duration,
and latency. The empty state is a small left-aligned block, not a centred hero.

### Rundown

Dense rows at 26px: timestamp, transcript preview, copy. The newest entry is
excluded when it duplicates the transcript above. The header carries the count
and a link to History.

### Sources rail

A 44px take counter, then label/value pairs for model, shortcut, status, and
destination, then a Configure link. The rail is the quietest surface in the
app — it answers "what is this using" without competing with the work.

### Settings index

Ten numbered rows in a 186px rail. The index is navigation and table of
contents at once: it scroll-spies the form, scrolls itself to keep the active
row visible, and scrolls only the form column when a row is clicked. The number
is decorative and hidden from assistive technology; the label carries the name.

### Shell chrome

The title bar holds navigation as small tracked caps with icons; the active
destination is flooded amber with black text. Window controls are 46px wide and
the close button turns red on hover. The status bar repeats the live state,
duration, mode, compute device, and shortcut so the app's state is readable from
any workspace.

## Do's and Don'ts

### Do:

- **Do** keep amber for signal only.
- **Do** draw the waveform from real microphone levels.
- **Do** separate surfaces with hairlines and tone, never shadows.
- **Do** keep the transcript the largest reading text on the screen.
- **Do** preserve editable transcripts, keyboard access, light/system themes,
  and every recovery state.

### Don't:

- **Don't** introduce a second chromatic colour.
- **Don't** round corners or add card shadows.
- **Don't** animate the waveform or any control when idle.
- **Don't** turn the dictation workspace back into a dashboard of cards.
- **Don't** treat a browser render as proof of microphone, GPU, or provider
  performance.
