//! Чиста логіка + мережеві виклики (блокуючі: викликати лише з окремих потоків).
use reqwest::blocking::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;

/// Лоадери модів, потім «лоадери» паків: на Modrinth версія ресурспаку має лоадер `minecraft`, датапаку `datapack`.
pub const LOADERS: [&str; 6] = ["fabric", "forge", "neoforge", "quilt", "minecraft", "datapack"];

/// Чи це «лоадер» ресурспаку/датапаку: CurseForge для таких версій модлоадера не має.
pub fn is_pack_loader(l: &str) -> bool {
    matches!(l, "minecraft" | "datapack")
}
pub const RELEASE_TYPES: [&str; 3] = ["release", "beta", "alpha"];

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Project {
    pub last_number: String,
    pub last_changelog: String,
    pub name: String,
    pub modrinth_id: String,
    pub curse_id: String,
    pub icon_url: String,
    /// Налаштування останньої публікації цього проєкту (різні моди мають різні MC/лоадери).
    pub mc_versions: String,
    pub loaders: Vec<String>,
    pub release_type: String,
    /// Шаблон змін із підстановками {version} {mc} {loaders} {date} {project}.
    pub changelog_template: String,
    /// Стабільний ідентифікатор запису (не індекс): присвоюється міграцією і при збереженні.
    pub id: String,
    pub pinned: bool,
    /// Шаблон назви версії ({project} {version} {mc} {loaders} {date}); порожній = назва за номером.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name_template: String,
    /// Незавершена форма цього проєкту, коли користувач перемкнувся на інший.
    #[serde(skip_serializing_if = "Draft::is_empty")]
    pub draft: Draft,
    /// Невідомі поля зберігаються при перезаписі (сумісність із новішими версіями).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Last {
    pub dir: String,
    pub project: usize,
    pub mc_versions: String,
    pub loaders: Vec<String>,
    pub release_type: String,
    /// Незавершена форма публікації (відновлюється після перезапуску).
    #[serde(skip_serializing_if = "Draft::is_empty")]
    pub draft: Draft,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Чернетка форми «Публікація»: прив'язана до id проєкту.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Draft {
    pub project: String,
    pub number: String,
    pub name: String,
    pub changelog: String,
    pub file: String,
}

impl Draft {
    pub fn is_empty(&self) -> bool {
        *self == Draft::default()
    }

    /// Чернетка діє лише для того самого проєкту і якщо номер ще не опубліковано (інакше це залишок минулого релізу).
    pub fn applies_to(&self, p: &Project) -> bool {
        !p.id.is_empty() && self.project == p.id && !(self.number.trim().is_empty() && self.changelog.trim().is_empty() && self.name.trim().is_empty() && self.file.is_empty())
            && !(self.number.trim() == p.last_number.trim() && !p.last_number.trim().is_empty())
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Config {
    /// Версія схеми конфігу (0 = файл до появи версіонування).
    pub schema: u32,
    pub modrinth_token: String,
    pub curse_token: String,
    pub curse_core_key: String,
    /// Час (Unix, с) останньої успішної перевірки токена/ключа; 0 = не перевірялось.
    pub modrinth_checked: u64,
    pub curse_checked: u64,
    pub core_checked: u64,
    /// Час (Unix, с) останньої успішної перевірки оновлень: фонова перевірка не частіше UPDATE_EVERY.
    #[serde(skip_serializing_if = "is_zero")]
    pub update_checked: u64,
    /// Версія оновлення, яку користувач пропустив (фонова перевірка її більше не показує).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub update_skip: String,
    /// Мова інтерфейсу: "en" або "ru"; порожньо = українська.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub lang: String,
    pub projects: Vec<Project>,
    pub last: Last,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

pub const SCHEMA: u32 = 2;

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Як часто фонова перевірка оновлень ходить у мережу.
pub const UPDATE_EVERY: u64 = 6 * 3600;

/// Чи пора перевіряти оновлення. Ручна перевірка (manual) завжди дозволена; годинник, що пішов назад, не блокує перевірку.
pub fn update_due(now: u64, last: u64, manual: bool) -> bool {
    manual || last > now || now - last >= UPDATE_EVERY
}

/// Чи показувати знайдене оновлення: фонова перевірка не нагадує про пропущену версію, ручна показує будь-яку.
pub fn update_visible(version: &str, skipped: &str, manual: bool) -> bool {
    manual || skipped.trim_start_matches('v') != version.trim_start_matches('v')
}

/// М'яке попередження перед публікацією: токен вибраної платформи ще жодного разу не перевіряли (0 = не перевірявся).
pub fn unchecked_tokens(mr: bool, cf: bool, mr_checked: u64, cf_checked: u64) -> Option<String> {
    let names: Vec<&str> = [(mr && mr_checked == 0, "Modrinth"), (cf && cf_checked == 0, "CurseForge")].into_iter().filter(|x| x.0).map(|x| x.1).collect();
    (!names.is_empty()).then(|| format!("Токен {} не перевірено: «Налаштування» → «Перевірити».", names.join(" і ")))
}

/// Прибирає секрети з тексту помилки (сервер може повернути у тілі те, що ми надіслали в заголовку).
pub fn mask_secrets(text: &str, secrets: &[&str]) -> String {
    secrets.iter().map(|s| s.trim()).filter(|s| s.len() >= 4).fold(text.to_string(), |t, s| t.replace(s, "•••"))
}

/// Застосовує `mask_secrets` до помилки результату.
pub fn masked<T>(r: Result<T, String>, secrets: &[&str]) -> Result<T, String> {
    r.map_err(|e| mask_secrets(&e, secrets))
}

/// Присвоює унікальні id проєктам без id або з дублем id.
pub fn ensure_ids(cfg: &mut Config) {
    let mut seen = std::collections::HashSet::new();
    let mut n = now_secs();
    for p in &mut cfg.projects {
        while p.id.is_empty() || !seen.insert(p.id.clone()) {
            n += 1;
            p.id = format!("p{n:x}");
        }
    }
}

/// Порядок для сортування: закріплені першими, далі за назвою (без регістру); perm[нова позиція] = стара.
pub fn sort_perm(ps: &[Project]) -> Vec<usize> {
    let mut v: Vec<usize> = (0..ps.len()).collect();
    v.sort_by_key(|&i| (!ps[i].pinned, ps[i].name.trim().to_lowercase()));
    v
}

/// Закріплені першими; порядок усередині груп лишається ручним (стабільне сортування).
pub fn pin_perm(ps: &[Project]) -> Vec<usize> {
    let mut v: Vec<usize> = (0..ps.len()).collect();
    v.sort_by_key(|&i| !ps[i].pinned);
    v
}

/// Індекси проєктів, що підходять під пошук (назва або ID платформ, без регістру); порожній запит = усі.
pub fn filter_projects(ps: &[Project], q: &str) -> Vec<usize> {
    let q = q.trim().to_lowercase();
    (0..ps.len()).filter(|&i| q.is_empty() || [&ps[i].name, &ps[i].modrinth_id, &ps[i].curse_id].iter().any(|f| f.to_lowercase().contains(&q))).collect()
}

/// Копія проєкту в кінець списку (з новим id, назвою «… (копія)», без закріплення). Повертає індекс копії.
pub fn duplicate_project(cfg: &mut Config, i: usize) -> Option<usize> {
    let mut p = cfg.projects.get(i)?.clone();
    let base = format!("{} (копія", p.name.trim());
    let mut name = format!("{base})");
    let mut n = 2;
    while cfg.projects.iter().any(|q| q.name == name) {
        name = format!("{base} {n})");
        n += 1;
    }
    (p.name, p.id, p.pinned, p.draft) = (name, String::new(), false, Draft::default());
    cfg.projects.push(p);
    ensure_ids(cfg);
    Some(cfg.projects.len() - 1)
}

/// Посилання на сторінку проєкту, вставлене користувачем.
#[derive(Debug, PartialEq)]
pub enum Link {
    Modrinth(String),
    CurseSlug(String),
    CurseId(String),
}

/// Розпізнає посилання modrinth.com/<тип>/<slug> та curseforge.com/minecraft/<розділ>/<slug> (або /projects/<id>).
/// Хост перевіряється точно (modrinth.com.evil.org не проходить); без схеми теж працює.
pub fn parse_project_link(s: &str) -> Option<Link> {
    let s = s.trim();
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    let (host, path) = s.split_once('/')?;
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let segs: Vec<&str> = path.split(['?', '#']).next().unwrap_or("").split('/').filter(|p| !p.is_empty()).collect();
    let slug = |x: &&str| (!x.is_empty() && x.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))).then(|| x.to_string());
    match (host, segs.as_slice()) {
        ("modrinth.com", ["mod" | "plugin" | "modpack" | "resourcepack" | "shader" | "datapack" | "project", x, ..]) => slug(x).map(Link::Modrinth),
        ("curseforge.com", ["minecraft", "mc-mods" | "modpacks" | "texture-packs" | "shaders" | "data-packs", x, ..]) => slug(x).map(Link::CurseSlug),
        ("curseforge.com", ["projects", x, ..]) if !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()) => Some(Link::CurseId(x.to_string())),
        _ => None,
    }
}

/// (id, назва, іконка) проєкту Modrinth за slug або id; токен не обов'язковий (публічні проєкти).
pub fn modrinth_lookup(token: &str, slug: &str) -> Result<(String, String, String), String> {
    if !slug_ok(slug) {
        return Err(format!("Некоректний slug «{}»", slug.chars().take(40).collect::<String>()));
    }
    let v = mr_get(&client_t(20)?, token, &format!("/project/{slug}")).map_err(|e| if e.contains("404") { format!("Проєкт «{slug}» не знайдено на Modrinth") } else { e })?;
    Ok((v["id"].as_str().ok_or("немає id проєкту")?.into(), v["title"].as_str().ok_or("немає назви проєкту")?.into(), v["icon_url"].as_str().unwrap_or("").into()))
}

/// (id, назва, іконка) проєкту CurseForge за slug через Core API (потрібен ключ).
pub fn curse_lookup_slug(key: &str, slug: &str) -> Result<(String, String, String), String> {
    if !slug_ok(slug) {
        return Err(format!("Некоректний slug «{}»", slug.chars().take(40).collect::<String>()));
    }
    let v = get_json(&client_t(20)?, &format!("{}/v1/mods/search?gameId=432&slug={slug}", core()), ("x-api-key", key))?;
    let d = v["data"].as_array().and_then(|a| a.first()).ok_or_else(|| format!("Проєкт «{slug}» не знайдено на CurseForge"))?;
    let (name, icon) = parse_curse_mod(&json!({ "data": d })).ok_or("немає назви проєкту")?;
    Ok((d["id"].as_u64().ok_or("немає id проєкту")?.to_string(), name, icon))
}

/// Результат пошуку за назвою на CurseForge.
#[derive(Clone, Debug, PartialEq)]
pub struct CfHit {
    pub id: String,
    pub name: String,
    pub icon_url: String,
}

/// Відповідь /v1/mods/search → список (id, назва, іконка); елементи без id чи назви пропускаються.
pub fn parse_curse_search(v: &Value) -> Vec<CfHit> {
    v["data"].as_array().map(|a| {
        a.iter().filter_map(|d| {
            let (name, icon_url) = parse_curse_mod(&json!({ "data": d }))?;
            Some(CfHit { id: d["id"].as_u64()?.to_string(), name, icon_url })
        }).collect()
    }).unwrap_or_default()
}

/// Пошук проєктів Minecraft на CurseForge за назвою через Core API (потрібен ключ).
pub fn curse_search(key: &str, query: &str) -> Result<Vec<CfHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let url = reqwest::Url::parse_with_params(&format!("{}/v1/mods/search", core()), [("gameId", "432"), ("searchFilter", q), ("pageSize", "10")]).map_err(|e| e.to_string())?;
    Ok(parse_curse_search(&get_json(&client_t(20)?, url.as_str(), ("x-api-key", key))?))
}

/// Для export: прибирає зайві поля, схожі на секрети.
fn strip_secrets(p: &Project) -> Project {
    fn secret(k: &str) -> bool {
        let k = k.to_lowercase();
        ["token", "key", "secret", "password", "auth"].iter().any(|w| k.contains(w))
    }
    // ключі, схожі на секрети, прибираємо на будь-якій глибині невідомих полів
    fn scrub(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.retain(|k, _| !secret(k));
                m.values_mut().for_each(scrub);
            }
            Value::Array(a) => a.iter_mut().for_each(scrub),
            _ => {}
        }
    }
    let mut q = p.clone();
    q.draft = Draft::default(); // чернетка змін не є налаштуванням і не йде в експорт
    q.extra.retain(|k, _| !secret(k));
    q.extra.values_mut().for_each(scrub);
    q
}

/// Тека даних програми. Змінна MODWIRE_CONFIG_DIR (напр. для перевірки на порожніх даних) підміняє
/// справжню теку конфігу: тоді токени й проєкти користувача не читаються і не змінюються.
fn data_dir() -> Option<PathBuf> {
    match std::env::var_os("MODWIRE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(d) => Some(PathBuf::from(d)),
        None => dirs::config_dir().map(|d| d.join("mod-publisher")),
    }
}

fn config_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("config.json"))
}

pub enum Load {
    Fine,
    Recovered, // файл був пошкоджений, копію збережено
    Restored,  // файл був пошкоджений, дані відновлено з резервної копії
    ReadOnly,  // файл є, але не читається/не збережено копію: не перезаписувати
}

fn backup_path(p: &Path) -> PathBuf {
    p.with_file_name("config.backup.json")
}

pub fn load_config() -> (Config, Load) {
    match config_path() {
        Some(p) => load_from(&p),
        None => (Config::default(), Load::Fine),
    }
}

/// Читає конфіг із p. Пошкоджений файл відкладається в мічену копію, дані беруться з config.backup.json,
/// якщо він читається. Старіша схема спершу копіюється в config.pre-vN.json, потім мігрується (поля не губляться).
pub fn load_from(p: &Path) -> (Config, Load) {
    let s = match std::fs::read_to_string(p) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Config::default(), Load::Fine),
        Err(_) => return (Config::default(), Load::ReadOnly),
    };
    match serde_json::from_str::<Config>(&s) {
        Ok(c) => (migrate(c, p, &s), Load::Fine),
        // мітка часу, щоб не затерти попередню копію
        Err(_) => match std::fs::rename(p, p.with_extension(format!("json.{}.bak", now_secs()))) {
            Ok(()) => match std::fs::read_to_string(backup_path(p)).ok().and_then(|b| serde_json::from_str::<Config>(&b).ok()) {
                Some(c) => (migrate(c, p, ""), Load::Restored),
                None => (Config::default(), Load::Recovered),
            },
            Err(_) => (Config::default(), Load::ReadOnly),
        },
    }
}

fn migrate(mut c: Config, p: &Path, raw: &str) -> Config {
    if c.schema < SCHEMA {
        if !raw.is_empty() {
            let keep = p.with_file_name(format!("config.pre-v{SCHEMA}.json"));
            if !keep.exists() {
                let _ = std::fs::write(keep, raw);
            }
        }
        // v0 -> v1: токени без зайвих пробілів/переносів (вставка з буфера), решта без змін
        for t in [&mut c.modrinth_token, &mut c.curse_token, &mut c.curse_core_key] {
            *t = t.trim().to_string();
        }
        // v1 -> v2: стабільні id проєктів
        ensure_ids(&mut c);
        c.schema = SCHEMA;
    }
    c
}

/// Збій запису не блокує роботу — повертаємо лише текст помилки.
/// Ексклюзивне блокування файлом: друга копія не перезапише конфіг. Ok(None) = уже запущено,
/// Err = блокування неможливе (тоді стартуємо без нього). Тримати живим до виходу.
pub fn single_instance_lock() -> Result<Option<std::fs::File>, String> {
    let dir = config_path().and_then(|p| p.parent().map(Path::to_path_buf)).ok_or("немає теки конфігу")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    lock_at(&dir.join(".lock"))
}

fn lock_at(path: &Path) -> Result<Option<std::fs::File>, String> {
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).map_err(|e| e.to_string())?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Збій запису не блокує роботу — повертаємо лише текст помилки.
fn history_path() -> Option<PathBuf> {
    config_path().map(|p| p.with_file_name("history.jsonl"))
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Контекст запису історії для відновлення форми («Повторити з цього запису»). Усі поля необов'язкові.
#[derive(Default, Clone, Debug)]
pub struct HistCtx {
    pub project_id: String,
    pub name: String,
    pub release_type: String,
    pub mc_versions: String,
    pub loaders: Vec<String>,
}

/// Дописує результат публікації в history.jsonl (помилки запису ігноруються).
pub fn history_add_ctx(project: &str, number: &str, platform: &str, res: &Result<String, String>, ctx: &HistCtx) {
    if let Some(p) = history_path() {
        history_add_with(&p, project, number, platform, res, ctx);
    }
}

/// Що відновлюється з запису історії.
#[derive(Debug, PartialEq)]
pub struct HistRestore {
    /// project = id проєкту (або порожній для старих записів), number/name — з запису; changelog і файл не зберігаються.
    pub draft: Draft,
    pub release_type: String,
    pub mc_versions: String,
    pub loaders: Vec<String>,
}

/// Дані форми з запису історії. None, якщо в записі немає номера. Старі записи (без нових полів) дають лише проєкт-назву в `project`
/// порожнім, а номер із запису: проєкт шукається за назвою на боці UI.
pub fn restore_from_history(e: &Value) -> Option<HistRestore> {
    let number = e["number"].as_str().filter(|n| !n.trim().is_empty())?.to_string();
    let g = |k: &str| e[k].as_str().unwrap_or("").to_string();
    Some(HistRestore {
        draft: Draft { project: g("pid"), number, name: g("vname"), ..Default::default() },
        release_type: g("rtype"),
        mc_versions: g("mc"),
        loaders: e["loaders"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
    })
}

fn is_repeat(recent: &[(PathBuf, String, u64)], p: &Path, key: &str, now: u64) -> bool {
    recent.iter().any(|(pp, k, t)| pp == p && k == key && now.saturating_sub(*t) < 5)
}

#[cfg(test)]
fn history_add_at(p: &Path, project: &str, number: &str, platform: &str, res: &Result<String, String>) {
    history_add_with(p, project, number, platform, res, &HistCtx::default());
}

fn history_add_with(p: &Path, project: &str, number: &str, platform: &str, res: &Result<String, String>, ctx: &HistCtx) {
    use std::io::Write;
    // запис і обрізання під одним замком: два потоки публікації (Modrinth і CurseForge) не гублять рядки
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let msg = match res { Ok(m) | Err(m) => m };
    // той самий запис упродовж 5 с (подвійна відправка події) не дублюємо
    static RECENT: std::sync::Mutex<Vec<(PathBuf, String, u64)>> = std::sync::Mutex::new(Vec::new());
    let key = format!("{project}\u{0}{number}\u{0}{platform}\u{0}{}\u{0}{msg}", res.is_ok());
    let now = now_secs();
    {
        let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
        recent.retain(|r| now.saturating_sub(r.2) < 5);
        if is_repeat(&recent, p, &key, now) {
            return;
        }
        recent.push((p.to_path_buf(), key, now));
    }
    let mut line = json!({"t": now, "project": project, "number": number, "platform": platform, "ok": res.is_ok(), "msg": msg});
    // нові поля додаємо лише коли вони є: старі версії програми їх ігнорують, а старі рядки читаються як раніше
    for (k, v) in [("pid", &ctx.project_id), ("vname", &ctx.name), ("rtype", &ctx.release_type), ("mc", &ctx.mc_versions)] {
        if !v.is_empty() {
            line[k] = json!(v);
        }
    }
    if !ctx.loaders.is_empty() {
        line["loaders"] = json!(ctx.loaders);
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{line}");
    }
    // shortcut: обрізаємо до 2000 рядків, коли файл виріс; для більшого обсягу потрібна ротація
    if std::fs::metadata(p).is_ok_and(|m| m.len() > 1_000_000) {
        let all = std::fs::read_to_string(p).unwrap_or_default();
        let keep: Vec<&str> = all.lines().rev().take(2000).collect();
        let _ = write_atomic(p, (keep.into_iter().rev().collect::<Vec<_>>().join("\n") + "\n").as_bytes());
    }
}

/// Останні n записів, найновіші першими.
pub fn history_load(n: usize) -> Vec<Value> {
    history_path().map(|p| history_load_at(&p, n)).unwrap_or_default()
}

fn history_load_at(p: &Path, n: usize) -> Vec<Value> {
    // читаємо лише хвіст файлу (старт не залежить від розміру історії); перший рядок вікна може бути обрізаний
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(p) else { return vec![] };
    let len = f.metadata().map_or(0, |m| m.len());
    let win = (n as u64 * 1024).max(128 * 1024);
    let start = len.saturating_sub(win);
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(start)).is_err() || f.read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let s = String::from_utf8_lossy(&buf);
    let body = if start > 0 { s.split_once('\n').map_or("", |x| x.1) } else { &s };
    body.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(Value::is_object).take(n).collect()
}

/// Видаляє всю історію публікацій.
pub fn history_clear() -> Result<(), String> {
    match history_path() {
        Some(p) => history_clear_at(&p),
        None => Ok(()),
    }
}

fn history_clear_at(p: &Path) -> Result<(), String> {
    match std::fs::remove_file(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// Записи, що містять query (без регістру) у проєкті, номері, платформі чи повідомленні; only_fail лишає помилки.
pub fn history_filter<'a>(h: &'a [Value], query: &str, only_fail: bool) -> Vec<&'a Value> {
    let q = query.trim().to_lowercase();
    h.iter()
        .filter(|e| !only_fail || e["ok"] != true)
        .filter(|e| q.is_empty() || ["project", "number", "platform", "msg", "vname"].iter().any(|k| e[*k].as_str().is_some_and(|s| s.to_lowercase().contains(&q))))
        .collect()
}

/// Посилання на опубліковану версію з повідомлення успішного запису історії.
pub fn history_link(e: &Value) -> Option<String> {
    if e["ok"] != true {
        return None;
    }
    e["msg"].as_str()?.split_whitespace().find(|w| w.starts_with("https://")).map(String::from)
}

fn csv_cell(s: &str) -> String {
    // лапки подвоюємо; формули Excel (=,+,-,@) нейтралізуємо апострофом
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() };
    format!("\"{}\"", s.replace('"', "\"\"").replace(['\r', '\n'], " "))
}

/// Історія у CSV (від старих до нових): час UTC, проєкт, номер, платформа, результат, повідомлення.
#[cfg(test)]
pub fn history_csv(h: &[Value]) -> String {
    let mut out = String::from(CSV_HEAD);
    for e in h.iter().rev() {
        out += &csv_row(e);
    }
    out
}

const CSV_HEAD: &str = "\u{feff}time_utc,project,number,platform,result,message\r\n";

fn csv_row(e: &Value) -> String {
    let t = e["t"].as_u64().unwrap_or(0);
    let (y, m, d) = ymd(t);
    let when = format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", t % 86400 / 3600, t % 3600 / 60, t % 60);
    let g = |k: &str| csv_cell(e[k].as_str().unwrap_or(""));
    format!("{when},{},{},{},{},{}\r\n", g("project"), g("number"), g("platform"), if e["ok"] == true { "ok" } else { "error" }, g("msg"))
}

/// Експортує ВСЮ історію з файлу в CSV потоково (рядок за рядком, без читання файлу в пам'ять); від старих до нових.
/// Пошкоджені рядки пропускаються. Повертає кількість записів.
pub fn history_export_to(dst: &Path) -> Result<usize, String> {
    let src = history_path().ok_or("немає теки даних")?;
    history_export_at(&src, dst)
}

fn history_export_at(src: &Path, dst: &Path) -> Result<usize, String> {
    use std::io::{BufRead, Write};
    let mut out = std::io::BufWriter::new(std::fs::File::create(dst).map_err(|e| format!("Не вдалося створити файл: {e}"))?);
    out.write_all(CSV_HEAD.as_bytes()).map_err(|e| e.to_string())?;
    let mut n = 0;
    if let Ok(f) = std::fs::File::open(src) {
        for line in std::io::BufReader::new(f).split(b'\n') {
            let line = line.map_err(|e| e.to_string())?;
            if let Ok(e) = serde_json::from_slice::<Value>(&line) {
                out.write_all(csv_row(&e).as_bytes()).map_err(|e| e.to_string())?;
                n += 1;
            }
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(n)
}

/// "5 хв тому" для запису з відміткою часу t.
pub fn ago(t: u64) -> String {
    let d = now_secs().saturating_sub(t);
    match d {
        0..=59 => "щойно".into(),
        60..=3599 => format!("{} хв тому", d / 60),
        3600..=86399 => format!("{} год тому", d / 3600),
        _ => {
            let (y, m, day) = ymd(t);
            format!("{day:02}.{m:02}.{y} UTC")
        }
    }
}

/// Дата (UTC) з Unix-часу, алгоритм civil_from_days.
fn ymd(t: u64) -> (i64, u32, u32) {
    let z = (t / 86400) as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Запис через тимчасовий файл + rename: обрив посеред запису не лишає обрізаний файл.
fn write_atomic(p: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = p.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600); // токени: читає лише власник
    let mut f = o.open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?; // дані на диску до rename
    drop(f);
    std::fs::rename(&tmp, p)
}

pub fn save_config(cfg: &Config) -> Result<(), String> {
    save_to(&config_path().ok_or("немає теки конфігу")?, cfg)
}

/// Атомарний запис; попередній чинний конфіг лишається як config.backup.json.
pub fn save_to(p: &Path, cfg: &Config) -> Result<(), String> {
    std::fs::create_dir_all(p.parent().ok_or("немає теки конфігу")?).map_err(|e| e.to_string())?;
    let s = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    if [&cfg.modrinth_token, &cfg.curse_token, &cfg.curse_core_key].iter().all(|t| t.trim().is_empty()) {
        // токенів немає (напр. після «Очистити токени»): копії з токенами не лишаємо
        let _ = std::fs::remove_file(backup_path(p));
        let _ = std::fs::remove_file(p.with_file_name(format!("config.pre-v{SCHEMA}.json")));
    } else if std::fs::read_to_string(p).is_ok_and(|old| old != s && serde_json::from_str::<Config>(&old).is_ok()) {
        let _ = std::fs::copy(p, backup_path(p)); // збій копії не блокує збереження
    }
    write_atomic(p, s.as_bytes()).map_err(|e| e.to_string())
}

/// Проєкти без токенів у JSON для перенесення на інший комп'ютер.
pub fn export_projects(projects: &[Project]) -> String {
    let clean: Vec<Project> = projects.iter().map(strip_secrets).collect();
    serde_json::to_string_pretty(&json!({"modwire_projects": 1, "projects": clean})).unwrap_or_default()
}

/// Розбирає експорт (або повний config.json) у список проєктів; токени ігноруються.
pub fn parse_projects_file(s: &str) -> Result<Vec<Project>, String> {
    let v: Value = serde_json::from_str(s).map_err(|e| format!("не JSON: {e}"))?;
    let list = if v.is_array() { v } else { v["projects"].clone() };
    let ps: Vec<Project> = serde_json::from_value(list).map_err(|_| "У файлі немає списку проєктів".to_string())?;
    // секретоподібні поля з чужого файлу не потрапляють у наш конфіг
    Ok(ps.iter().map(strip_secrets).filter(|p| !p.name.trim().is_empty() || !p.modrinth_id.is_empty() || !p.curse_id.is_empty()).collect())
}

/// Зливає імпортовані проєкти: збіг за ID платформи доповнює наявний, решта додається. (додано, об'єднано)
pub fn merge_projects(cfg: &mut Config, incoming: Vec<Project>) -> (usize, usize) {
    let (mut added, mut merged) = (0, 0);
    for p in incoming {
        let same = |q: &Project| (!p.modrinth_id.is_empty() && q.modrinth_id == p.modrinth_id) || (!p.curse_id.is_empty() && q.curse_id == p.curse_id);
        match cfg.projects.iter_mut().find(|q| same(q)) {
            Some(t) => {
                if t.modrinth_id.is_empty() { t.modrinth_id = p.modrinth_id; }
                if t.curse_id.is_empty() { t.curse_id = p.curse_id; }
                if t.icon_url.is_empty() { t.icon_url = p.icon_url; }
                if t.changelog_template.is_empty() { t.changelog_template = p.changelog_template; }
                if t.name_template.is_empty() { t.name_template = p.name_template; }
                merged += 1;
            }
            None => {
                cfg.projects.push(p);
                added += 1;
            }
        }
    }
    (added, merged)
}

/// Підставляє {version} {mc} {loaders} {date} {project} у шаблон змін.
pub fn render_template(tpl: &str, project: &str, r: &Release) -> String {
    let (y, m, d) = ymd(now_secs());
    tpl.replace("{version}", r.number.trim())
        .replace("{mc}", &r.mc_versions.join(", "))
        .replace("{loaders}", &r.loaders.join(", "))
        .replace("{date}", &format!("{y}-{m:02}-{d:02}"))
        .replace("{project}", project)
}

#[derive(Clone, Default)]
pub struct Release {
    pub name: String,
    pub number: String,
    pub mc_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub release_type: String,
    pub changelog: String,
    /// Лише Modrinth: позначити версію рекомендованою.
    pub featured: bool,
    /// Лише Modrinth: slug/id модів-залежностей, що їх користувач вирішив надіслати (зіставляються з проєктами перед відправкою).
    pub dep_slugs: Vec<String>,
    /// Лише Modrinth: id проєктів-залежностей після зіставлення (заповнює publish_modrinth).
    pub dep_ids: Vec<String>,
    /// Лише Modrinth: додаткові файли версії (sources, dev-jar), до MAX_EXTRA.
    pub extra_files: Vec<PathBuf>,
}

pub const MAX_EXTRA: usize = 3;

/// Перевірка додаткового файлу: існує, .jar/.zip, не порожній, у межах ліміту, не збігається з основним за назвою.
fn check_extra(e: &Path, main: &Path) -> Result<(), String> {
    let name = e.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !e.is_file() {
        return Err(format!("Додатковий файл «{name}» не існує"));
    }
    let ext = e.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
    if !["jar", "zip"].contains(&ext.as_str()) {
        return Err(format!("Додатковий файл «{name}»: дозволені лише .jar і .zip"));
    }
    match std::fs::metadata(e).map(|m| m.len()) {
        Ok(0) => return Err(format!("Додатковий файл «{name}» порожній (0 байт)")),
        Ok(n) if n > MAX_FILE => return Err(format!("Додатковий файл «{name}» завеликий ({} МБ): ліміт {} МБ", n >> 20, MAX_FILE >> 20)),
        _ => {}
    }
    if e.file_name() == main.file_name() {
        return Err(format!("Додатковий файл має ту саму назву, що й основний («{name}»): Modrinth таке відхиляє"));
    }
    Ok(())
}

/// Короткий підсумок майбутньої відправки для нижньої панелі: що саме піде на сайти.
pub fn release_summary(project: &str, r: &Release, mr: bool, cf: bool) -> String {
    let mut p = vec![format!("«{}»", display_name(r)), r.release_type.clone(), format!("MC {}", r.mc_versions.join(", ")), r.loaders.join(", ")];
    p.push(match (mr, cf) { (true, true) => "Modrinth + CurseForge", (true, false) => "Modrinth", (false, true) => "CurseForge", _ => "без платформ" }.into());
    if mr {
        if !r.extra_files.is_empty() { p.push(format!("+{} дод. файл(и)", r.extra_files.len())); }
        if !r.dep_slugs.is_empty() { p.push(format!("залежності: {}", r.dep_slugs.join(", "))); }
        if r.featured { p.push("рекомендована".into()); }
    }
    format!("{project}: {}", p.join(" · "))
}

/// Тип релізу за позначкою в номері: alpha/snapshot/dev → alpha, beta/rc/pre → beta; без позначки None.
pub fn detect_release_type(number: &str) -> Option<&'static str> {
    let low = number.to_lowercase();
    let mut found = None;
    for t in low.split(|c: char| !c.is_ascii_alphanumeric()).skip(1) {
        match t.trim_end_matches(|c: char| c.is_ascii_digit()) {
            "alpha" | "snapshot" | "dev" | "nightly" => return Some("alpha"),
            "beta" | "rc" | "pre" | "preview" => found = Some("beta"),
            _ => {}
        }
    }
    found
}

/// Наступний номер версії: збільшує останнє число ("1.0.9" -> "1.0.10"). Без цифр повертає "".
pub fn bump(n: &str) -> String {
    match n.split_once('+') {
        Some((a, b)) => match bump_core(a).as_str() {
            "" => String::new(),
            x => format!("{x}+{b}"),
        },
        None => bump_core(n),
    }
}

fn bump_core(n: &str) -> String {
    let b = n.as_bytes();
    let Some(end) = b.iter().rposition(|c| c.is_ascii_digit()) else { return String::new() };
    let start = b[..=end].iter().rposition(|c| !c.is_ascii_digit()).map_or(0, |i| i + 1);
    match n[start..=end].parse::<u64>() {
        Ok(v) if v < u64::MAX => format!("{}{}{}", &n[..start], v + 1, &n[end + 1..]),
        _ => String::new(),
    }
}

fn human(st: reqwest::StatusCode) -> &'static str {
    match st.as_u16() {
        400 => crate::i18n::tr("Сервер відхилив дані. Перевірте версії MC, лоадери й номер версії та спробуйте знову"),
        401 => crate::i18n::tr("Токен недійсний або без потрібних прав. Створіть новий токен на сайті й вставте його в «Налаштування»"),
        403 => crate::i18n::tr("Немає доступу до проєкту. Перевірте токен і ID проєкту: токен має належати автору або учаснику проєкту"),
        404 => crate::i18n::tr("Не знайдено. Перевірте ID проєкту"),
        409 | 422 => crate::i18n::tr("Така версія вже існує або дані некоректні. Змініть номер версії або перевірте поля й спробуйте знову"),
        413 => crate::i18n::tr("Файл завеликий для платформи. Оберіть менший файл (ліміт 500 МБ)"),
        429 => crate::i18n::tr("Забагато запитів. Зачекайте хвилину й повторіть"),
        408 | 502..=504 => crate::i18n::tr("Сервіс тимчасово недоступний (помилка на боці сервера). Зачекайте кілька хвилин і повторіть; статус сервісу дивіться на його сайті"),
        500..=599 => crate::i18n::tr("Помилка на боці сервера. Зачекайте й повторіть пізніше; якщо не минає, перевірте статус сервісу"),
        _ => crate::i18n::tr("Помилка запиту. Перевірте дані й токен та повторіть; якщо не минає, напишіть автору програми"),
    }
}

/// Тести, що ставлять MODWIRE_CONFIG_DIR (env глобальний), беруть цей замок.
#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Default, Debug, Clone)]
pub struct JarInfo {
    pub version: Option<String>,
    pub name: Option<String>,
    pub loaders: Vec<&'static str>,
    /// Лише точно задані версії MC (діапазони на кшталт ">=1.20" не вгадуються).
    pub mc_versions: Vec<String>,
    /// Обов'язкові залежності-моди (без Minecraft і самого лоадера): лише для інформування автора.
    pub deps: Vec<String>,
}

/// Скільки записів архіву переглядаємо, щоб відрізнити ресурспак від датапаку (великі паки мають тисячі файлів).
const MAX_PACK_SCAN: usize = 20_000;
const MAX_DEPS_SEEN: usize = 64;
/// Скільки залежностей зіставляємо з Modrinth за одну публікацію (кожна = мережевий запит).
const MAX_DEPS_SEND: usize = 20;

/// Slug/id проєкту безпечний для підстановки в URL (літери, цифри, - _ .; без / ? # та "..").
fn slug_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// Id мода-залежності без службових (minecraft, java, лоадери).
fn push_dep(out: &mut Vec<String>, id: &str) {
    let id = id.trim();
    let skip = ["minecraft", "java", "fabricloader", "fabric-loader", "quilt_loader", "quilt-loader", "forge", "neoforge", "fml", "javafml", "mixinextras"];
    // ліміт: jar з тисячами залежностей не має роздувати список і гальмувати перевірку дублів
    if out.len() < MAX_DEPS_SEEN && !id.is_empty() && !skip.contains(&id) && !out.iter().any(|o| o == id) {
        out.push(id.to_string());
    }
}

/// Обов'язкові залежності з [[dependencies.*]] mods.toml (mandatory=true або type="required").
pub fn toml_deps(t: &str) -> Vec<String> {
    let mut out = vec![];
    let (mut in_dep, mut id, mut req) = (false, String::new(), false);
    let val = |l: &str, k: &str| {
        let r = l.strip_prefix(k)?.trim_start().strip_prefix('=')?;
        Some(r.split('#').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'').to_string())
    };
    for l in t.lines().map(str::trim) {
        if l.starts_with('[') {
            if in_dep && req {
                push_dep(&mut out, &id);
            }
            (in_dep, id, req) = (l.starts_with("[[dependencies."), String::new(), false);
        } else if let Some(v) = val(l, "modId") {
            id = v;
        } else if let Some(v) = val(l, "mandatory") {
            req = v == "true";
        } else if let Some(v) = val(l, "type") {
            req = v == "required";
        }
    }
    if in_dep && req {
        push_dep(&mut out, &id);
    }
    out
}

/// "1.20.1", "[1.20.1]" або "=1.20.1" → точна версія MC; діапазони й маски → None.
pub fn mc_exact(s: &str) -> Option<String> {
    let v = s.trim().trim_start_matches('=').trim_matches(|c| c == '[' || c == ']').trim();
    let ok = v.split('.').count() >= 2 && v.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()));
    ok.then(|| v.to_string())
}

fn push_mc(out: &mut Vec<String>, v: Option<String>) {
    if let Some(v) = v.filter(|v| out.len() < MAX_DEPS_SEEN && !out.contains(v)) {
        out.push(v);
    }
}

/// Точні версії minecraft із блоку [[dependencies.*]] з modId="minecraft" у mods.toml.
pub fn toml_mc(t: &str) -> Vec<String> {
    let (mut in_dep, mut is_mc, mut range) = (false, false, None::<String>);
    let mut out = vec![];
    let val = |l: &str, k: &str| {
        let r = l.strip_prefix(k)?.trim_start().strip_prefix('=')?;
        Some(r.split('#').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'').to_string())
    };
    let mut flush = |in_dep: bool, is_mc: bool, range: &mut Option<String>| {
        if in_dep && is_mc && let Some(r) = range.take() {
            push_mc(&mut out, mc_exact(&r));
        }
        *range = None;
    };
    for l in t.lines().map(str::trim) {
        if l.starts_with('[') {
            flush(in_dep, is_mc, &mut range);
            in_dep = l.starts_with("[[dependencies.");
            is_mc = false;
            continue;
        }
        if let Some(v) = val(l, "modId") {
            is_mc = v == "minecraft";
        } else if let Some(v) = val(l, "versionRange") {
            range = Some(v);
        }
    }
    flush(in_dep, is_mc, &mut range);
    out
}

fn manifest_version(t: &str) -> Option<String> {
    let v = t.lines().find_map(|l| l.strip_prefix("Implementation-Version:"))?.trim();
    (!v.is_empty() && !v.starts_with("${")).then(|| v.to_string())
}

/// Версія з [[mods]] у (neoforge.)mods.toml; плейсхолдери на кшталт ${file.jarVersion} ігноруємо.
pub fn toml_version(t: &str) -> Option<String> {
    let mut in_mods = false;
    for l in t.lines().map(str::trim) {
        if l.starts_with('[') {
            in_mods = l.split('#').next().unwrap_or("").trim() == "[[mods]]";
            continue;
        }
        let Some(rest) = l.strip_prefix("version").filter(|_| in_mods) else { continue };
        let Some(v) = rest.trim_start().strip_prefix('=') else { continue };
        let v = v.split('#').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'');
        if !v.is_empty() && !v.starts_with("${") {
            return Some(v.to_string());
        }
    }
    None
}

fn jar_entry(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    use std::io::Read;
    let f = z.by_name(name).ok()?;
    let mut s = String::new();
    f.take(1_000_000).read_to_string(&mut s).ok()?;
    Some(s)
}

/// Версія й лоадери з метаданих jar. None, якщо файл не zip.
/// Попередження, якщо вміст файлу не збігається з обраними лоадерами / версіями MC (порожній список = все гаразд).
pub fn compat_warnings(info: &JarInfo, mc: &[String], loaders: &[String]) -> Vec<String> {
    let mut w = vec![];
    if !info.loaders.is_empty() {
        // Quilt запускає Fabric-моди, тож для fabric-файлу quilt не вважаємо помилкою
        let ok = |l: &str| info.loaders.contains(&l) || (l == "quilt" && info.loaders.contains(&"fabric"));
        let missing: Vec<&String> = loaders.iter().filter(|l| !ok(l.as_str())).collect();
        if !missing.is_empty() {
            let m: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
            w.push(format!("Файл виглядає як {}, але вибрано також: {}. Перевірте лоадери.", info.loaders.join("/"), m.join(", ")));
        }
    }
    if !info.mc_versions.is_empty() {
        let extra: Vec<&str> = mc.iter().filter(|v| !info.mc_versions.contains(v)).map(String::as_str).collect();
        if !extra.is_empty() {
            w.push(format!("У файлі вказано Minecraft {}, а вибрано також: {}. Перевірте версії.", info.mc_versions.join(", "), extra.join(", ")));
        }
    }
    w
}

pub fn read_jar(path: &Path) -> Option<JarInfo> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut info = JarInfo::default();
    let real = |v: Option<&str>| v.filter(|v| !v.is_empty() && !v.starts_with("${")).map(String::from);
    if let Some(t) = jar_entry(&mut z, "fabric.mod.json") {
        info.loaders.push("fabric");
        let v: Value = serde_json::from_str(&t).unwrap_or_default();
        info.version = info.version.or(real(v["version"].as_str()));
        info.name = info.name.or(real(v["name"].as_str()));
        match &v["depends"]["minecraft"] {
            Value::String(s) => push_mc(&mut info.mc_versions, mc_exact(s)),
            Value::Array(a) => a.iter().for_each(|s| push_mc(&mut info.mc_versions, s.as_str().and_then(mc_exact))),
            _ => {}
        }
        if let Some(m) = v["depends"].as_object() {
            m.keys().for_each(|k| push_dep(&mut info.deps, k));
        }
    }
    if let Some(t) = jar_entry(&mut z, "quilt.mod.json") {
        info.loaders.push("quilt");
        let v: Value = serde_json::from_str(&t).unwrap_or_default();
        info.version = info.version.or(real(v["quilt_loader"]["version"].as_str()));
        info.name = info.name.or(real(v["quilt_loader"]["metadata"]["name"].as_str()));
        for d in v["quilt_loader"]["depends"].as_array().into_iter().flatten() {
            if let Some(id) = d.as_str().or(d["id"].as_str()) {
                push_dep(&mut info.deps, id);
            }
        }
    }
    for (entry, loader) in [("META-INF/neoforge.mods.toml", "neoforge"), ("META-INF/mods.toml", "forge")] {
        if let Some(t) = jar_entry(&mut z, entry) {
            info.loaders.push(loader);
            info.version = info.version.or(toml_version(&t));
            toml_deps(&t).iter().for_each(|d| push_dep(&mut info.deps, d));
            toml_mc(&t).into_iter().for_each(|v| push_mc(&mut info.mc_versions, Some(v)));
        }
    }
    // .mrpack: modrinth.index.json з точними залежностями
    if let Some(t) = jar_entry(&mut z, "modrinth.index.json") {
        let v: Value = serde_json::from_str(&t).unwrap_or_default();
        info.version = info.version.or(real(v["versionId"].as_str()));
        info.name = info.name.or(real(v["name"].as_str()));
        push_mc(&mut info.mc_versions, v["dependencies"]["minecraft"].as_str().and_then(mc_exact));
        for (dep, loader) in [("fabric-loader", "fabric"), ("quilt-loader", "quilt"), ("forge", "forge"), ("neoforge", "neoforge")] {
            if v["dependencies"][dep].is_string() && !info.loaders.contains(&loader) {
                info.loaders.push(loader);
            }
        }
    }
    // ресурспак/датапак: є pack.mcmeta, а метаданих мода немає. assets/ = ресурспак (лоадер minecraft), data/ = датапак
    // Ресурспаки завжди .zip: .jar або mcmod.info (Forge 1.12 і старші, теж з pack.mcmeta й assets/) означає мод, а не пак.
    // pack.mcmeta шукаємо за іменем (не залежить від ліміту й порядку записів), у списку імен лише префікс assets/ чи data/.
    let is_jar = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar"));
    if info.loaders.is_empty() && !is_jar && z.index_for_name("mcmod.info").is_none() && z.index_for_name("pack.mcmeta").is_some() {
        let names: Vec<String> = z.file_names().take(MAX_PACK_SCAN).flatten().map(|n| n.into_owned()).collect();
        if names.iter().any(|n| n.starts_with("assets/")) {
            info.loaders.push("minecraft");
        } else if names.iter().any(|n| n.starts_with("data/")) {
            info.loaders.push("datapack");
        }
    }
    if info.version.is_none() {
        info.version = jar_entry(&mut z, "META-INF/MANIFEST.MF").and_then(|t| manifest_version(&t));
    }
    Some(info)
}

/// Версії Minecraft з Modrinth /tag/game_version (нові першими): (назва, чи реліз).
pub fn parse_game_versions(v: &Value) -> Vec<(String, bool)> {
    v.as_array().map(|a| {
        a.iter()
            .filter_map(|g| Some((g["version"].as_str()?.to_string(), g["version_type"] == "release")))
            .collect()
    }).unwrap_or_default()
}

/// Публічний список версій MC (без токена).
pub fn game_versions() -> Result<Vec<(String, bool)>, String> {
    let r = client_get()?.get(format!("{}/v2/tag/game_version", mr())).send();
    Ok(parse_game_versions(&json_of(r.map_err(net_err)?)?))
}

/// Перша непорожня секція змін (без заголовка). Береться рівень 2, а якщо його нема, найвищий наявний;
/// підзаголовки глибшого рівня лишаються в секції, `#` усередині ``` ігноруються.
/// Файл без заголовків повертається цілком.
pub fn first_section(t: &str) -> String {
    let level = |l: &str| {
        let n = l.chars().take_while(|c| *c == '#').count();
        if n > 0 && l.chars().nth(n) == Some(' ') { n } else { 0 }
    };
    let lines: Vec<&str> = t.lines().collect();
    let mut fence = false;
    let heads: Vec<(usize, usize)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            if l.trim_start().starts_with("```") {
                fence = !fence;
                return None;
            }
            (!fence && level(l) > 0).then(|| (i, level(l)))
        })
        .collect();
    let Some(min) = heads.iter().map(|h| h.1).min() else { return t.trim().to_string() };
    let lv = if heads.iter().any(|h| h.1 == 2) { 2 } else { min };
    for (k, &(start, l)) in heads.iter().enumerate() {
        if l != lv {
            continue;
        }
        let end = heads[k + 1..].iter().find(|h| h.1 <= lv).map_or(lines.len(), |h| h.0);
        let body = lines[start + 1..end].join("\n").trim().to_string();
        if !body.is_empty() {
            return body;
        }
    }
    String::new()
}

pub fn parse_mc(s: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for v in s.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|v| !v.is_empty()) {
        if !out.iter().any(|o| o == v) {
            out.push(v.to_string());
        }
    }
    out
}

const MAX_NUMBER: usize = 64;
/// Ліміт Modrinth на changelog; CurseForge не нижчий.
const MAX_CHANGELOG: usize = 65536;

/// Порядок: проєкт+платформа → номер/MC/лоадери → файл існує → розширення.
pub fn validate(project_ok: bool, any_platform: bool, r: &Release, file: Option<&Path>) -> Result<(), String> {
    if !project_ok {
        return Err("Оберіть проєкт".into());
    }
    if !any_platform {
        return Err("Увімкніть хоча б одну платформу".into());
    }
    if r.number.trim().is_empty() {
        return Err("Вкажіть номер версії".into());
    }
    if r.number.trim().chars().count() > MAX_NUMBER {
        return Err(format!("Номер версії задовгий (максимум {MAX_NUMBER} символів)"));
    }
    if r.changelog.chars().count() > MAX_CHANGELOG {
        return Err(format!("Changelog задовгий ({} із {MAX_CHANGELOG} символів)", r.changelog.chars().count()));
    }
    if r.mc_versions.is_empty() {
        return Err("Вкажіть версії Minecraft".into());
    }
    if r.loaders.is_empty() {
        return Err("Оберіть хоча б один лоадер".into());
    }
    let f = file.ok_or("Оберіть файл")?;
    if r.extra_files.len() > MAX_EXTRA {
        return Err(format!("Додаткових файлів не більше {MAX_EXTRA}"));
    }
    if !f.is_file() {
        return Err("Файл не існує".into());
    }
    let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !["jar", "zip", "mrpack"].contains(&ext.as_str()) {
        return Err("Дозволені лише .jar, .zip, .mrpack".into());
    }
    match std::fs::metadata(f).map(|m| m.len()) {
        Ok(0) => return Err("Файл порожній (0 байт)".into()),
        Ok(n) if n > MAX_FILE => return Err(format!("Файл завеликий ({} МБ): ліміт платформ {} МБ", n >> 20, MAX_FILE >> 20)),
        _ => {}
    }
    for (k, e) in r.extra_files.iter().enumerate() {
        check_extra(e, f)?;
        if r.extra_files[..k].iter().any(|o| o.file_name() == e.file_name()) {
            return Err("Серед додаткових файлів є однакові назви".into());
        }
    }
    Ok(())
}

fn display_name(r: &Release) -> &str {
    if r.name.trim().is_empty() { r.number.trim() } else { r.name.trim() }
}

pub fn modrinth_data(r: &Release, project_id: &str) -> Value {
    json!({
        "name": display_name(r),
        "version_number": r.number.trim(),
        "changelog": r.changelog,
        "dependencies": r.dep_ids.iter().map(|id| json!({"project_id": id, "dependency_type": "required"})).collect::<Vec<_>>(),
        "game_versions": r.mc_versions,
        "version_type": r.release_type,
        "loaders": r.loaders,
        "featured": r.featured,
        "project_id": project_id,
        "file_parts": std::iter::once("file".to_string()).chain((1..=r.extra_files.len()).map(|i| format!("file{i}"))).collect::<Vec<_>>(),
    })
}

pub fn curse_metadata(r: &Release, ids: &[u64]) -> Value {
    json!({
        "changelog": r.changelog,
        "changelogType": "markdown",
        "displayName": display_name(r),
        "gameVersions": ids,
        "releaseType": r.release_type,
    })
}

/// Назви версій не унікальні між типами: беремо лише типи "Minecraft*" та "Modloader".
pub fn resolve_curse_ids(types: &Value, versions: &Value, mc: &[String], loaders: &[String]) -> Result<Vec<u64>, String> {
    let empty = vec![];
    let mut mc_types = vec![];
    let mut loader_type = None;
    for t in types.as_array().unwrap_or(&empty) {
        let (Some(id), Some(name)) = (t["id"].as_u64(), t["name"].as_str()) else { continue };
        if name.starts_with("Minecraft") {
            mc_types.push(id);
        } else if name == "Modloader" {
            loader_type = Some(id);
        }
    }
    let vers = versions.as_array().unwrap_or(&empty);
    let find = |ok: &dyn Fn(u64) -> bool, want: &str| {
        vers.iter().find_map(|v| {
            let name = v["name"].as_str()?;
            (ok(v["gameVersionTypeID"].as_u64()?) && name.eq_ignore_ascii_case(want)).then(|| v["id"].as_u64())?
        })
    };
    let (mut ids, mut missing) = (vec![], vec![]);
    for m in mc {
        match find(&|t| mc_types.contains(&t), m) {
            Some(id) => ids.push(id),
            None => missing.push(format!("версію MC {m}")),
        }
    }
    // ресурспаки й датапаки на CurseForge мають лише версії Minecraft, без модлоадера
    for l in loaders.iter().filter(|l| !is_pack_loader(l)) {
        match find(&|t| Some(t) == loader_type, l) {
            Some(id) => ids.push(id),
            None => missing.push(format!("лоадер {l}")),
        }
    }
    if missing.is_empty() { Ok(ids) } else { Err(format!("CurseForge не знає: {}", missing.join(", "))) }
}

pub struct MrProject {
    pub id: String,
    pub title: String,
    pub slug: String,
    pub icon_url: String,
}

pub fn parse_user(v: &Value) -> Option<(String, String)> {
    Some((v["id"].as_str()?.into(), v["username"].as_str()?.into()))
}

pub fn parse_projects(v: &Value) -> Vec<MrProject> {
    v.as_array().map(|a| {
        a.iter()
            .filter_map(|p| {
                Some(MrProject {
                    id: p["id"].as_str()?.into(),
                    title: p["title"].as_str()?.into(),
                    slug: p["slug"].as_str().unwrap_or("").into(),
                    icon_url: p["icon_url"].as_str().unwrap_or("").into(),
                })
            })
            .collect()
    }).unwrap_or_default()
}

/// (назва, url іконки) з відповіді CurseForge Core API.
pub fn parse_curse_mod(v: &Value) -> Option<(String, String)> {
    let d = &v["data"];
    let icon = d["logo"]["thumbnailUrl"].as_str().or(d["logo"]["url"].as_str()).unwrap_or("");
    Some((d["name"].as_str()?.into(), icon.into()))
}

// ---------- мережа ----------

/// Прогрес відправки файлу (байти).
#[derive(Default)]
pub struct Progress {
    pub sent: AtomicU64,
    pub total: AtomicU64,
    /// Початок відправки, мс від UNIX-епохи (0 = ще не почато).
    pub started_ms: AtomicU64,
}

/// Швидкість (байт/с) і орієнтовний залишок (с) за відправленим і часом; None, поки замало даних.
pub fn speed_eta(sent: u64, total: u64, elapsed_ms: u64) -> Option<(u64, u64)> {
    if sent == 0 || elapsed_ms < 500 {
        return None;
    }
    let bps = sent * 1000 / elapsed_ms;
    (bps > 0).then(|| (bps, total.saturating_sub(sent) / bps))
}

impl Progress {
    pub fn speed_eta(&self) -> Option<(u64, u64)> {
        let st = self.started_ms.load(Relaxed);
        (st > 0).then(|| speed_eta(self.sent.load(Relaxed), self.total.load(Relaxed), now_secs_ms().saturating_sub(st))).flatten()
    }
}

fn now_secs_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Спільний стан завантаження: прогрес платформи + прапорець скасування.
#[derive(Clone, Default)]
pub struct Up {
    pub prog: Arc<Progress>,
    pub cancel: Arc<AtomicBool>,
    /// true, поки платформа публікується (для окремих кнопок скасування).
    pub active: Arc<AtomicBool>,
}

/// Читач файлу, що рахує відправлені байти й обривається при скасуванні.
struct Tracked {
    f: std::fs::File,
    up: Up,
}

impl std::io::Read for Tracked {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.up.cancel.load(Relaxed) {
            return Err(std::io::Error::other("cancelled"));
        }
        let n = self.f.read(buf)?;
        self.up.prog.sent.fetch_add(n as u64, Relaxed);
        Ok(n)
    }
}

/// Таймаут завантаження файлу: великі збірки на повільному каналі, але не нескінченно.
const UPLOAD_SECS: u64 = 3600;

/// Верхня межа розміру файлу (Modrinth/CurseForge приймають до ~500 МБ).
const MAX_FILE: u64 = 500 << 20;

#[cfg(test)]
static TEST_BASE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Базова адреса сервісу; у тестах підміняється на локальний mock-сервер.
fn base(default: &str) -> String {
    #[cfg(test)]
    if let Some(b) = TEST_BASE.lock().unwrap().clone() {
        return b;
    }
    default.to_string()
}

fn mr() -> String {
    base("https://api.modrinth.com")
}
fn cf() -> String {
    base("https://minecraft.curseforge.com")
}
fn core() -> String {
    base("https://api.curseforge.com")
}

/// Для завантаження файлів (довгий таймаут).
fn client() -> Result<Client, String> {
    client_t(300)
}

/// Для коротких GET-запитів (перевірка токена, списки, версії): не висимо хвилинами на мертвому з'єднанні.
fn client_get() -> Result<Client, String> {
    client_t(30)
}

/// Клієнт для запитів із токенами: редирект лише в межах того самого хоста й без пониження https, інакше
/// заголовки X-Api-Token / x-api-key (reqwest знімає лише Authorization) пішли б на чужий сервер.
fn client_t(secs: u64) -> Result<Client, String> {
    client_with(secs, reqwest::redirect::Policy::custom(|a| {
        let prev = a.previous().last();
        let ok = a.previous().len() < 5 && prev.is_some_and(|p| p.host_str() == a.url().host_str() && p.port_or_known_default() == a.url().port_or_known_default() && !(p.scheme() == "https" && a.url().scheme() != "https"));
        if ok { a.follow() } else { a.stop() }
    }))
}

/// Клієнт без токенів (іконки, GitHub, завантаження оновлення): звичайні редиректи, напр. на CDN.
fn client_open(secs: u64) -> Result<Client, String> {
    client_with(secs, reqwest::redirect::Policy::limited(5))
}

fn client_with(secs: u64, redirect: reqwest::redirect::Policy) -> Result<Client, String> {
    Client::builder()
        .redirect(redirect)
        .timeout(Duration::from_secs(secs))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("mod-publisher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// Помилка під час/після відправки: сервер міг прийняти файл.
fn sent_err(e: reqwest::Error) -> String {
    format!("{e}. Файл міг бути завантажений — перевірте на сайті перед повтором")
}

const NET_ADVICE: &str = "Немає зв'язку з сервером (мережа або таймаут). Перевірте інтернет, VPN, проксі чи брандмауер і спробуйте ще раз.";
/// Порада про мережу мовою інтерфейсу (константа лишається українською для перевірок).
fn net_advice() -> &'static str {
    crate::i18n::tr(NET_ADVICE)
}

fn net_err(e: reqwest::Error) -> String {
    // is_request: з'єднання закрито/скинуто посеред запиту; проксі, що не відповідає, дає is_connect
    if e.is_timeout() || e.is_connect() || e.is_body() || e.is_decode() || e.is_request() {
        format!("{} ({e})", net_advice())
    } else {
        e.to_string()
    }
}

/// Текст помилки з тіла відповіді API (Modrinth: description, CurseForge: errorMessage), інакше обрізане тіло.
fn api_msg(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let m = ["description", "errorMessage", "message", "error"].iter().find_map(|k| v[*k].as_str()).unwrap_or(body.trim());
    let m: String = m.chars().take(300).collect();
    m
}

const MAX_BODY: u64 = 20 * 1024 * 1024;

fn json_of(resp: reqwest::blocking::Response) -> Result<Value, String> {
    let st = resp.status();
    let retry = resp.headers().get("retry-after").and_then(|h| h.to_str().ok()).map(str::to_string);
    // відповідь читаємо з обмеженням: збійний проксі/сервер не має з'їсти пам'ять
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(resp, MAX_BODY + 1), &mut buf).map_err(|e| format!("{} ({e})", net_advice()))?;
    if buf.len() as u64 > MAX_BODY {
        return Err("Відповідь сервера завелика: схоже на збій сервісу чи сторінку проксі. Спробуйте пізніше".into());
    }
    let body = String::from_utf8_lossy(&buf).into_owned();
    if !st.is_success() {
        let wait = retry.filter(|_| st.as_u16() == 429).map_or(String::new(), |s| format!(" Зачекайте {s} с."));
        return Err(format!("{}.{wait} (HTTP {st}: {})", human(st), api_msg(&body)));
    }
    serde_json::from_str(&body).map_err(|e| format!("Сервер повернув не те, що очікувалось (можливо, сторінка проксі чи збій сервісу): {e}. Спробуйте пізніше"))
}

/// GET із повтором (до 3 спроб) для збоїв мережі, 429 і 5xx. Лише для читання: POST-завантаження не повторюємо.
fn get_json(c: &Client, url: &str, header: (&str, &str)) -> Result<Value, String> {
    let pause = Duration::from_millis(if cfg!(test) { 10 } else { 1500 });
    for attempt in 0..3 {
        let last = attempt == 2;
        let mut wait = None;
        match c.get(url).header(header.0, header.1).send() {
            Ok(r) if !last && (r.status().as_u16() == 429 || r.status().is_server_error()) => {
                // Retry-After сервера поважаємо, але не довше 10 с: це фоновий запит, за яким чекає користувач
                wait = r.headers().get("retry-after").and_then(|h| h.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()).map(|s| Duration::from_secs(s.min(10)));
            }
            Ok(r) => return json_of(r),
            Err(e) if !last && (e.is_timeout() || e.is_connect() || e.is_request()) => {}
            Err(e) => return Err(net_err(e)),
        }
        std::thread::sleep(if cfg!(test) { pause * (attempt + 1) } else { wait.unwrap_or(pause * (attempt + 1)) });
    }
    unreachable!()
}

fn mr_get(c: &Client, token: &str, path: &str) -> Result<Value, String> {
    get_json(c, &format!("{}/v2{path}", mr()), ("Authorization", token))
}

/// Перевірка токена Modrinth: хто це і чи читає він список проєктів. Без права читання (PROJECT_READ) токен
/// може лишатися робочим для публікації за ID, тому це попередження, а не помилка.
pub fn modrinth_token_check(token: &str) -> Result<String, String> {
    let c = client_get()?;
    let (id, name) = parse_user(&mr_get(&c, token, "/user")?).ok_or("немає id користувача")?;
    Ok(match mr_get(&c, token, &format!("/user/{id}/projects")) {
        Ok(v) => format!("{name}, проєктів: {}", v.as_array().map_or(0, Vec::len)),
        Err(e) if e.contains("Токен недійсний") || e.contains("Немає доступу") => {
            format!("{name}. Увага: токен не читає список проєктів (немає права PROJECT_READ). Публікація за ID можлива, якщо є право створення версій (VERSION_CREATE)")
        }
        Err(e) => return Err(e),
    })
}

/// Перевірка доступу до конкретного проєкту CurseForge через Core API-ключ: проєкт існує і його видно.
pub fn curse_project_check(key: &str, id: &str) -> Result<String, String> {
    let (name, _) = curse_mod_info(key, id)?;
    Ok(format!("проєкт «{name}» знайдено"))
}

/// Перевірка upload-токена: ендпоінт версій вимагає чинний X-Api-Token.
pub fn curse_token_check(token: &str) -> Result<String, String> {
    let r = client_get()?.get(format!("{}/api/game/version-types", cf())).header("X-Api-Token", token).send();
    json_of(r.map_err(net_err)?).map(|_| "токен приймається (дійсний для завантаження файлів)".to_string())
}

/// Перевірка Core API-ключа запитом про гру Minecraft (id 432).
pub fn curse_key_check(key: &str) -> Result<String, String> {
    let r = client_get()?.get(format!("{}/v1/games/432", core())).header("x-api-key", key).send();
    json_of(r.map_err(net_err)?).map(|_| "ключ приймається".to_string())
}

pub fn modrinth_projects(token: &str) -> Result<Vec<MrProject>, String> {
    let c = client()?;
    let (id, _) = parse_user(&mr_get(&c, token, "/user")?).ok_or("немає id користувача")?;
    Ok(parse_projects(&mr_get(&c, token, &format!("/user/{id}/projects"))?))
}

/// (назва, url іконки) проєкту Modrinth за id/slug.
fn modrinth_project_info_with(c: &Client, token: &str, id: &str) -> Result<(String, String), String> {
    let v = mr_get(c, token, &format!("/project/{id}"))?;
    Ok((v["title"].as_str().ok_or("немає назви проєкту")?.into(), v["icon_url"].as_str().unwrap_or("").into()))
}

/// Оновлені (modrinth_id, curse_id, назва, іконка) для проєктів, що відповіли; помилки окремих проєктів пропускаються.
/// Один клієнт на всі запити (без нового TLS-з'єднання на кожен проєкт) і не більше 3 запитів одночасно.
pub fn refresh_projects(mr_token: &str, core_key: &str, ps: &[Project]) -> Vec<(String, String, String, String)> {
    let Ok(client) = client_t(20) else { return Vec::new() };
    let fetch = |p: &Project| -> Option<(String, String, String, String)> {
        let info = if !p.modrinth_id.is_empty() && !mr_token.is_empty() {
            modrinth_project_info_with(&client, mr_token, &p.modrinth_id).ok()
        } else if !p.curse_id.is_empty() && !core_key.is_empty() {
            curse_mod_info_with(&client, core_key, &p.curse_id).ok()
        } else {
            None
        }?;
        Some((p.modrinth_id.clone(), p.curse_id.clone(), info.0, info.1))
    };
    // three workers pull projects from a shared counter: latency overlaps, but the load stays small
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(vec![None; ps.len()]);
    std::thread::scope(|s| {
        for _ in 0..ps.len().min(3) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(p) = ps.get(i) else { break };
                    let r = fetch(p);
                    results.lock().unwrap_or_else(|e| e.into_inner())[i] = r;
                }
            });
        }
    });
    results.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().flatten().collect()
}

/// Числові частини номера ("1.0.10-beta" -> [1,0,10]) для порівняння версій.
fn num_parts(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}

/// М'яке попередження (не блокування): номер уже публікувався або менший за попередній.
pub fn version_warning(number: &str, project: &str, last_number: &str, history: &[Value]) -> Option<String> {
    let n = number.trim();
    if n.is_empty() {
        return None;
    }
    if history.iter().any(|e| e["ok"] == true && e["project"] == project && e["number"] == n) {
        return Some(format!("Версію {n} цього проєкту вже було опубліковано (за історією)."));
    }
    let (a, b) = (num_parts(n), num_parts(last_number));
    (!a.is_empty() && !b.is_empty() && a < b).then(|| format!("Номер {n} менший за попередній {last_number}."))
}

pub fn curse_mod_info(key: &str, id: &str) -> Result<(String, String), String> {
    curse_mod_info_with(&client_get()?, key, id)
}

fn curse_mod_info_with(c: &Client, key: &str, id: &str) -> Result<(String, String), String> {
    let r = c.get(format!("{}/v1/mods/{id}", core())).header("x-api-key", key).send();
    parse_curse_mod(&json_of(r.map_err(net_err)?)?).ok_or_else(|| "немає назви мода".into())
}

pub fn icon_path(url: &str) -> Option<PathBuf> {
    // FNV-1a: стабільний між версіями Rust, на відміну від DefaultHasher
    let h = url.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let base = match std::env::var_os("MODWIRE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(d) => Some(PathBuf::from(d)),
        None => dirs::cache_dir().map(|d| d.join("mod-publisher")),
    };
    base.map(|d| d.join("icons").join(format!("{h:x}.png")))
}

/// Завантажує іконку в дисковий кеш (якщо ще немає). Помилки ігноруються.
pub fn cache_icon(url: &str) {
    let Some(p) = icon_path(url) else { return };
    if !icon_url_ok(url) || p.exists() {
        return;
    }
    let _ = (|| -> Result<(), String> {
        use std::io::Read;
        let r = client_open(30)?.get(url).send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        r.take(ICON_MAX + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > ICON_MAX {
            return Err("іконка завелика".into());
        }
        std::fs::create_dir_all(p.parent().ok_or("немає теки")?).map_err(|e| e.to_string())?;
        write_atomic(&p, &bytes).map_err(|e| e.to_string())
    })();
}

const ICON_MAX: u64 = 2 * 1024 * 1024;

/// Іконку тягнемо лише з https: адреса може прийти з чужого файлу імпорту (не опитуємо локальну мережу по http).
fn icon_url_ok(url: &str) -> bool {
    url.starts_with("https://") && url.len() < 2048
}

/// Читає текстовий файл не більше `max` байт (решта відкидається): величезний файл не з'їдає пам'ять.
pub fn read_text_capped(p: &Path, max: u64) -> Result<String, String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(p).and_then(|f| f.take(max).read_to_end(&mut buf)).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn file_part(file: &Path, up: &Up) -> Result<multipart::Part, String> {
    let f = std::fs::File::open(file).map_err(|_| "Файл зник з диска. Оберіть його знову".to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    up.prog.total.store(len, Relaxed);
    up.prog.sent.store(0, Relaxed);
    up.prog.started_ms.store(now_secs_ms(), Relaxed);
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let mime = if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")) { "application/java-archive" } else { "application/zip" };
    multipart::Part::reader_with_length(Tracked { f, up: up.clone() }, len)
        .file_name(name)
        .mime_str(mime)
        .map_err(|e| e.to_string())
}

fn upload_err(e: reqwest::Error, up: &Up) -> String {
    if up.cancel.load(Relaxed) {
        "Скасовано користувачем. Публікацію не завершено, але перевірте на сайті, чи не з'явилась версія".into()
    } else {
        sent_err(e)
    }
}

/// Додатковий файл версії: без лічильника прогресу (його веде основний файл).
/// Додатковий файл: скасування діє і під час його відправки (прогрес рахується лише для основного).
struct CancelRead {
    f: std::fs::File,
    up: Up,
}

impl std::io::Read for CancelRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.up.cancel.load(Relaxed) {
            return Err(std::io::Error::other("cancelled"));
        }
        self.f.read(buf)
    }
}

fn extra_part(file: &Path, up: &Up) -> Result<multipart::Part, String> {
    let f = std::fs::File::open(file).map_err(|_| "Додатковий файл зник з диска. Оберіть його знову".to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let mime = if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")) { "application/java-archive" } else { "application/zip" };
    multipart::Part::reader_with_length(CancelRead { f, up: up.clone() }, len).file_name(name).mime_str(mime).map_err(|e| e.to_string())
}

fn sha512_file(p: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha512};
    let mut h = Sha512::new();
    std::io::copy(&mut std::fs::File::open(p)?, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Версія Modrinth із таким самим файлом (за sha512). Будь-яка помилка чи 404 = None: перевірка не блокує публікацію.
fn version_by_hash(token: &str, hash: &str) -> Option<Value> {
    let r = client_t(15).ok()?.get(format!("{}/v2/version_file/{hash}?algorithm=sha512", mr())).header("Authorization", token).send().ok()?;
    if r.status().is_success() { r.json().ok() } else { None }
}

fn json_part(s: &str) -> Result<multipart::Part, String> {
    multipart::Part::text(s.to_string()).mime_str("application/json").map_err(|e| e.to_string())
}

/// POST-завантаження. Повторюється один раз ЛИШЕ при помилці з'єднання: запит не дійшов до сервера, дубля бути не може.
fn post_upload(url: &str, header: (&str, &str), mut build: impl FnMut() -> Result<multipart::Form, String>, up: &Up) -> Result<Value, String> {
    let c = client_t(UPLOAD_SECS)?;
    for attempt in 0..2 {
        match c.post(url).header(header.0, header.1).multipart(build()?).send() {
            Err(e) if attempt == 0 && e.is_connect() && !up.cancel.load(Relaxed) => std::thread::sleep(Duration::from_millis(if cfg!(test) { 10 } else { 2000 })),
            // 429: сервер відхилив запит до обробки, файл не прийнято: один повтор після Retry-After (до 30 с)
            Ok(r) if attempt == 0 && r.status().as_u16() == 429 && !up.cancel.load(Relaxed) => {
                let secs = r.headers().get("retry-after").and_then(|h| h.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(2).min(30);
                std::thread::sleep(if cfg!(test) { Duration::from_millis(10) } else { Duration::from_secs(secs) });
            }
            r => return json_of(r.map_err(|e| if e.is_connect() { net_err(e) } else { upload_err(e, up) })?),
        }
    }
    unreachable!()
}

const CANCELLED: &str = "Скасовано користувачем до початку відправки файлу. Нічого не опубліковано";

/// Спільна перевірка перед відправкою: файл на місці, не порожній, не більший за ліміт, не скасовано.
fn preflight(file: &Path, up: &Up) -> Result<(), String> {
    let len = std::fs::metadata(file).ok().filter(|m| m.is_file()).ok_or("Файл зник з диска. Оберіть його знову")?.len();
    if len == 0 {
        return Err("Файл порожній (0 байт)".into());
    }
    if len > MAX_FILE {
        return Err(format!("Файл завеликий ({} МБ): ліміт платформ {} МБ", len >> 20, MAX_FILE >> 20));
    }
    let mut magic = [0u8; 2];
    if std::io::Read::read_exact(&mut std::fs::File::open(file).map_err(|e| e.to_string())?, &mut magic).is_err() || &magic != b"PK" {
        return Err("Файл не схожий на jar/zip-архів (пошкоджений або перейменований)".into());
    }
    if up.cancel.load(Relaxed) {
        return Err(CANCELLED.into());
    }
    Ok(())
}

pub fn publish_modrinth(token: &str, project_id: &str, r: &Release, file: &Path, up: &Up) -> Result<String, String> {
    preflight(file, up)?;
    for e in &r.extra_files {
        preflight(e, up)?;
    }
    let existing = mr_get(&client_get()?, token, &format!("/project/{project_id}/version"))?;
    if existing.as_array().is_some_and(|a| a.iter().any(|v| v["version_number"] == r.number.trim() && v["loaders"].as_array().is_some_and(|l| l.iter().any(|x| r.loaders.iter().any(|y| x == y.as_str()))))) {
        return Err(format!("версія {} для цього лоадера вже є на Modrinth, пропущено (змініть номер, якщо це нова збірка)", r.number.trim()));
    }
    // той самий файл під іншим номером: Modrinth відхилив би його вже після завантаження
    if let Some(v) = sha512_file(file).ok().and_then(|h| version_by_hash(token, &h)).filter(|v| v["project_id"] == project_id) {
        return Err(format!("цей файл уже опубліковано на Modrinth як версію {} (id {}), пропущено", v["version_number"].as_str().unwrap_or("?"), v["id"].as_str().unwrap_or("?")));
    }
    let mut r = r.clone();
    r.dep_ids.clear();
    let mut missing = vec![];
    for slug in r.dep_slugs.clone().iter().take(MAX_DEPS_SEND) {
        if up.cancel.load(Relaxed) {
            return Err(CANCELLED.into());
        }
        match modrinth_lookup(token, slug) {
            Ok((id, ..)) => {
                if id != project_id && !r.dep_ids.contains(&id) {
                    r.dep_ids.push(id);
                }
            }
            Err(_) => missing.push(slug.as_str().to_string()),
        }
    }
    let data = modrinth_data(&r, project_id).to_string();
    let build = || {
        let mut form = multipart::Form::new().part("data", json_part(&data)?).part("file", file_part(file, up)?);
        for (i, e) in r.extra_files.iter().enumerate() {
            form = form.part(format!("file{}", i + 1), extra_part(e, up)?);
        }
        Ok(form)
    };
    let v = post_upload(&format!("{}/v2/version", mr()), ("Authorization", token), build, up)?;
    let note = if missing.is_empty() { String::new() } else { format!(" (залежності не знайдено на Modrinth і не додані: {})", missing.join(", ")) };
    match v["id"].as_str() {
        Some(id) => Ok(format!("версія {id}{note} https://modrinth.com/project/{project_id}/version/{id}")),
        None => Ok("версію опубліковано (сервер не повернув id)".into()),
    }
}

/// Чи є серед файлів CurseForge (відповідь Core API) файл із такою displayName.
pub fn curse_has_name(files: &Value, name: &str) -> bool {
    files["data"].as_array().is_some_and(|a| a.iter().any(|f| f["displayName"] == name))
}

pub fn publish_curse(token: &str, core_key: &str, project_id: &str, r: &Release, file: &Path, up: &Up) -> Result<String, String> {
    preflight(file, up)?;
    let c = client()?;
    let mut unchecked = true; // без чинного Core-ключа дубль не перевіряється
    if !core_key.is_empty() {
        // shortcut: до 10 сторінок по 50 файлів (500 останніх); помилка перевірки не блокує публікацію
        for page in 0..10u32 {
            let url = format!("{}/v1/mods/{project_id}/files?pageSize=50&index={}", core(), page * 50);
            match get_json(&c, &url, ("x-api-key", core_key)) {
                Ok(files) => {
                    if curse_has_name(&files, display_name(r)) {
                        return Err(format!("файл «{}» уже є на CurseForge, пропущено (змініть номер або назву)", display_name(r)));
                    }
                    unchecked = false;
                    if files["data"].as_array().map_or(0, |a| a.len()) < 50 {
                        break;
                    }
                }
                Err(_) => {
                    unchecked = true;
                    break;
                }
            }
        }
    }
    let get = |p: &str| get_json(&c, &format!("{}/api/game/{p}", cf()), ("X-Api-Token", token));
    let ids = resolve_curse_ids(&get("version-types")?, &get("versions")?, &r.mc_versions, &r.loaders)?;
    if up.cancel.load(Relaxed) {
        return Err(CANCELLED.into());
    }
    let meta = curse_metadata(r, &ids).to_string();
    let v = post_upload(&format!("{}/api/projects/{project_id}/upload-file", cf()), ("X-Api-Token", token), || Ok(multipart::Form::new().part("metadata", json_part(&meta)?).part("file", file_part(file, up)?)), up)?;
    Ok(format!(
        "файл {}{} https://www.curseforge.com/projects/{project_id}",
        v["id"].as_u64().map_or("(id не повернуто)".to_string(), |i| i.to_string()),
        if unchecked { " (дубль не перевірено: потрібен чинний Core API-ключ)" } else { "" }
    ))
}

// ---------- автооновлення ----------

/// GitHub-репозиторій із релізами (власник/назва). Плейсхолдер "OWNER/REPO" вимикає перевірку оновлень.
/// Тег релізу має бути "vX.Y.Z" і збігатися з версією в Cargo.toml.
pub const UPDATE_REPO: &str = "Waffylka/ModWire";
/// Ім'я файлу в релізі, який завантажує автооновлення (його кладе release.yml для Windows).
const UPDATE_ASSET: &str = "modwire.exe";
const UPDATE_MAX: u64 = 200 * 1024 * 1024;

#[derive(Debug)]
pub struct Update {
    pub version: String,
    /// Пряме посилання на файл для Windows; порожнє, якщо в релізі його немає (тоді відкриваємо сторінку).
    pub url: String,
    /// Розмір файлу з метаданих релізу (0 = невідомий); завантажений файл має збігатися.
    pub size: u64,
    /// SHA-256 файлу з метаданих релізу (hex, нижній регістр); порожній, якщо GitHub його не повідомив.
    pub sha256: String,
    pub page: String,
}

fn ver_parts(s: &str) -> Option<[u64; 3]> {
    let core = s.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut out = [0u64; 3];
    let mut n = 0;
    for part in core.split('.') {
        *out.get_mut(n)? = part.parse().ok()?;
        n += 1;
    }
    (n > 0).then_some(out)
}

/// true, якщо `tag` строго новіший за `current` (обидва у вигляді "1.2.3" або "v1.2.3").
pub fn is_newer(tag: &str, current: &str) -> bool {
    matches!((ver_parts(tag), ver_parts(current)), (Some(a), Some(b)) if a > b)
}

/// Адреса файлу з релізу саме цього репозиторію: https, github.com, без логіна, шлях
/// /власник/назва/releases/download/<тег>/<файл>. Розбір через Url прибирає "." і ".." до порівняння.
pub fn trusted_url(url: &str, repo: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    let Some((owner, name)) = repo.split_once('/') else { return false };
    let segs: Vec<&str> = u.path_segments().map_or_else(Vec::new, |s| s.collect());
    u.scheme() == "https"
        && u.host_str() == Some("github.com")
        && u.port().is_none()
        && u.username().is_empty()
        && segs.len() == 6
        && segs[0].eq_ignore_ascii_case(owner)
        && segs[1].eq_ignore_ascii_case(name)
        && segs[2] == "releases"
        && segs[3] == "download"
        && segs[4..].iter().all(|x| !x.is_empty() && *x != "." && *x != "..")
}

/// Розбирає відповідь GitHub `releases/latest`. None: реліз не новіший або без валідного тега.
pub fn parse_release(v: &Value, repo: &str, current: &str) -> Option<Update> {
    if v["draft"].as_bool().unwrap_or(false) || v["prerelease"].as_bool().unwrap_or(false) {
        return None;
    }
    let tag = v["tag_name"].as_str()?;
    // теги з суфіксом (v1.0.0-rc1) вважаємо пробними й не пропонуємо
    if tag.contains('-') || !is_newer(tag, current) {
        return None;
    }
    let asset = v["assets"].as_array().and_then(|a| {
        a.iter().find(|x| x["name"].as_str() == Some(UPDATE_ASSET) && x["browser_download_url"].as_str().is_some_and(|u| trusted_url(u, repo)))
    });
    let url = asset.and_then(|x| x["browser_download_url"].as_str());
    let size = asset.and_then(|x| x["size"].as_u64()).unwrap_or(0);
    let sha256 = asset.and_then(|x| x["digest"].as_str()).and_then(|d| d.strip_prefix("sha256:")).filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).map_or(String::new(), str::to_lowercase);
    let page = v["html_url"].as_str().filter(|u| u.starts_with("https://github.com/")).map_or_else(|| format!("https://github.com/{repo}/releases/latest"), str::to_string);
    Some(Update { version: tag.trim_start_matches('v').to_string(), url: url.unwrap_or("").to_string(), size, sha256, page })
}

/// Запитує останній реліз. Ok(None): оновлень немає або репозиторій ще не налаштовано (без мережевого запиту).
pub fn check_update() -> Result<Option<Update>, String> {
    check_repo(UPDATE_REPO)
}

fn check_repo(repo: &str) -> Result<Option<Update>, String> {
    if repo.contains("OWNER") {
        return Ok(None);
    }
    let c = client_open(20)?;
    let v = match get_json(&c, &format!("{}/repos/{repo}/releases/latest", base("https://api.github.com")), ("accept", "application/vnd.github+json")) {
        Ok(v) => v,
        // GitHub віддає 404, поки в репозиторії немає жодного релізу: оновлень просто немає
        Err(e) if e.contains("(HTTP 404") => return Ok(None),
        Err(e) if e.contains("(HTTP 403") || e.contains("(HTTP 429") => {
            return Err("GitHub тимчасово обмежив кількість запитів із вашої адреси. Спробуйте за годину або завантажте нову версію зі сторінки релізів".into());
        }
        Err(e) => return Err(e),
    };
    Ok(parse_release(&v, repo, env!("CARGO_PKG_VERSION")))
}

/// Видаляє залишок попереднього оновлення (modwire.exe.old). Помилку ігноруємо: файл міг ще тримати старий процес.
pub fn cleanup_update() {
    if let Ok(exe) = std::env::current_exe() {
        cleanup_at(&exe);
    }
}

fn cleanup_at(exe: &Path) {
    let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    let _ = std::fs::remove_file(exe.with_extension("exe.new"));
    let _ = std::fs::remove_file(exe.with_extension("exe.bad"));
}

/// Що робити зі слідами оновлення на старті.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StartAction {
    /// Нічого не очікуємо: прибрати .old/.new.
    Clean,
    /// Щойно оновлені (`--updated`): .old лишається, доки UI не пропрацює STABLE_SECS; тоді `confirm_update`.
    AwaitStable,
    /// Є .old, але попередній запуск після оновлення не дійшов до маркера: запропонувати повернути стару версію.
    OfferRollback,
}

/// Скільки секунд живого UI вважаємо доказом, що нова збірка стартує стабільно.
pub const STABLE_SECS: u64 = 10;

pub fn start_action(updated: bool, old_exists: bool, marker: bool) -> StartAction {
    match (updated, old_exists, marker) {
        (true, ..) => StartAction::AwaitStable,
        (false, true, false) => StartAction::OfferRollback,
        _ => StartAction::Clean,
    }
}

fn marker_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("update.ok"))
}

/// Дія для цього запуску за станом файлів поруч із exe.
pub fn current_start_action(updated: bool) -> StartAction {
    let old = std::env::current_exe().is_ok_and(|e| e.with_extension("exe.old").exists());
    start_action(updated, old, marker_path().is_some_and(|m| m.exists()))
}

/// Нова збірка пропрацювала стабільно: маркер `update.ok` і прибирання .old.
pub fn confirm_update() {
    if let Ok(exe) = std::env::current_exe() {
        confirm_update_at(&exe);
    }
}

fn confirm_update_at(exe: &Path) {
    if let Some(m) = marker_path() {
        if let Some(d) = m.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(m, b"ok");
    }
    cleanup_at(exe);
}

/// Перед новою заміною старий маркер не має приховувати проблему.
fn clear_update_marker() {
    if let Some(m) = marker_path() {
        let _ = std::fs::remove_file(m);
    }
}

/// Відкат із працюючої (нестабільної) збірки: exe -> .bad, .old -> exe. Windows дозволяє перейменувати запущений файл.
/// Після Ok треба запустити exe знову й вийти. `.bad` прибере наступний запуск.
pub fn rollback_running_at(exe: &Path) -> Result<(), String> {
    let (old, bad) = (exe.with_extension("exe.old"), exe.with_extension("exe.bad"));
    if !old.exists() {
        return Err("Немає копії попередньої версії для відкату".into());
    }
    let _ = std::fs::remove_file(&bad);
    std::fs::rename(exe, &bad).map_err(|e| format!("Не вдалося відкласти поточну версію: {e}"))?;
    if let Err(e) = std::fs::rename(&old, exe) {
        let _ = std::fs::rename(&bad, exe);
        return Err(format!("Не вдалося повернути попередню версію: {e}"));
    }
    clear_update_marker();
    Ok(())
}

pub fn rollback_running() -> Result<(), String> {
    rollback_running_at(&std::env::current_exe().map_err(|e| e.to_string())?)
}

/// Запускає exe знову (після відкату); `--updated` змушує нову копію дочекатися звільнення блокування.
pub fn relaunch() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).arg("--updated").spawn().map(|_| ()).map_err(|e| format!("Не вдалося перезапустити: {e}. Запустіть програму вручну."))
}

/// Перевіряє завантажений файл: розмір і сигнатура виконуваного файлу Windows («MZ»).
fn valid_exe(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let len = f.metadata().map_or(0, |m| m.len());
    let mut head = [0u8; 2];
    (1_000_000..=UPDATE_MAX).contains(&len) && f.read_exact(&mut head).is_ok() && &head == b"MZ"
}

/// SHA-256 файлу (hex).
fn sha256_file(p: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    std::io::copy(&mut std::fs::File::open(p)?, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Перевіряє, що в теці exe можна створювати файли (інакше заміна не вдасться, напр. Program Files).
fn check_writable(dir: &Path) -> Result<(), String> {
    let t = dir.join(format!(".modwire_write_test{}", std::process::id()));
    std::fs::File::create(&t).map_err(|e| format!("Немає прав на запис у теку програми ({}): {e}. Перемістіть modwire.exe в теку, де можна писати (напр. Документи), або запустіть від імені адміністратора.", dir.display()))?;
    let _ = std::fs::remove_file(t);
    Ok(())
}

/// Відкат: повертає modwire.exe.old на місце exe (нова збірка не стартувала).
fn rollback_at(exe: &Path) -> Result<(), String> {
    let old = exe.with_extension("exe.old");
    if !old.exists() {
        return Err("Немає копії попередньої версії для відкату".into());
    }
    let _ = std::fs::remove_file(exe);
    std::fs::rename(&old, exe).map_err(|e| format!("Не вдалося повернути попередню версію: {e}. Перейменуйте {} у modwire.exe", old.display()))
}

/// Запускає щойно встановлену копію і чекає кілька секунд: якщо вона одразу аварійно завершилась,
/// повертає стару версію й повідомляє про це. Ok = нова копія працює, можна виходити.
pub fn restart_checked() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(&exe).arg("--updated").spawn().map_err(|e| format!("Оновлено, але не вдалося перезапустити: {e}. Запустіть програму вручну."))?;
    for _ in 0..16 {
        std::thread::sleep(Duration::from_millis(250));
        if let Ok(Some(st)) = child.try_wait() {
            if st.success() {
                return Ok(());
            }
            return Err(match rollback_at(&exe) {
                Ok(()) => "Нова версія не запустилась; повернуто попередню. Спробуйте пізніше або завантажте оновлення вручну.".into(),
                Err(e) => e,
            });
        }
    }
    Ok(())
}

static UPD_TOTAL: AtomicU64 = AtomicU64::new(0);
static UPD_DONE: AtomicU64 = AtomicU64::new(0);

/// (завантажено, усього) байтів оновлення; усього = 0, якщо розмір невідомий. Для смуги прогресу в UI.
pub fn update_progress() -> (u64, u64) {
    (UPD_DONE.load(Relaxed), UPD_TOTAL.load(Relaxed))
}

/// Лише для режиму знімків: підставити прогрес.
pub fn set_update_progress(done: u64, total: u64) {
    UPD_DONE.store(done, Relaxed);
    UPD_TOTAL.store(total, Relaxed);
}

fn download_to(url: &str, dest: &Path, size: u64) -> Result<(), String> {
    use std::io::{Read, Write};
    let c = client_open(900)?;
    let resp = c.get(url).send().map_err(net_err)?;
    if !resp.status().is_success() {
        return Err(format!("Сервер відповів {}", resp.status()));
    }
    UPD_TOTAL.store(if size > 0 { size } else { resp.content_length().unwrap_or(0) }, Relaxed);
    UPD_DONE.store(0, Relaxed);
    let res = (|| {
        let mut out = std::fs::File::create(dest).map_err(|e| format!("Не вдалося записати файл оновлення поруч із програмою: {e}"))?;
        let (mut src, mut buf, mut n) = (resp.take(UPDATE_MAX + 1), vec![0u8; 64 * 1024], 0u64);
        loop {
            // обрив посеред завантаження: та сама порада про мережу, що й для запиту
            let k = src.read(&mut buf).map_err(|e| format!("{} ({e})", net_advice()))?;
            if k == 0 {
                break;
            }
            out.write_all(&buf[..k]).map_err(|e| e.to_string())?;
            n += k as u64;
            UPD_DONE.store(n, Relaxed);
        }
        out.flush().map_err(|e| e.to_string())?;
        if n > UPDATE_MAX {
            return Err("Файл оновлення завеликий".to_string());
        }
        if size > 0 && n != size {
            return Err(format!("Файл завантажено не повністю ({n} з {size} байт)"));
        }
        Ok(())
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    res
}

/// Завантажує нову версію й підміняє поточний exe (Windows дозволяє перейменувати працюючий файл).
/// Після Ok треба запустити exe знову і вийти. Лише адреси з релізів нашого репозиторію.
pub fn apply_update(url: &str, size: u64, sha256: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Автооновлення працює лише у Windows: завантажте нову версію зі сторінки релізу".into());
    }
    if !trusted_url(url, UPDATE_REPO) {
        return Err("Недовірена адреса оновлення".into());
    }
    apply_at(&std::env::current_exe().map_err(|e| e.to_string())?, url, size, sha256)
}

/// Те саме для довільного `exe` (у тестах це тимчасова копія, а не працюючий файл); адресу тут уже не перевіряємо.
fn apply_at(exe: &Path, url: &str, size: u64, sha256: &str) -> Result<(), String> {
    let (new, old) = (exe.with_extension("exe.new"), exe.with_extension("exe.old"));
    check_writable(exe.parent().ok_or("немає теки програми")?)?;
    if UPDATING.swap(true, Relaxed) {
        return Err("Оновлення вже виконується".into());
    }
    clear_update_marker();
    let res = download_to(url, &new, size).and_then(|()| install(exe, &new, &old, sha256));
    UPDATING.store(false, Relaxed);
    res
}

static UPDATING: AtomicBool = AtomicBool::new(false);

/// Перевіряє завантажений `new` (сума, сигнатура) і підміняє ним `exe`; стара копія лишається як `old`.
fn install(exe: &Path, new: &Path, old: &Path, sha256: &str) -> Result<(), String> {
    // файл може тримати антивірус: це не «пошкоджений» і не «не збігається сума»
    if let Err(e) = std::fs::File::open(new) {
        return Err(format!("Не вдалося прочитати завантажений файл ({e}): його, імовірно, перевіряє антивірус. Зачекайте хвилину й повторіть"));
    }
    if !sha256.is_empty() && sha256_file(new).map_or(true, |h| !h.eq_ignore_ascii_case(sha256)) {
        let _ = std::fs::remove_file(new);
        return Err("Контрольна сума завантаженого файлу не збігається з релізом; оновлення скасовано".into());
    }
    if !valid_exe(new) {
        let _ = std::fs::remove_file(new);
        return Err("Завантажений файл пошкоджений або не є програмою Windows".into());
    }
    let _ = std::fs::remove_file(old);
    std::fs::rename(exe, old).map_err(|e| format!("Не вдалося замінити програму: {e}"))?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::remove_file(new);
        if std::fs::rename(old, exe).is_err() {
            return Err(format!("Не вдалося замінити програму: {e}. Якщо modwire.exe зник, перейменуйте {} назад у modwire.exe", old.display()));
        }
        return Err(format!("Не вдалося замінити програму: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn curse_search_parsing() {
        let v = json!({"data": [
            {"id": 11, "name": "Alpha", "logo": {"thumbnailUrl": "https://x/a.png"}},
            {"id": 22, "name": "Beta"},
            {"name": "NoId"},
            {"id": 33}
        ]});
        let h = parse_curse_search(&v);
        assert_eq!(h.len(), 2, "{h:?}");
        assert_eq!((h[0].id.as_str(), h[0].name.as_str(), h[0].icon_url.as_str()), ("11", "Alpha", "https://x/a.png"));
        assert_eq!((h[1].id.as_str(), h[1].icon_url.as_str()), ("22", ""));
        assert!(parse_curse_search(&json!({})).is_empty());
        assert_eq!(curse_search("k", "  ").unwrap(), vec![]);
    }

    use super::*;

    fn rel() -> Release {
        Release {
            name: "".into(),
            number: "1.0.0".into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec!["fabric".into()],
            release_type: "release".into(),
            changelog: "ok".into(),
            ..Default::default()
        }
    }

    #[test]
    fn curse_ids_by_type() {
        let types = json!([{"id":1,"name":"Minecraft 1.20"},{"id":2,"name":"Java 17"},{"id":68441,"name":"Modloader"}]);
        let vers = json!([
            {"id":10,"gameVersionTypeID":2,"name":"1.20.1"},
            {"id":11,"gameVersionTypeID":1,"name":"1.20.1"},
            {"id":12,"gameVersionTypeID":68441,"name":"Fabric"},
            {"id":13,"gameVersionTypeID":68441,"name":"NeoForge"}
        ]);
        let ids = resolve_curse_ids(&types, &vers, &["1.20.1".into()], &["fabric".into(), "neoforge".into()]).unwrap();
        assert_eq!(ids, vec![11, 12, 13]);
        let err = resolve_curse_ids(&types, &vers, &["1.99".into()], &["quilt".into()]).unwrap_err();
        assert!(err.contains("1.99") && err.contains("quilt"));
    }

    #[test]
    fn read_jar_is_bounded_and_ignores_paths() {
        use std::io::Write;
        let p = std::env::temp_dir().join(format!("mp_bomb_{}.jar", std::process::id()));
        let mut w = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file("../../evil.txt", o).unwrap(); // zip-slip: імена лише порівнюються, на диск нічого не пишеться
        w.write_all(b"x").unwrap();
        w.start_file("fabric.mod.json", o).unwrap(); // 20 МБ нулів, стиснуті в кілька КБ
        w.write_all(&vec![0u8; 20_000_000]).unwrap();
        w.finish().unwrap();
        let t = std::time::Instant::now();
        let info = read_jar(&p).expect("zip читається");
        assert!(t.elapsed() < Duration::from_secs(10));
        assert!(info.version.is_none());
        assert!(!p.parent().unwrap().parent().unwrap().join("evil.txt").exists());
        let _ = std::fs::remove_file(&p);
        std::fs::write(&p, b"not a zip").unwrap();
        assert!(read_jar(&p).is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn edge_cases_no_panic() {
        assert_eq!(bump("v9é"), "v10é");
        assert_eq!(bump("é1"), "é2");
        assert_eq!(bump("99999999999999999999"), "");
        assert_eq!(bump("18446744073709551615"), "");
        assert_eq!(bump(""), "");
        assert_eq!(bump("1.0+x"), "1.1+x");
        assert_eq!(bump("+b"), "");
        assert_eq!(parse_mc(" 1.20.1,,1.20.1
1.21 "), vec!["1.20.1", "1.21"]);
        assert!(parse_mc("").is_empty());
        assert_eq!(first_section(""), "");
        assert_eq!(first_section("## 
## a
текст"), "текст");
        assert_eq!(ymd(0), (1970, 1, 1));
        assert_eq!(ymd(951_782_400), (2000, 2, 29));
    }

    #[test]
    fn clearing_tokens_removes_token_copies() {
        let d = std::env::temp_dir().join(format!("mp_clear_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("config.json");
        let mut c = Config { modrinth_token: "secret-token".into(), ..Default::default() };
        save_to(&p, &c).unwrap();
        c.modrinth_token.clear();
        c.projects.push(Project { name: "A".into(), ..Default::default() });
        // друге збереження з токеном створило б копію; після очищення копії з токенами зникають
        std::fs::write(backup_path(&p), "{\"modrinth_token\":\"secret-token\"}").unwrap();
        std::fs::write(d.join(format!("config.pre-v{SCHEMA}.json")), "{\"modrinth_token\":\"secret-token\"}").unwrap();
        save_to(&p, &c).unwrap();
        assert!(!backup_path(&p).exists());
        assert!(!d.join(format!("config.pre-v{SCHEMA}.json")).exists());
        assert!(!std::fs::read_to_string(&p).unwrap().contains("secret-token"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn load_edge_cases() {
        let d = std::env::temp_dir().join(format!("mp_cfg_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("config.json");
        assert!(matches!(load_from(&p).1, Load::Fine)); // немає файлу
        std::fs::write(&p, "").unwrap(); // порожній файл = пошкоджений, не паніка
        assert!(matches!(load_from(&p).1, Load::Recovered));
        std::fs::write(&p, "{\"modrinth_token\":\" t \",\"future\":{\"a\":1},\"projects\":[{\"name\":\"X\",\"zzz\":5}]}").unwrap();
        let (c, st) = load_from(&p);
        assert!(matches!(st, Load::Fine));
        assert_eq!(c.modrinth_token, "t");
        assert!(c.extra.contains_key("future") && c.projects[0].extra.contains_key("zzz"));
        save_to(&p, &c).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("future") && s.contains("zzz"));
        assert!(!export_projects(&c.projects).contains("\"t\""), "токен не в експорті");
        std::fs::write(&p, "{broken").unwrap(); // є резервна копія з попереднього збереження
        let (c2, st) = load_from(&p);
        assert!(matches!(st, Load::Restored) || matches!(st, Load::Recovered));
        let _ = c2;
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn project_links_parse() {
        use Link::*;
        assert_eq!(parse_project_link("https://modrinth.com/mod/sodium"), Some(Modrinth("sodium".into())));
        assert_eq!(parse_project_link(" modrinth.com/modpack/Fabulously-Optimized/versions?x=1#a "), Some(Modrinth("Fabulously-Optimized".into())));
        assert_eq!(parse_project_link("https://www.modrinth.com/plugin/x_y.z"), Some(Modrinth("x_y.z".into())));
        assert_eq!(parse_project_link("https://www.curseforge.com/minecraft/mc-mods/jei/files"), Some(CurseSlug("jei".into())));
        assert_eq!(parse_project_link("https://www.curseforge.com/projects/238222"), Some(CurseId("238222".into())));
        for bad in ["sodium", "123456", "https://modrinth.com.evil.org/mod/x", "https://evil.org/modrinth.com/mod/x", "https://modrinth.com/mod/", "https://modrinth.com/user/x", "https://curseforge.com/projects/abc", "https://modrinth.com/mod/a b"] {
            assert_eq!(parse_project_link(bad), None, "{bad}");
        }
    }

    #[test]
    fn lookup_by_slug_with_mocks() {
        let (r, seen) = with_mock(1, vec![("GET /v2/project/sodium", 200, r#"{"id":"AANobbMI","title":"Sodium","icon_url":"https://i/x.png"}"#)], || modrinth_lookup("", "sodium"));
        assert_eq!(r.unwrap(), ("AANobbMI".into(), "Sodium".into(), "https://i/x.png".into()));
        assert!(seen[0].starts_with("GET /v2/project/sodium"));
        let (r, _) = with_mock(1, vec![("GET /v2/project/nope", 404, "{}")], || modrinth_lookup("t", "nope"));
        assert!(r.unwrap_err().contains("не знайдено"));
        let (r, seen) = with_mock(1, vec![("GET /v1/mods/search?gameId=432&slug=jei", 200, r#"{"data":[{"id":238222,"name":"JEI","logo":{"thumbnailUrl":"https://i/j.png"}}]}"#)], || curse_lookup_slug("k", "jei"));
        assert_eq!(r.unwrap(), ("238222".into(), "JEI".into(), "https://i/j.png".into()));
        assert!(seen[0].to_lowercase().contains("x-api-key: k"));
        let (r, _) = with_mock(1, vec![("GET /v1/mods/search", 200, r#"{"data":[]}"#)], || curse_lookup_slug("k", "zzz"));
        assert!(r.unwrap_err().contains("не знайдено"));
    }

    #[test]
    fn project_filter_duplicate() {
        let mk = |n: &str, m: &str, c: &str| Project { name: n.into(), modrinth_id: m.into(), curse_id: c.into(), ..Default::default() };
        let mut cfg = Config { projects: vec![mk("Alpha Mod", "AbC123", ""), mk("Beta", "", "777")], ..Default::default() };
        assert_eq!(filter_projects(&cfg.projects, ""), vec![0, 1]);
        assert_eq!(filter_projects(&cfg.projects, " alpha"), vec![0]);
        assert_eq!(filter_projects(&cfg.projects, "abc1"), vec![0]);
        assert_eq!(filter_projects(&cfg.projects, "77"), vec![1]);
        assert!(filter_projects(&cfg.projects, "zzz").is_empty());
        ensure_ids(&mut cfg);
        cfg.projects[0].pinned = true;
        let a = duplicate_project(&mut cfg, 0).unwrap();
        let b = duplicate_project(&mut cfg, 0).unwrap();
        assert_eq!((cfg.projects[a].name.as_str(), cfg.projects[b].name.as_str()), ("Alpha Mod (копія)", "Alpha Mod (копія 2)"));
        assert!(!cfg.projects[a].pinned && cfg.projects[a].modrinth_id == "AbC123");
        let ids: std::collections::HashSet<_> = cfg.projects.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids.len(), cfg.projects.len());
        assert!(duplicate_project(&mut cfg, 99).is_none());
    }

    #[test]
    fn jar_dependencies_detected() {
        let d = tmp_dir("deps");
        let f = d.join("d.jar");
        zip_with(&f, &[("fabric.mod.json", r#"{"version":"1","depends":{"minecraft":"1.20.1","fabricloader":">=0.14","java":">=17","fabric-api":"*","cloth-config":"*"}}"#)]);
        let mut deps = read_jar(&f).unwrap().deps;
        deps.sort();
        assert_eq!(deps, vec!["cloth-config", "fabric-api"]);
        let toml = "[[mods]]\nversion=\"1\"\n[[dependencies.m]]\nmodId=\"minecraft\"\nmandatory=true\n[[dependencies.m]]\nmodId=\"geckolib\"\nmandatory=true\n[[dependencies.m]]\nmodId=\"jei\"\nmandatory=false\n[[dependencies.m]]\nmodId=\"curios\"\ntype=\"required\"\n";
        assert_eq!(toml_deps(toml), vec!["geckolib", "curios"]);
    }

    #[test]
    fn project_name_template_and_draft_skip() {
        let p = Project { name: "M".into(), ..Default::default() };
        let j = serde_json::to_string(&p).unwrap();
        assert!(!j.contains("name_template") && !j.contains("\"draft\""), "{j}");
        let q: Project = serde_json::from_str(r#"{"name":"M","name_template":"{project} {version}"}"#).unwrap();
        let r = Release { number: "1.2".into(), ..Default::default() };
        assert_eq!(render_template(&q.name_template, &q.name, &r), "M 1.2");
        let d = Project { draft: Draft { number: "9".into(), ..Default::default() }, ..Default::default() };
        assert!(strip_secrets(&d).draft.is_empty());
    }

    #[test]
    fn release_type_from_number() {
        assert_eq!(parse_mc("1.20.1;1.20.2, 1.20.1"), vec!["1.20.1", "1.20.2"]);
        assert_eq!(detect_release_type("1.2.0-beta.1"), Some("beta"));
        assert_eq!(detect_release_type("1.2.0+RC2"), Some("beta"));
        assert_eq!(detect_release_type("2.0-alpha3"), Some("alpha"));
        assert_eq!(detect_release_type("1.0.0-SNAPSHOT"), Some("alpha"));
        assert_eq!(detect_release_type("1.0.0-beta-dev"), Some("alpha"));
        assert_eq!(detect_release_type("1.2.0"), None);
        assert_eq!(detect_release_type("1.20.1-1.2.0"), None);
        assert_eq!(detect_release_type("preview"), None); // перша частина — не суфікс
    }

    #[test]
    fn draft_rules_and_roundtrip() {
        let p = Project { id: "p1".into(), last_number: "1.0.0".into(), ..Default::default() };
        let d = Draft { project: "p1".into(), number: "1.0.1".into(), changelog: "fix".into(), ..Default::default() };
        assert!(d.applies_to(&p));
        assert!(!Draft { project: "p2".into(), ..d.clone() }.applies_to(&p)); // інший проєкт
        assert!(!Draft { number: "1.0.0".into(), ..d.clone() }.applies_to(&p)); // уже опубліковано
        assert!(!Draft { project: "p1".into(), ..Default::default() }.applies_to(&p)); // порожня
        // старий конфіг без draft читається, нова чернетка переживає запис/читання
        let old: Config = serde_json::from_str(r#"{"last":{"dir":"x"}}"#).unwrap();
        assert_eq!(old.last.draft, Draft::default());
        let path = tmp_dir("draft").join("config.json");
        let mut cfg = Config::default();
        cfg.last.draft = d.clone();
        save_to(&path, &cfg).unwrap();
        assert_eq!(load_from(&path).0.last.draft, d);
    }

    #[test]
    fn bump_versions() {
        assert_eq!(bump("1.0.9"), "1.0.10");
        assert_eq!(bump("v2"), "v3");
        assert_eq!(bump("1.2.3-beta"), "1.2.4-beta");
        assert_eq!(bump("beta"), "");
        assert_eq!(bump("0.5.0+1.20.1"), "0.5.1+1.20.1");
    }

    #[test]
    fn toml_versions() {
        assert_eq!(toml_version("modLoader=\"x\"\n[[mods]]\nmodId=\"a\"\nversion=\"1.2.3\"\nversionRange=\"1\"\n"), Some("1.2.3".into()));
        assert_eq!(toml_version("[[mods]]\nversion=\"${file.jarVersion}\"\n"), None);
        assert_eq!(toml_version("[[dependencies.a]]\nversion=\"9\"\n"), None);
    }

    #[test]
    fn game_version_tags() {
        let v = json!([{"version":"1.21","version_type":"release"},{"version":"24w01a","version_type":"snapshot"},{"version":"1.20.1","version_type":"release"}]);
        let all = parse_game_versions(&v);
        assert_eq!(all.len(), 3);
        assert_eq!(all.iter().filter(|g| g.1).map(|g| g.0.as_str()).collect::<Vec<_>>(), vec!["1.21", "1.20.1"]);
    }

    #[test]
    fn single_instance() {
        let p = std::env::temp_dir().join(format!("mp_lock_test_{}", std::process::id()));
        let first = lock_at(&p).unwrap();
        assert!(first.is_some());
        assert!(lock_at(&p).unwrap().is_none(), "друга копія має бути заблокована");
        drop(first);
        assert!(lock_at(&p).unwrap().is_some());
    }

    #[test]
    fn curse_dup_name() {
        let f = json!({"data":[{"displayName":"1.0.0"},{"displayName":"0.9"}]});
        assert!(curse_has_name(&f, "1.0.0"));
        assert!(!curse_has_name(&f, "1.0.1"));
    }

    #[test]
    fn dates() {
        assert_eq!(ymd(0), (1970, 1, 1));
        assert_eq!(ymd(1_700_000_000), (2023, 11, 14));
    }

    #[test]
    fn changelog_sections() {
        assert_eq!(first_section("# Changelog\n\n## 1.1\n- a\n- b\n\n## 1.0\n- c"), "- a\n- b");
        assert_eq!(first_section("### only\nx\n### next\ny"), "x");
        // Keep a Changelog: порожня [Unreleased] пропускається
        assert_eq!(first_section("# Changelog\n\n## [Unreleased]\n\n## [1.0.0]\n- a"), "- a");
        // підзаголовки глибшого рівня лишаються в секції
        assert_eq!(first_section("# 1.2\n### Fixed\n- x\n### Added\n- y\n# 1.1\n- z"), "### Fixed\n- x\n### Added\n- y");
        // # у блоці коду не є заголовком
        assert_eq!(first_section("## 1.0\n```\n# comment\n```\n- a\n## 0.9\n- b"), "```\n# comment\n```\n- a");
        assert_eq!(first_section("  просто текст \n"), "просто текст");
    }

    // ----- mock-сервер: на кожне з'єднання одна відповідь за маршрутом "МЕТОД /шлях" -----
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn mock_secs(n: usize, routes: Vec<(&'static str, u16, &'static str)>, secs: u64) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        l.set_nonblocking(true).unwrap();
        let h = std::thread::spawn(move || {
            let mut seen = vec![];
            // не більше n з'єднань і не довше 5 с: якщо клієнт відкрив менше, тест падає, а не висить
            let deadline = std::time::Instant::now() + Duration::from_secs(secs);
            while seen.len() < n && std::time::Instant::now() < deadline {
                let Ok((mut s, _)) = l.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                s.set_nonblocking(false).ok();
                s.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let (mut buf, mut tmp) = (vec![], [0u8; 8192]);
                let head_end = loop {
                    match s.read(&mut tmp) {
                        Ok(0) | Err(_) => break buf.len(),
                        Ok(k) => {
                            buf.extend_from_slice(&tmp[..k]);
                            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break i + 4;
                            }
                        }
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                let mut got = buf.len() - head_end;
                // тіло без Content-Length (chunked, великі файли) читаємо до завершального чанка; у `seen` зберігаємо лише початок
                let chunked = head.to_lowercase().contains("transfer-encoding: chunked");
                let mut tail: Vec<u8> = buf[head_end.min(buf.len())..].iter().rev().take(5).rev().copied().collect();
                while got < len || (chunked && !tail.ends_with(b"0\r\n\r\n")) {
                    match s.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => {
                            got += k;
                            if buf.len() < head_end + 262_144 {
                                buf.extend_from_slice(&tmp[..k]);
                            }
                            tail.extend_from_slice(&tmp[..k]);
                            tail.drain(..tail.len().saturating_sub(5));
                        }
                    }
                }
                let body_text = String::from_utf8_lossy(&buf[head_end.min(buf.len())..]).into_owned();
                let line = head.lines().next().unwrap_or("").to_string();
                let hit = routes.iter().find(|(k, _, _)| line.starts_with(k));
                let (code, body) = hit.map_or((404, "{}"), |r| (r.1, r.2));
                let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                seen.push(format!("{head}{body_text}"));
            }
            seen
        });
        (url, h)
    }

    /// Скидає TEST_BASE навіть якщо тест панікує всередині f.
    struct BaseGuard;
    impl Drop for BaseGuard {
        fn drop(&mut self) {
            *TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    fn with_mock<T>(n: usize, routes: Vec<(&'static str, u16, &'static str)>, f: impl FnOnce() -> T) -> (T, Vec<String>) {
        with_mock_secs(n, routes, 5, f)
    }

    fn with_mock_secs<T>(n: usize, routes: Vec<(&'static str, u16, &'static str)>, secs: u64, f: impl FnOnce() -> T) -> (T, Vec<String>) {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (url, h) = mock_secs(n, routes, secs);
        *TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(url);
        let out = {
            let _b = BaseGuard;
            f()
        };
        (out, h.join().unwrap())
    }

    /// Новий тимчасовий файл на кожен виклик: тести йдуть паралельно, а спільний шлях давав гонку
    /// (один тест перезаписував файл, поки інший його читав: порожній або обрізаний файл).
    fn jar() -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!("mp_mock_{}_{}.jar", std::process::id(), N.fetch_add(1, Relaxed)));
        std::fs::write(&p, [b"PK".as_slice(), &[7u8; 4998]].concat()).unwrap();
        p
    }

    #[test]
    fn modrinth_publish_flow() {
        let up = Up::default();
        let (r, seen) = with_mock(3, vec![("GET /v2/project/P/version", 200, "[]"), ("POST /v2/version", 200, r#"{"id":"VER1"}"#)], || {
            publish_modrinth("tok", "P", &rel(), &jar(), &up)
        });
        let ok = r.unwrap();
        assert!(ok.contains("VER1") && ok.contains("https://modrinth.com/project/P/version/VER1"), "{ok}");
        assert_eq!(seen.len(), 3, "версії, хеш файлу, відправка");
        let all = seen.join("\n").to_lowercase();
        assert!(all.contains("authorization: tok"), "токен без Bearer");
        assert!(all.contains("user-agent: mod-publisher/"));
        assert_eq!(up.prog.sent.load(Relaxed), 5000);
        assert_eq!(up.prog.total.load(Relaxed), 5000);
    }

    #[test]
    fn modrinth_duplicate_and_errors() {
        let dup = r#"[{"version_number":"1.0.0","loaders":["fabric"]}]"#;
        let (r, seen) = with_mock(1, vec![("GET /v2/project/P/version", 200, dup)], || publish_modrinth("t", "P", &rel(), &jar(), &Up::default()));
        assert!(r.unwrap_err().contains("вже є"));
        assert_eq!(seen.len(), 1, "після дубля файл не відправляється");

        // той самий номер, але інший лоадер: дозволено
        let other = r#"[{"version_number":"1.0.0","loaders":["forge"]}]"#;
        let (r, _) = with_mock(3, vec![("GET /v2/project/P/version", 200, other), ("POST /v2/version", 200, r#"{"id":"V2"}"#)], || {
            publish_modrinth("t", "P", &rel(), &jar(), &Up::default())
        });
        assert!(r.is_ok());

        let (r, _) = with_mock(1, vec![("GET /v2/project/P/version", 401, "{}")], || publish_modrinth("t", "P", &rel(), &jar(), &Up::default()));
        assert!(r.unwrap_err().contains("Токен недійсний"));
    }

    #[test]
    fn get_retries_5xx_and_shows_api_message() {
        let (r, seen) = with_mock(3, vec![("GET /v2/project/P/version", 503, r#"{"description":"busy"}"#)], || publish_modrinth("t", "P", &rel(), &jar(), &Up::default()));
        let e = r.unwrap_err();
        assert!(e.contains("busy") && e.contains("сервера"), "{e}");
        assert_eq!(seen.len(), 3, "3 спроби GET");
        // 4xx не повторюється
        let (r, seen) = with_mock(1, vec![("GET /v2/project/P/version", 400, r#"{"error":"invalid_input","description":"bad"}"#)], || publish_modrinth("t", "P", &rel(), &jar(), &Up::default()));
        assert!(r.unwrap_err().contains("bad"));
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn preflight_rejects_empty_and_cancelled_without_network() {
        let empty = std::env::temp_dir().join(format!("mp_empty_{}.jar", std::process::id()));
        std::fs::write(&empty, b"").unwrap();
        assert!(publish_modrinth("t", "P", &rel(), &empty, &Up::default()).unwrap_err().contains("порожній"));
        let up = Up::default();
        up.cancel.store(true, Relaxed);
        assert!(publish_curse("t", "k", "7", &rel(), &jar(), &up).unwrap_err().contains("Скасовано"));
        assert!(api_msg("<html>x</html>").contains("html") && api_msg(r#"{"errorMessage":"m"}"#) == "m");
    }

    #[test]
    fn post_retries_only_on_connect_error() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut built = 0;
        let r = post_upload("http://127.0.0.1:1/x", ("a", "b"), || { built += 1; Ok(multipart::Form::new().text("k", "v")) }, &Up::default());
        assert!(r.is_err());
        assert_eq!(built, 2, "одна повторна спроба при відмові з'єднання");
        let up = Up::default();
        up.cancel.store(true, Relaxed);
        let mut built = 0;
        let _ = post_upload("http://127.0.0.1:1/x", ("a", "b"), || { built += 1; Ok(multipart::Form::new()) }, &up);
        assert_eq!(built, 1, "після скасування не повторюємо");
    }

    #[test]
    fn partial_failure_is_independent_per_platform() {
        let types = r#"[{"id":1,"name":"Minecraft 1.20"},{"id":68441,"name":"Modloader"}]"#;
        let vers = r#"[{"id":11,"gameVersionTypeID":1,"name":"1.20.1"},{"id":12,"gameVersionTypeID":68441,"name":"Fabric"}]"#;
        let f = jar();
        let (res, _) = with_mock(
            6,
            vec![
                ("GET /v2/project/P/version", 200, "[]"),
                ("POST /v2/version", 200, r#"{"id":"V9"}"#),
                ("GET /api/game/version-types", 200, types),
                ("GET /api/game/versions", 200, vers),
                ("POST /api/projects/7/upload-file", 403, r#"{"errorMessage":"no rights"}"#),
            ],
            || (publish_modrinth("t", "P", &rel(), &f, &Up::default()), publish_curse("t", "", "7", &rel(), &f, &Up::default())),
        );
        assert!(res.0.unwrap().contains("V9"));
        let e = res.1.unwrap_err();
        assert!(e.contains("no rights") && e.contains("Немає доступу"), "{e}");
    }

    #[test]
    fn post_429_retried_once() {
        let f = jar();
        let (r, seen) = with_mock(4, vec![("GET /v2/project/P/version", 200, "[]"), ("POST /v2/version", 429, "{}")], || publish_modrinth("t", "P", &rel(), &f, &Up::default()));
        assert!(r.unwrap_err().contains("Забагато"));
        assert_eq!(seen.len(), 4, "GET версій + GET хеша + POST + один повтор POST");
    }

    #[test]
    fn rejects_non_archive() {
        let p = std::env::temp_dir().join(format!("mp_fake_{}.jar", std::process::id()));
        std::fs::write(&p, b"not a zip").unwrap();
        assert!(publish_modrinth("t", "P", &rel(), &p, &Up::default()).unwrap_err().contains("не схожий"));
    }

    #[test]
    fn eta_and_limits() {
        assert_eq!(speed_eta(0, 100, 5000), None);
        assert_eq!(speed_eta(10, 100, 100), None);
        assert_eq!(speed_eta(1000, 5000, 1000), Some((1000, 4)));
        let mut r = rel();
        r.changelog = "я".repeat(MAX_CHANGELOG + 1);
        let f = jar();
        assert!(validate(true, true, &r, Some(&f)).unwrap_err().contains("Changelog"));
        r.changelog.clear();
        r.number = "1".repeat(65);
        assert!(validate(true, true, &r, Some(&f)).unwrap_err().contains("задовгий"));
    }

    #[test]
    fn upload_can_be_cancelled() {
        let up = Up::default();
        up.cancel.store(true, Relaxed);
        let (r, _) = with_mock(1, vec![("GET /v2/project/P/version", 200, "[]")], || publish_modrinth("t", "P", &rel(), &jar(), &up));
        assert!(r.unwrap_err().contains("Скасовано"));
    }

    #[test]
    fn curse_publish_and_duplicate() {
        let types = r#"[{"id":1,"name":"Minecraft 1.20"},{"id":68441,"name":"Modloader"}]"#;
        let vers = r#"[{"id":11,"gameVersionTypeID":1,"name":"1.20.1"},{"id":12,"gameVersionTypeID":68441,"name":"Fabric"}]"#;
        let routes = |files: &'static str| {
            vec![
                ("GET /v1/mods/7/files", 200, files),
                ("GET /api/game/version-types", 200, types),
                ("GET /api/game/versions", 200, vers),
                ("POST /api/projects/7/upload-file", 200, r#"{"id":99}"#),
            ]
        };
        let (r, _) = with_mock(4, routes(r#"{"data":[{"displayName":"0.9"}]}"#), || publish_curse("t", "key", "7", &rel(), &jar(), &Up::default()));
        let ok = r.unwrap();
        assert!(ok.contains("файл 99") && !ok.contains("не перевірено"), "{ok}");

        let (r, seen) = with_mock(1, routes(r#"{"data":[{"displayName":"1.0.0"}]}"#), || publish_curse("t", "key", "7", &rel(), &jar(), &Up::default()));
        assert!(r.unwrap_err().contains("уже є"));
        assert_eq!(seen.len(), 1);

        // без Core-ключа дубль не перевіряється, і користувач про це знає
        let (r, _) = with_mock(3, routes("{}"), || publish_curse("t", "", "7", &rel(), &jar(), &Up::default()));
        assert!(r.unwrap().contains("не перевірено"));
    }

    #[test]
    fn curse_duplicate_on_second_page() {
        let page1: &'static str = Box::leak(format!(r#"{{"data":[{}]}}"#, vec![r#"{"displayName":"x"}"#; 50].join(",")).into_boxed_str());
        let types = r#"[{"id":1,"name":"Minecraft 1.20"},{"id":68441,"name":"Modloader"}]"#;
        let (r, seen) = with_mock(
            2,
            vec![
                ("GET /v1/mods/7/files?pageSize=50&index=0", 200, page1),
                ("GET /v1/mods/7/files?pageSize=50&index=50", 200, r#"{"data":[{"displayName":"1.0.0"}]}"#),
                ("GET /api/game/version-types", 200, types),
            ],
            || publish_curse("t", "key", "7", &rel(), &jar(), &Up::default()),
        );
        assert!(r.unwrap_err().contains("уже є"));
        assert_eq!(seen.len(), 2, "дві сторінки, далі файл не відправляється");
    }

    /// Тіло multipart-запиту з частиною "data": JSON між першим "{" і наступним рядком-межею.
    fn data_json(req: &str) -> Value {
        let i = req.find("name=\"data\"").expect("частина data");
        let body = &req[i..];
        let (a, b) = (body.find('{').unwrap(), body.find("
--").unwrap());
        serde_json::from_str(&body[a..b]).unwrap()
    }

    #[test]
    fn options_off_keep_legacy_request() {
        // payload без нових опцій = точно той, що був раніше
        assert_eq!(
            modrinth_data(&rel(), "AAA"),
            json!({"name":"1.0.0","version_number":"1.0.0","changelog":"ok","dependencies":[],"game_versions":["1.20.1"],"version_type":"release","loaders":["fabric"],"featured":false,"project_id":"AAA","file_parts":["file"]})
        );
        // і в мережі: лише перевірка версій, хеш і відправка; жодного запиту проєктів-залежностей
        let (r, seen) = with_mock(3, vec![("GET /v2/project/P/version", 200, "[]"), ("POST /v2/version", 200, r#"{"id":"V1"}"#)], || publish_modrinth("t", "P", &rel(), &jar(), &Up::default()));
        assert!(r.is_ok());
        assert_eq!(seen.len(), 3);
        assert!(seen[1].starts_with("GET /v2/version_file/") && seen[1].contains("algorithm=sha512"));
        let d = data_json(&seen[2]);
        assert_eq!((d["dependencies"].clone(), d["featured"].clone(), d["file_parts"].clone()), (json!([]), json!(false), json!(["file"])));
        assert!(!seen[2].contains("name=\"file1\""));
    }

    #[test]
    fn dependencies_featured_and_extra_files_are_sent() {
        let extra = jar();
        let mut r = rel();
        (r.featured, r.dep_slugs, r.extra_files) = (true, vec!["fabric-api".into(), "ghost".into(), "P".into()], vec![extra]);
        let routes = vec![
            ("GET /v2/project/P/version", 200, "[]"),
            ("GET /v2/project/fabric-api", 200, r#"{"id":"P7dR8mSH","title":"Fabric API"}"#),
            ("GET /v2/project/ghost", 404, "{}"),
            ("GET /v2/project/P ", 200, r#"{"id":"P","title":"self"}"#),
            ("POST /v2/version", 200, r#"{"id":"V2"}"#),
        ];
        let (res, seen) = with_mock(6, routes, || publish_modrinth("t", "P", &r, &jar(), &Up::default()));
        let ok = res.unwrap();
        assert!(ok.contains("V2") && ok.contains("ghost"), "ненайдена залежність названа в підсумку: {ok}");
        let post = seen.iter().find(|s| s.starts_with("POST /v2/version")).unwrap();
        let d = data_json(post);
        assert_eq!(d["dependencies"], json!([{"project_id":"P7dR8mSH","dependency_type":"required"}]), "власний проєкт і невідомі пропущено");
        assert_eq!((d["featured"].clone(), d["file_parts"].clone()), (json!(true), json!(["file", "file1"])));
        assert!(post.contains("name=\"file\"") && post.contains("name=\"file1\""));
    }

    #[test]
    fn same_file_hash_stops_before_upload() {
        let hit = r#"{"id":"OLD1","project_id":"P","version_number":"0.9"}"#;
        let (r, seen) = with_mock(2, vec![("GET /v2/project/P/version", 200, "[]"), ("GET /v2/version_file/", 200, hit)], || publish_modrinth("t", "P", &rel(), &jar(), &Up::default()));
        let e = r.unwrap_err();
        assert!(e.contains("0.9") && e.contains("OLD1") && e.contains("файл"), "{e}");
        assert_eq!(seen.len(), 2, "файл не відправляється");
        // той самий файл в іншому проєкті не блокує
        let other = r#"{"id":"X","project_id":"Q","version_number":"1"}"#;
        let (r, _) = with_mock(3, vec![("GET /v2/project/P/version", 200, "[]"), ("GET /v2/version_file/", 200, other), ("POST /v2/version", 200, r#"{"id":"V3"}"#)], || publish_modrinth("t", "P", &rel(), &jar(), &Up::default()));
        assert!(r.is_ok());
    }

    #[test]
    fn extra_files_validation_and_summary() {
        let d = tmp_dir("extra");
        let main = d.join("mod.jar");
        zip_with(&main, &[("a", "x")]);
        let mk = |n: &str| {
            let p = d.join(n);
            zip_with(&p, &[("a", "x")]);
            p
        };
        let mut r = rel();
        r.extra_files = vec![mk("mod-sources.jar")];
        assert!(validate(true, true, &r, Some(&main)).is_ok());
        r.extra_files = vec![mk("a.jar"), mk("b.jar"), mk("c.jar"), mk("d.jar")];
        assert!(validate(true, true, &r, Some(&main)).unwrap_err().contains("не більше 3"));
        r.extra_files = vec![mk("x.mrpack")];
        assert!(validate(true, true, &r, Some(&main)).unwrap_err().contains(".jar і .zip"));
        r.extra_files = vec![d.join("none.jar")];
        assert!(validate(true, true, &r, Some(&main)).unwrap_err().contains("не існує"));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let same = d.join("sub").join("mod.jar");
        zip_with(&same, &[("a", "x")]);
        r.extra_files = vec![same];
        assert!(validate(true, true, &r, Some(&main)).unwrap_err().contains("ту саму назву"));
        let (e1, e2) = (mk("e.jar"), d.join("sub").join("e.jar"));
        zip_with(&e2, &[("a", "x")]);
        r.extra_files = vec![e1, e2];
        assert!(validate(true, true, &r, Some(&main)).unwrap_err().contains("однакові"));

        let mut r = rel();
        assert_eq!(release_summary("Mod", &r, true, true), "Mod: «1.0.0» · release · MC 1.20.1 · fabric · Modrinth + CurseForge");
        (r.featured, r.dep_slugs, r.extra_files, r.name) = (true, vec!["fabric-api".into()], vec![mk("s.jar")], "Моя".into());
        assert_eq!(release_summary("Mod", &r, true, false), "Mod: «Моя» · release · MC 1.20.1 · fabric · Modrinth · +1 дод. файл(и) · залежності: fabric-api · рекомендована");
        assert!(!release_summary("Mod", &r, false, true).contains("рекомендована"), "лише Modrinth");
    }

    #[test]
    fn token_checks_with_mocks() {
        let user = r#"{"id":"u1","username":"bob"}"#;
        let (r, _) = with_mock(2, vec![("GET /v2/user/u1/projects", 200, r#"[{"id":"a","title":"A"},{"id":"b","title":"B"}]"#), ("GET /v2/user", 200, user)], || modrinth_token_check("t"));
        assert_eq!(r.unwrap(), "bob, проєктів: 2");
        // токен без PROJECT_READ: дійсний, але з попередженням
        let (r, _) = with_mock(2, vec![("GET /v2/user/u1/projects", 401, "{}"), ("GET /v2/user", 200, user)], || modrinth_token_check("t"));
        let m = r.unwrap();
        assert!(m.starts_with("bob.") && m.contains("PROJECT_READ"), "{m}");
        // недійсний токен: зрозуміле пояснення
        let (r, _) = with_mock(1, vec![("GET /v2/user", 401, r#"{"description":"Invalid token"}"#)], || modrinth_token_check("t"));
        assert!(r.unwrap_err().contains("Токен недійсний"));
        // CurseForge: 401/403 мають пояснення, 200 приймається
        let (r, _) = with_mock(1, vec![("GET /api/game/version-types", 200, "[]")], || curse_token_check("t"));
        assert!(r.unwrap().contains("приймається"));
        let (r, _) = with_mock(1, vec![("GET /api/game/version-types", 403, r#"{"errorMessage":"nope"}"#)], || curse_token_check("t"));
        assert!(r.unwrap_err().contains("Немає доступу"));
        let (r, _) = with_mock(1, vec![("GET /v1/mods/7", 200, r#"{"data":{"name":"JEI"}}"#)], || curse_project_check("k", "7"));
        assert_eq!(r.unwrap(), "проєкт «JEI» знайдено");
        let (r, _) = with_mock(1, vec![("GET /v1/mods/8", 404, "{}")], || curse_project_check("k", "8"));
        assert!(r.is_err());
    }

    #[test]
    fn secrets_are_masked_in_errors() {
        assert_eq!(mask_secrets("bad token mrp_SECRET123 here", &["mrp_SECRET123"]), "bad token ••• here");
        assert_eq!(mask_secrets("x", &["", "ab"]), "x", "порожні й короткі не чіпаємо");
        let t = "mrp_SECRETSECRET123";
        let m = mask_secrets(&format!("HTTP 401: Authorization: {t} (also {t})"), &[t, "", " ab "]);
        assert!(!m.contains("SECRET") && m.matches("•••").count() == 2, "{m}");
        assert_eq!(mask_secrets("tok123456 x", &["  tok123456\n"]), "••• x", "пробіли з вставки обрізаються");
        // сервер ехо-повертає токен у повідомленні: після masked його немає
        let (r, _) = with_mock(1, vec![("GET /v2/user", 400, r#"{"description":"invalid header Authorization: mrp_SECRET123"}"#)], || masked(modrinth_token_check("mrp_SECRET123"), &["mrp_SECRET123"]));
        let e = r.unwrap_err();
        assert!(!e.contains("SECRET123") && e.contains("•••"), "{e}");
    }

    #[test]
    fn update_throttle_skip_and_prerelease() {
        let t = 1_000_000;
        assert!(update_due(t, 0, false) && !update_due(t, t - 60, false) && update_due(t, t - UPDATE_EVERY, false));
        assert!(update_due(t, t - 60, true), "ручна перевірка не обмежується");
        assert!(update_due(t, t + 500, false), "годинник пішов назад");
        assert!(!update_visible("0.4.0", "0.4.0", false) && !update_visible("0.4.0", "v0.4.0", false));
        assert!(update_visible("0.4.1", "0.4.0", false) && update_visible("0.4.0", "0.4.0", true));
        let rel = |tag: &str| json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": []});
        assert!(parse_release(&rel("v99.0.0-rc1"), "o/r", "0.3.0").is_none(), "пробні теги ігноруються");
        assert!(parse_release(&rel("v99.0.0+build5"), "o/r", "0.3.0").is_some());
        assert!(parse_release(&rel("v99.0.0"), "o/r", "0.3.0").is_some());
        assert!(parse_release(&json!({"tag_name":"v99.0.0","prerelease":true}), "o/r", "0.3.0").is_none());
        // нові поля конфігу порожні = у файл не потрапляють
        let j = serde_json::to_string(&Config::default()).unwrap();
        assert!(!j.contains("update_checked") && !j.contains("update_skip"), "{j}");
    }

    #[test]
    fn config_forward_backward_compat() {
        // «нова» програма (0.3) зберігає всі свої поля
        let mut cfg = Config { schema: SCHEMA, ..Default::default() };
        (cfg.update_checked, cfg.update_skip) = (123, "0.4.0".into());
        cfg.projects.push(Project { id: "p1".into(), name: "M".into(), name_template: "{project} {version}".into(), draft: Draft { project: "p1".into(), number: "2".into(), ..Default::default() }, ..Default::default() });
        cfg.last.draft = Draft { project: "p1".into(), changelog: "c".into(), ..Default::default() };
        let json = serde_json::to_string(&cfg).unwrap();
        // «стара» програма (0.2): знає лише базові поля, решту тримає як невідоме (serde flatten) і пише назад
        #[derive(Serialize, Deserialize, Default)]
        struct OldProject { name: String, id: String, #[serde(flatten)] extra: serde_json::Map<String, Value> }
        #[derive(Serialize, Deserialize, Default)]
        struct OldCfg { schema: u32, projects: Vec<OldProject>, last: serde_json::Map<String, Value>, #[serde(flatten)] extra: serde_json::Map<String, Value> }
        let old: OldCfg = serde_json::from_str(&json).unwrap();
        assert_eq!(old.projects[0].name, "M");
        let back: Config = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!((back.update_checked, back.update_skip.as_str()), (123, "0.4.0"));
        assert_eq!(back.projects[0].name_template, "{project} {version}");
        assert_eq!(back.projects[0].draft.number, "2");
        assert_eq!(back.last.draft.changelog, "c");
        // і старий файл (без нових полів) відкривається без втрат
        let (c, st) = load_from(&{
            let p = tmp_dir("compat").join("config.json");
            std::fs::write(&p, r#"{"schema":2,"projects":[{"id":"p1","name":"A","future":1}],"mystery":true}"#).unwrap();
            p
        });
        assert!(matches!(st, Load::Fine) && c.projects[0].extra["future"] == 1 && c.extra["mystery"] == true && c.update_checked == 0);
    }

    #[test]
    fn export_import_keeps_name_template_not_draft_or_secrets() {
        let mut p = Project { id: "p1".into(), name: "M".into(), modrinth_id: "AAA".into(), name_template: "{project} v{version}".into(), draft: Draft { number: "9".into(), changelog: "секрет".into(), ..Default::default() }, ..Default::default() };
        p.extra.insert("api_token".into(), json!("tok"));
        p.extra.insert("future".into(), json!(1));
        let out = export_projects(&[p]);
        assert!(!out.contains("tok\"") && !out.contains("секрет") && !out.contains("\"draft\""), "{out}");
        let got = parse_projects_file(&out).unwrap();
        assert_eq!((got[0].name_template.as_str(), got[0].extra.get("future")), ("{project} v{version}", Some(&json!(1))));
        // злиття доповнює порожній шаблон, але не затирає наявний
        let mut cfg = Config { projects: vec![Project { modrinth_id: "AAA".into(), name: "M".into(), ..Default::default() }], ..Default::default() };
        merge_projects(&mut cfg, got.clone());
        assert_eq!(cfg.projects[0].name_template, "{project} v{version}");
        cfg.projects[0].name_template = "мій".into();
        merge_projects(&mut cfg, got);
        assert_eq!(cfg.projects[0].name_template, "мій");
    }

    #[test]
    fn corrupt_config_with_corrupt_backup_is_saved_aside() {
        let d = tmp_dir("corrupt2");
        let p = d.join("config.json");
        std::fs::write(&p, "{ не json").unwrap();
        std::fs::write(backup_path(&p), "{ теж не json").unwrap();
        let (c, st) = load_from(&p);
        assert!(matches!(st, Load::Recovered) && c.projects.is_empty());
        let bak = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().ends_with(".bak"));
        assert!(bak, "зіпсований файл збережено поруч");
        assert!(!p.exists() || serde_json::from_str::<Config>(&std::fs::read_to_string(&p).unwrap()).is_ok());
    }

    #[test]
    fn history_context_restore_and_old_rows() {
        let d = tmp_dir("histctx");
        let p = d.join("history.jsonl");
        // старий рядок без нових полів
        std::fs::write(&p, "{\"t\":1,\"project\":\"Old\",\"number\":\"0.9\",\"platform\":\"Modrinth\",\"ok\":true,\"msg\":\"x\"}\n").unwrap();
        let ctx = HistCtx { project_id: "p7".into(), name: "Моя версія".into(), release_type: "beta".into(), mc_versions: "1.20.1, 1.20.2".into(), loaders: vec!["fabric".into(), "quilt".into()] };
        history_add_with(&p, "New", "1.0", "Modrinth", &Ok("v".into()), &ctx);
        let h = history_load_at(&p, 10);
        assert_eq!(h.len(), 2);
        let r = restore_from_history(&h[0]).unwrap();
        assert_eq!((r.draft.project.as_str(), r.draft.number.as_str(), r.draft.name.as_str()), ("p7", "1.0", "Моя версія"));
        assert_eq!((r.release_type.as_str(), r.mc_versions.as_str(), r.loaders.len()), ("beta", "1.20.1, 1.20.2", 2));
        let old = restore_from_history(&h[1]).unwrap();
        assert_eq!((old.draft.project.as_str(), old.draft.number.as_str(), old.release_type.as_str()), ("", "0.9", ""));
        assert!(restore_from_history(&json!({"project": "x"})).is_none());
        // без контексту нові ключі в рядок не потрапляють
        let p2 = d.join("h2.jsonl");
        history_add_at(&p2, "A", "1", "Modrinth", &Ok("k".into()));
        assert!(!std::fs::read_to_string(&p2).unwrap().contains("pid"));
        assert!(history_filter(&h, "моя версія", false).len() == 1, "пошук за назвою версії");
    }

    #[test]
    fn history_filter_fast_and_export_all_streams_whole_file() {
        let d = tmp_dir("histbig");
        let p = d.join("history.jsonl");
        let mut text = String::new();
        for i in 0..5000u64 {
            let (ok, msg) = if i % 7 == 0 { (false, "помилка мережі") } else { (true, "версія ok") };
            text += &format!("{}\n", json!({"t": 1_700_000_000 + i, "project": format!("Mod{}", i % 40), "number": format!("1.{i}"), "platform": "Modrinth", "ok": ok, "msg": msg}));
        }
        text += "не json\n";
        std::fs::write(&p, &text).unwrap();
        let all: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let t0 = std::time::Instant::now();
        let hits = history_filter(&all, "mod7", false).len();
        let fails = history_filter(&all, "", true).len();
        assert!(t0.elapsed() < Duration::from_secs(2), "фільтр 5000 записів: {:?}", t0.elapsed());
        assert_eq!((hits, fails), (125, 715));
        // «експорт усіх»: файл читається потоково, а не лише хвіст із 300 записів
        let out = d.join("all.csv");
        let n = history_export_at(&p, &out).unwrap();
        assert_eq!(n, 5000, "пошкоджений рядок пропущено");
        let csv = std::fs::read_to_string(&out).unwrap();
        assert_eq!(csv.lines().count(), 5001);
        assert!(csv.lines().nth(1).unwrap().contains("\"Mod0\",\"1.0\","), "від старих до нових");
        // відсутній файл історії: лише заголовок
        assert_eq!(history_export_at(&d.join("none.jsonl"), &d.join("e.csv")).unwrap(), 0);
    }

    #[test]
    fn update_download_reports_progress_and_checks_size() {
        let body: &'static str = Box::leak("x".repeat(300).into_boxed_str());
        let d = tmp_dir("updprog");
        let dest = d.join("new.exe");
        let (r, _) = with_mock(1, vec![("GET /file", 200, body)], || {
            let url = format!("{}/file", TEST_BASE.lock().unwrap().clone().unwrap());
            download_to(&url, &dest, 300)
        });
        assert!(r.is_ok());
        assert_eq!(update_progress(), (300, 300));
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), 300);
        // розмір не збігається з метаданими: файл видаляється
        let (r, _) = with_mock(1, vec![("GET /file", 200, body)], || {
            let url = format!("{}/file", TEST_BASE.lock().unwrap().clone().unwrap());
            download_to(&url, &dest, 999)
        });
        assert!(r.unwrap_err().contains("не повністю") && !dest.exists());
    }

    /// Тека даних оновлення (update.ok) підміняється на тимчасову, щоб тест не чіпав справжній маркер користувача.
    /// Береться після SERIAL (порядок SERIAL -> ENV_LOCK, як в інших тестах) і знімає змінну навіть при паніці.
    struct CfgEnv<'a> {
        _lock: std::sync::MutexGuard<'a, ()>,
    }
    impl CfgEnv<'_> {
        fn set(dir: &Path) -> Self {
            let g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            // shortcut: set_var глобальний; усі тести з env беруть ENV_LOCK
            unsafe { std::env::set_var("MODWIRE_CONFIG_DIR", dir) };
            Self { _lock: g }
        }
    }
    impl Drop for CfgEnv<'_> {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
        }
    }

    fn fake_exe_body(sig: &str) -> &'static str {
        Box::leak(format!("{sig}{}", "x".repeat(1_200_000)).into_boxed_str())
    }

    fn sha_hex(s: &str) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Фальшивий exe для оновлення: ціль заміни завжди тимчасова копія, не працюючий файл тесту.
    fn upd_exe(tag: &str) -> (PathBuf, PathBuf) {
        let d = tmp_dir(tag);
        std::fs::write(d.join("modwire.exe"), b"v1-good").unwrap();
        (d.clone(), d.join("modwire.exe"))
    }

    #[test]
    fn update_full_chain_on_mock_github() {
        let (d, exe) = upd_exe("upd_chain");
        let body = fake_exe_body("MZ");
        let sum = sha_hex(body);
        let rel: &'static str = Box::leak(
            json!({"tag_name":"v99.0.0","html_url":"https://github.com/Waffylka/ModWire/releases/tag/v99.0.0","assets":[
                {"name":"modwire.exe","size":body.len(),"digest":format!("sha256:{sum}"),
                 "browser_download_url":"https://github.com/Waffylka/ModWire/releases/download/v99.0.0/modwire.exe"}]})
            .to_string()
            .into_boxed_str(),
        );
        let (r, seen) = with_mock_secs(2, vec![("GET /repos/Waffylka/ModWire/releases/latest", 200, rel), ("GET /file", 200, body)], 90, || {
            let _e = CfgEnv::set(&d.join("cfg"));
            let u = check_repo(UPDATE_REPO).unwrap().expect("новіший реліз знайдено");
            assert_eq!((u.version.as_str(), u.size, u.sha256.as_str()), ("99.0.0", body.len() as u64, sum.as_str()));
            assert!(trusted_url(&u.url, UPDATE_REPO), "посилання з релізу довірене");
            // старий маркер не має приховувати проблему нової збірки
            std::fs::create_dir_all(d.join("cfg")).unwrap();
            std::fs::write(d.join("cfg").join("update.ok"), b"ok").unwrap();
            // завантажуємо з mock (на github.com тест не ходить), решта ланцюга справжня
            let url = format!("{}/file", TEST_BASE.lock().unwrap().clone().unwrap());
            apply_at(&exe, &url, u.size, &u.sha256)?;
            assert!(!d.join("cfg").join("update.ok").exists());
            assert_eq!(update_progress().0, body.len() as u64);
            assert_eq!(std::fs::read(&exe).unwrap().len(), body.len(), "exe замінено новим");
            assert_eq!(std::fs::read(exe.with_extension("exe.old")).unwrap(), b"v1-good");
            // нова збірка пропрацювала STABLE_SECS: маркер є, .old прибрано
            confirm_update_at(&exe);
            assert!(d.join("cfg").join("update.ok").exists() && !exe.with_extension("exe.old").exists());
            Ok::<(), String>(())
        });
        r.unwrap();
        assert!(seen[0].to_lowercase().contains("accept: application/vnd.github+json") && seen[1].starts_with("GET /file"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn update_broken_downloads_leave_exe_intact() {
        let good = fake_exe_body("MZ");
        let good_sum = sha_hex(good);
        let pk = fake_exe_body("PK");
        let n = good.len() as u64;
        // (тіло, заявлений розмір, digest, що має бути в помилці)
        let cases: Vec<(&'static str, u64, String, &str)> = vec![
            (good, n, "0".repeat(64), "Контрольна сума"),
            (pk, n, String::new(), "не є програмою"),
            (good, n - 10, good_sum.clone(), "не повністю"),
            (good, n + 10, good_sum.clone(), "не повністю"),
            ("MZ-tiny", 0, String::new(), "не є програмою"),
        ];
        for (i, (body, size, sum, want)) in cases.into_iter().enumerate() {
            let (d, exe) = upd_exe(&format!("upd_bad{i}"));
            let (r, _) = with_mock_secs(1, vec![("GET /file", 200, body)], 90, || {
                let _e = CfgEnv::set(&d.join("cfg"));
                let url = format!("{}/file", TEST_BASE.lock().unwrap().clone().unwrap());
                apply_at(&exe, &url, size, &sum)
            });
            let e = r.unwrap_err();
            assert!(e.contains(want), "випадок {i}: «{e}»");
            assert_eq!(std::fs::read(&exe).unwrap(), b"v1-good", "випадок {i}: exe не чіпали");
            assert!(!exe.with_extension("exe.new").exists() && !exe.with_extension("exe.old").exists(), "випадок {i}: без сміття");
            let _ = std::fs::remove_dir_all(&d);
        }
        // сервер віддає 404 замість файлу
        let (d, exe) = upd_exe("upd_404");
        let (r, _) = with_mock_secs(1, vec![("GET /file", 404, "{}")], 90, || {
            let _e = CfgEnv::set(&d.join("cfg"));
            apply_at(&exe, &format!("{}/file", TEST_BASE.lock().unwrap().clone().unwrap()), 0, "")
        });
        assert!(r.unwrap_err().contains("404") && std::fs::read(&exe).unwrap() == b"v1-good");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn update_check_handles_github_errors_and_missing_asset() {
        // ліміт запитів GitHub: зрозуміле повідомлення, не «немає доступу до проєкту»
        let rate = r#"{"message":"API rate limit exceeded for 1.2.3.4."}"#;
        let (r, _) = with_mock(1, vec![("GET /repos/", 403, rate)], || check_repo(UPDATE_REPO));
        let e = r.unwrap_err();
        assert!(e.contains("обмежив") && !e.contains("проєкту"), "{e}");
        // 404: у репозиторії ще немає релізів: оновлень немає, не помилка
        let (r, _) = with_mock(1, vec![("GET /repos/", 404, r#"{"message":"Not Found"}"#)], || check_repo(UPDATE_REPO));
        assert!(r.unwrap().is_none());
        // не JSON (сторінка проксі)
        let (r, _) = with_mock(1, vec![("GET /repos/", 200, "<html>proxy</html>")], || check_repo(UPDATE_REPO));
        assert!(r.is_err());
        // реліз без файлу для Windows або з чужою адресою: url порожній, сторінка релізу лишається
        let none = json!({"tag_name":"v99.0.0","assets":[{"name":"Modwire.dmg","size":1,"browser_download_url":"https://github.com/Waffylka/ModWire/releases/download/v99.0.0/Modwire.dmg"}]});
        let u = parse_release(&none, UPDATE_REPO, "0.3.0").unwrap();
        assert!(u.url.is_empty() && u.page.starts_with("https://github.com/"));
        let evil = json!({"tag_name":"v99.0.0","assets":[{"name":"modwire.exe","size":1,"browser_download_url":"https://evil.example/releases/download/v99.0.0/modwire.exe"}]});
        assert!(parse_release(&evil, UPDATE_REPO, "0.3.0").unwrap().url.is_empty());
    }

    #[test]
    fn every_error_code_has_advice() {
        let advice = ["Перевірте", "Створіть", "Зачекайте", "Змініть", "Оберіть", "Повторіть", "повторіть", "спробуйте"];
        for code in [400u16, 401, 403, 404, 408, 409, 413, 418, 422, 429, 500, 502, 503, 504, 599] {
            let m = human(reqwest::StatusCode::from_u16(code).unwrap());
            assert!(advice.iter().any(|a| m.contains(a)), "{code}: «{m}» без поради");
            assert!(m.chars().count() > 30, "{code}: надто коротко");
        }
        // і мережева помилка підказує, що робити
        assert!(NET_ADVICE.contains("Перевірте інтернет"));
    }

    #[test]
    fn payloads() {
        let m = modrinth_data(&rel(), "AAA");
        assert_eq!(m["name"], "1.0.0");
        assert_eq!(m["file_parts"][0], "file");
        assert_eq!(m["featured"], false);
        let c = curse_metadata(&rel(), &[5, 6]);
        assert_eq!(c["changelogType"], "markdown");
        assert_eq!(c["gameVersions"], json!([5, 6]));
    }

    #[test]
    fn parsing() {
        let (id, name) = parse_user(&json!({"id":"u1","username":"bob"})).unwrap();
        assert_eq!((id.as_str(), name.as_str()), ("u1", "bob"));
        let p = parse_projects(&json!([{"id":"a","title":"A","icon_url":null},{"id":"b","title":"B","icon_url":"http://x"}]));
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].icon_url, "");
        assert_eq!(p[1].icon_url, "http://x");
        let (n, i) = parse_curse_mod(&json!({"data":{"name":"M","logo":{"thumbnailUrl":"t"}}})).unwrap();
        assert_eq!((n.as_str(), i.as_str()), ("M", "t"));
        assert_eq!(parse_mc(" 1.20.1, ,1.20.2 1.20.1"), vec!["1.20.1", "1.20.2"]);
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mp_t4_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn zip_with(path: &Path, files: &[(&str, &str)]) {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (n, c) in files {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(c.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn jar_autofill() {
        let d = tmp_dir("jar");
        let f = d.join("a.jar");
        zip_with(&f, &[("fabric.mod.json", r#"{"name":"Cool","version":"2.1.0","depends":{"minecraft":["1.20.1","1.20.2"]}}"#)]);
        let i = read_jar(&f).unwrap();
        assert_eq!((i.version.as_deref(), i.name.as_deref()), (Some("2.1.0"), Some("Cool")));
        assert_eq!(i.mc_versions, vec!["1.20.1", "1.20.2"]);
        assert_eq!(i.loaders, vec!["fabric"]);

        let toml = "[[mods]]\nversion=\"${file.jarVersion}\"\n[[dependencies.m]]\nmodId=\"minecraft\"\nversionRange=\"[1.21.1]\"\n[[dependencies.m]]\nmodId=\"neoforge\"\nversionRange=\"[21,)\"\n";
        zip_with(&f, &[("META-INF/neoforge.mods.toml", toml), ("META-INF/MANIFEST.MF", "Implementation-Version: 3.0\n")]);
        let i = read_jar(&f).unwrap();
        assert_eq!((i.version.as_deref(), i.mc_versions.clone(), i.loaders.clone()), (Some("3.0"), vec!["1.21.1".to_string()], vec!["neoforge"]));

        let mr = d.join("p.mrpack");
        zip_with(&mr, &[("modrinth.index.json", r#"{"name":"Pack","versionId":"1.4","dependencies":{"minecraft":"1.20.4","fabric-loader":"0.15"}}"#)]);
        let i = read_jar(&mr).unwrap();
        assert_eq!((i.version.as_deref(), i.mc_versions, i.loaders), (Some("1.4"), vec!["1.20.4".to_string()], vec!["fabric"]));

        assert!(mc_exact(">=1.20").is_none() && mc_exact("1.20.x").is_none() && mc_exact("~1.20.1").is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn resource_and_data_packs_get_their_own_loader() {
        let d = tmp_dir("packs");
        let (res, data, nothing, modjar) = (d.join("res.zip"), d.join("data.zip"), d.join("plain.zip"), d.join("mod.zip"));
        zip_with(&res, &[("pack.mcmeta", "{}"), ("assets/minecraft/lang/en_us.json", "{}")]);
        zip_with(&data, &[("pack.mcmeta", "{}"), ("data/x/recipes/a.json", "{}")]);
        zip_with(&nothing, &[("readme.txt", "x")]);
        // Forge 1.12: mcmod.info замість mods.toml, але pack.mcmeta й assets/ теж є. Це мод (.jar), а не ресурспак
        let (old_forge, old_forge_zip) = (d.join("old.jar"), d.join("old.zip"));
        zip_with(&old_forge, &[("mcmod.info", "[]"), ("pack.mcmeta", "{}"), ("assets/m/a.png", "")]);
        zip_with(&old_forge_zip, &[("mcmod.info", "[]"), ("pack.mcmeta", "{}"), ("assets/m/a.png", "")]);
        assert!(read_jar(&old_forge).unwrap().loaders.is_empty() && read_jar(&old_forge_zip).unwrap().loaders.is_empty(), "лоадери мода не підміняються на minecraft");
        // pack.mcmeta після тисяч assets/: знаходиться за іменем, а не за порядком у каталозі
        let big = d.join("big.zip");
        {
            use std::io::Write;
            let mut w = zip::ZipWriter::new(std::fs::File::create(&big).unwrap());
            for i in 0..2_000 {
                w.start_file(format!("assets/minecraft/textures/t{i}.png"), zip::write::SimpleFileOptions::default()).unwrap();
            }
            w.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"{}").unwrap();
            w.finish().unwrap();
        }
        assert_eq!(read_jar(&big).unwrap().loaders, vec!["minecraft"]);
        // мод із pack.mcmeta (так буває) лишається модом: лоадер береться з його метаданих
        zip_with(&modjar, &[("pack.mcmeta", "{}"), ("assets/m/a.png", ""), ("fabric.mod.json", r#"{"version":"1"}"#)]);
        assert_eq!(read_jar(&res).unwrap().loaders, vec!["minecraft"]);
        assert_eq!(read_jar(&data).unwrap().loaders, vec!["datapack"]);
        assert!(read_jar(&nothing).unwrap().loaders.is_empty());
        assert_eq!(read_jar(&modjar).unwrap().loaders, vec!["fabric"]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn curseforge_ignores_pack_loaders_but_still_needs_mod_loaders() {
        let types = json!([{"id":1,"name":"Minecraft 1.20"},{"id":68441,"name":"Modloader"}]);
        let versions = json!([{"id":10,"gameVersionTypeID":1,"name":"1.20.1"},{"id":20,"gameVersionTypeID":68441,"name":"Fabric"}]);
        let mc = vec!["1.20.1".to_string()];
        assert_eq!(resolve_curse_ids(&types, &versions, &mc, &["minecraft".to_string()]).unwrap(), vec![10], "ресурспак: лише версія MC");
        assert_eq!(resolve_curse_ids(&types, &versions, &mc, &["fabric".to_string()]).unwrap(), vec![10, 20]);
        assert!(resolve_curse_ids(&types, &versions, &mc, &["forge".to_string()]).is_err(), "невідомий модлоадер лишається помилкою");
        assert!(LOADERS.contains(&"minecraft") && is_pack_loader("datapack") && !is_pack_loader("fabric"));
    }

    #[test]
    fn config_roundtrip_backup_and_migration() {
        let d = tmp_dir("cfg");
        let p = d.join("config.json");
        // старий файл без schema, з невідомими полями й пробілами в токені
        std::fs::write(&p, r#"{"modrinth_token":" tok\n","future":{"a":1},"projects":[{"name":"X","modrinth_id":"M","novel":5}]}"#).unwrap();
        let (mut c, st) = load_from(&p);
        assert!(matches!(st, Load::Fine));
        assert_eq!((c.schema, c.modrinth_token.as_str()), (SCHEMA, "tok"));
        assert!(d.join("config.pre-v2.json").exists(), "оригінал збережено перед міграцією");
        c.projects[0].name = "Y".into();
        save_to(&p, &c).unwrap();
        let saved = std::fs::read_to_string(&p).unwrap();
        assert!(saved.contains("\"future\"") && saved.contains("\"novel\""), "невідомі поля не губляться");
        save_to(&p, &c).unwrap();
        c.projects[0].name = "Z".into();
        save_to(&p, &c).unwrap();
        assert!(std::fs::read_to_string(backup_path(&p)).unwrap().contains("\"Y\""), "бекап = попередній стан");
        // пошкодження основного файлу: відновлення з бекапу
        std::fs::write(&p, "{broken").unwrap();
        let (c2, st) = load_from(&p);
        assert!(matches!(st, Load::Restored) && c2.projects[0].name == "Y");
        // без бекапу: скидання, але зіпсований файл зберігається
        let _ = std::fs::remove_file(backup_path(&p));
        std::fs::write(&p, "{broken").unwrap();
        let (c3, st) = load_from(&p);
        assert!(matches!(st, Load::Recovered) && c3.projects.is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn history_filter_clear_and_trim() {
        let d = tmp_dir("hist");
        let p = d.join("history.jsonl");
        history_add_at(&p, "Alpha", "1.0", "Modrinth", &Ok("ok".into()));
        history_add_at(&p, "Beta", "2.0", "CurseForge", &Err("Токен недійсний".into()));
        let h = history_load_at(&p, 10);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0]["project"], "Beta");
        assert_eq!(history_filter(&h, "", true).len(), 1);
        assert_eq!(history_filter(&h, "ALPHA", false).len(), 1);
        assert_eq!(history_filter(&h, "недійсний", false)[0]["project"], "Beta");
        assert!(history_filter(&h, "zzz", false).is_empty());
        history_clear_at(&p).unwrap();
        assert!(history_load_at(&p, 10).is_empty());
        history_clear_at(&p).unwrap(); // повторне очищення не помилка
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn projects_export_import_merge_template() {
        let mut cfg = Config::default();
        cfg.projects.push(Project { name: "A".into(), modrinth_id: "m1".into(), ..Default::default() });
        let incoming = parse_projects_file(&export_projects(&[
            Project { name: "A".into(), modrinth_id: "m1".into(), curse_id: "c1".into(), ..Default::default() },
            Project { name: "B".into(), curse_id: "c2".into(), ..Default::default() },
        ])).unwrap();
        assert_eq!(merge_projects(&mut cfg, incoming), (1, 1));
        assert_eq!((cfg.projects.len(), cfg.projects[0].curse_id.as_str()), (2, "c1"));
        assert!(parse_projects_file("{}").is_err() && parse_projects_file("не json").is_err());
        let t = render_template("## {project} {version} ({mc}, {loaders})", "A", &rel());
        assert_eq!(t, "## A 1.0.0 (1.20.1, fabric)");
    }

    #[test]
    fn ids_migration_and_sorting() {
        let d = tmp_dir("ids");
        let p = d.join("config.json");
        std::fs::write(&p, r#"{"schema":1,"projects":[{"name":"b"},{"name":"A","id":"x"},{"name":"c","id":"x"},{"name":"D"}]}"#).unwrap();
        let (c, _) = load_from(&p);
        let ids: std::collections::HashSet<_> = c.projects.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids.len(), 4);
        assert!(c.projects.iter().all(|p| !p.id.is_empty()) && c.projects[1].id == "x", "наявний id зберігається");
        let mut ps = c.projects.clone();
        ps[3].pinned = true; // D
        assert_eq!(sort_perm(&ps), vec![3, 1, 0, 2]); // D, A, b, c
        assert_eq!(pin_perm(&ps), vec![3, 0, 1, 2], "закріплення не чіпає ручний порядок решти");
        // порядок стабільний при однакових ключах
        assert_eq!(sort_perm(&[Project::default(), Project::default()]), vec![0, 1]);
        let mut q = Project { name: "q".into(), ..Default::default() };
        q.extra.insert("api_token".into(), json!("secret"));
        q.extra.insert("note".into(), json!("ok"));
        let ex = export_projects(&[q]);
        assert!(!ex.contains("secret") && ex.contains("\"note\""));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn version_warnings() {
        let h = vec![json!({"ok":true,"project":"A","number":"1.2.0"}), json!({"ok":false,"project":"A","number":"1.3.0"})];
        assert!(version_warning("1.2.0", "A", "1.2.0", &h).unwrap().contains("вже було"));
        assert!(version_warning("1.3.0", "A", "1.2.0", &h).is_none(), "невдала спроба не рахується");
        assert!(version_warning("1.2.0", "B", "", &h).is_none());
        assert!(version_warning("1.1.9", "A", "1.2.0", &[]).unwrap().contains("менший"));
        assert!(version_warning("1.10.0", "A", "1.9.0", &[]).is_none(), "порівняння числове");
        assert!(version_warning("", "A", "1.0", &[]).is_none());
    }

    #[test]
    fn project_info_refresh_via_mock() {
        let ps = vec![Project { name: "Old".into(), modrinth_id: "P".into(), ..Default::default() }];
        let (r, seen) = with_mock(1, vec![("GET /v2/project/P", 200, r#"{"title":"New","icon_url":"http://i"}"#)], || refresh_projects("tok", "", &ps));
        assert_eq!(r, vec![("P".to_string(), String::new(), "New".to_string(), "http://i".to_string())]);
        assert_eq!(seen.len(), 1);
        assert!(refresh_projects("", "", &ps).is_empty(), "без токена мережі немає");
    }

    #[test]
    fn config_merge_tolerates_unknown_and_missing_fields() {
        let d = tmp_dir("merge");
        let p = d.join("config.json");
        std::fs::write(&p, r#"{"schema":1,"projects":[{"name":"A","modrinth_id":"m"}],"last":{"dir":"x","zzz":1}}"#).unwrap();
        let (mut c, _) = load_from(&p);
        let n = merge_projects(&mut c, vec![Project { name: "A2".into(), modrinth_id: "m".into(), curse_id: "c".into(), ..Default::default() }]);
        assert_eq!(n, (0, 1));
        save_to(&p, &c).unwrap();
        let (c2, st) = load_from(&p);
        assert!(matches!(st, Load::Fine) && c2.projects.len() == 1 && c2.projects[0].curse_id == "c" && c2.last.dir == "x");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn validation_order() {
        let f = std::env::temp_dir().join(format!("mp_test_{}.jar", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        let txt = std::env::temp_dir().join(format!("mp_test_{}.txt", std::process::id()));
        std::fs::write(&txt, b"x").unwrap();
        assert!(validate(false, false, &Release::default(), None).unwrap_err().contains("проєкт"));
        assert!(validate(true, false, &Release::default(), None).unwrap_err().contains("платформу"));
        assert!(validate(true, true, &Release::default(), None).unwrap_err().contains("номер"));
        assert!(validate(true, true, &rel(), None).unwrap_err().contains("файл"));
        assert!(validate(true, true, &rel(), Some(Path::new("nope.jar"))).unwrap_err().contains("не існує"));
        assert!(validate(true, true, &rel(), Some(&txt)).unwrap_err().contains(".jar"));
        assert!(validate(true, true, &rel(), Some(&f)).is_ok());
    }

    #[test]
    fn update_versions_and_release_parsing() {
        assert!(is_newer("v0.3.0", "0.2.0") && is_newer("1.0", "0.9.9") && is_newer("0.2.1", "0.2.0"));
        assert!(!is_newer("v0.2.0", "0.2.0") && !is_newer("0.1.9", "0.2.0") && !is_newer("junk", "0.2.0") && !is_newer("1.2.3.4", "0.2.0"));
        assert!(is_newer("v0.10.0", "0.9.0"), "числове, а не рядкове порівняння");
        let rel = json!({"tag_name":"v0.3.0","html_url":"https://github.com/o/r/releases/tag/v0.3.0","assets":[
            {"name":"Modwire.dmg","browser_download_url":"https://github.com/o/r/releases/download/v0.3.0/Modwire.dmg"},
            {"name":"modwire.exe","browser_download_url":"https://github.com/o/r/releases/download/v0.3.0/modwire.exe"}]});
        let u = parse_release(&rel, "o/r", "0.2.0").unwrap();
        assert_eq!((u.version.as_str(), u.url.as_str()), ("0.3.0", "https://github.com/o/r/releases/download/v0.3.0/modwire.exe"));
        assert!(parse_release(&rel, "o/r", "0.3.0").is_none(), "та сама версія");
        // файл із чужого репозиторію не приймаємо: лишається лише сторінка
        let evil = json!({"tag_name":"v9.0.0","assets":[{"name":"modwire.exe","browser_download_url":"https://evil.example/modwire.exe"}]});
        let u = parse_release(&evil, "o/r", "0.2.0").unwrap();
        assert!(u.url.is_empty() && u.page.starts_with("https://github.com/o/r/"));
        assert!(parse_release(&json!({"tag_name":"v9.0.0","prerelease":true}), "o/r", "0.2.0").is_none());
        assert!(check_repo("OWNER/REPO").unwrap().is_none(), "плейсхолдер: без мережі");
        assert!(parse_release(&json!({"tag_name":"v0.3.0-rc1"}), "o/r", "0.2.0").is_none(), "пробний тег");
        let sized = json!({"tag_name":"v0.3.0","assets":[{"name":"modwire.exe","size":1234,"browser_download_url":"https://github.com/o/r/releases/download/v0.3.0/modwire.exe"}]});
        assert_eq!(parse_release(&sized, "o/r", "0.2.0").unwrap().size, 1234);
    }

    #[test]
    fn update_url_must_belong_to_repo() {
        let ok = "https://github.com/Waffylka/ModWire/releases/download/v0.3.0/modwire.exe";
        assert!(trusted_url(ok, "Waffylka/ModWire") && trusted_url(ok, "waffylka/modwire"));
        for bad in [
            "http://github.com/Waffylka/ModWire/releases/download/v0.3.0/modwire.exe",
            "https://evil.example/Waffylka/ModWire/releases/download/v0.3.0/modwire.exe",
            "https://github.com@evil.example/Waffylka/ModWire/releases/download/v0.3.0/modwire.exe",
            "https://user@github.com/Waffylka/ModWire/releases/download/v0.3.0/modwire.exe",
            "https://github.com/Waffylka/ModWire2/releases/download/v0.3.0/modwire.exe",
            "https://github.com/Waffylka/ModWire/releases/download/../../../evil/x/releases/download/v1/modwire.exe",
            "https://github.com/Waffylka/ModWire/releases/download/%2e%2e/%2e%2e/%2e%2e/evil/modwire.exe",
            "https://github.com/Waffylka/ModWire/releases/download/v0.3.0/",
            "https://github.com:444/Waffylka/ModWire/releases/download/v0.3.0/modwire.exe",
            "not a url",
        ] {
            assert!(!trusted_url(bad, "Waffylka/ModWire"), "{bad}");
        }
    }

    #[test]
    fn update_download_validation() {
        let d = std::env::temp_dir().join(format!("mp_upd_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let good = d.join("g.exe");
        let mut b = vec![0u8; 1_200_000];
        b[..2].copy_from_slice(b"MZ");
        std::fs::write(&good, &b).unwrap();
        assert!(valid_exe(&good));
        std::fs::write(d.join("small.exe"), b"MZ").unwrap();
        b[..2].copy_from_slice(b"PK");
        std::fs::write(d.join("zip.exe"), &b).unwrap();
        assert!(!valid_exe(&d.join("small.exe")) && !valid_exe(&d.join("zip.exe")) && !valid_exe(&d.join("none.exe")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn update_digest_rollback_and_write_check() {
        let d = std::env::temp_dir().join(format!("mp_upd2_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // sha256("abc")
        std::fs::write(d.join("f"), b"abc").unwrap();
        assert_eq!(sha256_file(&d.join("f")).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let rel = json!({"tag_name":"v9.0.0","assets":[{"name":"modwire.exe","size":5,"digest":format!("SHA256:{h}"),"browser_download_url":"https://github.com/o/r/releases/download/v9.0.0/modwire.exe"}]});
        assert_eq!(parse_release(&rel, "o/r", "1.0.0").unwrap().sha256, "", "некоректний префікс ігнорується");
        let rel = json!({"tag_name":"v9.0.0","assets":[{"name":"modwire.exe","size":5,"digest":format!("sha256:{}", h.to_uppercase()),"browser_download_url":"https://github.com/o/r/releases/download/v9.0.0/modwire.exe"}]});
        assert_eq!(parse_release(&rel, "o/r", "1.0.0").unwrap().sha256, h);
        // відкат: .old повертається на місце зламаного exe
        let exe = d.join("modwire.exe");
        std::fs::write(&exe, b"new-broken").unwrap();
        std::fs::write(exe.with_extension("exe.old"), b"old-good").unwrap();
        rollback_at(&exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"old-good");
        assert!(rollback_at(&exe).is_err(), "без .old відкату немає");
        assert!(check_writable(&d).is_ok());
        let e = check_writable(&d.join("немає")).unwrap_err();
        assert!(e.contains("Немає прав на запис"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn validate_rejects_empty_file() {
        let p = std::env::temp_dir().join(format!("mp_val_empty_{}.jar", std::process::id()));
        std::fs::write(&p, b"").unwrap();
        assert!(validate(true, true, &rel(), Some(&p)).unwrap_err().contains("порожній"));
        std::fs::write(&p, b"PK").unwrap();
        assert!(validate(true, true, &rel(), Some(&p)).is_ok());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn update_install_edge_cases() {
        let d = std::env::temp_dir().join(format!("mp_inst_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (exe, new, old) = (d.join("modwire.exe"), d.join("modwire.exe.new"), d.join("modwire.exe.old"));
        let mut b = vec![0u8; 1_200_000];
        b[..2].copy_from_slice(b"MZ");
        std::fs::write(&exe, b"old-version").unwrap();
        // залишок попередньої спроби (.new і .old) не заважає; digest відсутній -> лишаються розмір і сигнатура
        std::fs::write(&old, b"stale").unwrap();
        std::fs::write(&new, &b).unwrap();
        install(&exe, &new, &old, "").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap().len(), b.len());
        assert_eq!(std::fs::read(&old).unwrap(), b"old-version");
        assert!(!new.exists());
        // невірна сума: exe не чіпаємо, .new прибрано
        std::fs::write(&new, &b).unwrap();
        let err = install(&exe, &new, &old, &"0".repeat(64)).unwrap_err();
        assert!(err.contains("Контрольна сума") && !new.exists());
        assert_eq!(std::fs::read(&exe).unwrap().len(), b.len());
        // не exe: те саме
        std::fs::write(&new, b"PK-not-exe").unwrap();
        assert!(install(&exe, &new, &old, "").is_err() && !new.exists());
        // повторний запуск оновлення (замок: прапорець UPDATING глобальний, його читають і тести ланцюга)
        if cfg!(windows) {
            let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            UPDATING.store(true, Relaxed);
            let r = apply_update("https://github.com/Waffylka/ModWire/releases/download/v9.9.9/modwire.exe", 0, "");
            UPDATING.store(false, Relaxed);
            assert!(r.unwrap_err().contains("вже виконується"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn history_reads_only_tail() {
        let p = std::env::temp_dir().join(format!("mp_hist_tail_{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&p);
        for i in 0..2000 {
            history_add_at(&p, "P", &format!("{i}"), "Modrinth", &Ok("x".repeat(200)));
        }
        let h = history_load_at(&p, 5);
        assert_eq!(h.len(), 5);
        assert_eq!(h[0]["number"], "1999");
        assert_eq!(h[4]["number"], "1995");
        let big = history_load_at(&p, 100_000);
        assert!(big.iter().all(|e| e["project"] == "P"), "обрізаний перший рядок вікна відкинуто");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn config_empty_corrupt_and_two_dirs() {
        let base = std::env::temp_dir().join(format!("mp_cfg3_{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        // порожній і пошкоджений файли: не паніка, дефолтний конфіг, оригінал відкладено в .bak
        for (name, content) in [("empty", ""), ("junk", "{not json")] {
            let p = base.join(format!("{name}.json"));
            std::fs::write(&p, content).unwrap();
            let (c, st) = load_from(&p);
            assert!(c.projects.is_empty() && matches!(st, Load::Recovered | Load::Restored));
            assert!(!p.exists());
        }
        // дві копії з різними теками не блокують одна одну; у тій самій теці друга бачить блокування
        let l1 = lock_at(&a.join(".lock")).unwrap();
        let l2 = lock_at(&b.join(".lock")).unwrap();
        assert!(l1.is_some() && l2.is_some());
        assert!(lock_at(&a.join(".lock")).unwrap().is_none());
        // збереження в одну теку не торкається іншої
        let cfg = Config { modrinth_token: "tok".into(), ..Default::default() };
        save_to(&a.join("config.json"), &cfg).unwrap();
        assert!(!b.join("config.json").exists());
        drop((l1, l2));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn api_errors_have_advice() {
        assert!(human(reqwest::StatusCode::BAD_GATEWAY).contains("Зачекайте"));
        assert!(human(reqwest::StatusCode::UNAUTHORIZED).contains("Створіть новий токен"));
    }

    #[test]
    fn publish_connection_drop_mid_body_is_safe_to_retry() {
        use std::io::{Read, Write};
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        *TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("http://{}", l.local_addr().unwrap()));
        let _reset = BaseGuard;
        l.set_nonblocking(true).unwrap();
        let h = std::thread::spawn(move || {
            let mut posts = 0;
            let end = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < end && posts < 1 {
                let Ok((mut s, _)) = l.accept() else { std::thread::sleep(Duration::from_millis(5)); continue };
                s.set_nonblocking(false).ok();
                s.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                if buf[..n].starts_with(b"GET") {
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]");
                } else {
                    posts += 1;
                    let _ = s.read(&mut buf); // частина тіла, далі обрив
                }
            }
            posts
        });
        let r = publish_modrinth("t", "P", &rel(), &jar(), &Up::default());
        let e = r.unwrap_err();
        assert!(e.contains("перевірте на сайті"), "{e}");
        assert_eq!(h.join().unwrap(), 1, "POST після обриву не повторюється автоматично");
    }

    #[test]
    fn compat_warnings_flag_wrong_loader_and_mc() {
        let info = JarInfo { loaders: vec!["fabric"], mc_versions: vec!["1.20.1".into()], ..Default::default() };
        assert!(compat_warnings(&info, &["1.20.1".into()], &["fabric".into()]).is_empty());
        let w = compat_warnings(&info, &["1.20.1".into(), "1.19".into()], &["forge".into()]);
        assert_eq!(w.len(), 2);
        assert!(w[0].contains("forge") && w[1].contains("1.19"));
        assert!(compat_warnings(&JarInfo::default(), &["1.0".into()], &["forge".into()]).is_empty(), "без метаданих не гадаємо");
    }

    #[test]
    fn schema2_config_opens_without_loss_and_tokens_never_leak() {
        let d = std::env::temp_dir().join(format!("mp_s2_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("config.json");
        let orig = json!({
            "schema": 2, "modrinth_token": "mrp_SECRET1", "curse_token": "cf-SECRET2", "curse_core_key": "core-SECRET3",
            "modrinth_checked": 11, "curse_checked": 22, "core_checked": 33, "future_top": [1, 2],
            "projects": [{
                "last_number": "1.0", "last_changelog": "x", "name": "A", "modrinth_id": "M1", "curse_id": "7", "icon_url": "https://i/x.png",
                "mc_versions": "1.20.1", "loaders": ["fabric"], "release_type": "beta", "changelog_template": "t {version}",
                "id": "p1", "pinned": true, "future_p": {"k": "v"}, "api_token": "SECRET4"
            }],
            "last": {"dir": "C:/m", "project": 0, "mc_versions": "1.20.1", "loaders": ["fabric"], "release_type": "beta", "future_l": 5}
        });
        std::fs::write(&p, serde_json::to_string_pretty(&orig).unwrap()).unwrap();
        let (c, st) = load_from(&p);
        assert!(matches!(st, Load::Fine));
        save_to(&p, &c).unwrap();
        let back: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(back, orig, "нічого не втрачено і не додано");
        // секрети не виходять ні в експорт, ні в CSV історії, ні в запис історії
        let exp = export_projects(&c.projects);
        assert!(!exp.contains("SECRET"), "{exp}");
        let hp = d.join("h.jsonl");
        history_add_at(&hp, "A", "1.0", "Modrinth", &Err("помилка".into()));
        let hist = history_load_at(&hp, 10);
        assert!(!history_csv(&hist).contains("SECRET") && !std::fs::read_to_string(&hp).unwrap().contains("SECRET"));
        // імпорт чужого повного config.json теж не переносить токени (ні верхні, ні у проєктах)
        let imported = parse_projects_file(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(imported[0].extra.get("api_token").is_none() && imported[0].extra.contains_key("future_p"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn history_link_and_csv() {
        let ok = json!({"t":86400u64,"project":"A, \"x\"","number":"1.0","platform":"Modrinth","ok":true,"msg":"версія 1 https://modrinth.com/project/p/version/1"});
        let bad = json!({"t":0u64,"project":"=B","number":"2","platform":"CurseForge","ok":false,"msg":"https://x.y помилка\nрядок"});
        assert_eq!(history_link(&ok).as_deref(), Some("https://modrinth.com/project/p/version/1"));
        assert_eq!(history_link(&bad), None, "для помилок посилання не пропонуємо");
        let csv = history_csv(&[ok, bad]); // найновіші першими -> у файлі від старих
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].starts_with("1970-01-01 00:00:00,\"'=B\"") && lines[1].contains("error"));
        assert!(lines[2].starts_with("1970-01-02 00:00:00,\"A, \"\"x\"\"\""));
    }

    #[test]
    fn last_keeps_unknown_fields() {
        let c: Config = serde_json::from_str(r#"{"last":{"dir":"x","future":{"a":1}}}"#).unwrap();
        assert!(serde_json::to_string(&c).unwrap().contains(r#""future":{"a":1}"#));
    }

    #[test]
    fn config_dir_override_redirects_all_paths() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _e = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = std::env::temp_dir().join(format!("mp_cfgdir_{}", std::process::id()));
        // shortcut: set_var глобальний; безпечно, бо інші тести не читають цю змінну поза SERIAL
        unsafe { std::env::set_var("MODWIRE_CONFIG_DIR", &d) };
        let (cfg, hist, icon) = (config_path(), history_path(), icon_path("http://x/y.png"));
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
        assert_eq!(cfg.unwrap(), d.join("config.json"));
        assert_eq!(hist.unwrap(), d.join("history.jsonl"));
        assert!(icon.unwrap().starts_with(d.join("icons")));
    }

    #[test]
    fn final_review_fixes() {
        // Quilt запускає Fabric-моди
        let info = JarInfo { loaders: vec!["fabric"], ..Default::default() };
        assert!(compat_warnings(&info, &[], &["fabric".into(), "quilt".into()]).is_empty());
        assert!(!compat_warnings(&info, &[], &["forge".into()]).is_empty());
        // CSV: формули, що починаються з табуляції чи CR
        assert!(csv_cell(&format!("{}=1+1", char::from(9))).starts_with("\"'") && csv_cell("=1+1").starts_with("\"'") && csv_cell(&format!("{}@x", char::from(13))).starts_with("\"'"));
        // секрети у вкладених невідомих полях
        let mut p = Project::default();
        p.extra.insert("future".into(), json!({"api_token": "x", "ok": 1, "deep": [{"secret": "y", "name": "n"}]}));
        let s = serde_json::to_string(&strip_secrets(&p)).unwrap();
        assert!(!s.contains("api_token") && !s.contains("secret") && s.contains("\"ok\":1") && s.contains("\"name\":\"n\""), "{s}");
    }

    #[test]
    fn icons_https_only_and_text_read_is_capped() {
        assert!(icon_url_ok("https://cdn.modrinth.com/a.png"));
        for bad in ["http://192.168.0.1/a.png", "file:///C:/x.png", "ftp://x/a.png", "", &format!("https://x/{}", "a".repeat(3000))] {
            assert!(!icon_url_ok(bad), "{bad}");
        }
        let d = tmp_dir("capped");
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("big.txt");
        std::fs::write(&f, "a".repeat(5000)).unwrap();
        assert_eq!(read_text_capped(&f, 100).unwrap().len(), 100);
        assert_eq!(read_text_capped(&f, 10_000).unwrap().len(), 5000);
        assert!(read_text_capped(&d.join("nope.txt"), 10).is_err());
    }

    #[test]
    fn token_clients_do_not_follow_cross_host_redirects() {
        use std::io::{Read, Write};
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let evil = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        evil.set_nonblocking(true).unwrap();
        let a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (a_url, loc) = (format!("http://{}", a.local_addr().unwrap()), format!("http://{}/steal", evil.local_addr().unwrap()));
        let h = std::thread::spawn(move || {
            let (mut s, _) = a.accept().unwrap();
            let _ = s.read(&mut [0u8; 4096]);
            let _ = write!(s, "HTTP/1.1 302 Found\r\nLocation: {loc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        });
        *TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(a_url);
        let r = {
            let _b = BaseGuard;
            curse_token_check("SECRET-TOKEN")
        };
        h.join().unwrap();
        assert!(r.is_err(), "редирект на інший хост не слід виконувати: {r:?}");
        assert!(evil.accept().is_err(), "токен не має дійти до іншого хоста");
    }

    #[test]
    fn history_add_is_serialized() {
        let d = tmp_dir("hist_race");
        let p = d.join("history.jsonl");
        let ths: Vec<_> = (0..8)
            .map(|t| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for i in 0..30 {
                        history_add_at(&p, "P", &format!("{t}.{i}"), "Modrinth", &Ok("x".repeat(40_000)));
                    }
                })
            })
            .collect();
        ths.into_iter().for_each(|t| t.join().unwrap());
        // файл виріс понад 1 МБ, тож відбулась обрізка до 2000 рядків; жоден рядок не пошкоджений
        let all = std::fs::read_to_string(&p).unwrap();
        assert!(all.lines().count() > 0 && all.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()));
    }

    #[test]
    fn update_rollback_roundtrip_and_grace() {
        let d = tmp_dir("upd_rb");
        let (exe, new, old) = (d.join("modwire.exe"), d.join("modwire.exe.new"), d.join("modwire.exe.old"));
        let mut b = vec![0u8; 1_200_000];
        b[..2].copy_from_slice(b"MZ");
        std::fs::write(&exe, b"v1-good").unwrap();
        std::fs::write(&new, &b).unwrap();
        install(&exe, &new, &old, "").unwrap();
        // нова збірка не стартувала: відкат повертає v1, а .old більше немає
        rollback_at(&exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"v1-good");
        assert!(!old.exists());
        // невдала перевірка нового файлу не чіпає ні exe, ні наявного .old
        std::fs::write(&old, b"prev").unwrap();
        std::fs::write(&new, b"junk").unwrap();
        assert!(install(&exe, &new, &old, "").is_err());
        assert_eq!((std::fs::read(&exe).unwrap(), std::fs::read(&old).unwrap()), (b"v1-good".to_vec(), b"prev".to_vec()));
        // після --updated .old лишається, доки UI не пропрацює STABLE_SECS (маркер); звичайний запуск прибирає
        assert_eq!(start_action(true, true, false), StartAction::AwaitStable);
        assert_eq!(start_action(false, true, false), StartAction::OfferRollback, ".old без маркера = збій після оновлення");
        assert_eq!(start_action(false, true, true), StartAction::Clean);
        assert_eq!(start_action(false, false, false), StartAction::Clean);
        std::fs::write(&new, b"x").unwrap();
        cleanup_at(&exe);
        assert!(!old.exists() && !new.exists() && exe.exists());
    }

    #[test]
    fn jar_dependency_lists_are_bounded_and_slugs_safe() {
        // 200 тис. залежностей у fabric.mod.json: розбір швидкий, список обмежений
        let d = tmp_dir("deps_cap");
        let depends: String = (0..200_000).map(|i| format!("\"m{i}\":\"*\"")).collect::<Vec<_>>().join(",");
        let f = d.join("big.jar");
        zip_with(&f, &[("fabric.mod.json", &format!("{{\"version\":\"1\",\"depends\":{{{depends}}}}}"))]);
        let t = std::time::Instant::now();
        let info = read_jar(&f).unwrap();
        assert!(info.deps.len() <= MAX_DEPS_SEEN && t.elapsed() < Duration::from_secs(5), "{} за {:?}", info.deps.len(), t.elapsed());
        // зіпсований/порожній toml і дивні символи не панікують
        for t in ["", "[[dependencies.m]]", "[[dependencies.m]]\nmodId=\nmandatory", "modId = \"x\"\nmandatory=true", "[[dependencies.\n\u{1F600} = \u{1F600}"] {
            let _ = (toml_deps(t), toml_mc(t), toml_version(t));
        }
        assert_eq!(toml_deps("[[dependencies.m]]\nmodId=\"geckolib\" # c\nmandatory=true\n"), vec!["geckolib"]);
        // slug із чужого jar не потрапляє в URL: помилка без запитів
        for bad in ["../user", "a/b", "x?y=1", "", "..", &"a".repeat(65), "кирилиця"] {
            assert!(!slug_ok(bad), "{bad}");
            assert!(modrinth_lookup("t", bad).unwrap_err().contains("Некоректний"));
            assert!(curse_lookup_slug("k", bad).unwrap_err().contains("Некоректний"));
        }
        assert!(slug_ok("fabric-api") && slug_ok("AANobbMI") && slug_ok("a.b_c"));
    }

    #[test]
    fn extra_file_upload_honours_cancel() {
        use std::io::Read;
        let d = tmp_dir("extra_cancel");
        let f = d.join("x.jar");
        std::fs::write(&f, vec![1u8; 10_000]).unwrap();
        let up = Up::default();
        let mut r = CancelRead { f: std::fs::File::open(&f).unwrap(), up: up.clone() };
        let mut buf = [0u8; 100];
        assert_eq!(r.read(&mut buf).unwrap(), 100);
        up.cancel.store(true, Relaxed);
        assert!(r.read(&mut buf).is_err(), "скасування обриває й додатковий файл");
    }

    #[test]
    fn publish_100mb_streams_without_loading_into_memory() {
        // файл 100 МБ (рідкий): мок читає тіло; прогрес доходить до кінця
        let d = tmp_dir("big100");
        let f = d.join("big.jar");
        {
            let mut h = std::fs::File::create(&f).unwrap();
            std::io::Write::write_all(&mut h, b"PK").unwrap();
            h.set_len(100 << 20).unwrap();
        }
        let up = Up::default();
        let t = std::time::Instant::now();
        let (r, seen) = with_mock_secs(3, vec![("GET /v2/project/P/version", 200, "[]"), ("GET /v2/version_file", 404, "{}"), ("POST /v2/version", 200, r#"{"id":"BIG"}"#)], 120, || publish_modrinth("tok", "P", &rel(), &f, &up));
        let secs = t.elapsed().as_secs_f64();
        assert_eq!(seen.len(), 3);
        assert!(r.unwrap().contains("BIG"));
        assert_eq!(up.prog.sent.load(Relaxed), 100 << 20);
        eprintln!("100 МБ на моку: {secs:.2} с ({:.0} МБ/с)", 100.0 / secs);
    }

    #[test]
    fn rollback_of_running_build_and_confirm_marker() {
        let _e = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = tmp_dir("upd_marker");
        unsafe { std::env::set_var("MODWIRE_CONFIG_DIR", &d) };
        let exe = d.join("modwire.exe");
        let (old, bad) = (exe.with_extension("exe.old"), exe.with_extension("exe.bad"));
        std::fs::write(&exe, b"v2-unstable").unwrap();
        std::fs::write(&old, b"v1-good").unwrap();
        assert!(!marker_path().unwrap().exists());
        rollback_running_at(&exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"v1-good");
        assert_eq!(std::fs::read(&bad).unwrap(), b"v2-unstable");
        assert!(!old.exists());
        assert!(rollback_running_at(&exe).is_err(), "без .old відкату немає");
        assert_eq!(std::fs::read(&exe).unwrap(), b"v1-good", "помилка не ламає exe");
        // підтвердження: маркер є, залишки прибрано
        std::fs::write(&old, b"x").unwrap();
        confirm_update_at(&exe);
        assert!(marker_path().unwrap().exists() && !bad.exists() && !old.exists());
        clear_update_marker();
        assert!(!marker_path().unwrap().exists());
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    #[test]
    fn history_repeat_within_5s_is_skipped() {
        let k = "P\u{0}1.0\u{0}Modrinth\u{0}true\u{0}ok";
        let p = Path::new("h.jsonl");
        let prev = [(p.to_path_buf(), "x".to_string(), 99u64), (p.to_path_buf(), k.to_string(), 100u64)];
        assert!(is_repeat(&prev, p, k, 104));
        assert!(!is_repeat(&prev, p, k, 105), "через 5 с це вже новий запис");
        assert!(!is_repeat(&prev, p, "інший", 101));
        assert!(!is_repeat(&prev, Path::new("other.jsonl"), k, 101));
        assert!(!is_repeat(&[], p, k, 101));
        let d = tmp_dir("hist_dedup");
        let f = d.join("history.jsonl");
        for _ in 0..3 {
            history_add_at(&f, "Dup", "1.0", "Modrinth", &Ok("версія X".into()));
        }
        history_add_at(&f, "Dup", "1.0", "CurseForge", &Ok("версія X".into()));
        assert_eq!(std::fs::read_to_string(&f).unwrap().lines().count(), 2, "повтор Modrinth не записано, CurseForge записано");
    }

    #[cfg(windows)]
    #[test]
    fn locked_files_give_clear_messages_on_windows() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = tmp_dir("locked");
        let (exe, new, old) = (d.join("modwire.exe"), d.join("modwire.exe.new"), d.join("modwire.exe.old"));
        let mut b = vec![0u8; 1_200_000];
        b[..2].copy_from_slice(b"MZ");
        std::fs::write(&exe, b"v1-running").unwrap();
        // .new тримає антивірус (share_mode(0) = ексклюзивно): зрозуміле повідомлення, а не «пошкоджений»
        std::fs::write(&new, &b).unwrap();
        {
            let _lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&new).unwrap();
            let e = install(&exe, &new, &old, &"0".repeat(64)).unwrap_err();
            assert!(e.contains("антивірус") && !e.contains("пошкоджений"), "{e}");
            assert_eq!(std::fs::read(&exe).unwrap(), b"v1-running");
        }
        // .old зайнятий: заміна не починається, exe цілий, повідомлення згадує причину
        std::fs::write(&old, b"locked-old").unwrap();
        {
            let _lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&old).unwrap();
            let e = install(&exe, &new, &old, "").unwrap_err();
            assert!(e.contains("Не вдалося замінити програму"), "{e}");
            assert_eq!(std::fs::read(&exe).unwrap(), b"v1-running");
        }
    }

    #[test]
    fn history_survives_odd_files_and_old_rows() {
        let d = tmp_dir("hist_odd");
        let p = d.join("history.jsonl");
        // немає файлу, порожній файл, один рядок
        assert!(history_load_at(&p, 10).is_empty());
        std::fs::write(&p, "").unwrap();
        assert!(history_load_at(&p, 10).is_empty());
        std::fs::write(&p, "{\"t\":1,\"project\":\"A\"}\n").unwrap();
        assert_eq!(history_load_at(&p, 10).len(), 1, "старий рядок без нових полів читається");
        // битий JSON, порожні рядки, сміття і зворотна сумісність (рядки без ok/msg/t) не ламають решту
        std::fs::write(&p, "{\"t\":1,\"project\":\"A\",\"ok\":true,\"msg\":\"x\"}\n{broken\n\n\u{0}\u{0}\n[1,2]\n\"str\"\n{\"project\":\"B\"}\n").unwrap();
        let h = history_load_at(&p, 10);
        let names: Vec<&str> = h.iter().filter_map(|e| e["project"].as_str()).collect();
        assert_eq!(names, ["B", "A"], "новіші першими, непарсовані пропущені");
        // фільтри й CSV на рядках без полів: без паніки
        assert_eq!(history_filter(&h, "b", false).len(), 1);
        assert_eq!(history_filter(&h, "", true).len(), 1, "без ok це не успіх");
        assert!(history_link(&h[0]).is_none());
        let csv = history_csv(&h);
        assert!(csv.starts_with('\u{feff}') && csv.lines().count() == 3, "{csv}");
        assert_eq!(history_csv(&[]).lines().count(), 1, "порожня історія: лише заголовок");
        // межа вікна всередині багатобайтового символа та рядок довший за вікно
        let line = format!("{{\"t\":2,\"project\":\"{}\",\"ok\":true,\"msg\":\"{}\"}}\n", "Я", "ї".repeat(1000));
        let mut body = String::new();
        for _ in 0..400 {
            body += &line;
        }
        std::fs::write(&p, &body).unwrap();
        let h = history_load_at(&p, 5);
        assert!(!h.is_empty() && h.len() <= 5 && h.iter().all(|e| e["project"] == "Я"));
        let huge = format!("{{\"t\":3,\"project\":\"OK\"}}\n{{\"t\":4,\"msg\":\"{}\"}}\n", "z".repeat(400_000));
        std::fs::write(&p, huge).unwrap();
        let h = history_load_at(&p, 5);
        assert!(h.len() <= 2, "надвеликий хвостовий рядок обрізається вікном і пропускається");
    }

    // ----- вимірювання (cargo test --release -- --ignored --nocapture bench_) -----
    fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
        let t = std::time::Instant::now();
        let r = f();
        eprintln!("BENCH {label}: {:?}", t.elapsed());
        r
    }

    #[test]
    #[ignore]
    fn bench_config_and_history() {
        let d = tmp_dir("bench_cfg");
        let mut cfg = Config::default();
        for i in 0..200 {
            cfg.projects.push(Project { name: format!("Проєкт {i}"), modrinth_id: format!("mr{i}"), curse_id: format!("{i}"), icon_url: format!("https://cdn.example/{i}.png"), changelog_template: "x".repeat(300), ..Default::default() });
        }
        cfg.modrinth_token = "t".into();
        cfg.last.draft = Draft { project: "p".into(), number: "1".into(), name: "n".into(), changelog: "c".repeat(20_000), file: "f".into() };
        ensure_ids(&mut cfg);
        let cp = d.join("config.json");
        timed("save_to 200 проєктів + чернетка", || save_to(&cp, &cfg).unwrap());
        timed("save_to повторно (з бекапом)", || save_to(&cp, &cfg).unwrap());
        timed("load_from", || load_from(&cp));
        for n in [2000usize, 20000] {
            let hp = d.join(format!("h{n}.jsonl"));
            let mut s = String::new();
            for i in 0..n {
                s += &format!("{{\"t\":{i},\"project\":\"Проєкт {}\",\"number\":\"1.{i}\",\"platform\":\"Modrinth\",\"ok\":true,\"msg\":\"версія abc https://modrinth.com/project/x/version/abc\"}}\n", i % 50);
            }
            std::fs::write(&hp, s).unwrap();
            let h = timed(&format!("history_load({n} рядків, n=300)"), || history_load_at(&hp, 300));
            timed(&format!("history_filter 300 ({n})"), || history_filter(&h, "проєкт 7", false).len());
            timed(&format!("history_csv {n}"), || history_export_at(&hp, &d.join("o.csv")).unwrap());
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    #[ignore]
    fn bench_files() {
        use std::io::Write;
        let d = tmp_dir("bench_files");
        let jar = d.join("big.jar");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        w.start_file("fabric.mod.json", o).unwrap();
        w.write_all(br#"{"version":"1.0.0","name":"M","depends":{"minecraft":"1.20.1"}}"#).unwrap();
        let blob = vec![7u8; 1 << 20];
        for i in 0..80 {
            w.start_file(format!("assets/blob{i}.bin"), o).unwrap();
            w.write_all(&blob).unwrap();
        }
        for i in 0..5000 {
            w.start_file(format!("com/x/C{i}.class"), o).unwrap();
            w.write_all(b"x").unwrap();
        }
        w.finish().unwrap();
        timed("read_jar 80 МБ / 5000 записів", || read_jar(&jar));
        let mp = d.join("big.mrpack");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&mp).unwrap());
        w.start_file("modrinth.index.json", o).unwrap();
        let files: Vec<String> = (0..2000).map(|i| format!("{{\"path\":\"mods/m{i}.jar\",\"hashes\":{{\"sha1\":\"{i:040}\"}}}}")).collect();
        w.write_all(format!("{{\"versionId\":\"1\",\"name\":\"P\",\"files\":[{}],\"dependencies\":{{\"minecraft\":\"1.20.1\",\"fabric-loader\":\"0.15\"}}}}", files.join(",")).as_bytes()).unwrap();
        for i in 0..40 {
            w.start_file(format!("overrides/o{i}.bin"), o).unwrap();
            w.write_all(&blob).unwrap();
        }
        w.finish().unwrap();
        timed("read_jar mrpack 2000 файлів / 40 МБ", || read_jar(&mp));
        let big = d.join("big.bin");
        let mut f = std::fs::File::create(&big).unwrap();
        for _ in 0..500 {
            f.write_all(&blob).unwrap();
            f.write_all(&blob).unwrap();
        }
        drop(f);
        timed("sha512_file 1000 МБ", || sha512_file(&big).unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unchecked_tokens_warns_softly() {
        assert!(unchecked_tokens(true, true, 5, 5).is_none());
        assert!(unchecked_tokens(false, false, 0, 0).is_none(), "платформа не вибрана");
        let w = unchecked_tokens(true, true, 0, 9).unwrap();
        assert!(w.contains("Modrinth") && !w.contains("CurseForge"), "{w}");
        assert!(unchecked_tokens(true, true, 0, 0).unwrap().contains("Modrinth і CurseForge"));
    }

    #[test]
    fn body_read_failure_gets_network_advice() {
        use std::io::{Read, Write};
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        // сервер обіцяє 1000 байт і обриває з'єднання: помилка читання тіла має нести пораду про мережу
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/x", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 1024];
            let _ = s.read(&mut b);
            s.write_all(b"HTTP/1.1 200 OK
Content-Length: 1000

{").unwrap();
        });
        let c = client_t(5).unwrap();
        let e = get_json(&c, &url, ("x", "y")).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("Немає зв'язку"), "{e}");
    }

    /// Сирий сервер: на кожне з'єднання одна дія (відповідь байтами, пауза перед нею або обрив без відповіді).
    fn raw_server(acts: Vec<(u64, Option<Vec<u8>>)>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            for (delay, resp) in acts {
                let Ok((mut s, _)) = l.accept() else { return };
                let mut b = [0u8; 2048];
                let _ = s.read(&mut b);
                std::thread::sleep(Duration::from_millis(delay));
                if let Some(r) = resp {
                    let _ = s.write_all(&r);
                }
            }
        });
        (url, h)
    }

    fn http(code: u16, extra: &str, body: &str) -> Option<Vec<u8>> {
        Some(format!("HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}", body.len()).into_bytes())
    }

    #[test]
    fn get_retries_5xx_with_retry_after_and_429_then_succeeds() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (url, h) = raw_server(vec![(0, http(503, "Retry-After: 1\r\n", "{}")), (0, http(429, "", "{}")), (0, http(200, "", "{\"ok\":1}"))]);
        let v = get_json(&client_t(5).unwrap(), &url, ("x", "y")).unwrap();
        h.join().unwrap();
        assert_eq!(v["ok"], 1);
    }

    #[test]
    fn get_gives_clear_error_after_persistent_5xx() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (url, h) = raw_server(vec![(0, http(502, "", "{}")), (0, http(502, "", "{}")), (0, http(502, "", "{}"))]);
        let e = get_json(&client_t(5).unwrap(), &url, ("x", "y")).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("Сервіс тимчасово недоступний") && e.contains("502"), "{e}");
    }

    #[test]
    fn get_closed_connection_gets_network_advice() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (url, h) = raw_server(vec![(0, None), (0, None), (0, None)]);
        let e = get_json(&client_t(5).unwrap(), &url, ("x", "y")).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("Немає зв'язку"), "{e}");
    }

    #[test]
    fn get_slow_response_times_out_with_advice() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (url, h) = raw_server(vec![(2500, None), (2500, None), (2500, None)]);
        let e = get_json(&client_t(1).unwrap(), &url, ("x", "y")).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("Немає зв'язку"), "{e}");
    }

    #[test]
    fn invalid_json_and_oversized_body_are_reported() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (url, h) = raw_server(vec![(0, http(200, "", "<html>proxy login</html>"))]);
        let e = get_json(&client_t(5).unwrap(), &url, ("x", "y")).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("Сервер повернув не те"), "{e}");
        let big = "x".repeat(MAX_BODY as usize + 10);
        let (url, h) = raw_server(vec![(0, http(200, "", &big))]);
        let e = get_json(&client_t(10).unwrap(), &url, ("x", "y")).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("завелика"), "{e}");
    }

    #[test]
    fn dead_proxy_gives_network_advice() {
        let c = reqwest::blocking::Client::builder().proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap()).timeout(Duration::from_secs(5)).build().unwrap();
        let e = net_err(c.get("http://example.invalid/").send().unwrap_err());
        assert!(e.contains("проксі") && e.contains("Немає зв'язку"), "{e}");
    }

    #[test]
    fn game_versions_failure_is_a_clean_error_not_a_panic() {
        let (r, _) = with_mock(1, vec![("GET /v2/tag/game_version", 200, "not json")], game_versions);
        assert!(r.unwrap_err().contains("Сервер повернув не те"));
    }
}
