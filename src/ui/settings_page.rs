use super::*;
use egui::{Align, Layout};
use iphoneplay::settings::Theme;

const TABS: [(&str, char, &str); 5] = [
    ("外观", theme::SUN, "让界面适合你的使用习惯。"),
    ("画面与声音", theme::SCREEN, "调整电脑上的预览与声音播放。"),
    ("键鼠控制", theme::KEYBOARD, "明确连接，随时释放。"),
    ("录制与截图", theme::CAMERA, "保存画面，留住每个瞬间。"),
    ("高级与关于", theme::INFO, "连接选项、诊断与开源信息。"),
];

impl UiModel {
    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("settings_nav")
            .exact_size(if ui.available_width() < 880. {
                180.
            } else {
                212.
            })
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 18)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.;
                egui::ScrollArea::vertical()
                    .id_salt("settings_navigation_scroll")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            theme::icon(ui, theme::BACK, 18., ui.visuals().weak_text_color());
                            if ui.link("返回投屏").clicked() {
                                self.actions.push(Action::Back);
                            }
                        });
                        ui.add_space(14.);
                        ui.heading("设置");
                        ui.add_space(12.);
                        for (index, (label, icon, _)) in TABS.iter().enumerate() {
                            let selected = self.settings_tab == index;
                            let response = egui::Frame::new()
                                .fill(if selected {
                                    ui.visuals().selection.bg_fill
                                } else {
                                    egui::Color32::TRANSPARENT
                                })
                                .corner_radius(9)
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        theme::icon(
                                            ui,
                                            *icon,
                                            19.,
                                            if selected {
                                                self.primary()
                                            } else {
                                                ui.visuals().text_color()
                                            },
                                        );
                                        ui.add_space(2.);
                                        ui.label(RichText::new(*label).size(14.));
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());
                            if response.clicked() {
                                self.settings_tab = index;
                            }
                        }
                        if ui.available_height() > 32. {
                            ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                                secondary(ui, format!("iPhonePlay  {}", env!("CARGO_PKG_VERSION")));
                            });
                        }
                    });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 20)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.;
                    ui.set_width(ui.available_width().min(840.));
                    ui.label(
                        RichText::new(TABS[self.settings_tab].0)
                            .size(26.)
                            .family(egui::FontFamily::Name("heading".into())),
                    );
                    secondary(ui, TABS[self.settings_tab].2);
                    ui.add_space(16.);
                    match self.settings_tab {
                        0 => self.appearance(ui),
                        1 => self.picture_sound(ui),
                        2 => self.input_settings(ui),
                        3 => self.media_settings(ui),
                        _ => self.advanced(ui),
                    }
                    ui.add_space(16.);
                    secondary(ui, "更改会自动保存");
                });
            });
    }
    fn appearance(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("主题").size(17.).strong());
        secondary(ui, "可随 Windows 自动切换。");
        ui.add_space(8.);
        let themes = [Theme::System, Theme::Light, Theme::Dark];
        if ui.available_width() < 360. {
            for (i, theme_value) in themes.into_iter().enumerate() {
                let selected = self.settings.theme == theme_value;
                if theme_card(ui, self.theme_textures.get(i), theme_value, selected).clicked() {
                    self.settings.theme = theme_value;
                }
            }
        } else {
            ui.columns(3, |cols| {
                for (i, theme_value) in themes.into_iter().enumerate() {
                    let selected = self.settings.theme == theme_value;
                    if theme_card(
                        &mut cols[i],
                        self.theme_textures.get(i),
                        theme_value,
                        selected,
                    )
                    .clicked()
                    {
                        self.settings.theme = theme_value;
                    }
                }
            });
        }
        ui.add_space(10.);
        ui.separator();
        setting_row(
            ui,
            "界面缩放",
            "相对 Windows DPI 调整文字与控件大小",
            |ui| {
                egui::ComboBox::from_id_salt("ui_scale")
                    .selected_text(format!("{}%", (self.settings.scale * 100.).round()))
                    .width(150.)
                    .show_ui(ui, |ui| {
                        for scale in [0.75, 1., 1.25, 1.5, 1.75, 2.] {
                            ui.selectable_value(
                                &mut self.settings.scale,
                                scale,
                                format!("{}%", (scale * 100.) as u32),
                            );
                        }
                    });
            },
        );
        setting_row(ui, "界面语言", "首版提供简体中文", |ui| {
            ui.label("简体中文");
        });
        setting_row(
            ui,
            "减少动态效果",
            "简化页面切换与提示动画",
            |ui| {
                theme::toggle(ui, &mut self.settings.reduced_motion);
            },
        );
        ui.add_space(16.);
        ui.label(RichText::new("窗口行为").size(17.).strong());
        setting_row(
            ui,
            "关闭主窗口时",
            "收起到托盘后，投屏和录制仍在后台继续",
            |ui| {
                egui::ComboBox::from_id_salt("close_behavior")
                    .selected_text(if self.settings.close_to_tray {
                        "最小化到托盘"
                    } else {
                        "退出软件"
                    })
                    .width(150.)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.settings.close_to_tray, true, "最小化到托盘");
                        ui.selectable_value(&mut self.settings.close_to_tray, false, "退出软件");
                    });
            },
        );
        setting_row(
            ui,
            "记住窗口大小",
            "启动时恢复上次的窗口尺寸",
            |ui| {
                theme::toggle(ui, &mut self.settings.remember_window);
            },
        );
    }
    fn picture_sound(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("本地预览").size(17.).strong());
        secondary(ui, "这些上限用于电脑预览，不等同于手机采集质量或录制设置。");
        setting_row(
            ui,
            "预览分辨率上限",
            "按源画面比例显示，不添加机身外框",
            |ui| {
                egui::ComboBox::from_id_salt("preview_res")
                    .selected_text(format!("长边 {}", self.settings.preview_long_edge))
                    .show_ui(ui, |ui| {
                        for v in [960, 1280, 1920, 2560, 3840] {
                            ui.selectable_value(
                                &mut self.settings.preview_long_edge,
                                v,
                                format!("长边 {v}"),
                            );
                        }
                    });
            },
        );
        setting_row(
            ui,
            "预览帧率上限",
            "实际帧率受手机、连接与解码能力影响",
            |ui| {
                egui::ComboBox::from_id_salt("preview_fps")
                    .selected_text(format!("{} FPS", self.settings.preview_fps))
                    .show_ui(ui, |ui| {
                        for v in [30, 60] {
                            ui.selectable_value(
                                &mut self.settings.preview_fps,
                                v,
                                format!("{v} FPS"),
                            );
                        }
                    });
            },
        );
        setting_row(
            ui,
            "预览圆角",
            "仅改变本地预览，截图和录像保留完整画面",
            |ui| {
                theme::toggle(ui, &mut self.settings.preview_rounded);
            },
        );
        if self.settings.preview_rounded {
            setting_row(
                ui,
                "圆角半径",
                "按逻辑像素调整预览圆角",
                |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.settings.preview_corner_radius, 8.0..=32.0)
                            .suffix(" px"),
                    );
                },
            );
        }
        setting_row(
            ui,
            "悬浮窗置顶",
            "让悬浮预览显示在其他窗口上方，不影响截图和录像",
            |ui| {
                theme::toggle(ui, &mut self.settings.floating_always_on_top);
            },
        );
        ui.add_space(16.);
        ui.label(RichText::new("手机声音").size(17.).strong());
        setting_row(
            ui,
            "在电脑播放手机声音",
            "静音只影响本地播放，不影响录像中的手机声音",
            |ui| {
                theme::toggle(ui, &mut self.settings.play_audio);
            },
        );
        setting_row(ui, "播放音量", "不改变手机系统音量", |ui| {
            ui.add(egui::Slider::new(&mut self.settings.volume, 0.0..=1.0).show_value(false));
            ui.label(format!("{}%", (self.settings.volume * 100.) as u32));
        });
    }
    fn input_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            theme::icon(ui, theme::BLUETOOTH, 18., self.primary());
            ui.label(RichText::new("蓝牙连接").size(17.).strong());
        });
        ui.label(RichText::new(&self.control_status).size(14.));
        secondary(ui, format!("Windows 蓝牙名称：{}", self.bluetooth_name));
        ui.add_space(8.);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("打开反控连接引导")
                            .size(14.)
                            .color(Color32::WHITE),
                    )
                    .fill(self.primary())
                    .min_size(Vec2::new(156., 32.)),
                )
                .clicked()
            {
                self.actions.push(Action::ShowControlGuide);
            }
            if ui.button("打开 Windows 蓝牙设置").clicked() {
                self.actions.push(Action::OpenBluetoothSettings);
            }
        });
        ui.add_space(16.);
        ui.label(RichText::new("当前绑定设备").size(17.).strong());
        if let Some(bound) = self.bound_client.as_deref() {
            let name = self
                .clients
                .iter()
                .find(|(id, _)| id == bound)
                .map(|(_, name)| name.as_str())
                .unwrap_or(bound);
            ui.label(name);
        } else {
            secondary(ui, "尚未绑定。连接后，请手动确认正在投屏的 iPhone。");
        }
        ui.add_space(8.);
        ui.horizontal_wrapped(|ui| {
            if ui.button("重新配对").clicked() {
                self.actions.push(Action::ForgetBinding);
                self.actions.push(Action::ShowControlGuide);
            }
            if ui
                .add_enabled(
                    self.bound_client.is_some(),
                    egui::Button::new("解除应用内绑定"),
                )
                .clicked()
            {
                self.actions.push(Action::ForgetBinding);
            }
        });
        secondary(ui, "解除应用内绑定不会删除 Windows 的系统配对记录。");
        if !self.clients.is_empty() {
            ui.add_space(12.);
            ui.label(RichText::new("确认当前手机").size(15.));
            secondary(ui, "仅列出已订阅键鼠服务的设备；名称不会用于自动绑定。");
            for (id, name) in &self.clients {
                ui.horizontal_wrapped(|ui| {
                    ui.label(name);
                    if self.bound_client.as_deref() == Some(id.as_str()) {
                        secondary(ui, "已绑定");
                    } else if ui.button("这是我的 iPhone").clicked() {
                        self.actions.push(Action::ConfirmClient(id.clone()));
                    }
                });
            }
        } else {
            ui.add_space(8.);
            secondary(ui, "当前没有可确认的蓝牙设备。");
        }
        ui.add_space(16.);
        ui.separator();
        setting_row(
            ui,
            "鼠标灵敏度",
            "调整手机端指针的相对移动速度",
            |ui| {
                ui.add(
                    egui::Slider::new(&mut self.settings.mouse_sensitivity, 0.25..=3.0).suffix("×"),
                );
            },
        );
        setting_row(ui, "滚轮灵敏度", "调整滚动量", |ui| {
            ui.add(egui::Slider::new(&mut self.settings.wheel_sensitivity, 0.25..=3.0).suffix("×"));
        });
        setting_row(
            ui,
            "释放键鼠控制",
            "失焦、进入设置和最小化也会释放；恢复焦点不会自动接管",
            |ui| {
                ui.label(RichText::new("F9").monospace().strong());
            },
        );
        ui.add_space(16.);
        secondary(
            ui,
            "只有你主动开启控制、投屏区域获得焦点且绑定设备有效时，才会向手机发送键鼠输入。",
        );
    }
    fn media_settings(&mut self, ui: &mut egui::Ui) {
        let record_dir = self.settings.recording_dir.display().to_string();
        let shot_dir = self.settings.screenshot_dir.display().to_string();
        setting_row(ui, "录像保存目录", &record_dir, |ui| {
            if ui.button("更改").clicked() {
                self.actions.push(Action::PickDirectory(false));
            }
        });
        setting_row(ui, "截图保存目录", &shot_dir, |ui| {
            if ui.button("更改").clicked() {
                self.actions.push(Action::PickDirectory(true));
            }
        });
        setting_row(
            ui,
            "录制分辨率上限",
            "不放大源画面；横竖屏切换会等比例补边",
            |ui| {
                egui::ComboBox::from_id_salt("record_res")
                    .selected_text(format!("长边 {}", self.settings.recording_long_edge))
                    .show_ui(ui, |ui| {
                        for v in [1280, 1920, 2560, 3840] {
                            ui.selectable_value(
                                &mut self.settings.recording_long_edge,
                                v,
                                format!("长边 {v}"),
                            );
                        }
                    });
            },
        );
        setting_row(
            ui,
            "录制画质",
            "H.264 视频与 AAC 音频，保存为 MP4",
            |ui| {
                egui::ComboBox::from_id_salt("record_quality")
                    .selected_text(match self.settings.recording_bitrate {
                        6000 => "标准",
                        20000 => "高",
                        _ => "推荐",
                    })
                    .show_ui(ui, |ui| {
                        for (v, label) in [(6000, "标准"), (12000, "推荐"), (20000, "高")] {
                            ui.selectable_value(&mut self.settings.recording_bitrate, v, label);
                        }
                    });
            },
        );
        setting_row(ui, "录制帧率", "选择固定输出帧率", |ui| {
            egui::ComboBox::from_id_salt("record_fps")
                .selected_text(format!("{} FPS", self.settings.recording_fps))
                .show_ui(ui, |ui| {
                    for v in [30, 60] {
                        ui.selectable_value(
                            &mut self.settings.recording_fps,
                            v,
                            format!("{v} FPS"),
                        );
                    }
                });
        });
        setting_row(
            ui,
            "录制手机声音",
            "不录制麦克风；电脑播放音量不影响录像",
            |ui| {
                theme::toggle(ui, &mut self.settings.record_audio);
            },
        );
        ui.add_space(16.);
        secondary(
            ui,
            "截图始终取自原始解码画面，保存为 PNG，不包含软件界面或额外外框。",
        );
        if self.recording_busy() {
            ui.add_space(12.);
            ui.label("当前录像继续使用开始时的设置；更改将在下一次录制生效。");
        }
    }
    fn advanced(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("AirPlay 接收名称").size(17.).strong());
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.receiver_name)
                    .desired_width((ui.available_width() - 112.).clamp(140., 360.))
                    .char_limit(80),
            );
            if ui
                .add_enabled(
                    self.capture != CaptureState::Streaming,
                    egui::Button::new("应用名称"),
                )
                .clicked()
            {
                self.actions.push(Action::RestartReceiver);
            }
        });
        secondary(
            ui,
            "已有投屏时，请先断开后再应用接收名称。蓝牙配对显示 Windows 实际电脑名称。",
        );
        ui.add_space(16.);
        setting_row(
            ui,
            "USB 兼容模式",
            "更改将在下一次 USB 连接生效",
            |ui| {
                egui::ComboBox::from_id_salt("usb_mode")
                    .selected_text(match self.settings.usb_mode {
                        1 => "模式 B",
                        2 => "模式 C",
                        _ => "模式 A（默认）",
                    })
                    .show_ui(ui, |ui| {
                        for (v, l) in [(0, "模式 A（默认）"), (1, "模式 B"), (2, "模式 C")]
                        {
                            ui.selectable_value(&mut self.settings.usb_mode, v, l);
                        }
                    });
            },
        );
        setting_row(
            ui,
            "USB 驱动管理",
            "独立工具；只有点击后才会打开",
            |ui| {
                if ui.button("打开管理器").clicked() {
                    self.actions.push(Action::DriverManager);
                }
            },
        );
        setting_row(
            ui,
            "运行依赖",
            "检查投屏核心、无线宿主与编码器",
            |ui| {
                if ui.button("检查依赖").clicked() {
                    self.actions.push(Action::InspectDependencies);
                }
            },
        );
        setting_row(
            ui,
            "诊断日志",
            "包含运行状态和设备信息，可用于排查连接问题",
            |ui| {
                if ui.button("导出日志").clicked() {
                    self.actions.push(Action::ExportLogs);
                }
            },
        );
        ui.add_space(20.);
        ui.heading("iPhonePlay");
        secondary(
            ui,
            format!("版本 {} · Windows x64", env!("CARGO_PKG_VERSION")),
        );
        ui.add_space(12.);
        ui.label("Rust 原生界面与状态管理，复用 iPhoneMirror 的投屏核心。");
        secondary(
            ui,
            "GPL-3.0-only · © iPhoneMirror 原作者 RayrenSX 及贡献者。第三方组件版权归各自作者所有。",
        );
        ui.horizontal(|ui| {
            if ui.link("开源许可").clicked() {
                self.actions
                    .push(Action::OpenPath(PathBuf::from("LICENSE")));
            }
            if ui.link("第三方声明").clicked() {
                self.actions
                    .push(Action::OpenPath(PathBuf::from("THIRD_PARTY_NOTICES.md")));
            }
        });
    }
}

fn secondary(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(12.)
                .color(ui.visuals().weak_text_color()),
        )
        .wrap(),
    );
}

fn setting_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(0, 6))
            .show(ui, |ui| {
                let description_ui = |ui: &mut egui::Ui| {
                    ui.spacing_mut().item_spacing.y = 2.;
                    ui.label(RichText::new(title).size(15.));
                    ui.add(
                        egui::Label::new(
                            RichText::new(description)
                                .size(12.)
                                .color(ui.visuals().weak_text_color()),
                        )
                        .wrap(),
                    );
                };
                if ui.available_width() < 470. {
                    description_ui(ui);
                    ui.add_space(4.);
                    ui.horizontal_wrapped(control);
                } else {
                    ui.horizontal(|ui| {
                        let width = ui.available_width() - 224.;
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 34.),
                            Layout::top_down(Align::LEFT),
                            description_ui,
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), control);
                    });
                }
            });
        ui.separator();
    });
}

fn theme_card(
    ui: &mut egui::Ui,
    texture: Option<&egui::TextureHandle>,
    theme: Theme,
    selected: bool,
) -> egui::Response {
    let width = ui.available_width();
    let preview_height = ((width - 16.) * 0.6).clamp(56., 95.);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, preview_height + 52.), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::accesskit::Role::RadioButton,
            ui.is_enabled(),
            selected,
            theme.label(),
        )
    });
    let border = if selected {
        Color32::from_rgb(48, 145, 244)
    } else {
        ui.visuals().window_stroke.color
    };
    ui.painter().rect(
        rect,
        9,
        ui.visuals().faint_bg_color,
        egui::Stroke::new(if selected { 2. } else { 1. }, border),
        egui::StrokeKind::Inside,
    );
    let preview = egui::Rect::from_min_size(
        rect.min + Vec2::splat(8.),
        Vec2::new(width - 16., preview_height),
    );
    if let Some(texture) = texture {
        let source_size = texture.size_vec2();
        let scale = (preview.width() / source_size.x).min(preview.height() / source_size.y);
        let image_size = source_size * scale;
        ui.put(
            egui::Rect::from_center_size(preview.center(), image_size),
            egui::Image::new(texture)
                .fit_to_exact_size(image_size)
                .maintain_aspect_ratio(true),
        );
    } else {
        ui.put(preview, egui::Label::new("主题预览加载中").wrap());
    }
    let label_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 8., preview.bottom() + 6.),
        Vec2::new(width - 16., 30.),
    );
    let mut checked = selected;
    let selector = ui.put(
        label_rect,
        egui::Checkbox::new(&mut checked, RichText::new(theme.label()).size(14.)),
    );
    response.union(selector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_previews_keep_the_entire_image_and_its_aspect_ratio() {
        let context = egui::Context::default();
        theme::configure(&context);
        theme::apply(&context, &iphoneplay::settings::Settings::default());
        for source_size in [[360, 200], [200, 360]] {
            let texture = context.load_texture(
                "theme_preview_test",
                egui::ColorImage::filled(source_size, Color32::WHITE),
                egui::TextureOptions::LINEAR,
            );
            for card_width in [108., 240.] {
                let mut card = egui::Rect::NOTHING;
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(card_width, 200.),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        card = theme_card(ui, Some(&texture), Theme::System, true).rect;
                    },
                );
                let mut image_rect = egui::Rect::NOTHING;
                let mut image_uv = egui::Rect::NOTHING;
                for clipped in &output.shapes {
                    if let egui::epaint::Shape::Rect(rectangle) = &clipped.shape
                        && let Some(brush) = &rectangle.brush
                        && brush.fill_texture_id == texture.id()
                    {
                        image_rect = image_rect.union(rectangle.rect);
                        image_uv = image_uv.union(brush.uv);
                    }
                }
                output.drop_without_applying_deltas();
                assert!(image_rect.is_positive());
                assert!(card.contains_rect(image_rect));
                assert!(card.max.x <= card_width + 0.1);
                assert_eq!(
                    image_uv,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    "theme previews must not crop their texture: {image_uv:?}, source {source_size:?}, card width {card_width}"
                );
                let expected_ratio = source_size[0] as f32 / source_size[1] as f32;
                assert!((image_rect.width() / image_rect.height() - expected_ratio).abs() < 0.01);
            }
        }
    }

    #[test]
    fn settings_pages_fit_narrow_and_standard_window_widths() {
        for size in [[480., 320.], [640., 450.], [960., 640.], [1280., 900.]] {
            let context = egui::Context::default();
            theme::configure(&context);
            let mut model = UiModel::new(iphoneplay::settings::Settings::default());
            model.page = Page::Settings;
            model.settings.preview_rounded = true;
            model.clients = vec![("test-id".into(), "手动确认的测试手机".into())];
            for tab in 0..TABS.len() {
                model.settings_tab = tab;
                let mut content = egui::Rect::NOTHING;
                for _ in 0..2 {
                    context
                        .run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::from(size),
                                )),
                                ..Default::default()
                            },
                            |ui| {
                                model.show(ui);
                                content = ui.min_rect();
                            },
                        )
                        .drop_without_applying_deltas();
                }
                assert!(
                    content.max.x <= size[0] + 0.1,
                    "settings tab {tab} overflows at {size:?}: {content:?}"
                );
            }
            assert!(model.actions.is_empty());
        }
    }
}
