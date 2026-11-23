//! Режим знімків для перевірки дизайну без ручного керування вікном.
//! Вмикається лише змінною середовища MODWIRE_SHOT_DIR (разом із MODWIRE_CONFIG_DIR на тестову теку):
//! по черзі показує розділи й діалоги у двох розмірах вікна, зберігає PNG і закриває застосунок.
//! Без змінної `tick` одразу повертається; нічого не пише, у мережу не ходить.
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
    match name {
        "projects" | "projects_tall" => app.tab = Tab::Projects,
        "projects_confirm" => {
            app.tab = Tab::Projects;
            app.confirm_del = Some(0);
        }
        "merge" => {
            app.tab = Tab::Projects;
            app.merge_hidden = false;
            app.merge_ask = vec![(0, super::Project { name: "Aurora Lights".into(), curse_id: "999".into(), ..Default::default() })];
        }
        "publish" | "publish_tall" | "publish_done" => {
            app.cfg.modrinth_token = "fixture".into();
            app.cfg.curse_token = "fixture".into();
            app.tab = Tab::Publish;
            app.select_project(Some(0));
            if app.file.is_none() {
                let f = dir.join("aurora-lights-1.2.0.jar");
                let _ = std::fs::write(&f, b"fixture");
                app.set_file(f);
                app.number = "1.2.0".into();
                app.changelog = "- Нові ефекти світла\n- Виправлено вильоти при завантаженні світу".into();
                app.cfg.last.mc_versions = "1.20.1, 1.20.4".into();
                app.cfg.last.loaders = vec!["Fabric".into(), "Forge".into()];
            }
            if name == "publish_done" {
                app.mr_res = Some(Ok("Версію створено https://modrinth.com/mod/aurora/version/1".into()));
                app.cf_res = Some(Err("Сервер відповів HTTP 403: токен не має права завантажувати файли".into()));
            }
            {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                app.history = vec![
                    serde_json::json!({"ok": true, "project": "Aurora Lights", "number": "1.1.0", "platform": "Modrinth", "t": t - 3600, "msg": ""}),
                    serde_json::json!({"ok": false, "project": "Aurora Lights", "number": "1.1.0", "platform": "CurseForge", "t": t - 3500, "msg": "HTTP 403: токен не має права"}),
                    serde_json::json!({"ok": true, "project": "Stone Tools Plus", "number": "0.9.2", "platform": "Modrinth", "t": t - 86400 * 3, "msg": ""}),
                ];
            }
        }
        "settings" | "settings_tall" => {
            app.cfg.modrinth_token = "fixture".into();
            app.tab = Tab::Settings;
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
    let Some(&(scene, w, h)) = SCENES.get(st.idx) else {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        return;
    };
    if st.frame == 0 {
        let dir = st.dir.clone();
        scene_setup(app, scene, &dir, &mut st.saved_projects);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
    }
    st.frame += 1;
    if st.frame == 30 {
        let slot = st.slot.clone();
        ctx.request_screenshot(move |img| {
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(img);
        });
    }
    let got = st.slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(img) = got {
        let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
        let path = st.dir.join(format!("{scene}_{w}x{h}.png"));
        let _ = image::save_buffer(path, &bytes, img.width() as u32, img.height() as u32, image::ColorType::Rgba8);
        (st.idx, st.frame) = (st.idx + 1, 0);
    } else if st.frame > 400 {
        (st.idx, st.frame) = (st.idx + 1, 0); // не дочекались знімка: йдемо далі
    }
}
