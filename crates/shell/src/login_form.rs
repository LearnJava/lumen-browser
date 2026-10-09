//! Распознавание форм входа и полей пароля (UX-PASSWORDS, срез 1).
//!
//! Чистая функция над DOM: ничего не читает из хранилища и не показывает UI.
//! Следующие срезы (предложение сохранить, подстановка) опираются на
//! [`find_login_forms`], чтобы знать, какое поле — логин, а какое — пароль.

use lumen_dom::{Document, InputType, NodeId};

/// Что делает форма с паролем.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginKind {
    /// Одно поле пароля — вход.
    SignIn,
    /// Новый пароль (`autocomplete=new-password` или пара пароль + подтверждение) — регистрация.
    SignUp,
    /// Текущий пароль плюс новый (три поля или `current-password` + `new-password`).
    ChangePassword,
}

/// Одна форма (или набор полей вне `<form>`) с полем пароля.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginForm {
    /// `<form>`-владелец; `None` для полей вне формы (SPA без `<form>`).
    pub form: Option<NodeId>,
    /// Поле логина: ближайшее перед первым паролем текстовое поле.
    pub username: Option<NodeId>,
    /// Поля пароля в порядке документа (текущий, новый, подтверждение).
    pub passwords: Vec<NodeId>,
    pub kind: LoginKind,
}

/// Найти все формы с полями пароля в документе, в порядке появления.
pub fn find_login_forms(doc: &Document) -> Vec<LoginForm> {
    let mut inputs = Vec::new();
    collect_inputs(doc, doc.root(), &mut inputs);

    // Группировка по владельцу, порядок первой встречи сохраняется.
    let mut groups: Vec<(Option<NodeId>, Vec<NodeId>)> = Vec::new();
    for id in inputs {
        let owner = lumen_dom::find_ancestor_form(doc, id);
        match groups.iter_mut().find(|(o, _)| *o == owner) {
            Some((_, v)) => v.push(id),
            None => groups.push((owner, vec![id])),
        }
    }

    groups
        .into_iter()
        .filter_map(|(form, members)| build(doc, form, &members))
        .collect()
}

/// Что пользователь ввёл в форму: логин и тот пароль, который сайт будет считать
/// актуальным (при смене пароля — новый, а не текущий).
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    pub kind: LoginKind,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// Введённые логин и пароль формы `form`; `None`, если в ней нет поля пароля
/// или пароль пуст.
pub fn credentials(doc: &Document, form: NodeId) -> Option<Credentials> {
    let lf = find_login_forms(doc).into_iter().find(|f| f.form == Some(form))?;
    let password_node = match lf.kind {
        LoginKind::SignIn | LoginKind::SignUp => *lf.passwords.first()?,
        LoginKind::ChangePassword => lf
            .passwords
            .iter()
            .copied()
            .find(|&id| has_autocomplete(doc, id, "new-password"))
            .or_else(|| lf.passwords.get(1).copied())?,
    };
    let password = doc.control_value(password_node).into_owned();
    if password.is_empty() {
        return None;
    }
    let username = lf
        .username
        .map(|id| doc.control_value(id).trim().to_owned())
        .unwrap_or_default();
    Some(Credentials { username, password, kind: lf.kind })
}

/// Поля формы входа, которые можно заполнить сохранённым аккаунтом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillTarget {
    pub username: Option<NodeId>,
    pub password: NodeId,
}

/// Первая форма входа (`SignIn`) с пустым полем пароля. Регистрацию и смену
/// пароля не трогаем: там сохранённый пароль не нужен.
pub fn fill_target(doc: &Document) -> Option<FillTarget> {
    find_login_forms(doc).into_iter().find_map(|f| {
        let password = *f.passwords.first()?;
        (f.kind == LoginKind::SignIn && doc.control_value(password).is_empty())
            .then_some(FillTarget { username: f.username, password })
    })
}

fn has_autocomplete(doc: &Document, id: NodeId, token: &str) -> bool {
    doc.get(id)
        .get_attr("autocomplete")
        .is_some_and(|v| v.split_whitespace().any(|t| t.eq_ignore_ascii_case(token)))
}

fn build(doc: &Document, form: Option<NodeId>, members: &[NodeId]) -> Option<LoginForm> {
    let passwords: Vec<NodeId> = members
        .iter()
        .copied()
        .filter(|&id| doc.get(id).input_type() == Some(InputType::Password))
        .collect();
    let first = *passwords.first()?;
    let username = pick_username(doc, members, first);
    let kind = classify(doc, &passwords);
    Some(LoginForm { form, username, passwords, kind })
}

fn classify(doc: &Document, passwords: &[NodeId]) -> LoginKind {
    let has = |token: &str| {
        passwords.iter().any(|&id| {
            doc.get(id)
                .get_attr("autocomplete")
                .is_some_and(|v| v.split_whitespace().any(|t| t.eq_ignore_ascii_case(token)))
        })
    };
    match passwords.len() {
        0 | 1 => {
            if has("new-password") {
                LoginKind::SignUp
            } else {
                LoginKind::SignIn
            }
        }
        2 if has("current-password") && has("new-password") => LoginKind::ChangePassword,
        2 => LoginKind::SignUp,
        _ => LoginKind::ChangePassword,
    }
}

/// Логин: поле с `autocomplete=username` приоритетно, иначе ближайшее
/// предшествующее паролю текстовое/email/tel-поле.
fn pick_username(doc: &Document, members: &[NodeId], first_password: NodeId) -> Option<NodeId> {
    let is_text = |id: NodeId| {
        matches!(
            doc.get(id).input_type(),
            Some(InputType::Text | InputType::Email | InputType::Tel)
        )
    };
    if let Some(&id) = members.iter().find(|&&id| {
        is_text(id)
            && doc
                .get(id)
                .get_attr("autocomplete")
                .is_some_and(|v| v.split_whitespace().any(|t| t.eq_ignore_ascii_case("username")))
    }) {
        return Some(id);
    }
    members
        .iter()
        .copied()
        .take_while(|&id| id != first_password)
        .filter(|&id| is_text(id))
        .last()
}

/// Видимые, не отключённые `<input>` в порядке документа.
fn collect_inputs(doc: &Document, id: NodeId, out: &mut Vec<NodeId>) {
    let node = doc.get(id);
    if node.input_type().is_some()
        && node.get_attr("disabled").is_none()
        && node.get_attr("hidden").is_none()
    {
        out.push(id);
    }
    for &child in &node.children.clone() {
        collect_inputs(doc, child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(html: &str) -> (Document, Vec<LoginForm>) {
        let doc = lumen_html_parser::parse(html);
        let f = find_login_forms(&doc);
        (doc, f)
    }

    fn name_of(doc: &Document, id: Option<NodeId>) -> Option<String> {
        id.and_then(|i| doc.get(i).get_attr("name").map(str::to_owned))
    }

    #[test]
    fn simple_sign_in() {
        let (doc, f) = forms(
            r#"<form><input name="u"><input type="password" name="p"><input type="submit"></form>"#,
        );
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, LoginKind::SignIn);
        assert!(f[0].form.is_some());
        assert_eq!(name_of(&doc, f[0].username).as_deref(), Some("u"));
    }

    #[test]
    fn no_password_no_form() {
        let (_, f) = forms(r#"<form><input name="q"></form>"#);
        assert!(f.is_empty());
    }

    #[test]
    fn username_is_nearest_preceding_text() {
        let (doc, f) = forms(
            r#"<form><input name="search"><input type="email" name="mail">
               <input type="password" name="p"><input name="after"></form>"#,
        );
        assert_eq!(name_of(&doc, f[0].username).as_deref(), Some("mail"));
    }

    #[test]
    fn autocomplete_username_wins() {
        let (doc, f) = forms(
            r#"<form><input name="a" autocomplete="username"><input name="b">
               <input type="password"></form>"#,
        );
        assert_eq!(name_of(&doc, f[0].username).as_deref(), Some("a"));
    }

    #[test]
    fn sign_up_with_confirmation() {
        let (_, f) = forms(
            r#"<form><input name="u"><input type="password"><input type="password"></form>"#,
        );
        assert_eq!(f[0].kind, LoginKind::SignUp);
        assert_eq!(f[0].passwords.len(), 2);
    }

    #[test]
    fn new_password_autocomplete_is_sign_up() {
        let (_, f) = forms(r#"<form><input type="password" autocomplete="new-password"></form>"#);
        assert_eq!(f[0].kind, LoginKind::SignUp);
    }

    #[test]
    fn change_password() {
        let (_, f) = forms(
            r#"<form><input type="password" autocomplete="current-password">
               <input type="password" autocomplete="new-password"></form>"#,
        );
        assert_eq!(f[0].kind, LoginKind::ChangePassword);
        let (_, f) = forms(
            r#"<form><input type="password"><input type="password"><input type="password"></form>"#,
        );
        assert_eq!(f[0].kind, LoginKind::ChangePassword);
    }

    #[test]
    fn disabled_and_hidden_ignored() {
        let (_, f) = forms(
            r#"<form><input type="password" disabled><input type="password" hidden></form>"#,
        );
        assert!(f.is_empty());
    }

    #[test]
    fn formless_fields_group_together() {
        let (doc, f) = forms(r#"<div><input name="u"><input type="password"></div>"#);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].form, None);
        assert_eq!(name_of(&doc, f[0].username).as_deref(), Some("u"));
    }

    #[test]
    fn two_forms_are_separate() {
        let (_, f) = forms(
            r#"<form><input name="u"><input type="password"></form>
               <form><input name="e"><input type="password"><input type="password"></form>"#,
        );
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].kind, LoginKind::SignIn);
        assert_eq!(f[1].kind, LoginKind::SignUp);
    }

    #[test]
    fn credentials_read_typed_values() {
        let (mut doc, f) = forms(
            r#"<form><input name="u"><input type="password" name="p"></form>"#,
        );
        let (u, p) = (f[0].username.unwrap(), f[0].passwords[0]);
        doc.set_control_value(u, " anna ");
        doc.set_control_value(p, "s3cret");
        let c = credentials(&doc, f[0].form.unwrap()).unwrap();
        assert_eq!((c.username.as_str(), c.password.as_str()), ("anna", "s3cret"));
    }

    #[test]
    fn fill_target_skips_filled_and_sign_up_forms() {
        let (doc, f) = forms(r#"<form><input name="u"><input type="password"></form>"#);
        let t = fill_target(&doc).unwrap();
        assert_eq!((t.username, t.password), (f[0].username, f[0].passwords[0]));
        let (doc, _) = forms(r#"<form><input type="password" value="x"></form>"#);
        assert!(fill_target(&doc).is_none());
        let (doc, _) = forms(r#"<form><input type="password" autocomplete="new-password"></form>"#);
        assert!(fill_target(&doc).is_none());
    }

    #[test]
    fn credentials_empty_password_is_none() {
        let (doc, f) = forms(r#"<form><input name="u" value="a"><input type="password"></form>"#);
        assert!(credentials(&doc, f[0].form.unwrap()).is_none());
    }

    #[test]
    fn credentials_change_password_takes_the_new_one() {
        let (doc, f) = forms(
            r#"<form><input type="password" autocomplete="current-password" value="old">
               <input type="password" autocomplete="new-password" value="fresh"></form>"#,
        );
        let c = credentials(&doc, f[0].form.unwrap()).unwrap();
        assert_eq!(c.password, "fresh");
        assert_eq!(c.kind, LoginKind::ChangePassword);
    }

    #[test]
    fn credentials_debug_hides_password() {
        let (doc, f) = forms(r#"<form><input type="password" value="topsecret"></form>"#);
        let c = credentials(&doc, f[0].form.unwrap()).unwrap();
        assert!(!format!("{c:?}").contains("topsecret"));
    }
}
