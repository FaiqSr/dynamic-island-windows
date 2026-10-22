use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, TextFormat, Ui,
    Vec2,
};
use eframe::egui::TextureHandle;
use eframe::epaint::Galley;
use eframe::epaint::text::LayoutJob;

use crate::animation::Spring;

pub const ACCENT: Color32 = Color32::from_rgb(242, 139, 112);
pub const MUSIC_ACCENT: Color32 = Color32::from_rgb(111, 205, 189);
pub const TIMER_ACCENT: Color32 = Color32::from_rgb(247, 190, 93);
pub const SHELF_ACCENT: Color32 = Color32::from_rgb(123, 176, 245);
pub const SHORTCUT_ACCENT: Color32 = Color32::from_rgb(190, 151, 235);
pub const ALERT_ACCENT: Color32 = Color32::from_rgb(245, 125, 145);
pub const TEXT: Color32 = Color32::from_rgb(244, 241, 236);
pub const MUTED: Color32 = Color32::from_rgb(145, 143, 141);
pub const TRACK: Color32 = Color32::from_rgb(48, 46, 45);
pub const PILL: Color32 = Color32::from_rgb(37, 35, 34);
const SURFACE: Color32 = Color32::from_rgb(20, 19, 19);

pub const IDLE_SIZE: Vec2 = Vec2::new(280.0, 46.0);
pub const EXPANDED_SIZE: Vec2 = Vec2::new(560.0, 360.0);
/// Breathing room between the island edge and its content.
pub const PAD: f32 = 20.0;

fn lerp(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    a + (b - a) * t
}

/// Animates the island between its idle and expanded shapes.
pub fn morph(ui: &mut Ui, spring: &mut Spring, expanded: bool, visible: bool) -> Rect {
    let target = if expanded {
        1.0
    } else if visible {
        0.35
    } else {
        0.0
    };
    let t = spring.animate_to(target, 14.0);

    let size = if t <= 0.35 {
        lerp(Vec2::ZERO, IDLE_SIZE, t / 0.35)
    } else {
        lerp(IDLE_SIZE, EXPANDED_SIZE, (t - 0.35) / 0.65)
    };
    let radius = if t <= 0.35 {
        size.y * 0.5
    } else {
        egui::lerp(IDLE_SIZE.y * 0.5..=22.0, (t - 0.35) / 0.65)
    }
    .min(size.min_elem() * 0.5);
    let available = ui.max_rect();
    let outer = Rect::from_min_size(
        egui::pos2(available.center().x - size.x * 0.5, available.top()),
        size,
    );

    let painter = ui.painter().clone().with_clip_rect(ui.clip_rect());
    painter.rect_filled(
        outer.translate(Vec2::new(0.0, 5.0)),
        radius,
        Color32::from_black_alpha(72),
    );
    if t > 0.001 {
        surface(&painter, outer, radius, SURFACE, 1.0);
        painter.rect_stroke(
            outer,
            radius,
            Stroke::new(1.0, Color32::from_white_alpha(16)),
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [
                outer.left_top() + Vec2::new(radius, 1.0),
                outer.right_top() + Vec2::new(-radius, 1.0),
            ],
            Stroke::new(1.0, Color32::from_white_alpha(10)),
        );
    }

    ui.advance_cursor_after_rect(outer);
    outer
}

/// Button symbols used by the island controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Icon {
    Close,
    Previous,
    Play,
    Pause,
    Add,
    Repeat,
    /// Chevron pointing left, used for back navigation.
    ArrowLeft,
    /// Chevron pointing right, used for forward navigation.
    ArrowRight,
    /// A sheet of paper with a folded corner, used for shelved files.
    File,
    /// Speaker with sound waves, used for the volume slider.
    VolumeOn,
    /// Speaker with a cross instead of waves, used for the muted state.
    VolumeOff,
    /// A folder, used to pick a working directory for a shortcut.
    Folder,
    /// A terminal prompt, used for the empty-shortcuts placeholder.
    Terminal,
    /// Compatibility path for existing callers and uncommon symbols.
    Glyph(String),
}

impl From<&str> for Icon {
    fn from(glyph: &str) -> Self {
        match glyph {
            "\u{00d7}" => Self::Close,
            "\u{23ee}" => Self::Previous,
            "\u{23f8}" => Self::Pause,
            "\u{25b6}" => Self::Play,
            "\u{21ba}" => Self::Repeat,
            "+" => Self::Add,
            "file" => Self::File,
            _ => Self::Glyph(glyph.to_owned()),
        }
    }
}

/// Icon button with a circular hover highlight and hand-drawn standard icons.
pub fn icon_button(ui: &mut Ui, icon: impl Into<Icon>, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.circle_filled(rect.center(), size * 0.5, Color32::from_white_alpha(28));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    paint_icon(painter, rect.center(), icon.into(), size * 0.58, TEXT);
    response
}

/// Notification categories with matching hand-drawn glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationKind {
    Star,
    Info,
    Warning,
    Success,
    Error,
    Music,
    Timer,
}

/// Paints a shared dark surface and restrained offset shadow for islands and toasts.
pub fn surface(painter: &Painter, rect: Rect, radius: f32, fill: Color32, opacity: f32) {
    let opacity = opacity.clamp(0.0, 1.0);
    painter.rect_filled(rect, radius, fill.gamma_multiply(opacity));
}

/// A round icon button that grows slightly on hover, used for the compact nav controls.
pub fn nav_button(ui: &mut Ui, icon: impl Into<Icon>, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hovered = response.hovered();
    let radius = size * 0.5;
    let painter = ui.painter();
    painter.circle_filled(
        rect.center(),
        radius,
        Color32::from_white_alpha(if hovered { 26 } else { 12 }),
    );
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    paint_icon(
        painter,
        rect.center(),
        icon.into(),
        size * 0.42,
        if hovered { TEXT } else { MUTED },
    );
    response
}

/// Paints a notification glyph without relying on font-specific Unicode symbols.
pub fn notification_glyph(
    painter: &Painter,
    center: Pos2,
    kind: NotificationKind,
    size: f32,
    color: Color32,
) {
    paint_icon(
        painter,
        center,
        match kind {
            NotificationKind::Star => Icon::Glyph("star".into()),
            NotificationKind::Info => Icon::Glyph("info".into()),
            NotificationKind::Warning => Icon::Glyph("warning".into()),
            NotificationKind::Success => Icon::Glyph("success".into()),
            NotificationKind::Error => Icon::Glyph("error".into()),
            NotificationKind::Music => Icon::Glyph("music".into()),
            NotificationKind::Timer => Icon::Glyph("timer".into()),
        },
        size,
        color,
    );
}

/// Body of the speaker shared by [`Icon::VolumeOn`] and [`Icon::VolumeOff`]: a small box with
/// a cone opening to the right.
fn speaker(painter: &Painter, center: Pos2, s: f32, color: Color32) {
    let body = Rect::from_min_max(
        egui::pos2(center.x - s * 0.95, center.y - s * 0.35),
        egui::pos2(center.x - s * 0.5, center.y + s * 0.35),
    );
    painter.rect_filled(body, 0.5, color);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(center.x - s * 0.5, center.y - s * 0.35),
            egui::pos2(center.x + s * 0.42, center.y - s * 0.85),
            egui::pos2(center.x + s * 0.42, center.y + s * 0.85),
            egui::pos2(center.x - s * 0.5, center.y + s * 0.35),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Paints one of the hand-drawn icons at an arbitrary position, for callers that build
/// their own layout instead of using [`icon_button`].
pub fn paint_icon(painter: &Painter, center: Pos2, icon: Icon, size: f32, color: Color32) {
    let s = size * 0.5;
    let stroke = Stroke::new((size * 0.09).max(1.5), color);
    let line = |a, b| painter.line_segment([a, b], stroke);
    match icon {
        Icon::Close => {
            line(center + Vec2::new(-s, -s), center + Vec2::new(s, s));
            line(center + Vec2::new(s, -s), center + Vec2::new(-s, s));
        }
        Icon::Previous => {
            line(
                center + Vec2::new(-s * 0.55, -s),
                center + Vec2::new(-s * 0.55, s),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(-s * 0.25, 0.0),
                    center + Vec2::new(s, -s),
                    center + Vec2::new(s, s),
                ],
                color,
                Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(-s, 0.0),
                    center + Vec2::new(s * 0.25, -s),
                    center + Vec2::new(s * 0.25, s),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(-s * 0.65, -s),
                    center + Vec2::new(s, 0.0),
                    center + Vec2::new(-s * 0.65, s),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            painter.rect_filled(
                Rect::from_center_size(
                    center + Vec2::new(-s * 0.4, 0.0),
                    Vec2::new(s * 0.42, size),
                ),
                1.5,
                color,
            );
            painter.rect_filled(
                Rect::from_center_size(center + Vec2::new(s * 0.4, 0.0), Vec2::new(s * 0.42, size)),
                1.5,
                color,
            );
        }
        Icon::Add => {
            line(center + Vec2::new(-s, 0.0), center + Vec2::new(s, 0.0));
            line(center + Vec2::new(0.0, -s), center + Vec2::new(0.0, s));
        }
        Icon::Repeat => {
            painter.circle_stroke(center, s * 0.72, stroke);
            painter.circle_filled(
                center + Vec2::new(s * 0.58, -s * 0.42),
                stroke.width * 1.5,
                color,
            );
            painter.circle_filled(
                center + Vec2::new(-s * 0.58, s * 0.42),
                stroke.width * 1.5,
                color,
            );
        }
        Icon::ArrowLeft => {
            line(
                center + Vec2::new(s * 0.55, -s * 0.72),
                center + Vec2::new(-s * 0.55, 0.0),
            );
            line(
                center + Vec2::new(-s * 0.55, 0.0),
                center + Vec2::new(s * 0.55, s * 0.72),
            );
        }
        Icon::ArrowRight => {
            line(
                center + Vec2::new(-s * 0.55, -s * 0.72),
                center + Vec2::new(s * 0.55, 0.0),
            );
            line(
                center + Vec2::new(s * 0.55, 0.0),
                center + Vec2::new(-s * 0.55, s * 0.72),
            );
        }
        Icon::File => {
            // A sheet of paper with a folded top-right corner.
            let left = center.x - s * 0.62;
            let right = center.x + s * 0.62;
            let top = center.y - s;
            let bottom = center.y + s;
            let fold = s * 0.65;
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(left, top),
                    egui::pos2(right - fold, top),
                    egui::pos2(right, top + fold),
                    egui::pos2(right, bottom),
                    egui::pos2(left, bottom),
                    egui::pos2(left, top),
                ],
                stroke,
            ));
        }
        Icon::VolumeOn => {
            speaker(painter, center, s, color);
            // Two arcs opening to the right: sound coming out of the speaker.
            for (radius, extent) in [(s * 0.45, 0.55), (s * 0.8, 0.5)] {
                let right = center.x + s * 0.42;
                let steps = 8;
                let points: Vec<_> = (0..=steps)
                    .map(|i| {
                        let t = i as f32 / steps as f32;
                        let angle = -extent + 2.0 * extent * t;
                        egui::pos2(
                            right + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(points, stroke));
            }
        }
        Icon::VolumeOff => {
            speaker(painter, center, s, color);
            // Cross in the space the waves would occupy.
            let origin = center + Vec2::new(s * 0.72, 0.0);
            let arm = s * 0.3;
            line(
                origin + Vec2::new(-arm, -arm),
                origin + Vec2::new(arm, arm),
            );
            line(
                origin + Vec2::new(arm, -arm),
                origin + Vec2::new(-arm, arm),
            );
        }
        Icon::Folder => {
            // A tab over the left of a rounded body, the classic folder silhouette.
            let left = center.x - s;
            let right = center.x + s;
            let top = center.y - s * 0.5;
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(left, center.y + s * 0.75),
                    egui::pos2(left, top),
                    egui::pos2(center.x - s * 0.2, top),
                    egui::pos2(center.x + s * 0.05, center.y - s * 0.15),
                    egui::pos2(right, center.y - s * 0.15),
                    egui::pos2(right, center.y + s * 0.75),
                    egui::pos2(left, center.y + s * 0.75),
                ],
                stroke,
            ));
        }
        Icon::Terminal => {
            // A prompt: the ">_" of a terminal, framed by the two chevrons and an underscore.
            line(
                center + Vec2::new(-s * 0.75, -s * 0.4),
                center + Vec2::new(-s * 0.2, 0.0),
            );
            line(
                center + Vec2::new(-s * 0.2, 0.0),
                center + Vec2::new(-s * 0.75, s * 0.4),
            );
            line(
                center + Vec2::new(s * 0.05, s * 0.45),
                center + Vec2::new(s * 0.75, s * 0.45),
            );
        }
        Icon::Glyph(glyph) => {
            let paths: &[&[(f32, f32)]] = match glyph.as_str() {
                "star" => &[&[
                    (0.0, -1.0),
                    (0.22, -0.3),
                    (0.95, -0.3),
                    (0.36, 0.15),
                    (0.58, 0.9),
                    (0.0, 0.45),
                    (-0.58, 0.9),
                    (-0.36, 0.15),
                    (-0.95, -0.3),
                    (-0.22, -0.3),
                    (0.0, -1.0),
                ]],
                "info" => &[&[(0.0, -0.95), (0.0, -0.9)], &[(0.0, -0.4), (0.0, 0.75)]],
                "warning" => &[
                    &[(-0.9, 0.75), (0.0, -0.95), (0.9, 0.75), (-0.9, 0.75)],
                    &[(0.0, -0.35), (0.0, 0.2)],
                ],
                "success" => &[&[(-0.85, 0.0), (-0.2, 0.65), (0.9, -0.7)]],
                "error" => &[&[(-0.7, -0.7), (0.7, 0.7)], &[(0.7, -0.7), (-0.7, 0.7)]],
                "music" => &[
                    &[
                        (0.2, -0.9),
                        (0.2, 0.45),
                        (0.8, 0.3),
                        (0.8, -0.7),
                        (0.2, -0.9),
                    ],
                    &[(0.2, -0.5), (0.8, -0.65)],
                ],
                "timer" => &[&[(0.0, -0.8), (0.0, 0.05), (0.55, 0.35)]],
                _ => {
                    painter.text(
                        center,
                        Align2::CENTER_CENTER,
                        glyph,
                        FontId::proportional(size * 0.8),
                        color,
                    );
                    return;
                }
            };
            for path in paths {
                let points: Vec<_> = path
                    .iter()
                    .map(|(x, y)| center + Vec2::new(x * s, y * s))
                    .collect();
                painter.add(egui::Shape::line(points, stroke));
            }
            if glyph == "info" {
                painter.circle_filled(
                    center + Vec2::new(0.0, -s * 0.8),
                    stroke.width * 0.65,
                    color,
                );
            } else if glyph == "timer" {
                painter.circle_stroke(center, s * 0.72, stroke);
            }
        }
    }
}

pub fn label(ui: &mut Ui, text: &str, size: f32, color: Color32) -> Response {
    ui.label(egui::RichText::new(text).size(size).color(color))
}

/// Single-line text that never exceeds `max_width`.
///
/// `label` wraps at the available width and, once wrapped, a word that is still too wide
/// (a long track title, a file name, ...) simply overflows the row and bleeds out of the
/// island. Here the text is clipped to the space it may use, and the ellipsis comes from
/// [`fit_to`].
pub fn truncate(ui: &mut Ui, text: &str, size: f32, color: Color32, max_width: f32) -> Response {
    let font = FontId::proportional(size);
    let galley = fit_to(ui, text, &font, color, max_width);
    ui.add(egui::Label::new(galley).selectable(false))
}

/// Lays out `text` on a single line inside `max_width`, adding an ellipsis when it does not
/// fit, so long track titles can never bleed past the island padding.
pub fn fit_to(ui: &Ui, text: &str, font: &FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    ui.painter().layout_job(job)
}

/// Animated equalizer bars; flat when paused.
pub fn equalizer(ui: &mut Ui, playing: bool, height: f32) -> Response {
    let width = height * 1.5;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    if !playing {
        return response;
    }
    let painter = ui.painter();
    let time = ui.input(|i| i.time) as f32;
    let bars = 4;
    let step = rect.width() / (bars as f32 * 2.0);
    for i in 0..bars {
        let phase = time * 6.0 + i as f32 * 1.3;
        let amp = 0.35 + 0.65 * (phase.sin() * 0.5 + 0.5);
        let h = height * amp;
        let x = rect.left() + step * 0.5 + i as f32 * step * 2.0;
        let bar = Rect::from_min_max(
            egui::pos2(x, rect.bottom() - h),
            egui::pos2(x + step, rect.bottom()),
        );
        painter.rect_filled(bar, step * 0.5, ACCENT);
    }
    response
}

/// Scrub bar. Returns the clicked ratio, if any.
pub fn progress_bar_color(
    ui: &mut Ui,
    ratio: f32,
    height: f32,
    accent: Color32,
) -> (Response, Option<f32>) {
    // The hit area is taller than the visible line so it is easy to click, but it must not add
    // extra vertical space between rows. It is also clamped to the island width so it never
    // draws past the right padding.
    let hit = (height + 8.0).max(14.0);
    let width = ui.available_width().max(16.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, hit), Sense::click());
    let line = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), height));
    let painter = ui.painter();
    painter.rect_filled(line, height * 0.5, TRACK);
    let filled = Rect::from_min_size(
        line.min,
        Vec2::new(line.width() * ratio.clamp(0.0, 1.0), height),
    );
    painter.rect_filled(filled, height * 0.5, accent);

    let mut seek = None;
    if let Some(pos) = response.interact_pointer_pos() {
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            // Keep the knob fully inside the bar so it cannot poke out at either end.
            let knob = egui::pos2(
                pos.x
                    .clamp(line.left() + height * 1.2, line.right() - height * 1.2),
                line.center().y,
            );
            painter.circle_filled(knob, height * 1.2, Color32::WHITE);
        }
        if response.clicked() {
            seek = Some(((pos.x - line.left()) / line.width()).clamp(0.0, 1.0));
        }
    }
    (response, seek)
}

/// Volume slider `width` points wide. Dragging it also picks up a click anywhere on the track,
/// so the bar behaves like the native mixer rather than requiring a precise grab. Returns the
/// new ratio when it changed this frame.
pub fn volume_slider_color(ui: &mut Ui, ratio: f32, width: f32, accent: Color32) -> Option<f32> {
    // Same hit-area trick as `progress_bar`: tall enough to grab, without adding height.
    let height = 4.0;
    let hit = 16.0;
    let width = width.max(16.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, hit), Sense::click_and_drag());
    let line = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), height));
    let ratio = ratio.clamp(0.0, 1.0);
    let painter = ui.painter();
    painter.rect_filled(line, height * 0.5, TRACK);
    painter.rect_filled(
        Rect::from_min_size(line.min, Vec2::new(line.width() * ratio, height)),
        height * 0.5,
        if ratio > 0.0 { accent } else { TRACK },
    );

    let mut changed = None;
    if (response.hovered() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let knob = egui::pos2(
            pos.x
                .clamp(line.left() + height * 1.2, line.right() - height * 1.2),
            line.center().y,
        );
        painter.circle_filled(knob, height * 1.4, Color32::WHITE);
        if response.clicked() || response.dragged() {
            changed = Some(((pos.x - line.left()) / line.width()).clamp(0.0, 1.0));
        }
    } else {
        painter.circle_filled(
            egui::pos2(line.left() + line.width() * ratio, line.center().y),
            height * 1.1,
            Color32::WHITE,
        );
    }
    changed
}

/// Number of columns a desktop-style grid fits in `width` for tiles `tile` wide.
///
/// The count is at least one so a narrow island still lays out, and the tiles that come back
/// from [`grid_cell`] are sized to fill the width exactly rather than leaving a ragged edge.
pub fn grid_columns(width: f32, tile: f32) -> usize {
    ((width / tile).floor() as usize).max(1)
}

/// One tile of a grid: icon above a caption, highlighted on hover. Returns `(rect, response)`
/// so the caller can paint whatever the tile represents inside `rect`.
pub fn grid_cell(ui: &mut Ui, columns: usize, tile: f32, height: f32) -> (Rect, Response) {
    let columns = columns.max(1) as f32;
    let cell = Vec2::new((ui.available_width() / columns).max(0.0), height);
    let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
    // Hit area is the visual tile, centred in its cell, so the gap between tiles belongs to
    // neither and a click in the gutter does nothing.
    let inner = Rect::from_center_size(rect.center(), Vec2::new(tile, height));
    if response.hovered() {
        ui.painter()
            .rect_filled(inner, 12.0, Color32::from_white_alpha(16));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (inner, response)
}

/// Draws a real file icon into the square `rect`, falling back to a monogram tile when the
/// platform has no icon for the file.
///
/// `fallback` is painted instead of nothing so a grid never has holes: the first letter of the
/// name on a tinted rounded square, the way a file manager does it.
pub fn file_tile(
    ui: &mut Ui,
    rect: Rect,
    texture: Option<&TextureHandle>,
    label: &str,
    tint: Color32,
) {
    let painter = ui.painter();
    match texture {
        Some(texture) => {
            // Keep the aspect ratio; shell icons are square but extensions are not guaranteed.
            let side = rect.width().min(rect.height());
            let target =
                Rect::from_center_size(rect.center(), Vec2::new(side, side)).shrink(1.0);
            painter.image(
                texture.id(),
                target,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            let square = Rect::from_center_size(
                rect.center(),
                Vec2::splat(rect.width().min(rect.height())),
            )
            .shrink(2.0);
            painter.rect_filled(square, 8.0, tint.gamma_multiply(0.35));
            painter.text(
                square.center(),
                Align2::CENTER_CENTER,
                monogram(label),
                FontId::proportional(square.width() * 0.42),
                tint,
            );
        }
    }
}

/// First character of `label`, upper-cased, or `?` when there is nothing to show.
fn monogram(label: &str) -> String {
    label
        .trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

/// Small rounded tag. Never wider than the row it lives in.
pub fn pill(ui: &mut Ui, text: &str) -> Response {
    let color = Color32::from_rgb(200, 200, 210);
    let padding = Vec2::new(10.0, 4.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(11.0), color);
    let max_width = (ui.available_width() - padding.x * 2.0).max(8.0);
    let galley = if galley.size().x > max_width {
        fit_to(ui, text, &FontId::proportional(11.0), color, max_width)
    } else {
        galley
    };
    let (rect, response) = ui.allocate_exact_size(galley.size() + padding * 2.0, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, rect.height() * 0.5, PILL);
    // Nudge by half a pixel so the cap height reads as vertically centred in the pill.
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            (rect.center().y - galley.size().y * 0.5 - 0.5).round(),
        ),
        galley,
        color,
    );
    response
}
