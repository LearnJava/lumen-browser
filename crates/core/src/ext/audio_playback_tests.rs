use super::*;

#[test]
fn null_provider_alloc_returns_unique_ids() {
    let p = NullAudioPlaybackProvider;
    let h1 = p.alloc_handle();
    let h2 = p.alloc_handle();
    assert_ne!(h1, h2);
}

#[test]
fn null_provider_ready_state_is_zero() {
    let p = NullAudioPlaybackProvider;
    let h = p.alloc_handle();
    assert_eq!(p.ready_state(h), 0);
}

#[test]
fn null_provider_is_paused_true() {
    let p = NullAudioPlaybackProvider;
    let h = p.alloc_handle();
    assert!(p.is_paused(h));
}

#[test]
fn null_provider_duration_is_nan() {
    let p = NullAudioPlaybackProvider;
    let h = p.alloc_handle();
    assert!(p.duration(h).is_nan());
}

#[test]
fn default_can_play_type_mp3() {
    let p = NullAudioPlaybackProvider;
    assert_eq!(p.can_play_type("audio/mpeg"), "probably");
}

#[test]
fn default_can_play_type_ogg() {
    let p = NullAudioPlaybackProvider;
    // Контейнер без codecs: внутри может быть Opus — только "maybe".
    assert_eq!(p.can_play_type("audio/ogg"), "maybe");
    assert_eq!(p.can_play_type("audio/ogg; codecs=vorbis"), "probably");
    assert_eq!(p.can_play_type("audio/ogg; codecs=opus"), "");
}

#[test]
fn default_can_play_type_wav() {
    let p = NullAudioPlaybackProvider;
    assert_eq!(p.can_play_type("audio/wav"), "probably");
}

#[test]
fn default_can_play_type_undecodable_empty() {
    // rodio собран только с wav/mp3/flac/vorbis (BUG-1619).
    let p = NullAudioPlaybackProvider;
    assert_eq!(p.can_play_type("audio/webm"), "");
    assert_eq!(p.can_play_type("audio/aac"), "");
    assert_eq!(p.can_play_type("audio/mp4"), "");
    assert_eq!(p.can_play_type("audio/mp4; codecs=\"mp4a.40.2\""), "");
}

#[test]
fn default_can_play_type_unknown_empty() {
    let p = NullAudioPlaybackProvider;
    assert_eq!(p.can_play_type("video/mp4"), "");
}

#[test]
fn default_can_play_type_with_codec_param() {
    let p = NullAudioPlaybackProvider;
    // MIME with codecs parameter — strip at ';'
    assert_eq!(p.can_play_type("audio/mpeg; codecs=mp3"), "probably");
}
