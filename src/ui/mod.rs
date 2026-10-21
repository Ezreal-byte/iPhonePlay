mod screens;
mod settings_page;
pub mod theme;

use eframe::egui::{self, Color32, RichText, Vec2};
use iphoneplay::{
    settings::Settings,
    state::{CaptureState, Page, RecordingState, Source, Transition},
};
use std::{path::PathBuf, time::Instant};

#[derive(Clone, Debug)]
pub enum Action {
    ShowSettings,
    Back,
    ConnectUsb(usize),
    RefreshDevices,
    DriverManager,
    Transition(Transition),
    StartControl,
    ShowControlGuide,
    OpenBluetoothSettings,
    ToggleFloating,
    ConfirmClient(String),
    ActivateControl,
    ReleaseControl,
    ForgetBinding,
    StartRecording,
    StopRecording,
    Screenshot,
    RestartReceiver,
    ExportLogs,
    OpenPath(PathBuf),
    PickDirectory(bool),
    Minimize,
    Close,
    HideToTray,
    Fullscreen,
    WindowMaximize,
    InspectDependencies,
}

pub struct UiModel {
    pub settings: Settings,
    pub page: Page,
    pub settings_tab: usize,
    pub source: Source,
    pub capture: CaptureState,
    pub recording: RecordingState,
    pub record_started: Option<Instant>,
    pub receiver_ready: bool,
    pub status: String,
    pub device_name: String,
    pub source_size: [u32; 2],
    pub source_fps: f64,
    pub usb_devices: Vec<(String, String, bool)>,
    pub control_open: bool,
    pub control_status: String,
    pub control_active: bool,
    pub control_ready: bool,
    pub control_state: iphoneplay::control::ControlState,
    pub control_expand_request: bool,
    control_activate_after_modal: bool,
    pub floating: bool,
    pub bluetooth_name: String,
    pub clients: Vec<(String, String)>,
    pub bound_client: Option<String>,
    pub assistive_confirmed: bool,
    pub help: Option<&'static str>,
    pub toast: Option<(String, Instant, Option<PathBuf>)>,
    pub pending: Option<Transition>,
    pub actions: Vec<Action>,
    pub video_rect: Option<egui::Rect>,
    pub demo: bool,
    pub demo_texture: Option<egui::TextureHandle>,
    pub welcome_texture: Option<egui::TextureHandle>,
    pub theme_textures: Vec<egui::TextureHandle>,
    pub app_texture: Option<egui::TextureHandle>,
    pub dependency_report: Option<String>,
}

impl UiModel {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            page: Page::Welcome,
            settings_tab: 0,
            source: Source::AirPlay,
            capture: CaptureState::Preparing,
            recording: RecordingState::Idle,
            record_started: None,
            receiver_ready: false,
            status: "正在准备接收服务…".into(),
            device_name: String::new(),
            source_size: [0, 0],
            source_fps: 0.,
            usb_devices: vec![],
            control_open: false,
            control_status: "蓝牙反控未开启 · 可以先只投屏".into(),
            control_active: false,
            control_ready: false,
            control_state: iphoneplay::control::ControlState::Off,
            control_expand_request: false,
            control_activate_after_modal: false,
            floating: false,
            bluetooth_name: std::env::var("COMPUTERNAME").unwrap_or_default(),
            clients: vec![],
            bound_client: None,
            assistive_confirmed: false,
            help: None,
            toast: None,
            pending: None,
            actions: vec![],
            video_rect: None,
            demo: false,
            demo_texture: None,
            welcome_texture: None,
            theme_textures: vec![],
            app_texture: None,
            dependency_report: None,
        }
    }
    pub fn notify(&mut self, message: impl Into<String>, path: Option<PathBuf>) {
        self.toast = Some((message.into(), Instant::now(), path));
    }
    pub fn recording_busy(&self) -> bool {
        matches!(
            self.recording,
            RecordingState::Starting | RecordingState::Recording | RecordingState::Finalizing
        )
    }
    pub fn primary(&self) -> Color32 {
        Color32::from_rgb(48, 145, 244)
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let activate_after_modal = std::mem::take(&mut self.control_activate_after_modal);
        self.video_rect = None;
        let ctx = ui.ctx().clone();
        theme::apply(&ctx, &self.settings);
        ui.painter().rect_filled(
            ui.max_rect(),
            0,
            if ctx.theme() == egui::Theme::Dark {
                Color32::from_rgb(28, 29, 32)
            } else {
                Color32::from_rgb(245, 246, 248)
            },
        );
        egui::Panel::top("titlebar")
            .exact_size(if ui.available_width() < 620. {
                46.
            } else {
                54.
            })
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(24, 7)),
            )
            .show(ui, |ui| self.titlebar(ui));
        if self.demo {
            egui::Panel::top("demo_notice")
                .exact_size(29.)
                .frame(egui::Frame::new().fill(Color32::from_rgb(98, 73, 25)))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.label(
                            RichText::new("界面演示 · 当前没有真实手机连接，录制与反控不可用")
                                .size(13.)
                                .color(Color32::WHITE),
                        );
                    });
                });
        }
        if let Some((message, time, path)) = &self.toast.clone()
            && time.elapsed().as_secs() < 10
        {
            egui::Panel::bottom("toast")
                .frame(
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .inner_margin(12),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(message);
                        if let Some(path) = path
                            && ui.link("打开所在目录").clicked()
                        {
                            self.actions.push(Action::OpenPath(path.clone()));
                        }
                        if ui.small_button("关闭").clicked() {
                            self.toast = None;
                        }
                    });
                });
        }
        match self.page {
            Page::Welcome => self.welcome(ui),
            Page::Usb => self.usb(ui),
            Page::Workspace => self.workspace(ui, false),
            Page::Recovery => self.workspace(ui, true),
            Page::Settings => self.settings_page(ui),
        }
        // A native child HWND paints above egui, including popups opening
        // upward from the toolbar. Hide it for the entire popup lifetime.
        if egui::Popup::is_any_open(&ctx) {
            self.video_rect = None;
        }
        if let Some(help) = self.help {
            self.video_rect = None;
            self.help_window(&ctx, help);
        }
        if self.pending.is_some() {
            self.video_rect = None;
            egui::Modal::new(egui::Id::unique("finishing")).show(&ctx, |ui| {
                ui.set_width(380.);
                ui.heading("正在安全结束当前会话");
                ui.add_space(12.);
                ui.label("键鼠已释放。正在结束当前会话；如有录像，先完成封装。完成后会自动继续。");
                ui.add_space(12.);
                ui.spinner();
            });
        }
        if activate_after_modal
            && self.page == Page::Workspace
            && self.help.is_none()
            && self.pending.is_none()
            && self.actions.is_empty()
        {
            self.actions.push(Action::ActivateControl);
        }
    }

    fn titlebar(&mut self, ui: &mut egui::Ui) {
        let compact = ui.available_width() < 620.;
        ui.horizontal_centered(|ui| {
            if compact {
                ui.spacing_mut().item_spacing.x = 8.;
            }
            if let Some(texture) = &self.app_texture {
                ui.add(egui::Image::new(texture).fit_to_exact_size(Vec2::splat(30.)));
            }
            ui.label(
                RichText::new("iPhonePlay")
                    .size(19.)
                    .family(egui::FontFamily::Name("heading".into())),
            );
            let remaining = (ui.available_width() - if compact { 228. } else { 330. }).max(0.);
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(remaining, 34.), egui::Sense::drag());
            let _ = rect;
            if response.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if response.double_clicked() {
                self.actions.push(Action::WindowMaximize);
            }
            if !compact {
                egui::ComboBox::from_id_salt("header_theme")
                    .selected_text(theme::theme_label(
                        self.settings.theme,
                        ui.visuals().text_color(),
                    ))
                    .width(116.)
                    .show_ui(ui, |ui| {
                        for theme in [
                            iphoneplay::settings::Theme::System,
                            iphoneplay::settings::Theme::Light,
                            iphoneplay::settings::Theme::Dark,
                        ] {
                            ui.selectable_value(
                                &mut self.settings.theme,
                                theme,
                                theme::theme_label(theme, ui.visuals().text_color()),
                            );
                        }
                    });
            } else {
                ui.menu_button(
                    theme::icon_text(theme::theme_icon(self.settings.theme), 18.),
                    |ui| {
                        for choice in [
                            iphoneplay::settings::Theme::System,
                            iphoneplay::settings::Theme::Light,
                            iphoneplay::settings::Theme::Dark,
                        ] {
                            if ui
                                .selectable_value(
                                    &mut self.settings.theme,
                                    choice,
                                    theme::theme_label(choice, ui.visuals().text_color()),
                                )
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                    },
                );
            }
            if theme::icon_button(ui, theme::SETTINGS, "设置").clicked() {
                self.actions.push(Action::ShowSettings);
            }
            ui.separator();
            if theme::icon_button(ui, theme::MINIMIZE, "最小化").clicked() {
                self.actions.push(Action::Minimize);
            }
            if theme::icon_button(ui, theme::MAXIMIZE, "最大化 / 还原").clicked() {
                self.actions.push(Action::WindowMaximize);
            }
            if theme::icon_button(ui, theme::CLOSE, "关闭").clicked() {
                self.actions.push(Action::Close);
            }
        });
    }

    fn help_window(&mut self, ctx: &egui::Context, kind: &'static str) {
        let viewport = ctx.content_rect();
        let dialog_width = (viewport.width() - 56.).clamp(180., 490.);
        let body_height = (viewport.height() - 112.).max(80.);
        let response = egui::Modal::new(egui::Id::unique("connection_help")).show(ctx,|ui| {
            ui.set_width(dialog_width);
            ui.horizontal(|ui|{ui.heading(kind);ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if theme::icon_button(ui,theme::CLOSE,"关闭指引").clicked(){self.help=None;}});});
            ui.add_space(10.);
            egui::ScrollArea::vertical().id_salt(("help_body", kind)).max_height(body_height).show(ui, |ui| {
            match kind {
                "连接蓝牙反控"=>{self.control_guide_content(ui);},
                "连接恢复指引"=>{self.recovery_rail(ui);},
                "后台继续运行"=>{ui.label("关闭窗口后，投屏和录制仍在后台继续。键鼠控制已释放。");ui.add_space(12.);ui.label("点击任务栏通知区域中的 iPhonePlay 图标可重新打开。托盘菜单可以退出软件。");if primary_button(ui,"知道了，收起到托盘").clicked(){self.settings.tray_notice_seen=true;self.help=None;self.actions.push(Action::HideToTray);}},
                "依赖检查"=>{ui.label(self.dependency_report.as_deref().unwrap_or("正在检查…"));},
                "蓝牙反控连接"=>{
                    ui.label("先连接 AirPlay 或 USB 投屏，再打开反控连接引导。投屏后会准备蓝牙键鼠服务。");
                    step(ui,"1","开启辅助触控","在 iPhone 设置 → 辅助功能 → 触控中开启辅助触控。");
                    step(ui,"2","从手机连接电脑",&format!("在辅助触控 → 设备 → 蓝牙设备中选择“{}”。",self.bluetooth_name));
                    step(ui,"3","在应用确认手机","手机接入键鼠服务后，应用会显示待确认设备。确认后点击“开始控制”，F9 可随时释放。");
                    if ui.button("打开 Windows 蓝牙设置").clicked(){ self.actions.push(Action::OpenBluetoothSettings); }
                    ui.hyperlink_to("Apple 手机端连接说明", "https://support.apple.com/zh-cn/111775");
                },
                _=> {
                    step(ui,"1","连接同一局域网","iPhone 使用 Wi-Fi；电脑可以使用 Wi-Fi 或网线。请避免访客网络的设备隔离。");
                    step(ui,"2","在 iPhone 打开屏幕镜像","从屏幕右上角向下轻扫打开控制中心，轻点“屏幕镜像”。");
                    step(ui,"3","选择这台电脑",&format!("在设备列表中选择“{}”。收到画面后会自动进入投屏。",self.settings.receiver_name));
                    ui.separator();
                    egui::CollapsingHeader::new("找不到电脑").default_open(kind=="找不到电脑").show(ui,|ui|{ui.label("确认接收服务已就绪；Windows 网络设为专用网络，并允许 iPhonePlay 通过防火墙。关闭设备间隔离后重新打开屏幕镜像列表。");if ui.button("重新准备接收服务").clicked(){self.actions.push(Action::RestartReceiver);}});
                    egui::CollapsingHeader::new("能看到电脑，但连不上 / 没有画面").show(ui,|ui|{ui.label("在 iPhone 上停止屏幕镜像，解锁手机后重新选择此电脑。仍有问题时，可切换 USB 连接或导出日志。");if ui.link("导出诊断日志").clicked(){self.actions.push(Action::ExportLogs);}});
                }
            }
            });
        });
        if response.should_close() {
            self.help = None;
        }
    }
}

pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(Color32::WHITE).size(15.))
            .fill(Color32::from_rgb(28, 128, 233))
            .min_size(Vec2::new(184., 38.))
            .corner_radius(9),
    )
}
pub fn secondary(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(
        RichText::new(text)
            .size(13.)
            .color(ui.visuals().weak_text_color()),
    );
}
pub fn step(ui: &mut egui::Ui, n: &str, title: &str, body: &str) {
    ui.horizontal_top(|ui| {
        ui.label(
            RichText::new(n)
                .size(19.)
                .color(Color32::from_rgb(78, 159, 245)),
        );
        ui.add_space(6.);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(15.).strong());
            secondary(ui, body);
        });
    });
    ui.add_space(10.);
}
