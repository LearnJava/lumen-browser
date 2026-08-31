use super::*;

/// HTML5 form input types (HTML Standard §4.10.5). Спека определяет
/// 22 значения; Phase 0 кладёт все известные + `Other(String)` для
/// forward-compat. Тип `text` — default (если атрибут отсутствует или
/// не распознан); прочие неизвестные → `Other` (UI может render-ить как text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputType {
    /// `text` (default) — однострочное текстовое поле.
    Text,
    /// `password` — обфусцированный ввод.
    Password,
    /// `email` — формальная email-валидация.
    Email,
    /// `tel` — номер телефона (нет жёсткого формата).
    Tel,
    /// `url` — URL (формальная валидация).
    Url,
    /// `number` — численный ввод с stepper-ом.
    Number,
    /// `search` — текстовое поле с UI-варьированием (clear button).
    Search,
    /// `date` — date picker.
    Date,
    /// `datetime-local` — date+time picker.
    DateTimeLocal,
    /// `time` — time picker.
    Time,
    /// `month` — month/year picker.
    Month,
    /// `week` — week picker.
    Week,
    /// `color` — color picker.
    Color,
    /// `range` — slider.
    Range,
    /// `checkbox` — boolean checkbox.
    Checkbox,
    /// `radio` — radio button (один из группы по `name`).
    Radio,
    /// `file` — file upload.
    File,
    /// `submit` — submit button.
    Submit,
    /// `reset` — reset-form button.
    Reset,
    /// `button` — generic button (без submit-behavior).
    Button,
    /// `image` — submit button с изображением.
    Image,
    /// `hidden` — невидимое поле для server-side данных.
    Hidden,
    /// Forward-compat для не-описанных типов (или typo в HTML).
    Other(String),
}

impl InputType {
    /// Распарсить значение `type`-атрибута. Case-insensitive по
    /// HTML5 §4.10.5.1.4 «Attribute idioms».
    pub fn parse(s: &str) -> Self {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "text" | "" => Self::Text,
            "password" => Self::Password,
            "email" => Self::Email,
            "tel" => Self::Tel,
            "url" => Self::Url,
            "number" => Self::Number,
            "search" => Self::Search,
            "date" => Self::Date,
            "datetime-local" => Self::DateTimeLocal,
            "time" => Self::Time,
            "month" => Self::Month,
            "week" => Self::Week,
            "color" => Self::Color,
            "range" => Self::Range,
            "checkbox" => Self::Checkbox,
            "radio" => Self::Radio,
            "file" => Self::File,
            "submit" => Self::Submit,
            "reset" => Self::Reset,
            "button" => Self::Button,
            "image" => Self::Image,
            "hidden" => Self::Hidden,
            _ => Self::Other(lc),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Text => "text",
            Self::Password => "password",
            Self::Email => "email",
            Self::Tel => "tel",
            Self::Url => "url",
            Self::Number => "number",
            Self::Search => "search",
            Self::Date => "date",
            Self::DateTimeLocal => "datetime-local",
            Self::Time => "time",
            Self::Month => "month",
            Self::Week => "week",
            Self::Color => "color",
            Self::Range => "range",
            Self::Checkbox => "checkbox",
            Self::Radio => "radio",
            Self::File => "file",
            Self::Submit => "submit",
            Self::Reset => "reset",
            Self::Button => "button",
            Self::Image => "image",
            Self::Hidden => "hidden",
            Self::Other(s) => s.as_str(),
        }
    }

    /// Текстовая семантика — поле с буквенным контентом, на котором
    /// можно делать text selection, IME, и т.д. Включает text/password/
    /// email/tel/url/number/search.
    pub fn is_textual(&self) -> bool {
        matches!(
            self,
            Self::Text | Self::Password | Self::Email | Self::Tel
                | Self::Url | Self::Number | Self::Search
        )
    }

    /// Кнопочная семантика — submit/reset/button/image, рендерится
    /// как button.
    pub fn is_button_like(&self) -> bool {
        matches!(
            self,
            Self::Submit | Self::Reset | Self::Button | Self::Image
        )
    }
}

/// HTML Living Standard `inputmode` attribute values — hint to user agent about
/// virtual keyboard shown for input/textarea elements. Default is `Text` (standard keyboard).
///
/// Used by shell for IME/virtual keyboard selection; phase 3 extension for Phase 1
/// composition infrastructure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// `none` — no virtual keyboard should be shown.
    None,
    /// `text` (default) — standard text input keyboard.
    Text,
    /// `decimal` — numeric keypad with decimal point (e.g. "12.34").
    Decimal,
    /// `numeric` — numeric keypad without decimal.
    Numeric,
    /// `tel` — telephone keypad.
    Tel,
    /// `search` — optimized for search queries.
    Search,
    /// `email` — optimized for email input.
    Email,
    /// `url` — optimized for URL input.
    Url,
}

impl InputMode {
    /// Parse `inputmode` attribute value. Case-insensitive per HTML spec.
    /// Unknown values default to `Text`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "decimal" => Self::Decimal,
            "numeric" => Self::Numeric,
            "tel" => Self::Tel,
            "search" => Self::Search,
            "email" => Self::Email,
            "url" => Self::Url,
            _ => Self::Text,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::None => "none",
            Self::Text => "text",
            Self::Decimal => "decimal",
            Self::Numeric => "numeric",
            Self::Tel => "tel",
            Self::Search => "search",
            Self::Email => "email",
            Self::Url => "url",
        }
    }
}

/// Данные `<form>` элемента — URL назначения, метод и число полей ввода.
#[derive(Debug, Clone)]
pub struct FormInfo {
    /// Значение атрибута `action` (пустая строка если отсутствует).
    pub action: String,
    /// Значение атрибута `method` в нижнем регистре, по умолчанию `"get"`.
    pub method: String,
    /// Число дочерних элементов-потомков типа input/select/textarea/button.
    pub field_count: usize,
}

/// Результат попытки отправить форму (HTML5 §4.10.22 form submission algorithm).
///
/// При вызове `submit_form(doc, form_id)` функция выполняет constraint validation
/// для всех submittable контролов в форме:
/// - Если форма невалидна → возвращает `Invalid` с NodeId-ами всех невалидных элементов
/// - Если форма валидна → возвращает `Valid` с собранными form data и параметрами отправки
#[derive(Debug, Clone)]
pub enum FormSubmitEvent {
    /// Форма прошла constraint validation — готова к отправке.
    Valid {
        /// Значение атрибута `action` (пустая строка если отсутствует).
        action: String,
        /// Значение атрибута `method` в нижнем регистре (default: "get").
        method: String,
        /// Собранные поля формы: `(name, value)` пары всех submittable контролов.
        fields: Vec<(String, String)>,
    },
    /// Форма не прошла constraint validation — содержит невалидные контролы.
    Invalid {
        /// NodeId-ы всех невалидных submittable контролов (в DOM-порядке).
        invalid_controls: Vec<NodeId>,
    },
}

fn count_form_controls(doc: &Document, id: NodeId) -> usize {
    let mut count = 0;
    for &child in &doc.get(id).children.clone() {
        if doc
            .get(child)
            .element_name()
            .map(|n| {
                matches!(
                    n.local.to_ascii_lowercase().as_str(),
                    "input" | "select" | "textarea" | "button"
                )
            })
            .unwrap_or(false)
        {
            count += 1;
        }
        count += count_form_controls(doc, child);
    }
    count
}

pub(crate) fn collect_forms(doc: &Document, id: NodeId, out: &mut Vec<FormInfo>) {
    let node = doc.get(id);
    if node
        .element_name()
        .map(|n| n.local.eq_ignore_ascii_case("form"))
        .unwrap_or(false)
    {
        let action = node.get_attr("action").unwrap_or("").to_string();
        let method = node
            .get_attr("method")
            .unwrap_or("get")
            .to_ascii_lowercase();
        let field_count = count_form_controls(doc, id);
        out.push(FormInfo {
            action,
            method,
            field_count,
        });
        return;
    }
    for &child in &node.children.clone() {
        collect_forms(doc, child, out);
    }
}

/// Гейт отправки форм по sandbox-флагу HTML §7.6.5.
///
/// Если `sandbox` содержит [`SandboxFlags::FORMS`] — отправка заблокирована;
/// функция логирует число заблокированных форм и возвращает его.
/// Если флаг не установлен — возвращает 0. В Phase 0 реальной отправки
/// нет; вызов устанавливает инфраструктуру для будущего FormRuntime.
pub fn check_form_gate(doc: &Document, sandbox: SandboxFlags) -> usize {
    let mut forms = Vec::new();
    collect_forms(doc, doc.root(), &mut forms);
    if forms.is_empty() {
        return 0;
    }
    if sandbox.contains(SandboxFlags::FORMS) {
        eprintln!(
            "sandbox: заблокировано {} форм(ы) (sandbox=forms)",
            forms.len()
        );
        return forms.len();
    }
    0
}

/// Найти ближайший предок `<form>` для узла `node`.
///
/// Реализует шаг «find the form owner» из HTML LS §form-associated elements:
/// поднимаемся вверх по цепочке родителей до первого элемента с тегом `form`.
/// Возвращает `None` если узел не вложен ни в какую форму.
pub fn find_ancestor_form(doc: &Document, mut node: NodeId) -> Option<NodeId> {
    while let Some(parent) = doc.get(node).parent {
        if doc.get(parent).element_name()
            .map(|q| q.local.eq_ignore_ascii_case("form"))
            .unwrap_or(false)
        {
            return Some(parent);
        }
        node = parent;
    }
    None
}

/// Собрать имена и значения submittable-контролов формы из DOM-атрибутов.
///
/// Обходит потомков `form_id` depth-first и возвращает `(name, value)` для
/// каждого `<input>`, `<textarea>`, `<select>` у которых есть атрибут `name`
/// и который не является disabled. `<input type="submit">` и `<input type="reset">`
/// не включаются в набор данных (они не submittable в смысле HTML LS).
///
/// Значения берутся из [`Document::control_value`] — то есть runtime-значение
/// контрола (что набрал пользователь или присвоил скрипт), а `value`-атрибут
/// служит лишь дефолтом (BUG-441).
pub fn collect_dom_form_fields(doc: &Document, form_id: NodeId) -> Vec<(String, String)> {
    let mut out = Vec::new();
    collect_fields_in(doc, form_id, form_id, &mut out);
    out
}

fn collect_fields_in(doc: &Document, id: NodeId, form_id: NodeId, out: &mut Vec<(String, String)>) {
    let node = doc.get(id);
    let tag = node.element_name().map(|q| q.local.as_str()).unwrap_or("");
    match tag {
        "input" => {
            let itype = node
                .get_attr("type")
                .unwrap_or("text")
                .to_ascii_lowercase();
            // submit/reset/button/image не включаются в набор данных.
            if matches!(itype.as_str(), "submit" | "reset" | "button" | "image") {
                return;
            }
            if node.get_attr("disabled").is_some() {
                return;
            }
            if let Some(name) = node.get_attr("name").filter(|n| !n.is_empty()) {
                // checkbox и radio включаются только если checked (текущее
                // состояние, не только атрибут-дефолт — BUG-444).
                if matches!(itype.as_str(), "checkbox" | "radio") {
                    if !doc.control_checked(id) {
                        return;
                    }
                    let value = node.get_attr("value").unwrap_or("on").to_string();
                    out.push((name.to_string(), value));
                } else {
                    let name = name.to_string();
                    out.push((name, doc.control_value(id).into_owned()));
                }
            }
        }
        "textarea" => {
            if node.get_attr("disabled").is_some() {
                return;
            }
            if let Some(name) = node.get_attr("name").filter(|n| !n.is_empty()) {
                // HTML LS §4.10.11: a textarea has no `value` attribute — its
                // default value is the child text, its current value the dirty
                // one. Reading `value=` here always yielded `""` (BUG-441).
                let name = name.to_string();
                out.push((name, doc.control_value(id).into_owned()));
            }
        }
        "select" => {
            if node.get_attr("disabled").is_some() {
                return;
            }
            if let Some(name) = node.get_attr("name").filter(|n| !n.is_empty()) {
                // Ищем первый выбранный <option>; если нет — первый <option>.
                let selected = find_selected_option(doc, id);
                out.push((name.to_string(), selected));
            }
        }
        // Не рекурсируем внутрь вложенных форм (HTML LS не поддерживает
        // nested forms, но такие страницы встречаются).
        "form" if id != form_id => return,
        _ => {}
    }
    for &child in &node.children.clone() {
        collect_fields_in(doc, child, form_id, out);
    }
}

fn find_selected_option(doc: &Document, select_id: NodeId) -> String {
    let node = doc.get(select_id);
    let mut first_value = String::new();
    for &child in &node.children.clone() {
        let ch = doc.get(child);
        if ch.element_name().map(|q| q.local.eq_ignore_ascii_case("option")).unwrap_or(false) {
            let val = ch.get_attr("value")
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    ch.children.first().and_then(|&t| {
                        if let NodeData::Text(data) = &doc.get(t).data {
                            Some(data.clone())
                        } else {
                            None
                        }
                    }).unwrap_or_default()
                });
            if first_value.is_empty() {
                first_value = val.clone();
            }
            if ch.get_attr("selected").is_some() {
                return val;
            }
        }
    }
    first_value
}

// ──────────────────────────────────────────────────────────────────────────────
// HTML5 Constraint Validation API (§4.10.21)
// ──────────────────────────────────────────────────────────────────────────────

/// Validity state for a form control — HTML5 §4.10.21.1 `ValidityState` interface.
///
/// Phase 0: `pattern_mismatch`, `step_mismatch`, `bad_input`, `custom_error`
/// are always `false` (require runtime state or regex engine).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidityState {
    /// Required field with no value / unchecked checkbox.
    pub value_missing: bool,
    /// `type=email` or `type=url` with syntactically wrong value.
    pub type_mismatch: bool,
    /// `pattern` attribute not matched — Phase 0: always false.
    pub pattern_mismatch: bool,
    /// Value length exceeds `maxlength`.
    pub too_long: bool,
    /// Value length is less than `minlength`.
    pub too_short: bool,
    /// Numeric value is less than `min`.
    pub range_underflow: bool,
    /// Numeric value is greater than `max`.
    pub range_overflow: bool,
    /// Value doesn't match `step` — Phase 0: always false.
    pub step_mismatch: bool,
    /// User agent can't convert the input — Phase 0: always false.
    pub bad_input: bool,
    /// `setCustomValidity("")` was called with non-empty string — Phase 0: always false.
    pub custom_error: bool,
}

impl ValidityState {
    /// Returns `true` when all flags are `false` (element satisfies all constraints).
    pub fn valid(&self) -> bool {
        !self.value_missing
            && !self.type_mismatch
            && !self.pattern_mismatch
            && !self.too_long
            && !self.too_short
            && !self.range_underflow
            && !self.range_overflow
            && !self.step_mismatch
            && !self.bad_input
            && !self.custom_error
    }
}

/// Returns the validity state for `node`, or `None` if the node is not a
/// form-associated element subject to constraint validation (HTML5 §4.10.21.2).
///
/// "Barred" conditions (return `None`):
///   - Not an `<input>`, `<select>`, or `<textarea>`.
///   - `<input type="hidden|button|submit|reset|image">`.
///   - Any element with the `disabled` attribute.
pub fn element_validity(doc: &Document, node: NodeId) -> Option<ValidityState> {
    let node_ref = doc.get(node);
    let tag = node_ref.element_name()?.local.as_str().to_ascii_lowercase();
    let tag = tag.as_str();

    let t_lower;
    let (is_input, itype) = match tag {
        "input" => {
            t_lower = node_ref
                .get_attr("type")
                .map(|t| t.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "text".to_string());
            let t = t_lower.as_str();
            // Barred input types per HTML5 §4.10.21.2.
            if matches!(t, "hidden" | "button" | "submit" | "reset" | "image") {
                return None;
            }
            (true, t)
        }
        "select" | "textarea" => (false, tag),
        _ => return None,
    };

    if node_ref.get_attr("disabled").is_some() {
        return None;
    }

    let mut vs = ValidityState::default();

    // --- valueMissing (HTML5 §4.10.21.4.1) ---
    if node_ref.get_attr("required").is_some() {
        let missing = if is_input {
            match itype {
                // Current checkedness, not just the `checked` attribute
                // default — a control the user unticked must validate on
                // what it actually holds (BUG-444, mirrors BUG-441).
                "checkbox" | "radio" => !doc.control_checked(node),
                _ => doc.control_value(node).trim().is_empty(),
            }
        } else if tag == "textarea" {
            doc.control_value(node).trim().is_empty()
        } else {
            // select: simplified — checks for non-empty selected value.
            node_ref.get_attr("value").unwrap_or("").trim().is_empty()
        };
        vs.value_missing = missing;
    }

    if is_input {
        // Current value, not the `value=` default — a field the user filled in
        // must validate on what it actually holds (BUG-441).
        let value = doc.control_value(node);
        let value = value.as_ref();

        // --- typeMismatch (HTML5 §4.10.21.4.2) ---
        if !value.is_empty() {
            if itype == "email" {
                vs.type_mismatch = !is_valid_email_dom(value);
            } else if itype == "url" {
                vs.type_mismatch = !is_valid_url_dom(value);
            }
        }

        // --- rangeUnderflow / rangeOverflow (HTML5 §4.10.21.4.6-7) ---
        let supports_range = matches!(itype, "number" | "range" | "date" | "time");
        if supports_range {
            if let Some(val_num) = parse_html_float(value) {
                let min_num = node_ref.get_attr("min").and_then(parse_html_float);
                let max_num = node_ref.get_attr("max").and_then(parse_html_float);
                if let Some(min) = min_num {
                    vs.range_underflow = val_num < min;
                }
                if let Some(max) = max_num {
                    vs.range_overflow = val_num > max;
                }
            } else if itype == "range" {
                // range with no/invalid value uses default mid-point — never under/overflow.
            }
        }

        // --- tooLong (HTML5 §4.10.21.4.8) ---
        if let Some(max_len) = node_ref.get_attr("maxlength").and_then(|v| v.trim().parse::<usize>().ok()) {
            vs.too_long = value.chars().count() > max_len;
        }

        // --- tooShort (HTML5 §4.10.21.4.9): only when field has a value ---
        if let Some(min_len) = node_ref.get_attr("minlength").and_then(|v| v.trim().parse::<usize>().ok()) {
            vs.too_short = !value.is_empty() && value.chars().count() < min_len;
        }
    } else if tag == "textarea" {
        let value = doc.control_value(node);
        if let Some(max_len) = node_ref.get_attr("maxlength").and_then(|v| v.trim().parse::<usize>().ok()) {
            vs.too_long = value.chars().count() > max_len;
        }
        if let Some(min_len) = node_ref.get_attr("minlength").and_then(|v| v.trim().parse::<usize>().ok()) {
            vs.too_short = !value.is_empty() && value.chars().count() < min_len;
        }
    }

    Some(vs)
}

/// Returns `true` if all submittable controls in `form_id` satisfy their
/// constraints (HTML5 §4.10.22.3 «statically validate the constraints»).
///
/// Returns `false` as soon as one invalid control is found.
/// All controls are barred controls — `check_validity_form` returns `true`
/// (vacuously valid — HTML5: «an element satisfies its constraints» when barred).
pub fn check_validity_form(doc: &Document, form_id: NodeId) -> bool {
    let mut all_valid = true;
    collect_validity_in(doc, form_id, form_id, &mut all_valid);
    all_valid
}

/// Returns the `NodeId`s of all invalid (failing constraint validation) controls
/// inside `form_id`, in DOM order.
pub fn invalid_controls_in_form(doc: &Document, form_id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect_invalid_in(doc, form_id, form_id, &mut out);
    out
}

/// Execute HTML5 form submission algorithm (§4.10.22 «Form submission»).
///
/// Performs constraint validation on all submittable controls within `form_id`:
/// - If any control fails validation → returns `FormSubmitEvent::Invalid` with list of failing controls
/// - If all pass → returns `FormSubmitEvent::Valid` with collected form data (name-value pairs)
///
/// The `action` and `method` attributes are extracted from the form element (both default to
/// standard HTML5 values: empty action and "get" method respectively).
///
/// Form data collection uses DOM attributes (not runtime state); P3 integration should apply
/// runtime FormState to get user-entered values.
pub fn submit_form(doc: &Document, form_id: NodeId) -> FormSubmitEvent {
    // Check if form_id actually points to a form element
    let form_node = doc.get(form_id);
    let is_form = form_node
        .element_name()
        .map(|q| q.local.eq_ignore_ascii_case("form"))
        .unwrap_or(false);

    if !is_form {
        // Not a form element — treat as vacuously valid (no controls to validate)
        return FormSubmitEvent::Valid {
            action: String::new(),
            method: "get".to_string(),
            fields: Vec::new(),
        };
    }

    // Extract action and method from form attributes
    let action = form_node.get_attr("action").unwrap_or("").to_string();
    let method = form_node
        .get_attr("method")
        .unwrap_or("get")
        .to_ascii_lowercase();

    // Perform constraint validation
    if !check_validity_form(doc, form_id) {
        // Form contains invalid controls — collect them and return Invalid
        let invalid_controls = invalid_controls_in_form(doc, form_id);
        return FormSubmitEvent::Invalid { invalid_controls };
    }

    // Form is valid — collect fields
    let fields = collect_dom_form_fields(doc, form_id);

    FormSubmitEvent::Valid {
        action,
        method,
        fields,
    }
}

fn collect_validity_in(doc: &Document, id: NodeId, form_id: NodeId, all_valid: &mut bool) {
    if !*all_valid {
        return; // early exit on first failure
    }
    let tag = doc.get(id).element_name().map(|q| q.local.as_str().to_ascii_lowercase()).unwrap_or_default();
    if matches!(tag.as_str(), "input" | "select" | "textarea")
        && element_validity(doc, id).is_some_and(|vs| !vs.valid())
    {
        *all_valid = false;
        return;
    }
    if tag == "form" && id != form_id {
        return; // don't cross into nested forms
    }
    for &child in &doc.get(id).children.clone() {
        collect_validity_in(doc, child, form_id, all_valid);
        if !*all_valid {
            return;
        }
    }
}

fn collect_invalid_in(doc: &Document, id: NodeId, form_id: NodeId, out: &mut Vec<NodeId>) {
    let tag = doc.get(id).element_name().map(|q| q.local.as_str().to_ascii_lowercase()).unwrap_or_default();
    if matches!(tag.as_str(), "input" | "select" | "textarea")
        && element_validity(doc, id).is_some_and(|vs| !vs.valid())
    {
        out.push(id);
    }
    if tag == "form" && id != form_id {
        return;
    }
    for &child in &doc.get(id).children.clone() {
        collect_invalid_in(doc, child, form_id, out);
    }
}
