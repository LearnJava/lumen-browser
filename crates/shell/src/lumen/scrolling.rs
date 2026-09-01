//! Moving the page: scroll bounds, the overflow containers a wheel notch may
//! be aimed at, CSS scroll snapping, the smooth-scroll animation and touchpad
//! momentum.
//!
//! The maths of an individual animation lives in `crate::scroll_anim` and
//! `crate::momentum_anim`, the per-notch step sizes in `crate::scroll::metrics`;
//! what is here is the part that needs the live page - which container is under
//! the cursor, how far the display list says the document reaches, and which of
//! the two panes of a split view the delta belongs to.

use crate::*;

impl Lumen {
    /// РњР°РєСЃРёРјР°Р»СЊРЅС‹Р№ РІР°Р»РёРґРЅС‹Р№ scroll_y: РЅРёС‡РµРіРѕ РЅРµ СЃРєСЂРѕР»Р»РёРј, РµСЃР»Рё РєРѕРЅС‚РµРЅС‚
    /// РїРѕРјРµС‰Р°РµС‚СЃСЏ РІ viewport. РРЅР°С‡Рµ вЂ” `content_height в€’ viewport_height`.
    pub(crate) fn max_scroll(&self) -> f32 {
        (self.content_height - self.viewport_height_css()).max(0.0)
    }

    /// РњР°РєСЃРёРјР°Р»СЊРЅС‹Р№ РІР°Р»РёРґРЅС‹Р№ scroll_x: 0 РµСЃР»Рё РєРѕРЅС‚РµРЅС‚ РїРѕРјРµС‰Р°РµС‚СЃСЏ РїРѕ С€РёСЂРёРЅРµ.
    ///
    /// РСЃРїРѕР»СЊР·СѓРµС‚ `page_content_width_css()` вЂ” РїРѕР»РЅР°СЏ С€РёСЂРёРЅР° РјРёРЅСѓСЃ РїР°РЅРµР»СЊ РІРєР»Р°РґРѕРє.
    pub(crate) fn max_scroll_x(&self) -> f32 {
        (self.content_width - self.page_content_width_css()).max(0.0)
    }

    /// Rebuild `snap_containers` from the current `layout_box`.
    ///
    /// Called whenever `layout_box` changes (relayout, page load, tab switch).
    /// Cheap when the page has no `scroll-snap-type` declarations (returns empty).
    pub(crate) fn update_snap_containers(&mut self) {
        match &self.layout_box {
            Some(lb) => self.snap_containers = collect_snap_containers(lb),
            None => self.snap_containers.clear(),
        }
    }

    /// Rebuild `scroll_containers` from the current `layout_box`.
    ///
    /// Called whenever `layout_box` changes (relayout, page load, tab switch).
    /// Used by the wheel handler to route scroll events to overflow containers.
    pub(crate) fn update_scroll_containers(&mut self) {
        match &self.layout_box {
            Some(lb) => self.scroll_containers = collect_scroll_containers(lb),
            None => self.scroll_containers.clear(),
        }
    }

    /// Try to scroll an overflow container under the cursor by `(dx, dy)` CSS px.
    ///
    /// Returns `true` if a container was found and scrolled, `false` if no
    /// overflow container is under the cursor (caller should scroll the page).
    ///
    /// The cursor position is converted from physical pixels to document-space
    /// CSS px (adds page scroll offsets so hit-testing works on scrolled pages).
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn try_scroll_overflow_container(&mut self, dx: f32, dy: f32) -> bool {
        let Some(cursor) = self.cursor_position else { return false };
        if self.layout_box.is_none() { return false; }

        let dpr = self.renderer.as_ref().map_or(1.0_f32, |r| r.scale_factor() as f32);
        let x_css = (cursor.x as f32) / dpr + self.scroll_x;
        let y_css = (cursor.y as f32) / dpr + self.scroll_y;

        let Some(target) = find_scroll_container_at(&self.scroll_containers, x_css, y_css) else {
            return false;
        };
        let target_nid = target.index() as u32;

        // Find current position and compute new target.
        let current = self.scroll_containers.iter()
            .find(|c| c.node == target)
            .map(|c| (c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height,
                      c.clip_rect.width, c.clip_rect.height,
                      c.overscroll_behavior_x, c.overscroll_behavior_y));
        let Some((cur_x, cur_y, sw, sh, clip_w, clip_h, ob_x, ob_y)) = current else { return false };

        let new_x = (cur_x + dx).clamp(0.0, (sw - clip_w).max(0.0));
        let new_y = (cur_y + dy).clamp(0.0, (sh - clip_h).max(0.0));

        // CSS Overscroll Behavior L1 В§3 вЂ” scroll-chain stop. If the container is
        // at its boundary on every axis and `overscroll-behavior` permits it, let
        // the residual delta propagate to the page; otherwise the chain stops
        // here (event consumed even if the container did not move).
        let moved_x = (new_x - cur_x).abs() > f32::EPSILON;
        let moved_y = (new_y - cur_y).abs() > f32::EPSILON;
        if lumen_layout::overscroll_should_propagate(ob_x, ob_y, dx, dy, moved_x, moved_y) {
            return false;
        }
        if !moved_x && !moved_y {
            // Boundary reached but propagation is blocked (contain/none) вЂ” consume
            // the gesture without a relayout/redraw.
            return true;
        }

        // Borrow layout_box mutably after releasing the immutable scroll_containers borrow.
        let scrolled = if let Some(lb) = self.layout_box.as_mut() {
            set_scroll_position(lb, target, new_x, new_y)
        } else {
            false
        };
        if scrolled {
            // Р‘С‹СЃС‚СЂС‹Р№ РїСѓС‚СЊ: С‚РѕС‡РµС‡РЅС‹Р№ РїР°С‚С‡ СЃРєСЂРѕР»Р»-СЃР»РѕСЏ РІ РіРѕС‚РѕРІРѕРј display list вЂ”
            // layout РґРµС‚РµР№ РїСЂРё СЃРєСЂРѕР»Р»Рµ РЅРµ РјРµРЅСЏРµС‚СЃСЏ, РїРѕСЌС‚РѕРјСѓ РїРѕР»РЅР°СЏ РїРµСЂРµСЃР±РѕСЂРєР°
            // paint_ordered РЅР° РєР°Р¶РґС‹Р№ С‚РёРє РєРѕР»РµСЃР° РЅРµ РЅСѓР¶РЅР° (СЃРј.
            // lumen_paint::patch_scroll_layer; СЌРєРІРёРІР°Р»РµРЅС‚РЅРѕСЃС‚СЊ РїРµСЂРµСЃР±РѕСЂРєРµ
            // Р·Р°РєСЂРµРїР»РµРЅР° С‚РµСЃС‚Р°РјРё patch_scroll_layer_* РІ display_list.rs).
            // РЎРїРёСЃРѕРє РїСЂР°РІРёС‚СЃСЏ РќРђ РњР•РЎРўР• вЂ” РІРµСЂСЃРёСЋ Р±Р°РјРїР°РµРј Р·Р°СЂР°РЅРµРµ: Р·Р°РјС‹РєР°РЅРёРµ РЅРёР¶Рµ
            // Р·Р°С…РІР°С‚С‹РІР°РµС‚ С‚РѕР»СЊРєРѕ РїРѕР»Рµ `display_list` (`layout_box` Р·Р°РЅСЏС‚
            // СЃРѕСЃРµРґРЅРёРј Р·Р°РёРјСЃС‚РІРѕРІР°РЅРёРµРј), РїРѕСЌС‚РѕРјСѓ `&mut self` РІРЅСѓС‚СЂРё РЅРµРіРѕ РЅРµС‚.
            self.bump_display_list_epoch();
            let patched = lumen_layout::find_box_by_node(
                self.layout_box.as_ref().unwrap(),
                target,
            )
            .is_some_and(|cb| lumen_paint::patch_scroll_layer(&mut self.display_list, cb));
            if patched {
                // РўРѕС‡РµС‡РЅР°СЏ РїСЂР°РІРєР°: РіСЂСЏР·РЅС‹Рµ С‚РѕР»СЊРєРѕ С‚Р°Р№Р»С‹ РїРѕРґ РєРѕРЅС‚РµР№РЅРµСЂРѕРј.
                if let Some(c) = self.scroll_containers.iter().find(|c| c.node == target) {
                    self.tile_grid.mark_rect_dirty(c.clip_rect);
                }
            } else {
                // Fallback: РїРѕР»РЅР°СЏ РїРµСЂРµСЃР±РѕСЂРєР° РїСЂРё Р»СЋР±РѕР№ РЅРµСЃС‚Р°РЅРґР°СЂС‚РЅРѕР№ СЃС‚СЂСѓРєС‚СѓСЂРµ DL.
                let new_dl = paint_ordered(self.layout_box.as_ref().unwrap());
                self.tile_grid.update_from_diff(&self.display_list, &new_dl);
                self.set_display_list(new_dl);
            }
            self.update_scroll_containers();
            let states: std::collections::HashMap<_, _> = self.scroll_containers.iter()
                .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                .collect();
            // ADR-016 M2.2c-2d (16): overflow-container scroll fire-and-forget void
            // (`update_scroll_states` push в†’ `fire_element_scroll`) С‡РµСЂРµР· `route_task_js`.
            // `states` (owned `HashMap`) Рё `target_nid` (`u32`, Copy) РїРµСЂРµРµР·Р¶Р°СЋС‚ РІ
            // `move`-Р·Р°РјС‹РєР°РЅРёРµ `Send + 'static`; РїРѕСЂСЏРґРѕРє pushв†’dispatch СЃРѕС…СЂР°РЅС‘РЅ РІРЅСѓС‚СЂРё
            // РѕРґРЅРѕРіРѕ `task`. РџРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґРёС‚ off-UI-thread;
            // Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹, **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ**.
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                js.update_scroll_states(states);
                js.fire_element_scroll(target_nid);
                // BUG-822: one wheel notch over a container is applied
                // instantly, so it is a complete scroll sequence of its own вЂ”
                // unlike the page, which routes the wheel through
                // `scroll_by_smooth` and therefore ends once per animation.
                js.fire_element_scrollend(target_nid);
            });
            self.request_redraw();
            true
        } else {
            false
        }
    }

    /// Прокрутить overflow-контейнер (`overflow: scroll|auto`) ВНУТРИ
    /// под-документа фрейма под курсором на `(dx, dy)` CSS px (FRAME-3 срез 3,
    /// остаток дорожки — зеркало [`Self::try_scroll_overflow_container`] на
    /// уровне под-документа, тем же движком `lumen_layout`).
    ///
    /// Цель ищется через [`frames::pointer_target`], не через собственный
    /// hit-тест: точка внутри фрейма приходит уже в системе координат
    /// ВЬЮПОРТА ребёнка (`FramePointerHit::client`), а `ScrollContainer::clip_rect`
    /// — в document-space РЕБЁНКА, поэтому к `client` нужно прибавить его же
    /// `scroll_x`/`scroll_y` — тот же перевод, что `pointer_target` сам делает
    /// при спуске во ВЛОЖЕННЫЙ фрейм (`frames.rs`, комментарий среза 17).
    ///
    /// Вызывается ДО [`Self::try_scroll_frame`]: overflow-контейнер внутри
    /// под-документа — самый глубокий скроллер под курсором и должен победить
    /// прокрутку фрейма целиком, точно так же, как контейнеры страницы
    /// побеждают прокрутку самой страницы.
    #[allow(clippy::unwrap_used)]  // тот же приём, что try_scroll_overflow_container
    pub(crate) fn try_scroll_frame_overflow_container(&mut self, dx: f32, dy: f32) -> bool {
        if self.frames.is_empty() {
            return false;
        }
        let Some(cursor) = self.cursor_position else { return false };
        let dpr = self.renderer.as_ref().map_or(1.0_f32, |r| r.scale_factor() as f32);
        let target = self.pointer_target((cursor.x as f32) / dpr, (cursor.y as f32) / dpr);
        let Some(hit) = target.frame else { return false };
        let idx = hit.frame;
        let x_css = hit.client.x + self.frames[idx].scroll_x;
        let y_css = hit.client.y + self.frames[idx].scroll_y;

        let Some(target_node) =
            find_scroll_container_at(&self.frames[idx].scroll_containers, x_css, y_css)
        else {
            return false;
        };

        let current = self.frames[idx].scroll_containers.iter()
            .find(|c| c.node == target_node)
            .map(|c| (c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height,
                      c.clip_rect.width, c.clip_rect.height,
                      c.overscroll_behavior_x, c.overscroll_behavior_y));
        let Some((cur_x, cur_y, sw, sh, clip_w, clip_h, ob_x, ob_y)) = current else { return false };

        let new_x = (cur_x + dx).clamp(0.0, (sw - clip_w).max(0.0));
        let new_y = (cur_y + dy).clamp(0.0, (sh - clip_h).max(0.0));

        // CSS Overscroll Behavior L1 §3 — та же chain-семантика, что у
        // страничных контейнеров: на границе с `auto` жест уходит дальше
        // (к прокрутке всего фрейма), `contain`/`none` его глушит на месте.
        let moved_x = (new_x - cur_x).abs() > f32::EPSILON;
        let moved_y = (new_y - cur_y).abs() > f32::EPSILON;
        if lumen_layout::overscroll_should_propagate(ob_x, ob_y, dx, dy, moved_x, moved_y) {
            return false;
        }
        if !moved_x && !moved_y {
            return true;
        }

        let scrolled = if let Some(lb) = self.frames[idx].layout.as_mut() {
            set_scroll_position(lb, target_node, new_x, new_y)
        } else {
            false
        };
        if !scrolled {
            return false;
        }

        self.frames[idx].scroll_containers =
            lumen_layout::collect_scroll_containers(self.frames[idx].layout.as_ref().unwrap());
        // Точечного патча (как `patch_scroll_layer` у страницы) для фрейма
        // нет — тот же грубый грануляр, что уже принят у [`Self::apply_frame_scroll`]:
        // пересобрать content_dl фрейма (и его предков — функция сама
        // пропагирует «потомок перерисовался» по цепочке хостов) и список
        // страницы целиком.
        let mut relaid = vec![false; self.frames.len()];
        relaid[idx] = true;
        frames::rebuild_frame_display_lists(&mut self.frames, &relaid);
        let rebuilt = self.layout_box.as_ref().map(paint_ordered);
        if let Some(new_dl) = rebuilt {
            self.tile_grid.update_from_diff(&self.display_list, &new_dl);
            self.set_display_list(new_dl);
        }

        // CSSOM-View §14 для элемента ВНУТРИ ребёнка: `frames[idx].js` — его
        // собственный рантайм (как уже зовёт его напрямую, без роутинга на
        // engine-поток, `apply_frame_scroll` — фреймы не engine-threaded,
        // FRAME-1), а не `route_task_js`, который адресует ТОЛЬКО контекст
        // страницы.
        if let Some(js) = self.frames[idx].js.clone() {
            let states: std::collections::HashMap<_, _> = self.frames[idx].scroll_containers.iter()
                .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                .collect();
            let target_nid = target_node.index() as u32;
            js.update_scroll_states(states);
            js.fire_element_scroll(target_nid);
            // BUG-822: один notch колеса над контейнером — законченная
            // последовательность прокрутки сама по себе, симметрично тому, что
            // страничный `try_scroll_overflow_container` уже делает.
            js.fire_element_scrollend(target_nid);
        }

        self.request_redraw();
        true
    }

    /// Прокрутить под-документ фрейма под курсором на `(dx, dy)` CSS px
    /// (BUG-480 срез 17; горизонталь — FRAME-3 срез 1).
    ///
    /// `true` — колесо ПОГЛОЩЕНО фреймом хотя бы по одной оси; страница тогда
    /// не двигается вовсе, как и при попадании в overflow-контейнер (та же
    /// грубая гранулярность: остаток по оси, упёршейся в край, для этого
    /// notch-а просто теряется, а не докатывается странице — см.
    /// `try_scroll_overflow_container`). `false` — точка не во фрейме ЛИБО
    /// фрейм уже на своём краю по обеим осям: тогда весь жест уходит дальше
    /// по цепочке (CSS Overscroll Behavior L1 §3, значение по умолчанию
    /// `auto`), то есть достигнув края под-документа колесо продолжает
    /// крутить страницу — ровно то, чего ждёт человек.
    pub(crate) fn try_scroll_frame(&mut self, dx: f32, dy: f32) -> bool {
        if (dx == 0.0 && dy == 0.0) || self.frames.is_empty() {
            return false;
        }
        let Some(cursor) = self.cursor_position else { return false };
        let dpr = self.renderer.as_ref().map_or(1.0_f32, |r| r.scale_factor() as f32);
        let target = self.pointer_target((cursor.x as f32) / dpr, (cursor.y as f32) / dpr);
        // Спуск идёт до САМОГО ГЛУБОКОГО фрейма под точкой: колесо адресует
        // ближайший к курсору скроллер, как и у вложенных overflow-контейнеров.
        let Some(hit) = target.frame else { return false };
        let want_y = self.frames[hit.frame].scroll_y + dy;
        let want_x = self.frames[hit.frame].scroll_x + dx;
        // Обе оси независимы: `apply_frame_scroll{,_x}` — no-op (`false`) без
        // редро/ребилда, если ось уже на пределе, так что порядок вызовов не
        // маскирует движение другой оси.
        let moved_y = self.apply_frame_scroll(hit.frame, want_y);
        let moved_x = self.apply_frame_scroll_x(hit.frame, want_x);
        moved_y || moved_x
    }

    /// Поставить под-документ фрейма `idx` в абсолютную позицию `y`
    /// (BUG-480 срез 17) — общее тело колеса и программного
    /// `window.scrollTo`/`scrollBy` из скрипта самого ребёнка.
    ///
    /// `false` — позиция не изменилась (зажата пределом): ни перерисовки, ни
    /// событий, ни поглощения жеста.
    pub(crate) fn apply_frame_scroll(&mut self, idx: usize, y: f32) -> bool {
        let Some(new_y) = frames::scroll_frame_to(&mut self.frames, idx, y) else {
            return false;
        };
        // Список страницы пересобирается целиком: вклейка содержимого фрейма
        // живёт в `set_display_list`, а её смещение только что изменилось.
        // Точечного патча (как `patch_scroll_layer` у overflow-контейнеров)
        // для фрейма нет — его содержимое приезжает отдельным списком.
        let rebuilt = self
            .layout_box
            .as_ref()
            .map(paint_ordered);
        if let Some(new_dl) = rebuilt {
            self.tile_grid.update_from_diff(&self.display_list, &new_dl);
            self.set_display_list(new_dl);
        }
        // CSSOM-View §14 «run the scroll steps» для ВЬЮПОРТА ребёнка: у него
        // свой документ и свой `window`, поэтому события шлём в его рантайм
        // напрямую (хэндлы фреймов живут только на UI-стороне, ADR-014).
        // Прокрутка фрейма мгновенна на обоих путях — своей анимации у неё
        // нет, — поэтому последовательность закончилась в том же кадре и
        // `scrollend` идёт следом за `scroll` (BUG-822, как у контейнеров).
        #[cfg(feature = "v8")]
        if let Some(js) = self.frames[idx].js.clone() {
            if js.set_page_scroll_y(new_y) {
                js.fire_window_scroll();
            }
            if js.page_scrollend_due(true, true) {
                js.fire_window_scrollend();
            }
        }
        self.request_redraw();
        true
    }

    /// Поставить под-документ фрейма `idx` в абсолютную ГОРИЗОНТАЛЬНУЮ
    /// позицию `x` (FRAME-3 срез 1) — колёсный аналог [`Self::apply_frame_scroll`],
    /// без JS-моста: `window.scrollX` ребёнка захардкожен в 0 (см. doc-comment
    /// на `FrameHandle::scroll_x`), так что `scroll`/`scrollend` здесь не
    /// шлётся — симметрично тому, что `scroll_x_by` странице их тоже не шлёт.
    pub(crate) fn apply_frame_scroll_x(&mut self, idx: usize, x: f32) -> bool {
        if frames::scroll_frame_to_x(&mut self.frames, idx, x).is_none() {
            return false;
        }
        let rebuilt = self
            .layout_box
            .as_ref()
            .map(paint_ordered);
        if let Some(new_dl) = rebuilt {
            self.tile_grid.update_from_diff(&self.display_list, &new_dl);
            self.set_display_list(new_dl);
        }
        self.request_redraw();
        true
    }

    /// Прокрутить под-документ ФОКУСНОГО фрейма клавиатурой на `(dx, dy)`
    /// CSS px (FRAME-3 срез 2) — клавиатурный аналог [`Self::try_scroll_frame`].
    /// У клавиши нет курсора, поэтому целевой фрейм — `self.focused_frame`
    /// (то же поле, что уже маршрутизирует движение каретки — срез с кареткой
    /// внутри фрейма, `keyboard.rs`), а не `pointer_target`.
    ///
    /// `true` — фрейм поглотил жест хотя бы по одной оси; `false` — фокуса
    /// во фрейме нет, либо фрейм уже на своём краю по обеим запрошенным
    /// осям (CSS Overscroll Behavior L1 §3, default `auto`): тогда клавиша
    /// должна продолжить листать страницу как раньше.
    pub(crate) fn try_scroll_focused_frame(&mut self, dx: f32, dy: f32) -> bool {
        if (dx == 0.0 && dy == 0.0) || self.frames.is_empty() {
            return false;
        }
        let Some((idx, _)) = self.focused_frame else { return false };
        let want_y = self.frames[idx].scroll_y + dy;
        let want_x = self.frames[idx].scroll_x + dx;
        // Обе оси независимы — тот же порядок вызовов, что у `try_scroll_frame`.
        let moved_y = self.apply_frame_scroll(idx, want_y);
        let moved_x = self.apply_frame_scroll_x(idx, want_x);
        moved_y || moved_x
    }

    /// Поставить под-документ ФОКУСНОГО фрейма в АБСОЛЮТНУЮ вертикальную
    /// позицию (FRAME-3 срез 2) — клавиатурный аналог для `Home`/`End`,
    /// зеркало [`Self::try_scroll_focused_frame`] по абсолютной, а не
    /// относительной позиции. `f32::INFINITY` = «в самый низ», как у
    /// [`Self::scroll_active_pane_to`].
    ///
    /// `true`/`false` — та же CSS Overscroll Behavior L1 §3 семантика: `false`
    /// либо без фокуса во фрейме, либо позиция не изменилась (уже на краю).
    pub(crate) fn try_scroll_focused_frame_to(&mut self, y: f32) -> bool {
        if self.frames.is_empty() {
            return false;
        }
        let Some((idx, _)) = self.focused_frame else { return false };
        let target = if y.is_infinite() {
            frames::frame_max_scroll(&self.frames[idx])
        } else {
            y
        };
        self.apply_frame_scroll(idx, target)
    }

    /// BUG-338: bring `target_rect` (a target element's absolute border-box
    /// rect) into view within every scrolling overflow ancestor of `node`,
    /// vertical axis only вЂ” the ancestor-walk part of `Element.scrollIntoView()`
    /// that fragment navigation is supposed to invoke but never did (only the
    /// page-level scroll below ran). Walks the DOM parent chain from `node`,
    /// scrolls each `ScrollContainer` match whose current viewport doesn't
    /// already contain `target_rect` just enough to bring it in (align the
    /// nearer edge), and leaves already-visible containers untouched. Content
    /// boxes carry absolute (unscrolled) coordinates вЂ” see `PushScrollLayer`'s
    /// paint-time `translate(-scroll_x, -scroll_y)` вЂ” so each container's
    /// adjustment is independent of its ancestors' own scroll offset.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn scroll_nested_ancestors_into_view(&mut self, node: NodeId, target_rect: lumen_core::geom::Rect) {
        let Some(src) = self.layout_source.as_ref() else { return };
        let mut ancestor = src.document.lock().unwrap().get(node).parent;
        while let Some(n) = ancestor {
            let Some(c) = self.scroll_containers.iter().find(|c| c.node == n) else {
                ancestor = src.document.lock().unwrap().get(n).parent;
                continue;
            };
            let visible_top = target_rect.y - c.scroll_y;
            let visible_bottom = target_rect.y + target_rect.height - c.scroll_y;
            let new_scroll_y = if visible_top < c.clip_rect.y {
                c.scroll_y - (c.clip_rect.y - visible_top)
            } else if visible_bottom > c.clip_rect.y + c.clip_rect.height {
                c.scroll_y + (visible_bottom - (c.clip_rect.y + c.clip_rect.height))
            } else {
                c.scroll_y
            };
            if (new_scroll_y - c.scroll_y).abs() > f32::EPSILON
                && let Some(lb) = self.layout_box.as_mut()
            {
                set_scroll_position(lb, n, c.scroll_x, new_scroll_y);
            }
            ancestor = src.document.lock().unwrap().get(n).parent;
        }
        self.update_scroll_containers();
    }

    /// Apply CSS Scroll Snap L1 to a proposed page-level Y scroll offset.
    ///
    /// Finds the snap container whose node matches the root layout box (html
    /// element), overrides its rect with the viewport dimensions (the snap port
    /// for page scroll is the viewport, not the full document), then calls
    /// `find_snap_target`. Returns `target_y` unchanged if no snap applies.
    fn apply_page_y_snap(&self, target_y: f32) -> f32 {
        let root_node = match &self.layout_box {
            Some(lb) => lb.node,
            None => return target_y,
        };
        let vw = self.viewport_width_css();
        let vh = self.viewport_height_css();
        for sc in &self.snap_containers {
            if sc.node == root_node {
                // Proximity threshold uses viewport size, not full document size.
                let mut sc_viewport = sc.clone();
                sc_viewport.rect = lumen_core::geom::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                if let Some((_, sy)) = find_snap_target(
                    &sc_viewport,
                    (self.scroll_x, self.scroll_y),
                    (self.scroll_x, target_y),
                ) {
                    return clamp_scroll(sy, self.max_scroll());
                }
            }
        }
        target_y
    }

    /// Apply CSS Scroll Snap L1 to a proposed page-level X scroll offset.
    ///
    /// Mirror of `apply_page_y_snap` for horizontal scroll.
    fn apply_page_x_snap(&self, target_x: f32) -> f32 {
        let root_node = match &self.layout_box {
            Some(lb) => lb.node,
            None => return target_x,
        };
        let vw = self.viewport_width_css();
        let vh = self.viewport_height_css();
        for sc in &self.snap_containers {
            if sc.node == root_node {
                let mut sc_viewport = sc.clone();
                sc_viewport.rect = lumen_core::geom::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                if let Some((sx, _)) = find_snap_target(
                    &sc_viewport,
                    (self.scroll_x, self.scroll_y),
                    (target_x, self.scroll_y),
                ) {
                    return clamp_scroll(sx, self.max_scroll_x());
                }
            }
        }
        target_x
    }

    /// Р“РѕСЂРёР·РѕРЅС‚Р°Р»СЊРЅС‹Р№ СЃРєСЂРѕР»Р» РЅР° delta CSS px (РёРЅСЃС‚Р°РЅС‚РЅС‹Р№).
    pub(crate) fn scroll_x_by(&mut self, delta: f32) {
        let clamped = clamp_scroll(self.scroll_x + delta, self.max_scroll_x());
        let snapped = self.apply_page_x_snap(clamped);
        if (snapped - self.scroll_x).abs() > f32::EPSILON {
            self.scroll_x = snapped;
            self.request_redraw();
        }
    }

    /// РЈСЃС‚Р°РЅРѕРІРёС‚СЊ scroll_y РІ Р°Р±СЃРѕР»СЋС‚РЅРѕРµ Р·РЅР°С‡РµРЅРёРµ (РїРѕСЃР»Рµ clamping-Р°). `f32::INFINITY`
    /// = В«Рє СЃР°РјРѕРјСѓ РЅРёР·СѓВ», `0.0` = В«РІРІРµСЂС…В». Р—Р°РїСЂР°С€РёРІР°РµС‚ redraw С‚РѕР»СЊРєРѕ РµСЃР»Рё Р·РЅР°С‡РµРЅРёРµ
    /// РґРµР№СЃС‚РІРёС‚РµР»СЊРЅРѕ РёР·РјРµРЅРёР»РѕСЃСЊ вЂ” РёРЅР°С‡Рµ wheel-spam РІ СЃР°РјРѕРј РЅРёР·Сѓ РЅРµ РґС‘СЂРіР°Р» Р±С‹ GPU.
    ///
    /// РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РґР»СЏ РёРЅСЃС‚Р°РЅС‚-РїСѓС‚РµР№: drag thumb scrollbar-Р°. Р”Р»СЏ
    /// РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРёС… scroll-РєРѕРјР°РЅРґ (wheel / keys / page-jump / find) вЂ”
    /// `start_smooth_scroll` / `scroll_by_smooth`.
    pub(crate) fn scroll_to(&mut self, target: f32) {
        // РРЅСЃС‚Р°РЅС‚-РїСѓС‚СЊ cancel-РёС‚ Р°РєС‚РёРІРЅСѓСЋ Р°РЅРёРјР°С†РёСЋ вЂ” РјС‹ С‚РѕР»СЊРєРѕ С‡С‚Рѕ
        // *РїСЂРёРєР°Р·Р°Р»Рё* Р±С‹С‚СЊ РІ РєРѕРЅРєСЂРµС‚РЅРѕР№ С‚РѕС‡РєРµ.
        self.scroll_anim = None;
        let clamped = clamp_scroll(target, self.max_scroll());
        if (clamped - self.scroll_y).abs() > f32::EPSILON {
            self.scroll_y = clamped;
            self.request_redraw();
        }
    }

    /// Р—Р°РїСѓСЃС‚РёС‚СЊ smooth-scroll Рє target Y. Cancel-РёС‚ Р°РєС‚РёРІРЅСѓСЋ Р°РЅРёРјР°С†РёСЋ.
    /// Target РєР»Р°РјРїРёС‚СЃСЏ. Р•СЃР»Рё target == С‚РµРєСѓС‰РµРјСѓ scroll_y вЂ” Р°РЅРёРјР°С†РёСЏ РЅРµ
    /// СЃС‚Р°СЂС‚СѓРµС‚ (Рё С‚РµРєСѓС‰Р°СЏ СЃР±СЂР°СЃС‹РІР°РµС‚СЃСЏ). РџСЂРёРјРµРЅСЏРµС‚ CSS Scroll Snap L1 РµСЃР»Рё
    /// СЃС‚СЂР°РЅРёС†Р° РѕР±СЉСЏРІР»СЏРµС‚ `scroll-snap-type` РЅР° РєРѕСЂРЅРµРІРѕРј СЌР»РµРјРµРЅС‚Рµ.
    pub(crate) fn start_smooth_scroll(&mut self, target: f32) {
        let max = self.max_scroll();
        let target_clamped = clamp_scroll(target, max);
        // Apply page-level CSS Scroll Snap L1: snap to the nearest declared
        // snap point before starting the animation.
        let target_clamped = self.apply_page_y_snap(target_clamped);
        if (target_clamped - self.scroll_y).abs() <= f32::EPSILON {
            self.scroll_anim = None;
            return;
        }
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        self.scroll_anim = Some(scroll_anim::ScrollAnim {
            start_y: self.scroll_y,
            target_y: target_clamped,
            start_time_ms: now_ms,
        });
        self.request_redraw();
    }

    /// Smooth-РІР°СЂРёР°РЅС‚ `scroll_by`. Р•СЃР»Рё СѓР¶Рµ РёРґС‘С‚ Р°РЅРёРјР°С†РёСЏ вЂ” delta
    /// РґРѕР±Р°РІР»СЏРµС‚СЃСЏ Рє РµС‘ target-Сѓ, Р° РЅРµ Рє С‚РµРєСѓС‰РµРјСѓ scroll_y. Р­С‚Рѕ РїСЂР°РІРёР»СЊРЅР°СЏ
    /// СЃРµРјР°РЅС‚РёРєР° РґР»СЏ repeat-input (key-repeat, wheel-spam): РєР°Р¶РґРѕРµ
    /// РЅР°Р¶Р°С‚РёРµ РґРѕРїРёСЃС‹РІР°РµС‚ delta Рє С‚РѕС‡РєРµ РЅР°Р·РЅР°С‡РµРЅРёСЏ, Р° РЅРµ РґС‘СЂРіР°РµС‚ Р°РЅРёРјР°С†РёСЋ
    /// РІ РѕР±СЂР°С‚РЅСѓСЋ СЃС‚РѕСЂРѕРЅСѓ.
    pub(crate) fn scroll_by_smooth(&mut self, delta: f32) {
        let base = self.scroll_anim.as_ref().map_or(self.scroll_y, |a| a.target());
        self.start_smooth_scroll(base + delta);
    }

    /// Scroll the currently focused pane by `delta` CSS px.
    ///
    /// In split mode, routes to the right pane when it has focus; otherwise
    /// falls through to `scroll_by_smooth` for the left (active) pane.
    pub(crate) fn scroll_active_pane(&mut self, delta: f32) {
        // Pre-compute viewport height before mutably borrowing split_view.
        let vh = self.viewport_height_css();
        let right_focused = self
            .split_view
            .as_ref()
            .is_some_and(|sv| sv.focused == panels::split_view::SplitFocus::Right);
        if right_focused {
            if let Some(ref mut sv) = self.split_view {
                let max = (sv.right.content_height - vh).max(0.0);
                sv.right.scroll_y = (sv.right.scroll_y + delta).clamp(0.0, max);
            }
            self.request_redraw();
            return;
        }
        self.scroll_by_smooth(delta);
    }

    /// Scroll the currently focused pane to an absolute position.
    ///
    /// `target = f32::INFINITY` scrolls to the bottom of the pane's content.
    pub(crate) fn scroll_active_pane_to(&mut self, target: f32) {
        let vh = self.viewport_height_css();
        let right_focused = self
            .split_view
            .as_ref()
            .is_some_and(|sv| sv.focused == panels::split_view::SplitFocus::Right);
        if right_focused {
            if let Some(ref mut sv) = self.split_view {
                let max = (sv.right.content_height - vh).max(0.0);
                sv.right.scroll_y = target.clamp(0.0, max);
            }
            self.request_redraw();
            return;
        }
        self.start_smooth_scroll(target);
    }

    /// РўРёРє Р°РЅРёРјР°С†РёРё РїРµСЂРµРґ `Renderer::render`. Р•СЃР»Рё Р°РЅРёРјР°С†РёСЏ Р°РєС‚РёРІРЅР° вЂ”
    /// РѕР±РЅРѕРІР»СЏРµС‚ `scroll_y` РїРѕ out-cubic easing Рё РІРѕР·РІСЂР°С‰Р°РµС‚ `true`,
    /// СЃРёРіРЅР°Р»РёР·РёСЂСѓСЏ caller-Сѓ Р·Р°РїСЂРѕСЃРёС‚СЊ РµС‰С‘ РѕРґРёРЅ redraw. РЎР±СЂР°СЃС‹РІР°РµС‚
    /// `scroll_anim` РїРѕ Р·Р°РІРµСЂС€РµРЅРёРё.
    pub(crate) fn advance_scroll_anim(&mut self) -> bool {
        let Some(anim) = self.scroll_anim else {
            return false;
        };
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let (y, done) = anim.sample(now_ms);
        self.scroll_y = clamp_scroll(y, self.max_scroll());
        if done {
            self.scroll_anim = None;
            false
        } else {
            true
        }
    }

    /// ADR-016 M1.3: РїРµСЂРµРґР°С‚СЊ Р°РєС‚РёРІРЅСѓСЋ РёРЅРµСЂС†РёСЋ СЂРµРЅРґРµСЂ-РїРѕС‚РѕРєСѓ, С‡С‚РѕР±С‹ РїСЂРµР·РµРЅС‚Р°С†РёСЏ
    /// РїСЂРѕРґРѕР»Р¶Р°Р»Р°СЃСЊ РЅР° vsync, РґР°Р¶Рµ РµСЃР»Рё UI-РїРѕС‚РѕРє Р·Р°СЃС‚РѕРїРѕСЂРёС‚СЃСЏ (РґРѕР»РіРёР№ JS-С‚РёРє).
    /// No-op РЅР° РѕРґРЅРѕРїРѕС‚РѕС‡РЅРѕРј Р±СЌРєРµРЅРґРµ (РјРµС‚РѕРґ С‚СЂРµР№С‚Р° РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ РїСѓСЃС‚РѕР№), РїРѕСЌС‚РѕРјСѓ
    /// РїСЂРё РІС‹РєР»СЋС‡РµРЅРЅРѕРј `LUMEN_RENDER_THREAD` РїРѕРІРµРґРµРЅРёРµ РЅРµ РјРµРЅСЏРµС‚СЃСЏ.
    pub(crate) fn forward_momentum_start(&mut self, vel_y: f32, vel_x: f32) {
        let max_y = self.max_scroll();
        let max_x = self.max_scroll_x();
        if let Some(r) = self.renderer.as_mut() {
            r.start_render_momentum(vel_y, vel_x, max_y, max_x);
        }
    }

    /// ADR-016 M1.3: РѕС‚РјРµРЅРёС‚СЊ render-side РёРЅРµСЂС†РёСЋ (РЅРѕРІС‹Р№ Р¶РµСЃС‚, РЅР°РІРёРіР°С†РёСЏ, РєРѕРЅРµС†
    /// Р°РЅРёРјР°С†РёРё). No-op РЅР° РѕРґРЅРѕРїРѕС‚РѕС‡РЅРѕРј Р±СЌРєРµРЅРґРµ.
    pub(crate) fn forward_momentum_stop(&mut self) {
        if let Some(r) = self.renderer.as_mut() {
            r.stop_render_momentum();
        }
    }

    /// РўРёРє momentum-Р°РЅРёРјР°С†РёРё. РћР±РЅРѕРІР»СЏРµС‚ `scroll_y` / `scroll_x` РЅР°РїСЂСЏРјСѓСЋ
    /// (Р±РµР· smooth-scroll Р°РЅРёРјР°С†РёРё). Р’РѕР·РІСЂР°С‰Р°РµС‚ `true` РїРѕРєР° Р°РЅРёРјР°С†РёСЏ Р¶РёРІР°.
    pub(crate) fn advance_momentum(&mut self, now_ms: f64) -> bool {
        let Some(ref mut anim) = self.momentum_anim else {
            return false;
        };
        let (dy, dx, done) = anim.advance(now_ms);
        if dy != 0.0 {
            let new_y = clamp_scroll(self.scroll_y + dy, self.max_scroll());
            if (new_y - self.scroll_y).abs() > f32::EPSILON {
                self.scroll_y = new_y;
            }
        }
        if dx != 0.0 {
            let new_x = clamp_scroll(self.scroll_x + dx, self.max_scroll_x());
            if (new_x - self.scroll_x).abs() > f32::EPSILON {
                self.scroll_x = new_x;
            }
        }
        if done {
            self.momentum_anim = None;
            // РРЅРµСЂС†РёСЏ РёСЃСЃСЏРєР»Р° вЂ” СЃРЅСЏС‚СЊ РІР»Р°РґРµРЅРёРµ СЃ СЂРµРЅРґРµСЂ-РїРѕС‚РѕРєР° (РѕРЅ С‚Р°РєР¶Рµ
            // СЃР°РјРѕР·Р°РІРµСЂС€Р°РµС‚СЃСЏ РїРѕ С‚РѕРјСѓ Р¶Рµ РїРѕСЂРѕРіСѓ, РЅРѕ СЏРІРЅР°СЏ РѕС‚РјРµРЅР° РґРµС‚РµСЂРјРёРЅРёСЂСѓРµС‚).
            self.forward_momentum_stop();
            false
        } else {
            true
        }
    }
}
