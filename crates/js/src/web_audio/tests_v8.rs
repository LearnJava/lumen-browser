    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn rt_with_web_audio() -> V8JsRuntime {
        rt_with_web_audio_noise("https://example.test", false)
    }

    /// Like [`rt_with_web_audio`], but lets a BUG-908 test pick the origin and
    /// force the noise decision explicitly via
    /// [`super::install_web_audio_api_v8_inner`] instead of
    /// `LUMEN_DISABLE_AUDIO_NOISE` — the env var latches process-wide on
    /// first read, which would make a noise-enabled test and a noise-disabled
    /// test (e.g. `bug828_offline_render_is_not_silent`'s exact zero/non-zero
    /// count, the same assertion shape WPT's `webaudio/*` constant-source
    /// tests make) race each other for which answer wins, in the same binary.
    fn rt_with_web_audio_noise(origin: &str, noise_enabled: bool) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            if (typeof DOMException === 'undefined') {
                function DOMException(msg, name) {
                    var e = new Error(msg); e.name = name || 'Error'; return e;
                }
                globalThis.DOMException = DOMException;
            }
            "#,
        )
        .unwrap();
        super::install_web_audio_api_v8_inner(&rt, origin, noise_enabled).unwrap();
        rt
    }

    #[test]
    fn audio_context_classes_exist() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                typeof AudioContext === 'function'
                  && typeof OfflineAudioContext === 'function'
                  && typeof AudioBuffer === 'function'
                  && typeof AudioParam === 'function'
                  && typeof AudioNode === 'function'
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_context_initial_state() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac = new AudioContext();
                ac.state === 'running'
                  && typeof ac.currentTime === 'number'
                  && typeof ac.sampleRate === 'number'
                  && ac.destination instanceof AudioDestinationNode
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_context_suspend_resume_close() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac = new AudioContext();
                var suspendPromise = ac.suspend();
                var resumePromise  = ac.resume();
                var closePromise   = ac.close();
                suspendPromise instanceof Promise
                  && resumePromise instanceof Promise
                  && closePromise instanceof Promise
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_node_classes_exist() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                typeof GainNode === 'function'
                  && typeof OscillatorNode === 'function'
                  && typeof AudioBufferSourceNode === 'function'
                  && typeof ConstantSourceNode === 'function'
                  && typeof BiquadFilterNode === 'function'
                  && typeof AnalyserNode === 'function'
                  && typeof DelayNode === 'function'
                  && typeof DynamicsCompressorNode === 'function'
                  && typeof StereoPannerNode === 'function'
                  && typeof PannerNode === 'function'
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_context_factory_methods() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac = new AudioContext();
                var gain = ac.createGain();
                var osc  = ac.createOscillator();
                var buf  = ac.createBuffer(1, 100, 44100);
                var src  = ac.createBufferSource();
                var bq   = ac.createBiquadFilter();
                var an   = ac.createAnalyser();
                var cs   = ac.createConstantSource();
                gain instanceof GainNode
                  && osc  instanceof OscillatorNode
                  && buf  instanceof AudioBuffer
                  && src  instanceof AudioBufferSourceNode
                  && bq   instanceof BiquadFilterNode
                  && an   instanceof AnalyserNode
                  && cs   instanceof ConstantSourceNode
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn oscillator_node_type_and_freq() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac  = new AudioContext();
                var osc = ac.createOscillator();
                osc.type === 'sine'
                  && osc.frequency instanceof AudioParam
                  && osc.frequency.value === 440
                  && osc.detune instanceof AudioParam
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_node_connect_disconnect() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac   = new AudioContext();
                var gain = ac.createGain();
                var osc  = ac.createOscillator();
                var result = osc.connect(gain);
                var linked = gain._inputs.length === 1 && gain._inputs[0].node === osc;
                osc.disconnect();
                result === gain && osc._connections.length === 0
                  && linked && gain._inputs.length === 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_buffer_channel_data() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var buf = new AudioBuffer({ numberOfChannels: 2, length: 128, sampleRate: 44100 });
                buf.numberOfChannels === 2
                  && buf.length === 128
                  && buf.sampleRate === 44100
                  && buf.getChannelData(0) instanceof Float32Array
                  && buf.getChannelData(0).length === 128
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_param_set_value_at_time() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac   = new AudioContext();
                var gain = ac.createGain();
                gain.gain.value = 0.5;
                gain.gain.setValueAtTime(0.8, 0);
                gain.gain.value === 0.8
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn offline_audio_context_start_rendering() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var oac = new OfflineAudioContext(1, 44100, 44100);
                oac instanceof OfflineAudioContext
                  && oac.length === 44100
                  && oac.numberOfChannels === 1
                  && oac.startRendering() instanceof Promise
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn decode_audio_data_returns_promise() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac = new AudioContext();
                var buf = new ArrayBuffer(16);
                ac.decodeAudioData(buf) instanceof Promise
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// BUG-1088: `renderSizeHint` sets `renderQuantumSize`; invalid hints throw.
    #[test]
    fn bug1088_render_size_hint() {
        let rt = rt_with_web_audio();
        let v = rt
            .eval(
                r#"
                var bad = 0;
                ['bogus', 0, -1, 1.5e10].forEach(function(h) {
                  try { new AudioContext({renderSizeHint: h}); } catch (e) { if (e instanceof TypeError) bad++; }
                });
                [new AudioContext().renderQuantumSize,
                 new AudioContext({renderSizeHint: 256}).renderQuantumSize,
                 new AudioContext({renderSizeHint: 'hardware'}).renderQuantumSize,
                 new OfflineAudioContext({numberOfChannels:1, length:512, sampleRate:44100, renderSizeHint: 64}).renderQuantumSize,
                 bad].join(',')
                "#,
            )
            .unwrap();
        assert_eq!(v, JsValue::String("128,256,128,64,4".to_string()));
    }

    /// BUG-1090: node constructors validate the context and the options.
    #[test]
    fn bug1090_node_constructors_validate() {
        let rt = rt_with_web_audio();
        let v = rt
            .eval(
                r#"
                var c = new AudioContext();
                function name(f) { try { f(); return 'ok'; } catch (e) { return e.name; } }
                [name(function() { new AnalyserNode(); }),
                 name(function() { new AnalyserNode(1); }),
                 name(function() { new AnalyserNode(c, 42); }),
                 name(function() { new AnalyserNode(c, {fftSize: 33}); }),
                 name(function() { new PannerNode(c, {refDistance: -1}); }),
                 name(function() { new PannerNode(c, {coneOuterGain: 2}); }),
                 name(function() { new PannerNode(c, {channelCount: 3}); }),
                 name(function() { new GainNode(c, {channelCountMode: 'foo'}); }),
                 name(function() { new ChannelMergerNode(c, {numberOfInputs: 99}); }),
                 name(function() { new ChannelSplitterNode(c, {channelCount: 3}); }),
                 name(function() { new OscillatorNode(c, {type: 'custom'}); }),
                 name(function() { new AudioBuffer({length: 1}); }),
                 name(function() { new OfflineAudioContext(3, 42); }),
                 name(function() { new GainNode(c, {gain: 2}); })].join(',')
                "#,
            )
            .unwrap();
        assert_eq!(
            v,
            JsValue::String(
                "TypeError,TypeError,TypeError,IndexSizeError,RangeError,InvalidStateError,NotSupportedError,TypeError,IndexSizeError,InvalidStateError,InvalidStateError,TypeError,TypeError,ok"
                    .to_string()
            )
        );
    }

    /// BUG-1089: `sinkId`/`setSinkId()` and `playbackStats`/`playoutStats`.
    #[test]
    fn bug1089_sink_id_and_playback_stats() {
        let rt = rt_with_web_audio();
        let v = rt
            .eval(
                r#"
                var c = new AudioContext();
                var out = [c.sinkId === '', typeof c.setSinkId, c.playbackStats.totalDuration,
                  c.playoutStats.totalFramesDuration, typeof c.playbackStats.resetLatency,
                  Object.keys(c.playbackStats.toJSON()).length];
                var bad = 0;
                try { new AudioContext({sinkId: {type: 'x'}}); } catch (e) { if (e instanceof TypeError) bad++; }
                out.push(bad, new AudioContext({sinkId: {type: 'none'}}).sinkId instanceof AudioSinkInfo);
                out.join(',')
                "#,
            )
            .unwrap();
        assert_eq!(v, JsValue::String("true,function,0,0,function,6,1,true".to_string()));
    }

    /// BUG-591: `oncomplete` used to run inside a bare `catch (e) {}`, which
    /// is why every `webaudio/resources/audioparam-testing.js` comparison —
    /// all of which run from that handler — died without a word (BUG-828
    /// names this as the reason those files TIMEOUT instead of failing).
    ///
    /// The call is typeof-guarded, and this runtime is exactly why: it has no
    /// page DOM at all, so `_lumen_report_exception` does not exist unless a
    /// caller (here, the test) supplies one. An unguarded call would throw a
    /// `ReferenceError` from inside the `catch` and take the rest of the
    /// dispatch with it — worse than the swallow it replaces.
    #[test]
    fn bug591_offline_context_oncomplete_exception_is_reported() {
        let rt = rt_with_web_audio();
        let reported = rt
            .eval(
                r#"
                var seen = null;
                __lumen_C._lumen_report_exception = function(e) { seen = e.message; };
                var oc = new OfflineAudioContext(1, 128, 44100);
                oc.oncomplete = function() { throw new Error('oncomplete-boom'); };
                oc.startRendering();
                seen
                "#,
            )
            .unwrap();
        assert_eq!(reported, JsValue::String("oncomplete-boom".to_string()));
    }

    /// The other half of the guard: with no reporter installed the handler's
    /// exception must still not escape the shim (the DOM-less runtime is a
    /// real configuration — `--dump-*`, SVG rasterization, unit tests).
    #[test]
    fn bug591_offline_context_oncomplete_exception_without_reporter_is_contained() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var oc = new OfflineAudioContext(1, 128, 44100);
                oc.oncomplete = function() { throw new Error('no-reporter-boom'); };
                oc.startRendering();
                true
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn webkit_audio_context_alias() {
        let rt = rt_with_web_audio();
        let ok = rt.eval("typeof webkitAudioContext === 'function'").unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn analyser_frequency_bin_count() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac = new AudioContext();
                var an = ac.createAnalyser();
                an.fftSize === 2048 && an.fftSize / 2 === 1024
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    // ── BUG-828: the offline render itself ──────────────────────────────────
    //
    // This runtime has no `setTimeout`, so the shim's task queue falls back to
    // an inline call (documented at `_wa_task`) — which is what lets these
    // tests read the rendered buffer through `oncomplete` in the same `eval`.
    // On a page the same code is a task, as the spec requires.
    //
    // A `Promise` callback is a different matter: V8 drains its microtask
    // queue when the *script* ends, so a `.then` never runs mid-`eval`. Any
    // test that needs one splits into two evals — setup, then assertion.

    /// The defect this bug is named for: a graph playing for the whole render
    /// used to produce a buffer without a single non-zero sample.
    #[test]
    fn bug828_offline_render_is_not_silent() {
        let rt = rt_with_web_audio();
        let nonzero = rt
            .eval(
                r#"
                var ctx  = new OfflineAudioContext(1, 4410, 44100);
                var osc  = ctx.createOscillator();
                var gain = ctx.createGain();
                osc.connect(gain);
                gain.connect(ctx.destination);
                gain.gain.setValueAtTime(0.25, 0);
                gain.gain.linearRampToValueAtTime(1.0, 0.05);
                osc.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                var data = rendered.getChannelData(0);
                var n = 0;
                for (var i = 0; i < data.length; i++) if (data[i] !== 0) n++;
                n
                "#,
            )
            .unwrap();
        // A sine starting at phase 0 has its first sample at exactly zero;
        // every other frame of the 4410 carries signal.
        assert_eq!(nonzero, JsValue::Number(4409.0));
    }

    // ── BUG-908: ADR-007 Layer 4 audio fingerprint noise ─────────────────────

    /// The seed itself: stable for the same origin, different across origins
    /// (the property the whole defence rests on — see `audio_noise_seed`).
    #[test]
    fn bug908_noise_seed_differs_by_origin_and_is_stable() {
        let a1 = super::audio_noise_seed("https://a.example");
        let a2 = super::audio_noise_seed("https://a.example");
        let b = super::audio_noise_seed("https://b.example");
        assert_eq!(a1, a2, "same origin must reproduce the same seed");
        assert_ne!(a1, b, "different origins must not share a seed");
    }

    /// Renders a 128-sample constant-0.5 source and returns the *sum* of the
    /// channel, not one sample: each sample's noise is one of {-1,0,+1}·1e-7,
    /// so a single-sample comparison has a real one-in-three chance of two
    /// different origins landing on the same delta by luck. Summing 128 of
    /// them makes an all-samples collision between two different seeds
    /// astronomically unlikely while keeping the same ±(1e-7 · n) budget
    /// check meaningful. Returns the raw eval result (`JsValue::Number`); the
    /// `#[test]` callers unwrap it themselves — `clippy.toml`'s
    /// `allow-panic-in-tests` only covers a `#[test]` function's own body,
    /// not a shared non-`#[test]` helper (docs/lint-policy.md §10).
    fn rendered_constant_source_sum(origin: &str, noise_enabled: bool) -> JsValue {
        rendered_constant_source(origin, noise_enabled, "sum")
    }

    /// The same render as `rendered_constant_source_sum`, but every sample
    /// (comma-joined, `Float32` widened to a double so the text is exact)
    /// instead of their sum. A sum of 128 samples that each move by 0, or
    /// ±1e-7, collapses to a few dozen distinct `f32`-ULP steps, so two
    /// sessions' sums coincide by chance on a few percent of seeds (BUG-1168);
    /// the sample vectors coincide with probability 3^-128.
    fn rendered_constant_source_samples(origin: &str, noise_enabled: bool) -> JsValue {
        rendered_constant_source(origin, noise_enabled, "Array.prototype.join.call(data, ',')")
    }

    fn rendered_constant_source(origin: &str, noise_enabled: bool, result: &str) -> JsValue {
        let rt = rt_with_web_audio_noise(origin, noise_enabled);
        rt.eval(&
            r#"
            var ctx = new OfflineAudioContext(1, 128, 44100);
            var src = ctx.createConstantSource();
            src.offset.value = 0.5;
            src.connect(ctx.destination);
            src.start(0);
            var rendered = null;
            ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
            ctx.startRendering();
            var data = rendered.getChannelData(0), sum = 0;
            for (var i = 0; i < data.length; i++) sum += data[i];
            RESULT
            "#
            .replace("RESULT", result),
        )
        .unwrap()
    }

    /// The defect this bug is named for: two sessions (here, two origins
    /// standing in for two runs — see `rt_with_web_audio_noise`) rendering the
    /// same graph must NOT get the bit-identical sample, or hashing the
    /// buffer is a working fingerprint again.
    #[test]
    fn bug908_rendered_buffer_differs_across_sessions_when_noise_is_on() {
        let (JsValue::String(sa), JsValue::String(sb)) = (
            rendered_constant_source_samples("https://a.example", true),
            rendered_constant_source_samples("https://b.example", true),
        ) else {
            panic!("expected sample lists from both renders");
        };
        assert_ne!(sa, sb, "two sessions must not render a bit-identical buffer");
        let (JsValue::Number(a), JsValue::Number(b)) = (
            rendered_constant_source_sum("https://a.example", true),
            rendered_constant_source_sum("https://b.example", true),
        ) else {
            panic!("expected Number sums from both renders");
        };
        // ADR-007 Layer 4's own figure is ±1e-7 per sample over 128 samples;
        // the per-sample bound is widened to 2e-7 for the nearest-`f32`-ULP
        // rounding of that delta once added into the buffer's
        // `Float32Array` storage (~1.19e-7 near 0.5).
        let budget = 128.0 * 2e-7;
        assert!((a - 64.0).abs() <= budget, "noise exceeded its budget: {a}");
        assert!((b - 64.0).abs() <= budget, "noise exceeded its budget: {b}");
    }

    /// `LUMEN_DISABLE_AUDIO_NOISE`'s effect (here, the explicit
    /// `noise_enabled: false` `rt_with_web_audio_noise` takes instead of the
    /// env var — see that helper's doc comment): WPT's `webaudio/*`
    /// constant-source tests need this bit-exact reproducibility.
    #[test]
    fn bug908_rendered_buffer_is_bit_exact_when_noise_is_off() {
        let (JsValue::Number(a), JsValue::Number(b)) = (
            rendered_constant_source_sum("https://a.example", false),
            rendered_constant_source_sum("https://b.example", false),
        ) else {
            panic!("expected Number sums from both renders");
        };
        assert_eq!(a, 64.0);
        assert_eq!(b, 64.0);
    }

    /// `AudioBuffer.getChannelData()` must keep returning the same object on
    /// every call (`audiobuffer-getChannelData.html`) even with noise on —
    /// the noise is baked into the buffer once at render completion, not
    /// applied per read (see `_waApplyNoiseToBuffer`'s doc comment in the
    /// shim).
    #[test]
    fn bug908_get_channel_data_identity_holds_with_noise_on() {
        let rt = rt_with_web_audio_noise("https://a.example", true);
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 128, 44100);
                var src = ctx.createConstantSource();
                src.connect(ctx.destination);
                src.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                var a = rendered.getChannelData(0);
                var b = rendered.getChannelData(0);
                a === b && a[10] === b[10]
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// A script-authored buffer (`createBuffer()` + `copyToChannel()`) is the
    /// page's own content, not the implementation's — perturbing it would
    /// break real audio-editing use cases for no anti-fingerprinting benefit,
    /// so it must read back exactly what was written even with noise on.
    #[test]
    fn bug908_script_authored_buffer_is_never_perturbed() {
        let rt = rt_with_web_audio_noise("https://a.example", true);
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 128, 44100);
                var buf = ctx.createBuffer(1, 4, 44100);
                buf.copyToChannel(new Float32Array([0.5, 0.25, -0.5, 0]), 0);
                buf.getChannelData(0)[0] === 0.5
                  && buf.getChannelData(0)[1] === 0.25
                  && buf.getChannelData(0)[2] === -0.5
                  && buf.getChannelData(0)[3] === 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `AudioParam` automation reaches the rendered samples: a constant source
    /// under a linear gain ramp traces the ramp itself.
    #[test]
    fn bug828_param_automation_shapes_the_render() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx  = new OfflineAudioContext(1, 44100, 44100);
                var src  = ctx.createConstantSource();
                var gain = ctx.createGain();
                src.connect(gain);
                gain.connect(ctx.destination);
                gain.gain.setValueAtTime(0.0, 0);
                gain.gain.linearRampToValueAtTime(1.0, 1.0);
                src.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                var d = rendered.getChannelData(0);
                Math.abs(d[22050] - 0.5) < 0.001
                  && Math.abs(d[0]) < 0.001
                  && d[44099] > 0.99
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `setValueCurveAtTime` and `setTargetAtTime` land on the timeline too.
    #[test]
    fn bug828_param_curve_and_target_segments() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 128, 44100);
                var g = ctx.createGain();
                g.gain.setValueCurveAtTime([0, 1], 0, 1);
                var half = g.gain._valueAt(0.5);
                var g2 = ctx.createGain();
                g2.gain.setValueAtTime(1, 0);
                g2.gain.setTargetAtTime(0, 0, 1);
                var decayed = g2.gain._valueAt(1);
                Math.abs(half - 0.5) < 1e-6 && Math.abs(decayed - Math.exp(-1)) < 1e-6
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `ended` had no dispatch site at all: an `AudioBufferSourceNode` whose
    /// buffer runs out mid-render must fire it, in both handler forms.
    #[test]
    fn bug828_buffer_source_fires_ended() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 44100, 44100);
                var buf = ctx.createBuffer(1, 5512, 44100);
                var src = ctx.createBufferSource();
                src.buffer = buf;
                src.connect(ctx.destination);
                var viaProp = 0, viaListener = 0;
                src.onended = function() { viaProp++; };
                src.addEventListener('ended', function() { viaListener++; });
                src.start();
                var len = 0;
                ctx.oncomplete = function(e) { len = e.renderedBuffer.length; };
                ctx.startRendering();
                viaProp === 1 && viaListener === 1 && len === 44100
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// A stopped oscillator fires `ended` exactly once and stays silent after
    /// its stop time.
    #[test]
    fn bug828_oscillator_stop_fires_ended_once_and_goes_silent() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 44100, 44100);
                var osc = ctx.createOscillator();
                osc.connect(ctx.destination);
                var ended = 0;
                osc.onended = function() { ended++; };
                osc.start(0);
                osc.stop(0.5);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                var d = rendered.getChannelData(0);
                var tailNonzero = 0;
                for (var i = 30000; i < d.length; i++) if (d[i] !== 0) tailNonzero++;
                ended === 1 && tailNonzero === 0 && d[1000] !== 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `suspend(t)` used to be a bare `Promise.resolve()`. It must now stop the
    /// render at a quantum boundary with the context still open, and `resume()`
    /// must carry it to the end.
    #[test]
    fn bug828_offline_suspend_stops_the_render_and_resume_finishes_it() {
        let rt = rt_with_web_audio();
        rt.eval(
            r#"
            var ctx = new OfflineAudioContext(1, 44100, 44100);
            var osc = ctx.createOscillator();
            osc.connect(ctx.destination);
            osc.start();
            var atSuspend = null, stateAtSuspend = null, len = 0;
            ctx.suspend(0.5).then(function() {
                atSuspend = ctx.currentTime;
                stateAtSuspend = ctx.state;
                ctx.resume();
            });
            ctx.oncomplete = function(e) { len = e.renderedBuffer.length; };
            ctx.startRendering();
            "#,
        )
        .unwrap();
        // 0.5 s is 22050 frames, rounded up to the next 128-frame boundary:
        // 173 * 128 = 22144. The whole suspend/resume handshake runs on the
        // microtask queue V8 drains when the eval above returns.
        let ok = rt
            .eval(
                r#"
                stateAtSuspend === 'suspended'
                  && Math.abs(atSuspend - 22144 / 44100) < 1e-9
                  && len === 44100
                  && ctx.state === 'closed'
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// A node connected into an `AudioParam` is summed onto the automation
    /// curve rather than ignored.
    #[test]
    fn bug828_node_connected_to_param_is_summed() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 256, 44100);
                var carrier = ctx.createConstantSource();
                var gain = ctx.createGain();
                gain.gain.setValueAtTime(0.0, 0);
                var mod = ctx.createConstantSource();
                mod.offset.value = 0.5;
                mod.connect(gain.gain);
                mod.start(0);
                carrier.connect(gain);
                gain.connect(ctx.destination);
                carrier.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                Math.abs(rendered.getChannelData(0)[100] - 0.5) < 1e-6
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// A cycle in the graph must not recurse forever — the pull guard hands
    /// out silence for the second visit inside one quantum.
    #[test]
    fn bug828_feedback_cycle_terminates() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 256, 44100);
                var a = ctx.createGain(), b = ctx.createGain();
                a.connect(b); b.connect(a);
                a.connect(ctx.destination);
                var src = ctx.createConstantSource();
                src.connect(a); src.start(0);
                var len = 0;
                ctx.oncomplete = function(e) { len = e.renderedBuffer.length; };
                ctx.startRendering();
                len === 256
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// The merger routes input `j` onto output channel `j`, so a stereo
    /// destination gets two independently written channels.
    #[test]
    fn bug828_channel_merger_routes_inputs_to_channels() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(2, 256, 44100);
                var m = ctx.createChannelMerger(2);
                var l = ctx.createConstantSource(); l.offset.value = 0.25;
                var r = ctx.createConstantSource(); r.offset.value = 0.75;
                l.connect(m, 0, 0);
                r.connect(m, 0, 1);
                m.connect(ctx.destination);
                l.start(0); r.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                Math.abs(rendered.getChannelData(0)[10] - 0.25) < 1e-6
                  && Math.abs(rendered.getChannelData(1)[10] - 0.75) < 1e-6
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `startRendering()` a second time rejects instead of re-rendering.
    #[test]
    fn bug828_second_start_rendering_rejects() {
        let rt = rt_with_web_audio();
        rt.eval(
            r#"
            var ctx = new OfflineAudioContext(1, 128, 44100);
            var rejected = null;
            ctx.startRendering();
            ctx.startRendering().catch(function(e) { rejected = e.name; });
            "#,
        )
        .unwrap();
        let ok = rt.eval("rejected === 'InvalidStateError'").unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// BUG-1091: a rendered constant through one node, sampled at frame 200.
    fn render_one(setup: &str) -> JsValue {
        let rt = rt_with_web_audio();
        rt.eval(&format!(
            r#"
            var ctx = new OfflineAudioContext(2, 512, 44100);
            var src = ctx.createConstantSource(); src.offset.value = 1;
            {setup}
            src.start(0);
            var rendered = null;
            ctx.oncomplete = function(e) {{ rendered = e.renderedBuffer; }};
            ctx.startRendering();
            rendered.getChannelData(0)[200] + ',' + rendered.getChannelData(1)[200]
            "#
        ))
        .unwrap()
    }

    /// Inverse distance model: ref 1 at distance 2 gives gain 0.5, then the
    /// straight-ahead (azimuth 0) equal-power split applies cos/sin(pi/4).
    #[test]
    fn bug1091_panner_inverse_distance_attenuates() {
        let v = render_one(
            "var p = ctx.createPanner(); p.positionZ.value = -2; src.connect(p); p.connect(ctx.destination);",
        );
        let JsValue::String(s) = v else { panic!("string expected") };
        let l: f64 = s.split(',').next().unwrap().parse().unwrap();
        assert!((l - 0.5 * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-5, "got {s}");
    }

    /// A one-sample impulse response delays and scales (normalize off).
    #[test]
    fn bug1091_convolver_applies_impulse_response() {
        let v = render_one(
            "var b = ctx.createBuffer(1, 4, 44100); b.getChannelData(0)[2] = 0.5;              var c = ctx.createConvolver(); c.normalize = false; c.buffer = b;              src.connect(c); c.connect(ctx.destination);",
        );
        let JsValue::String(s) = v else { panic!("string expected") };
        let l: f64 = s.split(',').next().unwrap().parse().unwrap();
        assert!((l - 0.5).abs() < 1e-6, "got {s}");
    }

    /// Full-scale input above threshold comes out quieter than a bypass.
    #[test]
    fn bug1091_compressor_reduces_loud_signal() {
        let v = render_one(
            "var k = ctx.createDynamicsCompressor(); k.ratio.value = 20; k.threshold.value = -40;              src.connect(k); k.connect(ctx.destination);",
        );
        let JsValue::String(s) = v else { panic!("string expected") };
        let l: f64 = s.split(',').next().unwrap().parse().unwrap();
        assert!(l > 0.0 && l < 0.9, "got {s}");
    }
