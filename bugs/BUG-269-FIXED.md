# BUG-269

**Статус:** FIXED 2026-07-04
**Компонент:** layout/shell (замещаемые элементы, aspect-ratio)
**Симптом:** `<img>` с атрибутом `width` (или CSS-шириной), но БЕЗ явной высоты рендерится с нулевой высотой — картинка невидима. Ожидание (CSS 2.1 §10.6.2 / CSS Images §5): высота считается из intrinsic aspect-ratio (`height = width × (ih / iw)`).

---

## Репро

```html
<img src="pic.png">              <!-- рисуется: intrinsic 120×80 -->
<img src="pic.png" width="240">  <!-- НЕ рисуется: высота схлопнулась -->
```

Headless: `lumen --screenshot out.png repro.html` — второй блок отсутствует (проверено пиксельным замером: одна полоса вместо двух). Воспроизводится и с PNG, и с растеризованным SVG (найдено при приёмке RP-5, формат источника нерелевантен).

## Первопричина (подтверждена по коду)

Два места в `crates/shell/src/main.rs`:

1. Гейт `let wants_intrinsic = !req.has_explicit_width && !req.has_explicit_height;`
   (фаза декода картинок) пропускал `apply_intrinsic_size`, если задана **хоть
   одна** сторона. При `<img width="240">` intrinsic вообще не прокидывался.
2. Плюс сам `apply_intrinsic_size` заполнял недостающие слоты **сырым** intrinsic
   значением, а не из aspect-ratio.

Итог для `width="240"`: DOM оставался с `width=240`, без `height` и без intrinsic;
`resolve_image_source` для обычного `<img src>` даёт `intrinsic_height = None`, а
layout при `style.height = None` и отсутствии intrinsic → used height = 0.

## Как починено

1. Гейт заменён на `!(has_explicit_width && has_explicit_height)` — intrinsic
   применяется, когда не заданы **обе** стороны.
2. `apply_intrinsic_size` стал aspect-ratio-aware: если одна сторона задана
   целочисленным px-атрибутом автора, недостающая считается из intrinsic
   отношения (`height = round(width × ih / iw)` и симметрично, CSS 2.1 §10.6.2);
   если задана нецелочисленно (проценты) — недостающая падает на сырое intrinsic
   значение (картинка видима). Обе стороны пустые → сырые intrinsic. Push только
   в пустые слоты (presentational hint, specificity 0 — авторский CSS побеждает).

## Валидация

- 5 юнит-тестов `bug269_*` в `crates/shell/src/main.rs` (`mod tests`):
  fixed-width→ratio (240×160), fixed-height→ratio, no-attrs→intrinsic,
  both-attrs→unchanged, percentage-width→intrinsic-height fallback.
- Регресс: `<img>` без атрибутов и с обоими атрибутами не меняются.
