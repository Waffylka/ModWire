//! Локалізація: `tr(ключ)` повертає рядок поточної мови. Поки в таблиці лише найчастіші рядки інтерфейсу;
//! решта переноситься в колах 11-13 (див. журнал). Мова за замовчуванням — українська, перемикача в UI ще немає.
//! Новий рядок: додайте рядок у `TABLE` (ключ, uk, en); ключ — `група.назва`. Тест перевіряє унікальність і повноту.
use std::sync::atomic::{AtomicU8, Ordering::Relaxed};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)] // En вмикається перемикачем мови (кола 11-13)
pub enum Lang {
    Uk,
    En,
}

static LANG: AtomicU8 = AtomicU8::new(0);

#[allow(dead_code)] // використає перемикач мови
pub fn set_lang(l: Lang) {
    LANG.store(l as u8, Relaxed);
}

pub fn lang() -> Lang {
    if LANG.load(Relaxed) == Lang::En as u8 { Lang::En } else { Lang::Uk }
}

/// (ключ, українською, англійською)
const TABLE: &[(&str, &str, &str)] = &[
    ("btn.cancel", "Скасувати", "Cancel"),
    ("btn.close", "Закрити", "Close"),
    ("btn.done", "Готово", "Done"),
    ("btn.add", "Додати", "Add"),
    ("btn.add_project", "Додати проєкт", "Add project"),
    ("btn.publish", "Опублікувати", "Publish"),
    ("btn.check", "Перевірити", "Check"),
    ("btn.open", "Відкрити", "Open"),
    ("btn.copy_link", "Копіювати посилання", "Copy link"),
    ("btn.hide", "Сховати", "Hide"),
    ("btn.details", "Докладніше", "Details"),
    ("label.error", "Помилка", "Error"),
    ("nav.projects", "Проєкти", "Projects"),
    ("nav.publish", "Публікація", "Publish"),
    ("nav.settings", "Налаштування", "Settings"),
    ("status.checking", "Перевіряємо…", "Checking…"),
    ("notice.link_copied", "Посилання скопійовано.", "Link copied."),
];

/// Рядок за ключем. Невідомий ключ повертається як є (помітно в UI, не панікує).
pub fn tr(key: &'static str) -> &'static str {
    match TABLE.iter().find(|r| r.0 == key) {
        Some(r) if lang() == Lang::En => r.2,
        Some(r) => r.1,
        None => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_complete_and_unique() {
        for (i, r) in TABLE.iter().enumerate() {
            assert!(!r.1.is_empty() && !r.2.is_empty(), "порожній переклад: {}", r.0);
            assert!(TABLE[..i].iter().all(|o| o.0 != r.0), "дубль ключа: {}", r.0);
        }
        assert_eq!(tr("btn.cancel"), "Скасувати");
        assert_eq!(tr("no.such.key"), "no.such.key");
    }
}
