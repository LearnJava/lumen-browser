//! Bounded-parallelism helper for subresource fetching.
//!
//! Every subresource pass of the load pipeline (stylesheets, images, scripts,
//! frames) is I/O-bound on the network, and doing it sequentially on the UI
//! thread was the main cost of a heavy page — hence [`parallel_map`], which
//! keeps result order while capping the thread count.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3d); behaviour and
//! signatures are unchanged.

use crate::*;

/// РњР°РєСЃРёРјСѓРј РѕРґРЅРѕРІСЂРµРјРµРЅРЅС‹С… РїРѕС‚РѕРєРѕРІ Р·Р°РіСЂСѓР·РєРё РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ РѕРґРЅРѕРіРѕ С‚РёРїР°.
/// РџРѕРґРѕР±СЂР°РЅРѕ РїРѕРґ С‚РёРїРёС‡РЅС‹Р№ Р±СЂР°СѓР·РµСЂРЅС‹Р№ Р»РёРјРёС‚ СЃРѕРµРґРёРЅРµРЅРёР№ РЅР° С…РѕСЃС‚ (~6) + Р·Р°РїР°СЃ.
/// Р Р°Р±РѕС‚Р° I/O-bound (СЃРµС‚РµРІРѕР№ fetch), РїРѕС‚РѕРєРё РІ РѕСЃРЅРѕРІРЅРѕРј СЃРїСЏС‚ РЅР° СЃРѕРєРµС‚Рµ, РїРѕСЌС‚РѕРјСѓ
/// РЅРµР±РѕР»СЊС€РѕР№ РѕРІРµСЂСЃР°Р±СЃРєСЂРёРїС€РЅ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ С‡РёСЃР»Р° СЏРґРµСЂ РґРѕРїСѓСЃС‚РёРј Рё РїРѕР»РµР·РµРЅ.
const MAX_PARALLEL_FETCHES: usize = 8;

/// РџСЂРёРјРµРЅРёС‚СЊ `f` Рє РєР°Р¶РґРѕРјСѓ СЌР»РµРјРµРЅС‚Сѓ `items` РїР°СЂР°Р»Р»РµР»СЊРЅРѕ, РЎРћРҐР РђРќРЇРЇ РїРѕСЂСЏРґРѕРє
/// СЂРµР·СѓР»СЊС‚Р°С‚РѕРІ (СЂРµР·СѓР»СЊС‚Р°С‚ `i` СЃРѕРѕС‚РІРµС‚СЃС‚РІСѓРµС‚ `items[i]`). Р§РёСЃР»Рѕ РѕРґРЅРѕРІСЂРµРјРµРЅРЅС‹С…
/// РїРѕС‚РѕРєРѕРІ РѕРіСЂР°РЅРёС‡РµРЅРѕ `MAX_PARALLEL_FETCHES`, С‡С‚РѕР±С‹ РЅРµ РїР»РѕРґРёС‚СЊ РїРѕ РїРѕС‚РѕРєСѓ РЅР°
/// РєР°Р¶РґС‹Р№ РїРѕРґСЂРµСЃСѓСЂСЃ вЂ” С‚СЏР¶С‘Р»С‹Рµ СЃС‚СЂР°РЅРёС†С‹ РёРјРµСЋС‚ РґРµСЃСЏС‚РєРё РєР°СЂС‚РёРЅРѕРє/СЃРєСЂРёРїС‚РѕРІ.
///
/// Р“Р»Р°РІРЅС‹Р№ С‚РѕСЂРјРѕР· С‚СЏР¶С‘Р»С‹С… СЃС‚СЂР°РЅРёС† вЂ” РїРѕСЃР»РµРґРѕРІР°С‚РµР»СЊРЅР°СЏ СЃРµС‚РµРІР°СЏ Р·Р°РіСЂСѓР·РєР°
/// РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ РІ UI-РїРѕС‚РѕРєРµ (РґРёР°РіРЅРѕР· 2026-06-16: ~5.3 СЃ РёР· 7.5 СЃ РЅР° lenta.ru).
/// РџР°СЂР°Р»Р»РµР»РёР·Р°С†РёСЏ fetch'Р° РґР°С‘С‚ Г—5вЂ“6 РїРѕ Р·Р°РјРµСЂР°Рј curl. Р”РµРєРѕРґРёСЂРѕРІР°РЅРёРµ (CPU)
/// РїСЂРё Р¶РµР»Р°РЅРёРё С‚РѕР¶Рµ РІС‹РїРѕР»РЅСЏРµС‚СЃСЏ РІРЅСѓС‚СЂРё `f`, СЂР°Р·СЉРµР·Р¶Р°СЏСЃСЊ РїРѕ РїРѕС‚РѕРєР°Рј.
///
/// `f` РїРѕР»СѓС‡Р°РµС‚ `(РёРЅРґРµРєСЃ, &СЌР»РµРјРµРЅС‚)` Рё РґРѕР»Р¶РЅР° Р±С‹С‚СЊ `Sync` (РІС‹Р·С‹РІР°РµС‚СЃСЏ РёР· РІСЃРµС…
/// РїРѕС‚РѕРєРѕРІ). РџР°РЅРёРєРё РІРЅСѓС‚СЂРё `f` РЅРµ РіР»РѕС‚Р°СЋС‚СЃСЏ вЂ” РїСЂРѕР±СЂР°СЃС‹РІР°СЋС‚СЃСЏ РїСЂРё join.
#[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
#[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
pub(crate) fn parallel_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
{
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = items.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        // РћРґРёРЅ СЌР»РµРјРµРЅС‚ вЂ” РЅРµ РїРѕРґРЅРёРјР°РµРј РїРѕС‚РѕРєРё.
        return vec![f(0, &items[0])];
    }

    let workers = MAX_PARALLEL_FETCHES.min(n);
    let next = AtomicUsize::new(0);
    // РџРѕ РѕРґРЅРѕР№ СЏС‡РµР№РєРµ РЅР° СЂРµР·СѓР»СЊС‚Р°С‚; РІРѕСЂРєРµСЂС‹ РїРёС€СѓС‚ СЃС‚СЂРѕРіРѕ РІ СЃРІРѕР№ РёРЅРґРµРєСЃ, РіРѕРЅРѕРє РЅРµС‚.
    let slots: Vec<Mutex<Option<R>>> = (0..n).map(|_| Mutex::new(None)).collect();
    let f = &f;
    let slots_ref = &slots;
    let next_ref = &next;

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(move || loop {
                let i = next_ref.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let r = f(i, &items[i]);
                *slots_ref[i].lock().unwrap() = Some(r);
            });
        }
    });

    slots
        .into_iter()
        .map(|cell| cell.into_inner().unwrap().expect("worker filled every slot"))
        .collect()
}
