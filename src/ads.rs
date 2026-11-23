//! Ненав'язлива монетизація: одна спонсорська плитка і посилання на підтримку автора.
//! Без трекінгу й мережевих запитів: показується лише статичний текст, посилання відкривається за кліком.
//! Замініть заглушки нижче; `SPONSOR = None` прибирає плитку зовсім.

pub struct Sponsor {
    pub title: &'static str,
    pub text: &'static str,
    pub cta: &'static str,
    pub url: &'static str,
}

pub const SPONSOR: Option<Sponsor> = Some(Sponsor {
    title: "Місце для вашого партнера",
    text: "Наприклад, хостинг серверів Minecraft",
    cta: "Докладніше",
    url: "https://example.com/?ref=modwire", // ЗАГЛУШКА: підставте партнерське посилання
});

/// Куди веде «Підтримати автора» (Ko-fi, Patreon, банка тощо). `None` ховає блок.
pub const DONATE_URL: Option<&str> = Some("https://example.com/donate"); // ЗАГЛУШКА

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_https() {
        if let Some(s) = SPONSOR {
            assert!(s.url.starts_with("https://"));
        }
        if let Some(u) = DONATE_URL {
            assert!(u.starts_with("https://"));
        }
    }
}
