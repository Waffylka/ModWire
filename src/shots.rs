//! Режим знімків: перевірка дизайну без ручного керування вікном. Без змінних середовища нічого не робить.
//!
//! Запуск (PowerShell; ЗАВЖДИ з обома змінними, інакше режим не вмикається, а застосунок візьме справжній конфіг):
//!   $env:MODWIRE_CONFIG_DIR = '<тека з вигаданим config.json: кілька проєктів з Modrinth/CurseForge ID, без токенів>'
//!   $env:MODWIRE_SHOT_DIR   = '<тека для PNG>'
//!   Start-Process <cargo target>\debug\modwire.exe -Wait
//! Необов'язково: MODWIRE_SHOT_SCALE=1.0 (масштаб інтерфейсу, у назві файлу з'явиться @1x),
//!                MODWIRE_SHOT_ONLY=<префікс назви сцени> (напр. `icons`, `publish`).
//! Застосунок по черзі показує сцени зі списку `SCENES` (розділи, діалоги, порожній стан; «tall» — високе вікно замість
//! прокрутки, `icons` — аркуш значків), зберігає `<сцена>_<ш>x<в>.png` і закривається. Знімки йдуть у фізичних
//! пікселях (за масштабу 1.5 — у 1.5 раза більші за назву). У режимі: читання/запис конфігу вимкнено (`readonly`),
//! мережа не використовується (перевірка оновлень і список версій MC пропущені, чипи MC підставлені, токени фіктивні).
//! Нова сцена: рядок у `SCENES` і гілка в `scene_setup`.
use super::{App, Tab};
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

type Slot = Arc<Mutex<Option<Arc<egui::ColorImage>>>>;

struct State {
    dir: PathBuf,
    idx: usize,
    frame: u32,
    slot: Slot,
    saved_projects: Option<Vec<super::Project>>,
}

static STATE: OnceLock<Option<Mutex<State>>> = OnceLock::new();

/// (сцена, ширина, висота у логічних пікселях). «tall» — високе вікно замість прокрутки: видно всю сторінку.
const SCENES: &[(&str, f32, f32)] = &[
    ("projects", 900.0, 580.0), ("projects", 760.0, 480.0),
    ("projects_tall", 900.0, 1000.0), ("projects_tall", 760.0, 1000.0),
    ("projects_confirm", 900.0, 1000.0),
    ("publish", 900.0, 580.0), ("publish", 760.0, 480.0),
    ("publish_tall", 900.0, 2000.0), ("publish_tall", 760.0, 2000.0),
    ("publish_done", 900.0, 580.0), ("publish_done", 760.0, 480.0),
    ("settings", 900.0, 580.0), ("settings", 760.0, 480.0),
    ("settings_tall", 760.0, 1000.0),
    ("add_dialog", 900.0, 580.0), ("add_dialog", 760.0, 480.0),
    ("merge", 900.0, 580.0), ("merge", 760.0, 480.0),
    ("empty", 900.0, 580.0), ("empty", 760.0, 480.0),
    ("settings_update", 900.0, 580.0), ("settings_updating", 900.0, 580.0), ("settings_updateerr", 900.0, 580.0),
    ("publish_busy", 900.0, 580.0), ("publish_busy", 760.0, 480.0),
    ("close_dialog", 900.0, 580.0),
    ("rollback_dialog", 900.0, 580.0),
    ("history_big", 900.0, 580.0), ("history_empty", 900.0, 2100.0), ("history_errors", 900.0, 2700.0),
    ("icons", 900.0, 520.0),
    // видимий фокус: у tick подаємо Tab-и (сцена `<розділ>_focus`)
    ("projects_focus", 900.0, 580.0), ("publish_focus", 900.0, 580.0), ("settings_focus", 900.0, 580.0),
    // великі вікна: перевірка розтягування контенту
    ("projects", 1280.0, 800.0),
    ("publish", 1280.0, 800.0),
    ("publish_done", 1280.0, 800.0),
    ("settings", 1280.0, 800.0),
    ("add_dialog", 1280.0, 800.0),
    ("projects", 1600.0, 1000.0),
    ("publish", 1600.0, 1000.0),
    ("publish_done", 1600.0, 1000.0),
    ("settings", 1600.0, 1000.0),
    ("add_dialog", 1600.0, 1000.0),
];

fn state() -> Option<&'static Mutex<State>> {
    STATE
        .get_or_init(|| {
            let dir = PathBuf::from(std::env::var_os("MODWIRE_SHOT_DIR").filter(|v| !v.is_empty())?);
            std::env::var_os("MODWIRE_CONFIG_DIR").filter(|v| !v.is_empty())?; // без тестової теки не запускаємося
            std::fs::create_dir_all(&dir).ok()?;
            Some(Mutex::new(State { dir, idx: 0, frame: 0, slot: Default::default(), saved_projects: None }))
        })
        .as_ref()
}

fn scene_setup(app: &mut App, name: &str, dir: &std::path::Path, saved: &mut Option<Vec<super::Project>>) {
    if let Some(p) = saved.take() {
        app.cfg.projects = p; // повертаємо проєкти після порожнього стану
    }
    app.add_open = false;
    app.merge_ask.clear();
    app.confirm_del = None;
    app.cfg.modrinth_token.clear(); // діалог «Додати проєкт» сам тягне список, якщо є токен
    app.cfg.curse_token.clear();
    (app.mr_res, app.cf_res, app.msg) = (None, None, String::new());
    app.notice.clear();
    app.start_act = super::StartAction::Clean;
    (app.hist_q, app.hist_fail, app.hist_all) = (String::new(), false, false);
    (app.update, app.upd_applying, app.upd_err, app.pending, app.confirm_close, app.featured) = (None, false, String::new(), 0, false, false);
    match name {
        "projects" | "projects_tall" | "projects_focus" => app.tab = Tab::Projects,
        "projects_confirm" => {
            app.tab = Tab::Projects;
            app.confirm_del = Some(0);
        }
        "merge" => {
            app.tab = Tab::Projects;
            app.merge_hidden = false;
            app.merge_ask = vec![(0, super::Project { name: "Aurora Lights".into(), curse_id: "999".into(), ..Default::default() })];
        }
        "publish" | "publish_focus" | "publish_tall" | "publish_done" | "publish_busy" | "close_dialog" | "rollback_dialog" | "history_big" | "history_empty" | "history_errors" => {
            app.cfg.modrinth_token = "fixture".into();
            app.cfg.curse_token = "fixture".into();
            app.tab = Tab::Publish;
            app.select_project(Some(0));
            // сцени «history_*» мають показувати сам розділ історії, а не крок «Проєкт» (історія порожня лише в history_empty)
            app.pub_step = if name.starts_with("history") && name != "history_empty" { 6 } else { 0 };
            if name == "publish_tall" { app.pub_step = 5; }
            if app.file.is_none() {
                let f = dir.join("aurora-lights-1.2.0.jar");
                let _ = std::fs::write(&f, b"fixture");
                app.set_file(f, None);
                app.number = "1.2.0".into();
                app.changelog = "- Нові ефекти світла\n- Виправлено вильоти при завантаженні світу".into();
                app.cfg.last.mc_versions = "1.20.1, 1.20.4".into();
                app.cfg.last.loaders = vec!["fabric".into(), "forge".into()];
            }
            app.featured = name == "publish_tall";
            // після set_file (він вмикає зведення): повна форма показує зведення, а майстер (крок «Куди публікувати» з розкритими додатками) знімаємо окремими сценами
            app.edit_mode = matches!(name, "publish_tall" | "publish_focus");
            if name == "publish_busy" || name == "close_dialog" {
                use std::sync::atomic::Ordering::Relaxed;
                app.pending = 2;
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
                for (up, sent) in [(&app.up_mr, 62_000_000u64), (&app.up_cf, 18_000_000u64)] {
                    up.active.store(true, Relaxed);
                    up.prog.total.store(100_000_000, Relaxed);
                    up.prog.sent.store(sent, Relaxed);
                    up.prog.started_ms.store(now.saturating_sub(8_000), Relaxed);
                }
                app.confirm_close = name == "close_dialog";
            }
            if name == "publish_done" {
                app.mr_res = Some(Ok("Версію створено https://modrinth.com/mod/aurora/version/1".into()));
                app.cf_res = Some(Err("Сервер відповів HTTP 403: токен не має права завантажувати файли".into()));
            }
            if name == "rollback_dialog" {
                app.start_act = super::StartAction::OfferRollback;
            }
            if name != "history_empty" {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                app.history = vec![
                    serde_json::json!({"ok": true, "project": "Aurora Lights", "number": "1.1.0", "platform": "Modrinth", "t": t - 3600, "msg": ""}),
                    serde_json::json!({"ok": false, "project": "Aurora Lights", "number": "1.1.0", "platform": "CurseForge", "t": t - 3500, "msg": "HTTP 403: токен не має права"}),
                    serde_json::json!({"ok": true, "project": "Stone Tools Plus", "number": "0.9.2", "platform": "Modrinth", "t": t - 86400 * 3, "msg": ""}),
                ];
                if name == "history_big" {
                    // перевірка швидкості: повний ліміт історії в пам'яті, усі рядки розкриті
                    app.hist_all = true;
                    for k in 0..300u64 {
                        app.history.push(serde_json::json!({"ok": k % 4 != 0, "project": format!("Проєкт {k}"), "number": format!("1.{k}.0"), "platform": if k % 2 == 0 { "Modrinth" } else { "CurseForge" }, "t": t - 600 * (k + 1), "msg": if k % 4 != 0 { "" } else { "HTTP 403: токен не має права" }}));
                    }
                }
                if name == "history_errors" {
                    let long = "HTTP 400: поле game_versions містить версію 1.20.9, якої не існує на Modrinth; перевірте список версій Minecraft і спробуйте ще раз після виправлення (довге повідомлення для перевірки переносу).";
                    for k in 0..8u64 {
                        app.history.push(serde_json::json!({"ok": k % 3 == 0, "project": "Дуже довга назва мода для перевірки переносу в історії", "number": format!("2.{k}.0-beta.{k}"), "platform": if k % 2 == 0 { "Modrinth" } else { "CurseForge" }, "t": t - 7200 * (k + 1), "msg": if k % 3 == 0 { "" } else { long }}));
                    }
                }
            } else {
                app.history.clear();
            }
        }
        "settings" | "settings_focus" | "settings_tall" | "settings_update" | "settings_updating" | "settings_updateerr" => {
            app.cfg.modrinth_token = "fixture".into();
            app.tab = Tab::Settings;
            if name.starts_with("settings_upd") {
                app.update = Some(super::Update { version: "0.4.0".into(), url: "https://example.invalid/modwire.exe".into(), size: 0, sha256: String::new(), page: "https://example.invalid/release".into() });
                app.upd_applying = name == "settings_updating";
                if app.upd_applying {
                    super::set_update_progress(7 << 20, 12 << 20);
                }
                if name == "settings_updateerr" {
                    app.upd_err = "Не вдалося встановити оновлення: немає прав на запис у теку програми.".into();
                }
            }
        }
        "add_dialog" => {
            app.tab = Tab::Projects;
            app.add_open = true;
        }
        "empty" => {
            app.tab = Tab::Projects;
            *saved = Some(std::mem::take(&mut app.cfg.projects));
        }
        _ => {}
    }
}

/// Викликається щокадру на початку `App::ui`.
pub fn tick(app: &mut App, ctx: &egui::Context) {
    let Some(st) = state() else { return };
    let mut st = st.lock().unwrap_or_else(|e| e.into_inner());
    // жодних мереж, збережень і автоцентрування
    app.readonly = true;
    app.upd_started = true;
    app.centered = true;
    app.mc_known = ["1.21.1", "1.20.6", "1.20.4", "1.20.1"].map(String::from).to_vec();
    app.mc_all = app.mc_known.clone();
    ctx.request_repaint();
    // MODWIRE_SHOT_SCALE=1.0 — масштаб інтерфейсу (за замовчуванням системний); MODWIRE_SHOT_ONLY=icons — лише сцени з цією назвою
    let scale = std::env::var("MODWIRE_SHOT_SCALE").ok().and_then(|v| v.parse::<f32>().ok());
    if let Some(sc) = scale && (ctx.pixels_per_point() - sc).abs() > 0.001 {
        ctx.set_pixels_per_point(sc);
    }
    if let Ok(only) = std::env::var("MODWIRE_SHOT_ONLY") {
        while SCENES.get(st.idx).is_some_and(|s| !s.0.starts_with(only.as_str())) {
            st.idx += 1;
        }
    }
    let Some(&(scene, w, h)) = SCENES.get(st.idx) else {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        return;
    };
    if st.frame == 0 {
        let dir = st.dir.clone();
        scene_setup(app, scene, &dir, &mut st.saved_projects);
        ctx.memory_mut(|m| if let Some(id) = m.focused() { m.surrender_focus(id) }); // фокус із попередньої сцени не тягнемо
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
    }
    if scene == "icons" {
        icon_sheet(ctx);
    }
    st.frame += 1;
    if scene.ends_with("_focus") && [6, 10, 14, 18, 22].contains(&st.frame) {
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::Next));
    }
    if st.frame == 30 {
        let slot = st.slot.clone();
        ctx.request_screenshot(move |img| {
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(img);
        });
    }
    let got = st.slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(img) = got {
        let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
        let path = st.dir.join(format!("{scene}_{w}x{h}{}.png", scale.map_or(String::new(), |s| format!("@{s}x"))));
        let _ = image::save_buffer(path, &bytes, img.width() as u32, img.height() as u32, image::ColorType::Rgba8);
        (st.idx, st.frame) = (st.idx + 1, 0);
    } else if st.frame > 400 {
        (st.idx, st.frame) = (st.idx + 1, 0); // не дочекались знімка: йдемо далі
    }
}

/// Аркуш усіх значків у розмірах 12–32 px і зразки кнопок/бейджів/банера для перевірки вирівнювання.
fn icon_sheet(ctx: &egui::Context) {
    use super::{draw_icon, ghost_btn, icon_btn, Ic, BG, MUTED, TEXT};
    let all = [Ic::Plus, Ic::Check, Ic::Cross, Ic::Upload, Ic::Gear, Ic::Trash, Ic::External, Ic::Eye, Ic::Warn, Ic::Info, Ic::Grid, Ic::File, Ic::Chevron, Ic::Refresh, Ic::Pin, Ic::ChevronUp, Ic::Copy];
    egui::Area::new(egui::Id::NULL.with("shots_icons")).order(egui::Order::Foreground).fixed_pos(egui::pos2(0.0, 0.0)).show(ctx, |ui| {
        ui.painter().rect_filled(ctx.content_rect(), 0, BG);
        let mut y = 28.0;
        for size in [12.0, 14.0, 16.0, 20.0, 32.0] {
            for (i, ic) in all.iter().enumerate() {
                draw_icon(ui.painter(), egui::pos2(30.0 + i as f32 * 46.0, y), *ic, size, TEXT);
            }
            ui.painter().text(egui::pos2(880.0, y), egui::Align2::RIGHT_CENTER, format!("{size}px"), egui::FontId::proportional(12.0), MUTED);
            y += size.max(24.0) + 18.0;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(egui::pos2(24.0, y + 4.0), egui::vec2(850.0, 200.0))), |ui| {
            ui.horizontal(|ui| {
                for (ic, t) in [(Ic::Plus, "Додати"), (Ic::Upload, "Файл"), (Ic::Trash, "Видалити"), (Ic::Refresh, "Оновити"), (Ic::External, "Відкрити")] {
                    ghost_btn(ui, Some(ic), t);
                }
                icon_btn(ui, Some(Ic::Check), "Перевірити", true);
            });
            ui.add_space(8.0);
            super::badge_ic(ui, "Готово", super::SUCCESS, Some(Ic::Check));
            ui.add_space(4.0);
            super::banner(ui, super::Kind::Warn, "Попередження з довгим текстом, щоб побачити вирівнювання значка по першому рядку.");
            ui.add_space(4.0);
            super::banner(ui, super::Kind::Error, "Помилка");
        });
    });
}
