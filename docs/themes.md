# Themes

```python
app = ktui.App(theme="tokyo-night")
app.set_theme("kohaku-light")
app.set_theme("kohaku-dark", {"accent": "#ff8fb1", "tool": "sapphire"}, truecolor=True)
print(ktui.THEMES)
```

## Schemes

| Scheme | Notes |
| ------ | ----- |
| `kohaku-dark` (default), `kohaku-light` | the Kohaku gem palette: iolite accent, amber attention, sapphire tools |
| `colorblind-dark`, `colorblind-light` | the Kohaku layout with Okabe–Ito colours: success is blue and errors orange, so they never depend on telling red from green |
| `solarized-dark`, `solarized-light` | Solarized |
| `tokyo-night` | Tokyo Night |
| `terminal` | only the terminal's own 16 colours and its default background: no bands, no tints |

Switching at runtime redraws everything, including full-screen content already laid out.

## Colour roles

Components ask for roles, not colours, so a scheme or an override changes them everywhere:

| Role | Used for |
| ---- | -------- |
| `text`, `muted`, `faint` | body text, secondary text, hints |
| `accent` | focus, the prompt mark, selected items |
| `user`, `assistant` | icons of the `user` and `assistant` cell roles |
| `tool`, `agent` | `tool` cells (commands, steps) and nested or delegated work |
| `running`, `attention`, `danger`, `ok` | spinners, warnings, errors, success |
| `code`, `border` | inline code, frames |
| `selection_fg`, `selection_bg` | the selection bar |
| `user_bg` | the band behind the user's own messages |
| `focus_bg` | the tint of the area of a dialog that has the keys |

`overrides` maps a role or a palette name to `#rrggbb` or another name, so
`{"accent": "iolite"}` and `{"accent": "#9b8cff"}` both work. A component's `tone` prop takes
the same names.

## Truecolor

24-bit colour is used when the terminal supports it: `COLORTERM` says `truecolor` or `24bit`,
or the host is one known to (Windows Terminal, VS Code, iTerm2, WezTerm, Ghostty). Otherwise every
colour falls back to the nearest of the 256-colour palette.
`truecolor=True` or `False` forces it either way.
