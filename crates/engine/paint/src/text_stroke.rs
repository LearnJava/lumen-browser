//! `-webkit-text-stroke`: общая для бэкендов подстановка `DrawTextStroke` →
//! `DrawSvgStroke`. Контуры глифов снимает CPU-путь шрифтов
//! (`cpu_raster/text_stroke.rs`), поэтому без фичи `cpu-render` обводка не
//! рисуется (команда пропускается каждым бэкендом).

use std::borrow::Cow;

use lumen_core::FontProvider;

use crate::DisplayCommand;

/// Список без `DrawTextStroke` (почти всегда) возвращается как есть, без
/// копирования; иначе каждая такая команда заменяется на `DrawSvgStroke` с
/// контурами глифов (или удаляется, если штриховать нечего).
#[must_use]
pub(crate) fn expand_all<'a>(
    commands: &'a [DisplayCommand],
    provider: Option<&dyn FontProvider>,
) -> Cow<'a, [DisplayCommand]> {
    if !commands.iter().any(|c| matches!(c, DisplayCommand::DrawTextStroke { .. })) {
        return Cow::Borrowed(commands);
    }
    let mut out = Vec::with_capacity(commands.len());
    for cmd in commands {
        if matches!(cmd, DisplayCommand::DrawTextStroke { .. }) {
            #[cfg(feature = "cpu-render")]
            if let Some(stroke) = crate::cpu_raster::text_stroke::expand(cmd, provider) {
                out.push(stroke);
            }
            #[cfg(not(feature = "cpu-render"))]
            let _ = provider;
        } else {
            out.push(cmd.clone());
        }
    }
    Cow::Owned(out)
}
