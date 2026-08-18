//! Жизненный цикл внутреннего модуля.
//!
//! Все крупные подсистемы (network, storage, render, ...) реализуют этот
//! trait, чтобы shell мог их инициализировать и останавливать единообразно.
//! Плагины этот trait НЕ реализуют — у них своя capability-обвязка.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use crate::error::Result;

pub trait Module: Send + Sync {
    fn name(&self) -> &str;

    fn init(&mut self) -> Result<()> {
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}
