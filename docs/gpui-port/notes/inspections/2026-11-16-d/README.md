# 2026-11-16 (d): Kotlin colours in the real window (commit ee441bf1)

Taken by the integrator from its own debug binary (temp home, temp `HOME` and `ZDOTDIR`, short `PATH`, a temp trusted project with one Kotlin file). I looked at both. The integrator's mark names had the themes swapped; the files are named here by what they show.

- `kotlin-dark.png`, `kotlin-light.png`: keywords (`package`, `class`, `val`, `private`, `var`, `fun`, `if`, `return`) in the keyword colour, strings green, the line comment and the doc comment italic grey, `@file` and `@Deprecated` and the `${ }` of a template in the annotation colour, numbers blue, `null` italic, declared names and the function's name in their role colours.

Seen, not fixed: in `"hi $name, …"` the `$` takes the template colour and `name` stays in the string's colour.
