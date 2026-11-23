#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod logic;
mod shots;

use eframe::egui::{self, Color32, RichText, Stroke};
use logic::*;
use std::sync::atomic::Ordering::Relaxed;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

// Палітра: темна, з двома фірмовими акцентами платформ.
const BG: Color32 = Color32::from_rgb(0x14, 0x12, 0x0f);
const SURFACE: Color32 = Color32::from_rgb(0x1d, 0x1a, 0x17);
const FIELD: Color32 = Color32::from_rgb(0x2a, 0x26, 0x22);
const HOVER: Color32 = Color32::from_rgb(0x35, 0x2f, 0x29);
const ACTIVE: Color32 = Color32::from_rgb(0x41, 0x3a, 0x33);
const UI_BORDER: Color32 = Color32::from_rgb(0x7a, 0x70, 0x65);
const BORDER: Color32 = Color32::from_rgb(0x3a, 0x34, 0x2d);
const TEXT: Color32 = Color32::from_rgb(0xf1, 0xeb, 0xe3);
const MUTED: Color32 = Color32::from_rgb(0xa5, 0x9b, 0x8e);
const ACCENT: Color32 = Color32::from_rgb(0xe8, 0xa5, 0x4b);
const ON_ACCENT: Color32 = Color32::from_rgb(0x15, 0x12, 0x0e);
const LINK: Color32 = Color32::from_rgb(0xf3, 0xc2, 0x7e);
const SUCCESS: Color32 = Color32::from_rgb(0x8d, 0xbf, 0x6a);
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
const CARD_R: u8 = 12; // контейнери
const R_CTRL: u8 = 8; // контроли
const R_BADGE: u8 = 6; // бейджі
const CARD_PAD: i8 = 16; // внутрішній відступ картки
const STEP_DOT: f32 = 24.0; // діаметр кола кроку
const STEP_LINE: f32 = 2.0; // товщина з'єднувача кроків
const PROJECT_CARD_H: f32 = 132.0;
const CARD_MIN_W: f32 = 270.0;
const GAP: f32 = 16.0;
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
    CfInfo(String, Option<usize>, Result<(String, String), String>),
    Done(P, Result<String, String>),
    File(PathBuf),
    Changelog(String),
    Notice(String),
    Imported(Vec<Project>),
    Refreshed(Vec<(String, String, String, String)>),
    Update(Result<Option<Update>, String>, bool), // bool: перевірку запустив користувач
    Updated(Result<(), String>),
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
    merge_ask: Vec<(usize, Project)>, // черга питань про однакову назву
    centered: bool,                  // вікно вже центровано після першого кадру
    add_msg: String,
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
    open_steps: [bool; 6], // розгорнуті кроки публікації (згортає лише користувач)
    merge_hidden: bool, // діалог «Однакова назва» закрито Esc без рішення
    add_shown: bool,
    file_note: String,          // що підставлено з файлу (показується в кроці «Файл»)
    focus_step: Option<usize>,  // крок, до якого треба прокрутити (клік на «Ще не готово»)
    focus_hist: bool,           // прокрутити до історії (кнопка після успішної публікації)
}

/// Стан перевірки токена.
#[derive(Default, Clone, PartialEq)]
enum Chk {
    #[default]
    Idle,
    Checking,
    Ok(String),
    Err(String),
}

/// Стан «перевірено N тому» зі збереженої відмітки часу (0 = не перевірялось).
fn checked_msg(t: u64) -> Chk {
    if t == 0 { Chk::Idle } else { Chk::Ok(format!("Перевірено {}", ago(t))) }
}

fn status(ui: &mut egui::Ui, c: &Chk) {
    match c {
        Chk::Idle => {}
        Chk::Checking => {
            ui.spinner();
            ui.label(RichText::new("Перевірка…").color(MUTED));
        }
        Chk::Ok(m) | Chk::Err(m) => {
            let ok = matches!(c, Chk::Ok(_));
            let r = ui.scope(|ui| if ok { icon_label_c(ui, Ic::Check, SUCCESS, m, SUCCESS) } else { icon_label_c(ui, Ic::Cross, ERROR, m, ERROR) });
            ui.ctx().accesskit_node_builder(r.response.id, |n| n.set_live(egui::accesskit::Live::Polite)); // результат перевірки озвучується
        }
    }
}

/// Відокремлює посилання (слово, що починається з https://) від тексту результату.
fn split_url(s: &str) -> (String, Option<String>) {
    let url = s.split_whitespace().find(|w| w.starts_with("https://")).map(String::from);
    let txt = s.split_whitespace().filter(|w| !w.starts_with("https://")).collect::<Vec<_>>().join(" ");
    (txt, url)
}

/// Діалог вибору файлу змін; бере першу секцію (останню версію).
fn pick_changelog(dir: String) -> Option<Msg> {
    let mut d = rfd::FileDialog::new().add_filter("Зміни", &["md", "txt"]);
    if !dir.is_empty() {
        d = d.set_directory(dir);
    }
    let text = std::fs::read_to_string(d.pick_file()?).ok()?;
    Some(Msg::Changelog(first_section(&text.chars().take(200_000).collect::<String>())))
}

fn pick_file(dir: String) -> Option<Msg> {
    let mut d = rfd::FileDialog::new().add_filter("Мод", &["jar", "zip", "mrpack"]);
    if !dir.is_empty() {
        d = d.set_directory(dir);
    }
    d.pick_file().map(Msg::File)
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
    ctx.set_visuals(v);

    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (egui::TextStyle::Heading, egui::FontId::proportional(26.0)),
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
        s.spacing.interact_size.y = 36.0;
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

/// Заголовок блоку (стиль Title).
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
    let resp = ui.add(egui::Button::new(RichText::new("Як отримати").color(LINK)).frame(false).min_size(egui::vec2(0.0, 28.0)));
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(320.0)
        .show(|ui| {
            ui.label(help);
            if icon_btn(ui, Some(Ic::External), "Відкрити сторінку", true).clicked() {
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
        btn_paint(ui, None, &self.text, BtnKind::Solid(self.fill, self.fg), self.min.x, self.min.y.max(36.0))
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
            _ => p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside),
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
    Solid { text: text.to_owned(), fill: ACCENT, fg: ON_ACCENT, min: egui::vec2(0.0, 36.0) }
}

fn danger(text: &str) -> Solid {
    Solid { text: text.to_owned(), fill: DANGER, fg: Color32::WHITE, min: egui::vec2(0.0, 36.0) }
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
    let r = egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(egui::CornerRadius::same(CARD_R))
        .inner_margin(egui::Margin::same(CARD_PAD))
        .show(ui, f);
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
}

fn step(ui: &mut egui::Ui, spec: StepSpec, open: &mut bool, f: impl FnOnce(&mut egui::Ui)) {
    let StepSpec { n, name, done, required, summary, scroll } = spec;
    if (required && !done) || scroll {
        *open = true; // примусово розгорнутий крок лишається розгорнутим, поки користувач сам не згорне
    }
    if scroll {
        ui.scroll_to_cursor(Some(egui::Align::TOP));
    }
    let expanded = *open;
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        if n > 1 {
            // з'єднувач з попереднім кроком; коло малюється поверх
            let c = ui.cursor().min;
            let gap = ui.spacing().item_spacing.y;
            let x = c.x + STEP_DOT / 2.0;
            ui.painter().line_segment([egui::pos2(x, c.y - CARD_PAD as f32 - gap), egui::pos2(x, c.y + STEP_DOT / 2.0)], Stroke::new(STEP_LINE, UI_BORDER));
        }
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(STEP_DOT, STEP_DOT), egui::Sense::hover());
            if done {
                ui.painter().circle_filled(rect.center(), STEP_DOT / 2.0, SUCCESS);
                draw_icon(ui.painter(), rect.center(), Ic::Check, 12.0, ON_ACCENT);
            } else {
                ui.painter().circle_filled(rect.center(), STEP_DOT / 2.0, ACCENT);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, n.to_string(), egui::FontId::proportional(13.0), ON_ACCENT);
            }
            ui.label(title(name));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !expanded {
                    if ghost_btn(ui, None, "Змінити").clicked() {
                        *open = true;
                    }
                } else if done && ghost_btn(ui, Some(Ic::Chevron), "Згорнути").clicked() {
                    *open = false;
                }
            });
        });
        if expanded {
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

/// Статус токена: «Задано» або «Не вказано».
fn token_badge(ui: &mut egui::Ui, value: &str) {
    if value.trim().is_empty() {
        badge(ui, "Не вказано", MUTED);
    } else {
        badge(ui, "Задано", SUCCESS);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Info,
    Warn,
    Error,
    Ok,
}

/// Єдине повідомлення: смуга 3px зліва, значок і текст; відступи 12/8.
fn banner(ui: &mut egui::Ui, kind: Kind, text: &str) {
    let (col, ic) = match kind {
        Kind::Info => (MUTED, Ic::Info),
        Kind::Warn => (WARN, Ic::Warn),
        Kind::Error => (ERROR, Ic::Cross),
        Kind::Ok => (SUCCESS, Ic::Check),
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
                let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 20.0), egui::Sense::hover());
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
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        draw_icon(ui.painter(), rect.center(), ic, 16.0, col);
        ui.add(egui::Label::new(RichText::new(text).color(TEXT)).truncate());
    });
}

/// Значок + текст заданого кольору.
fn icon_label_c(ui: &mut egui::Ui, ic: Ic, col: Color32, text: &str, text_col: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        draw_icon(ui.painter(), rect.center(), ic, 16.0, col);
        ui.add(egui::Label::new(RichText::new(text).color(text_col)).truncate());
    });
}

/// Рамка модального вікна.
fn modal_frame() -> egui::Frame {
    egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).corner_radius(egui::CornerRadius::same(CARD_R)).inner_margin(egui::Margin::same(24))
}

/// Кнопка-«привид»: без заливки, при наведенні акцент.
fn ghost_btn(ui: &mut egui::Ui, ic: Option<Ic>, label: &str) -> egui::Response {
    btn_paint(ui, ic, label, BtnKind::Ghost, 0.0, 36.0)
}

/// Квадратна кнопка лише зі значком (36x36); підпис — у підказці й для скрінрідерів. `on` — значок бурштиновий.
fn icon_only(ui: &mut egui::Ui, ic: Ic, tip: &str, on: bool) -> egui::Response {
    let enabled = ui.is_enabled();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
    let t = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered() && enabled, 0.12);
    let down = enabled && resp.is_pointer_button_down_on();
    let p = ui.painter();
    let fill = if down { ACTIVE } else { FIELD.lerp_to_gamma(HOVER, t) };
    let border = if enabled { UI_BORDER.lerp_to_gamma(ACCENT.gamma_multiply(0.6), t) } else { BORDER };
    p.rect(rect, R_CTRL, fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    if resp.has_focus() {
        p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
    }
    let col = if !enabled { MUTED.gamma_multiply(0.5) } else if on { ACCENT } else { MUTED.lerp_to_gamma(TEXT, t) };
    draw_icon(p, rect.center(), ic, 16.0, col);
    let resp = resp.on_hover_text(tip);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Кнопка закриття (X) без рамки.
fn close_x(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Закрити"));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, R_CTRL, HOVER);
    }
    if resp.has_focus() {
        p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
    }
    draw_icon(p, rect.center(), Ic::Cross, 14.0, if resp.hovered() { TEXT } else { MUTED });
    resp.on_hover_text("Закрити")
}

/// Помилка поля: рамка ERROR 1.5px і червоний текст під полем.
fn field_error(ui: &mut egui::Ui, resp: &egui::Response, msg: &str) {
    ui.painter().rect_stroke(resp.rect.expand(1.0), R_CTRL, Stroke::new(1.5, ERROR), egui::StrokeKind::Outside);
    ui.label(RichText::new(msg).color(ERROR).size(HINT_SZ));
}

/// Кнопка з намальованим значком; `primary` — акцентна заливка.
fn icon_btn(ui: &mut egui::Ui, ic: Option<Ic>, label: &str, primary: bool) -> egui::Response {
    btn_paint(ui, ic, label, if primary { BtnKind::Solid(ACCENT, ON_ACCENT) } else { BtnKind::Field }, 0.0, 36.0)
}

/// Кнопка-перемикач (лоадер, тип релізу, платформа): активна з галочкою, кольоровою обводкою 2px і заливкою.
fn choice_col(ui: &mut egui::Ui, on: bool, label: &str, col: Color32, alpha: f32) -> egui::Response {
    choice_sz(ui, on, label, col, alpha, 36.0, egui::TextStyle::Button.resolve(ui.style()))
}

/// Чип версії Minecraft: нижчий (28) і дрібніший (13).
fn mc_chip(ui: &mut egui::Ui, on: bool, label: &str) -> egui::Response {
    choice_sz(ui, on, label, ACCENT, 0.20, 28.0, egui::FontId::proportional(13.0))
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
        p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
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
    let mut prep = egui::Frame::new().fill(FIELD).corner_radius(egui::CornerRadius::same(R_CTRL)).inner_margin(egui::Margin::symmetric(8, 0)).begin(ui);
    let mut focused = false;
    {
        let ui = &mut prep.content_ui;
        ui.horizontal(|ui| {
            let w = ui.available_width() - 40.0;
            let te = ui.add(egui::TextEdit::singleline(value).password(!*show).hint_text("Вставте сюди (Ctrl+V)").frame(egui::Frame::NONE).desired_width(w).min_size(egui::vec2(w, 34.0)).align(egui::Align2::LEFT_CENTER));
            changed = te.changed();
            focused = te.has_focus();
            let label = if *show { "Сховати" } else { "Показати" };
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(rect, R_CTRL, HOVER);
            }
            if resp.has_focus() {
                p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
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
    prep.frame.stroke = if focused { Stroke::new(2.0, ACCENT) } else { Stroke::new(1.0, UI_BORDER) };
    prep.end(ui);
    changed
}

fn icon(ui: &mut egui::Ui, url: &str, name: &str, size: f32) {
    // Читаємо байти самі: завантажувач file:// у Windows не знаходив кешовані іконки.
    let cached = icon_path(url).filter(|p| !url.is_empty() && p.exists()).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b)));
    match cached {
        Some((p, bytes)) => {
            let uri = format!("bytes://{}", p.to_string_lossy().replace('\\', "/"));
            ui.add(egui::Image::from_bytes(uri, bytes).fit_to_exact_size(egui::vec2(size, size)).corner_radius(R_CTRL));
        }
        None => {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            ui.painter().rect_filled(rect, R_CTRL, FIELD);
            let c = name.chars().next().unwrap_or('?').to_uppercase().to_string();
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, c, egui::FontId::proportional(size * 0.5), TEXT);
        }
    }
}

impl App {
    fn new() -> Self {
        let (tx, rx) = channel();
        let (cfg, state) = load_config();
        let l = &cfg.last;
        let sel = (l.project < cfg.projects.len()).then_some(l.project);
        let rt = RELEASE_TYPES.iter().position(|r| *r == l.release_type).unwrap_or(0);
        Self {
            tab: Tab::Projects, tx, rx, add_open: false, target: None, mr_list: vec![], mr_loading: false, mr_msg: String::new(),
            cf_id: String::new(), merge_ask: vec![], centered: false, add_msg: String::new(), sel, file: None,
            name: String::new(), number: sel.and_then(|i| cfg.projects.get(i)).map(|p| bump(&p.last_number)).unwrap_or_default(), release_type: rt, changelog: String::new(),
            use_mr: true, use_cf: true, mr_res: None, cf_res: None, pending: 0, msg: String::new(),
            show_tokens: false, confirm_del: None, confirm_clear: false, check_msg: checked_msg(cfg.modrinth_checked), up_mr: Up::default(), up_cf: Up::default(), cur_cl: String::new(), cur_meta: Default::default(),
            notice_seen: None, update: None, upd_checking: false, upd_applying: false, refreshing: false, upd_err: String::new(), upd_started: false, quit: false,
            notice: match state {
                Load::Fine => String::new(),
                Load::Restored => "Конфіг було пошкоджено: налаштування відновлено з резервної копії (config.backup.json), зіпсований файл збережено поруч.".into(),
                Load::Recovered =>"Конфіг було пошкоджено: копію збережено в теці конфігу (файл .bak), налаштування скинуто.".into(),
                Load::ReadOnly => "Не вдалося прочитати конфіг: збереження вимкнено, щоб не перезаписати старі дані. Закрийте програми, що тримають файл, і перезапустіть.".into(),
            },
            readonly: matches!(state, Load::ReadOnly), adding: 0, cf_auto_off: false, cur_idx: None, history: history_load(HIST_MAX), hist_q: String::new(), hist_fail: false, hist_all: false, hist_confirm: false, cur: Default::default(), mc_known: vec![], mc_all: vec![], mc_loading: false, mc_next: None, cf_msg: checked_msg(cfg.curse_checked), key_msg: checked_msg(cfg.core_checked), merge_hidden: false, add_shown: false, file_note: String::new(), focus_step: None, focus_hist: false, open_steps: [true; 6], cfg,
        }
    }

    fn save(&mut self) {
        if self.readonly {
            return;
        }
        ensure_ids(&mut self.cfg);
        if let Err(e) = save_config(&self.cfg) {
            self.msg = format!("Не вдалося зберегти конфіг: {e}");
        }
    }

    fn spawn<F: FnOnce() -> Msg + Send + 'static>(&self, ctx: &egui::Context, f: F) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
    }

    fn spawn_opt<F: FnOnce() -> Option<Msg> + Send + 'static>(&self, ctx: &egui::Context, f: F) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(m) = f() {
                let _ = tx.send(m);
                ctx.request_repaint();
            }
        });
    }

    fn set_file(&mut self, f: PathBuf) {
        // CurseForge не приймає .mrpack: знімаємо галочку, а для іншого файлу повертаємо
        if f.extension().is_some_and(|e| e.eq_ignore_ascii_case("mrpack")) {
            self.cf_auto_off = self.cf_auto_off || self.use_cf;
            self.use_cf = false;
        } else if self.cf_auto_off {
            (self.use_cf, self.cf_auto_off) = (true, false);
        }
        self.file_note.clear();
        if let Some(i) = read_jar(&f) {
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
            if i.version.is_some() { got.push(format!("номер версії {}", self.number)); }
            if !i.mc_versions.is_empty() { got.push(format!("Minecraft {}", self.cfg.last.mc_versions)); }
            if !i.loaders.is_empty() { got.push(format!("лоадери: {}", self.cfg.last.loaders.join(", "))); }
            if !got.is_empty() {
                self.file_note = format!("З файлу підставлено: {}. Перевірте кроки «Версія» і «Сумісність».", got.join("; "));
            }
        }
        if let Some(d) = f.parent() {
            self.cfg.last.dir = d.to_string_lossy().into_owned();
        }
        self.file = Some(f);
    }

    fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
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
                        Err(e) => self.mr_msg = format!("Не вдалося завантажити проєкти: {e}"),
                    }
                }
                Msg::Check(r) => {
                    if r.is_ok() { self.cfg.modrinth_checked = now_secs(); self.save(); }
                    self.check_msg = match r {
                        Ok(n) => Chk::Ok(format!("Готово. Користувач: {n}")),
                        Err(e) => Chk::Err(format!("Помилка: {e}")),
                    }
                }
                Msg::CheckCf(r) => {
                    if r.is_ok() { self.cfg.curse_checked = now_secs(); self.save(); }
                    self.cf_msg = r.map_or_else(|e| Chk::Err(format!("Помилка: {e}")), |m| Chk::Ok(format!("Готово: {m}")));
                }
                Msg::CheckKey(r) => {
                    if r.is_ok() { self.cfg.core_checked = now_secs(); self.save(); }
                    self.key_msg = r.map_or_else(|e| Chk::Err(format!("Помилка: {e}")), |m| Chk::Ok(format!("Готово: {m}")));
                }
                Msg::CfInfo(id, target, r) => {
                    self.adding = self.adding.saturating_sub(1);
                    let (name, icon) = match r {
                        Ok(v) => v,
                        Err(e) => {
                            if e.contains("HTTP 404") {
                                self.notice = format!("Проєкт з ID {id} не знайдено на CurseForge: перевірте ID.");
                                self.cf_id.clear();
                                continue;
                            }
                            self.add_msg = format!("Core API: {e}. Додано вручну.");
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
                Msg::File(f) => self.set_file(f),
                Msg::Changelog(t) => self.changelog = t,
                Msg::Notice(t) => self.notice = t,
                Msg::Update(r, manual) => {
                    self.upd_checking = false;
                    match r {
                        Ok(Some(u)) => self.update = Some(u),
                        Ok(None) if manual => {
                            self.notice = if UPDATE_REPO.contains("OWNER") { "Репозиторій оновлень ще не налаштовано (UPDATE_REPO у logic.rs).".into() } else { format!("У вас остання версія ({}).", env!("CARGO_PKG_VERSION")) };
                        }
                        Err(e) if manual => self.upd_err = format!("Не вдалося перевірити оновлення: {e}"),
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
                    self.notice = format!("Оновлено дані проєктів: {n}.");
                    self.save();
                }
                Msg::Imported(ps) => {
                    let (added, merged) = merge_projects(&mut self.cfg, ps);
                    self.notice = format!("Імпорт: додано {added}, доповнено {merged}.");
                    self.save();
                }
                Msg::Done(p, r) => {
                    self.pending = self.pending.saturating_sub(1);
                    history_add(&self.cur.0, &self.cur.1, if matches!(p, P::Mr) { "Modrinth" } else { "CurseForge" }, &r);
                    self.history = history_load(HIST_MAX);
                    if r.is_ok() && let Some(i) = self.cur_idx.take() {
                        if let Some(p) = self.cfg.projects.get_mut(i) {
                            p.last_number = self.cur.1.clone();
                            p.last_changelog = self.cur_cl.clone();
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

    /// Змінює обраний проєкт і скидає стан попередньої публікації.
    fn select_project(&mut self, i: Option<usize>) {
        if self.sel != i {
            self.sel = i;
            (self.use_mr, self.use_cf, self.mr_res, self.cf_res) = (true, true, None, None);
            self.cf_auto_off = false;
            // номер із попередньої публікації цього проєкту, changelog не переносимо між проєктами
            self.number = i.and_then(|i| self.cfg.projects.get(i)).map(|p| bump(&p.last_number)).unwrap_or_default();
            self.changelog.clear();
            // MC/лоадери/тип останньої публікації цього проєкту (різні моди: різні налаштування)
            if let Some(p) = i.and_then(|i| self.cfg.projects.get(i)) {
                if !p.mc_versions.is_empty() { self.cfg.last.mc_versions = p.mc_versions.clone(); }
                if !p.loaders.is_empty() { self.cfg.last.loaders = p.loaders.clone(); }
                if let Some(t) = RELEASE_TYPES.iter().position(|r| *r == p.release_type) { self.release_type = t; }
            }
        }
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
        self.notice = format!("Проєкт «{}» уже був у списку: ID платформ оновлено.", self.cfg.projects[i].name);
        self.save();
    }

    /// Питання про однакову назву: об'єднати з наявним проєктом чи створити окремий.
    fn merge_dialog(&mut self, ctx: &egui::Context) {
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
            ui.set_width(400.0);
            ui.horizontal(|ui| {
                ui.label(title("Однакова назва"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if close_x(ui).clicked() { hide = true; }
                });
            });
            ui.add_space(8.0);
            ui.label(format!("У списку вже є проєкт «{}». Це один і той самий мод?", cur.name));
            hint(ui, "Якщо так, платформи об'єднаються в один проєкт.");
            if replaces {
                ui.add_space(8.0);
                banner(ui, Kind::Warn, "Для цієї платформи вже вказано інший ID — він буде замінений.");
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ghost_btn(ui, None, "Ні, окремий проєкт").clicked() { choice = Some(false); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(primary("Так, об'єднати")).clicked() { choice = Some(true); }
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
            ui.add(egui::Image::from_bytes("bytes://modwire-icon.png", include_bytes!("../assets/icon.png").as_slice()).fit_to_exact_size(egui::vec2(32.0, 32.0)).corner_radius(9));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(title("Modwire"));
                ui.label(RichText::new("публікація модів").color(MUTED).size(HINT_SZ));
            });
        });
        ui.add_space(14.0);
        let y = ui.cursor().min.y;
        ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, BORDER));
        ui.add_space(14.0);
        for (t, label) in [(Tab::Projects, "Проєкти"), (Tab::Publish, "Публікація")] {
            self.tab_button(ui, t, label);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(MUTED.gamma_multiply(0.7)).size(12.0));
            ui.add_space(4.0);
            self.tab_button(ui, Tab::Settings, "Налаштування");
        });
    }

    /// Пункт меню: справжня кнопка (фокус з клавіатури), іконка і текст малюються поверх.
    fn tab_button(&mut self, ui: &mut egui::Ui, t: Tab, label: &str) {
        let active = self.tab == t;
        let resp = ui.add(egui::Button::selectable(active, "").frame(false).min_size(egui::vec2(ui.available_width(), 40.0)));
        let rect = resp.rect;
        let hov = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered(), 0.12);
        let act = ui.ctx().animate_bool_with_time(resp.id.with("act"), active, 0.15);
        let p = ui.painter();
        p.rect_filled(rect, R_CTRL, HOVER.gamma_multiply(hov * (1.0 - act)));
        p.rect_filled(rect, R_CTRL, ACCENT.gamma_multiply(0.12 * act));
        if resp.has_focus() {
            p.rect_stroke(rect, R_CTRL, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
        }
        p.rect_filled(egui::Rect::from_center_size(egui::pos2(rect.left() + 1.5, rect.center().y), egui::vec2(3.0, (rect.height() - 16.0) * act)), 2, ACCENT);
        let (icon_col, text_col) = (MUTED.lerp_to_gamma(ACCENT, act), MUTED.lerp_to_gamma(TEXT, act));
        let kind = match t {
            Tab::Projects => Ic::Grid,
            Tab::Publish => Ic::Upload,
            Tab::Settings => Ic::Gear,
        };
        draw_icon(p, egui::pos2(rect.left() + 24.0, rect.center().y), kind, 20.0, icon_col);
        p.text(egui::pos2(rect.left() + 46.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), text_col);
        if resp.clicked() {
            self.tab = t;
        }
    }

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
        if !self.upd_err.is_empty() {
            banner(ui, Kind::Error, &self.upd_err.clone());
            ui.horizontal(|ui| {
                // запасний шлях: якщо автооновлення не вдалося (права на запис, антивірус), можна завантажити вручну
                if let Some(page) = self.update.as_ref().map(|u| u.page.clone()).filter(|p| !p.is_empty())
                    && icon_btn(ui, Some(Ic::External), "Завантажити вручну", false).clicked()
                {
                    ctx.open_url(egui::OpenUrl::new_tab(page));
                }
                if ghost_btn(ui, Some(Ic::Cross), "Закрити").clicked() {
                    self.upd_err.clear();
                }
            });
            ui.add_space(8.0);
        }
        let Some(u) = self.update.as_ref() else { return };
        let (ver, url, size, sha, page) = (u.version.clone(), u.url.clone(), u.size, u.sha256.clone(), u.page.clone());
        banner(ui, Kind::Info, &format!("Доступна нова версія {ver} (у вас {}).", env!("CARGO_PKG_VERSION")));
        ui.horizontal(|ui| {
            if self.upd_applying {
                ui.spinner();
                hint(ui, "Завантажуємо оновлення… Програма перезапуститься сама.");
            } else if url.is_empty() || !cfg!(windows) {
                if icon_btn(ui, Some(Ic::External), "Відкрити сторінку релізу", true).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(page));
                }
                if ghost_btn(ui, None, "Пізніше").clicked() {
                    self.update = None;
                }
            } else {
                let can = self.pending == 0 && self.adding == 0;
                if ui.add_enabled(can, primary("Оновити")).on_disabled_hover_text("Дочекайтесь завершення публікації").clicked() {
                    self.upd_applying = true;
                    self.upd_err.clear();
                    self.spawn(ctx, move || Msg::Updated(apply_update(&url, size, &sha).and_then(|()| restart_checked())));
                }
                if ghost_btn(ui, None, "Пізніше").clicked() {
                    self.update = None;
                }
            }
        });
        ui.add_space(8.0);
    }

    fn projects_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.merge_hidden && !self.merge_ask.is_empty() {
            banner(ui, Kind::Warn, "Є невирішене питання: проєкт з такою назвою вже є. Видалення проєктів призупинено.");
            if ghost_btn(ui, None, "Вирішити").clicked() {
                self.merge_hidden = false;
            }
            ui.add_space(8.0);
        }
        let no_tokens = self.cfg.modrinth_token.trim().is_empty() && self.cfg.curse_token.trim().is_empty();
        let no_projects = self.cfg.projects.is_empty();

        // заголовок і кнопка в одному рядку, підказка нижче на всю ширину (інакше текст заходить під кнопку)
        ui.horizontal(|ui| {
            ui.heading("Проєкти");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_btn(ui, Some(Ic::Plus), "Додати проєкт", true).clicked() {
                    self.add_open = true;
                    self.target = None;
                    self.add_msg.clear();
                }
            });
        });
        hint(ui, "Ваші моди на Modrinth і CurseForge. Видалення не чіпає сайти.");
        ui.add_space(12.0);
        if !self.msg.is_empty() {
            banner(ui, Kind::Warn, &self.msg);
            ui.add_space(8.0);
        }

        if no_tokens || no_projects {
            let (mut to_settings, mut to_add) = (false, false);
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(title("Швидкий старт"));
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    let chip = |ui: &mut egui::Ui, need: bool, text: &str, done_text: &str, btn: &str, plus: bool| -> bool {
                        let mut clicked = false;
                        egui::Frame::new().fill(FIELD).stroke(Stroke::new(1.0, UI_BORDER)).corner_radius(egui::CornerRadius::same(R_CTRL))
                            .inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    if need { badge_ic(ui, "Потрібно", ACCENT, Some(Ic::Warn)) } else { badge_ic(ui, "Готово", SUCCESS, Some(Ic::Check)) }
                                    ui.label(if need { text } else { done_text });
                                    if need {
                                        clicked = if plus { icon_btn(ui, Some(Ic::Plus), btn, false).clicked() } else { ghost_btn(ui, Some(Ic::Gear), btn).clicked() };
                                    }
                                });
                            });
                        clicked
                    };
                    to_settings = chip(ui, no_tokens, "Вкажіть токен платформи", "Токен вказано", "Налаштування", false);
                    to_add = chip(ui, no_projects, "Додайте свій мод", "Проєкт додано", "Додати проєкт", true);
                });
            });
            ui.add_space(12.0);
            if to_settings {
                self.tab = Tab::Settings;
            }
            if to_add {
                self.add_open = true;
                self.target = None;
                self.add_msg.clear();
            }
        }

        // Сітка карток: кількість колонок залежить від ширини вікна.
        let n = self.cfg.projects.len();
        let cols = ((ui.available_width() + GAP) / (CARD_MIN_W + GAP)).floor().max(1.0) as usize;
        let (mut publish, mut delete, mut ask, mut cancel) = (None, None, None, false);
        let (mut pin, mut sort, mut pin_only) = (None::<usize>, false, false);
        let confirm = self.confirm_del;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if n == 0 {
                ui.vertical_centered(|ui| {
                    // кнопка додавання вже є в шапці й у «Швидкому старті»
                    empty_state(ui, Ic::Grid, "Проєктів ще немає", "Додайте мод, який вже є на Modrinth або CurseForge: він з'явиться тут карткою.");
                });
            }
            if n > 1 && ghost_btn(ui, None, "Сортувати за назвою").on_hover_text("Закріплені першими, далі за алфавітом").clicked() {
                sort = true;
            }
            for start in (0..n).step_by(cols) {
                ui.columns(cols, |c| {
                    for (k, col_ui) in c.iter_mut().enumerate() {
                        let i = start + k;
                        let Some(p) = self.cfg.projects.get(i) else { continue };
                        card_hover(col_ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), PROJECT_CARD_H), egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.horizontal(|ui| {
                                    icon(ui, &p.icon_url, &p.name, 44.0);
                                    ui.vertical(|ui| {
                                        let mut job = egui::text::LayoutJob::single_section(p.name.clone(), egui::TextFormat::simple(egui::TextStyle::Name("Title".into()).resolve(ui.style()), TEXT));
                                        job.wrap.max_rows = 2;
                                        ui.add(egui::Label::new(job).wrap());
                                        ui.horizontal_wrapped(|ui| {
                                            if !p.modrinth_id.is_empty() { brand_pill(ui, "Modrinth", MR_GREEN, MR_BG); }
                                            if !p.curse_id.is_empty() { brand_pill(ui, "CurseForge", CF_ORANGE, CF_BG); }
                                            if p.modrinth_id.is_empty() && p.curse_id.is_empty() { hint(ui, "Платформи не підключені"); }
                                        });
                                    });
                                });
                                // кнопки притиснуті донизу: bottom_up у решті висоти картки
                                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                                    let row = |ui: &mut egui::Ui| {
                                        if ui.add(Solid { text: "Опублікувати".into(), fill: MR_GREEN, fg: ON_ACCENT, min: egui::vec2(0.0, 36.0) }).clicked() { publish = Some(i); }
                                        if confirm == Some(i) {
                                            if ui.add(danger("Точно видалити?")).clicked() { delete = Some(i); }
                                            if ghost_btn(ui, Some(Ic::Cross), "Скасувати").clicked() { cancel = true; }
                                        } else {
                                            if icon_only(ui, Ic::Pin, if p.pinned { "Відкріпити" } else { "Закріпити: закріплені проєкти йдуть першими" }, p.pinned).clicked() { pin = Some(i); }
                                            if ui.add_enabled_ui(self.pending == 0 && self.adding == 0, |ui| icon_only(ui, Ic::Trash, "Видалити запис у Modwire", false)).inner.on_disabled_hover_text("Недоступно, поки триває публікація або додавання").clicked() {
                                                ask = Some(i);
                                            }
                                        }
                                    };
                                    if confirm == Some(i) { ui.horizontal_wrapped(row); } else { ui.horizontal(row); }
                                    if confirm == Some(i) {
                                        hint(ui, "Видалити запис у Modwire? На сайтах нічого не зміниться");
                                    }
                                });
                            });
                        });
                    }
                });
                ui.add_space(GAP);
            }
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
            self.confirm_del = None;
            self.cfg.projects.remove(i);
            let shift = |o: Option<usize>| match o { Some(s) if s == i => None, Some(s) if s > i => Some(s - 1), o => o };
            let new_sel = shift(self.sel);
            if new_sel.is_none() {
                self.select_project(None); // видалили обраний: скидаємо форму
            } else {
                self.sel = new_sel; // лише зсув індексу, введене не губимо
            }
            self.target = shift(self.target);
            self.cur_idx = shift(self.cur_idx); // публікація в процесі: індекс не має вказувати на чужий проєкт
            self.cfg.last.project = shift(Some(self.cfg.last.project)).unwrap_or(usize::MAX);
            self.save();
        }
        self.add_dialog(ctx);
    }

    fn add_dialog(&mut self, ctx: &egui::Context) {
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
                    ui.label(title("Додати проєкт"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if close_x(ui).clicked() { close = true; }
                    });
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical().id_salt("add_scroll").max_height((ctx.content_rect().height() - 140.0).max(200.0)).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(ui.available_width() - 12.0);
                hint(ui, "Мод має вже існувати на сайті. Оберіть, де він є, і він з'явиться у списку. Можна додати обидві платформи до одного проєкту.");
                ui.add_space(8.0);
                field_label(ui, "Куди додати");
                let label = |i: Option<usize>, ps: &[Project]| match i.and_then(|i| ps.get(i)) {
                    Some(p) => format!("Додати до: {}", p.name),
                    None => "Новий проєкт".to_string(),
                };
                combo_box("target").width(ui.available_width()).selected_text(label(self.target, &self.cfg.projects)).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.target, None, "Новий проєкт");
                    for (i, p) in self.cfg.projects.iter().enumerate() {
                        ui.selectable_value(&mut self.target, Some(i), format!("Додати до: {}", p.name));
                    }
                });

                // --- Modrinth ---
                ui.add_space(8.0);
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(title("Modrinth").color(SUCCESS));
                    });
                    let clicked = ui.add_enabled_ui(!self.mr_loading, |ui| ghost_btn(ui, Some(Ic::Upload), if self.mr_list.is_empty() { "Завантажити мої проєкти" } else { "Оновити список" })).inner.clicked();
                    // список підтягується сам при відкритті, якщо є токен
                    if clicked || (just_opened && !self.mr_loading && self.mr_list.is_empty() && !self.cfg.modrinth_token.trim().is_empty()) {
                        if self.cfg.modrinth_token.trim().is_empty() {
                            self.mr_msg = "Спершу вкажіть токен Modrinth у налаштуваннях".into();
                        } else {
                            self.mr_loading = true;
                            self.mr_msg.clear();
                            let t = self.cfg.modrinth_token.trim().to_string();
                            self.spawn(ctx, move || {
                                let r = modrinth_projects(&t);
                                if let Ok(l) = &r { l.iter().for_each(|p| cache_icon(&p.icon_url)); }
                                Msg::List(r)
                            });
                        }
                    }
                    if self.cfg.modrinth_token.trim().is_empty() {
                        hint(ui, "Для списку потрібен токен Modrinth (вкладка «Налаштування»).");
                    }
                    if !self.mr_msg.is_empty() {
                        banner(ui, Kind::Error, &self.mr_msg);
                    }
                    let mut pick = None;
                    egui::ScrollArea::vertical().id_salt("mr_list").max_height((ctx.content_rect().height() * 0.45).clamp(170.0, 380.0)).auto_shrink([false, true]).show(ui, |ui| {
                        if self.mr_list.is_empty() {
                            if self.mr_loading {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    hint(ui, "Завантаження…");
                                });
                            } else if !self.cfg.modrinth_token.trim().is_empty() && self.mr_msg.is_empty() {
                                hint(ui, "На цьому акаунті Modrinth проєктів не знайдено.");
                            }
                        }
                        for (i, p) in self.mr_list.iter().enumerate() {
                            let added = self.cfg.projects.iter().any(|q| q.modrinth_id == p.id);
                            ui.horizontal(|ui| {
                                icon(ui, &p.icon_url, &p.title, 32.0);
                                ui.add(egui::Label::new(&p.title).truncate());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if added {
                                        badge_ic(ui, "Уже додано", SUCCESS, Some(Ic::Check));
                                    } else if ghost_btn(ui, Some(Ic::Plus), "Додати").clicked() {
                                        pick = Some(i);
                                    }
                                });
                            });
                        }
                    });
                    if let Some(i) = pick {
                        let p = &self.mr_list[i];
                        let np = Project { name: p.title.clone(), modrinth_id: p.id.clone(), icon_url: p.icon_url.clone(), ..Default::default() };
                        self.add_project(np);
                        self.add_open = false;
                    }
                });

                // --- CurseForge ---
                ui.add_space(8.0);
                card(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(title("CurseForge").color(CF_COL));
                    });
                    let lid = field_label(ui, "ID проєкту *");
                    let idr = ui.add(egui::TextEdit::singleline(&mut self.cf_id).hint_text("наприклад, 123456").desired_width(f32::INFINITY)).labelled_by(lid);
                    if just_opened {
                        idr.request_focus();
                    }
                    if !self.cf_id.trim().is_empty() && !self.cf_id.trim().chars().all(|c| c.is_ascii_digit()) {
                        field_error(ui, &idr, "Лише цифри");
                    }
                    hint(ui, if self.cfg.curse_core_key.trim().is_empty() { "Без Core API-ключа проєкт отримає назву «CurseForge #ID»; ключ додається в налаштуваннях." } else { "Назву й іконку буде підтягнуто через Core API-ключ." });
                    let id_ok = !self.cf_id.trim().is_empty() && self.cf_id.trim().chars().all(|c| c.is_ascii_digit());
                    let key =self.cfg.curse_core_key.trim().to_string();
                    let can_add = id_ok;
                    let enter = idr.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.add_enabled(can_add, primary("Додати з CurseForge").min_size(egui::vec2(ui.available_width(), 34.0))).on_disabled_hover_text("Введіть числовий ID проєкту").clicked() || (can_add && enter) {
                        self.cf_id = self.cf_id.trim().to_string();
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
                });
                if !self.add_msg.is_empty() {
                    ui.add_space(8.0);
                    banner(ui, Kind::Warn, &self.add_msg);
                }
                });
            });
        if close || ((self.merge_ask.is_empty() || self.merge_hidden) && r.should_close()) {
            self.add_open = false;
        }
    }

    // ---------- Публікація ----------
    fn release(&self) -> Release {
        Release {
            name: self.name.clone(),
            number: self.number.trim().to_string(),
            mc_versions: parse_mc(&self.cfg.last.mc_versions),
            loaders: self.cfg.last.loaders.clone(),
            release_type: RELEASE_TYPES[self.release_type].into(),
            changelog: self.changelog.clone(),
        }
    }

    /// Жива перевірка готовності: що саме заважає натиснути «Опублікувати».
    fn check(&self, mr: bool, cf: bool) -> Result<(), String> {
        if self.upd_applying {
            return Err("Триває оновлення програми: зачекайте, поки вона перезапуститься".into());
        }
        validate(self.sel.is_some(), mr || cf, &self.release(), self.file.as_deref())?;
        if cf && self.file.as_ref().and_then(|f| f.extension()).is_some_and(|e| e.eq_ignore_ascii_case("mrpack")) {
            return Err("CurseForge не приймає .mrpack — вимкніть CurseForge".into());
        }
        if !self.mc_all.is_empty()
            && let Some(bad) = self.release().mc_versions.iter().find(|v| !self.mc_all.contains(v))
        {
            return Err(format!("Невідома версія Minecraft: {bad}"));
        }
        if mr && self.cfg.modrinth_token.trim().is_empty() {
            return Err("Немає токена Modrinth (вкладка «Налаштування»)".into());
        }
        if cf && self.cfg.curse_token.trim().is_empty() {
            return Err("Немає токена CurseForge (вкладка «Налаштування»)".into());
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
            format!("Minecraft {} · {}", self.cfg.last.mc_versions.trim(), self.cfg.last.loaders.join(", ")),
            self.changelog.lines().find(|l| !l.trim().is_empty()).map(|l| l.chars().take(80).collect()).unwrap_or_default(),
            plats.join(" і "),
        ]
    }

    /// Версії Minecraft: поле і швидкий вибір.
    fn mc_ui(&mut self, ui: &mut egui::Ui, focus: bool) {
        let lid = field_label(ui, "Версії Minecraft *");
        let r = ui.add(egui::TextEdit::singleline(&mut self.cfg.last.mc_versions).hint_text("напр. 1.20.1, 1.20.2").desired_width(f32::INFINITY)).labelled_by(lid);
        if focus {
            r.request_focus();
        }
        // швидкий вибір: останні релізи з Modrinth
        let mut typed = parse_mc(&self.cfg.last.mc_versions);
        if self.mc_all.is_empty() && !typed.is_empty() {
            ui.label(RichText::new("Версії не звірено зі списком (немає зв'язку з Modrinth), перевірить сервер.").color(MUTED).size(HINT_SZ));
        }
        if self.mc_known.is_empty() && self.mc_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                hint(ui, "Завантажуємо список версій…");
            });
        }
        if !self.mc_known.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for v in self.mc_known.iter().take(10) {
                    let on = typed.contains(v);
                    if mc_chip(ui, on, v).clicked() {
                        if on { typed.retain(|t| t != v) } else { typed.push(v.clone()) }
                        self.cfg.last.mc_versions = typed.join(", ");
                    }
                }
            });
            let bad: Vec<_> = parse_mc(&self.cfg.last.mc_versions).into_iter().filter(|t| !self.mc_all.contains(t)).collect();
            if !bad.is_empty() {
                ui.label(RichText::new(format!("Невідомі версії: {}", bad.join(", "))).color(WARN).size(HINT_SZ));
            }
        }
    }

    /// Лоадери.
    fn loaders_ui(&mut self, ui: &mut egui::Ui) {
        field_label(ui, "Лоадери *");
        ui.horizontal_wrapped(|ui| {
            for l in LOADERS {
                let on = self.cfg.last.loaders.iter().any(|x| x == l);
                if choice(ui, on, l).clicked() {
                    if on { self.cfg.last.loaders.retain(|x| x != l) } else { self.cfg.last.loaders.push(l.into()) }
                }
            }
        });
    }

    fn publish_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Публікація");
        if !self.cfg.projects.is_empty() && self.cfg.modrinth_token.trim().is_empty() && self.cfg.curse_token.trim().is_empty() {
            banner(ui, Kind::Warn, "Токени ще не вказано: заповнити форму можна, але надіслати файл — ні.");
            if ghost_btn(ui, Some(Ic::Gear), "Вказати токен").clicked() {
                self.tab = Tab::Settings;
            }
            ui.add_space(8.0);
        }
        hint(ui, "Заповніть кроки по черзі. Ctrl+Enter — опублікувати.");
        ui.add_space(12.0);
        if self.cfg.projects.is_empty() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    empty_state(ui, Ic::Grid, "Спершу додайте проєкт", "Публікація можлива, коли є хоч один проєкт.");
                    if ui.add(primary("Перейти до проєктів")).clicked() {
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
        let sp = |i: usize, name: &'static str, required: bool| StepSpec { n: i + 1, name, done: done[i], required, summary: &sum[i], scroll: focus == Some(i) };
        let mut open = self.open_steps;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            step(ui, sp(0, "Проєкт", true), &mut open[0], |ui| {
                let before = self.sel;
                let busy = self.pending > 0; // під час публікації проєкт не міняємо
                let text = self.sel.and_then(|i| self.cfg.projects.get(i)).map_or("Обрати проєкт".to_string(), |p| p.name.clone());
                ui.add_enabled_ui(!busy, |ui| {
                    combo_box("proj").width(ui.available_width()).selected_text(text).show_ui(ui, |ui| {
                        for (i, p) in self.cfg.projects.iter().enumerate() {
                            ui.selectable_value(&mut self.sel, Some(i), &p.name);
                        }
                    });
                });
                if before != self.sel {
                    let now = std::mem::replace(&mut self.sel, before);
                    self.select_project(now);
                }
            });

            step(ui, sp(1, "Файл", true), &mut open[1], |ui| {
                let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
                let t = ctx.animate_bool_with_time(ui.next_auto_id(), hovering, 0.12);
                let (stroke, fill) = (Stroke::new(1.0 + t, UI_BORDER.lerp_to_gamma(ACCENT, t)), FIELD.lerp_to_gamma(ACCENT, 0.08 * t));
                egui::Frame::new().fill(fill).stroke(stroke).corner_radius(egui::CornerRadius::same(CARD_R)).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(84.0);
                    ui.vertical_centered(|ui| {
                        if hovering {
                            ui.add_space(24.0);
                            ui.label(title("Відпустіть файл тут"));
                            ui.add_space(24.0);
                            return;
                        }
                        match &self.file {
                            Some(f) => {
                                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                ui.label(title(name));
                                hint(ui, "Файл обрано. Можна перетягнути інший або обрати його тут.");
                                if ghost_btn(ui, Some(Ic::Upload), "Обрати інший файл…").clicked() {
                                    // діалог у окремому потоці, щоб не блокувати UI
                                    { let d = self.cfg.last.dir.clone(); self.spawn_opt(ctx, move || pick_file(d)); }
                                }
                            }
                            None => {
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                                draw_icon(ui.painter(), rect.center(), Ic::Upload, 48.0, ACCENT.gamma_multiply(0.85));
                                ui.label(title("Перетягніть файл сюди"));
                                hint(ui, ".jar, .zip або .mrpack");
                                ui.add_space(8.0);
                                if ghost_btn(ui, Some(Ic::Upload), "Обрати файл…").clicked() {
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
                    banner(ui, Kind::Info, "CurseForge вимкнено автоматично: він не приймає .mrpack. Оберіть .jar або .zip, щоб повернути його.");
                }
            });

            step(ui, sp(2, "Версія", true), &mut open[2], |ui| {
                ui.columns(2, |c| {
                    let lid = field_label(&mut c[0], "Номер версії *");
                    let nr = c[0].add(egui::TextEdit::singleline(&mut self.number).hint_text("напр. 1.0.0").desired_width(f32::INFINITY)).labelled_by(lid);
                    if focus == Some(2) {
                        nr.request_focus();
                    }
                    if self.file.is_some() && self.number.trim().is_empty() {
                        field_error(&mut c[0], &nr, "Вкажіть номер версії");
                    } else if let Some(p) = self.sel.and_then(|i| self.cfg.projects.get(i))
                        && let Some(w) = version_warning(&self.number, &p.name, &p.last_number, &self.history)
                    {
                        c[0].label(RichText::new(w).color(WARN).size(HINT_SZ));
                    }
                    let lid = field_label(&mut c[1], "Назва");
                    c[1].add(egui::TextEdit::singleline(&mut self.name).hint_text("необов'язково, за замовч. — номер").desired_width(f32::INFINITY)).labelled_by(lid);
                });
                ui.add_space(8.0);
                hint(ui, "* обов'язкове поле");
                field_label(ui, "Тип релізу");
                ui.horizontal_wrapped(|ui| {
                    for (i, r) in RELEASE_TYPES.iter().enumerate() {
                        if choice(ui, self.release_type == i, r).clicked() {
                            self.release_type = i;
                        }
                    }
                });
            });

            step(ui, sp(3, "Сумісність", true), &mut open[3], |ui| {
                if ui.available_width() >= 480.0 {
                    ui.columns(2, |c| {
                        self.mc_ui(&mut c[0], focus == Some(3));
                        self.loaders_ui(&mut c[1]);
                    });
                } else {
                    self.mc_ui(ui, focus == Some(3));
                    ui.add_space(8.0);
                    self.loaders_ui(ui);
                }
            });

            step(ui, sp(4, "Зміни", false), &mut open[4], |ui| {
                let lid = field_label(ui, "Що змінилось");
                ui.add(egui::TextEdit::multiline(&mut self.changelog).hint_text("Можна списки (- пункт) і **жирний** текст").desired_rows(4).desired_width(f32::INFINITY)).labelled_by(lid);
                hint(ui, "У шаблонах: {version} {mc} {loaders} {date} {project}");
                ui.horizontal_wrapped(|ui| {
                if let Some(c) = self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| p.last_changelog.clone()).filter(|c| !c.is_empty())
                    && ghost_btn(ui, Some(Ic::File), "Взяти changelog минулої версії").clicked()
                {
                    self.changelog = c;
                }
                let tpl = self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| (p.name.clone(), p.changelog_template.clone())).filter(|t| !t.1.is_empty());
                if let Some((name, t)) = tpl
                    && ghost_btn(ui, Some(Ic::File), "Вставити шаблон змін").on_hover_text("Підстановки: {version} {mc} {loaders} {date} {project}").clicked()
                {
                    self.changelog = render_template(&t, &name, &self.release());
                }
                if !self.changelog.trim().is_empty()
                    && let Some(i) = self.sel.filter(|i| *i < self.cfg.projects.len())
                    && ghost_btn(ui, Some(Ic::File), "Зберегти як шаблон проєкту").on_hover_text("Поточний текст стане шаблоном; використайте {version}, {mc}, {loaders}, {date}, {project}").clicked()
                {
                    self.cfg.projects[i].changelog_template = self.changelog.clone();
                    self.notice = "Шаблон змін збережено для цього проєкту.".into();
                    self.save();
                }
                if ghost_btn(ui, Some(Ic::File), "Взяти з файлу CHANGELOG.md…").on_hover_text("Береться перша (остання за часом) секція файлу").clicked() {
                    let d = self.cfg.last.dir.clone();
                    self.spawn_opt(ctx, move || pick_changelog(d));
                }
                });
            });

            step(ui, sp(5, "Куди публікувати", true), &mut open[5], |ui| {
                let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
                let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
                if proj.is_none() {
                    hint(ui, "Спершу оберіть проєкт у кроці 1.");
                } else if !has_mr && !has_cf {
                    hint(ui, "Проєкт не підключений до жодної платформи. Додайте його у розділі «Проєкти».");
                }
                if self.cf_auto_off && !self.use_cf && has_cf {
                    banner(ui, Kind::Info, "CurseForge вимкнено автоматично, бо вибрано файл .mrpack.");
                }
                ui.horizontal_wrapped(|ui| {
                    for (has, on, name, col) in [(has_mr, &mut self.use_mr, "Modrinth", SUCCESS), (has_cf, &mut self.use_cf, "CurseForge", CF_COL)] {
                        let tip = "Проєкт не підключений до цієї платформи";
                        let r = ui.add_enabled_ui(has, |ui| choice_col(ui, *on && has, name, col, 0.14)).inner;
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
                            hint(ui, &format!("{name}: проєкт не підключений до цієї платформи."));
                        }
                    }
                }
            });

            self.publish_extras(ui);
        });
        self.open_steps = open;
    }

    /// Нижня панель: смуга результатів зверху, нижче підсумок ліворуч і кнопка праворуч.
    fn publish_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
        let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
        let (mr, cf) = (has_mr && self.use_mr, has_cf && self.use_cf);
        let ready = self.check(mr, cf);
        let busy = self.pending > 0;
        let bars: Vec<(&str, f32, Color32, String)> = if busy {
            [("Modrinth", &self.up_mr, &self.mr_res, SUCCESS), ("CurseForge", &self.up_cf, &self.cf_res, CF_COL)]
                .into_iter()
                .filter_map(|(n, up, res, col)| {
                    let total = up.prog.total.load(Relaxed);
                    (total > 0 && res.is_none()).then(|| {
                        let extra = up.prog.speed_eta().map_or(String::new(), |(bps, eta)| format!(" · {:.1} МБ/с · ще ~{} с", bps as f64 / 1_048_576.0, eta));
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
            egui::ScrollArea::vertical().id_salt("pub_res").max_height((ctx.content_rect().height() * 0.3).clamp(100.0, 220.0)).auto_shrink([false, true]).show(ui, |ui| {
                for (name, f, col, extra) in &bars {
                    // підпис над смугою: довжина тексту не зсуває початок смуги, обидві платформи вирівняні
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(RichText::new(format!("{name}: {}%{extra}", (f * 100.0) as u32)).color(MUTED).size(HINT_SZ));
                        ui.add(egui::ProgressBar::new(*f).desired_height(8.0).desired_width(ui.available_width()).fill(*col).corner_radius(4).animate(false));
                    });
                }
                if !self.msg.is_empty() {
                    banner(ui, Kind::Warn, &self.msg);
                }
                for (name, r) in [("Modrinth", &self.mr_res), ("CurseForge", &self.cf_res)] {
                    match r {
                        Some(Ok(s)) => {
                            let (txt, url) = split_url(s);
                            banner(ui, Kind::Ok, &format!("{name}: опубліковано. {txt}"));
                            if let Some(u) = url {
                                ui.horizontal_wrapped(|ui| {
                                    if icon_btn(ui, Some(Ic::External), "Відкрити на сайті", false).clicked() {
                                        ui.ctx().open_url(egui::OpenUrl::new_tab(u.clone()));
                                    }
                                    if ghost_btn(ui, None, "Копіювати посилання").clicked() {
                                        ui.ctx().copy_text(u);
                                        self.notice = "Посилання скопійовано.".into();
                                    }
                                });
                            }
                        }
                        Some(Err(e)) => {
                            banner(ui, Kind::Error, &format!("{name}: не вдалося опублікувати. {e}"));
                            if e.contains("перевірте на сайті") {
                                // запит міг дійти до сервера: даємо посилання й попереджаємо про дубль
                                hint(ui, "Файл міг уже потрапити на сайт. Перевірте список версій, перш ніж публікувати повторно, щоб не було дубля.");
                                let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
                                let url = proj.and_then(|p| if name == "Modrinth" { (!p.modrinth_id.is_empty()).then(|| format!("https://modrinth.com/project/{}/versions", p.modrinth_id)) } else { (!p.curse_id.is_empty()).then(|| format!("https://www.curseforge.com/projects/{}", p.curse_id)) });
                                if let Some(u) = url && icon_btn(ui, Some(Ic::External), "Відкрити проєкт на сайті", false).clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(u));
                                }
                            }
                        }
                        None => {}
                    }
                }
                if !busy && any_err {
                    hint(ui, if any_ok { "Частина платформ уже прийняла версію. Кнопка «Повторити» нижче надішле файл лише туди, де не вдалося, дублів не буде." } else { "Виправте причину й натисніть «Повторити»: введені дані збережено." });
                }
                if !busy && any_ok {
                    ui.horizontal(|ui| {
                        new_version = ghost_btn(ui, Some(Ic::Plus), "Нова версія").on_hover_text("Очищає форму для наступної публікації").clicked();
                        if ghost_btn(ui, Some(Ic::File), "До історії").on_hover_text("Прокрутити до списку публікацій").clicked() {
                            self.focus_hist = true;
                        }
                    });
                }
            });
            if new_version {
                (self.number, self.name, self.changelog, self.file) = Default::default();
                self.number = self.sel.and_then(|i| self.cfg.projects.get(i)).map(|p| bump(&p.last_number)).unwrap_or_default();
                (self.use_mr, self.use_cf, self.mr_res, self.cf_res) = (true, true, None, None);
                self.msg.clear();
            }
            ui.add_space(8.0);
        }
        let narrow = ui.available_width() < 560.0;
        let why = if busy { "Триває публікація".to_string() } else { ready.as_ref().err().cloned().unwrap_or_default() };
        let can = !busy && ready.is_ok();
        let failed: Vec<&str> = [("Modrinth", &self.mr_res), ("CurseForge", &self.cf_res)].into_iter().filter(|(_, r)| matches!(r, Some(Err(_)))).map(|(n, _)| n).collect();
        let label = if busy { "Публікація…".to_string() } else if !failed.is_empty() && can { format!("Повторити: {}", failed.join(" і ")) } else { "Опублікувати".to_string() };
        let mk = || primary(&label).min_size(egui::vec2(if narrow { 150.0 } else { 220.0 }, if narrow { 38.0 } else { 44.0 }));
        // у вузькому вікні — один рядок (підсумок і кнопка); два рядки лише під час публікації, де є кнопки «Скасувати»
        if narrow && busy {
            ui.horizontal(|ui| self.bar_summary(ui, busy, &ready, mr, cf));
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(can, mk()).on_disabled_hover_text(why).clicked() {
                    self.start_publish(ctx, mr, cf);
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(can, mk()).on_disabled_hover_text(why).clicked() {
                        self.start_publish(ctx, mr, cf);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| self.bar_summary(ui, busy, &ready, mr, cf));
                });
            });
        }
    }

    /// Підсумок у нижній панелі: прогрес, готовність або причина.
    fn bar_summary(&mut self, ui: &mut egui::Ui, busy: bool, ready: &Result<(), String>, mr: bool, cf: bool) {
        if busy {
            ui.spinner();
            let running: Vec<(&str, &Up)> = [("Modrinth", &self.up_mr), ("CurseForge", &self.up_cf)].into_iter().filter(|(_, u)| u.active.load(Relaxed)).collect();
            ui.label(if running.iter().all(|(_, u)| u.cancel.load(Relaxed)) { "Скасування…" } else { "Надсилаємо файл, зачекайте…" });
            let many = running.len() > 1;
            for (n, u) in running {
                let cancelling = u.cancel.load(Relaxed);
                let label = if many { format!("Скасувати {n}") } else { "Скасувати".to_string() };
                if ui.add_enabled_ui(!cancelling, |ui| ghost_btn(ui, Some(Ic::Cross), &label)).inner.clicked() {
                    u.cancel.store(true, Relaxed);
                }
            }
        } else {
            let d = self.steps_done();
            let cur = [0usize, 1, 2, 3, 5].into_iter().find(|&i| !d[i]).map_or(6, |i| i + 1);
            match ready {
                Ok(()) => {
                    let p = self.sel.and_then(|i| self.cfg.projects.get(i));
                    let to = match (mr, cf) { (true, true) => "Modrinth і CurseForge", (true, false) => "Modrinth", _ => "CurseForge" };
                    let txt = p.map_or_else(String::new, |p| format!("{} {} → {}", p.name, self.number.trim(), to));
                    icon_label_c(ui, Ic::Check, SUCCESS, &txt, SUCCESS);
                }
                Err(e) => {
                    ui.label(RichText::new(format!("Крок {cur} з 6")).color(MUTED).size(HINT_SZ));
                    icon_label(ui, Ic::Warn, WARN, &format!("Ще не готово: {e}"));
                    if e.contains("Немає токена") {
                        if ghost_btn(ui, Some(Ic::Gear), "До налаштувань").clicked() {
                            self.tab = Tab::Settings;
                        }
                    } else if cur <= 6 && ghost_btn(ui, None, &format!("До кроку {cur}")).on_hover_text("Прокрутити до першого незаповненого кроку").clicked() {
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
                let found =history_filter(&self.history, &self.hist_q, self.hist_fail);
                let limit = if self.hist_all { usize::MAX } else { 10 };
                let total = found.len();
                egui::CollapsingHeader::new(title(format!("Історія · {}", self.history.len()))).default_open(true).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.hist_q).hint_text("Пошук: проєкт, номер, платформа, текст").desired_width(260.0_f32.min(ui.available_width() - 8.0)));
                        if choice(ui, self.hist_fail, "Лише помилки").clicked() { self.hist_fail = !self.hist_fail; }
                        if self.hist_confirm {
                            if ui.add(danger("Так, очистити історію")).clicked() {
                                clear = true;
                            }
                            if ghost_btn(ui, Some(Ic::Cross), "Скасувати").clicked() {
                                self.hist_confirm = false;
                            }
                        } else {
                            if ghost_btn(ui, Some(Ic::File), "Експорт у CSV…").on_hover_text("Усі записи історії для таблиці").clicked() {
                                export = true;
                            }
                            if ghost_btn(ui, None, "Очистити історію").clicked() {
                                self.hist_confirm = true;
                            }
                        }
                    });
                    if found.is_empty() {
                        hint(ui, "Нічого не знайдено за цим фільтром.");
                    }
                    let avail = ui.available_width();
                    let narrow = avail < 560.0;
                    egui::Grid::new("history_grid").num_columns(if narrow { 2 } else { 4 }).spacing([16.0, 8.0]).min_col_width(60.0).show(ui, |ui| {
                        ui.spacing_mut().interact_size.y = 22.0; // комірки рядка вирівнюються по верху, без зсуву від висоти кнопки
                        for e in found.iter().take(limit) {
                            let ok = e["ok"].as_bool().unwrap_or(false);
                            let col = if ok { SUCCESS } else { ERROR };
                            let msg = e["msg"].as_str().unwrap_or("");
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                                draw_icon(ui.painter(), rect.center(), if ok { Ic::Check } else { Ic::Cross }, 14.0, col);
                                ui.label(RichText::new(if ok { "Готово" } else { "Помилка" }).color(col)).on_hover_text(msg);
                            });
                            let vr = ui.vertical(|ui| {
                                ui.set_max_width(if narrow { (avail - 130.0).max(120.0) } else { 220.0 });
                                let full = format!("{} {}", e["project"].as_str().unwrap_or("?"), e["number"].as_str().unwrap_or(""));
                                let tip = if msg.is_empty() { full.clone() } else { format!("{full}\n{msg}") };
                                ui.add(egui::Label::new(full).truncate()).on_hover_text(tip);
                                if narrow {
                                    hint(ui, &format!("{} · {}", e["platform"].as_str().unwrap_or(""), ago(e["t"].as_u64().unwrap_or(0))));
                                }
                                if let Some(u) = history_link(e) {
                                    ui.horizontal(|ui| {
                                        for (label, copy) in [("Відкрити", false), ("Копіювати посилання", true)] {
                                            let r = ui.add(egui::Label::new(RichText::new(label).color(ACCENT).size(HINT_SZ)).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if r.clicked() {
                                                if copy { ui.ctx().copy_text(u.clone()); copied = true; } else { ui.ctx().open_url(egui::OpenUrl::new_tab(u.clone())); }
                                            }
                                        }
                                    });
                                }
                                if !ok {
                                    let m: String = msg.chars().take(110).collect();
                                    ui.label(RichText::new(m).color(MUTED).size(HINT_SZ));
                                }
                            });
                            let r = if narrow {
                                vr.response // платформа і час уже в другому рядку
                            } else {
                                ui.label(e["platform"].as_str().unwrap_or(""));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(ago(e["t"].as_u64().unwrap_or(0))).color(MUTED));
                                }).response
                            };
                            ui.end_row();
                            let y = r.rect.bottom() + 4.0;
                            // підсвітка рядка під курсором (тонка, поверх тексту не заважає)
                            let row = egui::Rect::from_min_max(egui::pos2(ui.min_rect().left(), r.rect.top() - 4.0), egui::pos2(ui.max_rect().right(), y));
                            if ui.rect_contains_pointer(row) {
                                ui.painter().rect_filled(row, 4, ACCENT.gamma_multiply(0.06));
                            }
                            ui.painter().hline(ui.min_rect().left()..=ui.max_rect().right(), y, Stroke::new(1.0, BORDER));
                        }
                    });
                    if total > 10 && ghost_btn(ui, None, if self.hist_all { "Показати менше" } else { "Показати всі" }).clicked() {
                        self.hist_all = !self.hist_all;
                    }
                });
                if copied {
                    self.notice = "Посилання скопійовано.".into();
                }
                if export {
                    let csv = history_csv(&self.history);
                    self.spawn_opt(ui.ctx(), move || {
                        let p = rfd::FileDialog::new().set_file_name("modwire-history.csv").add_filter("CSV", &["csv"]).save_file()?;
                        Some(Msg::Notice(match std::fs::write(&p, csv) {
                            Ok(()) => format!("Історію експортовано: {}", p.display()),
                            Err(e) => format!("Не вдалося записати файл: {e}"),
                        }))
                    });
                }
                if clear {
                    match history_clear() {
                        Ok(()) => self.history.clear(),
                        Err(e) => self.msg = format!("Не вдалося очистити історію: {e}"),
                    }
                    self.hist_confirm = false;
                }
            } else {
                ui.add_space(8.0);
                hint(ui, "Історія публікацій з'явиться тут після першої відправки.");
            }
        }
    }

    fn start_publish(&mut self, ctx: &egui::Context, mr: bool, cf: bool) {
        self.msg.clear();
        let rel = self.release();
        let (Some(si), Some(file)) = (self.sel, self.file.clone()) else { return };
        let Some(p) = self.cfg.projects.get(si).cloned() else { return };
        self.cur = (p.name.clone(), rel.number.clone());
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
                    P::Mr => publish_modrinth(token.trim(), &id, &rel, &file, &up),
                    P::Cf => publish_curse(token.trim(), &core_key, &id, &rel, &file, &up),
                }))
                .unwrap_or_else(|_| Err("Внутрішня помилка під час публікації: перевірте на сайті, чи не з'явилась версія".into()));
                up.active.store(false, Relaxed);
                Msg::Done(plat, r)
            });
        }
    }

    // ---------- Налаштування ----------
    fn settings_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Налаштування");
        hint(ui, "Токени зберігаються на цьому комп'ютері. Зміни зберігаються автоматично.");
        ui.add_space(12.0);
        let mut changed = false;
        // Modrinth | CurseForge (upload-токен і Core API-ключ в одній плитці), нижче безпека.
        // У вузькому вікні плитки стають одна під одною; уся сторінка прокручується.
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if ui.available_width() < 620.0 {
                self.mr_card(ui, ctx, &mut changed);
                ui.add_space(12.0);
                self.cf_card(ui, ctx, &mut changed);
            } else {
                ui.columns(2, |cols| {
                    self.mr_card(&mut cols[0], ctx, &mut changed);
                    self.cf_card(&mut cols[1], ctx, &mut changed);
                });
            }
            ui.add_space(12.0);
            self.security_card(ui, ctx, &mut changed);
            ui.add_space(12.0);
            self.about_card(ui, ctx);
        });
        if changed {
            self.save();
        }
    }

    fn mr_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, changed: &mut bool) {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(title("Modrinth"));
                    token_badge(ui, &self.cfg.modrinth_token);
                                    });
                hint(ui, "Права токена: Create versions, Read user, Read projects.");
                link(ui, "1. Увійдіть на Modrinth.\n2. Сторінка Personal access tokens > «Create a PAT».\n3. Дайте право Create versions (і Read user, Read projects для списку проєктів).\n4. Скопіюйте токен і вставте сюди.", "https://modrinth.com/settings/pats");
                if token_field(ui, &mut self.cfg.modrinth_token, &mut self.show_tokens) {
                    self.cfg.modrinth_checked = 0;
                    *changed = true;
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled_ui(self.check_msg != Chk::Checking, |ui| icon_btn(ui, Some(Ic::Check), if self.check_msg == Chk::Checking { "Перевіряємо…" } else { "Перевірити" }, false)).inner.clicked() {
                        let t = self.cfg.modrinth_token.trim().to_string();
                        if t.is_empty() {
                            self.check_msg = Chk::Err("Спершу вставте токен".into());
                        } else {
                            self.check_msg = Chk::Checking;
                            self.spawn(ctx, move || Msg::Check(modrinth_user(&t).map(|u| u.1)));
                        }
                    }
                    status(ui, &self.check_msg);
                });
            });
    }

    fn cf_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, changed: &mut bool) {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(title("CurseForge"));
                    token_badge(ui, &self.cfg.curse_token);
                                    });
                hint(ui, "Upload API token для публікації файлів.");
                link(ui, "1. Увійдіть на CurseForge.\n2. Account > API Tokens.\n3. Створіть токен (Generate token) і скопіюйте.\n4. Вставте сюди.", "https://www.curseforge.com/account/api-tokens");
                if token_field(ui, &mut self.cfg.curse_token, &mut self.show_tokens) {
                    self.cfg.curse_checked = 0;
                    *changed = true;
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled_ui(self.cf_msg != Chk::Checking, |ui| icon_btn(ui, Some(Ic::Check), if self.cf_msg == Chk::Checking { "Перевіряємо…" } else { "Перевірити" }, false)).inner.clicked() {
                        let t = self.cfg.curse_token.trim().to_string();
                        if t.is_empty() {
                            self.cf_msg = Chk::Err("Спершу вставте токен".into());
                        } else {
                            self.cf_msg = Chk::Checking;
                            self.spawn(ctx, move || Msg::CheckCf(curse_token_check(&t)));
                        }
                    }
                    status(ui, &self.cf_msg);
                });
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Core API-ключ").strong().color(TEXT));
                    badge(ui, "Необов'язково", MUTED);
                                    });
                hint(ui, "Лише для назви й іконки проєкту під час додавання.");
                link(ui, "Потрібен лише щоб підтягувати назву й іконку проєкту.\n1. Увійдіть у CurseForge for Studios console.\n2. Розділ API keys > скопіюйте ключ.\n3. Вставте сюди.", "https://console.curseforge.com/#/api-keys");
                if token_field(ui, &mut self.cfg.curse_core_key, &mut self.show_tokens) {
                    self.cfg.core_checked = 0;
                    *changed = true;
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled_ui(self.key_msg != Chk::Checking, |ui| icon_btn(ui, Some(Ic::Check), if self.key_msg == Chk::Checking { "Перевіряємо…" } else { "Перевірити" }, false)).inner.clicked() {
                        let k = self.cfg.curse_core_key.trim().to_string();
                        if k.is_empty() {
                            self.key_msg = Chk::Err("Спершу вставте ключ".into());
                        } else {
                            self.key_msg = Chk::Checking;
                            self.spawn(ctx, move || Msg::CheckKey(curse_key_check(&k)));
                        }
                    }
                    status(ui, &self.key_msg);
                });
            });
    }

    fn about_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(title("Про застосунок"));
            hint(ui, &format!("Modwire {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(8.0);
            let busy = self.upd_checking || self.upd_applying;
            if ui.add_enabled_ui(!busy, |ui| icon_btn(ui, Some(Ic::Refresh), if self.upd_checking { "Перевіряємо…" } else { "Перевірити оновлення" }, false)).inner.clicked() {
                self.upd_checking = true;
                self.upd_err.clear();
                self.spawn(ctx, || Msg::Update(check_update(), true));
            }
        });
    }

    fn security_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, changed: &mut bool) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(title("Дані"));
            hint(ui, "Список проєктів (без токенів) можна перенести на інший комп'ютер. Імпорт доповнює список, а не замінює.");
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ghost_btn(ui, Some(Ic::File), "Експорт проєктів…").clicked() {
                    let json = export_projects(&self.cfg.projects);
                    self.spawn_opt(ctx, move || {
                        let p = rfd::FileDialog::new().set_file_name("modwire-projects.json").add_filter("JSON", &["json"]).save_file()?;
                        Some(Msg::Notice(match std::fs::write(&p, json) {
                            Ok(()) => format!("Проєкти експортовано: {}", p.display()),
                            Err(e) => format!("Не вдалося записати файл: {e}"),
                        }))
                    });
                }
                if ui.add_enabled_ui(!self.refreshing, |ui| ghost_btn(ui, Some(Ic::Refresh), "Оновити іконки проєктів")).inner.on_hover_text("Підтягує іконку з Modrinth/CurseForge (потрібен токен або Core API-ключ)").clicked() {
                    self.refreshing = true;
                    let (t, k, ps) = (self.cfg.modrinth_token.trim().to_string(), self.cfg.curse_core_key.trim().to_string(), self.cfg.projects.clone());
                    self.spawn(ctx, move || Msg::Refreshed(refresh_projects(&t, &k, &ps)));
                }
                if ghost_btn(ui, Some(Ic::File), "Імпорт проєктів…").clicked() {
                    self.spawn_opt(ctx, || {
                        let p = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()?;
                        let text = std::fs::read_to_string(p).map_err(|e| e.to_string()).map(|t| t.chars().take(2_000_000).collect::<String>());
                        Some(match text.and_then(|t| parse_projects_file(&t)) {
                            Ok(ps) => Msg::Imported(ps),
                            Err(e) => Msg::Notice(format!("Імпорт не вдався: {e}")),
                        })
                    });
                }
            });
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(title("Безпека"));
            hint(ui, "Токени в конфігу зберігаються відкритим текстом.");
            ui.add_space(8.0);
            if self.confirm_clear {
                banner(ui, Kind::Error, "Видалити всі токени з конфігу? Проєкти залишаться.");
                ui.horizontal(|ui| {
                    if ui.add(danger("Так, очистити")).clicked() {
                        self.cfg.modrinth_token.clear();
                        self.cfg.curse_token.clear();
                        self.cfg.curse_core_key.clear();
                        (self.cfg.modrinth_checked, self.cfg.curse_checked, self.cfg.core_checked) = (0, 0, 0);
                        (self.check_msg, self.cf_msg, self.key_msg) = Default::default();
                        self.confirm_clear = false;
                        *changed = true;
                    }
                    if ghost_btn(ui, Some(Ic::Cross), "Скасувати").clicked() {
                        self.confirm_clear = false;
                    }
                });
            } else if ui.add(danger("Очистити токени")).clicked() {
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
    Refresh,
    Pin,
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
        Ic::Pin => {
            // кнопка-«канцелярська»: голівка, основа й голка
            line(&[(-0.4, -0.85), (0.4, -0.85)]);
            line(&[(-0.28, -0.85), (-0.28, -0.1)]);
            line(&[(0.28, -0.85), (0.28, -0.1)]);
            line(&[(-0.7, -0.1), (0.7, -0.1)]);
            line(&[(0.0, -0.1), (0.0, 0.9)]);
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
        Ic::Grid => {
            for (dx, dy) in [(-0.45, -0.45), (0.45, -0.45), (-0.45, 0.45), (0.45, 0.45)] {
                p.rect_filled(egui::Rect::from_center_size(v(dx, dy), egui::vec2(0.65 * h, 0.65 * h)), 2, col);
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
        if self.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if !self.upd_started {
            self.upd_started = true;
            self.spawn(ctx, || Msg::Update(check_update(), false));
        }
        if !self.centered {
            // 60% монітора, але не менше мінімуму; центр = (монітор − вікно) / 2. Чекаємо, поки egui знає монітор.
            if let Some(mon) = ctx.input(|i| i.viewport().monitor_size) {
                let win = egui::vec2((mon.x * 0.6).max(760.0), (mon.y * 0.6).max(480.0));
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
        for (k, t) in [(egui::Key::Num1, Tab::Projects), (egui::Key::Num2, Tab::Publish), (egui::Key::Num3, Tab::Settings)] {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, k)) {
                self.tab = t; // Ctrl+1/2/3
            }
        }
        // Ctrl+Enter публікує з будь-якого розділу (якщо форма готова і немає діалогу)
        if self.pending == 0 && !self.add_open && self.merge_ask.is_empty() && !self.cfg.projects.is_empty() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            let proj = self.sel.and_then(|i| self.cfg.projects.get(i));
            let (has_mr, has_cf) = proj.map_or((false, false), |p| (!p.modrinth_id.is_empty(), !p.curse_id.is_empty()));
            let (mr, cf) = (has_mr && self.use_mr, has_cf && self.use_cf);
            // з іншого розділу Ctrl+Enter лише відкриває «Публікацію» (видно підсумок), щоб не відправляти наосліп
            let was_publish = self.tab == Tab::Publish;
            self.tab = Tab::Publish;
            if was_publish && self.check(mr, cf).is_ok() {
                self.start_publish(ctx, mr, cf); // інакше причину видно в нижній панелі
            }
        }
        if let Some(f) = ctx.input(|i| i.raw.dropped_files.first().map(|d| d.path().to_path_buf())) {
            self.set_file(f);
            self.tab = Tab::Publish;
        }
        egui::Panel::left("rail").exact_size(if ctx.content_rect().width() < 860.0 { RAIL_W - 16.0 } else { RAIL_W }).resizable(false)
            .frame(egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(12, 14)))
            .show(root, |ui| self.rail(ui));
        if self.tab == Tab::Publish && !self.cfg.projects.is_empty() {
            egui::Panel::bottom("publish_bar")
                .frame(egui::Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(24, 10)))
                .show(root, |ui| self.publish_bar(ui, ctx));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(24))).show(root, |ui| {
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
            self.update_ui(ui, ctx); self.notice_ui(ui, ctx); match self.tab {
            Tab::Projects => self.projects_ui(ui, ctx),
            Tab::Publish => self.publish_ui(ui, ctx),
            Tab::Settings => self.settings_ui(ui, ctx),
        } });
        self.merge_dialog(ctx);
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
            rfd::MessageDialog::new().set_title("Modwire").set_description("Modwire уже запущено.").show();
            return Ok(());
        }
        other => other.ok().flatten(),
    };
    cleanup_update(); // лише під блокуванням: друга копія не має стерти .new, який хтось завантажує
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
    use super::split_url;

    #[test]
    fn split_url_separates_link() {
        assert_eq!(split_url("Готово https://modrinth.com/mod/x/version/1"), ("Готово".into(), Some("https://modrinth.com/mod/x/version/1".into())));
        assert_eq!(split_url("без посилання"), ("без посилання".into(), None));
    }
}
