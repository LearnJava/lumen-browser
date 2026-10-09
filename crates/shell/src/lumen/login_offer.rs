//! «Сохранить пароль?»: предложение после отправки формы входа (UX-PASSWORDS, срез 2).

use crate::*;

impl Lumen {
    /// Вызывается из `run_form_submission` после прохождения валидации и
    /// события `submit`: если в форме есть поле пароля и сайт не в списке
    /// «никогда», показывает `#loginBar`. Ничего не пишет на диск — сохранение
    /// только по кнопке.
    pub(crate) fn offer_to_save_login(&mut self, form: NodeId) {
        if self.active_profile_is_anonymous() {
            return;
        }
        let Some(store) = password_store::global() else { return };
        let Some(origin) = self.source.url_str().and_then(password_store::origin_of) else {
            return;
        };
        let creds = self.layout_source.as_ref().and_then(|src| {
            let doc = src.document.lock().ok()?;
            login_form::credentials(&doc, form)
        });
        let Some(creds) = creds else { return };
        let offer = password_store::plan_offer(store, &origin, &creds);
        if offer.is_some() || self.login_offer.is_some() {
            self.login_offer = offer;
            self.login_gen = None;
            self.relayout_chrome_host();
        }
    }

    /// UX-AUTOFILL срез 2: после отправки формы без пароля предлагает запомнить
    /// введённые имя, почту, телефон и адрес. Платёжные поля не затрагиваются.
    /// На диск ничего не пишет — только по кнопке «Сохранить».
    pub(crate) fn offer_to_save_autofill(&mut self, form: NodeId) {
        if self.active_profile_is_anonymous() {
            return;
        }
        let Some(store) = autofill_store::global() else { return };
        let Some(origin) = self.source.url_str().and_then(password_store::origin_of) else {
            return;
        };
        let values = self.layout_source.as_ref().and_then(|src| {
            let doc = src.document.lock().ok()?;
            Some(autofill_store::collect_values(&doc, form))
        });
        let offer = values.and_then(|v| autofill_store::plan_offer(store, &origin, v));
        if offer.is_some() || self.autofill_offer.is_some() {
            self.autofill_offer = offer;
            self.relayout_chrome_host();
        }
    }

    /// Подстановка сохранённого аккаунта в форму входа свежезагруженной страницы
    /// (срез 3). Вызывается из `apply_loaded_page`; в анонимном профиле и без
    /// хранилища ничего не делает. Отправку формы не запускает.
    pub(crate) fn autofill_saved_login(&mut self) {
        self.login_fill = None;
        self.login_gen = None;
        if self.active_profile_is_anonymous() {
            return;
        }
        let Some(store) = password_store::global() else { return };
        let Some(origin) = self.source.url_str().and_then(password_store::origin_of) else {
            return;
        };
        let Some(src) = self.layout_source.as_ref() else { return };
        let Ok(mut doc) = src.document.lock() else { return };
        let Some(target) = login_form::fill_target(&doc) else {
            let fields = login_form::new_password_fields(&doc);
            drop(doc);
            if !fields.is_empty() {
                self.login_gen = Some(fields);
                self.relayout_chrome_host();
            }
            return;
        };
        let prefilled = target.username.map(|u| doc.control_value(u).trim().to_owned()).unwrap_or_default();
        let Some(fill) = password_store::plan_fill(store, &origin, target, &prefilled) else { return };
        if !write_fill(store, &mut doc, &fill) {
            return;
        }
        drop(doc);
        if fill.usernames.len() > 1 {
            self.login_fill = Some(fill);
            self.relayout_chrome_host();
        }
    }

    /// «Использовать»: записать сгенерированный пароль в поля нового пароля и
    /// подтверждения. Пароль живёт только в полях формы; сохранение — штатным
    /// предложением после отправки.
    fn use_generated_password(&mut self) {
        let Some(fields) = self.login_gen.take() else { return };
        if let Some(pw) = password_store::generate_password_os()
            && let Some(src) = self.layout_source.as_ref()
            && let Ok(mut doc) = src.document.lock()
        {
            for f in fields {
                forms::set_value(&mut doc, f, &pw);
            }
        }
        self.relayout_chrome_host();
    }

    /// «Другой аккаунт»: подставить следующий сохранённый логин.
    fn cycle_login_fill(&mut self) {
        let (Some(fill), Some(store)) = (self.login_fill.as_mut(), password_store::global()) else {
            return;
        };
        fill.advance();
        if let Some(src) = self.layout_source.as_ref()
            && let Ok(mut doc) = src.document.lock()
        {
            write_fill(store, &mut doc, fill);
        }
        self.relayout_chrome_host();
    }

    /// Данные для `#loginBar`; пароль в модель не попадает.
    pub(crate) fn login_offer_model(&self) -> lumen_chrome::ChromeLoginOfferModel {
        if self.login_offer.is_none()
            && let Some(o) = self.autofill_offer.as_ref()
        {
            let host = o.origin.split_once("://").map_or(o.origin.as_str(), |(_, h)| h);
            return lumen_chrome::ChromeLoginOfferModel {
                open: true,
                title: "Запомнить данные формы?".to_owned(),
                meta: format!("{host} · полей: {}", o.entries.len()),
                save_label: "Сохранить".to_owned(),
                never_label: "Не сейчас".to_owned(),
                ..Default::default()
            };
        }
        if self.login_offer.is_none() && self.login_gen.is_some() && self.login_fill.is_none() {
            let host = self.source.url_str().and_then(password_store::origin_of).unwrap_or_default();
            let host = host.split_once("://").map_or(host.as_str(), |(_, h)| h).to_owned();
            return lumen_chrome::ChromeLoginOfferModel {
                open: true,
                title: "Создать надёжный пароль?".to_owned(),
                meta: host,
                generate: true,
                ..Default::default()
            };
        }
        if self.login_offer.is_none()
            && let Some(f) = self.login_fill.as_ref()
        {
            let host = f.origin.split_once("://").map_or(f.origin.as_str(), |(_, h)| h);
            return lumen_chrome::ChromeLoginOfferModel {
                open: true,
                title: "Подставлен сохранённый пароль".to_owned(),
                meta: format!("{host} · {} ({} из {})", f.current(), f.index + 1, f.usernames.len()),
                fill: true,
                ..Default::default()
            };
        }
        let Some(o) = self.login_offer.as_ref() else { return Default::default() };
        let host = o.origin.split_once("://").map_or(o.origin.as_str(), |(_, h)| h);
        let who = if o.username.is_empty() { host.to_owned() } else { format!("{host} · {}", o.username) };
        lumen_chrome::ChromeLoginOfferModel {
            open: true,
            title: if o.update { "Обновить пароль?" } else { "Сохранить пароль?" }.to_owned(),
            meta: who,
            save_label: if o.update { "Обновить" } else { "Сохранить" }.to_owned(),
            fill: false,
            generate: false,
            never_label: String::new(),
        }
    }

    pub(crate) fn dispatch_login_action(&mut self, action: lumen_chrome::ChromeAction) {
        use lumen_chrome::ChromeAction;
        if self.login_offer.is_none()
            && let Some(offer) = self.autofill_offer.take()
        {
            if action == ChromeAction::SaveLogin
                && let Some(store) = autofill_store::global()
            {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                for (key, value) in &offer.entries {
                    if let Err(e) = store.record(&offer.origin, key, value, now) {
                        eprintln!("autofill: {e}");
                    }
                }
            }
            self.relayout_chrome_host();
            return;
        }
        if self.login_offer.is_none() {
            match action {
                ChromeAction::NextLogin => self.cycle_login_fill(),
                ChromeAction::UseGeneratedPassword => self.use_generated_password(),
                ChromeAction::DismissLogin => {
                    self.login_fill = None;
                    self.login_gen = None;
                    self.relayout_chrome_host();
                }
                _ => {}
            }
            return;
        }
        let Some(offer) = self.login_offer.take() else { return };
        if let Some(store) = password_store::global() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let result = match action {
                ChromeAction::SaveLogin => store
                    .save(&offer.origin, &offer.username, &offer.password, now)
                    .map(|_| ()),
                ChromeAction::NeverSaveLogin => store.never_save(&offer.origin),
                _ => Ok(()),
            };
            if let Err(e) = result {
                eprintln!("passwords: {e}");
            }
        }
        self.relayout_chrome_host();
    }
}

/// Записать выбранный аккаунт в поля формы; `false`, если пароль не расшифровался.
fn write_fill(store: &lumen_storage::SavedLogins, doc: &mut lumen_dom::Document, fill: &password_store::LoginFill) -> bool {
    let Ok(Some(saved)) = store.get(&fill.origin, fill.current()) else { return false };
    if let Some(u) = fill.target.username {
        forms::set_value(doc, u, &saved.username);
    }
    forms::set_value(doc, fill.target.password, &saved.password);
    true
}
