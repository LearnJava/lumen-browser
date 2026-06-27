1: ROADMAP.md:118 — done (navapi blocked; last commit `e31a08ee` completed Steps 8–10)
2: ROADMAP.md:124 — ph3-color-management in progress
   - Step 1: done — commit `c486539c` (`DisplayColorProfile` trait, Windows GDI `GetICMProfileA`, `Lumen::target_color_space()`).
   - Step 2: pending — wants generalization of `icc::build_rgb_transform` + `ColorFloat::to_display(target)`, but `ColorFloat` type is not present in the repo; blocked until upstream type lands or concrete target API is specified.
   - Steps 3–8: pending, dependent on Step 2 resolution.
