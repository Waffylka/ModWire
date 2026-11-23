//! Заставка на старті: велике анімоване лого (дріт росте, гілки, вузли, напис, імпульс по дроту),
//! потім воно зменшується й «влітає» в маленьку іконку в рейці, а фон розчиняється.
//!
//! Клік або будь-яка клавіша пропускають очікування (анімація одразу переходить у фінал).
//! `MODWIRE_NO_SPLASH=1` вимикає заставку. `MODWIRE_SPLASH_AT=<секунди>` заморожує її на цьому моменті
//! (для знімків режиму `shots`).

use crate::{ACCENT, BG, CF_ORANGE, MR_GREEN, MUTED, ON_ACCENT, TEXT};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use std::time::{Duration, Instant};

/// Моменти шкали часу, секунди.
const T_PULSE: f32 = 1.5; // старт імпульсу по дроту
const T_PULSE_END: f32 = 2.1;
const T_MORPH: f32 = 2.3; // початок переходу в маленьку іконку
const T_MORPH_LEN: f32 = 0.7;
pub const T_TOTAL: f32 = T_MORPH + T_MORPH_LEN;

pub struct Splash {
    start: Instant,
    id: egui::Id,
    done: bool,
    frozen: Option<f32>,
}

impl Splash {
    pub fn new() -> Self {
        let off = std::env::var_os("MODWIRE_NO_SPLASH").is_some_and(|v| !v.is_empty());
        let frozen = std::env::var("MODWIRE_SPLASH_AT").ok().and_then(|v| v.parse::<f32>().ok());
        // у режимі знімків заставки немає, якщо її явно не заморожено на потрібному моменті
        let shots = std::env::var_os("MODWIRE_SHOT_DIR").is_some_and(|v| !v.is_empty());
        Self { start: Instant::now(), id: egui::Id::unique("splash"), done: off || (shots && frozen.is_none()), frozen }
    }

    pub fn done(&self) -> bool {
        self.done
    }

    /// Малює заставку поверх усього. `target` — де в рейці лежить маленька іконка.
    pub fn show(&mut self, ctx: &egui::Context, target: Option<Rect>) {
        if self.done {
            return;
        }
        let t = self.frozen.unwrap_or_else(|| self.start.elapsed().as_secs_f32());
        if t >= T_TOTAL && self.frozen.is_none() {
            self.done = true;
            return;
        }
        let screen = ctx.content_rect();
        let target = target.unwrap_or_else(|| Rect::from_min_size(Pos2::new(20.0, 22.0), Vec2::splat(32.0)));
        let mut skip = false;
        egui::Area::new(self.id).order(egui::Order::Tooltip).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            skip = resp.clicked() && t < T_MORPH;
            paint(&ui.painter().with_clip_rect(rect), rect, t, target);
        });
        if self.frozen.is_none() {
            skip |= t < T_MORPH && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
            if skip {
                // ранній вихід: одразу до переходу в рейку
                self.start = Instant::now() - Duration::from_secs_f32(T_MORPH);
            }
            ctx.request_repaint_after(Duration::from_millis(16)); // ≈60 кадрів/с, без гонки за кожен vsync
        }
    }
}

// ---------- шкала часу ----------

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// Частка проходження відрізка [a, b] у момент t.
fn seg(t: f32, a: f32, b: f32) -> f32 {
    clamp01((t - a) / (b - a))
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

fn ease_in_out(x: f32) -> f32 {
    if x < 0.5 { 4.0 * x * x * x } else { 1.0 - (-2.0 * x + 2.0).powi(3) / 2.0 }
}

/// З невеликим перельотом: для «вискакування» вузлів.
fn ease_out_back(x: f32) -> f32 {
    let (c1, c3) = (1.70158f32, 2.70158f32);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_pos(a: Pos2, b: Pos2, t: f32) -> Pos2 {
    Pos2::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t))
}

fn lerp_col(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| lerp(x as f32, y as f32, t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

// ---------- геометрія знака (координати 64×64) ----------

/// Знак у двох станах: «велике лого» на темному тлі і «іконка» (на бурштиновій плитці).
#[derive(Clone, Copy)]
struct Geo {
    stem: [Pos2; 2],
    left: [Pos2; 4],
    right: [Pos2; 4],
    nodes: [Pos2; 2],
    node_r: f32,
    base: Pos2,
}

fn p(x: f32, y: f32) -> Pos2 {
    Pos2::new(x, y)
}

fn geo_logo() -> Geo {
    Geo {
        stem: [p(32.0, 57.0), p(32.0, 38.0)],
        left: [p(32.0, 38.0), p(32.0, 28.0), p(20.0, 30.0), p(20.0, 18.0)],
        right: [p(32.0, 38.0), p(32.0, 28.0), p(44.0, 30.0), p(44.0, 18.0)],
        nodes: [p(20.0, 13.0), p(44.0, 13.0)],
        node_r: 6.0,
        base: p(32.0, 58.0),
    }
}

fn geo_icon() -> Geo {
    Geo {
        stem: [p(32.0, 52.0), p(32.0, 37.0)],
        left: [p(32.0, 37.0), p(32.0, 29.0), p(21.0, 31.0), p(21.0, 21.0)],
        right: [p(32.0, 37.0), p(32.0, 29.0), p(43.0, 31.0), p(43.0, 21.0)],
        nodes: [p(21.0, 16.0), p(43.0, 16.0)],
        node_r: 5.5,
        base: p(32.0, 52.0),
    }
}

fn geo_lerp(a: &Geo, b: &Geo, t: f32) -> Geo {
    let l2 = |x: [Pos2; 2], y: [Pos2; 2]| [lerp_pos(x[0], y[0], t), lerp_pos(x[1], y[1], t)];
    let l4 = |x: [Pos2; 4], y: [Pos2; 4]| [0, 1, 2, 3].map(|i| lerp_pos(x[i], y[i], t));
    Geo { stem: l2(a.stem, b.stem), left: l4(a.left, b.left), right: l4(a.right, b.right), nodes: l2(a.nodes, b.nodes), node_r: lerp(a.node_r, b.node_r, t), base: lerp_pos(a.base, b.base, t) }
}

fn bez(c: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let (a, b, cc, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Pos2::new(a * c[0].x + b * c[1].x + cc * c[2].x + d * c[3].x, a * c[0].y + b * c[1].y + cc * c[2].y + d * c[3].y)
}

/// Точка на шляху «стовбур + гілка» за параметром u 0..1 (стовбур займає початок).
fn along(g: &Geo, branch: &[Pos2; 4], u: f32) -> Pos2 {
    const STEM: f32 = 0.42;
    if u < STEM { lerp_pos(g.stem[0], g.stem[1], u / STEM) } else { bez(branch, (u - STEM) / (1.0 - STEM)) }
}

// ---------- малювання ----------

struct Ctx<'a> {
    painter: &'a egui::Painter,
    origin: Pos2,
    k: f32, // пікселів на одиницю макета
}

impl Ctx<'_> {
    fn at(&self, q: Pos2) -> Pos2 {
        self.origin + q.to_vec2() * self.k
    }

    fn dot(&self, q: Pos2, r: f32, col: Color32) {
        if r > 0.0 && col.a() > 0 {
            self.painter.circle_filled(self.at(q), r * self.k, col);
        }
    }

    /// Дріт від початку шляху до частки `to` (товстий штрих із круглими кінцями), 5 одиниць завтовшки.
    fn wire(&self, g: &Geo, which: Option<&[Pos2; 4]>, from: f32, to: f32, col: Color32, width: f32) {
        if to <= from || col.a() == 0 {
            return;
        }
        // одна ламана з круглими кінцями замість сотні кружечків: у рази менше геометрії за кадр
        let n = 36;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let u = from + (to - from) * i as f32 / n as f32;
                self.at(match which {
                    Some(b) => along(g, b, u),
                    None => lerp_pos(g.stem[0], g.stem[1], u),
                })
            })
            .collect();
        self.painter.add(egui::epaint::PathShape::line(pts.clone(), Stroke::new(width * self.k, col)));
        for q in [pts[0], pts[n]] {
            self.painter.circle_filled(q, width / 2.0 * self.k, col);
        }
    }
}

/// Один кадр заставки в момент `t` секунд.
fn paint(painter: &egui::Painter, screen: Rect, t: f32, target: Rect) {
    let morph = ease_in_out(seg(t, T_MORPH, T_MORPH + T_MORPH_LEN));
    // фон розчиняється під кінець переходу, відкриваючи застосунок
    let bg_a = 1.0 - ease_out(seg(t, T_MORPH + 0.2, T_TOTAL));
    painter.rect_filled(screen, 0.0, BG.gamma_multiply(bg_a));

    // розкладка лого по центру; на вузькому вікні зменшуємо
    let fit = ((screen.width() - 80.0) / 640.0).min(1.0);
    let (mark, gap) = (220.0 * fit, 32.0 * fit);
    let word = painter.layout_no_wrap("Modwire".into(), egui::FontId::proportional(84.0 * fit), TEXT);
    let tag_font = egui::FontId::proportional(22.0 * fit);
    let tag = painter.layout_no_wrap("публікація модів".into(), tag_font.clone(), MUTED);
    let text_w = word.size().x.max(tag.size().x);
    let total_w = mark + gap + text_w;
    let left = screen.center().x - total_w / 2.0;
    let mark_rect_start = Rect::from_min_size(Pos2::new(left, screen.center().y - mark / 2.0), Vec2::splat(mark));
    // у переході знак їде з центру в рейку (позиція й розмір)
    let rect = Rect::from_min_max(lerp_pos(mark_rect_start.min, target.min, morph), lerp_pos(mark_rect_start.max, target.max, morph));
    let k = rect.width() / 64.0;
    let cx = Ctx { painter, origin: rect.min, k };

    // плитка іконки проявляється під час переходу
    if morph > 0.0 {
        let r = (16.0 * k).min(255.0) as u8;
        painter.rect_filled(rect, egui::CornerRadius::same(r), ACCENT.gamma_multiply(morph));
    }

    let g = geo_lerp(&geo_logo(), &geo_icon(), morph);
    let ink = ON_ACCENT;
    let wire_col = lerp_col(ACCENT, ink, morph);
    let green = lerp_col(MR_GREEN, ink, morph);
    let orange = lerp_col(CF_ORANGE, ink, morph);

    // основа: з'являється першою і зникає, коли знак стає іконкою
    let base_pop = ease_out_back(seg(t, 0.0, 0.2));
    cx.dot(g.base, 3.5 * base_pop, TEXT.gamma_multiply(1.0 - morph));

    // дріт: стовбур, потім дві гілки
    let stem_p = ease_out(seg(t, 0.05, 0.45));
    let br_p = ease_out(seg(t, 0.35, 0.85));
    // стовбур як окрема лінія; гілки по шляху від стику
    let wire_w = 5.0;
    let stem_len = 0.42;
    cx.wire(&g, None, 0.0, stem_p, wire_col, wire_w);
    for b in [&g.left, &g.right] {
        // гілка: u від STEM до STEM + br_p·(1−STEM)
        cx.wire(&g, Some(b), stem_len, stem_len + br_p * (1.0 - stem_len), wire_col, wire_w);
    }

    // імпульс (світла іскра) біжить по обох гілках
    let pulse = seg(t, T_PULSE, T_PULSE_END);
    if pulse > 0.0 && pulse < 1.0 && morph < 0.3 {
        let head = ease_in_out(pulse) * 1.14;
        let spark = Color32::from_rgb(0xff, 0xf1, 0xd6);
        for b in [&g.left, &g.right] {
            let pts: Vec<Pos2> = (0..=8).map(|i| head - 0.14 * i as f32 / 8.0).filter(|u| (0.0..=1.0).contains(u)).map(|u| cx.at(along(&g, b, u))).collect();
            if pts.len() > 1 && morph < 1.0 {
                painter.add(egui::epaint::PathShape::line(pts, Stroke::new(wire_w * (1.0 - morph) * k, spark)));
            }
        }
    }

    // вузли вискакують з перельотом, а при прибутті імпульсу пульсують
    let ping = if pulse >= 1.0 || t > T_PULSE_END { 1.0 + 0.4 * (1.0 - seg(t, T_PULSE_END, T_PULSE_END + 0.3)).powi(2) * seg(t, T_PULSE_END, T_PULSE_END + 0.1).min(1.0) } else { 1.0 };
    for (i, (pos, col)) in g.nodes.iter().zip([green, orange]).enumerate() {
        let pop = ease_out_back(seg(t, 0.8 + i as f32 * 0.07, 1.1 + i as f32 * 0.07));
        cx.dot(*pos, g.node_r * pop * ping, col);
    }

    // напис і підпис: виїжджають зліва, у переході зникають
    let text_a = 1.0 - seg(t, T_MORPH, T_MORPH + 0.25);
    if text_a > 0.0 {
        let tx = left + mark + gap;
        let ty = screen.center().y - (word.size().y + 8.0 * fit + tag.size().y) / 2.0;
        // «Mod» і «wire» двома фрагментами, щоб «wire» було бурштиновим
        let mod_part = painter.layout_no_wrap("Mod".into(), egui::FontId::proportional(84.0 * fit), TEXT);
        let a1 = ease_out(seg(t, 0.9, 1.3));
        let a2 = ease_out(seg(t, 1.0, 1.4));
        let a3 = ease_out(seg(t, 1.2, 1.5));
        let slide = 24.0 * fit;
        let font = egui::FontId::proportional(84.0 * fit);
        let mod_g = painter.layout_no_wrap("Mod".into(), font.clone(), TEXT.gamma_multiply(a1 * text_a));
        let wire_g = painter.layout_no_wrap("wire".into(), font, ACCENT.gamma_multiply(a2 * text_a));
        painter.galley(Pos2::new(tx - slide * (1.0 - a1), ty), mod_g, TEXT);
        painter.galley(Pos2::new(tx + mod_part.size().x - slide * (1.0 - a2), ty), wire_g, ACCENT);
        let tag_g = painter.layout_no_wrap("публікація модів".into(), tag_font, MUTED.gamma_multiply(a3 * text_a));
        painter.galley(Pos2::new(tx, ty + word.size().y + 8.0 * fit + 8.0 * fit * (1.0 - a3)), tag_g, MUTED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_stays_in_range_and_ends_at_one() {
        for f in [ease_out, ease_in_out] {
            assert_eq!(f(0.0), 0.0);
            assert!((f(1.0) - 1.0).abs() < 1e-6);
            assert!((0..=100).all(|i| (0.0..=1.0).contains(&f(i as f32 / 100.0))));
        }
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-6);
        assert!(ease_out_back(0.8) > 1.0, "перельот");
    }

    #[test]
    fn geometry_lerp_hits_both_ends_and_paths_join() {
        let (a, b) = (geo_logo(), geo_icon());
        assert_eq!(geo_lerp(&a, &b, 0.0).node_r, a.node_r);
        assert_eq!(geo_lerp(&a, &b, 1.0).stem[0], b.stem[0]);
        for g in [a, b] {
            assert_eq!(g.stem[1], g.left[0], "стовбур переходить у гілку");
            assert_eq!(g.stem[1], g.right[0]);
            assert_eq!(along(&g, &g.left, 0.0), g.stem[0]);
            assert!((along(&g, &g.left, 1.0) - g.left[3]).length() < 1e-4);
        }
    }

    #[test]
    fn timeline_is_ordered() {
        const { assert!(T_PULSE < T_PULSE_END && T_PULSE_END < T_MORPH) };
        assert_eq!(lerp_col(Color32::BLACK, Color32::WHITE, 0.5), Color32::from_rgb(128, 128, 128));
    }
}
