use super::*;
use egui::{Align, Align2, Layout, Pos2, Rect, Sense};

impl UiModel {
    pub(super) fn welcome(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("welcome_footer")
            .exact_size(54.)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(32, 8)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    theme::icon(ui, theme::USB, 18., ui.visuals().weak_text_color());
                    if ui.link("使用 USB 连接").clicked() {
                        self.actions
                            .push(Action::Transition(Transition::Switch(Source::Usb)));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        secondary(ui, "仅在本机与局域网内传输");
                        theme::icon(ui, theme::LOCK, 17., ui.visuals().weak_text_color());
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(40, 12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let compact = ui.available_height() < 700.;
                    ui.add_space(if compact { 14. } else { 36. });
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("把 iPhone，带到电脑上。")
                                .family(egui::FontFamily::Name("heading".into()))
                                .size(if compact { 28. } else { 32. })
                                .strong(),
                        );
                        secondary(ui, "打开屏幕镜像，即可开始。");
                        ui.add_space(12.);
                        if let Some(t) = &self.welcome_texture {
                            ui.add(egui::Image::new(t).fit_to_exact_size(Vec2::new(
                                if compact { 360. } else { 440. },
                                if compact { 146. } else { 180. },
                            )));
                        } else {
                            ui.add_space(24.);
                            theme::icon(ui, theme::MIRROR, 76., self.primary());
                            ui.add_space(24.);
                        }
                        ui.add_space(10.);
                        ui.horizontal(|ui| {
                            let name_width = ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    self.settings.receiver_name.clone(),
                                    egui::FontId::proportional(23.),
                                    ui.visuals().text_color(),
                                )
                                .size()
                                .x
                            });
                            ui.add_space(((ui.available_width() - name_width - 44.) / 2.).max(0.));
                            ui.label(RichText::new(&self.settings.receiver_name).size(23.));
                            if theme::icon_button(ui, theme::COPY, "复制接收名称").clicked() {
                                ui.ctx().copy_text(self.settings.receiver_name.clone());
                                self.notify("已复制接收名称", None);
                            }
                        });
                        ui.horizontal(|ui| {
                            let text = if self.receiver_ready {
                                "接收端已就绪 · 等待 iPhone 连接"
                            } else {
                                &self.status
                            };
                            let color = if self.receiver_ready {
                                Color32::from_rgb(67, 192, 103)
                            } else {
                                ui.visuals().weak_text_color()
                            };
                            let width = ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    text.to_owned(),
                                    egui::FontId::proportional(15.),
                                    color,
                                )
                                .size()
                                .x
                            });
                            ui.add_space(((ui.available_width() - width - 18.) / 2.).max(0.));
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(10.), Sense::hover());
                            ui.painter().circle_filled(r.center(), 4., color);
                            ui.label(RichText::new(text).size(15.));
                        });
                        if self.capture == CaptureState::Failed && ui.link("检查运行依赖").clicked()
                        {
                            self.actions.push(Action::InspectDependencies);
                        }
                    });
                    ui.add_space(if compact { 22. } else { 36. });
                    ui.columns(3, |cols| {
                        step(
                            &mut cols[0],
                            "1",
                            "连接同一局域网",
                            "iPhone 与电脑接入同一网络，电脑也可以使用网线。",
                        );
                        step(
                            &mut cols[1],
                            "2",
                            "打开「屏幕镜像」",
                            "在 iPhone 控制中心，轻点“屏幕镜像”。",
                        );
                        step(
                            &mut cols[2],
                            "3",
                            "选择此电脑",
                            "在设备列表中选择上方显示的接收名称。",
                        );
                    });
                    ui.vertical_centered(|ui| {
                        if primary_button(ui, "查看连接指引").clicked() {
                            self.help = Some("连接指引");
                        }
                        if ui.link("找不到电脑？").clicked() {
                            self.help = Some("找不到电脑");
                        }
                    });
                    ui.add_space(22.);
                    ui.separator();
                    ui.add_space(10.);
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .stroke(ui.visuals().window_stroke)
                        .corner_radius(10)
                        .inner_margin(18)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                theme::icon(ui, theme::MOUSE, 28., ui.visuals().text_color());
                                ui.add_space(10.);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("想用电脑操作 iPhone？").strong());
                                    secondary(ui, "连接后，可继续设置蓝牙反控");
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    secondary(ui, "可稍后设置");
                                });
                            });
                        });
                });
            });
    }

    pub(super) fn usb(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(36)).show(ui,|ui|{
            if ui.button("返回 AirPlay 连接").clicked(){self.actions.push(Action::Transition(Transition::Switch(Source::AirPlay)));}
            egui::ScrollArea::vertical().show(ui,|ui|{ui.vertical_centered(|ui|{ui.add_space(20.);theme::icon(ui,theme::USB,48.,self.primary());ui.heading("使用 USB 连接 iPhone");secondary(ui,"一根数据线，稳定连接。");});ui.add_space(28.);
                let width=ui.available_width().min(740.);let margin=(ui.available_width()-width)/2.;
                ui.horizontal(|ui|{ui.add_space(margin);ui.vertical(|ui|{ui.set_width(width);
                    step(ui,"1","连接 iPhone","使用支持数据传输的 USB 线连接手机与电脑。");
                    step(ui,"2","解锁并信任这台电脑","保持手机解锁，在 iPhone 弹出的提示中轻点“信任”，然后输入锁屏密码。");
                    step(ui,"3","检查设备与驱动","如缺少驱动，请手动打开驱动管理器；修复后点“重新检测”。");
                    ui.horizontal(|ui|{if ui.button("重新检测").clicked(){self.actions.push(Action::RefreshDevices);}
if ui.link("打开驱动管理器").clicked(){self.actions.push(Action::DriverManager);}});
                    ui.add_space(15.);
                    if self.usb_devices.is_empty(){secondary(ui,"尚未检测到可用 iPhone。请检查数据线，并完成手机上的信任提示。");}
                    for (index,(name,status,ready)) in self.usb_devices.clone().iter().enumerate(){egui::Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(16).corner_radius(10).show(ui,|ui|{ui.set_width(width-32.);ui.horizontal(|ui|{ui.vertical(|ui|{ui.strong(name);secondary(ui,status);});ui.with_layout(Layout::right_to_left(Align::Center),|ui|{if ui.add_enabled(*ready,egui::Button::new("开始投屏")).clicked(){self.actions.push(Action::ConnectUsb(index));}});});});}
                    ui.add_space(24.);secondary(ui,"USB 只负责投屏；键鼠控制仍通过蓝牙连接。");
                });});
            });
        });
    }

    pub(super) fn workspace(&mut self, ui: &mut egui::Ui, recovery: bool) {
        let compact_workspace = ui.available_width() < 900. || ui.available_height() < 420.;
        egui::Panel::top("session")
            .exact_size(if compact_workspace { 42. } else { 62. })
            .frame(egui::Frame::new().inner_margin(if compact_workspace {
                egui::Margin::symmetric(12, 4)
            } else {
                egui::Margin::symmetric(30, 10)
            }))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if compact_workspace {
                        ui.spacing_mut().item_spacing.x = 6.;
                    }
                    let name = if self.device_name.is_empty() {
                        "我的 iPhone"
                    } else {
                        &self.device_name
                    };
                    // Keep the status, source and disconnect action reachable
                    // even when the phone has a long user-defined name.
                    let name_limit = (ui.available_width()
                        - if compact_workspace { 300. } else { 450. })
                    .clamp(40., 260.);
                    let name_width = ui.fonts_mut(|fonts| {
                        fonts
                            .layout_no_wrap(
                                name.to_owned(),
                                egui::FontId::proportional(16.),
                                ui.visuals().text_color(),
                            )
                            .size()
                            .x
                            .min(name_limit)
                    });
                    ui.add_sized(
                        [name_width, 24.],
                        egui::Label::new(RichText::new(name).size(16.).strong()).truncate(),
                    )
                    .on_hover_text(name);
                    ui.separator();
                    ui.colored_label(
                        if recovery {
                            Color32::from_rgb(232, 176, 78)
                        } else {
                            Color32::from_rgb(67, 192, 103)
                        },
                        if recovery {
                            "连接已中断"
                        } else {
                            "投屏已连接"
                        },
                    );
                    ui.separator();
                    if !compact_workspace {
                        theme::icon(ui, theme::SCREEN, 18., ui.visuals().text_color());
                    }
                    ui.label(if self.source == Source::Usb {
                        "USB"
                    } else {
                        "AirPlay"
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("断开连接").clicked() {
                            self.actions
                                .push(Action::Transition(Transition::Disconnect));
                        }
                        if !compact_workspace
                            && ui
                                .add_enabled(
                                    !recovery && !self.demo,
                                    egui::Button::new(if self.floating {
                                        "收回投屏"
                                    } else {
                                        "悬浮窗口"
                                    }),
                                )
                                .clicked()
                        {
                            self.actions.push(Action::ToggleFloating);
                        }
                    });
                });
            });
        let narrow_guide = (self.control_open || recovery) && compact_workspace;
        if (self.control_open || recovery) && !narrow_guide {
            egui::Panel::right("guide_rail")
                .exact_size((ui.available_width() * 0.32).clamp(310., 400.))
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .inner_margin(24),
                )
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if recovery {
                            self.recovery_rail(ui);
                        } else {
                            self.control_rail(ui);
                        }
                    });
                });
        }
        let compact_toolbar = compact_workspace || ui.available_width() < 580.;
        egui::Panel::bottom("video_toolbar")
            .exact_size(if compact_toolbar { 76. } else { 128. })
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(
                12,
                if compact_toolbar { 4 } else { 8 },
            )))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if compact_toolbar {
                        ui.spacing_mut().item_spacing.y = 4.;
                    }
                    ui.horizontal(|ui| {
                        let width = if compact_toolbar {
                            ui.available_width()
                        } else {
                            540.
                        };
                        ui.add_space(((ui.available_width() - width) / 2.).max(0.));
                        egui::Frame::new()
                            .fill(ui.visuals().faint_bg_color)
                            .stroke(ui.visuals().window_stroke)
                            .corner_radius(12)
                            .inner_margin(egui::Margin::symmetric(
                                12,
                                if compact_toolbar { 3 } else { 4 },
                            ))
                            .show(ui, |ui| {
                                self.toolbar_controls(ui, recovery, compact_toolbar);
                            });
                    });
                    let status = if self.recording_busy() {
                        match self.recording {
                            RecordingState::Finalizing => "正在保存录像…".to_owned(),
                            RecordingState::Starting => "正在准备录制…".to_owned(),
                            _ => {
                                let seconds = self
                                    .record_started
                                    .map(|i| i.elapsed().as_secs())
                                    .unwrap_or(0);
                                format!("录制中  {:02}:{:02}", seconds / 60, seconds % 60)
                            }
                        }
                    } else {
                        self.control_status.clone()
                    };
                    let color = if self.recording_busy() {
                        Color32::from_rgb(241, 112, 112)
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(&status).size(14.).color(color)).truncate(),
                    )
                    .on_hover_text(status);
                });
            });
        if narrow_guide {
            // At high app zoom, a fixed side rail would consume most of the
            // video and toolbar width. Keep the full-width toolbar accessible;
            // move the optional guide below the video with a scrollable body.
            let body_height = (ui.available_height() * 0.45 - 42.).clamp(24., 190.);
            let initially_open = ui.available_height() >= 420.;
            let use_modal = ui.available_height() < 250.;
            let guide_title = if recovery {
                "连接恢复指引"
            } else {
                "连接蓝牙反控"
            };
            if use_modal && std::mem::take(&mut self.control_expand_request) {
                self.help = Some(guide_title);
            }
            egui::Panel::bottom("compact_guide")
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .inner_margin(egui::Margin::symmetric(12, 3)),
                )
                .show(ui, |ui| {
                    if use_modal {
                        if ui.button(format!("打开{guide_title}")).clicked() {
                            self.help = Some(guide_title);
                        }
                        return;
                    }
                    egui::CollapsingHeader::new(if recovery {
                        "连接恢复指引"
                    } else {
                        "蓝牙反控引导"
                    })
                    .id_salt("compact_guide_heading")
                    .default_open(initially_open)
                    .open(if std::mem::take(&mut self.control_expand_request) {
                        Some(true)
                    } else {
                        None
                    })
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("compact_guide_scroll")
                            .max_height(body_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if recovery {
                                    self.recovery_rail(ui);
                                } else {
                                    self.control_rail(ui);
                                }
                            });
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(28, 4)))
            .show(ui, |ui| {
                let available = ui.available_rect_before_wrap();
                let source = if self.source_size.contains(&0) {
                    [1170, 2532]
                } else {
                    self.source_size
                };
                let size =
                    iphoneplay::state::fit_aspect([available.width(), available.height()], source);
                let rect = Rect::from_center_size(available.center(), Vec2::new(size[0], size[1]));
                if self.floating {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(available), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space((available.height() * 0.35).max(12.));
                            theme::icon(ui, theme::FLOATING, 36., self.primary());
                            ui.heading("画面已移至悬浮窗口");
                            secondary(ui, "可在其他应用上方查看，录制会继续。");
                            if ui.button("收回主窗口").clicked() {
                                self.actions.push(Action::ToggleFloating);
                            }
                        });
                    });
                } else if recovery {
                    ui.painter()
                        .rect_filled(rect, 0, Color32::from_rgb(24, 29, 38));
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "画面已暂停",
                        egui::FontId::proportional(22.),
                        Color32::from_rgb(174, 183, 198),
                    );
                } else if self.demo {
                    if let Some(texture) = &self.demo_texture {
                        ui.painter().image(
                            texture.id(),
                            rect,
                            Rect::from_min_max(
                                Pos2::new(560. / 1487., 143. / 1058.),
                                Pos2::new(926. / 1487., 866. / 1058.),
                            ),
                            Color32::WHITE,
                        );
                    }
                } else {
                    self.video_rect = Some(rect);
                    ui.painter().rect_filled(rect, 0, Color32::BLACK);
                }
                ui.allocate_rect(available, Sense::hover());
            });
    }

    /// Returns the actual hit rectangles for headless layout verification.
    fn toolbar_controls(&mut self, ui: &mut egui::Ui, recovery: bool, compact: bool) -> [Rect; 6] {
        let mut rectangles = [Rect::NOTHING; 6];
        ui.horizontal(|ui| {
            let compact_width = if compact {
                ui.spacing_mut().item_spacing.x = 6.;
                ((ui.available_width() - 5. * 6.) / 6.).max(1.)
            } else {
                76.
            };
            for (index, rectangle) in rectangles.iter_mut().enumerate().take(5) {
                let (icon, label, accent) = match index {
                    0 => (
                        if self.settings.play_audio {
                            theme::SOUND
                        } else {
                            theme::MUTE
                        },
                        if compact && !self.settings.play_audio {
                            "已静音"
                        } else {
                            "声音"
                        },
                        false,
                    ),
                    1 => (theme::CAMERA, "截图", false),
                    2 => (
                        theme::RECORD,
                        if self.recording_busy() {
                            "停止录制"
                        } else {
                            "录制"
                        },
                        self.recording_busy(),
                    ),
                    3 => (theme::FULLSCREEN, "全屏", false),
                    _ => (
                        theme::MOUSE,
                        if self.control_active {
                            "释放控制"
                        } else {
                            "开启反控"
                        },
                        true,
                    ),
                };
                let enabled = match index {
                    1 | 2 => !recovery && !self.demo,
                    4 => !recovery,
                    _ => true,
                };
                let response = ui
                    .add_enabled_ui(enabled, |ui| {
                        if compact {
                            let mut text = RichText::new(label).size(12.);
                            if accent {
                                text = text.color(Color32::from_rgb(77, 163, 250));
                            }
                            ui.add_sized([compact_width, 36.], egui::Button::new(text).frame(false))
                                .on_hover_text(label)
                        } else {
                            theme::toolbar_button(ui, icon, label, accent)
                        }
                    })
                    .inner;
                *rectangle = response.rect;
                if response.clicked() {
                    match index {
                        0 => self.settings.play_audio = !self.settings.play_audio,
                        1 => self.actions.push(Action::Screenshot),
                        2 => self.actions.push(if self.recording_busy() {
                            Action::StopRecording
                        } else {
                            Action::StartRecording
                        }),
                        3 => self.actions.push(Action::Fullscreen),
                        _ => {
                            if self.control_active {
                                self.actions.push(Action::ReleaseControl);
                            } else {
                                self.control_open = true;
                                self.actions.push(Action::ShowControlGuide);
                            }
                        }
                    }
                }
            }
            if compact {
                let response = ui.add_sized(
                    [compact_width, 36.],
                    egui::Button::new(RichText::new("更多").size(12.)).frame(false),
                );
                rectangles[5] = response.rect;
                egui::Popup::menu(&response).show(|ui| self.workspace_more_menu(ui));
            } else {
                rectangles[5] = ui
                    .menu_button(theme::icon_text(theme::MORE, 24.), |ui| {
                        self.workspace_more_menu(ui)
                    })
                    .response
                    .rect;
            }
        });
        rectangles
    }

    fn workspace_more_menu(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                !self.demo && self.capture == CaptureState::Streaming,
                egui::Button::new(if self.floating {
                    "收回主窗口"
                } else {
                    "独立悬浮窗口"
                }),
            )
            .clicked()
        {
            self.actions.push(Action::ToggleFloating);
            ui.close();
        }
        ui.checkbox(&mut self.settings.preview_rounded, "圆角预览");
        ui.separator();
        if ui.button("设置").clicked() {
            self.actions.push(Action::ShowSettings);
            ui.close();
        }
        if ui
            .button(if self.source == Source::AirPlay {
                "切换到 USB"
            } else {
                "切换到 AirPlay"
            })
            .clicked()
        {
            self.actions.push(Action::Transition(Transition::Switch(
                if self.source == Source::AirPlay {
                    Source::Usb
                } else {
                    Source::AirPlay
                },
            )));
            ui.close();
        }
        if ui.button("打开录像目录").clicked() {
            self.actions
                .push(Action::OpenPath(self.settings.recording_dir.clone()));
            ui.close();
        }
        if ui.button("退出 iPhonePlay").clicked() {
            self.actions.push(Action::Transition(Transition::Exit));
            ui.close();
        }
    }

    fn control_rail(&mut self, ui: &mut egui::Ui) {
        ui.heading("连接蓝牙反控");
        self.control_guide_content(ui);
    }

    pub(super) fn control_guide_content(&mut self, ui: &mut egui::Ui) {
        use iphoneplay::control::ControlState;
        secondary(ui, "投屏保持连接，按下面的步骤连接键鼠。");
        ui.add_space(8.);
        egui::Frame::new()
            .fill(ui.visuals().selection.bg_fill)
            .corner_radius(8)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(match self.control_state {
                        ControlState::Off => "尚未开启服务",
                        ControlState::Checking => "正在准备蓝牙服务",
                        ControlState::WaitingPairing => "等待手机连接",
                        ControlState::WaitingConfirmation => "请选择当前手机",
                        ControlState::Ready => "设备已确认，可以开始控制",
                        ControlState::Active => "正在控制 · F9 释放",
                        ControlState::Paused => "控制已暂停",
                        ControlState::Failed => "蓝牙连接需要处理",
                    })
                    .strong(),
                );
                ui.add(egui::Label::new(RichText::new(&self.control_status).size(12.)).wrap());
            });
        ui.add_space(12.);
        step(
            ui,
            "1",
            "开启辅助触控",
            "在 iPhone 打开 设置 → 辅助功能 → 触控 → 辅助触控。",
        );
        ui.checkbox(&mut self.assistive_confirmed, "我已在手机上开启辅助触控");
        ui.add_space(12.);
        step(
            ui,
            "2",
            "在手机上连接电脑",
            &format!(
                "进入辅助触控 → 设备 → 蓝牙设备，选择“{}”，完成连接。",
                self.bluetooth_name
            ),
        );
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("复制电脑名称").clicked() {
                ui.ctx().copy_text(self.bluetooth_name.clone());
            }
            if ui.small_button("Windows 蓝牙设置").clicked() {
                self.actions.push(Action::OpenBluetoothSettings);
            }
        });
        ui.add_space(12.);
        step(
            ui,
            "3",
            "确认当前手机",
            "手机接入键鼠服务后，会显示在这里。请手动确认正在投屏的设备。",
        );
        for (id, name) in self.clients.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(name);
                if self.bound_client.as_deref() == Some(id.as_str()) {
                    ui.colored_label(self.primary(), "已确认");
                } else if ui.button("这是我的 iPhone").clicked() {
                    self.actions.push(Action::ConfirmClient(id));
                }
            });
        }
        if self.clients.is_empty() {
            secondary(
                ui,
                "尚未检测到手机接入。已配对的手机也请在上面的入口点一下电脑名称，重新连接。",
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button("重新检查连接").clicked() {
                    self.actions.push(Action::StartControl);
                }
                ui.hyperlink_to("手机端操作说明", "https://support.apple.com/zh-cn/111775");
            });
        }
        ui.add_space(12.);
        if self.control_ready {
            ui.add_enabled_ui(self.assistive_confirmed && !self.demo, |ui| {
                if primary_button(ui, "开始控制").clicked() {
                    if self.help.is_some() {
                        // The native preview is hidden behind a modal. Restore
                        // its HWND and input rectangle on the next UI frame
                        // before asking the backend to take keyboard focus.
                        self.help = None;
                        self.control_activate_after_modal = true;
                    } else {
                        self.actions.push(Action::ActivateControl);
                    }
                }
            });
        }
        secondary(ui, "按 F9 随时释放键鼠。离开投屏窗口后不会自动重新接管。");
        ui.add_space(12.);
        if ui.link("稍后设置，继续投屏").clicked() {
            self.actions.push(Action::ReleaseControl);
            self.control_open = false;
            self.help = None;
        }
    }

    pub(super) fn recovery_rail(&mut self, ui: &mut egui::Ui) {
        ui.heading("连接断开了");
        secondary(ui, "重新连接后，画面会回到这里。");
        ui.add_space(18.);
        ui.colored_label(Color32::from_rgb(228, 176, 87), "键鼠控制已释放");
        ui.add_space(18.);
        if self.source == Source::Usb {
            step(
                ui,
                "1",
                "检查 USB 连接",
                "重新插拔数据线，解锁 iPhone 并确认信任提示。",
            );
            if primary_button(ui, "返回 USB 连接").clicked() {
                self.actions
                    .push(Action::Transition(Transition::Switch(Source::Usb)));
            }
        } else {
            step(
                ui,
                "1",
                "确认仍在同一局域网",
                "检查 iPhone 与电脑的网络连接。",
            );
            step(
                ui,
                "2",
                "重新选择此电脑",
                &format!("在“屏幕镜像”中选择“{}”。", self.settings.receiver_name),
            );
            if primary_button(ui, "查看连接指引").clicked() {
                self.help = Some("连接指引");
            }
        }
        if ui.link("返回连接页").clicked() {
            self.actions
                .push(Action::Transition(Transition::Disconnect));
        }
        ui.add_space(20.);
        ui.separator();
        let guidance = if self.source == Source::Usb {
            [
                (
                    "未检测到 iPhone",
                    "换用支持数据传输的数据线，直接连接电脑 USB 端口；解锁 iPhone 并确认“信任这台电脑”，然后返回 USB 连接页重新检测。",
                ),
                (
                    "已检测到，但无法投屏",
                    "查看设备行中的驱动检查结果。若缺少驱动，请手动打开独立驱动管理器；修复后返回连接页重新检测，勿反复插拔正在安装驱动的设备。",
                ),
                (
                    "连接后没有画面",
                    "保持 iPhone 解锁并唤醒屏幕。返回连接页后重新连接；仍无首帧时，可断开后在高级设置尝试 USB 兼容模式，并导出诊断日志。",
                ),
            ]
        } else {
            [
                (
                    "找不到电脑",
                    "确认接收服务显示已就绪，iPhone 与电脑处于同一局域网。允许 iPhonePlay 通过 Windows 专用网络防火墙，并检查路由器是否启用访客网络或设备隔离。",
                ),
                (
                    "能看到，但连不上",
                    "先在 iPhone 的“屏幕镜像”中停止当前连接，保持手机解锁，再选择此电脑。若仍失败，返回连接页重新准备接收服务，然后重试。",
                ),
                (
                    "连接后没有画面",
                    "唤醒并解锁 iPhone，先切到主屏幕确认是否有画面。受保护的视频内容可能无法镜像；若主屏幕也没有画面，请停止屏幕镜像后重新连接，并导出诊断日志。",
                ),
            ]
        };
        for (title, instruction) in guidance {
            egui::CollapsingHeader::new(title).show(ui, |ui| {
                secondary(ui, instruction);
            });
        }
        ui.add_space(16.);
        egui::CollapsingHeader::new("诊断详情").show(ui, |ui| {
            ui.label(&self.status);
            if ui.link("导出诊断日志").clicked() {
                self.actions.push(Action::ExportLogs);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headless_context() -> egui::Context {
        let context = egui::Context::default();
        theme::configure(&context);
        theme::apply(&context, &iphoneplay::settings::Settings::default());
        context
    }

    #[test]
    fn all_six_toolbar_hit_targets_fit_small_and_wide_panes() {
        // These are effective egui widths after application zoom. 480 points
        // corresponds to a 960-logical-pixel window at 200% application zoom.
        for pane_width in [480., 512., 548., 580., 650., 960., 1280.] {
            let context = headless_context();
            let mut model = UiModel::new(iphoneplay::settings::Settings::default());
            let mut rectangles = [Rect::NOTHING; 6];
            let mut bounds = Rect::NOTHING;
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(pane_width, 160.),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        egui::Frame::new().inner_margin(12).show(ui, |ui| {
                            egui::Frame::new().inner_margin(12).show(ui, |ui| {
                                bounds = ui.available_rect_before_wrap();
                                rectangles = model.toolbar_controls(ui, false, pane_width < 580.);
                            });
                        });
                    },
                )
                .drop_without_applying_deltas();
            for (index, rectangle) in rectangles.iter().enumerate() {
                assert!(
                    rectangle.width() >= if pane_width < 580. { 44. } else { 32. },
                    "button {index} too narrow at {pane_width}: {rectangle:?}"
                );
                assert!(
                    rectangle.min.x >= bounds.min.x - 0.1 && rectangle.max.x <= bounds.max.x + 0.1,
                    "button {index} outside pane at {pane_width}: {rectangle:?}, bounds {bounds:?}"
                );
            }
            for pair in rectangles.windows(2) {
                assert!(
                    pair[0].max.x <= pair[1].min.x,
                    "toolbar hit targets overlap"
                );
            }
        }
    }

    #[test]
    fn compact_control_guide_leaves_a_visible_video_area() {
        for size in [
            [480., 320.],
            [640., 450.],
            [768., 512.],
            [960., 640.],
            [1280., 900.],
        ] {
            let context = headless_context();
            let mut model = UiModel::new(iphoneplay::settings::Settings::default());
            model.control_open = true;
            model.page = Page::Workspace;
            model.capture = CaptureState::Streaming;
            model.source_size = [1170, 2532];
            // Resolve the collapsing panel's first-frame state and any sizing pass.
            for _ in 0..2 {
                context
                    .run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::from(size))),
                            ..Default::default()
                        },
                        |ui| {
                            // Exercise the full root UI (including its real
                            // titlebar), reserving the demo notice's 29 points
                            // without replacing video_rect with a demo image.
                            egui::Panel::top("test_demo_notice")
                                .exact_size(29.)
                                .show(ui, |_| {});
                            model.show(ui);
                        },
                    )
                    .drop_without_applying_deltas();
            }
            let video = model
                .video_rect
                .expect("the compact guide must not hide the video surface");
            assert!(
                video.width() > 30. && video.height() >= 70.,
                "no useful video area at {size:?}: {video:?}"
            );
            assert!(video.min.x >= 0. && video.max.x <= size[0]);
            assert!(video.min.y >= 83. && video.max.y <= size[1]);
        }
    }

    fn render_model(
        context: &egui::Context,
        model: &mut UiModel,
        size: [f32; 2],
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::from(size))),
                events,
                ..Default::default()
            },
            |ui| model.show(ui),
        )
    }

    fn visible_text_rect(output: &egui::FullOutput, text: &str) -> Option<Rect> {
        output.shapes.iter().find_map(|clipped| {
            if let egui::epaint::Shape::Text(shape) = &clipped.shape
                && shape.galley.job.text == text
            {
                let rect = shape.visual_bounding_rect();
                if clipped.clip_rect.contains_rect(rect) {
                    return Some(rect);
                }
            }
            None
        })
    }

    #[test]
    fn long_device_names_do_not_push_disconnect_out_of_view() {
        for size in [[480., 320.], [640., 450.], [1280., 900.]] {
            let context = headless_context();
            let mut model = UiModel::new(iphoneplay::settings::Settings::default());
            model.page = Page::Workspace;
            model.capture = CaptureState::Streaming;
            model.source_size = [1170, 2532];
            model.device_name = "这是工作和家庭共用的超长名称 iPhone 17 Pro Max".repeat(4);
            for _ in 0..2 {
                render_model(&context, &mut model, size, vec![]).drop_without_applying_deltas();
            }
            let output = render_model(&context, &mut model, size, vec![]);
            let disconnect = visible_text_rect(&output, "断开连接");
            output.drop_without_applying_deltas();
            let disconnect = disconnect.expect("disconnect must remain visible with a long name");
            assert!(Rect::from_min_size(Pos2::ZERO, Vec2::from(size)).contains_rect(disconnect));
        }
    }

    #[test]
    fn explicit_control_guide_and_unconnected_help_fit_small_viewports() {
        for size in [[480., 320.], [512., 360.]] {
            for connected in [false, true] {
                let context = headless_context();
                let mut model = UiModel::new(iphoneplay::settings::Settings::default());
                if connected {
                    model.page = Page::Workspace;
                    model.capture = CaptureState::Streaming;
                    model.source_size = [1170, 2532];
                    model.control_open = true;
                    model.control_expand_request = true;
                } else {
                    model.page = Page::Settings;
                    model.settings_tab = 2;
                    model.help = Some("蓝牙反控连接");
                }
                for _ in 0..3 {
                    render_model(&context, &mut model, size, vec![]).drop_without_applying_deltas();
                }
                let dialog = context
                    .memory(|memory| memory.area_rect(egui::Id::unique("connection_help")))
                    .expect("guide must open as a modal");
                assert!(
                    Rect::from_min_size(Pos2::ZERO, Vec2::from(size)).contains_rect(dialog),
                    "guide is clipped at {size:?}: {dialog:?}"
                );
                assert!(
                    model.video_rect.is_none(),
                    "modal must hide the native preview"
                );
                render_model(
                    &context,
                    &mut model,
                    size,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                )
                .drop_without_applying_deltas();
                assert!(model.help.is_none(), "Escape must close the bounded guide");
                render_model(&context, &mut model, size, vec![]).drop_without_applying_deltas();
                assert_eq!(model.video_rect.is_some(), connected);
            }
        }
    }

    #[test]
    fn modal_control_activation_waits_until_the_preview_is_visible() {
        let context = headless_context();
        let mut model = UiModel::new(iphoneplay::settings::Settings::default());
        model.page = Page::Workspace;
        model.capture = CaptureState::Streaming;
        model.source_size = [1170, 2532];
        model.control_open = true;
        model.control_ready = true;
        model.assistive_confirmed = true;
        model.help = Some("连接蓝牙反控");
        let size = [480., 320.];
        for _ in 0..3 {
            render_model(&context, &mut model, size, vec![]).drop_without_applying_deltas();
        }
        let mut button = None;
        for _ in 0..8 {
            let output = render_model(
                &context,
                &mut model,
                size,
                vec![
                    egui::Event::PointerMoved(egui::pos2(240., 220.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., -1000.),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            button = visible_text_rect(&output, "开始控制").or(button);
            output.drop_without_applying_deltas();
        }
        let position = button
            .expect("scrolling must reveal the entire start-control label")
            .center();
        for pressed in [true, false] {
            render_model(
                &context,
                &mut model,
                size,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            )
            .drop_without_applying_deltas();
        }
        assert!(model.help.is_none());
        assert!(
            model.actions.is_empty(),
            "the hidden HWND must not receive activation"
        );
        render_model(&context, &mut model, size, vec![]).drop_without_applying_deltas();
        assert!(model.video_rect.is_some());
        assert!(matches!(
            model.actions.as_slice(),
            [Action::ActivateControl]
        ));
    }
}
