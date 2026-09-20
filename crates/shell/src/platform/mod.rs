//! Platform-specific shell integrations (OS clipboard, file dialog, audio capture, etc.).

pub mod audio_capture;
pub mod audio_player;
pub mod clipboard;
pub mod dark_mode;
pub mod display_color_profile;
pub mod file_dialog;
pub mod screen_capture;
#[cfg(feature = "ffmpeg-video")]
pub mod video_audio_sink;
pub mod wake_lock;
