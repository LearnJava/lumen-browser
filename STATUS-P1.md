In progress: —

Next:
1A unitless length quirk (img width/height attrs)  layout/src/style.rs:2945
1A IE7 line-height quirk (legend/fieldset)         layout/src/style.rs:4925
1A quirks test coverage                            layout/src/lib.rs:7550
1B Length type through all cascade declarations    layout/src/style.rs:5069 + :2699
1B Color type through all cascade declarations     layout/src/style.rs:494  + :2699
4B flex-direction + flex-wrap properties           layout/src/style.rs:1392 + :6720
4B flex-grow + flex-shrink + flex-basis            layout/src/style.rs:1392
4B flex item layout pass (main/cross axis)         layout/src/box_tree.rs:509
4B flex gap application                            layout/src/box_tree.rs:509
4B flex wrapping + multi-line logic                layout/src/box_tree.rs:509
5  ICU4x struct + segmenter init                   core/src/ext.rs (after NullUnicodeProvider)
5  line_break_opportunities impl                   core/src/ext.rs
5  grapheme_boundaries impl                        core/src/ext.rs
5  word_boundaries impl                            core/src/ext.rs
5  bidi_runs impl                                  core/src/ext.rs

Blocked:
5  ICU4x — needs UnicodeProvider wired into layout first

Recent: graphic-tests-rework 2026-05-19
