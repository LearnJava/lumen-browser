//! What a recognized mouse gesture does.
//!
//! The right-button drag and its classification into an action are
//! `crate::input::gesture`; this is the other half - running the action
//! against the live shell, which for every action defined so far means a
//! navigation or a tab command.

use crate::*;

impl Lumen {
    /// Execute a gesture action produced by the right-button drag recognizer.
    pub(crate) fn execute_gesture_action(
        &mut self,
        action: input::gesture::GestureAction,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        use input::gesture::GestureAction;
        match action {
            GestureAction::NavigateBack => self.navigate_back(),
            GestureAction::NavigateForward => self.navigate_forward(),
            GestureAction::NewTab => self.open_new_tab(),
            GestureAction::CloseTab => {
                let idx = self.tab_strip.active;
                self.close_tab(idx, event_loop);
            }
        }
    }
}
