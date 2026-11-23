//! Чиста логіка + мережеві виклики (блокуючі: викликати лише з окремих потоків).
use reqwest::blocking::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;

pub const LOADERS: [&str; 4] = ["fabric", "forge", "neoforge", "quilt"];
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
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
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
    pub projects: Vec<Project>,
    pub last: Last,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

pub const SCHEMA: u32 = 2;

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

/// Для export: прибирає зайві поля, схожі на секрети.
fn strip_secrets(p: &Project) -> Project {
    let mut q = p.clone();
    q.extra.retain(|k, _| {
        let k = k.to_lowercase();
        !["token", "key", "secret", "password", "auth"].iter().any(|w| k.contains(w))
    });
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

/// Дописує результат публікації в history.jsonl (помилки запису ігноруються).
pub fn history_add(project: &str, number: &str, platform: &str, res: &Result<String, String>) {
    if let Some(p) = history_path() {
        history_add_at(&p, project, number, platform, res);
    }
}

fn history_add_at(p: &Path, project: &str, number: &str, platform: &str, res: &Result<String, String>) {
    use std::io::Write;
    let msg = match res { Ok(m) | Err(m) => m };
    let line = json!({"t": now_secs(), "project": project, "number": number, "platform": platform, "ok": res.is_ok(), "msg": msg});
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
    let s = std::fs::read_to_string(p).unwrap_or_default();
    s.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(n).collect()
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
        .filter(|e| q.is_empty() || ["project", "number", "platform", "msg"].iter().any(|k| e[*k].as_str().is_some_and(|s| s.to_lowercase().contains(&q))))
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
    let s = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    format!("\"{}\"", s.replace('"', "\"\"").replace(['\r', '\n'], " "))
}

/// Історія у CSV (від старих до нових): час UTC, проєкт, номер, платформа, результат, повідомлення.
pub fn history_csv(h: &[Value]) -> String {
    let mut out = String::from("\u{feff}time_utc,project,number,platform,result,message\r\n");
    for e in h.iter().rev() {
        let t = e["t"].as_u64().unwrap_or(0);
        let (y, m, d) = ymd(t);
        let when = format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", t % 86400 / 3600, t % 3600 / 60, t % 60);
        let g = |k: &str| csv_cell(e[k].as_str().unwrap_or(""));
        out += &format!("{when},{},{},{},{},{}\r\n", g("project"), g("number"), g("platform"), if e["ok"] == true { "ok" } else { "error" }, g("msg"));
    }
    out
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
    Ok(ps.into_iter().filter(|p| !p.name.trim().is_empty() || !p.modrinth_id.is_empty() || !p.curse_id.is_empty()).collect())
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
        400 => "Сервер відхилив дані (перевірте версії MC, лоадери, номер)",
        401 => "Токен недійсний або без потрібних прав. Створіть новий токен на сайті й вставте його в «Налаштування»",
        403 => "Немає доступу до проєкту (перевірте токен і ID проєкту)",
        404 => "Не знайдено (перевірте ID проєкту)",
        409 | 422 => "Така версія вже існує або дані некоректні",
        413 => "Файл завеликий",
        429 => "Забагато запитів, спробуйте пізніше",
        500..=599 => "Помилка на боці сервера, спробуйте пізніше",
        _ => "Помилка запиту",
    }
}

#[derive(Default, Debug)]
pub struct JarInfo {
    pub version: Option<String>,
    pub name: Option<String>,
    pub loaders: Vec<&'static str>,
    /// Лише точно задані версії MC (діапазони на кшталт ">=1.20" не вгадуються).
    pub mc_versions: Vec<String>,
}

/// "1.20.1", "[1.20.1]" або "=1.20.1" → точна версія MC; діапазони й маски → None.
pub fn mc_exact(s: &str) -> Option<String> {
    let v = s.trim().trim_start_matches('=').trim_matches(|c| c == '[' || c == ']').trim();
    let ok = v.split('.').count() >= 2 && v.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()));
    ok.then(|| v.to_string())
}

fn push_mc(out: &mut Vec<String>, v: Option<String>) {
    if let Some(v) = v.filter(|v| !out.contains(v)) {
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
    }
    if let Some(t) = jar_entry(&mut z, "quilt.mod.json") {
        info.loaders.push("quilt");
        let v: Value = serde_json::from_str(&t).unwrap_or_default();
        info.version = info.version.or(real(v["quilt_loader"]["version"].as_str()));
        info.name = info.name.or(real(v["quilt_loader"]["metadata"]["name"].as_str()));
    }
    for (entry, loader) in [("META-INF/neoforge.mods.toml", "neoforge"), ("META-INF/mods.toml", "forge")] {
        if let Some(t) = jar_entry(&mut z, entry) {
            info.loaders.push(loader);
            info.version = info.version.or(toml_version(&t));
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
    let r = client()?.get(format!("{}/v2/tag/game_version", mr())).send();
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
    for v in s.split(|c: char| c == ',' || c.is_whitespace()).filter(|v| !v.is_empty()) {
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
        "dependencies": [],
        "game_versions": r.mc_versions,
        "version_type": r.release_type,
        "loaders": r.loaders,
        "featured": false,
        "project_id": project_id,
        "file_parts": ["file"],
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
    for l in loaders {
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

fn client() -> Result<Client, String> {
    client_t(300)
}

fn client_t(secs: u64) -> Result<Client, String> {
    Client::builder()
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

fn net_err(e: reqwest::Error) -> String {
    if e.is_timeout() || e.is_connect() {
        format!("Немає зв'язку з сервером (мережа або таймаут). Перевірте інтернет і спробуйте ще раз. ({e})")
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

fn json_of(resp: reqwest::blocking::Response) -> Result<Value, String> {
    let st = resp.status();
    let retry = resp.headers().get("retry-after").and_then(|h| h.to_str().ok()).map(str::to_string);
    let body = resp.text().map_err(|e| e.to_string())?;
    if !st.is_success() {
        let wait = retry.filter(|_| st.as_u16() == 429).map_or(String::new(), |s| format!(" Зачекайте {s} с."));
        return Err(format!("{}.{wait} (HTTP {st}: {})", human(st), api_msg(&body)));
    }
    serde_json::from_str(&body).map_err(|e| format!("некоректна відповідь: {e}"))
}

/// GET із повтором (до 3 спроб) для збоїв мережі, 429 і 5xx. Лише для читання: POST-завантаження не повторюємо.
fn get_json(c: &Client, url: &str, header: (&str, &str)) -> Result<Value, String> {
    let pause = Duration::from_millis(if cfg!(test) { 10 } else { 1500 });
    for attempt in 0..3 {
        let last = attempt == 2;
        match c.get(url).header(header.0, header.1).send() {
            Ok(r) if !last && (r.status().as_u16() == 429 || r.status().is_server_error()) => {}
            Ok(r) => return json_of(r),
            Err(e) if !last && (e.is_timeout() || e.is_connect()) => {}
            Err(e) => return Err(net_err(e)),
        }
        std::thread::sleep(pause * (attempt + 1));
    }
    unreachable!()
}

fn mr_get(c: &Client, token: &str, path: &str) -> Result<Value, String> {
    get_json(c, &format!("{}/v2{path}", mr()), ("Authorization", token))
}

pub fn modrinth_user(token: &str) -> Result<(String, String), String> {
    parse_user(&mr_get(&client()?, token, "/user")?).ok_or_else(|| "немає id користувача".into())
}

/// Перевірка upload-токена: ендпоінт версій вимагає чинний X-Api-Token.
pub fn curse_token_check(token: &str) -> Result<String, String> {
    let r = client()?.get(format!("{}/api/game/version-types", cf())).header("X-Api-Token", token).send();
    json_of(r.map_err(net_err)?).map(|_| "токен приймається".to_string())
}

/// Перевірка Core API-ключа запитом про гру Minecraft (id 432).
pub fn curse_key_check(key: &str) -> Result<String, String> {
    let r = client()?.get(format!("{}/v1/games/432", core())).header("x-api-key", key).send();
    json_of(r.map_err(net_err)?).map(|_| "ключ приймається".to_string())
}

pub fn modrinth_projects(token: &str) -> Result<Vec<MrProject>, String> {
    let c = client()?;
    let (id, _) = parse_user(&mr_get(&c, token, "/user")?).ok_or("немає id користувача")?;
    Ok(parse_projects(&mr_get(&c, token, &format!("/user/{id}/projects"))?))
}

/// (назва, url іконки) проєкту Modrinth за id/slug.
pub fn modrinth_project_info(token: &str, id: &str) -> Result<(String, String), String> {
    let v = mr_get(&client_t(20)?, token, &format!("/project/{id}"))?;
    Ok((v["title"].as_str().ok_or("немає назви проєкту")?.into(), v["icon_url"].as_str().unwrap_or("").into()))
}

/// Оновлені (modrinth_id, curse_id, назва, іконка) для проєктів, що відповіли; помилки окремих проєктів пропускаються.
pub fn refresh_projects(mr_token: &str, core_key: &str, ps: &[Project]) -> Vec<(String, String, String, String)> {
    ps.iter()
        .filter_map(|p| {
            let info = if !p.modrinth_id.is_empty() && !mr_token.is_empty() {
                modrinth_project_info(mr_token, &p.modrinth_id).ok()
            } else if !p.curse_id.is_empty() && !core_key.is_empty() {
                curse_mod_info(core_key, &p.curse_id).ok()
            } else {
                None
            }?;
            Some((p.modrinth_id.clone(), p.curse_id.clone(), info.0, info.1))
        })
        .collect()
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
    let r = client()?.get(format!("{}/v1/mods/{id}", core())).header("x-api-key", key).send();
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
    if url.is_empty() || p.exists() {
        return;
    }
    let _ = (|| -> Result<(), String> {
        let bytes = client()?.get(url).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes()).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        write_atomic(&p, &bytes).map_err(|e| e.to_string())
    })();
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
    let existing = mr_get(&client()?, token, &format!("/project/{project_id}/version"))?;
    if existing.as_array().is_some_and(|a| a.iter().any(|v| v["version_number"] == r.number.trim() && v["loaders"].as_array().is_some_and(|l| l.iter().any(|x| r.loaders.iter().any(|y| x == y.as_str()))))) {
        return Err(format!("версія {} для цього лоадера вже є на Modrinth, пропущено (змініть номер, якщо це нова збірка)", r.number.trim()));
    }
    let data = modrinth_data(r, project_id).to_string();
    let v = post_upload(&format!("{}/v2/version", mr()), ("Authorization", token), || Ok(multipart::Form::new().part("data", json_part(&data)?).part("file", file_part(file, up)?)), up)?;
    match v["id"].as_str() {
        Some(id) => Ok(format!("версія {id} https://modrinth.com/project/{project_id}/version/{id}")),
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
    let c = client_t(20)?;
    let v = get_json(&c, &format!("https://api.github.com/repos/{repo}/releases/latest"), ("accept", "application/vnd.github+json"))?;
    Ok(parse_release(&v, repo, env!("CARGO_PKG_VERSION")))
}

/// Видаляє залишок попереднього оновлення (modwire.exe.old). Помилку ігноруємо: файл міг ще тримати старий процес.
pub fn cleanup_update() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
        let _ = std::fs::remove_file(exe.with_extension("exe.new"));
    }
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

fn download_to(url: &str, dest: &Path, size: u64) -> Result<(), String> {
    use std::io::{Read, Write};
    let c = client_t(900)?;
    let resp = c.get(url).send().map_err(net_err)?;
    if !resp.status().is_success() {
        return Err(format!("Сервер відповів {}", resp.status()));
    }
    let res = (|| {
        let mut out = std::fs::File::create(dest).map_err(|e| format!("Не вдалося записати файл оновлення поруч із програмою: {e}"))?;
        let n = std::io::copy(&mut resp.take(UPDATE_MAX + 1), &mut out).map_err(|e| e.to_string())?;
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
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let (new, old) = (exe.with_extension("exe.new"), exe.with_extension("exe.old"));
    check_writable(exe.parent().ok_or("немає теки програми")?)?;
    download_to(url, &new, size)?;
    if !sha256.is_empty() && sha256_file(&new).map_or(true, |h| !h.eq_ignore_ascii_case(sha256)) {
        let _ = std::fs::remove_file(&new);
        return Err("Контрольна сума завантаженого файлу не збігається з релізом; оновлення скасовано".into());
    }
    if !valid_exe(&new) {
        let _ = std::fs::remove_file(&new);
        return Err("Завантажений файл пошкоджений або не є програмою Windows".into());
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("Не вдалося замінити програму: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::remove_file(&new);
        if std::fs::rename(&old, &exe).is_err() {
            return Err(format!("Не вдалося замінити програму: {e}. Якщо modwire.exe зник, перейменуйте {} назад у modwire.exe", old.display()));
        }
        return Err(format!("Не вдалося замінити програму: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel() -> Release {
        Release {
            name: "".into(),
            number: "1.0.0".into(),
            mc_versions: vec!["1.20.1".into()],
            loaders: vec!["fabric".into()],
            release_type: "release".into(),
            changelog: "ok".into(),
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

    fn mock(n: usize, routes: Vec<(&'static str, u16, &'static str)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        l.set_nonblocking(true).unwrap();
        let h = std::thread::spawn(move || {
            let mut seen = vec![];
            // не більше n з'єднань і не довше 5 с: якщо клієнт відкрив менше, тест падає, а не висить
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
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
                while got < len {
                    match s.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => got += k,
                    }
                }
                let line = head.lines().next().unwrap_or("").to_string();
                let (code, body) = routes.iter().find(|(k, _, _)| line.starts_with(k)).map_or((404, "{}"), |r| (r.1, r.2));
                let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                seen.push(head.clone());
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
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (url, h) = mock(n, routes);
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
        let (r, seen) = with_mock(2, vec![("GET /v2/project/P/version", 200, "[]"), ("POST /v2/version", 200, r#"{"id":"VER1"}"#)], || {
            publish_modrinth("tok", "P", &rel(), &jar(), &up)
        });
        let ok = r.unwrap();
        assert!(ok.contains("VER1") && ok.contains("https://modrinth.com/project/P/version/VER1"), "{ok}");
        assert_eq!(seen.len(), 2);
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
        let (r, _) = with_mock(2, vec![("GET /v2/project/P/version", 200, other), ("POST /v2/version", 200, r#"{"id":"V2"}"#)], || {
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
            5,
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
        let (r, seen) = with_mock(3, vec![("GET /v2/project/P/version", 200, "[]"), ("POST /v2/version", 429, "{}")], || publish_modrinth("t", "P", &rel(), &f, &Up::default()));
        assert!(r.unwrap_err().contains("Забагато"));
        assert_eq!(seen.len(), 3, "GET + POST + один повтор POST");
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
        let d = std::env::temp_dir().join(format!("mp_cfgdir_{}", std::process::id()));
        // shortcut: set_var глобальний; безпечно, бо інші тести не читають цю змінну поза SERIAL
        unsafe { std::env::set_var("MODWIRE_CONFIG_DIR", &d) };
        let (cfg, hist, icon) = (config_path(), history_path(), icon_path("http://x/y.png"));
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
        assert_eq!(cfg.unwrap(), d.join("config.json"));
        assert_eq!(hist.unwrap(), d.join("history.jsonl"));
        assert!(icon.unwrap().starts_with(d.join("icons")));
    }
}
