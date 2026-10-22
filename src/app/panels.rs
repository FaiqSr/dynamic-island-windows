use eframe::egui::{self, Align, Color32, FontId, Frame, Layout, Margin, Rect, Sense, Vec2};

use super::DynamicIsland;
use crate::notifications::NotificationView;
use crate::shortcuts::{Launch, Shortcut};
use crate::storage::{self, Item};
use crate::timer::TimerState;
use crate::ui::{self, ACCENT, MUTED, PILL, TEXT};

/// Gap between the leading items of a row and its right-aligned block.
const ROW_GAP: f32 = 12.0;
/// Smallest room a title must keep before the right-aligned tag is allowed to shrink it.
const MIN_TITLE: f32 = 48.0;

/// Desktop-style grid tiles: width, height, and the icon square inside a tile.
const TILE_WIDTH: f32 = 66.0;
const TILE_HEIGHT: f32 = 78.0;
const ICON_SIZE: f32 = 34.0;

/// The temporary shelf: files held so they can be dragged into another app.
pub fn shelf_tab(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    accent_rule(ui, ui::SHELF_ACCENT);
    shelf_header(app, ui);
    ui.add_space(10.0);

    if let Some(err) = app.shelf.error() {
        ui::truncate(ui, err, 10.0, ACCENT, ui.available_width());
        ui.add_space(6.0);
    }

    if app.shelf.is_empty() {
        empty_shelf(ui);
        return;
    }

    shelf_list(app, ui);
}

/// Title, file count, and the "pick a file" action.
fn shelf_header(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    let count = app.shelf.len();
    let tag = match count {
        0 => "EMPTY".to_owned(),
        1 => "1 FILE".to_owned(),
        n => format!("{n} FILES"),
    };
    title_row(ui, "Temporary shelf", &tag);
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if ui::icon_button(ui, "+", 26.0).clicked()
            && let Some(paths) = rfd::FileDialog::new().pick_files()
        {
            for path in paths {
                app.shelf.add(path);
            }
        }
        ui.add_space(10.0);
        if app.shelf.len() > 1 && right_button(ui, "Clear") {
            app.shelf.clear();
        }
        // The hint is right-aligned and clipped so it cannot push the island wider.
        right_area(ui, |ui| {
            right_label(ui, "drag a file out to send it", 10.0, MUTED);
        });
    });
}

/// Shown before anything is shelved: a framed target with the one instruction that matters.
fn empty_shelf(ui: &mut egui::Ui) {
    let height = ui.available_height().max(80.0);
    let width = ui.available_width().max(0.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    let inner = rect.shrink(2.0);

    painter.rect_stroke(
        inner,
        14.0,
        egui::Stroke::new(1.0, Color32::from_white_alpha(22)),
        egui::StrokeKind::Inside,
    );

    let icon_size = 30.0;
    let icon_center = egui::pos2(inner.center().x, inner.center().y - 22.0);
    ui::paint_icon(painter, icon_center, ui::Icon::File, icon_size, ACCENT);

    let font = FontId::proportional(12.0);
    let label = if ui.rect_contains_pointer(inner) {
        "drop to shelve"
    } else {
        "drop files here"
    };
    let galley = ui::fit_to(ui, label, &font, TEXT, inner.width() - 24.0);
    painter.galley(
        egui::pos2(
            inner.center().x - galley.size().x * 0.5,
            inner.center().y + 2.0,
        ),
        galley,
        TEXT,
    );

    // A dragging cursor over the empty target tints the whole area.
    if response.hovered() && !ui.input(|i| i.raw.hovered_files.is_empty()) {
        painter.rect_filled(inner, 14.0, Color32::from_rgba_unmultiplied(242, 139, 112, 18));
    }
}

/// Scrollable grid of shelved files, each tile a drag source. A tile is the real file icon with
/// its name underneath, the way a Windows desktop folder looks.
fn shelf_list(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    let height = (ui.available_height() - 2.0).max(40.0);
    let items: Vec<Item> = app.shelf.items().to_vec();
    let columns = ui::grid_columns(ui.available_width(), TILE_WIDTH);
    let mut drag: Option<std::path::PathBuf> = None;
    let mut dismiss = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(height)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            for row in items.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for item in row {
                        let id = egui::Id::unique(("shelf_item", &item.path));
                        let response = ui.dnd_drag_source(id, item.path.clone(), |ui| {
                            if shelf_tile(app, ui, item, columns) {
                                dismiss = Some(item.path.clone());
                            }
                        });
                        // Dragging is handled after the frame so the OS drag loop owns the mouse.
                        if response.response.drag_stopped() {
                            drag = Some(item.path.clone());
                        }
                    }
                });
            }
            ui.add_space(2.0);
        });

    if let Some(path) = dismiss {
        app.shelf.dismiss_path(&path);
    }

    if let Some(path) = drag {
        // Blocks until the user drops on another app or cancels with Escape.
        storage::drag_out(&[path]);
    }
}

/// One desktop-style tile: the file's own icon, its name, and a dismiss button on hover.
fn shelf_tile(
    app: &mut DynamicIsland,
    ui: &mut egui::Ui,
    item: &Item,
    columns: usize,
) -> bool {
    let ctx = ui.ctx().clone();
    let texture = app.icon_for(&ctx, &item.path, None);
    let (rect, response) = ui::grid_cell(ui, columns, TILE_WIDTH, TILE_HEIGHT);

    let icon_box = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + ICON_SIZE * 0.5 + 6.0),
        Vec2::splat(ICON_SIZE),
    );
    ui::file_tile(ui, icon_box, texture.as_ref(), &item.name, ACCENT);

    // Caption is clipped to one line. Hover swaps the name for size + type, which is the detail
    // a desktop tooltip would have shown, without needing a third row.
    let caption_width = rect.width() - 6.0;
    let caption = if response.hovered() {
        let ext = item.extension().to_uppercase();
        if ext.is_empty() {
            item.size_label()
        } else {
            format!("{} \u{00b7} {ext}", item.size_label())
        }
    } else {
        item.name.clone()
    };
    let caption_rect = Rect::from_min_max(
        egui::pos2(rect.left() + 3.0, icon_box.bottom() + 4.0),
        egui::pos2(rect.left() + 3.0 + caption_width, rect.bottom()),
    );
    let mut caption_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(caption_rect)
            .layout(Layout::top_down(Align::Center)),
    );
    ui::truncate(
        &mut caption_ui,
        &caption,
        10.0,
        if response.hovered() { MUTED } else { TEXT },
        caption_width,
    );

    // Dismiss appears only on hover so the grid stays clean. The circle is painted before the
    // button so the cross reads on top of it.
    if response.hovered() {
        let close_width = 16.0;
        let centre = egui::pos2(rect.right() - close_width, rect.top() + close_width * 0.5);
        ui.painter().circle_filled(centre, close_width * 0.5, PILL);
        let mut close_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(centre, Vec2::splat(close_width)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        if ui::icon_button(&mut close_ui, "\u{00d7}", close_width).clicked() {
            return true;
        }
    }
    false
}

pub fn music_tab(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    accent_rule(ui, ui::MUSIC_ACCENT);
    // When another app (Spotify, a browser, ...) owns the system media session, mirror that
    // session instead of the island's own decoder.
    if let Some(media) = app.system_media.clone() {
        system_music_panel(app, ui, &media);
        return;
    }
    if let Some(err) = app.audio.error() {
        ui::truncate(ui, err, 10.0, ACCENT, ui.available_width());
        ui.add_space(6.0);
    }

    let duration = app.audio.track().and_then(|t| t.duration);
    let pos = app.audio.position();
    let ratio = duration.map_or(0.0, |d| pos.as_secs_f32() / d.as_secs_f32().max(0.001));
    let title = app
        .audio
        .track()
        .map_or("Nothing playing", |t| t.title.as_str());
    let tag = if app.audio.is_playing() {
        "PLAYING"
    } else {
        "PAUSED"
    };

    title_row(ui, "Now playing", tag);
    ui.add_space(5.0);
    ui::truncate(ui, title, 20.0, TEXT, ui.available_width());
    ui.add_space(12.0);

    if let Some(ratio) = ui::progress_bar_color(ui, ratio, 4.0, ui::MUSIC_ACCENT).1 {
        app.audio.seek_ratio(ratio);
    }
    ui.add_space(4.0);

    time_row(ui, pos, duration);
    ui.add_space(4.0);

    let playing = app.audio.is_playing();
    let (rewind, toggle, open) = controls_row(ui, playing);
    if rewind {
        if let Some(duration) = duration {
            let target = pos.saturating_sub(std::time::Duration::from_secs(10));
            app.audio.seek_ratio(
                (target.as_secs_f32() / duration.as_secs_f32().max(0.001)).clamp(0.0, 1.0),
            );
        }
    }
    if toggle {
        app.audio.toggle();
    }
    if open && let Some(path) = pick_audio_file() {
        app.audio.load(path);
    }
    volume_row(app, ui);
}

fn system_music_panel(app: &mut DynamicIsland, ui: &mut egui::Ui, media: &crate::media::Snapshot) {
    let pos = app.media_position();
    let duration = media.duration;
    let ratio = duration.map_or(0.0, |d| {
        (pos.as_secs_f32() / d.as_secs_f32().max(0.001)).clamp(0.0, 1.0)
    });

    title_row(
        ui,
        "Now playing",
        if media.playing { "PLAYING" } else { "PAUSED" },
    );
    ui.add_space(5.0);
    ui::truncate(ui, &media.title, 20.0, TEXT, ui.available_width());
    ui.add_space(2.0);

    let subtitle = if media.artist.trim().is_empty() {
        media.app.clone()
    } else {
        format!("{} \u{00b7} {}", media.artist, media.app)
    };
    ui::truncate(ui, &subtitle, 11.0, MUTED, ui.available_width());
    ui.add_space(12.0);

    let bar = ui::progress_bar_color(ui, ratio, 4.0, ui::MUSIC_ACCENT).0;
    // A click on the bar maps to an absolute offset request, since the OS only accepts
    // relative seeks.
    if bar.clicked()
        && let (Some(total), Some(ptr)) = (duration, bar.interact_pointer_pos())
    {
        let target = ((ptr.x - bar.rect.left()) / bar.rect.width().max(1.0)).clamp(0.0, 1.0);
        let delta = total.as_secs_f32() * target - pos.as_secs_f32();
        app.media_seek(delta.round() as i64);
    }
    ui.add_space(2.0);

    time_row(ui, pos, duration);
    ui.add_space(4.0);

    let (rewind, toggle, open) = controls_row(ui, media.playing);
    if rewind {
        app.media_seek(-10);
    }
    if toggle {
        app.media_toggle();
    }
    if open && let Some(path) = pick_audio_file() {
        app.audio.load(path);
    }
    volume_row(app, ui);
}

/// Volume slider under the transport controls, shared by the local and system music views.
///
/// This is the *system* volume: moving it changes the Windows output device, not a gain inside
/// the island, so it is the same control as the one the speaker keys move. The mute button only
/// exists when the device could be reached at all, and the percentage is right-aligned so the
/// slider has a fixed, predictable width next to it.
fn volume_row(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    let level = app.music_volume();
    let muted = app.music_muted();
    let percent = format!("{:.0}%", level * 100.0);
    let percent_width = ui
        .painter()
        .layout_no_wrap(percent.clone(), FontId::proportional(10.0), TEXT)
        .size()
        .x
        .max(28.0);

    ui.add_space(6.0);
    let mut mute_clicked = false;
    let mut dragged = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if let Some(muted) = muted {
            let icon = if muted {
                ui::Icon::VolumeOff
            } else {
                ui::Icon::VolumeOn
            };
            // A muted slider is drawn flat, matching the state of the output.
            mute_clicked = ui::icon_button(ui, icon, 22.0).clicked();
            ui.add_space(8.0);
        }
        let slider_width = (ui.available_width() - percent_width - 8.0).max(40.0);
        // The slider gets exactly the room left over, so the percentage cannot be pushed out
        // past the island padding by a wider number.
        if let Some(value) = ui::volume_slider_color(ui, level, slider_width, ui::MUSIC_ACCENT) {
            dragged = Some(value);
        }
        ui.add_space(8.0);
        right_area(ui, |ui| {
            right_label(ui, &percent, 10.0, MUTED);
        });
    });

    if let Some(level) = dragged {
        app.set_music_volume(level);
    }
    if mute_clicked {
        app.toggle_music_mute();
    }
}

/// Saved shell commands: run `npm run dev`, open an app, anything that would otherwise need a
/// terminal and a `cd`.
///
/// Two modes, and only one is drawn at a time: the grid of tiles, or the add form. The form is
/// not shown above the grid because the content area is short (~300 pt); squeezing a stacked
/// form above a grid leaves neither enough room, which reads as "the inputs are missing".
pub fn shortcuts_tab(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    if app.shortcut_form_open {
        shortcut_form(app, ui);
        return;
    }

    accent_rule(ui, ui::SHORTCUT_ACCENT);
    title_row(ui, "Shortcuts", &shortcut_tag(app));
    ui.add_space(6.0);

    // `+` opens the form; `Kill` appears only while something is running.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if ui::icon_button(ui, "+", 26.0).clicked() {
            app.shortcut_form_open = true;
        }
        ui.add_space(8.0);
        if app.shortcuts.any_running() && right_button(ui, "Kill all") {
            app.kill_all_shortcuts();
        }
        ui.add_space(6.0);
        right_area(ui, |ui| {
            right_label(ui, "click a tile to run", 10.0, MUTED);
        });
    });

    if let Some((text, color)) = shortcut_status(app) {
        ui.add_space(4.0);
        ui::truncate(ui, text, 10.0, color, ui.available_width());
    }

    ui.add_space(6.0);

    if app.shortcuts.is_empty() {
        empty_shortcuts(ui);
        return;
    }

    shortcut_list(app, ui);
}

fn shortcut_tag(app: &DynamicIsland) -> String {
    if app.shortcuts.any_running() {
        return "RUNNING".to_owned();
    }
    match app.shortcuts.items().len() {
        0 => "EMPTY".to_owned(),
        1 => "1 COMMAND".to_owned(),
        n => format!("{n} COMMANDS"),
    }
}

/// One line for whatever happened last: a failed launch, a save error, or a finished command.
fn shortcut_status(app: &DynamicIsland) -> Option<(&str, Color32)> {
    match (app.shortcuts.launch(), app.shortcuts.status()) {
        (Some(Launch::Failed(msg)), _) => Some((msg.as_str(), ACCENT)),
        (None, Some(msg)) => Some((msg, ACCENT)),
        (Some(Launch::Ok(msg)), _) => Some((msg.as_str(), MUTED)),
        (None, None) => None,
    }
}

/// The add form, when opened, owns the whole page: three full-width fields stacked in a column,
/// then the actions. Full width means a long command or path is readable while typing, which a
/// half-width field could not do.
fn shortcut_form(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    title_row(ui, "New shortcut", "ADD");
    ui.add_space(8.0);

    let width = ui.available_width().max(0.0);
    field_label(ui, "Name");
    text_input(ui, &mut app.shortcut_name, "dev", width);
    ui.add_space(6.0);

    field_label(ui, "Command");
    text_input(ui, &mut app.shortcut_command, "npm run dev", width);
    ui.add_space(6.0);

    field_label(ui, "Folder");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let picker = 26.0;
        let dir_width = (width - picker - 6.0).max(40.0);
        if app.shortcut_dir.trim().is_empty() {
            // The field is a path display, not an input: a typed path is the easiest thing to
            // get wrong, so the folder is always chosen with the picker.
            let (rect, _) = ui.allocate_exact_size(Vec2::new(dir_width, 24.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, 6.0, Color32::from_white_alpha(6));
            let galley = ui::fit_to(
                ui,
                "home folder (default)",
                &FontId::proportional(11.0),
                MUTED,
                dir_width - 14.0,
            );
            ui.painter().galley(
                egui::pos2(rect.left() + 7.0, rect.center().y - galley.size().y * 0.5),
                galley,
                MUTED,
            );
        } else {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(dir_width, 24.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, 6.0, Color32::from_white_alpha(6));
            let galley =
                ui::fit_to(ui, &app.shortcut_dir, &FontId::proportional(11.0), TEXT, dir_width - 14.0);
            ui.painter().galley(
                egui::pos2(rect.left() + 7.0, rect.center().y - galley.size().y * 0.5),
                galley,
                TEXT,
            );
        }
        ui.add_space(6.0);
        if ui::icon_button(ui, ui::Icon::Folder, picker).clicked()
            && let Some(dir) = rfd::FileDialog::new().pick_folder()
        {
            app.shortcut_dir = dir.display().to_string();
        }
    });

    ui.add_space(10.0);

    // Actions: Save is primary, Cancel returns to the grid. Enter saves too, since the form is
    // the only thing on the page.
    let mut save = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        save = tab_button(ui, "Save", true).clicked();
        ui.add_space(8.0);
        if !app.shortcut_dir.trim().is_empty() {
            cancel = right_button(ui, "Use home folder");
        }
        ui.add_space(6.0);
        if right_button(ui, "Cancel") {
            cancel = true;
        }
    });

    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    // On a validation error the form stays open so the message can be read and fixed.
    if (save || enter)
        && app
            .shortcuts
            .add(&app.shortcut_name, &app.shortcut_command, &app.shortcut_dir)
    {
        app.shortcut_name.clear();
        app.shortcut_command.clear();
        app.shortcut_form_open = false;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) || (cancel && !enter) {
        app.shortcut_form_open = false;
    }
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    ui::label(ui, text, 10.0, MUTED);
    ui.add_space(2.0);
}

fn empty_shortcuts(ui: &mut egui::Ui) {
    let height = ui.available_height().max(60.0);
    let width = ui.available_width().max(0.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    let inner = rect.shrink(2.0);
    painter.rect_stroke(
        inner,
        14.0,
        egui::Stroke::new(1.0, Color32::from_white_alpha(22)),
        egui::StrokeKind::Inside,
    );
    ui::paint_icon(
        painter,
        egui::pos2(inner.center().x, inner.center().y - 16.0),
        ui::Icon::Terminal,
        28.0,
        ACCENT,
    );
    let font = FontId::proportional(12.0);
    let galley = ui::fit_to(ui, "no saved commands yet", &font, TEXT, inner.width() - 24.0);
    painter.galley(
        egui::pos2(
            inner.center().x - galley.size().x * 0.5,
            inner.center().y + 8.0,
        ),
        galley,
        TEXT,
    );
}

/// Last segment of a directory path, for a label that must stay short.
fn dir_tail(dir: &str) -> String {
    let dir = dir.trim_end_matches(['\\', '/']);
    dir.rsplit(['\\', '/'])
        .next()
        .filter(|tail| !tail.is_empty())
        .unwrap_or(dir)
        .to_owned()
}

/// Grid of saved commands. A tile runs on click, the way a desktop icon does; a running tile
/// shows a pulsing ring and offers `Stop` instead of `Run`.
fn shortcut_list(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    let height = (ui.available_height() - 2.0).max(40.0);
    let items: Vec<Shortcut> = app.shortcuts.items().to_vec();
    let columns = ui::grid_columns(ui.available_width(), TILE_WIDTH);
    let ctx = ui.ctx().clone();
    let mut action = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(height)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            for row in items.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for item in row {
                        let running = app.shortcuts.is_running(&item.name);
                        // The icon belongs to the executable the command launches, when that is
                        // a real file; otherwise the shell answers from the working folder, and a
                        // monogram tile is used.
                        let texture =
                            app.icon_for(&ctx, &shortcut_icon_path(item), Some(&item.command));
                        let chosen =
                            shortcut_tile(ui, item, running, columns, texture.as_ref());
                        if !matches!(chosen, ShortcutAction::None) {
                            action = Some((chosen, item.name.clone()));
                        }
                    }
                });
            }
            ui.add_space(2.0);
        });

    // Actions are keyed by name, not index: the list can be edited while a frame is in flight.
    let Some((chosen, name)) = action else {
        return;
    };
    let Some(index) = app.shortcuts.items().iter().position(|s| s.name == name) else {
        return;
    };
    match chosen {
        ShortcutAction::Run => app.shortcuts.run(index),
        ShortcutAction::Stop => app.shortcuts.kill(&name),
        ShortcutAction::Remove => app.shortcuts.remove(index),
        ShortcutAction::None => {}
    }
}

/// A path whose icon stands in for a shortcut: the working directory when one was picked, so a
/// project tile can show the folder icon instead of a blank document.
fn shortcut_icon_path(item: &Shortcut) -> std::path::PathBuf {
    if item.dir.trim().is_empty() {
        std::path::PathBuf::from(&item.name)
    } else {
        std::path::PathBuf::from(&item.dir)
    }
}

enum ShortcutAction {
    None,
    Run,
    Stop,
    Remove,
}

/// One command tile: icon, name, and a play badge that becomes a stop badge while it runs.
fn shortcut_tile(
    ui: &mut egui::Ui,
    item: &Shortcut,
    running: bool,
    columns: usize,
    texture: Option<&egui::TextureHandle>,
) -> ShortcutAction {
    let (rect, response) = ui::grid_cell(ui, columns, TILE_WIDTH, TILE_HEIGHT);
    let icon_box = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + ICON_SIZE * 0.5 + 6.0),
        Vec2::splat(ICON_SIZE),
    );
    // A directory target is not a file the shell knows, so the monogram is the honest fallback.
    ui::file_tile(ui, icon_box, texture, &item.name, ACCENT);

    if running {
        // A pulsing ring around the icon: the one thing that says "this is live" without text.
        let time = ui.input(|i| i.time) as f32;
        let phase = (time * 2.0).fract();
        ui.ctx().request_repaint();
        ui.painter().circle_stroke(
            icon_box.center(),
            ICON_SIZE * (0.55 + phase * 0.35),
            egui::Stroke::new(1.5, ACCENT.gamma_multiply(1.0 - phase)),
        );
    }

    // Badge: `▶` when idle, `■` while running, so the click target reads as run/stop without a
    // second line of text.
    let badge_rect = Rect::from_center_size(
        egui::pos2(icon_box.right() - 2.0, icon_box.bottom() - 2.0),
        Vec2::splat(14.0),
    );
    let badge_color = if running { ACCENT } else { Color32::from_gray(70) };
    ui.painter().circle_filled(badge_rect.center(), 7.0, PILL);
    if running {
        ui.painter().rect_filled(
            Rect::from_center_size(badge_rect.center(), Vec2::splat(5.0)),
            1.0,
            badge_color,
        );
    } else {
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(badge_rect.center().x - 2.0, badge_rect.center().y - 3.5),
                egui::pos2(badge_rect.center().x - 2.0, badge_rect.center().y + 3.5),
                egui::pos2(badge_rect.center().x + 3.5, badge_rect.center().y),
            ],
            badge_color,
            egui::Stroke::NONE,
        ));
    }

    // Caption: the name, or the working folder while hovering, which is the detail worth knowing
    // before launching a command.
    let caption_width = rect.width() - 6.0;
    let caption = if response.hovered() && !item.dir.trim().is_empty() {
        dir_tail(&item.dir)
    } else {
        item.name.clone()
    };
    let caption_rect = Rect::from_min_max(
        egui::pos2(rect.left() + 3.0, icon_box.bottom() + 4.0),
        egui::pos2(rect.left() + 3.0 + caption_width, rect.bottom()),
    );
    let mut caption_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(caption_rect)
            .layout(Layout::top_down(Align::Center)),
    );
    ui::truncate(
        &mut caption_ui,
        &caption,
        10.0,
        if running {
            ACCENT
        } else if response.hovered() {
            MUTED
        } else {
            TEXT
        },
        caption_width,
    );

    // Hover-only delete, same as the shelf tile. A running command cannot be deleted, because
    // that would strand the process with no tile left to stop it.
    if response.hovered() && !running {
        let close_width = 16.0;
        let centre = egui::pos2(rect.right() - close_width, rect.top() + close_width * 0.5);
        ui.painter().circle_filled(centre, close_width * 0.5, PILL);
        let mut close_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(centre, Vec2::splat(close_width)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        if ui::icon_button(&mut close_ui, "\u{00d7}", close_width).clicked() {
            return ShortcutAction::Remove;
        }
    }

    // Clicking anywhere on the tile launches, matching a desktop double-click in one step. While
    // the command runs, the same click stops it and everything it spawned.
    if response.clicked() {
        return if running {
            ShortcutAction::Stop
        } else {
            ShortcutAction::Run
        };
    }
    ShortcutAction::None
}

/// A single-line text field sized to `width`, with a visible outline so it reads as an input on
/// the dark island instead of blending into the page background.
fn text_input(ui: &mut egui::Ui, text: &mut String, hint: &str, width: f32) {
    // `desired_width` alone lets the field grow past the island on long input; the clamp keeps
    // every field inside the same column as the rows below it.
    let width = width.max(40.0);
    ui.add_sized(
        Vec2::new(width, 24.0),
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(width)
            .font(FontId::proportional(11.0))
            .text_color(TEXT)
            .margin(Vec2::new(7.0, 5.0)),
    );
}

pub fn timer_tab(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    accent_rule(ui, ui::TIMER_ACCENT);
    let state = app.timer.state();
    let tag = match state {
        TimerState::Idle => "READY",
        TimerState::Running => "RUNNING",
        TimerState::Paused => "PAUSED",
        TimerState::Finished => "DONE",
    };
    title_row(ui, "Focus session", tag);
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui::label(ui, &app.timer.label_time(), 48.0, TEXT);
    });
    ui.add_space(4.0);
    ui::progress_bar_color(ui, app.timer.progress(), 5.0, ui::TIMER_ACCENT);
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui::label(ui, "For", 11.0, MUTED);
        ui.add_space(5.0);
        for mins in [1u64, 5, 10, 25] {
            let selected = app.timer.duration().as_secs() == mins * 60;
            if tab_button(ui, &format!("{mins}m"), selected).clicked() {
                app.timer
                    .set_duration(std::time::Duration::from_secs(mins * 60));
                app.custom_minutes = mins;
            }
        }
        if state != TimerState::Running {
            right_area(ui, |ui| {
                ui.add(
                    egui::DragValue::new(&mut app.custom_minutes)
                        .range(1..=180)
                        .suffix("m")
                        .speed(0.2),
                )
                .on_hover_text("Custom duration");
                if tab_button(ui, "Set", false).clicked() {
                    app.timer
                        .set_duration(std::time::Duration::from_secs(app.custom_minutes * 60));
                }
            });
        }
    });
    ui.add_space(4.0);

    let glyph = if state == TimerState::Running {
        ui::Icon::Pause
    } else {
        ui::Icon::Play
    };
    let (toggle, reset, extend) = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let toggle = ui::icon_button(ui, glyph, 38.0).clicked();
            ui.add_space(4.0);
            let reset = ui::icon_button(ui, ui::Icon::Repeat, 32.0).clicked();
            let extend = right_button(ui, "+1m");
            (toggle, reset, extend)
        })
        .inner;

    if toggle {
        app.timer.toggle();
    }
    if reset {
        app.timer.reset();
    }
    if extend {
        let d = app.timer.duration() + std::time::Duration::from_secs(60);
        app.timer.set_duration(d);
    }
}

pub fn alerts_tab(app: &mut DynamicIsland, ui: &mut egui::Ui) {
    accent_rule(ui, ui::ALERT_ACCENT);
    let actions = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui::label(ui, "Notifications", 16.0, TEXT);
            !app.notifications.is_empty() && right_button(ui, "Clear")
        })
        .inner;
    if actions && !app.notifications.is_empty() {
        app.notifications.clear();
    }
    ui.add_space(8.0);

    let list_height = (ui.available_height() - 4.0).max(40.0);
    let snapshot = app.notifications.items();
    let mut dismissed = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(list_height)
        .show(ui, |ui| {
            if snapshot.is_empty() {
                ui.add_space(40.0);
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui::label(ui, "All caught up", 13.0, MUTED);
                });
                return;
            }
            for note in &snapshot {
                let mut dismiss = false;
                Frame::new()
                    .fill(PILL)
                    .corner_radius(12)
                    .inner_margin(Margin::symmetric(10, 8))
                    .outer_margin(Margin::symmetric(0, 3))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (icon_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                            ui::notification_glyph(
                                ui.painter(),
                                icon_rect.center(),
                                note.icon,
                                14.0,
                                ACCENT,
                            );
                            ui.add_space(8.0);
                            // Time and close button are right-aligned; the text block gets what
                            // is left so long bodies are truncated instead of overflowing.
                            let close_width = 18.0;
                            let meta_width = 26.0;
                            let text_width =
                                (ui.available_width() - meta_width - close_width - ROW_GAP * 2.0)
                                    .max(0.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui::truncate(ui, &note.title, 12.0, TEXT, text_width);
                                ui::truncate(ui, &note.body, 11.0, MUTED, text_width);
                            });
                            right_area(ui, |ui| {
                                if ui::icon_button(ui, "\u{00d7}", 18.0).clicked() {
                                    dismiss = true;
                                }
                                ui.add_space(6.0);
                                let left = (note.lifetime - note.age).max(0.0);
                                ui::label(ui, &format!("{left:.0}s"), 10.0, Color32::from_gray(90));
                            });
                        });
                    });
                if dismiss {
                    dismissed = Some(note.id);
                }
            }
        });

    if let Some(id) = dismissed {
        app.notifications.dismiss(id);
    }
}

/// Draws the toast below the collapsed island. Returns `true` when clicked.
pub fn floating_note(ui: &mut egui::Ui, island: Rect, note: &NotificationView) -> bool {
    // Wait for the island to finish collapsing before sliding the toast in.
    let pop = ((note.age - 0.35) / 0.35).clamp(0.0, 1.0);
    if pop <= 0.0 {
        return false;
    }
    let eased = pop * pop * (3.0 - 2.0 * pop);
    let width = island.width().max(280.0);
    let rect = Rect::from_min_size(
        egui::pos2(
            island.center().x - width * 0.5,
            island.bottom() + 2.0 - (1.0 - eased) * 8.0,
        ),
        Vec2::new(width, 58.0),
    );

    let remaining = (note.lifetime - note.age).max(0.0);
    let fade = (remaining / 0.35).clamp(0.0, 1.0);
    let opacity = eased * fade;
    let clip = rect.intersect(ui.clip_rect());
    let painter = ui.painter().with_clip_rect(clip);
    ui::surface(&painter, rect, 16.0, Color32::from_rgb(24, 24, 30), opacity);
    let alpha = |c: u8| (c as f32 * opacity) as u8;
    let text_color = Color32::from_rgba_unmultiplied(TEXT.r(), TEXT.g(), TEXT.b(), alpha(255));
    let muted = Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), alpha(255));
    ui::notification_glyph(
        &painter,
        egui::pos2(rect.left() + 20.0, rect.center().y),
        note.icon,
        18.0,
        ACCENT.gamma_multiply(opacity),
    );
    // Text is clipped to the room left between the icon and the countdown.
    let text_left = rect.left() + 42.0;
    let text_width = (rect.right() - 44.0 - text_left).max(0.0);
    let font = FontId::proportional(12.0);
    let title = ui::fit_to(ui, &note.title, &font, text_color, text_width);
    let title_y = rect.center().y - 11.0 - title.size().y * 0.5;
    painter.galley(egui::pos2(text_left, title_y), title, text_color);
    let font = FontId::proportional(11.0);
    let body = ui::fit_to(ui, &note.body, &font, muted, text_width);
    let body_y = rect.center().y + 7.0 - body.size().y * 0.5;
    painter.galley(egui::pos2(text_left, body_y), body, muted);
    painter.text(
        egui::pos2(rect.right() - 14.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{remaining:.0}s"),
        FontId::proportional(10.0),
        muted,
    );

    opacity > 0.05
        && ui
            .interact(
                rect,
                egui::Id::NULL.with(("floating_note", note.id)),
                Sense::click(),
            )
            .clicked()
}

pub fn tab_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(12.0), TEXT)
        .size()
        .x;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width + 24.0, 26.0), Sense::click());
    let fill = if selected {
        PILL
    } else if response.hovered() {
        Color32::from_gray(32)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 13.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::proportional(12.0),
        if selected { TEXT } else { MUTED },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Title row shared by every tab: a truncated title on the left, a status tag that ends
/// exactly on the island's right padding.
fn title_row(ui: &mut egui::Ui, title: &str, tag: &str) {
    let full = ui.available_width().max(0.0);
    // The tag keeps its natural width while the title still has room for a word or two;
    // only on an absurdly narrow island is the tag allowed to give way.
    let tag_width = (ui
        .painter()
        .layout_no_wrap(tag.to_owned(), FontId::proportional(11.0), TEXT)
        .size()
        .x
        + 20.0)
        .min((full - MIN_TITLE).max(0.0));
    let title_width = (full - tag_width - ROW_GAP).max(0.0);

    let (rect, _) = ui.allocate_exact_size(Vec2::new(full, 24.0), Sense::hover());
    let mut title_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(
                rect.min,
                Vec2::new(title_width, rect.height()),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    title_ui.spacing_mut().item_spacing.x = 0.0;
    ui::truncate(&mut title_ui, title, 18.0, TEXT, title_width);

    let tag_rect = Rect::from_min_max(
        egui::pos2(rect.right() - tag_width, rect.top()),
        rect.right_bottom(),
    );
    let mut tag_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(tag_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    tag_ui.spacing_mut().item_spacing.x = 0.0;
    ui::pill(&mut tag_ui, tag);
}

/// Transport controls plus a right-aligned hint. Returns `(rewind, play/pause, open file)`.
fn controls_row(ui: &mut egui::Ui, playing: bool) -> (bool, bool, bool) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::hover());
    let mut controls = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    controls.spacing_mut().item_spacing.x = 14.0;
    let rewind = rewind_button(&mut controls).clicked();
    let glyph = if playing { "\u{23f8}" } else { "\u{25b6}" };
    let toggle = ui::icon_button(&mut controls, glyph, 46.0).clicked();
    let open = ui::icon_button(&mut controls, "+", 34.0)
        .on_hover_text("Open audio file")
        .clicked();
    (rewind, toggle, open)
}

pub fn settings_tab(ui: &mut egui::Ui) {
    accent_rule(ui, ACCENT);
    title_row(ui, "Application", "SETTINGS");
    ui.add_space(12.0);
    let startup_enabled = startup_enabled();
    let mut startup_error = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.vertical(|ui| {
            ui::label(ui, "Launch at sign-in", 14.0, TEXT);
            ui::label(ui, "Start Dynamic Island when Windows starts.", 11.0, MUTED);
        });
        right_area(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(42.0, 24.0), Sense::click());
            let track = if startup_enabled {
                ui::MUSIC_ACCENT
            } else {
                PILL
            };
            ui.painter().rect_filled(rect, 12.0, track);
            let knob_x = if startup_enabled {
                rect.right() - 12.0
            } else {
                rect.left() + 12.0
            };
            ui.painter().circle_filled(
                egui::pos2(knob_x, rect.center().y),
                9.0,
                Color32::WHITE,
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                startup_error = set_startup_enabled(!startup_enabled).err();
            }
        });
    });
    if let Some(error) = startup_error {
        ui.add_space(4.0);
        ui::truncate(ui, &error, 10.0, ui::ALERT_ACCENT, ui.available_width());
    }
    ui.add_space(14.0);
    ui::label(ui, "Close the Dynamic Island", 14.0, TEXT);
    ui.add_space(4.0);
    ui::label(ui, "Exit the app and stop its background activity.", 11.0, MUTED);
    ui.add_space(12.0);
    if right_button(ui, "Close app") {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(windows)]
fn startup_enabled() -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, RegCloseKey, RegOpenKeyExW,
        RegQueryValueExW,
    };

    const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    let key_name: Vec<u16> = std::ffi::OsStr::new(KEY).encode_wide().chain(Some(0)).collect();
    let value_name: Vec<u16> = std::ffi::OsStr::new("DynamicIsland")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let expected = startup_command(&exe);
    let mut key: HKEY = 0 as HKEY;
    // SAFETY: valid null-terminated strings and an initialized output handle are passed.
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key_name.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) }
        != 0
    {
        return false;
    }
    let mut data = vec![0u16; 32768];
    let mut size = (data.len() * 2) as u32;
    // SAFETY: data is writable storage with size in bytes; names and key are valid.
    let result = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    // SAFETY: key was opened successfully above.
    unsafe { RegCloseKey(key) };
    result == 0
        && String::from_utf16_lossy(&data[..(size as usize / 2)])
            .trim_end_matches('\0')
            == expected
}

#[cfg(not(windows))]
fn startup_enabled() -> bool {
    false
}

fn startup_command(exe: &std::path::Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(windows)]
fn set_startup_enabled(enabled: bool) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
    };

    const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    let key_name: Vec<u16> = std::ffi::OsStr::new(KEY).encode_wide().chain(Some(0)).collect();
    let value_name: Vec<u16> = std::ffi::OsStr::new("DynamicIsland")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut key: HKEY = 0 as HKEY;
    // SAFETY: valid arguments and output handle; this only creates/opens the current user's Run key.
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_name.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(format!("Windows registry error {result}"));
    }

    let result = if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = startup_command(&exe);
        let mut data: Vec<u16> = std::ffi::OsStr::new(&command).encode_wide().collect();
        data.push(0);
        // SAFETY: data is a valid UTF-16 REG_SZ including its terminator.
        unsafe {
            RegSetValueExW(
                key,
                value_name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        }
    } else {
        // SAFETY: key and value name are valid; missing value is treated as already disabled.
        unsafe { RegDeleteValueW(key, value_name.as_ptr()) }
    };
    // SAFETY: key was opened successfully above.
    unsafe { RegCloseKey(key) };
    if result == 0 || (!enabled && result == 2) {
        Ok(())
    } else {
        Err(format!("Windows registry error {result}"))
    }
}

#[cfg(not(windows))]
fn set_startup_enabled(_enabled: bool) -> Result<(), String> {
    Err("Startup settings are available on Windows only".into())
}

fn accent_rule(ui: &mut egui::Ui, color: Color32) {
    let width = (ui.available_width() * 0.22).clamp(54.0, 110.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 3.0), Sense::hover());
    ui.painter().rect_filled(rect, 1.5, color);
    ui.add_space(7.0);
}

fn rewind_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
    let color = if response.hovered() { TEXT } else { MUTED };
    let painter = ui.painter();
    if response.hovered() {
        painter.circle_filled(rect.center(), rect.width() * 0.5, Color32::from_white_alpha(24));
    }

    // Counter-clockwise seek arc with an arrowhead and a compact 10-second mark.
    let center = rect.center();
    let radius = 11.0;
    let points: Vec<_> = (0..=20)
        .map(|i| {
            let angle = -0.72 + (i as f32 / 20.0) * std::f32::consts::TAU * 0.78;
            center + Vec2::new(angle.cos() * radius, angle.sin() * radius)
        })
        .collect();
    let stroke = egui::Stroke::new(1.7, color);
    painter.add(egui::Shape::line(points, stroke));
    let tip = center + Vec2::new(radius * 0.75, -radius * 0.66);
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            tip + Vec2::new(-5.5, 0.0),
            tip + Vec2::new(-1.5, 4.5),
        ],
        color,
        egui::Stroke::NONE,
    ));
    painter.text(
        center + Vec2::new(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        "10",
        FontId::proportional(8.0),
        color,
    );
    response
        .on_hover_text("Rewind 10 seconds")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `elapsed` on the left, `total` flush with the right padding.
fn time_row(ui: &mut egui::Ui, pos: std::time::Duration, total: Option<std::time::Duration>) {
    let elapsed = fmt_time(pos);
    let total = total.map_or_else(|| "--:--".to_owned(), fmt_time);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        right_label(ui, &total, 11.0, MUTED);
        ui.add_space(ROW_GAP);
        ui::label(ui, &elapsed, 11.0, MUTED);
    });
}

/// Right-aligned text that never spills out of `ui`.
fn right_label(ui: &mut egui::Ui, text: &str, size: f32, color: Color32) {
    let width = ui.available_width().max(0.0);
    ui::truncate(ui, text, size, color, width);
}

/// Runs `contents` in a sub-`Ui` that ends on the parent's right edge and grows leftwards,
/// so its first widget really is flush with the right padding.
fn right_area(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    let room = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(room, Sense::hover());
    let mut area = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    area.spacing_mut().item_spacing.x = 0.0;
    contents(&mut area);
}

/// Right-aligned pill button; returns whether it was clicked.
fn right_button(ui: &mut egui::Ui, text: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(
            ui.painter()
                .layout_no_wrap(text.to_owned(), FontId::proportional(12.0), TEXT)
                .size()
                .x
                + 24.0,
            26.0,
        ),
        Sense::click(),
    );
    let fill = if response.hovered() {
        Color32::from_gray(32)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 13.0, fill);
    let galley = ui::fit_to(
        ui,
        text,
        &FontId::proportional(12.0),
        MUTED,
        rect.width() - 12.0,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        MUTED,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn pick_audio_file() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a", "mp4"])
        .pick_file()
}

fn fmt_time(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}
