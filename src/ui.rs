use crate::app_state::{AppStateManager, Progress, Submenu};
use crate::update::do_update;
use egui::{Button, Color32, FullOutput, ProgressBar};
use egui_backend::egui;
use egui_backend::{sdl2::event::Event, DpiScaling, ShaderVersion};
use egui_sdl2_gl as egui_backend;
use egui_sdl2_gl::egui::{
    CornerRadius, FontData, FontDefinitions, FontFamily, Pos2, Rect, RichText, Spinner, Vec2,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::{io::Read, sync::Arc, time::Instant};

use crate::{Result, SDCARD_ROOT};

const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 768;
const DPI_SCALE: f32 = 4.0;
// MinUI font selection (from minuisettings.txt): font=0/1
// On-device Chinese glyph support is provided by .system/res/font2.ttf.
const FONTS: [&str; 2] = ["BPreplayBold-unhinted.otf", "font2.ttf"];

// Pre-warm glyph cache for on-device rendering.
// Some GPU/driver/font-atlas combinations only render glyphs that have already been rasterized.
// Also note: glyph atlases are effectively size-dependent, so we prewarm at multiple sizes.
const PREWARM_TEXT_ZH: &str = "\
警告\
NextUI 并不完全支持降级！\
旧版本可能导致部分设置丢失或不稳定\
可能需要手动编辑设置或文件\
已选择版本：\
当前已安装此版本！\
当前已是最新版本：\
发现新版本：\
最新版本：\
暂无版本信息\
返回\
我已了解\
返回到最新版本选项\
确认警告并打开更新选项\
快速更新\
完整更新\
仅更新 MinUI.zip\
解压完整压缩包（基础 + 扩展）\
仍要更新\
退出\
退出 NextUI 更新器\
忽略当前版本\
选择版本\
正在检查更新器更新...\
正在下载更新器...\
正在解压 NextUI 更新器\
解压更新包失败\
自更新成功！正在重启更新器...\
正在检查 NextUI 更新...\
正在获取 NextUI Release 列表...\
获取 Release 失败：\
未获取到任何 Release\
正在获取 NextUI Tag 列表...\
获取 Tag 失败：\
未获取到任何 Tag\
最新 Release 找不到对应的 Tag：\
自更新失败：\
更新失败：\
正在下载更新包...\
未找到可用版本\
未找到可下载的资源文件\
正在下载\
正在解压\
请稍候...\
Roms 文件夹\
已存在，跳过\
更新完成，准备重启...\
正在重启系统...\
GitHub API 请求失败：\
状态：\
响应头：\
下载完成！\
";

#[allow(clippy::too_many_lines)]
fn nextui_ui(ui: &mut egui::Ui, app_state: &'static AppStateManager) -> egui::Response {
    let current_version = app_state.current_version();
    let mut latest_release = app_state.nextui_release().clone();
    let mut latest_tag = app_state.nextui_tag().clone();
    let mut update_available = true;
    let latest_discarded = app_state.nextui_tag().clone().is_none();

    if app_state.release_selection_menu() {
        let index = app_state.nextui_releases_and_tags_index().unwrap_or(0);
        let relase_and_tag_vector = app_state.nextui_releases_and_tags().unwrap_or_default();
        let release_and_tag = relase_and_tag_vector.get(index).cloned();
        latest_release = release_and_tag.as_ref().map(|r| r.release.clone());
        latest_tag = release_and_tag.map(|r| r.tag.clone());
    }

    if app_state.release_selection_menu() & !app_state.release_selection_confirmed() {
        ui.add_space(16.0);
        ui.label(
            RichText::new(
                "警告\n\
            NextUI 并不完全支持降级！\n\
            旧版本可能导致部分设置丢失或不稳定\n\
            可能需要手动编辑设置或文件",
            )
            .size(10.0),
        );
    } else {
        // Show release information if available
        match (current_version, latest_tag, latest_release) {
            (Some(current_version), Some(tag), _) => {
                let selected_tag = hint_wrap_nextui_tag(app_state, &tag.name);
                if tag.commit.sha.starts_with(&current_version) && !latest_discarded {
                    if app_state.release_selection_menu() {
                        // selection view
                        ui.label(
                            RichText::new(format!(
                                "已选择版本：\n{selected_tag}\n当前已安装此版本！"
                            ))
                            .size(10.0),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!("当前已是最新版本：\n{selected_tag}")).size(10.0),
                        );
                    }
                    update_available = false;
                } else if app_state.release_selection_menu() {
                    // selection view
                    ui.label(RichText::new(format!("已选择版本：\n{selected_tag}")).size(10.0));
                } else {
                    ui.label(RichText::new(format!("发现新版本：\n{selected_tag}")).size(10.0));
                }
            }
            (_, _, Some(release)) => {
                if app_state.release_selection_menu() {
                    // selection view
                    let selected_tag = hint_wrap_nextui_tag(app_state, &release.tag_name);
                    ui.label(RichText::new(format!("已选择版本：\n{selected_tag}")).size(10.0));
                } else {
                    ui.label(
                        RichText::new(format!("最新版本：\nNextUI {}", release.tag_name))
                            .size(10.0),
                    );
                }
            }
            _ => {
                ui.label(RichText::new("暂无版本信息".to_string()).size(10.0));
            }
        }
    }

    ui.add_space(8.0);

    if app_state.release_selection_menu() & !app_state.release_selection_confirmed() {
        let back_button = ui.button("返回");
        if back_button.clicked() {
            app_state.set_release_selection_menu(false);
        }

        let confirm_button = ui.button("我已了解");
        if confirm_button.clicked() {
            app_state.set_release_selection_confirmed(true);
        }

        if back_button.has_focus() {
            app_state.set_hint(Some("返回到最新版本选项".to_string()));
        } else if confirm_button.has_focus() {
            app_state.set_hint(Some("确认警告并打开更新选项".to_string()));
        } else {
            app_state.set_hint(None);
        }

        back_button
    } else if update_available {
        let quick_update_button = ui.add(Button::new("快速更新"));

        // Initiate update if button clicked
        if quick_update_button.clicked() {
            // Clear any previous errors
            app_state.set_error(None);
            do_update(app_state, false);
        }

        ui.add_space(4.0);

        let full_update_button = ui.add(Button::new("完整更新"));

        if full_update_button.clicked() {
            // Clear any previous errors
            app_state.set_error(None);
            do_update(app_state, true);
        }

        // HINTS
        if quick_update_button.has_focus() {
            app_state.set_hint(Some("仅更新 MinUI.zip".to_string()));
        } else if full_update_button.has_focus() {
            app_state.set_hint(Some("解压完整压缩包（基础 + 扩展）".to_string()));
        } else {
            app_state.set_hint(None);
        }

        quick_update_button
    } else {
        let force_button = ui.button("仍要更新");
        if force_button.clicked() {
            app_state.set_nextui_tag(None); // forget the tag
        }

        let quit_button = ui.button("退出");
        if quit_button.clicked() {
            if app_state.release_selection_menu() {
                app_state.set_release_selection_menu(false);
            } else {
                app_state.set_should_quit(true);
            }
        }

        if quit_button.has_focus() {
            if app_state.release_selection_menu() {
                app_state.set_hint(Some("返回到最新版本选项".to_string()));
            } else {
                app_state.set_hint(Some("退出 NextUI 更新器".to_string()));
            }
        } else if force_button.has_focus() {
            app_state.set_hint(Some("忽略当前版本".to_string()));
        } else {
            app_state.set_hint(None);
        }

        quit_button
    }
}

// Map controller buttons to keyboard keys
fn controller_to_key(button: sdl2::controller::Button) -> Option<sdl2::keyboard::Keycode> {
    match button {
        sdl2::controller::Button::DPadUp => Some(sdl2::keyboard::Keycode::Up),
        sdl2::controller::Button::DPadDown => Some(sdl2::keyboard::Keycode::Down),
        sdl2::controller::Button::DPadLeft => Some(sdl2::keyboard::Keycode::Left),
        sdl2::controller::Button::DPadRight => Some(sdl2::keyboard::Keycode::Right),
        sdl2::controller::Button::B => Some(sdl2::keyboard::Keycode::Return),
        sdl2::controller::Button::A => Some(sdl2::keyboard::Keycode::Escape),
        sdl2::controller::Button::Y => Some(sdl2::keyboard::Keycode::X),
        _ => None,
    }
}

fn setup_ui_style() -> egui::Style {
    let mut style = egui::Style::default();
    style.spacing.button_padding = Vec2::new(8.0, 2.0);

    style.visuals.panel_fill = Color32::from_rgb(0, 0, 0);
    style.visuals.selection.bg_fill = Color32::WHITE;
    style.visuals.selection.stroke.color = Color32::GRAY;

    style.visuals.widgets.inactive.fg_stroke.color = Color32::WHITE;
    style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;

    style.visuals.widgets.active.bg_fill = Color32::WHITE;
    style.visuals.widgets.active.weak_bg_fill = Color32::WHITE;
    style.visuals.widgets.active.fg_stroke.color = Color32::BLACK;
    style.visuals.widgets.active.corner_radius = CornerRadius::same(255);

    style.visuals.widgets.noninteractive.fg_stroke.color = Color32::WHITE;
    style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;

    style.visuals.widgets.hovered.bg_fill = Color32::WHITE;
    style.visuals.widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.hovered.corner_radius = CornerRadius::same(255);

    style
}

fn init_sdl() -> Result<(
    sdl2::Sdl,
    sdl2::video::Window,
    sdl2::EventPump,
    Option<sdl2::controller::GameController>,
)> {
    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;

    // Initialize game controller subsystem
    let game_controller_subsystem = sdl_context.game_controller()?;
    let available = game_controller_subsystem.num_joysticks()?;

    // Attempt to open the first available game controller
    let controller = (0..available).find_map(|id| {
        if !game_controller_subsystem.is_game_controller(id) {
            return None;
        }

        match game_controller_subsystem.open(id) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("Failed to open controller {id}: {e:?}");
                None
            }
        }
    });

    // Create a window
    let window = video_subsystem
        .window(
            &format!("NextUI 更新器 {}", env!("CARGO_PKG_VERSION")),
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        )
        .position_centered()
        .opengl()
        .build()?;

    let event_pump = sdl_context.event_pump()?;

    Ok((sdl_context, window, event_pump, controller))
}

// Load font from file
fn load_font() -> Result<FontDefinitions> {
    fn get_font_preference() -> Result<usize> {
        // Load NextUI settings
        let mut settings_file =
            std::fs::File::open(SDCARD_ROOT.to_owned() + ".userdata/shared/minuisettings.txt")?;

        let mut settings = String::new();
        settings_file.read_to_string(&mut settings)?;

        // Very crappy parser
        Ok(settings.contains("font=1").into())
    }

    fn try_load_font_bytes(path: &std::path::Path) -> Option<Vec<u8>> {
        let mut bytes = vec![];
        std::fs::File::open(path)
            .and_then(|mut f| f.read_to_end(&mut bytes))
            .ok()
            .map(|_| bytes)
    }

    let preference = get_font_preference().unwrap_or(0);

    // Build a fallback list of fonts to maximize glyph coverage on-device.
    // Order matters: earlier fonts take precedence.
    let mut font_paths: Vec<PathBuf> = vec![];

    let mut preferred = PathBuf::from(SDCARD_ROOT);
    preferred.push(format!(".system/res/{}", FONTS[preference]));
    font_paths.push(preferred);

    // Common CJK-capable font on device
    let mut font2 = PathBuf::from(SDCARD_ROOT);
    font2.push(".system/res/font2.ttf");
    font_paths.push(font2);

    // Load any other fonts in .system/res as additional fallbacks.
    let mut res_dir = PathBuf::from(SDCARD_ROOT);
    res_dir.push(".system/res");
    if let Ok(entries) = std::fs::read_dir(&res_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !(ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf")) {
                continue;
            }
            font_paths.push(path);
        }
    }

    // De-dup while preserving order
    let mut deduped: Vec<PathBuf> = vec![];
    for p in font_paths {
        if !deduped.iter().any(|e| e == &p) {
            deduped.push(p);
        }
    }

    let mut font_data: BTreeMap<String, Arc<FontData>> = BTreeMap::new();
    let mut family_order: Vec<String> = vec![];

    for (idx, path) in deduped.iter().enumerate() {
        if let Some(bytes) = try_load_font_bytes(path.as_path()) {
            println!("Loading font: {}", path.display());
            let key = format!("font_{idx}");
            family_order.push(key.clone());
            font_data.insert(key, Arc::new(FontData::from_owned(bytes)));
        }
    }

    if family_order.is_empty() {
        return Err("No usable fonts found in .system/res".into());
    }

    let mut families = BTreeMap::new();
    families.insert(FontFamily::Proportional, family_order.clone());
    families.insert(FontFamily::Monospace, family_order);

    Ok(FontDefinitions {
        font_data,
        families,
    })
}

fn hint_wrap_nextui_tag(app_state: &'static AppStateManager, tag_name: &str) -> String {
    let mut selected_tag = format!("NextUI {tag_name}");
    if !app_state.release_selection_menu() {
        return selected_tag;
    }
    if !is_most_left_index(app_state) {
        selected_tag = format!("<<     {selected_tag}");
    }
    if !is_most_right_index(app_state) {
        selected_tag = format!("{selected_tag}     >>");
    }
    selected_tag
}

fn is_most_left_index(app_state: &'static AppStateManager) -> bool {
    let index = app_state.nextui_releases_and_tags_index().unwrap_or(0);
    let max_index = app_state
        .nextui_releases_and_tags()
        .unwrap_or_default()
        .len();
    index >= max_index - 1
}

fn is_most_right_index(app_state: &'static AppStateManager) -> bool {
    app_state.nextui_releases_and_tags_index() == Some(0)
}

fn handle_version_navigation(app_state: &'static AppStateManager, direction: i32) {
    if app_state.release_selection_menu() && app_state.release_selection_confirmed() {
        let index = app_state.nextui_releases_and_tags_index().unwrap_or(0);
        if direction < 0 && !is_most_left_index(app_state) {
            // Navigate left (older versions)
            app_state.set_nextui_releases_and_tags_index(Some(index + 1));
        } else if direction > 0 && !is_most_right_index(app_state) {
            // Navigate right (newer versions)
            app_state.set_nextui_releases_and_tags_index(Some(index - 1));
        }
    }
}

#[allow(clippy::too_many_lines)]
pub fn run_ui(app_state: &'static AppStateManager) -> Result<()> {
    // Initialize SDL and create window
    let (_sdl_context, window, mut event_pump, _controller) = init_sdl()?;

    // Create OpenGL context and egui painter
    let _gl_context = window.gl_create_context()?;
    let shader_ver = ShaderVersion::Adaptive;
    let (mut painter, mut egui_state) =
        egui_backend::with_sdl2(&window, shader_ver, DpiScaling::Custom(DPI_SCALE));

    // Create egui context and set style
    let egui_ctx = egui::Context::default();
    egui_ctx.set_style(setup_ui_style());

    // Font stuff
    if let Ok(fonts) = load_font() {
        egui_ctx.set_fonts(fonts);
    }

    let start_time: Instant = Instant::now();

    loop {
        if app_state.should_quit() {
            break;
        }

        egui_state.input.time = Some(start_time.elapsed().as_secs_f64());
        egui_ctx.begin_pass(egui_state.input.take());

        // UI rendering
        egui::CentralPanel::default().show(&egui_ctx, |ui| {
            ui.vertical_centered(|ui| {
                // Check application state
                let update_in_progress = app_state.current_operation().is_some();

                let title_prefix = format!("NextUI 更新器 {}", env!("CARGO_PKG_VERSION"));
                if app_state.release_selection_menu() {
                    if app_state.release_selection_confirmed() {
                        ui.label(
                            RichText::new(title_prefix + " 版本选择")
                                .color(Color32::from_rgb(150, 150, 150))
                                .size(10.0),
                        );
                    } else {
                        ui.label(
                            RichText::new(title_prefix + " 版本选择警告")
                                .color(Color32::from_rgb(150, 150, 150))
                                .size(10.0),
                        );
                    }
                } else {
                    ui.label(
                        RichText::new(title_prefix)
                            .color(Color32::from_rgb(150, 150, 150))
                            .size(10.0),
                    );
                }
                ui.add_space(4.0);

                ui.add_enabled_ui(!update_in_progress, |ui| {
                    let submenu = app_state.submenu();
                    let menu = match submenu {
                        Submenu::NextUI => nextui_ui(ui, app_state),
                    };

                    // Focus the first available button for controller navigation
                    ui.memory_mut(|r| {
                        if r.focused().is_none() {
                            r.request_focus(menu.id);
                        }
                    });
                });

                ui.add_space(8.0);

                // Display current operation
                if let Some(operation) = app_state.current_operation() {
                    ui.label(RichText::new(operation).color(Color32::from_rgb(150, 150, 150)).size(10.0));
                }

                // Display error if any
                if let Some(error) = app_state.error() {
                    ui.colored_label(Color32::from_rgb(255, 150, 150), RichText::new(error));
                }

                // Show progress bar if available
                if let Some(progress) = app_state.progress() {
                    match progress {
                        Progress::Indeterminate => {
                            ui.add_space(4.0);
                            ui.add(Spinner::new().color(Color32::WHITE));
                        }
                        Progress::Determinate(pr) => {
                            let mut progress_bar = ProgressBar::new(pr);
                            // Show percentage only if progress is > 10% to avoid text
                            // escaping the progress bar
                            if pr > 0.1 {
                                progress_bar = progress_bar.show_percentage();
                            }
                            ui.add(progress_bar);
                        }
                    }
                }
            });

            if !app_state.release_selection_menu() && app_state.current_operation().is_none() {
                egui::Area::new(egui::Id::new("version_selector_indicator"))
                    .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-2.0, -2.0))
                    .interactable(false)
                    .show(ui.ctx(), |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;

                            // Draw circle background for button
                            let button_size = 6.0;
                            let (rect, _response) = ui.allocate_exact_size(
                                Vec2::splat(button_size),
                                egui::Sense::empty(),
                            );
                            ui.painter().circle(
                                rect.center(),
                                button_size / 2.0,
                                Color32::from_rgb(60, 60, 60),
                                egui::Stroke::new(1.0, Color32::from_rgb(100, 100, 100)),
                            );
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "X",
                                egui::FontId::proportional(6.0),
                                Color32::from_rgb(180, 180, 180),
                            );

                            ui.label(
                                RichText::new("选择版本")
                                    .size(6.0)
                                    .color(Color32::from_rgb(100, 100, 100)),
                            );
                        });
                    });
            }

            if let Some(hint) = app_state.hint() {
                ui.allocate_new_ui(
                    egui::UiBuilder::new().max_rect(Rect {
                        min: Pos2 {
                            x: 0.0,
                            y: ui.max_rect().height() - 2.0,
                        },
                        max: Pos2 {
                            x: 1024.0 / DPI_SCALE,
                            y: ui.max_rect().height(),
                        },
                    }),
                    |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.label(RichText::new(hint).size(10.0));
                        });
                    },
                );
            }

            // HACK: for some reason dynamic text isn't rendered without this
            ui.allocate_ui(
                Vec2::ZERO,
                |ui| {
                    ui.label(
                        RichText::new(
                            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789~`!@#$%^&*()-=_+[]{};':\",.<>/?",
                        )
                        .size(10.0)
                        .color(Color32::TRANSPARENT)
                    );

                    // Main body labels/hints use ~10.0
                    ui.label(RichText::new(PREWARM_TEXT_ZH).size(10.0).color(Color32::TRANSPARENT));

                    // Buttons often use a larger default size; prewarm those too.
                    ui.label(RichText::new(PREWARM_TEXT_ZH).size(18.0).color(Color32::TRANSPARENT));

                    // Small indicator glyphs
                    ui.label(
                        RichText::new("X选择版本")
                            .size(6.0)
                            .color(Color32::TRANSPARENT),
                    );
                },
            );
        });

        // End frame and render
        let FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = egui_ctx.end_pass();

        let repaint_after = viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("Missing ViewportId::ROOT")
            .repaint_delay;

        // Process output
        egui_state.process_output(&window, &platform_output);

        // Paint and swap buffers
        let paint_jobs = egui_ctx.tessellate(shapes, pixels_per_point);
        painter.paint_jobs(None, textures_delta, paint_jobs);
        window.gl_swap_window();

        let handle_back_button = || {
            if app_state.release_selection_menu() {
                app_state.set_release_selection_menu(false);
            } else {
                app_state.set_should_quit(true);
            }
        };

        // Process events
        let mut process_event = |event| {
            match event {
                Event::Quit { .. } => app_state.set_should_quit(true),
                Event::ControllerButtonDown {
                    timestamp, button, ..
                } => {
                    if let Some(keycode) = controller_to_key(button) {
                        let key_event = Event::KeyDown {
                            keycode: Some(keycode),
                            timestamp,
                            window_id: window.id(),
                            scancode: Some(sdl2::keyboard::Scancode::Down),
                            keymod: sdl2::keyboard::Mod::empty(),
                            repeat: false,
                        };
                        egui_state.process_input(&window, key_event, &mut painter);
                    }
                }
                Event::ControllerButtonUp {
                    timestamp, button, ..
                } => {
                    if button == sdl2::controller::Button::A {
                        // Exit with "B" button
                        handle_back_button();
                    }

                    if app_state.release_selection_menu() {
                        // Handle left/right navigation in selection menu
                        if button == sdl2::controller::Button::DPadLeft {
                            handle_version_navigation(app_state, -1);
                        } else if button == sdl2::controller::Button::DPadRight {
                            handle_version_navigation(app_state, 1);
                        }
                    } else {
                        // Add X button to reach selection menu
                        if button == sdl2::controller::Button::Y {
                            app_state.set_release_selection_menu(true);
                        }
                    }

                    if let Some(keycode) = controller_to_key(button) {
                        let key_event = Event::KeyUp {
                            keycode: Some(keycode),
                            timestamp,
                            window_id: window.id(),
                            scancode: Some(sdl2::keyboard::Scancode::Down),
                            keymod: sdl2::keyboard::Mod::empty(),
                            repeat: false,
                        };

                        egui_state.process_input(&window, key_event, &mut painter);
                    }
                }
                // for easy testing on desktop
                Event::KeyDown { keycode, .. } => match keycode {
                    Some(sdl2::keyboard::Keycode::Escape) => handle_back_button(),
                    Some(sdl2::keyboard::Keycode::X) => app_state.set_release_selection_menu(true),
                    Some(sdl2::keyboard::Keycode::Left) => handle_version_navigation(app_state, -1),
                    Some(sdl2::keyboard::Keycode::Right) => handle_version_navigation(app_state, 1),
                    _ => {}
                },
                _ => {
                    // Process other input events
                    egui_state.process_input(&window, event, &mut painter);
                }
            }
        };

        if repaint_after.is_zero() {
            for event in event_pump.poll_iter() {
                process_event(event);
            }
        } else if let Some(event) = event_pump.wait_event_timeout(50) {
            process_event(event);
        }
    }

    Ok(())
}
