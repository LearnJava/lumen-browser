# WPT vendor notes — `media`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-05 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-media`, `docs/wpt-status.md`), scope ⬜ (candidate). Same pinned commit, `git sparse-checkout add` at the same commit hash, 39 files (`META.yml`, `LICENSE-WPT.md` copied from the sibling `measure-memory` category, 37 shared media fixtures — `.mp4`/`.webm`/`.mp3`/`.oga`/`.wav`/`.vtt`/`.png`/`.jpg` — referenced by out-of-category tests across `html/semantics/embedded-content`, `media-source`, `webaudio`, etc.). This top-level category is fixtures-only, same shape as `images` (`WPT-VENDOR-images`, formally closed 2026-08-04): zero of its own `.html`/`.js` test files, no `variant` hits, no `testdriver.js` hits. `run_report.py --all --root media --recursive` returns `no tests selected` instantly — no run to report, no probe applicable (no API of its own to exercise). No bug filed.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/media/`, 39 файлов: `META.yml`, `LICENSE-WPT.md` скопирован из соседней `measure-memory`, 37 общих медиа-фикстур — `.mp4`/`.webm`/`.mp3`/`.oga`/`.wav`/`.vtt`/`.png`/`.jpg`, на которые ссылаются вне-категорийные тесты в `html/semantics/embedded-content`, `media-source`, `webaudio` и др.). Категория верхнего уровня целиком фикстурная — тот же класс, что `images` (`WPT-VENDOR-images`, закрыта 2026-08-04): ноль собственных `.html`/`.js`-файлов, ни variant-ов, ни `testdriver.js`. `run_report.py --all --root media --recursive` мгновенно возвращает `no tests selected` — прогонять нечего, пробовать нечего (у категории нет своего API). Новых багов не заведено.
