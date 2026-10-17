//! GStreamer без линковки: `dlopen` + `dlsym` при первом обращении
//! (COMPAT-MEDIA-OSDEC-LINUX).
//!
//! Бинарник Lumen не зависит от GStreamer на этапе сборки и запуска: библиотеки
//! ищутся в системе при первой проверке `canPlayType`. Нет GStreamer — нет и
//! патентованных кодеков, а всё остальное (AV1, Vorbis) работает как раньше.
//!
//! Объявлены только функции, которые нужны конвейеру, с типами из
//! стабильного ABI GStreamer 1.x (он гарантирован в пределах ветки 1.x):
//! объекты непрозрачны, из структур читается лишь `GstMapInfo` и `GError`.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::sync::OnceLock;

/// `GstElement*` и прочие объекты GStreamer — непрозрачные указатели.
pub(super) type Obj = *mut c_void;

/// `GError` (GLib): домен, код, сообщение.
#[repr(C)]
pub(super) struct GError {
    pub domain: u32,
    pub code: c_int,
    pub message: *mut c_char,
}

/// `GList` (GLib): двусвязный список.
#[repr(C)]
pub(super) struct GList {
    pub data: *mut c_void,
    pub next: *mut GList,
    pub prev: *mut GList,
}

/// `GstMapInfo`: отображение памяти буфера (`gst_buffer_map`).
#[repr(C)]
pub(super) struct GstMapInfo {
    pub memory: *mut c_void,
    pub flags: c_uint,
    pub data: *mut u8,
    pub size: usize,
    pub maxsize: usize,
    pub user_data: [*mut c_void; 4],
    pub reserved: [*mut c_void; 4],
}

impl GstMapInfo {
    pub(super) fn zeroed() -> Self {
        Self {
            memory: std::ptr::null_mut(),
            flags: 0,
            data: std::ptr::null_mut(),
            size: 0,
            maxsize: 0,
            user_data: [std::ptr::null_mut(); 4],
            reserved: [std::ptr::null_mut(); 4],
        }
    }
}

/// `GST_MAP_READ`.
pub(super) const GST_MAP_READ: c_uint = 1;
/// `GST_STATE_NULL` / `GST_STATE_PLAYING`.
pub(super) const GST_STATE_NULL: c_int = 1;
pub(super) const GST_STATE_PLAYING: c_int = 4;
/// `GST_MESSAGE_ERROR`.
pub(super) const GST_MESSAGE_ERROR: c_uint = 1 << 1;
/// `GST_ELEMENT_FACTORY_TYPE_DECODER`.
pub(super) const FACTORY_TYPE_DECODER: u64 = 1;
/// `GST_RANK_MARGINAL`: ниже в списки не берём (так же поступает `decodebin`).
pub(super) const RANK_MARGINAL: c_int = 64;
/// `GST_FLOW_OK` (после него идут `GST_FLOW_FLUSHING` = 1 и ошибки < 0).
pub(super) const GST_FLOW_OK: c_int = 0;

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// `RTLD_NOW`: неразрешённые символы — ошибка сразу, а не при первом вызове.
const RTLD_NOW: c_int = 2;

/// Библиотека, открытая через `dlopen` (не закрывается до конца процесса).
struct Handle(*mut c_void);

impl Handle {
    fn open(names: &[&CStr]) -> Option<Self> {
        names.iter().find_map(|name| {
            // SAFETY: `name` — NUL-терминированная строка; `dlopen` не хранит указатель.
            let h = unsafe { dlopen(name.as_ptr(), RTLD_NOW) };
            (!h.is_null()).then_some(Self(h))
        })
    }

    /// Адрес функции; `None`, если символа нет.
    fn symbol(&self, name: &CStr) -> Option<*mut c_void> {
        // SAFETY: дескриптор получен от `dlopen` и не закрывается; имя — NUL-терминированное.
        let p = unsafe { dlsym(self.0, name.as_ptr()) };
        (!p.is_null()).then_some(p)
    }
}

/// Объявляет таблицу функций: поле на функцию, загрузка по имени из нужной библиотеки.
macro_rules! gst_api {
    ($( $lib:ident => $name:ident ( $($arg:ty),* ) $(-> $ret:ty)? ; )*) => {
        /// Таблица функций GStreamer / GLib, загруженных динамически.
        #[allow(missing_docs)]
        pub(super) struct Api {
            $( pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?, )*
        }

        impl Api {
            fn load(libs: &Libs) -> Result<Self, String> {
                Ok(Self {
                    $(
                        $name: {
                            let name = concat!(stringify!($name), "\0");
                            // SAFETY: имя из литерала заканчивается NUL, внутренних NUL нет.
                            let cname = unsafe { CStr::from_bytes_with_nul_unchecked(name.as_bytes()) };
                            let p = libs.$lib.symbol(cname).ok_or_else(|| {
                                format!("в библиотеке GStreamer нет функции {}", stringify!($name))
                            })?;
                            // SAFETY: тип поля — сигнатура функции из заголовков GStreamer 1.x
                            // (ABI стабилен), указатель получен `dlsym` из той же библиотеки.
                            unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(p) }
                        },
                    )*
                })
            }
        }
    };
}

struct Libs {
    gst: Handle,
    app: Handle,
    glib: Handle,
}

gst_api! {
    gst => gst_init_check(*mut c_int, *mut *mut *mut c_char, *mut *mut GError) -> c_int;
    gst => gst_parse_launch(*const c_char, *mut *mut GError) -> Obj;
    gst => gst_bin_get_by_name(Obj, *const c_char) -> Obj;
    gst => gst_element_set_state(Obj, c_int) -> c_int;
    gst => gst_element_get_bus(Obj) -> Obj;
    gst => gst_bus_pop_filtered(Obj, c_uint) -> Obj;
    gst => gst_message_parse_error(Obj, *mut *mut GError, *mut *mut c_char);
    gst => gst_mini_object_unref(Obj);
    gst => gst_object_unref(Obj);
    gst => gst_caps_from_string(*const c_char) -> Obj;
    gst => gst_caps_get_structure(Obj, c_uint) -> Obj;
    gst => gst_structure_get_int(Obj, *const c_char, *mut c_int) -> c_int;
    gst => gst_buffer_new_allocate(Obj, usize, Obj) -> Obj;
    gst => gst_buffer_fill(Obj, usize, *const c_void, usize) -> usize;
    gst => gst_buffer_map(Obj, *mut GstMapInfo, c_uint) -> c_int;
    gst => gst_buffer_unmap(Obj, *mut GstMapInfo);
    gst => gst_sample_get_buffer(Obj) -> Obj;
    gst => gst_sample_get_caps(Obj) -> Obj;
    gst => gst_element_factory_find(*const c_char) -> Obj;
    gst => gst_element_factory_list_get_elements(u64, c_int) -> *mut GList;
    gst => gst_element_factory_can_sink_any_caps(Obj, Obj) -> c_int;
    gst => gst_plugin_feature_list_free(*mut GList);
    gst => gst_plugin_feature_set_rank(Obj, c_uint);
    gst => gst_plugin_feature_get_name(Obj) -> *const c_char;
    app => gst_app_src_set_caps(Obj, Obj);
    app => gst_app_src_push_buffer(Obj, Obj) -> c_int;
    app => gst_app_src_end_of_stream(Obj) -> c_int;
    app => gst_app_sink_try_pull_sample(Obj, u64) -> Obj;
    app => gst_app_sink_is_eos(Obj) -> c_int;
    glib => g_error_free(*mut GError);
    glib => g_free(*mut c_void);
}

// SAFETY: таблица — неизменяемые указатели на функции; GStreamer потокобезопасен.
unsafe impl Send for Api {}
// SAFETY: см. выше — общий доступ к указателям на функции безопасен.
unsafe impl Sync for Api {}

/// GStreamer, инициализированный один раз на процесс; `Err` — почему его нет.
pub(super) fn api() -> Result<&'static Api, &'static str> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(load).as_ref().map_err(String::as_str)
}

fn load() -> Result<Api, String> {
    let gst = Handle::open(&[c"libgstreamer-1.0.so.0"]).ok_or("libgstreamer-1.0.so.0 не найдена")?;
    let app = Handle::open(&[c"libgstapp-1.0.so.0"]).ok_or("libgstapp-1.0.so.0 не найдена")?;
    let glib = Handle::open(&[c"libglib-2.0.so.0"]).ok_or("libglib-2.0.so.0 не найдена")?;
    let api = Api::load(&Libs { gst, app, glib })?;
    let mut err: *mut GError = std::ptr::null_mut();
    // SAFETY: `argc`/`argv` — NULL (GStreamer не разбирает командную строку), `err` — выходной параметр.
    let ok = unsafe { (api.gst_init_check)(std::ptr::null_mut(), std::ptr::null_mut(), &raw mut err) };
    if ok == 0 {
        return Err(format!("gst_init_check: {}", take_error(&api, err)));
    }
    Ok(api)
}

/// Текст `GError` и его освобождение.
pub(super) fn take_error(api: &Api, err: *mut GError) -> String {
    if err.is_null() {
        return "неизвестная ошибка".to_string();
    }
    // SAFETY: `err` выдан GLib и не освобождён; `message` — NUL-терминированная строка или NULL.
    let text = unsafe {
        let msg = (*err).message;
        let text = if msg.is_null() { "без текста".to_string() } else { CStr::from_ptr(msg).to_string_lossy().into_owned() };
        (api.g_error_free)(err);
        text
    };
    text
}
