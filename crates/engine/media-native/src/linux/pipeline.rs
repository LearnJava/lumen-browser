//! Конвейер GStreamer `appsrc ! decodebin ! … ! appsink`: пакеты внутрь, готовые
//! кадры или PCM наружу (COMPAT-MEDIA-OSDEC-LINUX).

use std::ffi::{c_void, CString};
use std::time::Duration;

use super::ffi::{
    api, take_error, Api, GstMapInfo, Obj, GST_FLOW_OK, GST_MAP_READ, GST_MESSAGE_ERROR, GST_STATE_NULL,
    GST_STATE_PLAYING,
};

/// Работающий конвейер. Объекты GStreamer потокобезопасны, поэтому `Send`.
pub(super) struct Pipeline {
    api: &'static Api,
    pipeline: Obj,
    src: Obj,
    sink: Obj,
    bus: Obj,
}

// SAFETY: указатели принадлежат только этому значению, а API GStreamer допускает
// вызовы из любого потока (объекты синхронизируются сами).
unsafe impl Send for Pipeline {}

impl Pipeline {
    /// Построить конвейер по описанию (`appsrc name=src … appsink name=sink`),
    /// назначить `appsrc` caps входа и запустить.
    pub(super) fn new(description: &str, input_caps: &str) -> Result<Self, String> {
        let api = api().map_err(str::to_owned)?;
        let text = CString::new(description).map_err(|_| "описание конвейера содержит NUL".to_string())?;
        let mut err = std::ptr::null_mut();
        // SAFETY: строка NUL-терминирована, `err` — выходной параметр.
        let pipeline = unsafe { (api.gst_parse_launch)(text.as_ptr(), &raw mut err) };
        if pipeline.is_null() {
            return Err(format!("gst_parse_launch: {}", take_error(api, err)));
        }
        if !err.is_null() {
            // Предупреждение разбора при удачном результате: текст не нужен.
            let _ = take_error(api, err);
        }
        // С этого момента `Drop` освободит всё, что успели получить.
        let mut p = Self {
            api,
            pipeline,
            src: std::ptr::null_mut(),
            sink: std::ptr::null_mut(),
            bus: std::ptr::null_mut(),
        };
        // SAFETY: `pipeline` — живой `GstBin`; имена — NUL-терминированные литералы.
        p.src = unsafe { (api.gst_bin_get_by_name)(pipeline, c"src".as_ptr()) };
        // SAFETY: то же.
        p.sink = unsafe { (api.gst_bin_get_by_name)(pipeline, c"sink".as_ptr()) };
        if p.src.is_null() || p.sink.is_null() {
            return Err("в конвейере нет appsrc/appsink".to_string());
        }
        let caps_text = CString::new(input_caps).map_err(|_| "caps содержат NUL".to_string())?;
        // SAFETY: строка NUL-терминирована.
        let caps = unsafe { (api.gst_caps_from_string)(caps_text.as_ptr()) };
        if caps.is_null() {
            return Err(format!("GStreamer не разобрал caps: {input_caps}"));
        }
        // SAFETY: `src` — `GstAppSrc`; `appsrc` берёт свою ссылку на caps, нашу отпускаем.
        unsafe {
            (api.gst_app_src_set_caps)(p.src, caps);
            (api.gst_mini_object_unref)(caps);
        }
        // SAFETY: `pipeline` — живой элемент.
        p.bus = unsafe { (api.gst_element_get_bus)(pipeline) };
        // SAFETY: то же; результат `0` — `GST_STATE_CHANGE_FAILURE`.
        if unsafe { (api.gst_element_set_state)(pipeline, GST_STATE_PLAYING) } == 0 {
            return Err(p.error().unwrap_or_else(|| "конвейер не запустился".to_string()));
        }
        Ok(p)
    }

    /// Отправить в `appsrc` один пакет (копия байтов).
    pub(super) fn push(&self, data: &[u8]) -> Result<(), String> {
        // SAFETY: аллокатор и параметры — NULL (по умолчанию).
        let buffer = unsafe { (self.api.gst_buffer_new_allocate)(std::ptr::null_mut(), data.len(), std::ptr::null_mut()) };
        if buffer.is_null() {
            return Err("GStreamer не выделил буфер".to_string());
        }
        // SAFETY: буфер размером `data.len()`, источник — тот же срез.
        unsafe { (self.api.gst_buffer_fill)(buffer, 0, data.as_ptr().cast::<c_void>(), data.len()) };
        // SAFETY: `appsrc` забирает буфер себе в любом исходе (ссылку мы не держим).
        let flow = unsafe { (self.api.gst_app_src_push_buffer)(self.src, buffer) };
        if flow == GST_FLOW_OK { Ok(()) } else { Err(self.error().unwrap_or_else(|| format!("appsrc: GstFlowReturn {flow}"))) }
    }

    /// Больше пакетов не будет: декодеры сольют внутренние очереди.
    pub(super) fn end_of_stream(&self) {
        // SAFETY: `src` — живой `GstAppSrc`.
        unsafe { (self.api.gst_app_src_end_of_stream)(self.src) };
    }

    /// Следующий результат из `appsink`; `None` — за `timeout` ничего не появилось
    /// или поток закончился.
    pub(super) fn pull(&self, timeout: Duration) -> Option<Sample> {
        let ns = u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX);
        // SAFETY: `sink` — живой `GstAppSink`.
        let raw = unsafe { (self.api.gst_app_sink_try_pull_sample)(self.sink, ns) };
        // Лениво: `then_some` собрал бы `Sample` и при NULL, а его `Drop` освободил бы пустой указатель.
        (!raw.is_null()).then(|| Sample { api: self.api, raw })
    }

    /// Поток закончился (после `end_of_stream` все результаты выбраны).
    pub(super) fn is_eos(&self) -> bool {
        // SAFETY: `sink` — живой `GstAppSink`.
        unsafe { (self.api.gst_app_sink_is_eos)(self.sink) != 0 }
    }

    /// Первая необработанная ошибка конвейера (нет декодера, повреждённый поток…).
    pub(super) fn error(&self) -> Option<String> {
        if self.bus.is_null() {
            return None;
        }
        // SAFETY: `bus` — живая шина конвейера.
        let msg = unsafe { (self.api.gst_bus_pop_filtered)(self.bus, GST_MESSAGE_ERROR) };
        if msg.is_null() {
            return None;
        }
        let mut err = std::ptr::null_mut();
        let mut debug = std::ptr::null_mut();
        // SAFETY: `msg` — сообщение об ошибке; выходные параметры инициализированы NULL.
        unsafe { (self.api.gst_message_parse_error)(msg, &raw mut err, &raw mut debug) };
        let mut text = take_error(self.api, err);
        if !debug.is_null() {
            // SAFETY: `debug` — NUL-терминированная строка GLib, освобождаем `g_free`.
            unsafe {
                let detail = std::ffi::CStr::from_ptr(debug).to_string_lossy().into_owned();
                (self.api.g_free)(debug.cast::<c_void>());
                if !detail.is_empty() {
                    text.push_str(" (");
                    text.push_str(&detail);
                    text.push(')');
                }
            }
        }
        // SAFETY: освобождаем свою ссылку на сообщение.
        unsafe { (self.api.gst_mini_object_unref)(msg) };
        Some(text)
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        // SAFETY: каждый ненулевой указатель — наша ссылка; конвейер останавливаем
        // до освобождения, чтобы потоки GStreamer не трогали уже закрытые объекты.
        unsafe {
            (self.api.gst_element_set_state)(self.pipeline, GST_STATE_NULL);
            for obj in [self.sink, self.src, self.bus, self.pipeline] {
                if !obj.is_null() {
                    (self.api.gst_object_unref)(obj);
                }
            }
        }
    }
}

/// `GstSample` из `appsink`.
pub(super) struct Sample {
    api: &'static Api,
    raw: Obj,
}

impl Sample {
    /// Целое поле первой структуры caps образца (`width`, `height`…).
    pub(super) fn caps_int(&self, field: &std::ffi::CStr) -> Option<i32> {
        // SAFETY: образец жив; caps принадлежат образцу (ссылку не берём).
        let caps = unsafe { (self.api.gst_sample_get_caps)(self.raw) };
        if caps.is_null() {
            return None;
        }
        // SAFETY: `caps` живы; индекс 0 проверяется результатом.
        let structure = unsafe { (self.api.gst_caps_get_structure)(caps, 0) };
        if structure.is_null() {
            return None;
        }
        let mut value = 0;
        // SAFETY: структура жива, имя — NUL-терминированная строка, `value` — выходной параметр.
        let ok = unsafe { (self.api.gst_structure_get_int)(structure, field.as_ptr(), &raw mut value) };
        (ok != 0).then_some(value)
    }

    /// Отдать байты буфера образца `f` (на время отображения памяти).
    pub(super) fn with_data<R>(&self, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        // SAFETY: образец жив; буфер принадлежит образцу.
        let buffer = unsafe { (self.api.gst_sample_get_buffer)(self.raw) };
        if buffer.is_null() {
            return None;
        }
        let mut info = GstMapInfo::zeroed();
        // SAFETY: `info` — структура нужного размера; режим — чтение.
        if unsafe { (self.api.gst_buffer_map)(buffer, &raw mut info, GST_MAP_READ) } == 0 {
            return None;
        }
        let bytes: &[u8] = if info.data.is_null() || info.size == 0 {
            &[]
        } else {
            // SAFETY: пока память отображена, `data` указывает на `size` читаемых байт.
            unsafe { std::slice::from_raw_parts(info.data, info.size) }
        };
        let result = f(bytes);
        // SAFETY: отображение сделано выше этим же `info`.
        unsafe { (self.api.gst_buffer_unmap)(buffer, &raw mut info) };
        Some(result)
    }
}

impl Drop for Sample {
    fn drop(&mut self) {
        // SAFETY: владеем одной ссылкой на образец.
        unsafe { (self.api.gst_mini_object_unref)(self.raw) };
    }
}
