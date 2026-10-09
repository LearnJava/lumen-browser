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
            self.relayout_chrome_host();
        }
    }

    /// Данные для `#loginBar`; пароль в модель не попадает.
    pub(crate) fn login_offer_model(&self) -> lumen_chrome::ChromeLoginOfferModel {
        let Some(o) = self.login_offer.as_ref() else { return Default::default() };
        let host = o.origin.split_once("://").map_or(o.origin.as_str(), |(_, h)| h);
        let who = if o.username.is_empty() { host.to_owned() } else { format!("{host} · {}", o.username) };
        lumen_chrome::ChromeLoginOfferModel {
            open: true,
            title: if o.update { "Обновить пароль?" } else { "Сохранить пароль?" }.to_owned(),
            meta: who,
            save_label: if o.update { "Обновить" } else { "Сохранить" }.to_owned(),
        }
    }

    pub(crate) fn dispatch_login_action(&mut self, action: lumen_chrome::ChromeAction) {
        use lumen_chrome::ChromeAction;
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
