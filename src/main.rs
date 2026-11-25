#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod ads;
mod i18n;
mod logic;
mod shots;
mod splash;

use eframe::egui::{self, Color32, RichText, Stroke};
use logic::*;
use std::sync::atomic::Ordering::Relaxed;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

// Палітра: темна, з двома фірмовими акцентами платформ.
const BG: Color32 = Color32::from_rgb(0x11, 0x11, 0x13);
const SURFACE: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x24);
const FIELD: Color32 = Color32::from_rgb(0x26, 0x27, 0x2d);
const HOVER: Color32 = Color32::from_rgb(0x2c, 0x2d, 0x33);
const ACTIVE: Color32 = Color32::from_rgb(0x35, 0x36, 0x3d);
const UI_BORDER: Color32 = Color32::from_rgb(0x6e, 0x70, 0x80);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x3a);
const TEXT: Color32 = Color32::from_rgb(0xee, 0xef, 0xf4);
const MUTED: Color32 = Color32::from_rgb(0x9a, 0x9d, 0xaa);
// Акцент інтерфейсу = бурштин логотипа Modwire; зелений лишається лише за Modrinth (MR_GREEN), щоб колір платформи не плутався з кольором застосунку
const ACCENT: Color32 = BRAND;
const BRAND: Color32 = Color32::from_rgb(0xe8, 0xa5, 0x4b);
const BRAND_INK: Color32 = Color32::from_rgb(0x15, 0x12, 0x0e);
const ON_ACCENT: Color32 = BRAND_INK;
// Ролі кольору: бурштин (ACCENT) лише головна дія й «ви тут»; фокус клавіатури завжди світлий (FOCUS) і не схожий на виділення;
// посилання блідо-блакитні й підкреслені, не бурштинові; зелений і помаранчевий лише платформи.
const FOCUS: Color32 = TEXT;
const LINK: Color32 = Color32::from_rgb(0xa9, 0xc7, 0xf0);
const SUCCESS: Color32 = Color32::from_rgb(0x47, 0xe9, 0xb5);
// Фірмові кольори платформ для картки проєкту (як у класичній версії): зелений і помаранчевий
const MR_GREEN: Color32 = Color32::from_rgb(0x1b, 0xd9, 0x6a);
const MR_BG: Color32 = Color32::from_rgb(0x17, 0x3d, 0x2b);
const CF_ORANGE: Color32 = Color32::from_rgb(0xf1, 0x64, 0x36);
const CF_BG: Color32 = Color32::from_rgb(0x3b, 0x24, 0x20);
const CF_COL: Color32 = Color32::from_rgb(0xd8, 0xc9, 0xb4); // CurseForge: тепла пісочна крапка, відмінна від WARN/ACCENT/ERROR
const WARN: Color32 = Color32::from_rgb(0xe0, 0x7a, 0x4f);
const ERROR: Color32 = Color32::from_rgb(0xe8, 0x70, 0x6a);
const DANGER: Color32 = Color32::from_rgb(0xb8, 0x45, 0x3f);
const HINT_SZ: f32 = 13.0;
const RAIL_W: f32 = 200.0;
const CARD_R: u8 = 16; // контейнери
const R_CTRL: u8 = 8; // контроли
const R_BADGE: u8 = 6; // бейджі
const CARD_PAD: i8 = 16; // внутрішній відступ картки
const STEP_DOT: f32 = 24.0; // діаметр кола кроку
const STEP_LINE: f32 = 2.0; // товщина з'єднувача кроків
const PROJECT_CARD_H: f32 = 132.0;
const CARD_MIN_W: f32 = 270.0;
const GAP: f32 = 16.0;
const CONTENT_MAX_W: f32 = 1100.0; // на широких вікнах контент не розтягується далі, а центрується
const CTRL_H: f32 = 36.0; // кнопки, чипи, поля вводу
const TOUCH_H: f32 = 28.0; // мінімум для клікабельних рядків, посилань і прапорців
const PROG_H: f32 = 8.0; // смуга прогресу
const PROG_R: u8 = 4;
const HIST_MAX: usize = 300; // скільки записів історії тримаємо в пам'яті

#[derive(PartialEq, Clone, Copy)]
#[repr(u8)]
enum Tab {
    Projects,
    Publish,
    Settings,
}

#[derive(Clone, Copy)]
enum P {
    Mr,
    Cf,
}

enum Msg {
    List(Result<Vec<MrProject>, String>),
    McList(Result<Vec<(String, bool)>, String>),
    Check(Result<String, String>),
    CheckCf(Result<String, String>),
    CheckKey(Result<String, String>),
    CheckProj(Result<String, String>), // перевірка проєкту CurseForge через Core API
    CfInfo(String, Option<usize>, Result<(String, String), String>),
    Found(Option<usize>, Result<Box<Project>, String>), // проєкт знайдено за посиланням/slug
    CfSearch(Result<Vec<CfHit>, String>), // пошук CurseForge за назвою
    Done(P, Result<String, String>),
    File(PathBuf, Option<JarInfo>), // jar читається у фоновому потоці: великий файл/мережевий диск не гальмує кадр
    ExtraFile(PathBuf), // додатковий файл версії (Modrinth)
    Changelog(String),
    Notice(String),
    Imported(Vec<Project>),
    Refreshed(Vec<(String, String, String, String)>),
    Update(Result<Option<Update>, String>, bool), // bool: перевірку запустив користувач
    Updated(Result<(), String>),
    Panicked, // фоновий потік аварійно завершився: знімаємо прапорці «зайнято»
}

struct App {
    cfg: Config,
    tab: Tab,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    // діалог додавання
    add_open: bool,
    target: Option<usize>,
    mr_list: Vec<MrProject>,
    mr_loading: bool,
    mr_msg: String, // помилка завантаження списку Modrinth, показується під кнопкою
    cf_id: String,
    cur_name: String,                // назва версії поточної публікації (для історії)
    proj_chk: Chk,                   // результат перевірки проєкту CurseForge
    featured: bool,                  // Modrinth: рекомендована версія
    send_deps: bool,                 // Modrinth: надсилати залежності з файлу
    extra_files: Vec<PathBuf>,       // Modrinth: додаткові файли (до MAX_EXTRA)
    sel_mode: bool,                  // режим вибору для пакетного видалення
    picked: std::collections::HashSet<String>, // id вибраних проєктів
    batch_confirm: bool,
    merge_ask: Vec<(usize, Project)>, // черга питань про однакову назву
    centered: bool,                  // вікно вже центровано після першого кадру
    add_msg: String,
    add_tab: u8,              // вкладка вікна «Додати проєкт»: 0 Modrinth, 1 CurseForge
    cf_query: String,         // пошук CurseForge за назвою
    cf_hits: Vec<CfHit>,
    cf_loading: bool,
    // публікація
    sel: Option<usize>,
    file: Option<PathBuf>,
    name: String,
    number: String,
    release_type: usize,
    changelog: String,
    use_mr: bool,
    use_cf: bool,
    mr_res: Option<Result<String, String>>,
    cf_res: Option<Result<String, String>>,
    pending: u8,
    msg: String,
    // налаштування
    confirm_del: Option<usize>,
    confirm_clear: bool,
    show_tokens: bool,
    check_msg: Chk,
    notice: String,
    update: Option<Update>,
    upd_checking: bool,
    upd_applying: bool,
    refreshing: bool,
    upd_err: String,
    upd_started: bool,
    quit: bool, // після оновлення: нова копія вже запущена, закриваємося
    notice_seen: Option<(String, std::time::Instant)>, // текст і момент появи, для автозникання
    up_mr: Up, // прогрес і скасування завантаження
    up_cf: Up,
    cur_cl: String, // знімок changelog на момент старту публікації
    cur_meta: (String, Vec<String>, String), // знімок MC/лоадерів/типу релізу на момент старту
    readonly: bool,        // конфіг не читався: не перезаписувати
    adding: u8,            // запити додавання CurseForge в дорозі
    cf_auto_off: bool,     // галочку CurseForge зняв .mrpack, а не користувач
    cur_idx: Option<usize>, // проєкт поточної публікації
    history: Vec<serde_json::Value>,
    hist_q: String,       // пошук в історії
    proj_q: String,       // пошук у списку проєктів (з'являється, коли їх більше 6)
    hist_fail: bool,      // лише помилки
    hist_all: bool,       // показати більше 10
    hist_confirm: bool,   // підтвердження очищення
    cur: (String, String), // (проєкт, номер) поточної публікації для історії
    mc_known: Vec<String>, // релізи для чипів
    mc_all: Vec<String>,   // усі версії для перевірки
    mc_loading: bool,
    mc_next: Option<std::time::Instant>,
    cf_msg: Chk,
    key_msg: Chk,
    ver_open: bool, // вікно вибору версій Minecraft
    ver_q: String,  // пошук у ньому
    ver_filter: u8, // 0 релізи, 1 знімки, 2 усі
    upd_drawn: bool, // банер оновлення вже намальовано в цьому кадрі
    set_sec: usize, // розділ налаштувань: 0 записи, 1 дані, 2 оновлення, 3 безпека, 4 про застосунок
    pub_step: usize, // активна вкладка публікації: 0..=5 — кроки, 6 — історія
    open_steps: [bool; 6], // розгорнуті кроки публікації (згортає лише користувач)
    start_act: StartAction,                    // сліди оновлення на старті
    stable_at: Option<std::time::Instant>,     // коли нова збірка вважатиметься стабільною
    confirm_close: bool,  // показано діалог «Публікація триває»
    close_ok: bool,       // користувач підтвердив вихід під час публікації
    draft_at: Option<std::time::Instant>, // коли змінилась чернетка форми (запис на диск із затримкою)
    merge_hidden: bool, // діалог «Однакова назва» закрито Esc без рішення
    add_shown: bool,
    file_info: Option<JarInfo>, // метадані файлу для попередження про несумісність
    file_note: String,          // що підставлено з файлу (показується в кроці «Файл»)
    review_now: bool, // чи цього кадру показано зведення (права картка тоді не дублює його рядки)
    edit_mode: bool, // true = покроковий майстер; false лише після вибору файлу, коли форма вже повна: тоді зведення (не перехоплює форму посеред набору)
    ctrl_armed: Option<(std::time::Instant, Option<usize>)>, // перше Ctrl+Enter на «Публікації» (час, проєкт): друге за 2 с для того ж проєкту відправляє
    splash: splash::Splash,     // заставка на старті
    brand_rect: Option<egui::Rect>, // де в рейці маленька іконка (куди «влітає» заставка)
    focus_step: Option<usize>,  // крок, до якого треба прокрутити (клік на «Ще не готово»)
    focus_hist: bool,           // прокрутити до історії (кнопка після успішної публікації)
}

/// Стан перевірки токена.
#[derive(Default, Clone, PartialEq, Debug)]
enum Chk {
    #[default]
    Idle,
    Checking,
    Ok(String),
    Err(String),
    Stamp(u64), // «Перевірено N тому»: час (epoch, с), текст щоразу обчислюється наново
}

/// Стан «перевірено N тому» зі збереженої відмітки часу (0 = не перевірялось).
fn checked_msg(t: u64) -> Chk {
    if t == 0 { Chk::Idle } else { Chk::Stamp(t) }
}

/// « · 2.5 МБ/с · ще ~8 с» для рядка прогресу завантаження.
/// Закриття вікна треба підтвердити, якщо йде публікація і вихід ще не підтверджено.
fn must_ask_close(requested: bool, pending: u8, confirmed: bool) -> bool {
    requested && pending > 0 && !confirmed
}

fn speed_text(bps: u64, eta: u64) -> String {
    format!(" · {:.1} МБ/с · ще ~{} с", bps as f64 / 1_048_576.0, eta)
}

fn status(ui: &mut egui::Ui, c: &Chk) {
    match c {
        Chk::Idle => {}
        Chk::Stamp(t) => {
            icon_label_c(ui, Ic::Check, SUCCESS, &i18n::fmt(i18n::tr("Перевірено {}"), &[&(ago(*t))]), SUCCESS);
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(60)); // текст «N хв тому» оновлюється раз на хвилину
        }
        Chk::Checking => {
            ui.spinner();
            ui.label(RichText::new(i18n::tr("status.checking")).color(MUTED));
        }
        Chk::Ok(m) | Chk::Err(m) => {
            let ok = matches!(c, Chk::Ok(_));
            // повний текст з переносом рядків: за обрізаного повідомлення не видно відповіді сервера («HTTP 401: …»)
            let (ic, col) = if ok { (Ic::Check, SUCCESS) } else { (Ic::Cross, ERROR) };
            let r = ui.scope(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (rect, ir) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    spoken_icon(&ir, ic);
                    draw_icon(ui.painter(), rect.center(), ic, 16.0, col);
                    ui.add(egui::Label::new(RichText::new(m).color(col)).wrap());
                });
            });
            ui.ctx().accesskit_node_builder(r.response.id, |n| n.set_live(egui::accesskit::Live::Polite)); // результат перевірки озвучується
        }
    }
}

/// Одна тиха спонсорська плитка з позначкою «Реклама» внизу списку проєктів.
fn sponsor_tile(ui: &mut egui::Ui) {
    let Some(s) = ads::SPONSOR else { return };
    ui.add_space(8.0);
    // приглушена смуга без фону: не конкурує з картками й головною дією
    egui::Frame::new().stroke(Stroke::new(1.0, BORDER)).corner_radius(egui::CornerRadius::same(CARD_R)).inner_margin(egui::Margin::symmetric(CARD_PAD, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            badge(ui, i18n::tr("Реклама"), MUTED);
            ui.label(RichText::new(format!("{} · {}", s.title, s.text)).color(MUTED).size(HINT_SZ));
            let r = ui.add(egui::Button::new(RichText::new(s.cta).color(LINK).underline().size(HINT_SZ)).frame(false).min_size(egui::vec2(0.0, TOUCH_H)));
            if r.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(s.url));
            }
        });
    });
}

/// Відокремлює посилання (слово, що починається з https://) від тексту результату.
fn split_url(s: &str) -> (String, Option<String>) {
    let url = s.split_whitespace().find(|w| w.starts_with("https://")).map(String::from);
    let txt = s.split_whitespace().filter(|w| !w.starts_with("https://")).collect::<Vec<_>>().join(" ");
    (txt, url)
}

/// Діалог вибору файлу змін; бере першу секцію (останню версію).
fn pick_changelog(dir: String) -> Option<Msg> {
    let mut d = rfd::FileDialog::new().add_filter(i18n::tr("Опис змін"), &["md", "txt"]);
    if !dir.is_empty() {
        d = d.set_directory(dir);
    }
    let text = read_text_capped(&d.pick_file()?, 800_000).ok()?;
    Some(Msg::Changelog(first_section(&text.chars().take(200_000).collect::<String>())))
}

fn pick_file(dir: String) -> Option<Msg> {
    let mut d = rfd::FileDialog::new().add_filter(i18n::tr("Мод"), &["jar", "zip", "mrpack"]);
    if !dir.is_empty() {
        d = d.set_directory(dir);
    }
    d.pick_file().map(|f| {
        let info = read_jar(&f);
        Msg::File(f, info)
    })
}

fn pick_extra(dir: String) -> Option<Msg> {
    let mut d = rfd::FileDialog::new().add_filter(i18n::tr("Додатковий файл (sources, dev-jar)"), &["jar", "zip"]);
    if !dir.is_empty() {
        d = d.set_directory(dir);
    }
    d.pick_file().map(Msg::ExtraFile)
}

fn setup_style(ctx: &egui::Context) {
    // Inter (OFL) — найближчий вільний аналог шрифту iPhone, має кирилицю
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("inter".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Inter.ttf"))));
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "inter".into());
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = FIELD;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = LINK;
    v.warn_fg_color = WARN;
    v.menu_corner_radius = egui::CornerRadius::same(CARD_R);
    v.weak_text_color = Some(MUTED); // колір підказок у полях
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    v.error_fg_color = ERROR;
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.window_corner_radius = egui::CornerRadius::same(CARD_R);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.20);
    v.selection.stroke = Stroke::new(2.0, ACCENT);
    let r = egui::CornerRadius::same(R_CTRL);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, SURFACE),
        (&mut v.widgets.inactive, FIELD),
        (&mut v.widgets.hovered, HOVER),
        (&mut v.widgets.active, ACTIVE),
    ] {
        w.corner_radius = r;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, BORDER);
    }
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, UI_BORDER);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.open.bg_fill = HOVER;
    v.widgets.open.weak_bg_fill = HOVER;
    for w in [&mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.expansion = 0.0; // без «стрибка» розміру при наведенні
    }
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    v.widgets.active.fg_stroke = Stroke::new(1.5, ACCENT);
    v.slider_trailing_fill = true;
    ctx.set_theme(egui::ThemePreference::Dark); // застосунок лише темний: світла тема egui мала б синій дефолт
    ctx.set_visuals(v);

    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (egui::TextStyle::Heading, egui::FontId::proportional(28.0)),
            (egui::TextStyle::Body, egui::FontId::proportional(15.0)),
            (egui::TextStyle::Button, egui::FontId::proportional(15.0)),
            (egui::TextStyle::Small, egui::FontId::proportional(12.5)),
            (egui::TextStyle::Name("Title".into()), egui::FontId::proportional(18.0)),
            (egui::TextStyle::Monospace, egui::FontId::monospace(14.0)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.scroll = egui::style::ScrollStyle::solid();
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.button_padding = egui::vec2(14.0, 7.0);
        s.spacing.interact_size.y = CTRL_H;
    });
}

// ---------- Базові елементи ----------

/// Підказка під заголовком або полем: завжди читабельний колір.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(MUTED).size(HINT_SZ));
}

/// Підпис над полем: одразу видно, що саме вводити.
fn field_label(ui: &mut egui::Ui, text: &str) -> egui::Id {
    ui.label(RichText::new(text).color(TEXT).strong().size(14.0)).id // для labelled_by: скрінрідер читає підпис поля
}

/// Фокус повертається на елемент, що відкрив діалог: запам'ятовуємо його в кадр відкриття (до того, як діалог забере фокус)
/// і повертаємо в кадр закриття.
fn dialog_focus(ctx: &egui::Context, key: &'static str, open: bool) {
    let id = egui::Id::NULL.with(("dlg_focus", key));
    let st: Option<(bool, Option<egui::Id>)> = ctx.data(|d| d.get_temp(id));
    match (open, st) {
        (true, None | Some((false, _))) => {
            let f = ctx.memory(|m| m.focused());
            ctx.data_mut(|d| d.insert_temp(id, (true, f)));
        }
        (false, Some((true, saved))) => {
            if let Some(f) = saved {
                ctx.memory_mut(|m| m.request_focus(f));
            }
            ctx.data_mut(|d| d.insert_temp(id, (false, None::<egui::Id>)));
        }
        _ => {}
    }
}

/// Людська назва кнопки для скрінрідера (дієслово + об'єкт), коли видимий напис короткий або однаковий на кількох картках.
fn aria(r: egui::Response, name: impl Into<String>) -> egui::Response {
    let (enabled, name) = (r.enabled(), name.into());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name.clone()));
    r
}

/// Те саме для перемикачів: назва і стан «натиснуто/вибрано» (Role::CheckBox).
fn aria_check(r: egui::Response, name: impl Into<String>, on: bool) -> egui::Response {
    let (enabled, name) = (r.enabled(), name.into());
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, name.clone()));
    r
}

/// Назва для скрінрідера поля без видимого підпису (пошук): текст поля, назва й підказка.
fn name_edit(r: &egui::Response, name: &str, hint: &str, text: &str) {
    r.widget_info(|| {
        let mut i = egui::WidgetInfo::text_edit(true, text, text, hint);
        i.label = Some(name.to_string());
        i
    });
}

/// Заголовок блоку (стиль Title).
/// Смуга прогресу з видимою доріжкою (BORDER): стандартна зливалася з FIELD на картках.
fn progress_bar(ui: &mut egui::Ui, f: f32, w: f32, col: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, PROG_H), egui::Sense::hover());
    ui.painter().rect_filled(rect, PROG_R, BORDER);
    if f > 0.0 {
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2((rect.width() * f.clamp(0.0, 1.0)).max(PROG_H), PROG_H));
        ui.painter().rect_filled(fill, PROG_R, col);
    }
}

fn title(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).text_style(egui::TextStyle::Name("Title".into())).strong().color(TEXT)
}

/// Випадний список з намальованою стрілкою.
fn combo_box(salt: &str) -> egui::ComboBox<'_> {
    egui::ComboBox::from_id_salt(salt).icon(|ui, rect, vis, _open| {
        draw_icon(ui.painter(), rect.center(), Ic::Chevron, 12.0, vis.fg_stroke.color);
    })
}

fn link(ui: &mut egui::Ui, help: &str, url: &str) {
    let resp = ui.add(egui::Button::new(RichText::new(i18n::tr("Як отримати")).color(LINK).underline()).frame(false).min_size(egui::vec2(0.0, TOUCH_H)));
    let resp = aria(resp, i18n::tr("Як отримати: показати інструкцію"));
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(320.0)
        .show(|ui| {
            ui.label(help);
            if icon_btn(ui, Some(Ic::External), i18n::tr("Відкрити сторінку"), true).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        });
}

/// Суцільна кнопка з анімованими станами: hover світлішає, press темнішає, disabled тьмяніє,
/// фокус — кільце ззовні з проміжком (не зливається із заливкою).
struct Solid {
    text: String,
    fill: Color32,
    fg: Color32,
    min: egui::Vec2,
}

impl Solid {
    fn min_size(mut self, v: egui::Vec2) -> Self {
        self.min = v;
        self
    }
}

impl egui::Widget for Solid {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        btn_paint(ui, None, &self.text, BtnKind::Solid(self.fill, self.fg), self.min.x, self.min.y.max(CTRL_H))
    }
}

/// Вигляд кнопки: заливна (колір і колір тексту), з рамкою на тлі поля, або прозора з рамкою.
#[derive(Clone, Copy)]
enum BtnKind {
    Solid(Color32, Color32),
    Field,
    Ghost,
}

/// Єдина відмальовка всіх кнопок: hover світлішає за 120 мс, press темнішає, disabled тьмяніє,
/// фокус — кільце 2px (для заливних ззовні з проміжком). Розмір не залежить від стану: без стрибків.
fn btn_paint(ui: &mut egui::Ui, ic: Option<Ic>, label: &str, kind: BtnKind, min_w: f32, h: f32) -> egui::Response {
    let enabled = ui.is_enabled();
    let txt_col = match kind {
        BtnKind::Solid(_, fg) => fg,
        _ => TEXT,
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), egui::TextStyle::Button.resolve(ui.style()), txt_col);
    let icw = if ic.is_some() { 22.0 } else { 0.0 };
    let content_w = galley.size().x + icw;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2((content_w + 28.0).max(min_w), h), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let t = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered() && enabled, 0.12);
    let down = enabled && resp.is_pointer_button_down_on();
    let (fill, stroke, col) = match kind {
        BtnKind::Solid(base, fg) if enabled => {
            let f = if down { base.lerp_to_gamma(Color32::BLACK, 0.16) } else { base.lerp_to_gamma(Color32::WHITE, 0.12 * t) };
            (f, Stroke::NONE, fg)
        }
        BtnKind::Solid(..) => (FIELD, Stroke::NONE, MUTED.gamma_multiply(0.8)),
        _ if !enabled => (if matches!(kind, BtnKind::Field) { FIELD.gamma_multiply(0.5) } else { Color32::TRANSPARENT }, Stroke::new(1.0, BORDER), MUTED.gamma_multiply(0.7)),
        _ => {
            let rest = if matches!(kind, BtnKind::Field) { FIELD } else { Color32::TRANSPARENT };
            let f = if down { ACTIVE } else { rest.lerp_to_gamma(HOVER, t) };
            (f, Stroke::new(1.0, UI_BORDER.lerp_to_gamma(ACCENT.gamma_multiply(0.6), t)), TEXT)
        }
    };
    let p = ui.painter();
    p.rect(rect, R_CTRL, fill, stroke, egui::StrokeKind::Inside);
    if resp.has_focus() {
        match kind {
            BtnKind::Solid(..) => p.rect_stroke(rect.expand(2.0), R_CTRL + 2, Stroke::new(2.0, TEXT), egui::StrokeKind::Outside),
            _ => p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside),
        };
    }
    let x0 = rect.center().x - content_w / 2.0;
    if let Some(k) = ic {
        draw_icon(p, egui::pos2(x0 + 7.0, rect.center().y), k, 14.0, col);
    }
    p.galley(egui::pos2(x0 + icw, rect.center().y - galley.size().y / 2.0), galley, col);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

fn primary(text: &str) -> Solid {
    Solid { text: text.to_owned(), fill: ACCENT, fg: ON_ACCENT, min: egui::vec2(0.0, CTRL_H) }
}

fn danger(text: &str) -> Solid {
    Solid { text: text.to_owned(), fill: DANGER, fg: Color32::WHITE, min: egui::vec2(0.0, CTRL_H) }
}

fn card<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    card_r(ui, f).inner
}

/// Картка, що підсвічує рамку бурштином при наведенні курсора (без тіні й зсуву).
fn card_hover<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let r = card_r(ui, f);
    let t = ui.ctx().animate_bool_with_time(ui.scope_id().with(("card_hov", r.response.rect.min.x as i32, r.response.rect.min.y as i32)), ui.rect_contains_pointer(r.response.rect), 0.15);
    ui.painter().rect_stroke(r.response.rect, CARD_R, Stroke::new(1.0, ACCENT.gamma_multiply(0.45 * t)), egui::StrokeKind::Inside);
    r.inner
}

fn card_r<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    let frame = egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(egui::CornerRadius::same(CARD_R))
        .inner_margin(egui::Margin::same(CARD_PAD));
    // vertical: у ui.columns розкладка «justified» розтягувала пробіли в тексті, що переноситься
    let r = frame.show(ui, |ui| ui.vertical(f).inner);
    // глибина без тіні: ледь помітний світлий кант угорі, як у підсвіченого краю
    let rc = r.response.rect;
    let y = rc.top() + 1.0;
    ui.painter().line_segment([egui::pos2(rc.left() + CARD_R as f32, y), egui::pos2(rc.right() - CARD_R as f32, y)], Stroke::new(1.0, TEXT.gamma_multiply(0.06)));
    r
}

/// Крок форми: номер (або галочка) у колі + заголовок. Заповнений крок користувач може згорнути
/// до рядка-підсумку; незаповнений обов'язковий крок завжди розгорнутий.
#[derive(Clone, Copy)]
struct StepSpec<'a> {
    n: usize,
    name: &'a str,
    done: bool,
    required: bool,
    summary: &'a str,
    scroll: bool, // прокрутити до цього кроку й розгорнути
    tabbed: bool, // крок показано як окрему вкладку: без згортання й з'єднувача
}

/// Вкладка платформи у вікні додавання: крапка кольору платформи й назва; вибрана — світліша плитка в рамці.
fn add_tab_btn(ui: &mut egui::Ui, on: bool, name: &str, col: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, name.to_string()));
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 9, Color32::from_rgb(0x26, 0x27, 0x2d));
        p.rect_stroke(rect, 9, Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3c, 0x4a)), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, 9, HOVER.gamma_multiply(0.6));
    }
    if resp.has_focus() {
        p.rect_stroke(rect, 9, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
    }
    let g = p.layout_no_wrap(name.to_owned(), egui::FontId::proportional(14.0), if on { TEXT } else { MUTED });
    let x0 = rect.center().x - (g.size().x + 16.0) / 2.0;
    p.circle_filled(egui::pos2(x0 + 4.0, rect.center().y), 4.0, col);
    p.galley(egui::pos2(x0 + 16.0, rect.center().y - g.size().y / 2.0), g, if on { TEXT } else { MUTED });
    resp
}

/// Значок 22 px у рядку (банер у вікні додавання).
fn draw_icon_inline(ui: &mut egui::Ui, ic: Ic, col: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
    draw_icon(ui.painter(), rect.center(), ic, 22.0, col);
}

/// Зелена кнопка вікна додавання (як на макеті): висота 44, закруглення 10.
fn add_green(text: &str) -> Solid {
    Solid { text: text.to_owned(), fill: Color32::from_rgb(0x1b, 0xd9, 0x6a), fg: Color32::from_rgb(0x08, 0x16, 0x0e), min: egui::vec2(0.0, 44.0) }
}

/// Розділювач із підписом посередині: «── Ваші проєкти на Modrinth ──».
fn labeled_divider(ui: &mut egui::Ui, text: &str) {
    let g = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::proportional(HINT_SZ), MUTED);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
    let p = ui.painter();
    let (cx, cy) = (rect.center().x, rect.center().y);
    let half = g.size().x / 2.0 + 12.0;
    p.hline(rect.left()..=(cx - half).max(rect.left()), cy, Stroke::new(1.0, BORDER));
    p.hline((cx + half).min(rect.right())..=rect.right(), cy, Stroke::new(1.0, BORDER));
    p.galley(egui::pos2(cx - g.size().x / 2.0, cy - g.size().y / 2.0), g, MUTED);
}

/// Рядок проєкту у списку: іконка 40, назва, підпис, праворуч дія.
fn add_row(ui: &mut egui::Ui, icon_url: &str, name: &str, sub: &str, right: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(Color32::from_rgb(0x26, 0x27, 0x2d)).stroke(Stroke::new(1.0, BORDER)).corner_radius(12).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            icon(ui, icon_url, name, 40.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add(egui::Label::new(RichText::new(name).color(TEXT).size(14.5)).truncate());
                if !sub.is_empty() {
                    ui.add(egui::Label::new(RichText::new(sub).color(MUTED).size(12.5)).truncate());
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
        });
    });
}

/// Група версії для списку: «1.20» для 1.20.4, «Знімки» для всього, що не схоже на реліз.
fn mc_group(v: &str) -> String {
    if let Some(rest) = v.strip_prefix("1.") {
        let minor: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !minor.is_empty() && (rest.len() == minor.len() || rest[minor.len()..].starts_with('.')) {
            return format!("1.{minor}");
        }
    }
    "Знімки".to_string()
}

/// Рядок версії у вікні вибору: галочка й номер.
fn ver_row(ui: &mut egui::Ui, v: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, v.to_string()));
    let p = ui.painter();
    if on {
        p.rect_filled(rect, R_CTRL, ACCENT.gamma_multiply(0.12));
    } else if resp.hovered() {
        p.rect_filled(rect, R_CTRL, HOVER);
    }
    if resp.has_focus() {
        p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
    }
    let bx = egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), egui::vec2(20.0, 20.0));
    if on {
        p.rect_filled(bx, R_BADGE, ACCENT);
        draw_icon(p, bx.center(), Ic::Check, 12.0, ON_ACCENT);
    } else {
        p.rect_stroke(bx, R_BADGE, Stroke::new(1.5, UI_BORDER), egui::StrokeKind::Inside);
    }
    p.text(egui::pos2(rect.left() + 40.0, rect.center().y), egui::Align2::LEFT_CENTER, v, egui::FontId::proportional(14.0), TEXT);
    resp
}

/// Картка типу файлу (мод / ресурс-пак / датапак): назва й підпис, вибрана — акцентна рамка.
fn kind_card(ui: &mut egui::Ui, on: bool, name: &str, sub: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 60.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, format!("{name}: {sub}")));
    let p = ui.painter();
    let r = egui::CornerRadius::same(CARD_R.saturating_sub(2));
    p.rect_filled(rect, r, if on { ACCENT.gamma_multiply(0.12) } else if resp.hovered() { HOVER } else { FIELD });
    p.rect_stroke(rect, r, Stroke::new(if on { 2.0 } else { 1.0 }, if on { ACCENT } else { UI_BORDER }), egui::StrokeKind::Inside);
    if resp.has_focus() {
        p.rect_stroke(rect.expand(2.0), r, Stroke::new(2.0, ACCENT), egui::StrokeKind::Outside);
    }
    p.text(egui::pos2(rect.left() + 14.0, rect.center().y - 9.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(14.5), TEXT);
    p.text(egui::pos2(rect.left() + 14.0, rect.center().y + 11.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(12.0), MUTED);
    let bx = egui::Rect::from_center_size(egui::pos2(rect.right() - 22.0, rect.center().y), egui::vec2(22.0, 22.0));
    if on {
        p.rect_filled(bx, R_BADGE, ACCENT);
        draw_icon(p, bx.center(), Ic::Check, 13.0, ON_ACCENT);
    } else {
        p.rect_stroke(bx, R_BADGE, Stroke::new(1.5, UI_BORDER), egui::StrokeKind::Inside);
    }
    resp
}

/// Чи є місце для правої картки публікації (тоді нижньої панелі немає). Одна умова для ui() і publish_ui().
fn pub_side(ctx: &egui::Context) -> bool {
    let rail_w = if ctx.content_rect().width() < 860.0 { RAIL_W - 16.0 } else { RAIL_W };
    (ctx.content_rect().width() - rail_w - 48.0).min(CONTENT_MAX_W) >= 820.0
}

/// Опис кроку під заголовком.
fn step_desc(n: usize) -> &'static str {
    match n {
        1 => "Мод, нову версію якого ви публікуєте.",
        2 => "Назву й версію ми спробуємо прочитати з самого файлу.",
        3 => "Номер і назва, які побачать користувачі на платформах.",
        4 => "З якими версіями гри й лоадерами працює цей файл.",
        5 => "Необов'язково. Текст з'явиться на сторінці версії.",
        _ => "Можна на обидві платформи одразу.",
    }
}

/// Рядок проєкту у виборі: назва, остання версія, бейджі платформ справа.
fn project_row(ui: &mut egui::Ui, name: &str, last: &str, mr: bool, cf: bool, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, name.to_string()));
    let p = ui.painter();
    let r = egui::CornerRadius::same(CARD_R.saturating_sub(4));
    let fill = if on { ACCENT.gamma_multiply(0.12) } else if resp.hovered() { HOVER } else { FIELD };
    p.rect_filled(rect, r, fill);
    p.rect_stroke(rect, r, Stroke::new(if on { 2.0 } else { 1.0 }, if on { ACCENT } else { UI_BORDER }), egui::StrokeKind::Inside);
    if resp.has_focus() {
        p.rect_stroke(rect.expand(2.0), r, Stroke::new(2.0, FOCUS), egui::StrokeKind::Outside);
    }
    let sub = if last.trim().is_empty() { "ще не публікувався".to_string() } else { format!("остання версія {}", last.trim()) };
    p.text(egui::pos2(rect.left() + 16.0, rect.center().y + 10.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(12.0), MUTED);
    let mut x = rect.right() - 14.0;
    for (has, name, fg, bg) in [(cf, "CurseForge", CF_ORANGE, CF_BG), (mr, "Modrinth", MR_GREEN, MR_BG)] {
        if !has { continue; }
        let g = p.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.0), fg);
        let pill = egui::Rect::from_min_size(egui::pos2(x - g.size().x - 18.0, rect.center().y - 12.0), egui::vec2(g.size().x + 18.0, 24.0));
        p.rect_filled(pill, R_BADGE, bg);
        p.galley(egui::pos2(pill.left() + 9.0, pill.center().y - g.size().y / 2.0), g, fg);
        x = pill.left() - 6.0;
    }
    // назва після бейджів: обрізається з «…» до вільного місця, а не наїжджає на бейдж
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), egui::FontId::proportional(14.5), TEXT);
    job.wrap.max_width = (x - (rect.left() + 16.0) - 8.0).max(40.0);
    job.wrap.max_rows = 1; // без цього довга назва переноситься на другий рядок замість обрізання
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let g = p.layout_job(job);
    p.galley(egui::pos2(rect.left() + 16.0, rect.center().y - 9.0 - g.size().y / 2.0), g, TEXT);
    resp
}

/// Велика картка платформи з галочкою: вибрана — акцентна рамка.
fn platform_card(ui: &mut egui::Ui, on: bool, name: &str, col: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, name.to_string()));
    let p = ui.painter();
    let r = egui::CornerRadius::same(CARD_R.saturating_sub(2));
    p.rect_filled(rect, r, if on { ACCENT.gamma_multiply(0.12) } else if resp.hovered() { HOVER } else { FIELD });
    p.rect_stroke(rect, r, Stroke::new(if on { 2.0 } else { 1.0 }, if on { ACCENT } else { UI_BORDER }), egui::StrokeKind::Inside);
    if resp.has_focus() {
        p.rect_stroke(rect.expand(2.0), r, Stroke::new(2.0, FOCUS), egui::StrokeKind::Outside);
    }
    let bx = egui::Rect::from_center_size(egui::pos2(rect.left() + 28.0, rect.center().y), egui::vec2(22.0, 22.0));
    if on {
        p.rect_filled(bx, R_BADGE, ACCENT);
        draw_icon(p, bx.center(), Ic::Check, 13.0, ON_ACCENT);
    } else {
        p.rect_stroke(bx, R_BADGE, Stroke::new(1.5, UI_BORDER), egui::StrokeKind::Inside);
    }
    p.text(egui::pos2(rect.left() + 50.0, rect.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(15.0), col);
    resp
}

/// Шапка картки облікового запису: назва кольором платформи, статус токена, посилання «Як отримати» праворуч.
fn acc_head(ui: &mut egui::Ui, name: &str, col: Color32, token: &str, help: &str, url: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(17.0).strong().color(col));
        if token.trim().is_empty() {
            badge(ui, i18n::tr("Не вказано"), MUTED);
        } else {
            badge_ic(ui, i18n::tr("Підключено"), SUCCESS, Some(Ic::Check));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| link(ui, help, url));
    });
}

/// Рядок токена як на макеті: підпис, поле з оком і кнопка «Перевірити» в одному рядку. Повертає (змінено, натиснуто «Перевірити»).
fn token_row(ui: &mut egui::Ui, label: &str, value: &mut String, show: &mut bool, checking: bool, aria_label: &str) -> (bool, bool) {
    ui.label(RichText::new(label).size(13.0).color(TEXT));
    ui.add_space(2.0);
    let (mut edited, mut go) = (false, false);
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 130.0).max(160.0);
        ui.allocate_ui_with_layout(egui::vec2(w, 36.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            edited = token_field(ui, value, show);
        });
        go = ui.add_enabled_ui(!checking, |ui| aria(ghost_btn(ui, None, if checking { i18n::tr("status.checking") } else { i18n::tr("btn.check") }), aria_label.to_string())).inner.clicked();
    });
    (edited, go)
}

/// Заголовок розділу налаштувань: назва й короткий опис.
fn sec_head(ui: &mut egui::Ui, name: &str, desc: &str) {
    ui.label(RichText::new(name).size(20.0).color(TEXT));
    ui.add_space(2.0);
    hint(ui, desc);
    ui.add_space(8.0);
}

/// Рядок налаштування: назва й опис ліворуч, кнопка праворуч. Повертає, чи натиснули кнопку.
fn setting_row(ui: &mut egui::Ui, name: &str, desc: &str, btn: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let w = ui.available_width();
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2((w - 230.0).max(140.0), 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.label(RichText::new(name).color(TEXT).size(14.5));
            hint(ui, desc);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| clicked = btn(ui));
    });
    clicked
}

/// Тонкий розділювач між рядками налаштувань.
fn row_sep(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, BORDER));
    ui.add_space(8.0);
}

/// Чип з правом токена або клавішею.
fn mini_chip(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new().fill(FIELD).stroke(Stroke::new(1.0, BORDER)).corner_radius(R_BADGE).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
        ui.label(RichText::new(text).size(12.0).color(TEXT));
    });
}

/// Рядок гарячої клавіші: «Ctrl+1, Ctrl+2: опис» → опис ліворуч, клавіші чипами праворуч.
fn hotkey_row(ui: &mut egui::Ui, line: &str) {
    let (keys, desc) = line.split_once(": ").unwrap_or(("", line));
    let mut cs = desc.chars();
    let desc: String = cs.next().map(|c| c.to_uppercase().chain(cs).collect()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new(desc).size(HINT_SZ).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let groups: Vec<&str> = keys.split([',', '/']).map(str::trim).filter(|g| !g.is_empty()).collect();
            for (gi, g) in groups.iter().enumerate().rev() {
                let ks: Vec<&str> = g.split('+').map(str::trim).collect();
                for (ki, k) in ks.iter().enumerate().rev() {
                    mini_chip(ui, k);
                    if ki > 0 { ui.label(RichText::new("+").color(MUTED).size(12.0)); }
                }
                if gi > 0 { ui.label(RichText::new("/").color(MUTED).size(12.0)); }
            }
        });
    });
}

/// Пункт меню розділів налаштувань: значок, назва, необов'язкова крапка статусу справа.
fn side_item(ui: &mut egui::Ui, ic: Ic, label: &str, active: bool, dot: Option<Color32>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, label.to_string()));
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 10, Color32::from_rgb(0x25, 0x25, 0x27));
    } else if resp.hovered() {
        p.rect_filled(rect, 10, HOVER);
    }
    if resp.has_focus() {
        p.rect_stroke(rect, 10, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside);
    }
    draw_icon(p, egui::pos2(rect.left() + 24.0, rect.center().y), ic, 18.0, if active { ACCENT } else { MUTED });
    p.text(egui::pos2(rect.left() + 46.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(14.5), if active { TEXT } else { MUTED });
    if let Some(c) = dot {
        p.circle_filled(egui::pos2(rect.right() - 16.0, rect.center().y), 4.0, c);
    }
    resp
}

/// Вкладка степера: коло з номером (або галочкою) і назва; активна — пілюля з акцентною рамкою.
fn step_tab(ui: &mut egui::Ui, n: usize, label: &str, done: bool, active: bool) -> egui::Response {
    let col = if active || done { TEXT } else { MUTED };
    let g = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.5), col);
    let w = 8.0 + STEP_DOT + 8.0 + g.size().x + if active { 14.0 } else { 8.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, CTRL_H), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, i18n::fmt(i18n::tr("Крок {}: {}"), &[&n, &label])));
    let p = ui.painter();
    let rr = egui::CornerRadius::same(if active { 18 } else { R_CTRL });
    if active {
        p.rect_filled(rect, rr, Color32::from_rgb(0x25, 0x2a, 0x33));
        p.rect_stroke(rect, rr, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, rr, HOVER);
    }
    if resp.has_focus() {
        p.rect_stroke(rect, rr, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside);
    }
    let c = egui::pos2(rect.left() + 8.0 + STEP_DOT / 2.0, rect.center().y);
    // заливка = «ви тут», кільце з галочкою = «готово», сіре кільце = ще ні: місце й повнота читаються різно
    if active {
        p.circle_filled(c, STEP_DOT / 2.0, ACCENT);
    } else {
        p.circle_stroke(c, STEP_DOT / 2.0 - 0.75, Stroke::new(1.5, if done { ACCENT } else { UI_BORDER }));
    }
    if done && !active {
        draw_icon(p, c, Ic::Check, 12.0, ACCENT);
    } else {
        p.text(c, egui::Align2::CENTER_CENTER, n.to_string(), egui::FontId::proportional(12.0), if active { ON_ACCENT } else { MUTED });
    }
    p.galley(egui::pos2(c.x + STEP_DOT / 2.0 + 8.0, rect.center().y - g.size().y / 2.0), g, col);
    resp
}

fn step(ui: &mut egui::Ui, spec: StepSpec, open: &mut bool, f: impl FnOnce(&mut egui::Ui)) {
    let StepSpec { n, name, done, required, summary, scroll, tabbed } = spec;
    if (required && !done) || scroll || tabbed {
        *open = true; // примусово розгорнутий крок лишається розгорнутим, поки користувач сам не згорне
    }
    if scroll {
        ui.scroll_to_cursor(Some(egui::Align::TOP));
    }
    let expanded = *open;
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        if n > 1 && !tabbed {
            // з'єднувач з попереднім кроком; коло малюється поверх
            let c = ui.cursor().min;
            let gap = ui.spacing().item_spacing.y;
            let x = c.x + STEP_DOT / 2.0;
            ui.painter().line_segment([egui::pos2(x, c.y - CARD_PAD as f32 - gap), egui::pos2(x, c.y + STEP_DOT / 2.0)], Stroke::new(STEP_LINE, UI_BORDER));
        }
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(if tabbed { egui::Vec2::ZERO } else { egui::vec2(STEP_DOT, STEP_DOT) }, egui::Sense::hover());
            if tabbed {
            } else if done {
                ui.painter().circle_filled(rect.center(), STEP_DOT / 2.0, SUCCESS);
                draw_icon(ui.painter(), rect.center(), Ic::Check, 12.0, ON_ACCENT);
            } else {
                ui.painter().circle_filled(rect.center(), STEP_DOT / 2.0, ACCENT);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, n.to_string(), egui::FontId::proportional(13.0), ON_ACCENT);
            }
            if tabbed { ui.label(RichText::new(name).size(20.0).color(TEXT)); } else { ui.label(title(name)); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !expanded {
                    if aria(ghost_btn(ui, None, i18n::tr("Змінити")), i18n::fmt(i18n::tr("Змінити крок «{}»"), &[&(name)])).clicked() {
                        *open = true;
                    }
                } else if done && !tabbed && aria(icon_only(ui, Ic::ChevronUp, i18n::tr("Згорнути крок"), false), i18n::fmt(i18n::tr("Згорнути крок «{}»"), &[&(name)])).clicked() {
                    *open = false;
                }
            });
        });
        if expanded {
            if tabbed { hint(ui, step_desc(n)); }
            ui.add_space(8.0);
            f(ui);
        } else if !summary.is_empty() {
            ui.add(egui::Label::new(RichText::new(summary).color(MUTED).size(HINT_SZ)).truncate());
        }
    });
}

fn badge(ui: &mut egui::Ui, text: &str, col: Color32) {
    badge_n(ui, text, col, col, col, None, false);
}

fn badge_ic(ui: &mut egui::Ui, text: &str, col: Color32, ic: Option<Ic>) {
    badge_n(ui, text, col, col, col, ic, false);
}

/// Заливна фірмова пілюля платформи для картки проєкту.
fn brand_pill(ui: &mut egui::Ui, name: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(R_BADGE))
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.label(RichText::new(name).color(fg).strong().size(HINT_SZ));
        });
}

/// Бейдж висотою 24: контур 1px, значок (або крапка 6px) і текст.
fn badge_n(ui: &mut egui::Ui, text: &str, outline: Color32, dot: Color32, text_col: Color32, ic: Option<Ic>, ring: bool) {
    // статусні бейджі (колір контуру = колір значка) мають ледь помітну тонову заливку
    let tint = if outline == dot && dot != MUTED { dot.gamma_multiply(0.10) } else { Color32::TRANSPARENT };
    egui::Frame::new()
        .fill(tint)
        .stroke(Stroke::new(1.0, outline))
        .corner_radius(egui::CornerRadius::same(R_BADGE))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 16.0; // інакше рядок розтягується до висоти кнопки (36)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                match ic {
                    Some(k) => {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        draw_icon(ui.painter(), rect.center(), k, 12.0, dot);
                    }
                    None => {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                        if ring {
                            ui.painter().circle_stroke(rect.center(), 2.5, Stroke::new(1.5, dot));
                        } else {
                            ui.painter().circle_filled(rect.center(), 3.0, dot);
                        }
                    }
                }
                ui.label(RichText::new(text).color(text_col).strong().size(HINT_SZ));
            });
        });
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Info,
    Warn,
    Error,
}

/// Єдине повідомлення: смуга 3px зліва, значок і текст; відступи 12/8.
/// Слово замість значка-символу для скрінрідера (✓/✕ не озвучуються): «Успішно», «Помилка» тощо; None — значок суто декоративний.
fn icon_word(ic: Ic) -> Option<&'static str> {
    match ic {
        Ic::Check => Some(i18n::tr("Успішно")),
        Ic::Cross => Some(i18n::tr("Помилка")),
        Ic::Warn => Some(i18n::tr("Попередження")),
        Ic::Info => Some(i18n::tr("Інформація")),
        _ => None,
    }
}

fn spoken_icon(resp: &egui::Response, ic: Ic) {
    if let Some(w) = icon_word(ic) {
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, w));
    }
}

fn banner(ui: &mut egui::Ui, kind: Kind, text: &str) {
    let (col, ic) = match kind {
        Kind::Info => (MUTED, Ic::Info),
        Kind::Warn => (WARN, Ic::Warn),
        Kind::Error => (ERROR, Ic::Cross),
    };
    let fill = if kind == Kind::Info { FIELD } else { col.gamma_multiply(0.12) };
    // поява за 150 мс: прозорість росте від 0 на першому кадрі
    let fade = ui.ctx().animate_bool_with_time(ui.scope_id().with(("banner", kind as u8)), true, 0.15);
    ui.scope(|ui| banner_inner(ui, kind, text, col, ic, fill, fade));
}

fn banner_inner(ui: &mut egui::Ui, kind: Kind, text: &str, col: Color32, ic: Ic, fill: Color32, fade: f32) {
    ui.set_opacity(fade);
    let r = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, if kind == Kind::Info { BORDER } else { col.gamma_multiply(0.45) }))
        .corner_radius(egui::CornerRadius::same(R_CTRL))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                let (rect, ir) = ui.allocate_exact_size(egui::vec2(16.0, 20.0), egui::Sense::hover());
                spoken_icon(&ir, ic);
                draw_icon(ui.painter(), rect.center(), ic, 16.0, col);
                ui.add(egui::Label::new(RichText::new(text).color(TEXT)).wrap());
            });
        });
    // для скрінрідерів: повідомлення оголошується, помилки — негайно
    ui.ctx().accesskit_node_builder(r.response.id, |n| n.set_live(if kind == Kind::Error { egui::accesskit::Live::Assertive } else { egui::accesskit::Live::Polite }));
    let rc = r.response.rect;
    ui.painter().rect_filled(egui::Rect::from_min_size(rc.left_top() + egui::vec2(0.0, 4.0), egui::vec2(3.0, rc.height() - 8.0)), 2, col);
}

/// Значок + текст в один рядок.
fn icon_label(ui: &mut egui::Ui, ic: Ic, col: Color32, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, ir) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        spoken_icon(&ir, ic);
        draw_icon(ui.painter(), rect.center(), ic, 16.0, col);
        ui.add(egui::Label::new(RichText::new(text).color(TEXT)).truncate());
    });
}

/// Значок + текст заданого кольору.
fn icon_label_c(ui: &mut egui::Ui, ic: Ic, col: Color32, text: &str, text_col: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, ir) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        spoken_icon(&ir, ic);
        draw_icon(ui.painter(), rect.center(), ic, 16.0, col);
        ui.add(egui::Label::new(RichText::new(text).color(text_col)).truncate());
    });
}

/// Попередження перед публікацією: значок + текст, що переноситься (не обрізається: це остання перевірка перед незворотною дією).
fn warn_row(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, ir) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
        spoken_icon(&ir, Ic::Warn);
        draw_icon(ui.painter(), rect.center(), Ic::Warn, 16.0, WARN);
        ui.add(egui::Label::new(RichText::new(text).color(WARN).size(HINT_SZ)).wrap());
    });
}

/// Заголовок помилки платформи простою мовою: (текст, чи причина в токені/правах, чи є що показати в «Докладніше»).
/// Сирий текст сервера лишається за «Докладніше»: 401/403 майже завжди про токен, а не про файл.
fn error_headline(name: &str, e: &str) -> (String, bool, bool) {
    // помилка API має вигляд «Порада простою мовою (HTTP 403 Forbidden: тіло відповіді)»: порада йде в заголовок, сире за «Докладніше»
    if let Some((human, raw)) = e.split_once(" (HTTP ") {
        let token = raw.starts_with("401") || raw.starts_with("403"); // токен або права (403 також може бути ID проєкту: порада про це вже в тексті)
        return (format!("{name}: {human}"), token, true);
    }
    let short: String = e.split(". ").next().unwrap_or(e).lines().next().unwrap_or("").chars().take(90).collect();
    if short.trim().is_empty() {
        return (i18n::fmt(i18n::tr("{}: помилка"), &[&name]), false, false); // без «помилка. » з порожнім хвостом
    }
    let long = short.chars().count() < e.chars().count();
    (i18n::fmt(i18n::tr("{}: помилка. {}{}"), &[&name, &short, &if long { "…" } else { "" }]), false, long)
}

/// Підпис лоадера в чипі: для паків зрозуміла назва замість службового «minecraft».
fn loader_label(l: &str) -> &str {
    match l {
        "minecraft" => i18n::tr("Ресурспак"),
        "datapack" => i18n::tr("Датапак"),
        _ => l,
    }
}

/// Лоадери для людини: «Ресурспак» замість службового «minecraft» (у чипах, зведенні й примітці про файл).
fn loaders_text(loaders: &[String]) -> String {
    loaders.iter().map(|l| loader_label(l)).collect::<Vec<_>>().join(", ")
}

/// Вибрано і лоадер мода, і ресурспак/датапак водночас.
fn mixed_pack_loaders(loaders: &[String]) -> bool {
    loaders.iter().any(|l| is_pack_loader(l)) && loaders.iter().any(|l| !is_pack_loader(l))
}

/// Рамка модального вікна.
fn modal_frame() -> egui::Frame {
    egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).corner_radius(egui::CornerRadius::same(CARD_R)).inner_margin(egui::Margin::same(24))
}

/// Кнопка-«привид»: без заливки, при наведенні акцент.
fn ghost_btn(ui: &mut egui::Ui, ic: Option<Ic>, label: &str) -> egui::Response {
    btn_paint(ui, ic, label, BtnKind::Ghost, 0.0, CTRL_H)
}

/// Квадратна кнопка лише зі значком (36x36); підпис — у підказці й для скрінрідерів. `on` — значок бурштиновий.
fn icon_only(ui: &mut egui::Ui, ic: Ic, tip: &str, on: bool) -> egui::Response {
    let enabled = ui.is_enabled();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(CTRL_H, CTRL_H), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
    let t = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered() && enabled, 0.12);
    let down = enabled && resp.is_pointer_button_down_on();
    let p = ui.painter();
    let fill = if down { ACTIVE } else { FIELD.lerp_to_gamma(HOVER, t) };
    let border = if enabled { UI_BORDER.lerp_to_gamma(ACCENT.gamma_multiply(0.6), t) } else { BORDER };
    p.rect(rect, R_CTRL, fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    if resp.has_focus() {
        p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside);
    }
    let col = if !enabled { MUTED.gamma_multiply(0.5) } else if on { ACCENT } else { MUTED.lerp_to_gamma(TEXT, t) };
    draw_icon(p, rect.center(), ic, 16.0, col);
    let resp = resp.on_hover_text(tip);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Прапорець у дусі застосунку: квадрат 20px, увімкнений заливається акцентом із галочкою; клік по всьому рядку.
fn check_row(ui: &mut egui::Ui, on: &mut bool, label: &str) -> egui::Response {
    let enabled = ui.is_enabled();
    let galley = (!label.is_empty()).then(|| ui.painter().layout(label.to_owned(), egui::TextStyle::Body.resolve(ui.style()), if enabled { TEXT } else { MUTED }, (ui.available_width() - 32.0).max(80.0)));
    let w = 20.0 + galley.as_ref().map_or(0.0, |g| 10.0 + g.size().x);
    let h = galley.as_ref().map_or(0.0, |g| g.size().y).max(TOUCH_H);
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, *on, label));
    let t = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered() && enabled, 0.12);
    let on_t = ui.ctx().animate_bool_with_time(resp.id.with("on"), *on, 0.12);
    let bx = egui::Rect::from_center_size(egui::pos2(rect.left() + 10.0, rect.center().y), egui::vec2(20.0, 20.0));
    let p = ui.painter();
    let border = UI_BORDER.lerp_to_gamma(ACCENT.gamma_multiply(0.6), t).lerp_to_gamma(ACCENT, on_t);
    p.rect(bx, R_BADGE, FIELD.lerp_to_gamma(ACCENT, on_t), Stroke::new(1.0, if enabled { border } else { BORDER }), egui::StrokeKind::Inside);
    if on_t > 0.0 {
        draw_icon(p, bx.center(), Ic::Check, 13.0, ON_ACCENT.gamma_multiply(on_t));
    }
    if resp.has_focus() {
        p.rect_stroke(bx.expand(2.0), R_BADGE + 1, Stroke::new(2.0, FOCUS), egui::StrokeKind::Outside);
    }
    if let Some(g) = galley {
        p.galley(egui::pos2(rect.left() + 30.0, rect.center().y - g.size().y / 2.0), g, TEXT);
    }
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Кнопка закриття (X) без рамки.
fn close_x(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, i18n::tr("btn.close")));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, R_CTRL, HOVER);
    }
    if resp.has_focus() {
        p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside);
    }
    draw_icon(p, rect.center(), Ic::Cross, 14.0, if resp.hovered() { TEXT } else { MUTED });
    resp.on_hover_text(i18n::tr("btn.close"))
}

/// Помилка поля: рамка ERROR 1.5px і червоний текст під полем.
fn field_error(ui: &mut egui::Ui, resp: &egui::Response, msg: &str) {
    ui.painter().rect_stroke(resp.rect.expand(1.0), R_CTRL, Stroke::new(1.5, ERROR), egui::StrokeKind::Outside);
    ui.label(RichText::new(msg).color(ERROR).size(HINT_SZ));
}

/// Кнопка з намальованим значком; `primary` — акцентна заливка.
fn icon_btn(ui: &mut egui::Ui, ic: Option<Ic>, label: &str, primary: bool) -> egui::Response {
    btn_paint(ui, ic, label, if primary { BtnKind::Solid(ACCENT, ON_ACCENT) } else { BtnKind::Field }, 0.0, CTRL_H)
}

/// Кнопка-перемикач (лоадер, тип релізу, платформа): активна з галочкою, кольоровою обводкою 2px і заливкою.
fn choice_col(ui: &mut egui::Ui, on: bool, label: &str, col: Color32, alpha: f32) -> egui::Response {
    choice_sz(ui, on, label, col, alpha, CTRL_H, egui::TextStyle::Button.resolve(ui.style()))
}

fn choice_sz(ui: &mut egui::Ui, on: bool, label: &str, col: Color32, alpha: f32, h: f32, font: egui::FontId) -> egui::Response {
    let enabled = ui.is_enabled();
    let txt_col = if enabled { TEXT } else { MUTED };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, txt_col);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 28.0 + 18.0, h), egui::Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered() && enabled, 0.12);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, label));
    let down = enabled && resp.is_pointer_button_down_on();
    let (fill, stroke) = if on {
        (col.gamma_multiply(alpha + if down { 0.1 } else { 0.0 }), Stroke::new(2.0, col))
    } else {
        (if down { ACTIVE } else { FIELD.lerp_to_gamma(HOVER, t) }, Stroke::new(1.0, UI_BORDER.lerp_to_gamma(ACCENT.gamma_multiply(0.6), t)))
    };
    let p = ui.painter();
    p.rect(rect, R_CTRL, fill, stroke, egui::StrokeKind::Inside);
    if resp.has_focus() {
        p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside);
    }
    // вміст по центру; розмір кнопки не залежить від стану (місце під галочку резервується)
    let content_w = galley.size().x + if on { 18.0 } else { 0.0 };
    let x0 = rect.center().x - content_w / 2.0;
    if on {
        draw_icon(p, egui::pos2(x0 + 6.0, rect.center().y), Ic::Check, 12.0, col);
    }
    p.galley(egui::pos2(x0 + if on { 18.0 } else { 0.0 }, rect.center().y - galley.size().y / 2.0), galley, txt_col);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

fn choice(ui: &mut egui::Ui, on: bool, label: &str) -> egui::Response {
    choice_col(ui, on, label, ACCENT, 0.20)
}

/// Порожній стан: значок 48px, заголовок, підказка.
fn empty_state(ui: &mut egui::Ui, ic: Ic, name: &str, text: &str) {
    ui.add_space(16.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 36.0, FIELD);
    ui.painter().circle_stroke(rect.center(), 36.0, Stroke::new(1.0, BORDER));
    draw_icon(ui.painter(), rect.center(), ic, 32.0, ACCENT);
    ui.add_space(4.0);
    ui.label(title(name));
    hint(ui, text);
    ui.add_space(4.0);
}

/// Поле токена з іконкою-оком усередині тієї самої рамки.
fn token_field(ui: &mut egui::Ui, value: &mut String, show: &mut bool) -> bool {
    let mut changed = false;
    let mut prep = egui::Frame::new().fill(FIELD).stroke(Stroke::new(1.0, UI_BORDER)).corner_radius(egui::CornerRadius::same(R_CTRL)).inner_margin(egui::Margin::symmetric(8, 0)).begin(ui);
    let mut focused = false;
    {
        let ui = &mut prep.content_ui;
        ui.horizontal(|ui| {
            let w = ui.available_width() - 40.0;
            let te = ui.add(egui::TextEdit::singleline(value).password(!*show).hint_text(i18n::tr("Вставте сюди (Ctrl+V)")).frame(egui::Frame::NONE).desired_width(w).min_size(egui::vec2(w, 34.0)).align(egui::Align2::LEFT_CENTER));
            changed = te.changed();
            focused = te.has_focus();
            let label = if *show { i18n::tr("btn.hide") } else { i18n::tr("Показати") };
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, i18n::fmt(i18n::tr("{} токен"), &[&(label)])));
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(rect, R_CTRL, HOVER);
            }
            if resp.has_focus() {
                p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside);
            }
            let col = if resp.hovered() { TEXT } else { MUTED };
            draw_icon(p, rect.center(), Ic::Eye, 16.0, col);
            if *show {
                p.line_segment([rect.center() + egui::vec2(-7.0, 7.0), rect.center() + egui::vec2(7.0, -7.0)], Stroke::new(1.75, col));
            }
            if resp.on_hover_text(label).clicked() {
                *show = !*show;
            }
        });
    }
    // товщина рамки задана до begin(): інакше end() збільшує зайняту область на 1 px і зсуває картку ліворуч
    prep.frame.stroke = Stroke::new(1.0, if focused { ACCENT } else { UI_BORDER });
    let resp = prep.end(ui);
    if focused {
        ui.painter().rect_stroke(resp.rect, R_CTRL, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside);
    }
    changed
}

fn icon(ui: &mut egui::Ui, url: &str, name: &str, size: f32) {
    // Читаємо байти самі: завантажувач file:// у Windows не знаходив кешовані іконки.
    // байти іконки читаємо з диска один раз і тримаємо в пам'яті: читання на кожному кадрі давало ривки анімацій
    thread_local! {
        static ICONS: std::cell::RefCell<std::collections::HashMap<String, (std::path::PathBuf, egui::load::Bytes)>> = Default::default();
    }
    let cached = if url.is_empty() { None } else {
        ICONS.with_borrow_mut(|m| {
            if let Some(v) = m.get(url) { return Some(v.clone()); }
            let p = icon_path(url).filter(|p| p.exists())?;
            let b: egui::load::Bytes = std::fs::read(&p).ok()?.into();
            m.insert(url.to_string(), (p.clone(), b.clone()));
            Some((p, b))
        })
    };
    match cached {
        Some((p, bytes)) => {
            let uri = format!("bytes://{}", p.to_string_lossy().replace('\\', "/"));
            ui.add(egui::Image::from_bytes(uri, bytes).fit_to_exact_size(egui::vec2(size, size)).corner_radius(R_CTRL));
        }
        None => {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            // теплий відтінок за назвою: плитка з тонкою рамкою і літерою кольору відтінку (без синіх/фіолетових)
            const TINTS: [Color32; 5] = [ACCENT, WARN, SUCCESS, CF_COL, MR_GREEN];
            let tint = TINTS[name.bytes().fold(0usize, |a, c| a.wrapping_mul(31).wrapping_add(c as usize)) % TINTS.len()];
            ui.painter().rect(rect, R_CTRL, tint.gamma_multiply(0.16), Stroke::new(1.0, tint.gamma_multiply(0.45)), egui::StrokeKind::Inside);
            let c = name.chars().next().unwrap_or('?').to_uppercase().to_string();
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, c, egui::FontId::proportional(size * 0.5), tint);
        }
    }
}

/// Запускає завдання потоку; паніку перетворює на `Msg::Panicked`.
fn guarded(f: impl FnOnce() -> Msg) -> Msg {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(Msg::Panicked)
}

fn guarded_opt(f: impl FnOnce() -> Option<Msg>) -> Option<Msg> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(Some(Msg::Panicked))
}

impl App {
    fn new() -> Self {
        let (tx, rx) = channel();
        let (cfg, state) = load_config();
        i18n::set_lang(match cfg.lang.as_str() { "en" => i18n::Lang::En, "ru" => i18n::Lang::Ru, _ => i18n::Lang::Uk });
        let l = &cfg.last;
        let sel = (l.project < cfg.projects.len()).then_some(l.project);
        let rt = RELEASE_TYPES.iter().position(|r| *r == l.release_type).unwrap_or(0);
        let mut app = Self {
            tab: Tab::Projects,
            edit_mode: true, review_now: false, tx, rx, add_open: false, target: None, mr_list: vec![], mr_loading: false, mr_msg: String::new(),
            cf_id: String::new(), cur_name: String::new(), proj_chk: Chk::Idle, featured: false, send_deps: false, extra_files: vec![], sel_mode: false, picked: Default::default(), batch_confirm: false, merge_ask: vec![], centered: false, add_msg: String::new(), add_tab: 0, cf_query: String::new(), cf_hits: vec![], cf_loading: false, sel, file: None,
            name: String::new(), number: sel.and_then(|i| cfg.projects.get(i)).map(|p| bump(&p.last_number)).unwrap_or_default(), release_type: rt, changelog: String::new(),
            use_mr: true, use_cf: true, mr_res: None, cf_res: None, pending: 0, msg: String::new(),
            show_tokens: false, confirm_del: None, confirm_clear: false, check_msg: checked_msg(cfg.modrinth_checked), up_mr: Up::default(), up_cf: Up::default(), cur_cl: String::new(), cur_meta: Default::default(),
            notice_seen: None, update: None, upd_checking: false, upd_applying: false, refreshing: false, upd_err: String::new(), upd_started: false, quit: false,
            notice: match state {
                Load::Fine => String::new(),
                Load::Restored => "Конфіг було пошкоджено: налаштування відновлено з резервної копії (config.backup.json), зіпсований файл збережено поруч.".into(),
                Load::Recovered =>i18n::tr("Конфіг було пошкоджено: копію збережено в теці конфігу (файл .bak), налаштування скинуто.").into(),
                Load::ReadOnly => i18n::tr("Не вдалося прочитати конфіг: збереження вимкнено, щоб не перезаписати старі дані. Закрийте програми, що тримають файл, і перезапустіть.").into(),
            },
            readonly: matches!(state, Load::ReadOnly), adding: 0, cf_auto_off: false, cur_idx: None, history: history_load(HIST_MAX), hist_q: String::new(), hist_fail: false, hist_all: false, hist_confirm: false, cur: Default::default(), mc_known: vec![], mc_all: vec![], mc_loading: false, mc_next: None, cf_msg: checked_msg(cfg.curse_checked), key_msg: checked_msg(cfg.core_checked), merge_hidden: false, add_shown: false, file_note: String::new(), ctrl_armed: None, splash: splash::Splash::new(), brand_rect: None, file_info: None, focus_step: None, focus_hist: false, upd_drawn: false, ver_open: false, ver_q: String::new(), ver_filter: 0, set_sec: std::env::var("MODWIRE_SET_SEC").ok().and_then(|v| v.parse().ok()).unwrap_or(0), pub_step: std::env::var("MODWIRE_PUB_STEP").ok().and_then(|v| v.parse().ok()).unwrap_or(0), proj_q: String::new(), open_steps: [true; 6], start_act: START_ACTION.get().copied().unwrap_or(StartAction::Clean), stable_at: None, confirm_close: false, close_ok: false, draft_at: None, cfg,
        };
        app.restore_draft();
        app
    }

    /// Відновлює незавершену форму (номер, назва, зміни, файл) того самого проєкту після перезапуску.
    fn restore_draft(&mut self) {
        let d = self.cfg.last.draft.clone();
        if !self.sel.and_then(|i| self.cfg.projects.get(i)).is_some_and(|p| d.applies_to(p)) {
            return;
        }
        let f = PathBuf::from(&d.file);
        if !d.file.is_empty() && f.is_file() {
            let info = read_jar(&f); // старт: один файл чернетки
            self.set_file(f, info);
            self.file_note.clear();
        }
        if !d.number.trim().is_empty() {
            self.number = d.number;
            if let Some(k) = detect_release_type(&self.number).and_then(|t| RELEASE_TYPES.iter().position(|r| *r == t)) {
                self.release_type = k;
            }
        }
        (self.name, self.changelog) = (d.name, d.changelog);
        if self.notice.is_empty() && (self.file.is_some() || !self.number.trim().is_empty() && self.number != self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| bump(&p.last_number)).unwrap_or_default() || !self.changelog.trim().is_empty()) {
            self.notice = i18n::tr("Відновлено чернетку з минулого запуску.").into();
        }
    }

    /// Тримає чернетку в конфігу актуальною; на диск пише через 1,5 с після останньої правки (не на кожну літеру).
    fn sync_draft(&mut self, ctx: &egui::Context) {
        let Some(id) = self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| p.id.clone()).filter(|i| !i.is_empty()) else { return };
        let file = self.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let d = &self.cfg.last.draft;
        if (&d.project, &d.number, &d.name, &d.changelog, &d.file) != (&id, &self.number, &self.name, &self.changelog, &file) {
            self.cfg.last.draft = Draft { project: id, number: self.number.clone(), name: self.name.clone(), changelog: self.changelog.clone(), file };
            self.draft_at = Some(std::time::Instant::now());
        }
        if let Some(t) = self.draft_at {
            if t.elapsed() >= std::time::Duration::from_millis(1500) {
                self.draft_at = None;
                if !self.readonly {
                    let _ = save_config(&self.cfg);
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(1600));
            }
        }
    }

    fn save(&mut self) {
        if self.readonly {
            return;
        }
        ensure_ids(&mut self.cfg);
        if let Err(e) = save_config(&self.cfg) {
            self.msg = i18n::fmt(i18n::tr("Не вдалося зберегти конфіг: {}"), &[&(e)]);
        }
    }

    fn spawn<F: FnOnce() -> Msg + Send + 'static>(&self, ctx: &egui::Context, f: F) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(guarded(f));
            ctx.request_repaint();
        });
    }

    fn spawn_opt<F: FnOnce() -> Option<Msg> + Send + 'static>(&self, ctx: &egui::Context, f: F) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(m) = guarded_opt(f) {
                let _ = tx.send(m);
                ctx.request_repaint();
            }
        });
    }

    fn set_file(&mut self, f: PathBuf, info: Option<JarInfo>) {
        // CurseForge не приймає .mrpack: знімаємо галочку, а для іншого файлу повертаємо
        if f.extension().is_some_and(|e| e.eq_ignore_ascii_case("mrpack")) {
            self.cf_auto_off = self.cf_auto_off || self.use_cf;
            self.use_cf = false;
        } else if self.cf_auto_off {
            (self.use_cf, self.cf_auto_off) = (true, false);
        }
        self.file_note.clear();
        self.edit_mode = false; // новий файл: якщо форма вже повна (решта пам'ятається), одразу зведення; неповна покаже майстер
        self.send_deps = false; // залежності нового файлу користувач підтверджує заново
        self.file_info = info;
        if let Some(i) = self.file_info.clone() {
            if let Some(v) = i.version.clone() {
                self.number = v;
            }
            if !i.loaders.is_empty() {
                self.cfg.last.loaders = i.loaders.iter().map(|l| l.to_string()).collect();
            }
            if !i.mc_versions.is_empty() {
                self.cfg.last.mc_versions = i.mc_versions.join(", "); // точні версії з файлу надійніші за минулі
            }
            let mut got = vec![];
            if i.version.is_some() { got.push(i18n::fmt(i18n::tr("номер версії {}"), &[&(self.number)])); }
            if let Some(t) = i.version.as_deref().and_then(detect_release_type)
                && let Some(k) = RELEASE_TYPES.iter().position(|r| *r == t)
            {
                self.release_type = k;
                got.push(i18n::fmt(i18n::tr("тип релізу {}"), &[&(t)]));
            }
            if !i.mc_versions.is_empty() { got.push(format!("Minecraft {}", self.cfg.last.mc_versions)); }
            if !i.loaders.is_empty() { got.push(i18n::fmt(i18n::tr("лоадери: {}"), &[&(loaders_text(&self.cfg.last.loaders))])); }
            if !got.is_empty() {
                self.file_note = i18n::fmt(i18n::tr("З файлу підставлено: {}. Перевірте кроки «Версія» і «Сумісність»."), &[&(got.join("; "))]);
            }
            if !i.deps.is_empty() {
                if !self.file_note.is_empty() {
                    self.file_note.push(' ');
                }
                self.file_note.push_str(&i18n::fmt(i18n::tr("Мод залежить від: {}. Додайте залежності до версії на сайті: програма їх не надсилає."), &[&(i.deps.join(", "))]));
            }
        }
        if let Some(d) = f.parent() {
            self.cfg.last.dir = d.to_string_lossy().into_owned();
        }
        self.file = Some(f);
    }

    /// Фоновий потік впав: прапорці «зайнято» не мають лишатися назавжди.
    fn on_panic(&mut self) {
        let retry = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
        (self.mc_loading, self.mr_loading, self.upd_checking, self.upd_applying, self.refreshing, self.adding, self.cf_loading) = (false, false, false, false, false, 0, false);
        self.mc_next = retry;
        for c in [&mut self.check_msg, &mut self.cf_msg, &mut self.key_msg] {
            if matches!(c, Chk::Checking) {
                *c = Chk::Err(i18n::tr("Помилка: внутрішній збій. Спробуйте ще раз.").into());
            }
        }
        self.notice = i18n::tr("Фонове завдання аварійно завершилось. Спробуйте ще раз; якщо повторюється, повідомте розробників.").into();
    }

    fn mask(&self, e: String) -> String {
        mask_secrets(&e, &[&self.cfg.modrinth_token, &self.cfg.curse_token, &self.cfg.curse_core_key])
    }

    /// Токени не потрапляють у тексти помилок: сервер міг ехо-нути заголовок.
    fn mask_msg(&self, m: Msg) -> Msg {
        let f = |r: Result<String, String>| r.map_err(|e| self.mask(e));
        match m {
            Msg::Done(p, r) => Msg::Done(p, f(r)),
            Msg::Check(r) => Msg::Check(f(r)),
            Msg::CheckCf(r) => Msg::CheckCf(f(r)),
            Msg::CheckKey(r) => Msg::CheckKey(f(r)),
            Msg::CheckProj(r) => Msg::CheckProj(f(r)),
            Msg::Found(i, r) => Msg::Found(i, r.map_err(|e| self.mask(e))),
            Msg::CfSearch(r) => Msg::CfSearch(r.map_err(|e| self.mask(e))),
            Msg::List(r) => Msg::List(r.map_err(|e| self.mask(e))),
            Msg::CfInfo(id, t, r) => Msg::CfInfo(id, t, r.map_err(|e| self.mask(e))),
            Msg::Updated(r) => Msg::Updated(r.map_err(|e| self.mask(e))),
            other => other,
        }
    }

    fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            let m = self.mask_msg(m);
            match m {
                Msg::Panicked => self.on_panic(),
                Msg::McList(r) => {
                    self.mc_loading = false;
                    match r {
                        Ok(l) => {
                            self.mc_known = l.iter().filter(|g| g.1).map(|g| g.0.clone()).collect();
                            self.mc_all = l.into_iter().map(|g| g.0).collect();
                            if self.mc_all.is_empty() {
                                self.mc_next = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
                            }
                        }
                        // без мережі пробуємо знову через 30 с
                        Err(_) => self.mc_next = Some(std::time::Instant::now() + std::time::Duration::from_secs(30)),
                    }
                }
                Msg::List(r) => {
                    self.mr_loading = false;
                    match r {
                        Ok(l) => self.mr_list = l,
                        Err(e) => self.mr_msg = i18n::fmt(i18n::tr("Не вдалося завантажити проєкти: {}"), &[&(e)]),
                    }
                }
                Msg::Check(r) => {
                    if r.is_ok() { self.cfg.modrinth_checked = now_secs(); self.save(); }
                    self.check_msg = match r {
                        Ok(n) => Chk::Ok(i18n::fmt(i18n::tr("Готово: {}"), &[&(n)])),
                        Err(e) => Chk::Err(i18n::fmt(i18n::tr("Помилка: {}"), &[&(e)])),
                    }
                }
                Msg::CheckCf(r) => {
                    if r.is_ok() { self.cfg.curse_checked = now_secs(); self.save(); }
                    self.cf_msg = r.map_or_else(|e| Chk::Err(i18n::fmt(i18n::tr("Помилка: {}"), &[&(e)])), |m| Chk::Ok(i18n::fmt(i18n::tr("Готово: {}"), &[&(m)])));
                }
                Msg::CheckProj(r) => self.proj_chk = r.map_or_else(|e| Chk::Err(i18n::fmt(i18n::tr("Помилка: {}"), &[&(e)])), Chk::Ok),
                Msg::CheckKey(r) => {
                    if r.is_ok() { self.cfg.core_checked = now_secs(); self.save(); }
                    self.key_msg = r.map_or_else(|e| Chk::Err(i18n::fmt(i18n::tr("Помилка: {}"), &[&(e)])), |m| Chk::Ok(i18n::fmt(i18n::tr("Готово: {}"), &[&(m)])));
                }
                Msg::CfInfo(id, target, r) => {
                    self.adding = self.adding.saturating_sub(1);
                    let (name, icon) = match r {
                        Ok(v) => v,
                        Err(e) => {
                            if e.contains("HTTP 404") {
                                self.notice = i18n::fmt(i18n::tr("Проєкт з ID {} не знайдено на CurseForge: перевірте ID."), &[&(id)]);
                                self.cf_id.clear();
                                continue;
                            }
                            self.add_msg = i18n::fmt(i18n::tr("Core API: {}. Додано вручну."), &[&(e)]);
                            Default::default()
                        }
                    };
                    let name = if name.is_empty() { format!("CurseForge #{id}") } else { name };
                    if !self.add_msg.is_empty() && self.add_msg.starts_with("Core API") {
                        self.msg = std::mem::take(&mut self.add_msg); // показуємо на сторінці проєктів
                    }
                    self.target = target;
                    self.add_project(Project { name, curse_id: id, icon_url: icon, ..Default::default() });
                    self.cf_id.clear();
                }
                Msg::CfSearch(r) => {
                    self.cf_loading = false;
                    match r {
                        Ok(h) => {
                            if h.is_empty() { self.add_msg = i18n::tr("Нічого не знайдено за цією назвою.").into(); }
                            self.cf_hits = h;
                        }
                        Err(e) => self.add_msg = i18n::fmt(i18n::tr("Core API: {}. Пошук не вдався."), &[&(e)]),
                    }
                }
                Msg::Found(target, r) => {
                    self.adding = self.adding.saturating_sub(1);
                    match r {
                        Ok(p) => {
                            self.target = target;
                            self.add_project(*p);
                        }
                        Err(e) => self.msg = i18n::fmt(i18n::tr("Не вдалося додати за посиланням: {}"), &[&(e)]),
                    }
                }
                Msg::File(f, info) => self.set_file(f, info),
                Msg::ExtraFile(f) => {
                    if self.extra_files.len() < MAX_EXTRA && !self.extra_files.contains(&f) {
                        self.extra_files.push(f);
                    }
                }
                Msg::Changelog(t) => self.changelog = t,
                Msg::Notice(t) => self.notice = t,
                Msg::Update(r, manual) => {
                    self.upd_checking = false;
                    if r.is_ok() {
                        self.cfg.update_checked = now_secs();
                        self.save();
                    }
                    match r {
                        Ok(Some(u)) if update_visible(&u.version, &self.cfg.update_skip, manual) => self.update = Some(u),
                        Ok(Some(_)) => {} // фонова перевірка: цю версію користувач пропустив
                        Ok(None) if manual => {
                            self.notice = if UPDATE_REPO.contains("OWNER") { i18n::tr("Репозиторій оновлень ще не налаштовано (UPDATE_REPO у logic.rs).").into() } else { i18n::fmt(i18n::tr("У вас остання версія ({})."), &[&(env!("CARGO_PKG_VERSION"))]) };
                        }
                        Err(e) if manual => self.upd_err = i18n::fmt(i18n::tr("Не вдалося перевірити оновлення: {}"), &[&(e)]),
                        _ => {} // фонова перевірка при старті мовчить
                    }
                }
                Msg::Updated(r) => match r {
                    // нова копія вже запущена (restart_checked) і чекає, поки ми відпустимо блокування
                    Ok(()) => self.quit = true,
                    Err(e) => {
                        self.upd_applying = false;
                        self.upd_err = e;
                    }
                },
                Msg::Refreshed(list) => {
                    self.refreshing = false;
                    let n = list.len();
                    for (mr, cf, name, icon) in list {
                        // збіг за ID, бо список міг змінитись, поки йшов запит; ім'я чіпаємо лише порожнє
                        if let Some(p) = self.cfg.projects.iter_mut().find(|p| p.modrinth_id == mr && p.curse_id == cf) {
                            if p.name.trim().is_empty() { p.name = name; }
                            if !icon.is_empty() { p.icon_url = icon; }
                        }
                    }
                    self.notice = i18n::fmt(i18n::tr("Оновлено дані проєктів: {}."), &[&(n)]);
                    self.save();
                }
                Msg::Imported(ps) => {
                    let (added, merged) = merge_projects(&mut self.cfg, ps);
                    self.notice = i18n::fmt(i18n::tr("Імпорт: додано {}, доповнено {}."), &[&(added), &(merged)]);
                    self.save();
                }
                Msg::Done(p, r) => {
                    self.pending = self.pending.saturating_sub(1);
                    let ctx = HistCtx {
                        project_id: self.cur_idx.and_then(|i| self.cfg.projects.get(i)).map(|p| p.id.clone()).unwrap_or_default(),
                        name: self.cur_name.clone(),
                        release_type: self.cur_meta.2.clone(),
                        mc_versions: self.cur_meta.0.clone(),
                        loaders: self.cur_meta.1.clone(),
                    };
                    history_add_ctx(&self.cur.0, &self.cur.1, if matches!(p, P::Mr) { "Modrinth" } else { "CurseForge" }, &r, &ctx);
                    self.history = history_load(HIST_MAX);
                    if r.is_ok() && let Some(i) = self.cur_idx.take() {
                        if let Some(p) = self.cfg.projects.get_mut(i) {
                            p.last_number = self.cur.1.clone();
                            p.last_changelog = self.cur_cl.clone();
                            p.draft = Draft::default();
                            (p.mc_versions, p.loaders, p.release_type) = self.cur_meta.clone();
                        }
                        self.save();
                    }
                    let (slot, flag) = match p {
                        P::Mr => (&mut self.mr_res, &mut self.use_mr),
                        P::Cf => (&mut self.cf_res, &mut self.use_cf),
                    };
                    if r.is_ok() {
                        *flag = false; // захист від дубля
                    }
                    *slot = Some(r);
                }
            }
        }
    }

    /// Після оновлення: через STABLE_SECS живого UI підтверджуємо стабільність (маркер, прибирання .old).
    /// Якщо минулий запуск після оновлення не дожив до маркера, пропонуємо повернути попередню версію.
    fn update_guard(&mut self, ctx: &egui::Context) {
        match self.start_act {
            StartAction::AwaitStable => {
                let t = *self.stable_at.get_or_insert_with(std::time::Instant::now);
                let left = std::time::Duration::from_secs(STABLE_SECS).saturating_sub(t.elapsed());
                if left.is_zero() {
                    confirm_update();
                    self.start_act = StartAction::Clean;
                } else {
                    ctx.request_repaint_after(left);
                }
            }
            StartAction::OfferRollback => {
                if self.quit {
                    return; // повторний Enter, поки програма виходить: відкат не повторюємо
                }
                let (mut back, mut keep) = (false, false);
                egui::Modal::new(egui::Id::unique("rollback_dialog")).backdrop_color(Color32::from_black_alpha(160)).frame(modal_frame()).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.label(title(i18n::tr("Нова версія не стартувала стабільно")));
                    ui.add_space(8.0);
                    ui.label(i18n::tr("Після останнього оновлення програма закрилась раніше, ніж підтвердила нормальну роботу. Повернути попередню версію?"));
                    hint(ui, i18n::tr("Збережена копія попередньої версії лежить поруч із програмою (modwire.exe.old)."));
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        if ghost_btn(ui, None, i18n::tr("Лишити цю")).clicked() { keep = true; }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(primary(i18n::tr("Повернути попередню"))).clicked() { back = true; }
                        });
                    });
                });
                if back {
                    match rollback_running().and_then(|()| relaunch()) {
                        Ok(()) => self.quit = true,
                        Err(e) => {
                            self.upd_err = e;
                            self.start_act = StartAction::Clean;
                        }
                    }
                } else if keep {
                    confirm_update();
                    self.start_act = StartAction::Clean;
                }
            }
            StartAction::Clean => {}
        }
    }

    /// Діалог при закритті вікна під час публікації: лишитись або перервати й вийти.
    fn close_dialog(&mut self, ctx: &egui::Context) {
        dialog_focus(ctx, "close", self.confirm_close);
        let fid = egui::Id::NULL.with("close_focus_done");
        if !self.confirm_close {
            ctx.data_mut(|d| d.insert_temp(fid, false));
            return;
        }
        // безпечний вибір отримує фокус одразу: випадковий Enter не перериває публікацію
        let fresh = !ctx.data_mut(|d| d.get_temp::<bool>(fid)).unwrap_or(false);
        ctx.data_mut(|d| d.insert_temp(fid, true));
        let (mut stay, mut quit) = (false, false);
        let r = egui::Modal::new(egui::Id::unique("close_dialog")).backdrop_color(Color32::from_black_alpha(160)).frame(modal_frame()).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(title(i18n::tr("Публікація триває")));
            ui.add_space(8.0);
            ui.label(i18n::tr("Файл ще надсилається. Якщо закрити програму зараз, публікацію буде перервано, а версія могла частково з'явитись на сайті."));
            hint(ui, i18n::tr("Після виходу перевірте результат на сторінці проєкту."));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.add(danger(i18n::tr("Перервати й вийти"))).clicked() { quit = true; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = ui.add(primary(i18n::tr("Лишитись")));
                    if fresh { r.request_focus(); }
                    if r.clicked() { stay = true; }
                });
            });
        });
        if quit {
            self.up_mr.cancel.store(true, Relaxed);
            self.up_cf.cancel.store(true, Relaxed);
            self.close_ok = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if stay || r.should_close() {
            self.confirm_close = false;
        }
    }

    /// Змінює обраний проєкт і скидає стан попередньої публікації.
    fn select_project(&mut self, i: Option<usize>) {
        if self.sel != i {
            // незавершена форма лишається за проєктом і повертається, коли до нього повернуться
            if let Some(p) = self.sel.and_then(|o| self.cfg.projects.get_mut(o)) {
                p.draft = Draft { project: p.id.clone(), number: self.number.clone(), name: self.name.clone(), changelog: self.changelog.clone(), file: String::new() };
            }
            self.sel = i;
            self.edit_mode = true; // інший проєкт: майстер, а не зведення з файлом попереднього
            (self.use_mr, self.use_cf, self.mr_res, self.cf_res) = (true, true, None, None);
            self.cf_auto_off = false;
            // номер із попередньої публікації цього проєкту, changelog не переносимо між проєктами
            self.number = i.and_then(|i| self.cfg.projects.get(i)).map(|p| bump(&p.last_number)).unwrap_or_default();
            self.changelog.clear();
            self.name.clear();
            if let Some(d) = i.and_then(|i| self.cfg.projects.get(i)).filter(|p| p.draft.applies_to(p)).map(|p| p.draft.clone()) {
                if !d.number.trim().is_empty() {
                    self.number = d.number;
                }
                (self.name, self.changelog) = (d.name, d.changelog);
            }
            // MC/лоадери/тип останньої публікації цього проєкту (різні моди: різні налаштування)
            if let Some(p) = i.and_then(|i| self.cfg.projects.get(i)) {
                if !p.mc_versions.is_empty() { self.cfg.last.mc_versions = p.mc_versions.clone(); }
                if !p.loaders.is_empty() { self.cfg.last.loaders = p.loaders.clone(); }
                if let Some(t) = RELEASE_TYPES.iter().position(|r| *r == p.release_type) { self.release_type = t; }
            }
        }
    }

    /// «Повторити з цього запису»: проєкт, номер, назва версії, тип, MC і лоадери з історії; файл і зміни обирають заново.
    fn restore_history(&mut self, e: &serde_json::Value) {
        let Some(r) = restore_from_history(e) else { return };
        let name = e["project"].as_str().unwrap_or("");
        let idx = self.cfg.projects.iter().position(|p| !r.draft.project.is_empty() && p.id == r.draft.project).or_else(|| self.cfg.projects.iter().position(|p| p.name == name));
        let Some(i) = idx else {
            self.notice = i18n::fmt(i18n::tr("Проєкт «{}» уже видалено зі списку: форму не відновлено."), &[&(name)]);
            return;
        };
        self.select_project(Some(i));
        self.number = r.draft.number;
        self.name = r.draft.name;
        if let Some(t) = RELEASE_TYPES.iter().position(|x| *x == r.release_type) {
            self.release_type = t;
        }
        if !r.mc_versions.is_empty() {
            self.cfg.last.mc_versions = r.mc_versions;
        }
        if !r.loaders.is_empty() {
            self.cfg.last.loaders = r.loaders;
        }
        (self.use_mr, self.use_cf, self.mr_res, self.cf_res) = (true, true, None, None);
        (self.tab, self.pub_step, self.edit_mode) = (Tab::Publish, 1, true); // з історії на крок «Файл»: його й просить повідомлення нижче
        self.notice = i18n::tr("Форму відновлено із запису історії. Оберіть файл і перевірте зміни.").into();
    }

    /// Видаляє запис проєкту i і зсуває всі збережені індекси.
    fn remove_project(&mut self, i: usize) {
        self.confirm_del = None;
        if i >= self.cfg.projects.len() {
            return; // застарілий індекс (подвійне підтвердження): нічого не видаляємо і не панікуємо
        }
        self.cfg.projects.remove(i);
        let shift = |o: Option<usize>| match o { Some(s) if s == i => None, Some(s) if s > i && s != usize::MAX => Some(s - 1), o => o }; // MAX = маркер «немає», не зсуваємо
        let new_sel = shift(self.sel);
        if self.sel == Some(i) {
            self.sel = Some(usize::MAX); // видалений: його форму не можна записати в чужу чернетку
        }
        if new_sel.is_none() {
            self.select_project(None); // видалили обраний: скидаємо форму
        } else {
            self.sel = new_sel; // лише зсув індексу, введене не губимо
        }
        self.target = shift(self.target);
        self.cur_idx = shift(self.cur_idx); // публікація в процесі: індекс не має вказувати на чужий проєкт
        if self.cfg.last.project != usize::MAX {
            self.cfg.last.project = shift(Some(self.cfg.last.project)).unwrap_or(usize::MAX); // MAX = «немає проєкту», його не зсуваємо
        }
        self.save();
    }

    /// Переставляє проєкти за perm[нова] = стара і переводить усі збережені індекси на нові позиції.
    fn reorder(&mut self, perm: &[usize]) {
        let n = self.cfg.projects.len();
        if perm.len() != n || perm.iter().enumerate().all(|(a, &b)| a == b) {
            return;
        }
        let mut inv = vec![0; n];
        for (new, &old) in perm.iter().enumerate() {
            inv[old] = new;
        }
        let old = std::mem::take(&mut self.cfg.projects);
        self.cfg.projects = perm.iter().map(|&o| old[o].clone()).collect();
        let map = |o: Option<usize>| o.and_then(|i| inv.get(i).copied());
        self.sel = map(self.sel);
        self.target = map(self.target);
        self.confirm_del = map(self.confirm_del);
        self.cur_idx = map(self.cur_idx);
        self.cfg.last.project = map(Some(self.cfg.last.project)).unwrap_or(usize::MAX);
        self.save();
    }

    /// Додає до обраного target або створює новий; зберігає конфіг.
    fn add_project(&mut self, p: Project) {
        let dup = |q: &Project| (!p.modrinth_id.is_empty() && q.modrinth_id == p.modrinth_id) || (!p.curse_id.is_empty() && q.curse_id == p.curse_id);
        let target = self.target.filter(|i| *i < self.cfg.projects.len()).or_else(|| self.cfg.projects.iter().position(dup));
        if target.is_none() {
            // Та сама назва, але інша платформа: питаємо, чи це один мод.
            let key = p.name.trim().to_lowercase();
            if let Some(i) = self.cfg.projects.iter().position(|q| q.name.trim().to_lowercase() == key) {
                self.merge_ask.push((i, p)); // у черзі, щоб не втратити попереднє питання
                self.merge_hidden = false;
                return;
            }
        }
        match target {
            Some(i) => self.merge_into(i, p),
            None => {
                self.cfg.projects.push(p);
                self.save();
            }
        }
    }

    /// Додає ID платформ із p до існуючого проєкту i.
    fn merge_into(&mut self, i: usize, p: Project) {
        let t = &mut self.cfg.projects[i];
        if !p.modrinth_id.is_empty() { t.modrinth_id = p.modrinth_id; }
        if !p.curse_id.is_empty() { t.curse_id = p.curse_id; }
        if t.icon_url.is_empty() { t.icon_url = p.icon_url; }
        self.notice = i18n::fmt(i18n::tr("Проєкт «{}» уже був у списку: ID платформ оновлено."), &[&(self.cfg.projects[i].name)]);
        self.save();
    }

    /// Питання про однакову назву: об'єднати з наявним проєктом чи створити окремий.
    fn merge_dialog(&mut self, ctx: &egui::Context) {
        dialog_focus(ctx, "merge", !self.merge_ask.is_empty() && !self.merge_hidden);
        let Some((i, p)) = self.merge_ask.first().cloned() else { return };
        let Some(cur) = self.cfg.projects.get(i).cloned() else {
            self.merge_ask.remove(0); // проєкт зник — питання більше не актуальне
            return;
        };
        // Інший ID тієї самої платформи буде замінено — показуємо це в діалозі.
        let replaces = (!p.curse_id.is_empty() && !cur.curse_id.is_empty() && p.curse_id != cur.curse_id)
            || (!p.modrinth_id.is_empty() && !cur.modrinth_id.is_empty() && p.modrinth_id != cur.modrinth_id);
        if self.merge_hidden {
            return; // Esc закрив діалог без рішення: питання лишається в черзі
        }
        let mut choice = None;
        let mut hide = false;
        let r = egui::Modal::new(egui::Id::unique("merge_dialog")).backdrop_color(Color32::from_black_alpha(160)).frame(modal_frame()).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.horizontal(|ui| {
                ui.label(title(i18n::tr("Однакова назва")));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if close_x(ui).clicked() { hide = true; }
                });
            });
            ui.add_space(8.0);
            ui.label(i18n::fmt(i18n::tr("У списку вже є проєкт «{}». Це один і той самий мод?"), &[&(cur.name)]));
            hint(ui, i18n::tr("Якщо так, платформи об'єднаються в один проєкт."));
            if replaces {
                ui.add_space(8.0);
                banner(ui, Kind::Warn, i18n::tr("Для цієї платформи вже вказано інший ID — він буде замінений."));
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ghost_btn(ui, None, i18n::tr("Ні, окремий проєкт")).clicked() { choice = Some(false); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(primary(i18n::tr("Так, об'єднати"))).clicked() { choice = Some(true); }
                });
            });
        });
        if (hide || r.should_close()) && choice.is_none() {
            self.merge_hidden = true;
        }
        match choice {
            Some(true) => {
                self.merge_ask.remove(0);
                self.merge_into(i, p);
            }
            Some(false) => {
                self.merge_ask.remove(0);
                self.cfg.projects.push(p);
                self.save();
            }
            None => {}
        }
    }

    fn rail(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            let icon = ui.add(egui::Image::from_bytes("bytes://modwire-icon.png", include_bytes!("../assets/icon.png").as_slice()).fit_to_exact_size(egui::vec2(32.0, 32.0)).corner_radius(9));
            self.brand_rect = Some(icon.rect);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(title("Modwire"));
                ui.label(RichText::new(i18n::tr("публікація модів")).color(MUTED).size(HINT_SZ));
            });
        });
        ui.add_space(14.0);
        let y = ui.cursor().min.y;
        ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, BORDER));
        ui.add_space(14.0);
        for (t, label) in [(Tab::Projects, i18n::tr("nav.projects")), (Tab::Publish, i18n::tr("nav.publish"))] {
            self.tab_button(ui, t, label);
        }
        self.history_button(ui); // завжди в меню: розділ не зникає й не з'являється, порожня історія просто неактивна
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(MUTED.gamma_multiply(0.7)).size(12.0));
            ui.add_space(4.0);
            self.tab_button(ui, Tab::Settings, i18n::tr("nav.settings"));
            // знизу вгору: ця кнопка стоїть прямо над вкладкою «Налаштування»
            // у низькому вікні меню й так тісне: донат не відбирає місце в навігації
            if let Some(url) = ads::DONATE_URL.filter(|_| ui.ctx().content_rect().height() >= 560.0) {
                ui.add_space(8.0);
                // не `primary`: єдина головна дія застосунку — «Опублікувати»
                if ghost_btn(ui, None, i18n::tr("Підтримати автора")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        });
    }

    /// Пункт меню: справжня кнопка (фокус з клавіатури), іконка і текст малюються поверх.
    fn tab_button(&mut self, ui: &mut egui::Ui, t: Tab, label: &str) {
        // коли відкрита історія, активний її пункт, а не «Публікація»
        let hist_open = t == Tab::Publish && self.pub_step == 6 && !self.history.is_empty();
        // нумерація збігається з порядком меню зверху вниз: Проєкти, Публікація, Історія, Налаштування
        let key = match t { Tab::Projects => "Ctrl+1", Tab::Publish => "Ctrl+2", Tab::Settings => "Ctrl+4" };
        let kind = match t {
            Tab::Projects => Ic::Grid,
            Tab::Publish => Ic::Upload,
            Tab::Settings => Ic::Gear,
        };
        if rail_item(ui, self.tab == t && !hist_open, true, label, key, kind, "") {
            self.tab = t;
            if hist_open || (t == Tab::Publish && self.pub_step == 6) { self.pub_step = 5; } // назад до форми
        }
    }

    /// Пункт меню «Історія»: та сама сторінка «Публікація», крок історії. Неактивний, поки записів немає.
    fn history_button(&mut self, ui: &mut egui::Ui) {
        let active = self.tab == Tab::Publish && self.pub_step == 6 && !self.history.is_empty();
        let tip = if self.pending > 0 { i18n::tr("Дочекайтесь завершення публікації") } else { i18n::tr("Записи з'являться після першої публікації.") };
        if rail_item(ui, active, !self.history.is_empty() && self.pending == 0, i18n::tr("nav.history"), "Ctrl+3", Ic::Clock, tip) {
            self.open_history();
        }
    }

    /// Відкрити історію (Ctrl+3 і клік у меню); без записів нічого не робить.
    fn open_history(&mut self) {
        if self.pending > 0 {
            self.notice = i18n::tr("Дочекайтесь завершення публікації").into();
        } else if self.history.is_empty() {
            self.notice = i18n::tr("Історія порожня: записи з'являться після першої публікації.").into(); // Ctrl+3 не мовчить
        } else {
            (self.tab, self.pub_step) = (Tab::Publish, 6);
        }
    }
}

/// Пункт меню: справжня кнопка (фокус з клавіатури), іконка і текст малюються поверх. Повертає true на клік.
/// Клавіша показується в підказці при наведенні й у назві для скрінрідера; неактивний пункт пояснює чому в `tip`.
fn rail_item(ui: &mut egui::Ui, active: bool, enabled: bool, label: &str, key: &str, kind: Ic, tip: &str) -> bool {
    {
        let resp = ui.add_enabled(enabled, egui::Button::selectable(active, "").frame(false).min_size(egui::vec2(ui.available_width(), 40.0)));
        let resp = if !enabled { resp.on_disabled_hover_text(tip) } else if key.is_empty() { resp } else { resp.on_hover_text(format!("{label} ({key})")) };
        let dim = if enabled { 1.0 } else { 0.5 };
        // неактивний пункт не бере фокус, тож причину озвучуємо в самій назві
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, active, if !enabled { format!("{label}: {tip}") } else if key.is_empty() { label.to_owned() } else { i18n::fmt(i18n::tr("{}, розділ ({})"), &[&(label), &(key)]) }));
        let rect = resp.rect;
        let hov = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered(), 0.12);
        let act = ui.ctx().animate_bool_with_time(resp.id.with("act"), active, 0.15);
        let p = ui.painter();
        p.rect_filled(rect, R_CTRL, HOVER.gamma_multiply(hov * (1.0 - act)));
        p.rect_filled(rect, R_CTRL, Color32::from_rgb(0x25, 0x25, 0x27).gamma_multiply(act));
        if resp.has_focus() {
            p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, FOCUS), egui::StrokeKind::Inside);
        }
        let (icon_col, text_col) = (MUTED.lerp_to_gamma(ACCENT, act).gamma_multiply(dim), MUTED.lerp_to_gamma(TEXT, act).gamma_multiply(dim));
        draw_icon(p, egui::pos2(rect.left() + 24.0, rect.center().y), kind, 20.0, icon_col);
        p.text(egui::pos2(rect.left() + 46.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), text_col);
        resp.clicked()
    }
}

/// Справжнє натискання Ctrl+Enter, не автоповтор утриманої клавіші: egui зараховує й `repeat: true`,
/// і утримання «заряджало» б і публікувало одним натисканням.
fn fresh_ctrl_enter(events: &[egui::Event]) -> bool {
    events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, repeat: false, modifiers, .. } if modifiers.command))
}

impl App {
    // ---------- Проєкти ----------
    /// Загальне повідомлення над будь-яким розділом (результат імпорту, збереження шаблону тощо).
    fn notice_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.notice.is_empty() {
            return;
        }
        // Повідомлення зникає за 8 секунд; попередження режиму лише для читання лишається.
        if !self.readonly {
            let fresh = !matches!(&self.notice_seen, Some((t, _)) if *t == self.notice);
            if fresh {
                self.notice_seen = Some((self.notice.clone(), std::time::Instant::now()));
            }
            let left = std::time::Duration::from_secs(8).saturating_sub(self.notice_seen.as_ref().map_or_else(Default::default, |(_, at)| at.elapsed()));
            if left.is_zero() {
                self.notice.clear();
                self.notice_seen = None;
            } else {
                ctx.request_repaint_after(left);
            }
        }
        if !self.notice.is_empty() {
            banner(ui, Kind::Info, &self.notice.clone());
            ui.add_space(8.0);
        }
    }

    /// Банер «Доступна нова версія» з кнопкою «Оновити» (над будь-яким розділом).
    fn update_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if std::mem::replace(&mut self.upd_drawn, true) {
            return; // уже намальовано в цьому кадрі (перемикання розділу налаштувань)
        }
        if !self.upd_err.is_empty() {
            banner(ui, Kind::Error, &self.upd_err.clone());
            ui.horizontal(|ui| {
                // запасний шлях: якщо автооновлення не вдалося (права на запис, антивірус), можна завантажити вручну
                if let Some(page) = self.update.as_ref().map(|u| u.page.clone()).filter(|p| !p.is_empty())
                    && icon_btn(ui, Some(Ic::External), i18n::tr("Завантажити вручну"), false).clicked()
                {
                    ctx.open_url(egui::OpenUrl::new_tab(page));
                }
                // як «Пізніше» в банері оновлення: текстова кнопка без значка (значок лишається лише на дії, що відкриває посилання)
                if ghost_btn(ui, None, i18n::tr("btn.close")).clicked() {
                    self.upd_err.clear();
                }
            });
            ui.add_space(8.0);
        }
        let Some(u) = self.update.as_ref() else { return };
        let (ver, url, size, sha, page) = (u.version.clone(), u.url.clone(), u.size, u.sha256.clone(), u.page.clone());
        // одна картка: значок і текст, під ними дії; головна дія першою, «Пропустити» останньою
        egui::Frame::new().fill(Color32::from_rgb(0x1e, 0x22, 0x30)).stroke(Stroke::new(1.0, Color32::from_rgb(0x30, 0x36, 0x4e))).corner_radius(CARD_R).inner_margin(egui::Margin::same(CARD_PAD)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            icon_label_c(ui, Ic::Info, ACCENT, &i18n::fmt(i18n::tr("Доступна нова версія {} (у вас {})"), &[&(ver), &(env!("CARGO_PKG_VERSION"))]), TEXT);
            ui.add_space(2.0);
            if self.upd_applying {
                let (done, total) = update_progress();
                ui.horizontal(|ui| {
                    ui.spinner();
                    if total > 0 {
                        let f = (done as f32 / total as f32).min(1.0);
                        progress_bar(ui, f, (ui.available_width() - 56.0).max(60.0), ACCENT);
                        ui.label(RichText::new(format!("{:.0}%", f * 100.0)).color(MUTED));
                    }
                });
                hint(ui, &if total > 0 { i18n::fmt(i18n::tr("Завантажуємо оновлення… {} з {} МБ. Програма перезапуститься сама."), &[&(done >> 20), &(total >> 20)]) } else { i18n::tr("Завантажуємо оновлення… Програма перезапуститься сама.").to_owned() });
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
            ui.horizontal_wrapped(|ui| {
                if url.is_empty() || !cfg!(windows) {
                    if icon_btn(ui, Some(Ic::External), i18n::tr("Відкрити сторінку релізу"), true).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(page));
                    }
                } else {
                    let can = self.pending == 0 && self.adding == 0;
                    if ui.add_enabled(can, primary(i18n::tr("Оновити"))).on_disabled_hover_text(i18n::tr("Дочекайтесь завершення публікації")).clicked() {
                        self.upd_applying = true;
                        self.upd_err.clear();
                        self.spawn(ctx, move || Msg::Updated(apply_update(&url, size, &sha).and_then(|()| restart_checked())));
                    }
                }
                if ghost_btn(ui, None, i18n::tr("Пізніше")).clicked() {
                    self.update = None;
                }
                if ghost_btn(ui, None, i18n::tr("Пропустити цю версію")).on_hover_text(i18n::tr("Не нагадувати про неї; нові версії все одно покажуться")).clicked() {
                    self.cfg.update_skip = ver.clone();
                    self.update = None;
                    self.save();
                }
            });
        });
        ui.add_space(8.0);
    }

    fn projects_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.merge_hidden && !self.merge_ask.is_empty() {
            banner(ui, Kind::Warn, i18n::tr("Є невирішене питання: проєкт з такою назвою вже є. Видалення проєктів призупинено."));
            if ghost_btn(ui, None, i18n::tr("Вирішити")).clicked() {
                self.merge_hidden = false;
            }
            ui.add_space(8.0);
        }
        let no_tokens = self.cfg.modrinth_token.trim().is_empty() && self.cfg.curse_token.trim().is_empty();
        let no_projects = self.cfg.projects.is_empty();

        // заголовок і кнопка в одному рядку, підказка нижче на всю ширину (інакше текст заходить під кнопку)
        ui.horizontal(|ui| {
            ui.heading(i18n::tr("nav.projects"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !no_projects && icon_btn(ui, Some(Ic::Plus), i18n::tr("btn.add_project"), true).clicked() {
                    self.add_open = true;
                    self.target = None;
                    self.add_msg.clear();
                }
            });
        });
        hint(ui, i18n::tr("Ваші моди на Modrinth і CurseForge. Видалення не чіпає сайти."));
        ui.add_space(12.0);
        if !self.msg.is_empty() {
            banner(ui, Kind::Warn, &self.msg);
            ui.add_space(8.0);
        }

        if no_tokens && !no_projects {
            // лише те, чого бракує: проєкт уже є, тож рядок «Проєкт додано» і заголовок «Швидкий старт» були зайвими
            let mut to_settings = false;
            egui::Frame::new().fill(FIELD).stroke(Stroke::new(1.0, UI_BORDER)).corner_radius(egui::CornerRadius::same(R_CTRL)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(CTRL_H);
                ui.horizontal_wrapped(|ui| {
                    badge_ic(ui, i18n::tr("Потрібно"), WARN, Some(Ic::Warn)); // не бурштин: бурштин лише для головної дії
                    ui.label(i18n::tr("Для публікації потрібен токен Modrinth або CurseForge"));
                    to_settings = ghost_btn(ui, Some(Ic::Gear), i18n::tr("nav.settings")).clicked();
                });
            });
            ui.add_space(12.0);
            if to_settings {
                (self.tab, self.set_sec) = (Tab::Settings, 0);
            }
        }

        if no_projects {
            // перший запуск: знак, заголовок і один головний заклик; токен — тихий другий крок нижче
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.add(egui::Image::from_bytes("bytes://modwire-icon.png", include_bytes!("../assets/icon.png").as_slice()).fit_to_exact_size(egui::vec2(72.0, 72.0)).corner_radius(20));
                ui.add_space(8.0);
                ui.label(RichText::new(i18n::tr("Додайте свій перший мод")).size(22.0).color(TEXT));
                ui.label(RichText::new("Один раз вкажіть, де мод є на сайтах, і публікуйте нові версії в Modrinth і CurseForge одним кліком.").color(MUTED).size(HINT_SZ));
                ui.add_space(8.0);
                if icon_btn(ui, Some(Ic::Plus), i18n::tr("btn.add_project"), true).clicked() {
                    self.add_open = true;
                    self.target = None;
                    self.add_msg.clear();
                }
                if no_tokens {
                    ui.add_space(12.0);
                    hint(ui, i18n::tr("Для публікації знадобиться токен платформи."));
                    if ghost_btn(ui, Some(Ic::Gear), i18n::tr("nav.settings")).clicked() {
                        (self.tab, self.set_sec) = (Tab::Settings, 0);
                    }
                }
            });
        }

        // Сітка карток: кількість колонок залежить від ширини вікна.
        let n = self.cfg.projects.len();
        let cols = ((ui.available_width() + GAP) / (CARD_MIN_W + GAP)).floor().max(1.0) as usize;
        let (mut publish, mut delete, mut ask, mut cancel) = (None, None, None, false);
        let (mut pin, mut sort, mut pin_only) = (None::<usize>, false, false);
        let (mut dup, mut toggle, mut batch) = (None::<usize>, None::<String>, false);
        let sel_mode = self.sel_mode;
        // Esc скасовує підтвердження видалення (одиночного й пакетного)
        if (self.batch_confirm || self.confirm_del.is_some()) && !self.add_open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            (self.batch_confirm, self.confirm_del) = (false, None);
        }
        let confirm = self.confirm_del;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if n > 1 {
                ui.horizontal_wrapped(|ui| {
                    if n > 6 {
                        let r = ui.add(egui::TextEdit::singleline(&mut self.proj_q).hint_text(i18n::tr("Пошук за назвою")).min_size(egui::vec2(0.0, CTRL_H)).desired_width(260.0_f32.min(ui.available_width() - 8.0)));
                        name_edit(&r, i18n::tr("Пошук проєктів"), i18n::tr("Пошук за назвою"), &self.proj_q);
                    }
                    if icon_btn(ui, None, i18n::tr("Сортувати за назвою"), false).on_hover_text(i18n::tr("Закріплені першими, далі за алфавітом")).clicked() {
                        sort = true;
                    }
                    if icon_btn(ui, Some(Ic::Check), if self.sel_mode { i18n::tr("btn.done") } else { i18n::tr("Обрати") }, false).on_hover_text(i18n::tr("Обрати кілька проєктів, щоб видалити їх разом")).clicked() {
                        self.sel_mode = !self.sel_mode;
                        self.picked.clear();
                        self.batch_confirm = false;
                    }
                    if self.sel_mode && !self.picked.is_empty() {
                        if self.batch_confirm {
                            batch = ui.add(danger(&i18n::fmt(i18n::tr("Точно видалити ({})?"), &[&(self.picked.len())]))).clicked();
                            if ghost_btn(ui, Some(Ic::Cross), i18n::tr("btn.cancel")).clicked() {
                                self.batch_confirm = false;
                            }
                            hint(ui, i18n::tr("Видалить записи в Modwire. На сайтах нічого не зміниться."));
                        } else if ui.add(danger(&i18n::fmt(i18n::tr("Видалити вибрані ({})"), &[&(self.picked.len())]))).clicked() {
                            self.batch_confirm = true;
                        }
                    }
                });
                ui.add_space(8.0);
            }
            let vis = filter_projects(&self.cfg.projects, &self.proj_q);
            if n > 6 && vis.is_empty() {
                hint(ui, i18n::tr("Нічого не знайдено за цією назвою."));
            }
            for start in (0..vis.len()).step_by(cols) {
                ui.columns(cols, |c| {
                    for (k, col_ui) in c.iter_mut().enumerate() {
                        let Some(&i) = vis.get(start + k) else { continue };
                        let Some(p) = self.cfg.projects.get(i) else { continue };
                        card_hover(col_ui, |ui| {
                            ui.set_width(ui.available_width());
                            // підтвердження видалення довше за звичайну картку: даємо місце, щоб текст не лягав на бейджі
                            let card_h = if confirm == Some(i) { PROJECT_CARD_H + 64.0 } else { PROJECT_CARD_H };
                            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), card_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.horizontal(|ui| {
                                    if sel_mode && !p.id.is_empty() {
                                        let mut on = self.picked.contains(&p.id);
                                        if aria_check(check_row(ui, &mut on, "").on_hover_text(i18n::tr("Обрати для видалення")), i18n::fmt(i18n::tr("Обрати проєкт «{}» для видалення"), &[&(p.name)]), on).changed() {
                                            toggle = Some(p.id.clone());
                                        }
                                    }
                                    icon(ui, &p.icon_url, &p.name, 44.0);
                                    if p.pinned {
                                        let r = egui::Rect::from_center_size(egui::pos2(ui.max_rect().right() - 8.0, ui.max_rect().top() + 10.0), egui::vec2(16.0, 16.0));
                                        draw_icon(ui.painter(), r.center(), Ic::Pin, 14.0, ACCENT);
                                    }
                                    ui.vertical(|ui| {
                                        if p.pinned { ui.set_max_width(ui.available_width() - 22.0); }
                                        let mut job = egui::text::LayoutJob::single_section(p.name.clone(), egui::TextFormat::simple(egui::TextStyle::Name("Title".into()).resolve(ui.style()), TEXT));
                                        job.wrap.max_rows = 2;
                                        ui.add(egui::Label::new(job).wrap());
                                        ui.horizontal_wrapped(|ui| {
                                            if !p.modrinth_id.is_empty() { brand_pill(ui, "Modrinth", MR_GREEN, MR_BG); }
                                            if !p.curse_id.is_empty() { brand_pill(ui, "CurseForge", CF_ORANGE, CF_BG); }
                                            if p.modrinth_id.is_empty() && p.curse_id.is_empty() { hint(ui, i18n::tr("Платформи не підключені")); }
                                        });
                                    });
                                });
                                // кнопки притиснуті донизу: bottom_up у решті висоти картки
                                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                                    let row = |ui: &mut egui::Ui| {
                                        if confirm == Some(i) {
                                            // підтвердження замінює дії картки: «Точно видалити?» не стоїть упритул до «Опублікувати»
                                            if ui.add(danger(i18n::tr("Точно видалити?"))).clicked() { delete = Some(i); }
                                            if ghost_btn(ui, Some(Ic::Cross), i18n::tr("btn.cancel")).clicked() { cancel = true; }
                                        } else {
                                            // бурштинова лише в проєкту, з яким працюють зараз; в інших тиха
                                            let go = if self.sel == Some(i) { ui.add(primary(i18n::tr("btn.publish"))) } else { ghost_btn(ui, None, i18n::tr("btn.publish")) };
                                            if go.clicked() { publish = Some(i); }
                                            if aria(icon_only(ui, Ic::Copy, i18n::tr("Дублювати проєкт: копія з тими самими налаштуваннями"), false), i18n::fmt(i18n::tr("Дублювати проєкт «{}»"), &[&(p.name)])).clicked() { dup = Some(i); }
                                            if aria_check(icon_only(ui, Ic::Pin, if p.pinned { i18n::tr("Відкріпити") } else { i18n::tr("Закріпити: закріплені проєкти йдуть першими") }, p.pinned), i18n::fmt(i18n::tr("Закріпити проєкт «{}»"), &[&(p.name)]), p.pinned).clicked() { pin = Some(i); }
                                            if ui.add_enabled_ui(self.pending == 0 && self.adding == 0, |ui| aria(icon_only(ui, Ic::Trash, i18n::tr("Видалити запис у Modwire"), false), i18n::fmt(i18n::tr("Видалити проєкт «{}» зі списку Modwire"), &[&(p.name)]))).inner.on_disabled_hover_text(i18n::tr("Недоступно, поки триває публікація або додавання")).clicked() {
                                                ask = Some(i);
                                            }
                                        }
                                    };
                                    if confirm == Some(i) { ui.horizontal_wrapped(row); } else { ui.horizontal(row); }
                                    if confirm == Some(i) {
                                        hint(ui, i18n::tr("Видалити запис у Modwire? На сайтах нічого не зміниться"));
                                    }
                                });
                            });
                        });
                    }
                });
                ui.add_space(GAP);
            }
            if n > 0 { sponsor_tile(ui); } // одна тиха плитка під останньою карткою, прокручується разом зі списком
        });
        if self.pending > 0 || self.adding > 0 || !self.merge_ask.is_empty() {
            (ask, delete, pin, sort) = (None, None, None, false); // індекси не можна зсувати під час публікації
        }
        if let Some(i) = pin && let Some(p) = self.cfg.projects.get_mut(i) {
            p.pinned = !p.pinned;
            sort = true; // закріплення одразу піднімає проєкт нагору
            pin_only = true; // без перевпорядкування решти за алфавітом
        }
        if sort {
            let perm = if pin_only { pin_perm(&self.cfg.projects) } else { sort_perm(&self.cfg.projects) };
            self.reorder(&perm);
            self.save();
        }
        if let Some(i) = ask {
            self.confirm_del = Some(i);
        }
        if cancel {
            self.confirm_del = None;
        }
        if let Some(i) = publish {
            if self.pending == 0 {
                self.select_project(Some(i)); // під час публікації проєкт не міняємо
            }
            self.tab = Tab::Publish;
        }
        if let Some(i) = delete {
            self.remove_project(i);
        }
        if let Some(id) = toggle {
            if !self.picked.remove(&id) {
                self.picked.insert(id);
            }
            self.batch_confirm = false;
        }
        if self.pending == 0 && self.adding == 0 && self.merge_ask.is_empty() {
            if let Some(k) = dup.and_then(|i| duplicate_project(&mut self.cfg, i)) {
                self.notice = i18n::fmt(i18n::tr("Створено копію: «{}»."), &[&(self.cfg.projects[k].name)]);
                self.save();
            }
            if batch {
                let mut idx: Vec<usize> = (0..self.cfg.projects.len()).filter(|&i| !self.cfg.projects[i].id.is_empty() && self.picked.contains(&self.cfg.projects[i].id)).collect();
                idx.reverse(); // з кінця, щоб індекси не зсувались
                let cnt = idx.len();
                idx.into_iter().for_each(|i| self.remove_project(i));
                (self.sel_mode, self.batch_confirm) = (false, false);
                self.picked.clear();
                self.notice = i18n::fmt(i18n::tr("Видалено проєктів: {}. На сайтах нічого не змінено."), &[&(cnt)]);
            }
        }
        self.add_dialog(ctx);
    }

    fn add_dialog(&mut self, ctx: &egui::Context) {
        dialog_focus(ctx, "add", self.add_open);
        if !self.add_open {
            self.add_shown = false;
            return;
        }
        let just_opened = !self.add_shown;
        self.add_shown = true;
        let mut close = false;
        let r = egui::Modal::new(egui::Id::unique("add_dialog")).backdrop_color(Color32::from_black_alpha(160)).frame(modal_frame())
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 80.0).clamp(480.0, 720.0)); // ширше, але не більше за вікно
                ui.horizontal(|ui| {
                    ui.label(RichText::new(i18n::tr("btn.add_project")).size(22.0).color(TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if close_x(ui).clicked() { close = true; }
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt("add_scroll").max_height((ctx.content_rect().height() - 200.0).max(200.0)).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(ui.available_width() - 12.0);
                hint(ui, i18n::tr("Мод має вже існувати на сайті. Оберіть, де він є, і він з'явиться у списку. Можна додати обидві платформи до одного проєкту."));
                ui.add_space(12.0);
                // «Куди додати»: підпис ліворуч, список праворуч
                let label = |i: Option<usize>, ps: &[Project]| match i.and_then(|i| ps.get(i)) {
                    Some(p) => i18n::fmt(i18n::tr("Додати до: {}"), &[&(p.name)]),
                    None => i18n::tr("Новий проєкт").to_string(),
                };
                ui.horizontal(|ui| {
                    let (lrect, _) = ui.allocate_exact_size(egui::vec2(110.0, 36.0), egui::Sense::hover());
                    ui.painter().text(lrect.left_center(), egui::Align2::LEFT_CENTER, i18n::tr("Куди додати"), egui::FontId::proportional(13.0), TEXT);
                    combo_box("target").width(ui.available_width()).selected_text(label(self.target, &self.cfg.projects)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.target, None, i18n::tr("Новий проєкт"));
                        for (i, p) in self.cfg.projects.iter().enumerate() {
                            ui.selectable_value(&mut self.target, Some(i), i18n::fmt(i18n::tr("Додати до: {}"), &[&(p.name)]));
                        }
                    });
                });
                // --- вкладки платформ ---
                ui.add_space(12.0);
                egui::Frame::new().fill(Color32::from_rgb(0x17, 0x18, 0x1b)).stroke(Stroke::new(1.0, BORDER)).corner_radius(12).inner_margin(5).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.columns(2, |cols| {
                        if add_tab_btn(&mut cols[0], self.add_tab == 0, "Modrinth", MR_GREEN).clicked() { self.add_tab = 0; }
                        if add_tab_btn(&mut cols[1], self.add_tab == 1, "CurseForge", CF_ORANGE).clicked() { self.add_tab = 1; }
                    });
                });
                ui.add_space(12.0);
                if self.add_tab == 0 {
                    // --- Modrinth: лише список ваших проєктів (поле посилання прибрано, щоб списку було більше місця) ---
                    labeled_divider(ui, &i18n::fmt(i18n::tr("Ваші проєкти на {}"), &[&"Modrinth"]));
                    ui.add_space(6.0);
                    let has_token = !self.cfg.modrinth_token.trim().is_empty();
                    let mut clicked = false;
                    if has_token {
                        // рядок обмежує висоту: голий right_to_left забрав би всю висоту й зсунув список униз
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                clicked = ui.add_enabled_ui(!self.mr_loading, |ui| ghost_btn(ui, Some(Ic::Refresh), if self.mr_list.is_empty() { i18n::tr("Завантажити мої проєкти") } else { i18n::tr("Оновити список") })).inner.clicked();
                            });
                        });
                        ui.add_space(6.0);
                    }
                    // список підтягується сам при відкритті, якщо є токен
                    if clicked || (just_opened && !self.mr_loading && self.mr_list.is_empty() && has_token) {
                        self.mr_loading = true;
                        self.mr_msg.clear();
                        let t = self.cfg.modrinth_token.trim().to_string();
                        self.spawn(ctx, move || {
                            let r = modrinth_projects(&t);
                            if let Ok(l) = &r { l.iter().for_each(|p| cache_icon(&p.icon_url)); }
                            Msg::List(r)
                        });
                    }
                    if !has_token {
                        // жовтий банер із кнопкою, як на макеті
                        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(240, 163, 94, 26)).stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(240, 163, 94, 128))).corner_radius(14).inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                draw_icon_inline(ui, Ic::Warn, WARN);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Потрібен токен Modrinth.").strong().color(TEXT));
                                    ui.label(RichText::new("Щоб показати список ваших проєктів, вкажіть токен із правами Read user і Read projects.").color(MUTED).size(HINT_SZ));
                                });
                            });
                            ui.add_space(6.0);
                            if ghost_btn(ui, Some(Ic::Gear), "Відкрити налаштування").clicked() {
                                (self.tab, self.set_sec) = (Tab::Settings, 0);
                                self.add_open = false;
                            }
                        });
                    }
                    if !self.mr_msg.is_empty() {
                        banner(ui, Kind::Error, &self.mr_msg);
                    }
                    let mut pick = None;
                    egui::ScrollArea::vertical().id_salt("mr_list").max_height((ctx.content_rect().height() - 420.0).clamp(200.0, 560.0)).auto_shrink([false, true]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        if self.mr_list.is_empty() {
                            if self.mr_loading {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    hint(ui, i18n::tr("Завантаження…"));
                                });
                            } else if has_token && self.mr_msg.is_empty() {
                                hint(ui, "На цьому акаунті Modrinth проєктів не знайдено.");
                            }
                        }
                        for (i, p) in self.mr_list.iter().enumerate() {
                            let added = self.cfg.projects.iter().any(|q| q.modrinth_id == p.id);
                            add_row(ui, &p.icon_url, &p.title, &p.slug, |ui| {
                                if added {
                                    badge_ic(ui, i18n::tr("Уже додано"), SUCCESS, Some(Ic::Check));
                                } else if ghost_btn(ui, None, i18n::tr("btn.add")).clicked() {
                                    pick = Some(i);
                                }
                            });
                        }
                    });
                    if let Some(i) = pick {
                        let p = &self.mr_list[i];
                        let np = Project { name: p.title.clone(), modrinth_id: p.id.clone(), icon_url: p.icon_url.clone(), ..Default::default() };
                        self.add_project(np);
                        self.add_open = false;
                    }
                } else {
                    // --- CurseForge ---
                    let lid = field_label(ui, i18n::tr("ID або посилання *"));
                    let cf_link = parse_project_link(&self.cf_id);
                    let id_ok = !self.cf_id.trim().is_empty() && (self.cf_id.trim().chars().all(|c| c.is_ascii_digit()) || matches!(cf_link, Some(Link::CurseId(_) | Link::CurseSlug(_))));
                    let (mut go, mut idr) = (false, None);
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 110.0).max(160.0);
                        idr = Some(ui.add(egui::TextEdit::singleline(&mut self.cf_id).hint_text("123456 або curseforge.com/minecraft/mc-mods/назва").desired_width(w).min_size(egui::vec2(w, 44.0)).align(egui::Align2::LEFT_CENTER)).labelled_by(lid));
                        go = ui.add_enabled(id_ok, add_green(i18n::tr("btn.add"))).on_disabled_hover_text(i18n::tr("Введіть ID проєкту або посилання curseforge.com")).clicked();
                    });
                    let idr = idr.expect("поле створено в рядку вище");
                    if !self.cf_id.trim().is_empty() && !self.cf_id.trim().chars().all(|c| c.is_ascii_digit()) && cf_link.is_none() {
                        field_error(ui, &idr, i18n::tr("Лише цифри або посилання curseforge.com"));
                    }
                    hint(ui, if self.cfg.curse_core_key.trim().is_empty() { i18n::tr("Без Core API-ключа проєкт отримає назву «CurseForge #ID»; ключ додається в налаштуваннях.") } else { i18n::tr("Назву й іконку буде підтягнуто через Core API-ключ.") });
                    let key = self.cfg.curse_core_key.trim().to_string();
                    let enter = idr.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if go || (id_ok && enter) {
                        self.cf_id = self.cf_id.trim().to_string();
                        if let Some(Link::CurseId(id)) = parse_project_link(&self.cf_id) {
                            self.cf_id = id;
                        }
                        if let Some(Link::CurseSlug(slug)) = parse_project_link(&self.cf_id) {
                            if key.is_empty() {
                                self.add_msg = i18n::tr("За посиланням CurseForge шукає Core API, а ключа немає. Вкажіть його в налаштуваннях або введіть числовий ID зі сторінки проєкту.").into();
                            } else {
                                let target = self.target;
                                self.adding += 1; // Found зменшує лічильник
                                self.spawn(ctx, move || {
                                    let r = curse_lookup_slug(&key, &slug).map(|(id, name, icon)| {
                                        cache_icon(&icon);
                                        Box::new(Project { name, curse_id: id, icon_url: icon, ..Default::default() })
                                    });
                                    Msg::Found(target, r)
                                });
                                self.cf_id.clear();
                                self.add_open = false;
                            }
                        } else {
                            let (id, target) = (self.cf_id.clone(), self.target);
                            self.adding += 1; // CfInfo зменшує лічильник
                            if key.is_empty() {
                                self.spawn(ctx, move || Msg::CfInfo(id, target, Ok((String::new(), String::new()))));
                            } else {
                                self.spawn(ctx, move || {
                                    let r = curse_mod_info(&key, &id);
                                    if let Ok((_, u)) = &r { cache_icon(u); }
                                    Msg::CfInfo(id, target, r)
                                });
                            }
                            self.add_open = false;
                        }
                    }
                    // --- пошук за назвою (Core API-ключ) ---
                    ui.add_space(12.0);
                    labeled_divider(ui, i18n::tr("Або знайдіть за назвою"));
                    ui.add_space(8.0);
                    let search_key = self.cfg.curse_core_key.trim().to_string();
                    if search_key.is_empty() {
                        hint(ui, i18n::tr("Пошук за назвою потребує Core API-ключа: вкажіть його в налаштуваннях."));
                        if ghost_btn(ui, Some(Ic::Gear), "Відкрити налаштування").clicked() {
                            (self.tab, self.set_sec) = (Tab::Settings, 0);
                            self.add_open = false;
                        }
                    } else {
                        let mut go = false;
                        ui.horizontal(|ui| {
                            let w = (ui.available_width() - 110.0).max(160.0);
                            let r = ui.add(egui::TextEdit::singleline(&mut self.cf_query).hint_text(i18n::tr("Назва мода")).desired_width(w).min_size(egui::vec2(w, 44.0)).align(egui::Align2::LEFT_CENTER));
                            name_edit(&r, i18n::tr("Назва мода для пошуку на CurseForge"), i18n::tr("Назва мода"), &self.cf_query);
                            go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            go |= ui.add_enabled_ui(!self.cf_loading && !self.cf_query.trim().is_empty(), |ui| ghost_btn(ui, None, i18n::tr("Шукати"))).inner.clicked();
                        });
                        if go && !self.cf_loading && !self.cf_query.trim().is_empty() {
                            self.cf_loading = true;
                            self.add_msg.clear();
                            let q = self.cf_query.trim().to_string();
                            self.spawn(ctx, move || Msg::CfSearch(masked(curse_search(&search_key, &q), &[search_key.as_str()])));
                        }
                        if self.cf_loading {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                hint(ui, i18n::tr("Шукаємо…"));
                            });
                        }
                        let mut pick = None;
                        egui::ScrollArea::vertical().id_salt("cf_hits").max_height((ctx.content_rect().height() * 0.3).clamp(120.0, 240.0)).auto_shrink([false, true]).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 8.0;
                            for (i, h) in self.cf_hits.iter().enumerate() {
                                let added = self.cfg.projects.iter().any(|q| q.curse_id == h.id);
                                add_row(ui, &h.icon_url, &h.name, &format!("ID {}", h.id), |ui| {
                                    if added {
                                        badge_ic(ui, i18n::tr("Уже додано"), SUCCESS, Some(Ic::Check));
                                    } else if ghost_btn(ui, None, i18n::tr("btn.add")).clicked() {
                                        pick = Some(i);
                                    }
                                });
                            }
                        });
                        if let Some(i) = pick {
                            let h = self.cf_hits[i].clone();
                            cache_icon(&h.icon_url);
                            self.add_project(Project { name: h.name, curse_id: h.id, icon_url: h.icon_url, ..Default::default() });
                            self.add_open = false;
                        }
                    }
                }
                if !self.add_msg.is_empty() {
                    ui.add_space(8.0);
                    banner(ui, Kind::Warn, &self.add_msg);
                }
                });
                // підвал: «Закрити» праворуч під лінією
                ui.add_space(10.0);
                let (sep, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().hline(sep.x_range(), sep.center().y, Stroke::new(1.0, BORDER));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ghost_btn(ui, None, i18n::tr("btn.close")).clicked() { close = true; }
                    });
                });
            });
        if close || ((self.merge_ask.is_empty() || self.merge_hidden) && r.should_close()) {
            self.add_open = false;
        }
    }

    // ---------- Публікація ----------
    fn release(&self) -> Release {
        let mut r = Release {
            name: self.name.clone(),
            number: self.number.trim().to_string(),
            mc_versions: parse_mc(&self.cfg.last.mc_versions),
            loaders: self.cfg.last.loaders.clone(),
            release_type: RELEASE_TYPES[self.release_type].into(),
            changelog: self.changelog.clone(),
            featured: self.featured,
            dep_slugs: if self.send_deps { self.file_info.as_ref().map(|i| i.deps.clone()).unwrap_or_default() } else { vec![] },
            extra_files: self.extra_files.clone(),
            dep_ids: vec![],
        };
        // порожня «Назва» + шаблон проєкту = назва за шаблоном
        if r.name.trim().is_empty() && let Some(p) = self.sel.and_then(|i| self.cfg.projects.get(i)).filter(|p| !p.name_template.trim().is_empty()) {
            r.name = render_template(&p.name_template, &p.name, &r);
        }
        r
    }

    /// Жива перевірка готовності: що саме заважає натиснути «Опублікувати».
    fn check(&self, mr: bool, cf: bool) -> Result<(), String> {
        if self.upd_applying {
            return Err(i18n::tr("Триває оновлення програми: зачекайте, поки вона перезапуститься").into());
        }
        let rel = self.release();
        validate(self.sel.is_some(), mr || cf, &rel, self.file.as_deref())?;
        if cf && self.file.as_ref().and_then(|f| f.extension()).is_some_and(|e| e.eq_ignore_ascii_case("mrpack")) {
            return Err(i18n::tr("CurseForge не приймає .mrpack: вимкніть CurseForge").into());
        }
        if !self.mc_all.is_empty()
            && let Some(bad) = rel.mc_versions.iter().find(|v| !self.mc_all.contains(v))
        {
            return Err(i18n::fmt(i18n::tr("Невідома версія Minecraft: {}"), &[&(bad)]));
        }
        if mr && self.cfg.modrinth_token.trim().is_empty() {
            return Err("Немає токена Modrinth (вкладка «Налаштування»)".into());
        }
        if cf && self.cfg.curse_token.trim().is_empty() {
            return Err(i18n::tr("Немає токена CurseForge (вкладка «Налаштування»)").into());
        }
        Ok(())
    }

    /// Які кроки публікації заповнено.
    fn steps_done(&self) -> [bool; 6] {
        let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
        let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
        [
            self.sel.is_some(),
            self.file.is_some(),
            !self.number.trim().is_empty(),
            !parse_mc(&self.cfg.last.mc_versions).is_empty() && !self.cfg.last.loaders.is_empty(),
            !self.changelog.trim().is_empty(),
            (has_mr && self.use_mr) || (has_cf && self.use_cf),
        ]
    }

    /// Рядки-підсумки для згорнутих кроків.
    fn step_summaries(&self) -> [String; 6] {
        let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
        let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
        let mut plats = vec![];
        if has_mr && self.use_mr { plats.push("Modrinth"); }
        if has_cf && self.use_cf { plats.push("CurseForge"); }
        [
            proj.map_or_else(String::new, |p| p.name.clone()),
            self.file.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            format!("{} · {}", self.number.trim(), RELEASE_TYPES[self.release_type]),
            format!("Minecraft {} · {}", self.cfg.last.mc_versions.trim(), loaders_text(&self.cfg.last.loaders)),
            self.changelog.lines().find(|l| !l.trim().is_empty()).map(|l| l.chars().take(80).collect()).unwrap_or_default(),
            plats.join(" і "),
        ]
    }

    /// Версії Minecraft: поле і швидкий вибір.
    fn mc_ui(&mut self, ui: &mut egui::Ui, focus: bool) {
        let lid = field_label(ui, "Версії Minecraft *");
        let r = ui.add(egui::TextEdit::singleline(&mut self.cfg.last.mc_versions).hint_text(i18n::tr("напр. 1.20.1, 1.20.2")).desired_width(f32::INFINITY)).labelled_by(lid);
        if focus {
            r.request_focus();
        }
    }

    /// Повідомлення під полем версій: список не звірено, завантажується, невідомі версії.
    fn mc_chips(&mut self, ui: &mut egui::Ui) {
        let typed = parse_mc(&self.cfg.last.mc_versions);
        if self.mc_all.is_empty() && !typed.is_empty() {
            ui.label(RichText::new("Версії не звірено зі списком (немає зв'язку з Modrinth), перевірить сервер.").color(MUTED).size(HINT_SZ));
        }
        if self.mc_known.is_empty() && self.mc_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                hint(ui, i18n::tr("Завантажуємо список версій…"));
            });
        }
        if !self.mc_known.is_empty() {
            let bad: Vec<_> = typed.into_iter().filter(|t| !self.mc_all.contains(t)).collect();
            if !bad.is_empty() {
                ui.label(RichText::new(i18n::fmt(i18n::tr("Невідомі версії: {}"), &[&(bad.join(", "))])).color(WARN).size(HINT_SZ));
            }
        }
    }

    /// 0 — мод, 1 — ресурс-пак, 2 — датапак (за «лоадерами»-маркерами minecraft / datapack).
    fn pack_kind(&self) -> u8 {
        let l = &self.cfg.last.loaders;
        if l.iter().any(|x| x == "minecraft") { 1 } else if l.iter().any(|x| x == "datapack") { 2 } else { 0 }
    }

    /// Що публікуємо: мод, ресурс-пак чи датапак. Паки не мають лоадерів: лишається маркер для платформ.
    fn type_ui(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "Що публікуємо");
        let cur = self.pack_kind();
        let busy = self.pending > 0;
        let kinds = [("Мод", ".jar, потрібен лоадер"), ("Ресурс-пак", ".zip, текстури й звуки"), ("Датапак", ".zip, функції й рецепти")];
        ui.add_enabled_ui(!busy, |ui| {
            ui.columns(3, |cols| {
                for (k, (name, sub)) in kinds.iter().enumerate() {
                    if kind_card(&mut cols[k], cur as usize == k, name, sub).clicked() && cur as usize != k {
                        self.cfg.last.loaders.retain(|l| !is_pack_loader(l));
                        match k {
                            1 => self.cfg.last.loaders = vec!["minecraft".into()],
                            2 => self.cfg.last.loaders = vec!["datapack".into()],
                            _ => {}
                        }
                    }
                }
            });
        });
    }

    /// Вікно вибору версій Minecraft: пошук, фільтр, список груп із галочками.
    fn versions_dialog(&mut self, ctx: &egui::Context) {
        dialog_focus(ctx, "versions", self.ver_open);
        if !self.ver_open {
            return;
        }
        let (known, all) = (self.mc_known.clone(), self.mc_all.clone());
        let before = parse_mc(&self.cfg.last.mc_versions);
        let mut sel = before.clone();
        let mut close = false;
        let r = egui::Modal::new(egui::Id::unique("versions_dialog")).backdrop_color(Color32::from_black_alpha(160)).frame(modal_frame()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(300.0, 480.0));
            ui.horizontal(|ui| {
                ui.label(title("Версії Minecraft"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if close_x(ui).clicked() { close = true; }
                });
            });
            ui.add_space(8.0);
            let q = ui.add(egui::TextEdit::singleline(&mut self.ver_q).hint_text("Пошук, напр. 1.20").desired_width(f32::INFINITY).min_size(egui::vec2(0.0, CTRL_H)));
            name_edit(&q, "Пошук версії Minecraft", "Пошук, напр. 1.20", &self.ver_q);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, l) in ["Релізи", "Знімки", "Усі"].iter().enumerate() {
                    if choice(ui, self.ver_filter == i as u8, l).clicked() { self.ver_filter = i as u8; }
                }
            });
            ui.add_space(6.0);
            let needle = self.ver_q.trim().to_lowercase();
            let pool: Vec<&String> = match self.ver_filter {
                0 => known.iter().collect::<Vec<_>>(),
                1 => all.iter().filter(|v| !known.contains(v)).collect::<Vec<_>>(),
                _ => all.iter().collect::<Vec<_>>(),
            };
            let list: Vec<&String> = pool.into_iter().filter(|v| needle.is_empty() || v.to_lowercase().contains(&needle)).collect();
            egui::ScrollArea::vertical().id_salt("ver_list").max_height((ctx.content_rect().height() - 300.0).clamp(160.0, 420.0)).auto_shrink([false, true]).show(ui, |ui| {
                if list.is_empty() {
                    hint(ui, if all.is_empty() { "Список версій ще не завантажено (немає зв'язку з Modrinth). Можна вписати версії вручну в полі." } else { "Нічого не знайдено." });
                }
                let mut last = String::new();
                for v in list.iter().copied() {
                    let g = mc_group(v);
                    if g != last {
                        ui.add_space(6.0);
                        ui.label(RichText::new(&g).color(MUTED).size(12.0).strong());
                        last = g;
                    }
                    let on = sel.contains(v);
                    if ver_row(ui, v, on).clicked() {
                        if on { sel.retain(|x| x != v) } else { sel.push(v.to_string()) }
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                hint(ui, &format!("Обрано: {}", sel.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(primary("Готово")).clicked() { close = true; }
                    if ghost_btn(ui, None, "Очистити").clicked() { sel.clear(); }
                });
            });
        });
        if sel != before {
            self.cfg.last.mc_versions = sel.join(", ");
        }
        if close || r.should_close() {
            self.ver_open = false;
        }
    }

    /// Лоадери.
    fn loaders_ui(&mut self, ui: &mut egui::Ui) {
        field_label(ui, i18n::tr("Лоадери *"));
        ui.horizontal_wrapped(|ui| {
            for l in LOADERS.iter().copied().filter(|l| !is_pack_loader(l)) {
                let on = self.cfg.last.loaders.iter().any(|x| x == l);
                if choice(ui, on, loader_label(l)).clicked() {
                    if on { self.cfg.last.loaders.retain(|x| x != l) } else { self.cfg.last.loaders.push(l.into()) }
                }
            }
        });
        // ресурспак/датапак не поєднується з лоадерами модів: Modrinth відхилив би таку версію
        if mixed_pack_loaders(&self.cfg.last.loaders) {
            ui.label(RichText::new(i18n::tr("Ресурспак чи датапак не поєднується з лоадерами модів: лишіть один тип.")).color(WARN).size(HINT_SZ));
        }
        if let Some(i) = &self.file_info {
            for w in compat_warnings(i, &parse_mc(&self.cfg.last.mc_versions), &self.cfg.last.loaders) {
                ui.label(RichText::new(w).color(WARN).size(HINT_SZ));
            }
        }
    }

    /// Степер публікації: кожен крок — окрема вкладка; галочка — крок заповнено, пілюля — активний.
    fn publish_tabs(&mut self, ui: &mut egui::Ui, done: &[bool; 6]) {
        let names = [i18n::tr("Проєкт"), i18n::tr("Файл"), i18n::tr("Версія"), i18n::tr("Сумісність"), i18n::tr("Опис змін"), i18n::tr("Куди публікувати")];
        egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).corner_radius(CARD_R).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_enabled_ui(self.pending == 0, |ui| ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, n) in names.iter().enumerate() {
                    if step_tab(ui, i + 1, n, done[i], self.pub_step == i).clicked() { (self.pub_step, self.edit_mode) = (i, true); }
                    if i + 1 < names.len() {
                        let (rc, _) = ui.allocate_exact_size(egui::vec2(18.0, CTRL_H), egui::Sense::hover());
                        let col = if done[i] { ACCENT.gamma_multiply(0.5) } else { BORDER };
                        ui.painter().hline(rc.x_range().shrink(3.0), rc.center().y, Stroke::new(2.0, col));
                    }
                }
            }));
        });
    }

    /// Зведення замість майстра, коли все заповнено: що піде на сайти, і «Змінити» біля кожного рядка (клавіатурою теж).
    fn review_card(&mut self, ui: &mut egui::Ui, sum: &[String; 6]) {
        // «Опис змін», а не «Зміни»: поруч стоїть кнопка «Змінити», і два схожі слова плутали
        let names = [i18n::tr("Проєкт"), i18n::tr("Файл"), i18n::tr("Версія"), i18n::tr("Сумісність"), i18n::tr("Опис змін"), i18n::tr("Куди публікувати")];
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(i18n::tr("Перевірте перед публікацією")).size(20.0).color(TEXT));
            ui.add_space(8.0);
            // що підставлено з файлу й від чого залежить мод: у майстрі це видно в кроці «Файл», а тут його пропущено
            if !self.file_note.is_empty() {
                banner(ui, Kind::Info, &self.file_note);
                ui.add_space(8.0);
            }
            // під час публікації правити нічого: як і стрічка кроків у майстрі
            ui.add_enabled_ui(self.pending == 0, |ui| {
            for (i, n) in names.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(120.0, TOUCH_H), egui::Layout::left_to_right(egui::Align::Center), |ui| ui.label(RichText::new(*n).color(MUTED).size(HINT_SZ)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // «Змінити» як посилання (28 px, а не рамкова кнопка 36 px): шість рядків вміщаються і у вікні 480 px
                        let b = ui.add(egui::Button::new(RichText::new(i18n::tr("Змінити")).color(LINK).underline()).frame(false).min_size(egui::vec2(0.0, TOUCH_H)));
                        if aria(b.on_hover_cursor(egui::CursorIcon::PointingHand), i18n::fmt(i18n::tr("Змінити крок «{}»"), &[n])).clicked() {
                            (self.pub_step, self.edit_mode) = (i, true);
                        }
                        let v = if sum[i].trim().is_empty() { "—" } else { sum[i].as_str() };
                        ui.add(egui::Label::new(RichText::new(v).color(if sum[i].trim().is_empty() { MUTED } else { TEXT })).truncate());
                    });
                });
                if i + 1 < names.len() {
                    if ui.ctx().content_rect().height() < 600.0 {
                        // низьке вікно: тонкий розділювач без зайвих відступів
                        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                        ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, BORDER));
                    } else {
                        row_sep(ui);
                    }
                }
            }
            });
        });
    }

    /// Права картка: що буде опубліковано, стан і кнопка «Опублікувати».
    fn publish_summary(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
        let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
        let name = proj.map_or(i18n::tr("Проєкт не обрано").to_string(), |p| p.name.clone());
        let dash = |t: String| if t.trim().is_empty() { "—".to_string() } else { t };
        let rows = [
            (i18n::tr("Файл"), dash(self.file.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
            (i18n::tr("Версія"), dash(if self.number.trim().is_empty() { String::new() } else { format!("{} · {}", self.number.trim(), RELEASE_TYPES[self.release_type]) })),
            ("Minecraft", dash(self.cfg.last.mc_versions.trim().to_string())),
            (i18n::tr("Лоадери"), dash(loaders_text(&self.cfg.last.loaders))),
        ];
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(i18n::tr("БУДЕ ОПУБЛІКОВАНО")).color(MUTED).size(12.0).strong());
            ui.add_space(6.0);
            ui.label(title(name));
            ui.add_space(4.0);
            // у зведенні ліворуч ці самі рядки вже показано: на широкому вікні права картка їх не дублює
            if !self.review_now { ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = 16.0;
                ui.spacing_mut().item_spacing.y = 6.0;
                for (k, v) in rows {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(k).color(MUTED).size(HINT_SZ));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(v).size(HINT_SZ)).truncate());
                        });
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(i18n::tr("Опис змін")).color(MUTED).size(HINT_SZ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.changelog.trim().is_empty() {
                            // опис необов'язковий: приглушено, без тривожного кольору
                            ui.label(RichText::new(i18n::tr("ще не заповнено")).color(MUTED).size(HINT_SZ));
                        } else {
                            let first: String = self.changelog.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(40).collect();
                            ui.add(egui::Label::new(RichText::new(first).size(HINT_SZ)).truncate());
                        }
                    });
                });
            }); }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if has_mr && self.use_mr { brand_pill(ui, "Modrinth", MR_GREEN, MR_BG); }
                if has_cf && self.use_cf { brand_pill(ui, "CurseForge", CF_ORANGE, CF_BG); }
            });
            row_sep(ui);
            self.publish_bar_in(ui, ctx, true);
        });
    }

    /// Відкрита сторінка історії (пункт меню «Історія»): без майстра й панелі публікації.
    fn history_page(&self) -> bool {
        // focus_hist («До історії» після публікації) відкриває сторінку з першого ж кадру, без спалаху майстра
        // під час відправки сторінки історії немає: на ній не видно ні прогресу, ні «Скасувати» (на широкому вікні нижньої панелі нема взагалі)
        self.tab == Tab::Publish && (self.pub_step == 6 || self.focus_hist) && !self.history.is_empty() && self.pending == 0
    }

    /// Окрема сторінка історії: свій заголовок і шлях назад, без стрічки кроків і без кнопки «Опублікувати».
    fn history_ui(&mut self, ui: &mut egui::Ui) {
        self.pub_step = 6;
        ui.horizontal(|ui| {
            ui.heading(i18n::fmt(i18n::tr("Історія · {}"), &[&(self.history.len())]));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ghost_btn(ui, Some(Ic::Upload), i18n::tr("До публікації")).clicked() {
                    self.pub_step = 5;
                }
            });
        });
        hint(ui, i18n::tr("Усі публікації з цього комп'ютера. «Повторити з цього запису» підставляє його у форму."));
        // помилка дії на цій сторінці (напр. не вдалося очистити історію): нижньої панелі тут немає, тож показуємо її самі
        if !self.msg.is_empty() {
            banner(ui, Kind::Warn, &self.msg.clone());
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.publish_extras(ui));
    }

    fn publish_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.history_page() {
            return self.history_ui(ui);
        }
        ui.heading(i18n::tr("nav.publish"));
        if !self.cfg.projects.is_empty() && self.cfg.modrinth_token.trim().is_empty() && self.cfg.curse_token.trim().is_empty() {
            banner(ui, Kind::Warn, i18n::tr("Токени ще не вказано: заповнити форму можна, але надіслати файл — ні."));
            if ghost_btn(ui, Some(Ic::Gear), i18n::tr("Вказати токен")).clicked() {
                (self.tab, self.set_sec) = (Tab::Settings, 0);
            }
            ui.add_space(8.0);
        }
        let d = self.steps_done();
        let bar_has_results = self.mr_res.is_some() || self.cf_res.is_some() || !self.msg.is_empty();
        // у низькому вікні після публікації підказка відбирає місце в форми
        if !(bar_has_results && ui.ctx().content_rect().height() < 600.0) {
            let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
            let (hm, hc) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
            let warned = !self.pre_warnings(hm && self.use_mr, hc && self.use_cf).is_empty();
            // «Усе заповнено» не стоїть поруч із попередженням: воно б йому суперечило
            hint(ui, i18n::tr(if ![0, 1, 2, 3, 5].iter().all(|&i| d[i]) { "Заповніть кроки по черзі. Ctrl+Enter — опублікувати." } else if warned { "Заповнено, але є попередження. Ctrl+Enter — опублікувати." } else { "Усе заповнено. Ctrl+Enter — опублікувати." }));
        }
        ui.add_space(if bar_has_results { 4.0 } else { 12.0 });
        if self.cfg.projects.is_empty() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    empty_state(ui, Ic::Grid, i18n::tr("Спершу додайте проєкт"), i18n::tr("Публікація можлива, коли є хоч один проєкт."));
                    if ui.add(primary(i18n::tr("Перейти до проєктів"))).clicked() {
                        self.tab = Tab::Projects;
                    }
                    ui.add_space(8.0);
                });
            });
            return;
        }
        let done = self.steps_done();
        let sum = self.step_summaries();
        let focus = self.focus_step.take();
        // крок «Проєкт» / «Файл» згортається, щойно його виконано (і при вході, якщо проєкт уже обрано): форма не тоне у вже зробленому
        let prev = ctx.data_mut(|d| d.get_temp::<[bool; 2]>(egui::Id::NULL.with("prev_done")));
        for i in 0..2 {
            if done[i] && (prev.is_some_and(|p| !p[i]) || prev.is_none()) {
                self.open_steps[i] = false;
            }
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::NULL.with("prev_done"), [done[0], done[1]]));
        let sp =|i: usize, name: &'static str, required: bool| StepSpec { n: i + 1, name, done: done[i], required, summary: &sum[i], scroll: focus == Some(i), tabbed: true };
        if let Some(f) = focus { (self.pub_step, self.edit_mode) = (f, true); }
        if self.focus_hist {
            if self.history.is_empty() { self.focus_hist = false; } else { self.pub_step = 6; }
        }
        if self.history.is_empty() && self.pub_step > 5 { self.pub_step = 5; }
        self.pub_step = self.pub_step.min(6);
        // усе обов'язкове заповнено: замість покрокового майстра — зведення з «Змінити» біля кожного рядка
        let all_done = [0, 1, 2, 3, 5].iter().all(|&i| done[i]);
        if !all_done { self.edit_mode = true; } // неповна форма завжди в майстрі: зведення не перехоплює її посеред набору
        let review = all_done && !self.edit_mode && self.pub_step != 6;
        self.review_now = review;
        // у зведенні стрічка кроків лише забирає висоту (усі галочки, а «Змінити» біля рядка робить те саме)
        if !review {
            self.publish_tabs(ui, &done);
            ui.add_space(8.0);
        }
        let mut open = self.open_steps;
        let avail = ui.available_rect_before_wrap();
        let side = pub_side(ctx); // у вузькому вікні підсумок і кнопка лишаються в нижній панелі
        let main_rect = if side { egui::Rect::from_min_max(avail.min, egui::pos2(avail.right() - 340.0, avail.bottom())) } else { avail };
        ui.scope_builder(egui::UiBuilder::new().max_rect(main_rect), |ui| egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if review {
                self.review_card(ui, &sum);
                return;
            }
            if self.pub_step == 0 { step(ui, sp(0, i18n::tr("Проєкт"), true), &mut open[0], |ui| {
                let busy = self.pending > 0; // під час публікації проєкт не міняємо
                let mut pick: Option<usize> = None;
                ui.add_enabled_ui(!busy, |ui| {
                    for (i, p) in self.cfg.projects.iter().enumerate() {
                        if project_row(ui, &p.name, &p.last_number, !p.modrinth_id.is_empty(), !p.curse_id.is_empty(), self.sel == Some(i)).clicked() {
                            pick = Some(i);
                        }
                        ui.add_space(6.0);
                    }
                });
                if let Some(i) = pick.filter(|i| self.sel != Some(*i)) {
                    self.select_project(Some(i));
                }
                self.step_nav(ui);
            }); }

            if self.pub_step == 1 { step(ui, sp(1, i18n::tr("Файл"), true), &mut open[1], |ui| {
                let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
                let t = ctx.animate_bool_with_time(ui.next_auto_id(), hovering, 0.12);
                let (stroke, fill) = (Stroke::new(1.0 + t, UI_BORDER.lerp_to_gamma(ACCENT, t)), FIELD.lerp_to_gamma(ACCENT, 0.08 * t));
                egui::Frame::new().fill(fill).stroke(stroke).corner_radius(egui::CornerRadius::same(CARD_R)).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(84.0);
                    ui.vertical_centered(|ui| {
                        if hovering {
                            ui.add_space(24.0);
                            ui.label(title(i18n::tr("Відпустіть файл тут")));
                            ui.add_space(24.0);
                            return;
                        }
                        match &self.file {
                            Some(f) => {
                                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                ui.label(title(name));
                                if ghost_btn(ui, Some(Ic::Upload), i18n::tr("Обрати інший файл…")).clicked() {
                                    // діалог у окремому потоці, щоб не блокувати UI
                                    { let d = self.cfg.last.dir.clone(); self.spawn_opt(ctx, move || pick_file(d)); }
                                }
                            }
                            None => {
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                                draw_icon(ui.painter(), rect.center(), Ic::Upload, 48.0, ACCENT.gamma_multiply(0.85));
                                ui.label(title(i18n::tr("Перетягніть файл сюди")));
                                hint(ui, i18n::tr(".jar, .zip або .mrpack"));
                                ui.add_space(8.0);
                                if ghost_btn(ui, Some(Ic::Upload), i18n::tr("Обрати файл…")).clicked() {
                                    { let d = self.cfg.last.dir.clone(); self.spawn_opt(ctx, move || pick_file(d)); }
                                }
                            }
                        }
                    });
                });
                if !self.file_note.is_empty() && self.file.is_some() {
                    ui.add_space(8.0);
                    banner(ui, Kind::Info, &self.file_note);
                }
                if self.cf_auto_off && !self.use_cf {
                    ui.add_space(8.0);
                    banner(ui, Kind::Info, i18n::tr("CurseForge вимкнено автоматично: він не приймає .mrpack. Оберіть .jar або .zip, щоб повернути його."));
                }
                self.step_nav(ui);
            }); }

            if self.pub_step == 2 { step(ui, sp(2, i18n::tr("Версія"), true), &mut open[2], |ui| {
                ui.columns(2, |c| {
                    let lid = field_label(&mut c[0], i18n::tr("Номер версії *"));
                    let nr = c[0].add(egui::TextEdit::singleline(&mut self.number).hint_text(i18n::tr("напр. 1.0.0")).desired_width(f32::INFINITY)).labelled_by(lid);
                    if focus == Some(2) {
                        nr.request_focus();
                    }
                    if self.file.is_some() && self.number.trim().is_empty() {
                        field_error(&mut c[0], &nr, i18n::tr("Вкажіть номер версії"));
                    } else if let Some(p) = self.sel.and_then(|i| self.cfg.projects.get(i))
                        && let Some(w) = version_warning(&self.number, &p.name, &p.last_number, &self.history)
                    {
                        c[0].label(RichText::new(w).color(WARN).size(HINT_SZ));
                    }
                    if let Some(t) = detect_release_type(&self.number).filter(|t| *t != RELEASE_TYPES[self.release_type])
                        && let Some(k) = RELEASE_TYPES.iter().position(|r| *r == t)
                    {
                        c[0].horizontal_wrapped(|ui| {
                            hint(ui, &i18n::fmt(i18n::tr("Номер схожий на {}, а тип релізу інший."), &[&(t)]));
                            if ghost_btn(ui, None, &i18n::fmt(i18n::tr("Поставити {}"), &[&(t)])).clicked() {
                                self.release_type = k;
                            }
                        });
                    }
                    let lid = field_label(&mut c[1], i18n::tr("Назва"));
                    c[1].add(egui::TextEdit::singleline(&mut self.name).hint_text(i18n::tr("необов'язково, за замовч. — номер")).desired_width(f32::INFINITY)).labelled_by(lid);
                });
                ui.add_space(8.0);
                hint(ui, i18n::tr("* обов'язкове поле"));
                if let Some(i) = self.sel.filter(|i| *i < self.cfg.projects.len()) {
                    ui.add_space(4.0);
                    let lid = field_label(ui, i18n::tr("Шаблон назви версії (для цього проєкту)"));
                    let tr = ui.add(egui::TextEdit::singleline(&mut self.cfg.projects[i].name_template).hint_text("напр. {project} {version} для {mc}").desired_width(f32::INFINITY)).labelled_by(lid);
                    if tr.lost_focus() {
                        self.save();
                    }
                    if self.name.trim().is_empty() && !self.cfg.projects[i].name_template.trim().is_empty() {
                        hint(ui, &i18n::fmt(i18n::tr("Назва буде: {}"), &[&(self.release().name)]));
                    } else {
                        hint(ui, "Підставляється, коли «Назва» порожня. Підстановки: {project} {version} {mc} {loaders} {date}");
                    }
                }
                field_label(ui, "Тип релізу");
                ui.horizontal_wrapped(|ui| {
                    for (i, r) in RELEASE_TYPES.iter().enumerate() {
                        if choice(ui, self.release_type == i, r).clicked() {
                            self.release_type = i;
                        }
                    }
                });
                self.step_nav(ui);
            }); }

            if self.pub_step == 3 { step(ui, sp(3, i18n::tr("Сумісність"), true), &mut open[3], |ui| {
                self.type_ui(ui);
                ui.add_space(8.0);
                self.mc_ui(ui, focus == Some(3));
                ui.horizontal(|ui| {
                    if ghost_btn(ui, Some(Ic::File), "Обрати версії…").clicked() { self.ver_open = true; }
                    hint(ui, &format!("Обрано: {}", parse_mc(&self.cfg.last.mc_versions).len()));
                });
                self.mc_chips(ui);
                ui.add_space(8.0);
                if self.pack_kind() == 0 {
                    self.loaders_ui(ui);
                } else {
                    banner(ui, Kind::Info, "Лоадери не потрібні: ресурс-паки й датапаки працюють без лоадера. Платформи позначать файл автоматично.");
                }
                self.step_nav(ui);
            }); }

            if self.pub_step == 4 { step(ui, sp(4, i18n::tr("Опис змін"), false), &mut open[4], |ui| {
                let lid = field_label(ui, i18n::tr("Що змінилось"));
                ui.add(egui::TextEdit::multiline(&mut self.changelog).hint_text(i18n::tr("Можна списки (- пункт) і **жирний** текст")).desired_rows(4).desired_width(f32::INFINITY)).labelled_by(lid);
                if self.changelog.trim().is_empty() && self.file.is_some() {
                    ui.label(RichText::new(i18n::tr("Опис змін порожній: на сторінці версії не буде тексту. Можна публікувати й так.")).color(WARN).size(HINT_SZ));
                }
                hint(ui, i18n::tr("У шаблонах: {version} {mc} {loaders} {date} {project}"));
                ui.horizontal_wrapped(|ui| {
                if let Some(c) = self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| p.last_changelog.clone()).filter(|c| !c.is_empty())
                    && ghost_btn(ui, Some(Ic::File), i18n::tr("З минулої версії")).on_hover_text(i18n::tr("Взяти текст змін минулої версії")).clicked()
                {
                    self.changelog = c;
                }
                let tpl = self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| (p.name.clone(), p.changelog_template.clone())).filter(|t| !t.1.is_empty());
                if let Some((name, t)) = tpl
                    && ghost_btn(ui, Some(Ic::File), i18n::tr("Вставити шаблон")).on_hover_text(i18n::tr("Підстановки: {version} {mc} {loaders} {date} {project}")).clicked()
                {
                    self.changelog = render_template(&t, &name, &self.release());
                }
                // рідші дії в меню: на видноті лишаються дві найчастіші кнопки
                ui.menu_button(i18n::tr("Ще"), |ui| {
                    if !self.changelog.trim().is_empty()
                        && let Some(i) = self.sel.filter(|i| *i < self.cfg.projects.len())
                        && ghost_btn(ui, Some(Ic::File), i18n::tr("Зберегти шаблон")).on_hover_text(i18n::tr("Поточний текст стане шаблоном; використайте {version}, {mc}, {loaders}, {date}, {project}")).clicked()
                    {
                        self.cfg.projects[i].changelog_template = self.changelog.clone();
                        self.notice = i18n::tr("Шаблон змін збережено для цього проєкту.").into();
                        self.save();
                        ui.close();
                    }
                    if ghost_btn(ui, Some(Ic::File), i18n::tr("З файлу…")).on_hover_text(i18n::tr("Береться перша (остання за часом) секція файлу")).clicked() {
                        let d = self.cfg.last.dir.clone();
                        self.spawn_opt(ctx, move || pick_changelog(d));
                        ui.close();
                    }
                });
                });
                self.step_nav(ui);
            }); }

            if self.pub_step == 5 { step(ui, sp(5, i18n::tr("Куди публікувати"), true), &mut open[5], |ui| {
                let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
                let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
                if proj.is_none() {
                    hint(ui, i18n::tr("Спершу оберіть проєкт у кроці 1."));
                } else if !has_mr && !has_cf {
                    hint(ui, i18n::tr("Проєкт не підключений до жодної платформи. Додайте його у розділі «Проєкти»."));
                }
                if self.cf_auto_off && !self.use_cf && has_cf {
                    banner(ui, Kind::Info, i18n::tr("CurseForge вимкнено автоматично, бо вибрано файл .mrpack."));
                }
                ui.columns(2, |cols| {
                    for (k, (has, on, name, col)) in [(has_mr, &mut self.use_mr, "Modrinth", MR_GREEN), (has_cf, &mut self.use_cf, "CurseForge", CF_ORANGE)].into_iter().enumerate() {
                        let tip = i18n::tr("Проєкт не підключений до цієї платформи");
                        let r = cols[k].add_enabled_ui(has, |ui| platform_card(ui, *on && has, name, col)).inner;
                        if has && r.clicked() {
                            *on = !*on;
                        }
                        if !has {
                            r.on_disabled_hover_text(tip);
                        }
                    }
                });
                if proj.is_some() {
                    for (has, name) in [(has_mr, "Modrinth"), (has_cf, "CurseForge")] {
                        if !has {
                            hint(ui, &i18n::fmt(i18n::tr("{}: проєкт не підключений до цієї платформи."), &[&(name)]));
                        }
                    }
                }
                if has_cf && !self.cfg.curse_core_key.trim().is_empty() {
                    let id = self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| p.curse_id.clone()).unwrap_or_default();
                    if ghost_btn(ui, None, i18n::tr("Перевірити проєкт CurseForge")).on_hover_text(i18n::tr("Core API-ключ: чи існує проєкт з цим ID і чи його видно")).clicked() {
                        let k = self.cfg.curse_core_key.trim().to_string();
                        self.proj_chk = Chk::Checking;
                        self.spawn(ctx, move || Msg::CheckProj(masked(curse_project_check(&k, &id), &[k.as_str()])));
                    }
                    status(ui, &self.proj_chk);
                }
                if has_mr && self.use_mr {
                    // додаткові можливості лише для Modrinth; усе вимкнено за замовчуванням
                    ui.add_space(8.0);
                    // згорнуто, поки нічого з цього не ввімкнено: рідкісні дії не конкурують із вибором платформ
                    egui::CollapsingHeader::new(RichText::new(i18n::tr("Modrinth: додатково")).color(MUTED)).open((self.featured || self.send_deps || !self.extra_files.is_empty()).then_some(true)).show(ui, |ui| { // відкрито примусово, поки щось увімкнено: прихованих налаштувань публікації не буває
                    check_row(ui, &mut self.featured, i18n::tr("Позначити як рекомендовану"));
                    let deps = self.file_info.as_ref().map(|i| i.deps.clone()).unwrap_or_default();
                    if !deps.is_empty() {
                        check_row(ui, &mut self.send_deps, &i18n::fmt(i18n::tr("Надіслати залежності: {}"), &[&(deps.join(", "))])).on_hover_text(i18n::tr("Зіставляються з проєктами Modrinth за slug; ненайдені пропускаються. Позначаються як обов'язкові."));
                    }
                    let mut rm = None;
                    for (k, p) in self.extra_files.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).color(MUTED));
                            if ghost_btn(ui, Some(Ic::Cross), i18n::tr("Прибрати")).clicked() {
                                rm = Some(k);
                            }
                        });
                    }
                    if let Some(k) = rm {
                        self.extra_files.remove(k);
                    }
                    if self.extra_files.len() < MAX_EXTRA && ghost_btn(ui, Some(Ic::Plus), i18n::tr("Додатковий файл…")).on_hover_text(i18n::tr("Наприклад, sources або dev-jar (до 3). Лише Modrinth")).clicked() {
                        let d = self.cfg.last.dir.clone();
                        self.spawn_opt(ctx, move || pick_extra(d));
                    }
                    if self.use_cf && !self.extra_files.is_empty() {
                        hint(ui, i18n::tr("CurseForge отримає лише основний файл."));
                    }
                    });
                }
                self.step_nav(ui);
            }); }

            if self.pub_step == 6 {
                self.publish_extras(ui);
            }
        }));
        self.open_steps = open;
        if side {
            // після форми: Tab доходить до кнопки «Опублікувати» вже після полів
            let rc = egui::Rect::from_min_max(egui::pos2(avail.right() - 320.0, avail.top()), avail.max);
            ui.scope_builder(egui::UiBuilder::new().max_rect(rc), |ui| {
                egui::ScrollArea::vertical().id_salt("pub_side").auto_shrink([false, false]).show(ui, |ui| self.publish_summary(ui, ctx));
            });
        }
    }

    /// Підвал кроку: «Назад» ліворуч, «Далі: <наступний крок>» праворуч.
    fn step_nav(&mut self, ui: &mut egui::Ui) {
        let names = [i18n::tr("Проєкт"), i18n::tr("Файл"), i18n::tr("Версія"), i18n::tr("Сумісність"), i18n::tr("Опис змін"), i18n::tr("Куди публікувати")];
        let d = self.steps_done();
        row_sep(ui);
        ui.horizontal(|ui| {
            if self.pub_step > 0 && ghost_btn(ui, None, i18n::tr("Назад")).clicked() { self.pub_step -= 1; }
            if [0, 1, 2, 3, 5].iter().all(|&i| d[i]) && ghost_btn(ui, None, i18n::tr("До перевірки")).on_hover_text(i18n::tr("Повернутись до зведення перед публікацією")).clicked() {
                self.edit_mode = false;
            }
            if self.pub_step < 5 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can = d[self.pub_step] || self.pub_step == 4; // «Зміни» необов'язкові
                    let label = format!("{}: {}", i18n::tr("Далі"), names[self.pub_step + 1]);
                    if ui.add_enabled(can, primary(&label)).clicked() { self.pub_step += 1; }
                });
            }
        });
    }

    /// Нижня панель: смуга результатів зверху, нижче підсумок ліворуч і кнопка праворуч.
    fn publish_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.publish_bar_in(ui, ctx, false);
    }

    /// card = true: усередині правої картки (кнопка на всю ширину під статусом).
    fn publish_bar_in(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, card: bool) {
        let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
        let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
        let (mr, cf) = (has_mr && self.use_mr, has_cf && self.use_cf);
        let ready = self.check(mr, cf);
        let busy = self.pending > 0;
        let bars: Vec<(&str, f32, Color32, String)> = if busy {
            [("Modrinth", &self.up_mr, &self.mr_res, MR_GREEN), ("CurseForge", &self.up_cf, &self.cf_res, CF_ORANGE)]
                .into_iter()
                .filter_map(|(n, up, res, col)| {
                    let total = up.prog.total.load(Relaxed);
                    (total > 0 && res.is_none()).then(|| {
                        let extra = up.prog.speed_eta().map_or(String::new(), |(bps, eta)| speed_text(bps, eta));
                        (n, up.prog.sent.load(Relaxed) as f32 / total as f32, col, extra)
                    })
                })
                .collect()
        } else {
            vec![]
        };
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if !bars.is_empty() || !self.msg.is_empty() || self.mr_res.is_some() || self.cf_res.is_some() {
            let (any_ok, any_err) = (matches!(self.mr_res, Some(Ok(_))) || matches!(self.cf_res, Some(Ok(_))), matches!(self.mr_res, Some(Err(_))) || matches!(self.cf_res, Some(Err(_))));
            let mut new_version = false;
            if !busy && any_ok {
                // ряд кнопок над прокруткою: не обрізається в низькій зоні результатів
                ui.horizontal(|ui| {
                    new_version = ghost_btn(ui, Some(Ic::Plus), i18n::tr("Нова версія")).on_hover_text(i18n::tr("Очищає форму для наступної публікації")).clicked();
                    if ghost_btn(ui, Some(Ic::File), i18n::tr("До історії")).on_hover_text(i18n::tr("Прокрутити до списку публікацій")).clicked() {
                        self.focus_hist = true;
                    }
                });
            }
            // низьке вікно: результати згорнуті в один рядок (панель ≤30% висоти), повний список — за «Докладніше»
            let low = ctx.content_rect().height() < 600.0;
            let res_id = egui::Id::NULL.with("res_open");
            let res_open = ctx.data_mut(|d| d.get_temp::<bool>(res_id)).unwrap_or(false);
            let compact = low && !busy && bars.is_empty() && self.msg.is_empty();
            if compact {
                ui.horizontal_wrapped(|ui| { // кнопка «Докладніше» переходить на новий рядок, а не зникає за краєм
                    if !res_open {
                        for (name, r) in [("Modrinth", &self.mr_res), ("CurseForge", &self.cf_res)] {
                            match r {
                                Some(Ok(_)) => icon_label_c(ui, Ic::Check, SUCCESS, name, TEXT),
                                Some(Err(e)) => {
                                    // причина в рядку, а не лише «помилка»: повний текст за «Докладніше»
                                    let (head, token, _) = error_headline(name, e);
                                    // у вузькому рядку повний заголовок обрізався б посередині поради: для токена коротша форма
                                    let head = if token {
                                        // 403 може бути і про ID проєкту, 401 лише про токен
                                        let k = if e.contains("(HTTP 403") { i18n::tr("{}: немає доступу. Перевірте токен і ID проєкту.") } else { i18n::tr("{}: токен не підійшов. Перевірте його.") };
                                        i18n::fmt(k, &[&name])
                                    } else if head.chars().count() > 60 {
                                        let s: String = e.split(". ").next().unwrap_or(e).lines().next().unwrap_or("").chars().take(40).collect();
                                        format!("{name}: {s}{}", if s.chars().count() < e.chars().count() { "…" } else { "" })
                                    } else {
                                        head
                                    };
                                    icon_label_c(ui, Ic::Cross, ERROR, &head, TEXT)
                                }
                                None => {}
                            };
                        }
                    }
                    if aria(ghost_btn(ui, None, if res_open { i18n::tr("btn.hide") } else { i18n::tr("btn.details") }), if res_open { i18n::tr("Сховати докладні результати") } else { i18n::tr("Показати докладні результати") }).clicked() {
                        ctx.data_mut(|d| d.insert_temp(res_id, !res_open));
                    }
                });
            }
            if !compact || res_open {
            egui::ScrollArea::vertical().id_salt("pub_res").max_height((ctx.content_rect().height() * if low && busy { 0.26 } else if low && !res_open { 0.1 } else { 0.2 }).clamp(44.0, 220.0)).auto_shrink([false, true]).show(ui, |ui| {
                for (name, f, col, extra) in &bars {
                    // підпис над смугою: довжина тексту не зсуває початок смуги, обидві платформи вирівняні
                    // рядок: назва · смуга · відсоток і швидкість (фіксована колонка назви: смуги вирівняні)
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(egui::vec2(90.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| ui.label(RichText::new(*name).color(TEXT)));
                        let tail = if ui.available_width() > 420.0 { 170.0 } else { 56.0 };
                        progress_bar(ui, *f, (ui.available_width() - tail - 8.0).max(60.0), *col);
                        let txt = if tail > 100.0 { format!("{}%{extra}", (f * 100.0) as u32) } else { format!("{}%", (f * 100.0) as u32) };
                        ui.label(RichText::new(txt).color(MUTED).size(HINT_SZ));
                    });
                }
                if !self.msg.is_empty() {
                    banner(ui, Kind::Warn, &self.msg);
                }
                for (name, r) in [("Modrinth", &self.mr_res), ("CurseForge", &self.cf_res)] {
                    match r {
                        Some(Ok(s)) => {
                            let (_, url) = split_url(s);
                            ui.horizontal_wrapped(|ui| {
                                icon_label_c(ui, Ic::Check, SUCCESS, &i18n::fmt(i18n::tr("{}: опубліковано"), &[&(name)]), TEXT);
                                if let Some(u) = url {
                                    if icon_btn(ui, Some(Ic::External), i18n::tr("btn.open"), false).clicked() {
                                        ui.ctx().open_url(egui::OpenUrl::new_tab(u.clone()));
                                    }
                                    if ghost_btn(ui, None, i18n::tr("btn.copy_link")).clicked() {
                                        ui.ctx().copy_text(u);
                                        self.notice = i18n::tr("notice.link_copied").into();
                                    }
                                }
                            });
                        }
                        Some(Err(e)) => {
                            // простою мовою в рядку, сирий текст сервера за «Докладніше»
                            let (head, token, long) = error_headline(name, e);
                            let det_id = egui::Id::NULL.with(("res_detail", name));
                            let open = ctx.data_mut(|d| d.get_temp::<bool>(det_id)).unwrap_or(false);
                            ui.horizontal_wrapped(|ui| {
                                icon_label_c(ui, Ic::Cross, ERROR, &head, TEXT);
                                if token && ghost_btn(ui, Some(Ic::Gear), i18n::tr("До налаштувань")).clicked() {
                                    (self.tab, self.set_sec) = (Tab::Settings, 0);
                                }
                                if long && aria(ghost_btn(ui, None, if open { i18n::tr("btn.hide") } else { i18n::tr("btn.details") }), i18n::fmt(i18n::tr("{} текст помилки {}"), &[&(if open { i18n::tr("Сховати") } else { i18n::tr("Показати") }), &(name)])).clicked() {
                                    ctx.data_mut(|d| d.insert_temp(det_id, !open));
                                }
                            });
                            if open {
                                hint(ui, e);
                            }
                            if e.contains("перевірте на сайті") || e.contains("check the site") {
                                // запит міг дійти до сервера: даємо посилання й попереджаємо про дубль
                                hint(ui, i18n::tr("Файл міг уже потрапити на сайт. Перевірте список версій, перш ніж публікувати повторно, щоб не було дубля."));
                                let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
                                let url = proj.and_then(|p| if name == "Modrinth" { (!p.modrinth_id.is_empty()).then(|| format!("https://modrinth.com/project/{}/versions", p.modrinth_id)) } else { (!p.curse_id.is_empty()).then(|| format!("https://www.curseforge.com/projects/{}", p.curse_id)) });
                                if let Some(u) = url && icon_btn(ui, Some(Ic::External), i18n::tr("Відкрити проєкт на сайті"), false).clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(u));
                                }
                            }
                        }
                        None => {}
                    }
                }
                if !busy && any_err {
                    hint(ui, if any_ok { i18n::tr("«Повторити» надішле файл лише туди, де не вдалося: дублів не буде.") } else { i18n::tr("Виправте причину й натисніть «Повторити»: введені дані збережено.") });
                }
            });
            }
            if new_version {
                (self.number, self.name, self.changelog, self.file) = Default::default();
                self.number = self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| bump(&p.last_number)).unwrap_or_default();
                (self.use_mr, self.use_cf, self.mr_res, self.cf_res) = (true, true, None, None);
                (self.file_info, self.file_note) = (None, String::new());
                (self.extra_files, self.send_deps, self.featured, self.edit_mode) = (vec![], false, false, true);
                self.msg.clear();
            }
            ui.add_space(if ctx.content_rect().height() < 600.0 { 2.0 } else { 8.0 });
        }
        let narrow = ui.available_width() < 560.0;
        let why = if busy { i18n::tr("Триває публікація").to_string() } else { ready.as_ref().err().cloned().unwrap_or_default() };
        let can = !busy && ready.is_ok();
        let failed: Vec<&str> = [("Modrinth", &self.mr_res), ("CurseForge", &self.cf_res)].into_iter().filter(|(_, r)| matches!(r, Some(Err(_)))).map(|(n, _)| n).collect();
        let label = if busy { i18n::tr("Публікація…").to_string() } else if can && self.armed() { i18n::tr("Ще раз Ctrl+Enter — опублікувати").to_string() } else if !failed.is_empty() && can { i18n::fmt(i18n::tr("Повторити: {}"), &[&(failed.join(" і "))]) } else { i18n::tr("btn.publish").to_string() };
        let mk =|| primary(&label).min_size(egui::vec2(if narrow { 150.0 } else { 220.0 }, if narrow { 38.0 } else { 44.0 }));
        // у вузькому вікні — один рядок (підсумок і кнопка); два рядки лише під час публікації, де є кнопки «Скасувати»
        if card {
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| self.bar_summary(ui, busy, &ready, mr, cf, true));
                ui.add_space(8.0);
                let w = ui.available_width();
                if ui.add_enabled(can, primary(&label).min_size(egui::vec2(w, 44.0))).on_disabled_hover_text(why).clicked() {
                    self.start_publish(ctx, mr, cf);
                }
            });
        } else if busy {
            // вимкнена «Публікація…» нічого не дає й налазила б на кнопки «Скасувати»: під час відправки лише статус і «Скасувати»
            ui.horizontal_wrapped(|ui| self.bar_summary(ui, busy, &ready, mr, cf, false));
        } else {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(can, mk()).on_disabled_hover_text(why).clicked() {
                        self.start_publish(ctx, mr, cf);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| self.bar_summary(ui, busy, &ready, mr, cf, false));
                });
            });
        }
    }

    /// Підсумок у нижній панелі: прогрес, готовність або причина.
    fn bar_summary(&mut self, ui: &mut egui::Ui, busy: bool, ready: &Result<(), String>, mr: bool, cf: bool, in_card: bool) {
        if busy {
            // слот висотою з кнопку: у horizontal_wrapped рядок виростає лише з появою кнопок, і спінер без нього з'їжджає вниз
            let (slot, _) = ui.allocate_exact_size(egui::vec2(18.0, 38.0), egui::Sense::hover());
            ui.put(egui::Rect::from_center_size(slot.center(), egui::vec2(16.0, 16.0)), egui::Spinner::new().size(16.0));
            let running: Vec<(&str, &Up)> = [("Modrinth", &self.up_mr), ("CurseForge", &self.up_cf)].into_iter().filter(|(_, u)| u.active.load(Relaxed)).collect();
            ui.label(if running.iter().all(|(_, u)| u.cancel.load(Relaxed)) { i18n::tr("Скасування…") } else { i18n::tr("Надсилаємо файл, зачекайте…") });
            let many = running.len() > 1;
            let tight = ui.available_width() < 460.0; // після спінера й тексту лишилось мало місця: лише назва платформи (повна назва й для скрінрідера, і в підказці)
            for (n, u) in running {
                let cancelling = u.cancel.load(Relaxed);
                let full = if many { i18n::fmt(i18n::tr("Скасувати {}"), &[&(n)]) } else { i18n::tr("btn.cancel").to_string() };
                let label = if many && tight { n.to_string() } else { full.clone() };
                if ui.add_enabled_ui(!cancelling, |ui| aria(ghost_btn(ui, Some(Ic::Cross), &label), full.clone())).inner.on_hover_text(full).clicked() {
                    u.cancel.store(true, Relaxed);
                }
            }
        } else {
            let d = self.steps_done();
            let cur = [0usize, 1, 2, 3, 5].into_iter().find(|&i| !d[i]).map_or(6, |i| i + 1);
            match ready {
                Ok(()) => {
                    let p = self.sel.and_then(|i| self.cfg.projects.get(i));
                    // після часткового успіху показуємо лише платформи, куди ще надсилатимемо
                    let (mr_left, cf_left) = (mr && !matches!(self.mr_res, Some(Ok(_))), cf && !matches!(self.cf_res, Some(Ok(_))));
                    let (mr0, cf0) = (mr, cf);
                    let (mr, cf) = if mr_left || cf_left { (mr_left, cf_left) } else { (mr, cf) };
                    let to = match (mr, cf) { (true, true) => "Modrinth і CurseForge", (true, false) => "Modrinth", _ => "CurseForge" };
                    let txt = p.map_or_else(String::new, |p| format!("{} {} → {}", p.name, self.number.trim(), to));
                    let warns = self.pre_warnings(mr0, cf0);
                    // рядки один під одним: у вузькому вікні текст не обрізається збоку від кнопки
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        // ✓ «усе гаразд» лише без попереджень; з попередженнями рядок «що й куди» нейтральний, а попередження йдуть окремо під ним
                        // у правій картці назву проєкту й платформи вже показано вище: повторювати їх тут (і обрізати) не треба
                        if warns.is_empty() {
                            icon_label_c(ui, Ic::Check, SUCCESS, if in_card { i18n::tr("Готово до публікації") } else { &txt }, SUCCESS);
                            if let Some(p) = p.filter(|_| !in_card && ui.ctx().content_rect().height() >= 600.0) {
                                let full = release_summary(&p.name, &self.release(), mr0, cf0);
                                ui.add(egui::Label::new(RichText::new(&full).color(MUTED).size(HINT_SZ)).truncate()).on_hover_text(full);
                            }
                        } else {
                            if !in_card {
                                ui.add(egui::Label::new(RichText::new(&txt).color(TEXT)).truncate());
                            }
                            for w in &warns {
                                warn_row(ui, w);
                            }
                        }
                    });
                }
                Err(e) => {
                    ui.label(RichText::new(i18n::fmt(i18n::tr("Крок {} з 6"), &[&(cur)])).color(MUTED).size(HINT_SZ));
                    icon_label(ui, Ic::Warn, WARN, &i18n::fmt(i18n::tr("Ще не готово: {}"), &[&(e)]));
                    if e.contains("Немає токена") {
                        if ghost_btn(ui, Some(Ic::Gear), i18n::tr("До налаштувань")).clicked() {
                            (self.tab, self.set_sec) = (Tab::Settings, 0);
                        }
                    } else if cur <= 6 && ghost_btn(ui, None, &i18n::fmt(i18n::tr("До кроку {}"), &[&(cur)])).on_hover_text(i18n::tr("Прокрутити до першого незаповненого кроку")).clicked() {
                        self.focus_step = Some(cur - 1);
                    }
                }
            }
        }
    }

    /// Історія: у прокручуваній частині сторінки.
    fn publish_extras(&mut self, ui: &mut egui::Ui) {
        {
            if !self.history.is_empty() {
                ui.add_space(8.0);
                if std::mem::take(&mut self.focus_hist) {
                    ui.scroll_to_cursor(Some(egui::Align::TOP));
                }
                let (mut clear, mut export, mut copied) = (false, false, false);
                let mut again: Option<serde_json::Value> = None;
                let found =history_filter(&self.history, &self.hist_q, self.hist_fail);
                let limit = if self.hist_all { usize::MAX } else { 10 };
                let total = found.len();
                ui.scope(|ui| { // заголовок «Історія · N» тепер над сторінкою, тож згортання не потрібне
                    ui.horizontal_wrapped(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut self.hist_q).hint_text(i18n::tr("Пошук: проєкт, номер, платформа, текст")).min_size(egui::vec2(0.0, CTRL_H)).desired_width(340.0_f32.min(ui.available_width() - 8.0)));
                        name_edit(&r, i18n::tr("Пошук в історії публікацій"), i18n::tr("Пошук: проєкт, номер, платформа, текст"), &self.hist_q);
                        if choice(ui, self.hist_fail, i18n::tr("Лише помилки")).clicked() { self.hist_fail = !self.hist_fail; }
                        if self.hist_confirm {
                            if ui.add(danger(i18n::tr("Так, очистити історію"))).clicked() {
                                clear = true;
                            }
                            if ghost_btn(ui, Some(Ic::Cross), i18n::tr("btn.cancel")).clicked() {
                                self.hist_confirm = false;
                            }
                        } else {
                            if ghost_btn(ui, Some(Ic::File), i18n::tr("Експорт у CSV…")).on_hover_text(i18n::tr("Уся історія з файлу у таблицю")).clicked() {
                                export = true;
                            }
                            if ghost_btn(ui, None, i18n::tr("Очистити історію")).clicked() {
                                self.hist_confirm = true;
                            }
                        }
                    });
                    if found.is_empty() {
                        hint(ui, i18n::tr("Нічого не знайдено за цим фільтром."));
                    }
                    let avail = ui.available_width();
                    let narrow = avail < 560.0;
                    egui::Grid::new("history_grid").num_columns(if narrow { 1 } else { 3 }).spacing([16.0, 14.0]).min_col_width(60.0).show(ui, |ui| {
                        ui.spacing_mut().interact_size.y = 22.0; // комірки рядка вирівнюються по верху, без зсуву від висоти кнопки
                        for e in found.iter().take(limit) {
                            let ok = e["ok"].as_bool().unwrap_or(false);
                            let col = if ok { SUCCESS } else { ERROR };
                            let msg = e["msg"].as_str().unwrap_or("");
                            let vr = ui.vertical(|ui| {
                                // статус усередині першого рядка комірки: Grid центрує окрему комірку по висоті рядка і зсуває її від назви
                                ui.set_max_width(if narrow { (avail - 40.0).max(160.0) } else { 330.0 });
                                ui.horizontal(|ui| {
                                    ui.allocate_ui_with_layout(egui::vec2(96.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        ui.set_min_width(96.0);
                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), egui::Sense::hover());
                                        draw_icon(ui.painter(), rect.center(), if ok { Ic::Check } else { Ic::Cross }, 14.0, col);
                                        ui.label(RichText::new(if ok { i18n::tr("btn.done") } else { i18n::tr("label.error") }).color(col)).on_hover_text(msg);
                                    });
                                    let full = format!("{} {}", e["project"].as_str().unwrap_or("?"), e["number"].as_str().unwrap_or(""));
                                    let tip = if msg.is_empty() { full.clone() } else { format!("{full}\n{msg}") };
                                    ui.add(egui::Label::new(full).truncate()).on_hover_text(tip);
                                });
                                ui.horizontal_top(|ui| { // другорядні рядки вирівняні по назві проєкту, а не під статусом
                                ui.add_space(104.0);
                                ui.vertical(|ui| {
                                if narrow {
                                    hint(ui, &format!("{} · {}", e["platform"].as_str().unwrap_or(""), ago(e["t"].as_u64().unwrap_or(0))));
                                }
                                if let Some(u) = history_link(e) {
                                    ui.horizontal(|ui| {
                                        for (label, copy) in [(i18n::tr("btn.open"), false), (i18n::tr("btn.copy_link"), true)] {
                                            let r = ui.add(egui::Label::new(RichText::new(label).color(LINK).underline().size(HINT_SZ)).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if r.clicked() {
                                                if copy { ui.ctx().copy_text(u.clone()); copied = true; } else { ui.ctx().open_url(egui::OpenUrl::new_tab(u.clone())); }
                                            }
                                        }
                                    });
                                }
                                if e["number"].as_str().is_some_and(|n| !n.is_empty()) && self.pending == 0 {
                                    let r = ui.add(egui::Label::new(RichText::new(i18n::tr("Повторити з цього запису")).color(LINK).underline().size(HINT_SZ)).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if r.on_hover_text(i18n::tr("Підставити проєкт, номер, тип, MC і лоадери у форму публікації")).clicked() {
                                        again = Some((*e).clone());
                                    }
                                }
                                if !ok {
                                    let m: String = msg.chars().take(300).collect();
                                    ui.label(RichText::new(m).color(MUTED).size(HINT_SZ));
                                }
                                });
                                });
                            });
                            let r = if narrow {
                                vr.response // платформа і час уже в другому рядку
                            } else {
                                ui.horizontal_top(|ui| ui.label(e["platform"].as_str().unwrap_or("")));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                    ui.label(RichText::new(ago(e["t"].as_u64().unwrap_or(0))).color(MUTED));
                                }).response
                            };
                            ui.end_row();
                            let y = r.rect.bottom() + 7.0;
                            // підсвітка рядка під курсором (тонка, поверх тексту не заважає)
                            let row = egui::Rect::from_min_max(egui::pos2(ui.min_rect().left(), r.rect.top() - 7.0), egui::pos2(ui.max_rect().right(), y));
                            if ui.rect_contains_pointer(row) {
                                ui.painter().rect_filled(row, 4, ACCENT.gamma_multiply(0.06));
                            }
                            ui.painter().hline(ui.min_rect().left()..=ui.max_rect().right(), y, Stroke::new(1.0, BORDER));
                        }
                    });
                    if total > 10 && ghost_btn(ui, None, if self.hist_all { i18n::tr("Показати менше") } else { i18n::tr("Показати всі") }).clicked() {
                        self.hist_all = !self.hist_all;
                    }
                });
                if copied {
                    self.notice = i18n::tr("notice.link_copied").into();
                }
                if let Some(e) = again {
                    self.restore_history(&e);
                }
                if export {
                    self.spawn_opt(ui.ctx(), move || {
                        let p = rfd::FileDialog::new().set_file_name("modwire-history.csv").add_filter("CSV", &["csv"]).save_file()?;
                        Some(Msg::Notice(match history_export_to(&p) {
                            Ok(n) => i18n::fmt(i18n::tr("Історію експортовано ({} записів): {}"), &[&(n), &(p.display())]),
                            Err(e) => i18n::fmt(i18n::tr("Не вдалося записати файл: {}"), &[&(e)]),
                        }))
                    });
                }
                if clear {
                    match history_clear() {
                        Ok(()) => self.history.clear(),
                        Err(e) => self.msg = i18n::fmt(i18n::tr("Не вдалося очистити історію: {}"), &[&(e)]),
                    }
                    self.hist_confirm = false;
                }
            } else {
                ui.add_space(8.0);
                egui::Frame::new().stroke(Stroke::new(1.0, BORDER)).corner_radius(egui::CornerRadius::same(CARD_R)).inner_margin(egui::Margin::symmetric(CARD_PAD, 12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    icon_label_c(ui, Ic::Info, MUTED, i18n::tr("Історія порожня: записи з'являться після першої публікації."), MUTED);
                });
            }
        }
    }

    /// Попередження, які не блокують публікацію, але мають бути видні до неї: токен не перевірено, версія нижча за попередню.
    fn pre_warnings(&self, mr: bool, cf: bool) -> Vec<String> {
        // для платформи, куди вже була спроба відправки, «токен не перевірено» застаріле (результат уже показано); для решти лишається
        let mut w: Vec<String> = unchecked_tokens(mr && self.mr_res.is_none(), cf && self.cf_res.is_none(), self.cfg.modrinth_checked, self.cfg.curse_checked).into_iter().collect();
        // після часткового успіху «версію вже опубліковано» було б хибним: повтор для другої платформи й є наступним кроком
        let partial_ok = matches!(self.mr_res, Some(Ok(_))) || matches!(self.cf_res, Some(Ok(_)));
        if let Some(p) = self.sel.and_then(|i| self.cfg.projects.get(i)).filter(|_| !partial_ok) {
            w.extend(version_warning(&self.number, &p.name, &p.last_number, &self.history));
        }
        // те, що крок «Сумісність» показує лише всередині майстра, має бути видно й у зведенні
        let bad: Vec<String> = if self.mc_all.is_empty() { vec![] } else { parse_mc(&self.cfg.last.mc_versions).into_iter().filter(|t| !self.mc_all.contains(t)).collect() };
        if !bad.is_empty() {
            w.push(i18n::fmt(i18n::tr("Невідомі версії: {}"), &[&(bad.join(", "))]));
        }
        if let Some(i) = &self.file_info {
            w.extend(compat_warnings(i, &parse_mc(&self.cfg.last.mc_versions), &self.cfg.last.loaders).into_iter().map(|s| s.to_string()));
        }
        if mixed_pack_loaders(&self.cfg.last.loaders) {
            w.push(i18n::tr("Ресурспак чи датапак не поєднується з лоадерами модів: лишіть один тип.").to_string());
        }
        w
    }

    /// Чи «заряджено» Ctrl+Enter: перше натискання було менше 2 с тому для цього самого проєкту.
    fn armed(&self) -> bool {
        self.ctrl_armed.is_some_and(|(t, s)| s == self.sel && t.elapsed().as_secs_f32() < 2.0)
    }

    fn start_publish(&mut self, ctx: &egui::Context, mr: bool, cf: bool) {
        self.ctrl_armed = None;
        self.msg.clear();
        let rel = self.release();
        let (Some(si), Some(file)) = (self.sel, self.file.clone()) else { return };
        let Some(p) = self.cfg.projects.get(si).cloned() else { return };
        self.cur = (p.name.clone(), rel.number.clone());
        self.cur_name = rel.name.clone();
        self.cfg.last.project = si;
        self.cur_idx = Some(si);
        self.cfg.last.release_type = rel.release_type.clone();
        self.save();
        self.cur_cl = self.changelog.clone();
        self.cur_meta = (self.cfg.last.mc_versions.clone(), self.cfg.last.loaders.clone(), RELEASE_TYPES[self.release_type].into());
        // результат платформи, що вже опублікована, не стираємо: повтор надсилає лише туди, де не вдалося
        if mr { self.mr_res = None; }
        if cf { self.cf_res = None; }
        let core_key = self.cfg.curse_core_key.trim().to_string();
        if mr { self.up_mr = Up::default(); }
        if cf { self.up_cf = Up::default(); }
        for (on, plat, token, id) in [
            // токен обрізаємо так само, як у «Перевірити»: пробіл чи Enter зі вставки ламає заголовок
            (mr, P::Mr, self.cfg.modrinth_token.trim().to_string(), p.modrinth_id.clone()),
            (cf, P::Cf, self.cfg.curse_token.trim().to_string(), p.curse_id.clone()),
        ] {
            if !on { continue; }
            self.pending += 1;
            let (rel, file, core_key) = (rel.clone(), file.clone(), core_key.clone());
            let up = if matches!(plat, P::Mr) { self.up_mr.clone() } else { self.up_cf.clone() };
            up.active.store(true, Relaxed);
            self.spawn(ctx, move || {
                // паніка в потоці не має лишати інтерфейс у «Публікація…»
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match plat {
                    P::Mr => masked(publish_modrinth(token.trim(), &id, &rel, &file, &up), &[token.as_str()]),
                    P::Cf => masked(publish_curse(token.trim(), &core_key, &id, &rel, &file, &up), &[token.as_str(), core_key.as_str()]),
                }))
                .unwrap_or_else(|_| Err(i18n::tr("Внутрішня помилка під час публікації: перевірте на сайті, чи не з'явилась версія").into()));
                up.active.store(false, Relaxed);
                Msg::Done(plat, r)
            });
        }
    }

    // ---------- Налаштування ----------
    fn settings_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading(i18n::tr("nav.settings"));
        hint(ui, "Зміни зберігаються одразу. Токени лишаються на цьому комп'ютері.");
        ui.add_space(12.0);
        let mut changed = false;
        self.set_sec = self.set_sec.min(4);
        let items = [(Ic::Key, i18n::tr("Облікові записи")), (Ic::Db, i18n::tr("Дані")), (Ic::Refresh, i18n::tr("Оновлення")), (Ic::Shield, i18n::tr("Безпека")), (Ic::Info, i18n::tr("Про застосунок"))];
        let dots = [
            (!self.cfg.modrinth_token.trim().is_empty() || !self.cfg.curse_token.trim().is_empty()).then_some(SUCCESS),
            None,
            self.update.is_some().then_some(ACCENT),
            None,
            None,
        ];
        let avail = ui.available_rect_before_wrap();
        // Широке вікно: меню розділів ліворуч, вміст праворуч. Вузьке: розділи рядком чипів над вмістом.
        let body = if avail.width() >= 600.0 {
            let nav = egui::Rect::from_min_size(avail.min, egui::vec2(232.0, avail.height()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(nav), |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (i, (ic, label)) in items.iter().enumerate() {
                    if side_item(ui, *ic, label, self.set_sec == i, dots[i]).clicked() { self.set_sec = i; }
                }
            });
            egui::Rect::from_min_max(egui::pos2(avail.left() + 256.0, avail.top()), avail.max)
        } else {
            let top = ui.scope(|ui| {
                // п'ять розділів у вузькому вікні переносилися б сиротою на другий рядок: один список завжди вміщається
                combo_box("set_sec").width(ui.available_width()).selected_text(items[self.set_sec].1).show_ui(ui, |ui| {
                    for (i, (_, label)) in items.iter().enumerate() {
                        ui.selectable_value(&mut self.set_sec, i, *label);
                    }
                });
                ui.add_space(8.0);
            });
            egui::Rect::from_min_max(egui::pos2(avail.left(), top.response.rect.bottom()), avail.max)
        };
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.set_sec {
                    0 => {
                        sec_head(ui, i18n::tr("Облікові записи"), "Токени платформ, на які ви публікуєте.");
                        self.mr_card(ui, ctx, &mut changed);
                        ui.add_space(12.0);
                        self.cf_card(ui, ctx, &mut changed);
                    }
                    1 => { sec_head(ui, i18n::tr("Дані"), "Список проєктів без токенів можна перенести на інший комп'ютер."); self.data_card(ui, ctx) }
                    2 => { sec_head(ui, i18n::tr("Оновлення"), "Застосунок перевіряє нові версії на GitHub."); self.update_ui(ui, ctx); self.updates_card(ui, ctx) }
                    3 => { sec_head(ui, i18n::tr("Безпека"), "Де лежать ваші токени і як їх прибрати."); self.security_card(ui, &mut changed) }
                    _ => {
                        ui.horizontal(|ui| {
                            ui.add(egui::Image::from_bytes("bytes://modwire-icon.png", include_bytes!("../assets/icon.png").as_slice()).fit_to_exact_size(egui::vec2(48.0, 48.0)).corner_radius(13));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(format!("Modwire {}", env!("CARGO_PKG_VERSION"))).size(20.0).color(TEXT));
                                hint(ui, "Публікація версій модів на Modrinth і CurseForge.");
                            });
                        });
                        ui.add_space(12.0);
                        self.about_card(ui)
                    }
                }
            });
        });
        if changed {
            self.save();
        }
    }

    fn mr_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, changed: &mut bool) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            acc_head(ui, "Modrinth", MR_GREEN, &self.cfg.modrinth_token, "1. Увійдіть на Modrinth.\n2. Сторінка Personal access tokens > «Create a PAT».\n3. Дайте право Create versions (і Read user, Read projects для списку проєктів).\n4. Скопіюйте токен і вставте сюди.", "https://modrinth.com/settings/pats");
            ui.add_space(6.0);
            // права звичайним текстом з поясненням (назви API лишаються буквальними): рамки-чипи читалися як кнопки
            hint(ui, i18n::tr("Потрібні права: створення версій (Create versions), читання профілю (Read user), читання проєктів (Read projects)."));
            ui.add_space(8.0);
            let checking = self.check_msg == Chk::Checking;
            let (edited, go) = token_row(ui, "Токен", &mut self.cfg.modrinth_token, &mut self.show_tokens, checking, "Перевірити токен Modrinth");
            if edited {
                self.cfg.modrinth_checked = 0;
                *changed = true;
            }
            if go {
                let t = self.cfg.modrinth_token.trim().to_string();
                if t.is_empty() {
                    self.check_msg = Chk::Err(i18n::tr("Спершу вставте токен").into());
                } else {
                    self.check_msg = Chk::Checking;
                    // у помилці — довжина й префікс токена (без самого токена): видно, чи вставлено саме токен Modrinth (64 символи, mrp_…)
                    let note = format!(" [довжина токена: {}, починається з mrp_: {}]", t.chars().count(), if t.starts_with("mrp_") { "так" } else { "ні" });
                    self.spawn(ctx, move || Msg::Check(masked(modrinth_token_check(&t).map_err(|e| e + &note), &[t.as_str()])));
                }
            }
            status(ui, &self.check_msg);
        });
    }

    fn cf_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, changed: &mut bool) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            acc_head(ui, "CurseForge", CF_ORANGE, &self.cfg.curse_token, "1. Увійдіть на CurseForge.\n2. Account > API Tokens.\n3. Створіть токен (Generate token) і скопіюйте.\n4. Вставте сюди.", "https://www.curseforge.com/account/api-tokens");
            hint(ui, i18n::tr("Токен для завантаження файлів (Upload API token): його видають у налаштуваннях облікового запису CurseForge."));
            ui.add_space(8.0);
            let checking = self.cf_msg == Chk::Checking;
            let (edited, go) = token_row(ui, i18n::tr("Токен завантаження"), &mut self.cfg.curse_token, &mut self.show_tokens, checking, "Перевірити токен CurseForge");
            if edited {
                self.cfg.curse_checked = 0;
                *changed = true;
            }
            if go {
                let t = self.cfg.curse_token.trim().to_string();
                if t.is_empty() {
                    self.cf_msg = Chk::Err(i18n::tr("Спершу вставте токен").into());
                } else {
                    self.cf_msg = Chk::Checking;
                    self.spawn(ctx, move || Msg::CheckCf(masked(curse_token_check(&t), &[t.as_str()])));
                }
            }
            status(ui, &self.cf_msg);
            row_sep(ui);
            ui.horizontal(|ui| {
                ui.label(RichText::new(i18n::tr("Core API-ключ")).color(TEXT).size(14.5));
                badge(ui, i18n::tr("Необов'язково"), MUTED);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    link(ui, "1. Увійдіть у CurseForge for Studios console.\n2. Розділ API keys > скопіюйте ключ.\n3. Вставте сюди.", "https://console.curseforge.com/#/api-keys");
                });
            });
            hint(ui, i18n::tr("Лише для назви й іконки проєкту під час додавання."));
            ui.add_space(8.0);
            let checking = self.key_msg == Chk::Checking;
            let (edited, go) = token_row(ui, "Ключ", &mut self.cfg.curse_core_key, &mut self.show_tokens, checking, "Перевірити Core API-ключ");
            if edited {
                self.cfg.core_checked = 0;
                *changed = true;
            }
            if go {
                let k = self.cfg.curse_core_key.trim().to_string();
                if k.is_empty() {
                    self.key_msg = Chk::Err(i18n::tr("Спершу вставте ключ").into());
                } else {
                    self.key_msg = Chk::Checking;
                    self.spawn(ctx, move || Msg::CheckKey(masked(curse_key_check(&k), &[k.as_str()])));
                }
            }
            status(ui, &self.key_msg);
        });
    }

    fn updates_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_width(ui.available_width());
            let last = if self.cfg.update_checked > 0 { format!("Остання перевірка: {}", ago(self.cfg.update_checked)) } else { "Ще не перевірялось.".to_string() };
            let busy = self.upd_checking || self.upd_applying;
            let desc = format!("Modwire {}. {} Оновлення відбувається лише після вашого натискання «Оновити».", env!("CARGO_PKG_VERSION"), last);
            if setting_row(ui, i18n::tr("Перевірити оновлення"), &desc, |ui| ui.add_enabled_ui(!busy, |ui| icon_btn(ui, Some(Ic::Refresh), if self.upd_checking { i18n::tr("status.checking") } else { "Перевірити" }, false)).inner.clicked()) {
                self.upd_checking = true;
                self.upd_err.clear();
                self.spawn(ctx, || Msg::Update(check_update(), true));
            }
        });
    }

    fn about_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(i18n::tr("Мова інтерфейсу")).color(TEXT).size(14.5));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (code, label, l) in [("uk", "Українська", i18n::Lang::Uk), ("en", "English", i18n::Lang::En), ("ru", "Русский", i18n::Lang::Ru)] {
                    let on = i18n::lang() == l;
                    if choice(ui, on, label).clicked() && !on {
                        i18n::set_lang(l);
                        self.cfg.lang = if code == "uk" { String::new() } else { code.into() };
                        self.save();
                    }
                }
            });
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(i18n::tr("Гарячі клавіші (F1 відкриває цей розділ)")).color(TEXT).size(14.5));
            ui.add_space(4.0);
            for line in [i18n::tr("Ctrl+1, Ctrl+2, Ctrl+3, Ctrl+4: Проєкти, Публікація, Історія, Налаштування"), "Ctrl+Enter: опублікувати, коли форму заповнено", "Tab / Shift+Tab: перехід між елементами, Пробіл або Enter: натиснути", "Esc: закрити діалог чи скасувати видалення"] {
                hotkey_row(ui, line);
                ui.add_space(4.0);
            }
        });
    }

    fn data_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            if setting_row(ui, i18n::tr("Експорт проєктів…").trim_end_matches('…'), "Зберегти список проєктів у файл. Токени в нього не потрапляють.", |ui| ghost_btn(ui, Some(Ic::File), i18n::tr("Експорт проєктів…")).clicked()) {
                let json = export_projects(&self.cfg.projects);
                self.spawn_opt(ctx, move || {
                    let p = rfd::FileDialog::new().set_file_name("modwire-projects.json").add_filter("JSON", &["json"]).save_file()?;
                    Some(Msg::Notice(match std::fs::write(&p, json) {
                        Ok(()) => i18n::fmt(i18n::tr("Проєкти експортовано: {}"), &[&(p.display())]),
                        Err(e) => i18n::fmt(i18n::tr("Не вдалося записати файл: {}"), &[&(e)]),
                    }))
                });
            }
            row_sep(ui);
            if setting_row(ui, i18n::tr("Імпорт проєктів…").trim_end_matches('…'), "Доповнює поточний список, а не замінює його.", |ui| ghost_btn(ui, Some(Ic::File), i18n::tr("Імпорт проєктів…")).clicked()) {
                self.spawn_opt(ctx, || {
                    let p = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()?;
                    let text = read_text_capped(&p, 2_000_000);
                    Some(match text.and_then(|t| parse_projects_file(&t)) {
                        Ok(ps) => Msg::Imported(ps),
                        Err(e) => Msg::Notice(i18n::fmt(i18n::tr("Не вдалося імпортувати: {}"), &[&(e)])),
                    })
                });
            }
            row_sep(ui);
            let refreshing = self.refreshing;
            if setting_row(ui, i18n::tr("Оновити іконки проєктів"), "Підтягує іконку з Modrinth/CurseForge (потрібен токен або Core API-ключ).", |ui| ui.add_enabled_ui(!refreshing, |ui| ghost_btn(ui, Some(Ic::Refresh), i18n::tr("Оновити іконки проєктів"))).inner.clicked()) {
                self.refreshing = true;
                let (t, k, ps) = (self.cfg.modrinth_token.trim().to_string(), self.cfg.curse_core_key.trim().to_string(), self.cfg.projects.clone());
                self.spawn(ctx, move || Msg::Refreshed(refresh_projects(&t, &k, &ps)));
            }
        });
        ui.add_space(12.0);
        egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, Color32::from_rgb(0x4a, 0x2a, 0x2a))).corner_radius(CARD_R).inner_margin(egui::Margin::same(CARD_PAD)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if self.hist_confirm {
                banner(ui, Kind::Error, i18n::tr("Очистити всю історію публікацій? Сайти не зміняться."));
                ui.horizontal(|ui| {
                    if ui.add(danger(i18n::tr("Так, очистити історію"))).clicked() {
                        match history_clear() {
                            Ok(()) => self.history.clear(),
                            Err(e) => self.msg = i18n::fmt(i18n::tr("Не вдалося очистити історію: {}"), &[&(e)]),
                        }
                        self.hist_confirm = false;
                    }
                    if ghost_btn(ui, Some(Ic::Cross), i18n::tr("btn.cancel")).clicked() {
                        self.hist_confirm = false;
                    }
                });
            } else {
                let has_hist = !self.history.is_empty();
                if setting_row(ui, i18n::tr("Очистити історію"), "Прибирає записи лише в застосунку, сайти не змінюються.", |ui| ui.add_enabled(has_hist, danger(i18n::tr("Очистити історію"))).clicked()) {
                    self.hist_confirm = true;
                }
            }
        });
    }

    fn security_card(&mut self, ui: &mut egui::Ui, changed: &mut bool) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            banner(ui, Kind::Warn, i18n::tr("Токени в конфігу зберігаються відкритим текстом."));
            ui.add_space(8.0);
            if self.confirm_clear {
                banner(ui, Kind::Error, i18n::tr("Видалити всі токени з конфігу? Проєкти залишаться."));
                ui.horizontal(|ui| {
                    if ui.add(danger(i18n::tr("Так, очистити"))).clicked() {
                        self.cfg.modrinth_token.clear();
                        self.cfg.curse_token.clear();
                        self.cfg.curse_core_key.clear();
                        (self.cfg.modrinth_checked, self.cfg.curse_checked, self.cfg.core_checked) = (0, 0, 0);
                        (self.check_msg, self.cf_msg, self.key_msg) = Default::default();
                        self.confirm_clear = false;
                        *changed = true;
                    }
                    if ghost_btn(ui, Some(Ic::Cross), i18n::tr("btn.cancel")).clicked() {
                        self.confirm_clear = false;
                    }
                });
            } else if setting_row(ui, i18n::tr("Очистити токени"), "Видаляє токени Modrinth, CurseForge і Core API-ключ. Проєкти залишаться.", |ui| ui.add(danger(i18n::tr("Очистити токени"))).clicked()) {
                self.confirm_clear = true;
            }
        });
    }
}

#[derive(Clone, Copy)]
enum Ic {
    Plus,
    Check,
    Cross,
    Upload,
    Gear,
    Trash,
    External,
    Eye,
    Warn,
    Info,
    Grid,
    File,
    Chevron,
    ChevronUp,
    Copy,
    Refresh,
    Clock,
    Pin,
    Key,
    Db,
    Shield,
}

/// Єдиний малювальник значків: без зовнішніх файлів і без гліфів шрифту.
fn draw_icon(p: &egui::Painter, c: egui::Pos2, kind: Ic, size: f32, col: Color32) {
    let h = size / 2.0;
    let st = Stroke::new((size * 0.11).clamp(1.5, 2.4), col); // вага штриха росте з розміром: 12px — 1.5, 16px — 1.76, 48px — 2.4
    let v = |x: f32, y: f32| c + egui::vec2(x * h, y * h);
    let line = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(pts.iter().map(|&(x, y)| v(x, y)).collect(), st));
    };
    match kind {
        Ic::Plus => {
            line(&[(-0.7, 0.0), (0.7, 0.0)]);
            line(&[(0.0, -0.7), (0.0, 0.7)]);
        }
        Ic::Check => line(&[(-0.7, 0.05), (-0.2, 0.55), (0.75, -0.55)]),
        Ic::Cross => {
            line(&[(-0.6, -0.6), (0.6, 0.6)]);
            line(&[(0.6, -0.6), (-0.6, 0.6)]);
        }
        Ic::Upload => {
            line(&[(0.0, 0.5), (0.0, -0.7)]);
            line(&[(-0.5, -0.2), (0.0, -0.7), (0.5, -0.2)]);
            line(&[(-0.8, 0.9), (0.8, 0.9)]);
        }
        Ic::Gear => {
            p.circle_stroke(c, 0.55 * h, st);
            p.circle_filled(c, 0.2 * h, col);
            for i in 0..8 {
                let d = egui::Vec2::angled(i as f32 * std::f32::consts::FRAC_PI_4);
                p.line_segment([c + d * 0.72 * h, c + d * 0.98 * h], st);
            }
        }
        Ic::Trash => {
            line(&[(-0.8, -0.5), (0.8, -0.5)]);
            line(&[(-0.3, -0.5), (-0.3, -0.8), (0.3, -0.8), (0.3, -0.5)]);
            line(&[(-0.6, -0.5), (-0.5, 0.9), (0.5, 0.9), (0.6, -0.5)]);
        }
        Ic::External => {
            line(&[(0.1, -0.1), (0.8, -0.8)]);
            line(&[(0.2, -0.8), (0.8, -0.8), (0.8, -0.2)]);
            line(&[(-0.1, -0.7), (-0.8, -0.7), (-0.8, 0.8), (0.7, 0.8), (0.7, 0.1)]);
        }
        Ic::Eye => {
            let n = 14;
            let arc = |sign: f32| -> Vec<(f32, f32)> {
                (0..=n).map(|i| {
                    let t = i as f32 / n as f32;
                    (-0.95 + 1.9 * t, sign * 0.55 * (std::f32::consts::PI * t).sin())
                }).collect()
            };
            line(&arc(-1.0));
            line(&arc(1.0));
            p.circle_stroke(c, 0.28 * h, st);
        }
        Ic::Warn => {
            line(&[(0.0, -0.85), (0.9, 0.75), (-0.9, 0.75), (0.0, -0.85)]);
            line(&[(0.0, -0.25), (0.0, 0.2)]);
            p.circle_filled(v(0.0, 0.5), 0.1 * h + 0.6, col);
        }
        Ic::Info => {
            p.circle_stroke(c, 0.9 * h, st);
            line(&[(0.0, -0.05), (0.0, 0.5)]);
            p.circle_filled(v(0.0, -0.42), 0.1 * h + 0.6, col);
        }
        Ic::File => {
            line(&[(-0.6, -0.9), (0.2, -0.9), (0.7, -0.4), (0.7, 0.9), (-0.6, 0.9), (-0.6, -0.9)]);
            line(&[(-0.3, 0.1), (0.4, 0.1)]);
            line(&[(-0.3, 0.5), (0.4, 0.5)]);
        }
        Ic::Chevron => line(&[(-0.6, -0.25), (0.0, 0.35), (0.6, -0.25)]),
        Ic::ChevronUp => line(&[(-0.6, 0.25), (0.0, -0.35), (0.6, 0.25)]),
        Ic::Copy => {
            // два аркуші: задній показано кутом, передній — повним контуром
            line(&[(-0.2, -0.7), (-0.2, -0.9), (0.8, -0.9), (0.8, 0.5), (0.6, 0.5)]);
            line(&[(-0.8, -0.5), (0.45, -0.5), (0.45, 0.9), (-0.8, 0.9), (-0.8, -0.5)]);
        }
        Ic::Pin => {
            // кнопка-«канцелярська»: голівка, основа й голка
            line(&[(-0.4, -0.85), (0.4, -0.85)]);
            line(&[(-0.28, -0.85), (-0.28, -0.1)]);
            line(&[(0.28, -0.85), (0.28, -0.1)]);
            line(&[(-0.7, -0.1), (0.7, -0.1)]);
            line(&[(0.0, -0.1), (0.0, 0.9)]);
        }
        Ic::Clock => {
            p.circle_stroke(c, 0.9 * h, st);
            line(&[(0.0, -0.5), (0.0, 0.0), (0.4, 0.3)]);
        }
        Ic::Refresh => {
            // дуга ~300° і стрілка на кінці
            let pts: Vec<(f32, f32)> = (0..=16).map(|i| {
                let a = -0.6 + 5.3 * i as f32 / 16.0;
                (0.75 * a.cos(), 0.75 * a.sin())
            }).collect();
            line(&pts);
            let (ex, ey) = pts[pts.len() - 1];
            line(&[(ex - 0.3, ey - 0.35), (ex, ey), (ex - 0.3, ey + 0.35)]);
        }
        Ic::Key => {
            p.circle_stroke(v(-0.4, 0.4), 0.42 * h, st);
            line(&[(-0.1, 0.1), (0.8, -0.8)]);
            line(&[(0.4, -0.4), (0.7, -0.1)]);
        }
        Ic::Db => {
            let ell = |cy: f32, from: f32, to: f32| -> Vec<(f32, f32)> {
                (0..=14).map(|i| {
                    let a = from + (to - from) * i as f32 / 14.0;
                    (0.8 * a.cos(), cy + 0.3 * a.sin())
                }).collect()
            };
            line(&ell(-0.55, 0.0, std::f32::consts::TAU));
            line(&[(-0.8, -0.55), (-0.8, 0.55)]);
            line(&[(0.8, -0.55), (0.8, 0.55)]);
            line(&ell(0.55, 0.0, std::f32::consts::PI));
            line(&ell(0.0, 0.0, std::f32::consts::PI));
        }
        Ic::Shield => {
            line(&[(0.0, -0.9), (0.8, -0.6), (0.8, 0.1), (0.0, 0.9), (-0.8, 0.1), (-0.8, -0.6), (0.0, -0.9)]);
        }
        Ic::Grid => {
            for (dx, dy) in [(-0.48, -0.48), (0.48, -0.48), (-0.48, 0.48), (0.48, 0.48)] {
                p.rect_filled(egui::Rect::from_center_size(v(dx, dy), egui::vec2(0.78 * h, 0.78 * h)), 2, col);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        shots::tick(self, &ctx.clone());
        self.poll();
        if must_ask_close(ctx.input(|i| i.viewport().close_requested()), self.pending, self.close_ok) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.pending == 0 {
            self.confirm_close = false;
        }
        if self.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if !self.upd_started {
            self.upd_started = true;
            // не частіше за UPDATE_EVERY (6 годин); ручна кнопка в налаштуваннях працює завжди
            if update_due(now_secs(), self.cfg.update_checked, false) {
                self.spawn(ctx, || Msg::Update(check_update(), false));
            }
        }
        if !self.centered {
            // 72% ширини і 77% висоти монітора (розмір, до якого звик користувач), але не менше мінімуму; центр = (монітор − вікно) / 2. Чекаємо, поки egui знає монітор.
            if let Some(mon) = ctx.input(|i| i.viewport().monitor_size) {
                let win = egui::vec2((mon.x * 0.72).max(760.0), (mon.y * 0.77).max(480.0));
                let pos = egui::pos2(((mon.x - win.x) / 2.0).max(0.0), ((mon.y - win.y) / 2.0).max(0.0));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(win));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                self.centered = true;
            }
        }
        if self.mc_all.is_empty() {
            if !self.mc_loading && self.mc_next.is_none_or(|t| std::time::Instant::now() >= t) {
                self.mc_loading = true;
                self.spawn(ctx, || Msg::McList(game_versions()));
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(30));
        }
        // Ctrl+1/2/3/4 = Проєкти, Публікація, Історія, Налаштування: як меню зверху вниз
        for (k, t) in [(egui::Key::Num1, Some(Tab::Projects)), (egui::Key::Num2, Some(Tab::Publish)), (egui::Key::Num3, None), (egui::Key::Num4, Some(Tab::Settings))] {
            // за модальним діалогом розділи не перемикаємо
            if !self.add_open && self.merge_ask.is_empty() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, k)) {
                match t {
                    Some(t) => {
                        self.tab = t;
                        if t == Tab::Publish && self.pub_step == 6 { self.pub_step = 5; } // як і клік по «Публікація»: з історії назад до форми
                    }
                    None => self.open_history(),
                }
            }
        }
        if !self.add_open && self.merge_ask.is_empty() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F1)) {
            (self.tab, self.set_sec) = (Tab::Settings, 4); // F1: довідка про гарячі клавіші в «Про застосунок»
        }
        // Ctrl+Enter: з іншого розділу лише відкриває «Публікацію» (щоб побачити підсумок), там уже публікує (якщо форма готова і немає діалогу)
        let enter_fresh = ctx.input(|i| fresh_ctrl_enter(&i.events)); // до consume_key: він прибирає події
        if self.splash.done() && self.pending == 0 && !self.add_open && self.merge_ask.is_empty() && !self.cfg.projects.is_empty() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) && enter_fresh {
            let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
            let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
            let (mr, cf) = (has_mr && self.use_mr, has_cf && self.use_cf);
            // з іншого розділу Ctrl+Enter лише відкриває «Публікацію» (видно підсумок), щоб не відправляти наосліп
            // сторінка історії не «Публікація»: на ній немає ні підсумку, ні підказки «ще раз», тож публікувати звідти наосліп не можна
            let was_publish = self.tab == Tab::Publish && !self.history_page();
            self.tab = Tab::Publish;
            if !was_publish { (self.ctrl_armed, self.pub_step) = (None, if self.pub_step == 6 { 5 } else { self.pub_step }); }
            if was_publish && self.check(mr, cf).is_ok() {
                // публікація на два публічні сайти незворотна, а Ctrl+Enter легко натиснути в полі опису: просимо повторити
                if self.armed() {
                    self.start_publish(ctx, mr, cf);
                } else {
                    self.ctrl_armed = Some((std::time::Instant::now(), self.sel));
                    ctx.request_repaint_after(std::time::Duration::from_millis(2100)); // щоб підказка на кнопці зникла сама
                }
            } // інакше причину видно в нижній панелі
        }
        self.sync_draft(ctx);
        // під час публікації файл не міняємо: повтор для іншої платформи надіслав би інший файл
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|d| d.path().to_path_buf()));
        if let Some(f) = dropped.filter(|_| self.pending == 0) {
            let f2 = f.clone();
            self.spawn(ctx, move || Msg::File(f2.clone(), read_jar(&f2)));
            self.tab = Tab::Publish;
            if self.pub_step == 6 { self.pub_step = 5; } // файл кидають у форму, а не в історію
        }
        egui::Panel::left("rail").exact_size(if ctx.content_rect().width() < 860.0 { RAIL_W - 16.0 } else { RAIL_W }).resizable(false)
            .frame(egui::Frame::new().fill(Color32::from_rgb(0x0c, 0x0c, 0x0e)).stroke(Stroke::new(1.0, Color32::from_rgb(0x1a, 0x1a, 0x1c))).inner_margin(egui::Margin::symmetric(12, 14)))
            .show(root, |ui| self.rail(ui));
        // Нижня панель займає місце зараз (висота — з минулого кадру), а її віджети створюються ПІСЛЯ форми:
        // egui веде Tab за порядком створення віджетів, тож форма фокусується раніше за кнопку «Опублікувати».
        let bar_id = egui::Id::NULL.with("publish_bar_h");
        let rail_w = if ctx.content_rect().width() < 860.0 { RAIL_W - 16.0 } else { RAIL_W };
        let side = ((ctx.content_rect().width() - rail_w - CONTENT_MAX_W) / 2.0).max(24.0).floor();
        let pad_y = if ctx.content_rect().height() < 600.0 { 6.0 } else { 10.0 }; // низьке вікно: панель ≤30% висоти
        let bar_rect = (self.tab == Tab::Publish && !self.cfg.projects.is_empty() && !pub_side(ctx) && !self.history_page()).then(|| {
            let h = ctx.data_mut(|d| d.get_temp::<f32>(bar_id)).unwrap_or(64.0);
            egui::Panel::bottom("publish_bar")
                .frame(egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(24, pad_y as i8)))
                .exact_size(h)
                .show(root, |_| ())
                .response
                .rect
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(24))).show(root, |ui| {
            // широке вікно: колонка контенту по центру (поле Margin — i8, тому звужуємо вручну)
            let inner = ui.max_rect().shrink2(egui::vec2(side - 24.0, 0.0));
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            let ui = &mut ui;
            // м'яка поява вмісту при зміні розділу (150 мс)
            let cur = self.tab as u8;
            let (epoch, switched) = ctx.data_mut(|d| {
                let e = d.get_temp_mut_or_insert_with(egui::Id::NULL.with("tab_epoch"), || (cur, 0u32));
                let sw = e.0 != cur;
                if sw { *e = (cur, e.1 + 1); }
                (e.1, sw)
            });
            let fade_id = egui::Id::NULL.with(("tab_fade", epoch));
            if switched { ctx.animate_value_with_time(fade_id, 0.0, 0.0); } // стартова точка анімації
            ui.set_opacity(ctx.animate_value_with_time(fade_id, 1.0, 0.15));
            self.upd_drawn = false;
            if !(self.tab == Tab::Settings && self.set_sec == 2) { self.update_ui(ui, ctx); }
            self.notice_ui(ui, ctx); match self.tab {
            Tab::Projects => self.projects_ui(ui, ctx),
            Tab::Publish => self.publish_ui(ui, ctx),
            Tab::Settings => self.settings_ui(ui, ctx),
        } });
        if let Some(r) = bar_rect {
            let mut bar = root.new_child(egui::UiBuilder::new().max_rect(r.shrink2(egui::vec2(side, pad_y))).layout(egui::Layout::top_down(egui::Align::Min)));
            bar.set_clip_rect(r);
            self.publish_bar(&mut bar, ctx);
            let h = bar.min_rect().height() + 2.0 * pad_y;
            if (h - r.height()).abs() > 0.5 {
                ctx.data_mut(|d| d.insert_temp(bar_id, h));
                ctx.request_repaint(); // висота змінилась: наступний кадр розкладеться точно
            }
        }
        self.merge_dialog(ctx);
        self.versions_dialog(ctx);
        self.close_dialog(ctx);
        self.update_guard(ctx);
        self.splash.show(ctx, self.brand_rect);
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        if !self.readonly {
            let _ = save_config(&self.cfg);
        }
    }
}

fn app_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

/// Що робити зі слідами оновлення (визначає main до створення App).
static START_ACTION: std::sync::OnceLock<StartAction> = std::sync::OnceLock::new();

fn main() -> eframe::Result {
    // Err (немає прав на теку тощо): стартуємо без блокування, щоб не заважати роботі
    let mut lock = single_instance_lock();
    if std::env::args().any(|a| a == "--updated") {
        // нова копія стартує, поки стара ще закривається: чекаємо до 10 с, поки вона відпустить блокування
        for _ in 0..40 {
            if !matches!(lock, Ok(None)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
            lock = single_instance_lock();
        }
    }
    let _lock = match lock {
        Ok(None) => {
            rfd::MessageDialog::new().set_title("Modwire").set_description(i18n::tr("Modwire уже запущено.")).show();
            return Ok(());
        }
        other => other.ok().flatten(),
    };
    // лише під блокуванням: друга копія не має стерти .new, який хтось завантажує.
    // Після оновлення .old лишається, доки UI не пропрацює STABLE_SECS (тоді маркер update.ok);
    // якщо .old є без маркера, на старті пропонуємо повернути попередню версію.
    let act = current_start_action(std::env::args().any(|a| a == "--updated"));
    if act == StartAction::Clean {
        cleanup_update();
    }
    let _ = START_ACTION.set(act);
    let mut vb = egui::ViewportBuilder::default().with_inner_size([900.0, 580.0]).with_min_inner_size([760.0, 480.0]);
    if let Some(i) = app_icon() {
        vb = vb.with_icon(i);
    }
    // Не відновлювати позицію з минулого запуску (вона могла стояти за краєм екрана) і центрувати вікно.
    let opts = eframe::NativeOptions {
        viewport: vb,
        persist_window: false,
        centered: true,
        ..Default::default()
    };
    eframe::run_native("Modwire", opts, Box::new(|cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        setup_style(&cc.egui_ctx);
        Ok(Box::new(App::new()))
    }))
}

#[cfg(test)]
mod tests {
    use super::{checked_msg, guarded, guarded_opt, must_ask_close, setup_style, speed_text, split_url, App, Chk, Msg, Project, StartAction, Tab, P};
    use eframe::egui;

    #[test]
    fn split_url_separates_link() {
        assert_eq!(split_url("Готово https://modrinth.com/mod/x/version/1"), ("Готово".into(), Some("https://modrinth.com/mod/x/version/1".into())));
        assert_eq!(split_url("без посилання"), ("без посилання".into(), None));
    }

    #[test]
    fn progress_and_checked_text() {
        assert_eq!(speed_text(2_621_440, 8), " · 2.5 МБ/с · ще ~8 с");
        assert!(matches!(checked_msg(0), Chk::Idle));
        assert!(matches!(checked_msg(super::now_secs()), Chk::Stamp(t) if super::ago(t).contains("щойно")));
    }

    #[test]
    fn ctrl_enter_needs_second_press_for_same_project_within_2s() {
        let (_g, mut app) = test_app("armed");
        let now = std::time::Instant::now();
        assert!(!app.armed());
        app.sel = Some(0);
        app.ctrl_armed = Some((now, Some(0)));
        assert!(app.armed());
        app.sel = Some(1); // інший проєкт: перше натискання не рахується
        assert!(!app.armed());
        app.sel = Some(0);
        app.ctrl_armed = now.checked_sub(std::time::Duration::from_secs(3)).map(|t| (t, Some(0)));
        assert!(!app.armed(), "через 2 с запобіжник знімається");
    }

    #[test]
    fn review_starts_only_from_a_picked_file_never_from_typing() {
        let (_g, mut app) = test_app("review");
        app.sel = Some(0);
        assert!(app.edit_mode, "за замовчуванням покроковий майстер");
        let ctx = egui::Context::default();
        setup_style(&ctx);
        let draw = |app: &mut App| {
            frame(&ctx, vec![], &mut |ui| {
                let c = ui.ctx().clone();
                page(app, ui, &c, Tab::Publish);
            });
        };
        let all_done = |app: &App| [0, 1, 2, 3, 5].iter().all(|&i| app.steps_done()[i]);
        // файл вибрано, але номера версії й сумісності ще немає: форма неповна, тож майстер
        app.set_file(std::path::PathBuf::from("a.jar"), None);
        draw(&mut app);
        assert!(app.edit_mode && !all_done(&app), "неповна форма лишається в майстрі");
        // користувач дописав останнє обов'язкове поле: форма повна, але зведення не перехоплює її посеред набору
        app.number = "1".into();
        app.cfg.last.mc_versions = "1.20.1".into();
        app.cfg.last.loaders = vec!["fabric".into()];
        draw(&mut app);
        assert!(all_done(&app), "умова тесту: форма повна");
        assert!(app.edit_mode, "набір до кінця не вмикає зведення");
        // вибір файлу при повній формі: зведення
        app.set_file(std::path::PathBuf::from("b.jar"), None);
        draw(&mut app);
        assert!(!app.edit_mode, "вибір файлу при повній формі вмикає зведення");
    }

    #[test]
    fn token_errors_get_a_plain_headline_with_raw_text_behind_details() {
        // справжній формат API: порада простою мовою, потім « (HTTP код: тіло)»
        let (h, token, long) = super::error_headline("CurseForge", "Немає доступу до проєкту. Перевірте токен і ID проєкту (HTTP 403 Forbidden: access denied)");
        assert!(token && long && h.starts_with("CurseForge: Немає доступу") && !h.contains("403") && !h.contains("access denied"), "{h}");
        let (_, token, _) = super::error_headline("Modrinth", "Токен недійсний (HTTP 401 Unauthorized: x)");
        assert!(token, "401 теж про токен");
        // «HTTP 401» у тілі відповіді 500 не робить помилку токенною
        let (_, token, _) = super::error_headline("Modrinth", "Помилка на боці сервера (HTTP 500 Internal: тіло містить (HTTP 401)");
        assert!(!token);
        let (h, token, long) = super::error_headline("Modrinth", "Файл завеликий");
        assert!(!token && !long && h.contains("Файл завеликий"), "{h}");
        assert!(!super::error_headline("Modrinth", "").0.is_empty());
    }

    #[test]
    fn history_is_its_own_page_and_token_warning_is_per_platform() {
        let (_g, mut app) = test_app("histpage");
        app.sel = Some(0);
        app.open_history();
        assert!(!app.history_page(), "порожня історія сторінки не відкриває");
        app.history = vec![serde_json::json!({"ok": true, "project": "a", "number": "1.0", "platform": "Modrinth", "t": 1, "msg": ""})];
        app.open_history();
        assert!(app.history_page() && app.pub_step == 6);
        app.tab = Tab::Projects;
        assert!(!app.history_page());
        // «токен не перевірено» зникає лише для платформи, куди вже була спроба відправки
        (app.cfg.modrinth_checked, app.cfg.curse_checked) = (0, 0);
        assert!(app.pre_warnings(true, true).iter().any(|w| w.contains("Modrinth і CurseForge")));
        app.mr_res = Some(Ok("ok".into()));
        let w = app.pre_warnings(true, true);
        assert!(w.iter().any(|w| w.contains("CurseForge") && !w.contains("Modrinth")), "{w:?}");
        // невдала спроба теж знімає попередження для цієї платформи (результат помилки вже показано)
        app.mr_res = Some(Err("x".into()));
        assert!(app.pre_warnings(true, false).is_empty(), "Modrinth уже пробували, CurseForge вимкнено");
        // без запису про відправку немає сторінки історії: під час публікації її не відкривають
        app.mr_res = None;
        app.pending = 1;
        app.open_history();
        assert!(!app.history_page(), "під час відправки історія недоступна");
    }

    #[test]
    fn held_ctrl_enter_repeat_is_not_a_second_press() {
        let key = |repeat| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat, modifiers: egui::Modifiers::COMMAND };
        assert!(super::fresh_ctrl_enter(&[key(false)]));
        assert!(!super::fresh_ctrl_enter(&[key(true)]), "автоповтор не рахується");
        let plain = egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        assert!(!super::fresh_ctrl_enter(&[plain]), "Enter без Ctrl не рахується");
    }

    #[test]
    fn background_panic_becomes_message() {
        // аварія потоку не має лишати прапорці «зайнято»: замість мовчання приходить Msg::Panicked
        assert!(matches!(guarded(|| panic!("boom")), Msg::Panicked));
        assert!(matches!(guarded_opt(|| panic!("boom")), Some(Msg::Panicked)));
        assert!(matches!(guarded(|| Msg::Notice("ok".into())), Msg::Notice(_)));
        assert!(guarded_opt(|| None).is_none());
    }

    #[test]
    fn close_needs_confirmation_only_while_publishing() {
        assert!(must_ask_close(true, 1, false));
        assert!(!must_ask_close(true, 0, false), "без публікації закриваємо одразу");
        assert!(!must_ask_close(true, 2, true), "вихід уже підтверджено");
        assert!(!must_ask_close(false, 2, false));
    }

    /// App на тимчасовій теці даних (реальний конфіг не чіпаємо). Замок тримається до кінця тесту.
    fn test_app(tag: &str) -> (std::sync::MutexGuard<'static, ()>, App) {
        let g = crate::logic::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = std::env::temp_dir().join(format!("mp_app_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        // shortcut: set_var глобальний; усі тести з env беруть ENV_LOCK
        unsafe { std::env::set_var("MODWIRE_CONFIG_DIR", &d) };
        let mut app = App::new();
        app.cfg.projects = ["a", "b", "c"].map(|n| Project { name: n.into(), id: format!("id-{n}"), modrinth_id: format!("m{n}"), ..Default::default() }).to_vec();
        (g, app)
    }

    #[test]
    fn remove_selected_project_resets_form_and_never_leaks_draft() {
        let (_g, mut app) = test_app("rm_sel");
        app.sel = Some(1);
        (app.number, app.changelog, app.cur_idx) = ("7.7".into(), "текст".into(), Some(1));
        app.remove_project(1);
        assert_eq!(app.sel, None);
        assert!(app.cfg.projects.iter().all(|p| p.draft.number.is_empty() && p.draft.changelog.is_empty()), "форма видаленого не потрапляє в чужу чернетку");
        assert!(app.changelog.is_empty() && app.cur_idx.is_none());
        assert_eq!(app.cfg.projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["a", "c"]);
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    #[test]
    fn remove_earlier_project_shifts_indexes_and_keeps_input() {
        let (_g, mut app) = test_app("rm_shift");
        app.sel = Some(2);
        (app.number, app.target, app.cur_idx) = ("3.1".into(), Some(2), Some(2));
        app.remove_project(0);
        assert_eq!((app.sel, app.target, app.cur_idx), (Some(1), Some(1), Some(1)));
        assert_eq!(app.number, "3.1", "введене не губиться");
        app.remove_project(1); // видаляємо обраний
        assert_eq!(app.sel, None);
        assert_eq!(app.cfg.last.project, usize::MAX, "маркер «немає проєкту», а не чужий індекс");
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    #[test]
    fn select_project_stashes_and_restores_draft() {
        let (_g, mut app) = test_app("sel");
        app.select_project(Some(0));
        (app.number, app.changelog) = ("9.9".into(), "чернетка".into());
        app.select_project(Some(1));
        assert_eq!(app.cfg.projects[0].draft.number, "9.9");
        assert!(app.changelog.is_empty(), "changelog не переходить між проєктами");
        app.select_project(Some(0));
        assert_eq!((app.number.as_str(), app.changelog.as_str()), ("9.9", "чернетка"));
        app.select_project(Some(0)); // повторний вибір нічого не міняє
        assert_eq!(app.number, "9.9");
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    #[test]
    fn panic_message_resets_busy_flags() {
        let (_g, mut app) = test_app("panic");
        (app.mc_loading, app.mr_loading, app.upd_checking, app.upd_applying, app.refreshing, app.adding) = (true, true, true, true, true, 3);
        (app.check_msg, app.cf_msg) = (Chk::Checking, Chk::Checking);
        app.key_msg = Chk::Ok("було".into());
        app.tx.send(Msg::Panicked).unwrap();
        app.poll();
        assert!(!(app.mc_loading || app.mr_loading || app.upd_checking || app.upd_applying || app.refreshing) && app.adding == 0);
        assert!(matches!(app.check_msg, Chk::Err(_)) && matches!(app.cf_msg, Chk::Err(_)), "«Перевіряємо…» не залипає");
        assert!(matches!(app.key_msg, Chk::Ok(_)), "завершений статус не чіпаємо");
        assert!(app.mc_next.is_some() && !app.notice.is_empty());
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    #[test]
    fn tokens_never_reach_ui_or_history_messages() {
        let (_g, mut app) = test_app("mask");
        app.cfg.modrinth_token = " mrp_TOPSECRET_12345 ".into();
        app.cfg.curse_core_key = "core-KEY-abcdef".into();
        app.tx.send(Msg::Check(Err("bad: mrp_TOPSECRET_12345".into()))).unwrap();
        app.tx.send(Msg::CheckKey(Err("echo core-KEY-abcdef".into()))).unwrap();
        app.tx.send(Msg::Done(P::Mr, Err("401 Authorization: mrp_TOPSECRET_12345".into()))).unwrap();
        app.poll();
        let shown = format!("{:?}{:?}", app.check_msg, app.key_msg);
        assert!(!shown.contains("TOPSECRET") && !shown.contains("abcdef"), "{shown}");
        assert!(matches!(&app.mr_res, Some(Err(e)) if !e.contains("TOPSECRET") && e.contains("•••")));
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    #[test]
    fn stale_or_repeated_delete_confirmation_is_harmless() {
        let (_g, mut app) = test_app("stale_del");
        app.confirm_del = Some(1);
        app.remove_project(1);
        app.remove_project(1); // подвійне підтвердження: тепер це «c», видаляється один раз на запит
        assert_eq!(app.cfg.projects.len(), 1);
        app.remove_project(0);
        app.remove_project(0);
        app.remove_project(usize::MAX);
        assert!(app.cfg.projects.is_empty() && app.confirm_del.is_none());
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    /// Один кадр інтерфейсу без вікна: (повна відповідь egui, id віджета у фокусі).
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, f: &mut dyn FnMut(&mut egui::Ui)) -> (egui::FullOutput, Option<egui::Id>) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 580.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| f(ui));
        out.textures_delta.clear(); // без рендера дельти не застосовуються
        (out, ctx.memory(|m| m.focused()))
    }
    fn tab_key() -> Vec<egui::Event> {
        vec![egui::Event::Key { key: egui::Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]
    }

    /// Діалог показує App; проходимо Tab 14 разів: фокус не виходить за діалог (шар Foreground), а прямо після відкриття вже стоїть у діалозі або порядок іде зверху вниз.
    fn tab_walk(app: &mut App, show: &mut dyn FnMut(&mut App, &egui::Context)) -> Vec<(egui::Order, f32, f32)> {
        let ctx = egui::Context::default();
        setup_style(&ctx);
        let mut run = |app: &mut App, ev: Vec<egui::Event>| {
            let (_, f) = frame(&ctx, ev, &mut |ui| {
                let c = ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |ui| { app.projects_ui(ui, &c); });
                show(app, &c);
            });
            f
        };
        let mut seen = vec![];
        // фокус одразу після відкриття (до першого Tab) іде першим елементом
        for _ in 0..3 {
            if let Some(id) = run(app, vec![]) && let Some(r) = ctx.read_response(id) {
                seen = vec![(r.layer_id.order, r.rect.top(), r.rect.left())];
            }
        }
        for _ in 0..14 {
            if let Some(id) = run(app, tab_key()) && let Some(r) = ctx.read_response(id) {
                seen.push((r.layer_id.order, r.rect.top(), r.rect.left()));
            }
        }
        seen
    }

    #[test]
    fn tab_stays_inside_dialogs() {
        let (_g, mut app) = test_app("tabdlg");
        app.add_open = true;
        let seen = tab_walk(&mut app, &mut |a, c| a.add_dialog(c));
        eprintln!("add_dialog: {seen:?}");
        assert!(!seen.is_empty() && seen.iter().all(|s| s.0 == egui::Order::Foreground), "Tab вийшов з діалогу «Додати проєкт»: {seen:?}");
        app.add_open = false;
        app.confirm_close = true;
        let seen = tab_walk(&mut app, &mut |a, c| a.close_dialog(c));
        eprintln!("close_dialog: {seen:?}");
        assert!(seen[0].2 > 400.0, "у діалозі закриття спершу в фокусі «Лишитись»: {seen:?}");
        assert!(!seen.is_empty() && seen.iter().all(|s| s.0 == egui::Order::Foreground), "{seen:?}");
        app.confirm_close = false;
        app.merge_ask = vec![(0, app.cfg.projects[1].clone())];
        let seen = tab_walk(&mut app, &mut |a, c| a.merge_dialog(c));
        eprintln!("merge_dialog: {seen:?}");
        assert!(!seen.is_empty() && seen.iter().all(|s| s.0 == egui::Order::Foreground), "{seen:?}");
        app.merge_ask.clear();
        app.start_act = StartAction::OfferRollback;
        let seen = tab_walk(&mut app, &mut |a, c| a.update_guard(c));
        eprintln!("rollback_dialog: {seen:?}");
        assert!(!seen.is_empty() && seen.iter().all(|s| s.0 == egui::Order::Foreground), "{seen:?}");
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    /// Крок «Сумісність»: тип файлу, кнопка «Обрати версії…» і вікно вибору працюють без паніки й змінюють дані.
    #[test]
    fn compat_step_type_and_version_picker() {
        use egui::accesskit::Role;
        let (_g, mut app) = test_app("compat");
        app.tab = Tab::Publish;
        app.sel = Some(0);
        (app.pub_step, app.edit_mode) = (3, true);
        app.mc_all = vec!["1.20.4".into(), "1.20.1".into(), "24w14a".into()];
        app.mc_known = vec!["1.20.4".into(), "1.20.1".into()];
        app.cfg.last.mc_versions = "1.20.1".into();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        setup_style(&ctx);
        let run = |app: &mut App, ev: Vec<egui::Event>| {
            let (out, _) = frame(&ctx, ev, &mut |ui| {
                let c = ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |ui| app.publish_ui(ui, &c));
                app.versions_dialog(&c);
            });
            let mut found = std::collections::HashMap::new();
            if let Some(u) = out.platform_output.accesskit_update {
                for (_, n) in &u.nodes {
                    if n.role() == Role::Button && let (Some(l), Some(b)) = (n.label(), n.bounds()) {
                        found.insert(l.to_string(), egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32));
                    }
                }
            }
            found
        };
        let click = |pos: egui::Pos2| -> Vec<Vec<egui::Event>> {
            let pb = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            vec![vec![egui::Event::PointerMoved(pos)], vec![pb(true)], vec![pb(false)], vec![]]
        };
        let mut btns = Default::default();
        for _ in 0..4 { btns = run(&mut app, vec![]); }
        // тип «Ресурспак» ставить маркер-лоадер і прибирає лоадери модів
        app.cfg.last.loaders = vec!["fabric".into()];
        let pos = *btns.iter().find(|(k, _)| k.starts_with("Ресурс-пак")).map(|(_, v)| v).expect("картка «Ресурс-пак»");
        for ev in click(pos) { run(&mut app, ev); }
        assert_eq!(app.pack_kind(), 1, "після кліку по картці — ресурс-пак");
        assert_eq!(app.cfg.last.loaders, vec!["minecraft".to_string()], "лоадери модів прибрано");
        // кнопка вибору версій відкриває вікно
        let pos = *btns.get("Обрати версії…").expect("кнопка «Обрати версії…»");
        for ev in click(pos) { run(&mut app, ev); }
        assert!(app.ver_open, "вікно вибору версій відкрилось");
        let btns = run(&mut app, vec![]);
        let pos = *btns.get("1.20.4").expect("рядок версії 1.20.4 у вікні");
        for ev in click(pos) { run(&mut app, ev); }
        assert!(app.cfg.last.mc_versions.contains("1.20.4") && app.cfg.last.mc_versions.contains("1.20.1"), "{}", app.cfg.last.mc_versions);
        let btns = run(&mut app, vec![]);
        let pos = *btns.get("Готово").expect("кнопка «Готово»");
        for ev in click(pos) { run(&mut app, ev); }
        assert!(!app.ver_open, "«Готово» закриває вікно");
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    #[test]
    fn version_groups_and_pack_kind() {
        assert_eq!(crate::mc_group("1.20.4"), "1.20");
        assert_eq!(crate::mc_group("1.21"), "1.21");
        assert_eq!(crate::mc_group("1.9.4"), "1.9");
        assert_eq!(crate::mc_group("24w14a"), "Знімки");
        assert_eq!(crate::mc_group("1.20-pre1"), "Знімки");
        let (_g, mut app) = test_app("packkind");
        app.cfg.last.loaders = vec!["fabric".into()];
        assert_eq!(app.pack_kind(), 0);
        app.cfg.last.loaders = vec!["minecraft".into()];
        assert_eq!(app.pack_kind(), 1);
        app.cfg.last.loaders = vec!["datapack".into()];
        assert_eq!(app.pack_kind(), 2);
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    /// У простої (без публікації/оновлення/завантаження) жоден розділ не просить кадрів частіше, ніж раз на секунду.
    #[test]
    fn idle_ui_requests_no_frames() {
        let (_g, mut app) = test_app("idle");
        app.sel = Some(0);
        for tab in [Tab::Projects, Tab::Publish, Tab::Settings] {
            app.tab = tab;
            let ctx = egui::Context::default();
            setup_style(&ctx);
            let mut delays = vec![];
            for _ in 0..30 {
                let (out, _) = frame(&ctx, vec![], &mut |ui| {
                    let c = ui.ctx().clone();
                    egui::CentralPanel::default().show(ui, |ui| {
                        match tab {
                            Tab::Projects => app.projects_ui(ui, &c),
                            Tab::Publish => app.publish_ui(ui, &c),
                            Tab::Settings => app.settings_ui(ui, &c),
                        }
                    });
                    if tab == Tab::Publish {
                        app.publish_bar(ui, &c);
                    }
                });
                delays.push(out.viewport_output.values().map(|v| v.repaint_delay).min().unwrap_or_default());
            }
            let last = *delays.last().unwrap();
            eprintln!("{:?}: repaint_delay у простої = {last:?}", tab as u8);
            assert!(last >= std::time::Duration::from_secs(1), "{:?} просить кадр кожні {last:?}", tab as u8);
        }
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    /// Дерево accesskit за кілька кадрів: (роль, назва) кожного інтерактивного вузла без назви.
    fn unnamed_nodes(app: &mut App, show: &mut dyn FnMut(&mut App, &mut egui::Ui, &egui::Context)) -> Vec<String> {
        use egui::accesskit::Role;
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        setup_style(&ctx);
        let mut nodes = std::collections::HashMap::new();
        for _ in 0..4 {
            let (out, _) = frame(&ctx, vec![], &mut |ui| {
                let c = ui.ctx().clone();
                show(app, ui, &c);
            });
            if let Some(u) = out.platform_output.accesskit_update {
                nodes.extend(u.nodes);
            }
        }
        let mut bad = vec![];
        let mut names: Vec<String> = nodes.values().filter(|n| matches!(n.role(), Role::Button | Role::CheckBox | Role::TextInput | Role::MultilineTextInput | Role::ComboBox)).map(|n| format!("{:?}: {}", n.role(), n.label().or(n.value()).unwrap_or(""))).collect();
        names.sort();
        eprintln!("{}", names.join(" | "));
        for (id, n) in &nodes {
            if !matches!(n.role(), Role::Button | Role::CheckBox | Role::TextInput | Role::MultilineTextInput | Role::ComboBox | Role::Link | Role::RadioButton | Role::Switch) {
                continue;
            }
            let named = n.label().is_some_and(|l| !l.trim().is_empty())
                || n.value().is_some_and(|l| !l.trim().is_empty())
                || n.description().is_some_and(|l| !l.trim().is_empty())
                || n.labelled_by().iter().any(|l| nodes.get(l).is_some_and(|ln| ln.label().or(ln.value()).is_some_and(|t| !t.trim().is_empty())));
            if !named {
                bad.push(format!("{:?} {:?} (id {:?})", n.role(), n.label(), id));
            }
        }
        assert!(nodes.values().any(|n| n.role() == Role::Button), "дерево accesskit порожнє: enable_accesskit не спрацював");
        bad
    }

    fn page(app: &mut App, ui: &mut egui::Ui, c: &egui::Context, tab: Tab) {
        egui::CentralPanel::default().show(ui, |ui| match tab {
            Tab::Projects => app.projects_ui(ui, c),
            Tab::Publish => app.publish_ui(ui, c),
            Tab::Settings => app.settings_ui(ui, c),
        });
    }

    #[test]
    fn every_interactive_node_has_a_name() {
        let (_g, mut app) = test_app("a11y");
        app.sel = Some(0);
        app.history = vec![
            serde_json::json!({"ok": true, "project": "a", "number": "1.0", "platform": "Modrinth", "t": 1, "msg": "https://modrinth.com/mod/a/version/1"}),
            serde_json::json!({"ok": false, "project": "a", "number": "1.1", "platform": "CurseForge", "t": 2, "msg": "403"}),
        ];
        app.mr_res = Some(Ok("Опубліковано: https://modrinth.com/mod/a/version/1".into()));
        app.cf_res = Some(Err("Сервер відповів HTTP 403. Перевірте токен".into()));
        let mut bad = vec![];
        for tab in [Tab::Projects, Tab::Publish, Tab::Settings] {
            for sel_mode in [false, true] {
                app.sel_mode = sel_mode;
                app.confirm_del = (tab == Tab::Projects && !sel_mode).then_some(0);
                let b = unnamed_nodes(&mut app, &mut |a, ui, c| {
                    a.rail(ui);
                    page(a, ui, c, tab);
                    if tab == Tab::Publish {
                        a.publish_bar(ui, c);
                    }
                });
                bad.extend(b.into_iter().map(|x| format!("{:?}/sel={sel_mode}: {x}", tab as u8)));
            }
        }
        app.sel_mode = false;
        app.confirm_del = None;
        app.add_open = true;
        bad.extend(unnamed_nodes(&mut app, &mut |a, ui, c| { page(a, ui, c, Tab::Projects); a.add_dialog(c); }).into_iter().map(|x| format!("add: {x}")));
        app.add_open = false;
        app.confirm_close = true;
        bad.extend(unnamed_nodes(&mut app, &mut |a, ui, c| { page(a, ui, c, Tab::Projects); a.close_dialog(c); }).into_iter().map(|x| format!("close: {x}")));
        app.confirm_close = false;
        app.merge_ask = vec![(0, app.cfg.projects[1].clone())];
        bad.extend(unnamed_nodes(&mut app, &mut |a, ui, c| { page(a, ui, c, Tab::Projects); a.merge_dialog(c); }).into_iter().map(|x| format!("merge: {x}")));
        app.merge_ask.clear();
        app.start_act = StartAction::OfferRollback;
        bad.extend(unnamed_nodes(&mut app, &mut |a, ui, c| { page(a, ui, c, Tab::Projects); a.update_guard(c); }).into_iter().map(|x| format!("rollback: {x}")));
        app.start_act = StartAction::Clean;
        app.cfg.projects.clear();
        bad.extend(unnamed_nodes(&mut app, &mut |a, ui, c| { a.rail(ui); page(a, ui, c, Tab::Projects); }).into_iter().map(|x| format!("empty: {x}")));
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
        assert!(bad.is_empty(), "вузли без назви:
{}", bad.join("
"));
    }

    /// Послідовність Tab: (назва з accesskit, верх, ліво) кожного елемента у фокусі; перший елемент — фокус до першого Tab.
    /// Кожен виклик має власний Context: без спільного стану між тестами.
    fn tab_order(app: &mut App, presses: usize, show: &mut dyn FnMut(&mut App, &mut egui::Ui, &egui::Context)) -> Vec<(String, f32, f32)> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        setup_style(&ctx);
        let mut nodes = std::collections::HashMap::new();
        let mut run = |app: &mut App, ev: Vec<egui::Event>| {
            let (out, f) = frame(&ctx, ev, &mut |ui| {
                let c = ui.ctx().clone();
                show(app, ui, &c);
            });
            if let Some(u) = out.platform_output.accesskit_update {
                nodes.extend(u.nodes);
            }
            f
        };
        let mut ids = vec![];
        for _ in 0..3 {
            if let Some(id) = run(app, vec![]) {
                ids = vec![id];
            }
        }
        for _ in 0..presses {
            if let Some(id) = run(app, tab_key()) {
                ids.push(id);
            }
        }
        ids.into_iter()
            .filter_map(|id| {
                let r = ctx.read_response(id)?;
                let n = nodes.get(&id.accesskit_id());
                let named = |n: &egui::accesskit::Node| n.label().or(n.value()).filter(|t| !t.is_empty()).map(str::to_string);
                let name = n.and_then(named).or_else(|| n?.labelled_by().iter().find_map(|l| nodes.get(l).and_then(named))).unwrap_or_default();
                Some((name, r.rect.top(), r.rect.left()))
            })
            .collect()
    }

    #[test]
    fn tab_order_projects_and_publish_is_visual() {
        let (_g, mut app) = test_app("taborder");
        app.sel = Some(0);
        for tab in [Tab::Projects, Tab::Publish] {
            app.tab = tab;
            let seq = tab_order(&mut app, 40, &mut |a, ui, c| {
                a.rail(ui);
                page(a, ui, c, tab);
                if tab == Tab::Publish {
                    a.publish_bar(ui, c);
                }
            });
            eprintln!("{:?}: {}", tab as u8, seq.iter().map(|s| format!("{}@{:.0},{:.0}", s.0, s.1, s.2)).collect::<Vec<_>>().join(" > "));
            assert!(seq.len() > 8, "{:?}: фокус не рухається ({} елементів)", tab as u8, seq.len());
            assert!(seq.iter().all(|s| !s.0.is_empty()), "{:?}: елемент у фокусі без назви: {seq:?}", tab as u8);
            // після рейки (3 пункти) форма йде зверху вниз; нижня панель публікації — завжди останньою перед колом
            let cycle = seq[1..].iter().position(|s| s.0 == seq[0].0).map_or(seq.len(), |i| i + 1);
            let end = if tab == Tab::Publish { cycle - 1 } else { cycle };
            let tops: Vec<f32> = seq[3..end].iter().map(|s| s.1).collect();
            assert!(tops.windows(2).all(|w| w[0] <= w[1] + 1.0), "{:?}: порядок Tab не збігається з візуальним: {seq:?}", tab as u8);
        }
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    #[test]
    fn focus_returns_to_opener_after_dialog_closes() {
        let (_g, mut app) = test_app("focusret");
        app.tab = Tab::Projects;
        let ctx = egui::Context::default();
        setup_style(&ctx);
        let run = |app: &mut App, ev: Vec<egui::Event>| {
            frame(&ctx, ev, &mut |ui| {
                let c = ui.ctx().clone();
                page(app, ui, &c, Tab::Projects);
                app.add_dialog(&c);
            })
            .1
        };
        for _ in 0..2 { run(&mut app, vec![]); }
        let opener = (0..3).find_map(|_| run(&mut app, tab_key())).expect("є що фокусувати на сторінці");
        app.add_open = true;
        for _ in 0..4 { run(&mut app, vec![]); }
        assert_ne!(run(&mut app, vec![]), Some(opener), "у діалозі фокус має бути всередині діалогу");
        app.add_open = false;
        for _ in 0..3 { run(&mut app, vec![]); }
        assert_eq!(run(&mut app, vec![]), Some(opener), "після закриття діалогу фокус повертається на елемент, що його відкрив");
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }

    /// Статуси результатів мають слова для скрінрідера: «Успішно»/«Помилка», а не лише значки ✓/✕.
    #[test]
    fn result_status_icons_are_spoken() {
        let (_g, mut app) = test_app("spoken");
        app.sel = Some(0);
        app.mr_res = Some(Ok("Опубліковано: https://modrinth.com/mod/a/version/1".into()));
        app.cf_res = Some(Err("Сервер відповів HTTP 403".into()));
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        setup_style(&ctx);
        let mut labels = std::collections::HashSet::new();
        for _ in 0..3 {
            let (out, _) = frame(&ctx, vec![], &mut |ui| {
                let c = ui.ctx().clone();
                app.publish_bar(ui, &c);
            });
            for (_, n) in out.platform_output.accesskit_update.into_iter().flat_map(|u| u.nodes) {
                labels.extend(n.label().map(str::to_string));
            }
        }
        assert!(labels.contains("Успішно") && labels.contains("Помилка"), "{labels:?}");
        unsafe { std::env::remove_var("MODWIRE_CONFIG_DIR") };
    }
}
