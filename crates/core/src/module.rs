//! Жизненный цикл внутреннего модуля.
//!
//! Все крупные подсистемы (network, storage, render, ...) реализуют этот
//! trait, чтобы shell мог их инициализировать и останавливать единообразно.
//! Плагины этот trait НЕ реализуют — у них своя capability-обвязка.

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
