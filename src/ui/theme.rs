use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Vec2};
use iphoneplay::settings::{Settings, Theme};

// Microsoft Segoe Fluent Icons, with the Windows 10 MDL2 system font as fallback.
// https://learn.microsoft.com/windows/apps/design/style/segoe-fluent-icons-font
pub const MIRROR: char = '\u{E8B7}';
pub const SETTINGS: char = '\u{E713}';
pub const MINIMIZE: char = '\u{E921}';
pub const MAXIMIZE: char = '\u{E922}';
pub const CLOSE: char = '\u{E8BB}';
pub const SCREEN: char = '\u{E7F4}';
pub const MOUSE: char = '\u{E962}';
pub const USB: char = '\u{E88E}';
pub const LOCK: char = '\u{E72E}';
pub const COPY: char = '\u{E8C8}';
pub const SOUND: char = '\u{E767}';
pub const MUTE: char = '\u{E74F}';
pub const CAMERA: char = '\u{E722}';
pub const RECORD: char = '\u{E7C8}';
pub const FULLSCREEN: char = '\u{E740}';
pub const MORE: char = '\u{E712}';
pub const BACK: char = '\u{E72B}';
pub const SUN: char = '\u{E706}';
pub const MOON: char = '\u{E708}';
pub const SYSTEM_THEME: char = '\u{E793}';
pub const BLUETOOTH: char = '\u{E702}';
pub const FLOATING: char = '\u{E8A7}';
pub const KEYBOARD: char = '\u{E765}';
pub const INFO: char = '\u{E946}';

pub fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let fontdir = std::env::var("WINDIR").unwrap_or_else(|_| "C:/Windows".into());
    for (name, paths, family) in [
        ("latin", vec!["segoeui.ttf"], FontFamily::Proportional),
        (
            "chinese",
            vec!["msyh.ttc", "simhei.ttf"],
            FontFamily::Proportional,
        ),
        (
            "fluent",
            vec!["SegoeIcons.ttf", "segmdl2.ttf"],
            FontFamily::Name("icons".into()),
        ),
        (
            "latin-bold",
            vec!["segoeuib.ttf"],
            FontFamily::Name("heading".into()),
        ),
        (
            "chinese-bold",
            vec!["msyhbd.ttc", "msyh.ttc"],
            FontFamily::Name("heading".into()),
        ),
    ] {
        for path in paths {
            if let Ok(bytes) = std::fs::read(format!("{fontdir}/Fonts/{path}")) {
                fonts
                    .font_data
                    .insert(name.into(), egui::FontData::from_owned(bytes).into());
                let list = fonts.families.entry(family.clone()).or_default();
                if name == "latin" {
                    list.insert(0, name.into());
                } else {
                    list.push(name.into());
                }
                break;
            }
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, settings: &Settings) {
    ctx.set_theme(match settings.theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
    ctx.set_zoom_factor(settings.scale);
    let dark = ctx.theme() == egui::Theme::Dark;
    let bg = if dark {
        Color32::from_rgb(28, 29, 32)
    } else {
        Color32::from_rgb(245, 246, 248)
    };
    let surface = if dark {
        Color32::from_rgb(34, 36, 41)
    } else {
        Color32::WHITE
    };
    let border = if dark {
        Color32::from_rgb(53, 57, 65)
    } else {
        Color32::from_rgb(220, 224, 231)
    };
    let text = if dark {
        Color32::from_rgb(245, 245, 247)
    } else {
        Color32::from_rgb(24, 32, 45)
    };
    ctx.style_mut_of(ctx.theme(), |s| {
        s.text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.));
        s.text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.));
        s.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::new(24., FontFamily::Name("heading".into())),
        );
        s.text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.));
        s.spacing.item_spacing = Vec2::new(8., 6.);
        s.spacing.button_padding = Vec2::new(10., 6.);
        s.spacing.interact_size = Vec2::new(32., 30.);
        s.animation_time = if settings.reduced_motion { 0. } else { 0.12 };
        s.visuals.panel_fill = bg;
        s.visuals.window_fill = surface;
        s.visuals.extreme_bg_color = bg;
        s.visuals.faint_bg_color = surface;
        s.visuals.override_text_color = Some(text);
        s.visuals.weak_text_color = Some(if dark {
            Color32::from_rgb(174, 179, 189)
        } else {
            Color32::from_rgb(98, 108, 122)
        });
        s.visuals.window_stroke = egui::Stroke::new(1., border);
        s.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1., border);
        s.visuals.widgets.inactive.bg_fill = surface;
        s.visuals.widgets.inactive.weak_bg_fill = surface;
        s.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., border);
        s.visuals.widgets.hovered.bg_fill = if dark {
            Color32::from_rgb(47, 53, 63)
        } else {
            Color32::from_rgb(231, 239, 251)
        };
        s.visuals.widgets.hovered.weak_bg_fill = s.visuals.widgets.hovered.bg_fill;
        s.visuals.widgets.active.bg_fill = Color32::from_rgb(28, 128, 233);
        s.visuals.selection.bg_fill = if dark {
            Color32::from_rgb(36, 60, 87)
        } else {
            Color32::from_rgb(219, 234, 252)
        };
        for w in [
            &mut s.visuals.widgets.inactive,
            &mut s.visuals.widgets.hovered,
            &mut s.visuals.widgets.active,
            &mut s.visuals.widgets.noninteractive,
        ] {
            w.corner_radius = egui::CornerRadius::same(8);
        }
    });
}

pub fn icon_text(icon: char, size: f32) -> RichText {
    RichText::new(icon.to_string()).font(FontId::new(size, FontFamily::Name("icons".into())))
}
pub fn theme_icon(theme: Theme) -> char {
    match theme {
        Theme::System => SYSTEM_THEME,
        Theme::Light => SUN,
        Theme::Dark => MOON,
    }
}
pub fn theme_label(theme: Theme, color: Color32) -> egui::text::LayoutJob {
    let mut label = egui::text::LayoutJob::default();
    label.append(
        &format!("{}  ", theme_icon(theme)),
        0.,
        egui::TextFormat {
            font_id: FontId::new(15., FontFamily::Name("icons".into())),
            color,
            ..Default::default()
        },
    );
    label.append(
        theme.label(),
        0.,
        egui::TextFormat {
            font_id: FontId::proportional(14.),
            color,
            ..Default::default()
        },
    );
    label
}
pub fn icon(ui: &mut egui::Ui, value: char, size: f32, color: Color32) {
    ui.label(icon_text(value, size).color(color));
}
pub fn icon_button(ui: &mut egui::Ui, value: char, tip: &str) -> egui::Response {
    ui.add(
        egui::Button::new(icon_text(value, 18.))
            .frame(false)
            .min_size(Vec2::splat(32.)),
    )
    .on_hover_text(tip)
}

pub fn toggle(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(44., 24.), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::accesskit::Role::Switch,
            ui.is_enabled(),
            *value,
            "开关",
        )
    });
    let background = if *value {
        Color32::from_rgb(28, 128, 233)
    } else if ui.visuals().dark_mode {
        Color32::from_rgb(67, 73, 84)
    } else {
        Color32::from_rgb(190, 198, 209)
    };
    ui.painter().rect_filled(rect, 12, background);
    ui.painter().circle_filled(
        egui::pos2(
            if *value {
                rect.right() - 12.
            } else {
                rect.left() + 12.
            },
            rect.center().y,
        ),
        9.,
        Color32::WHITE,
    );
    response
}
pub fn toolbar_button(ui: &mut egui::Ui, value: char, label: &str, accent: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(76., 65.), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 8, ui.visuals().widgets.hovered.bg_fill);
    }
    let color = if accent {
        Color32::from_rgb(77, 163, 250)
    } else {
        ui.visuals().text_color()
    };
    ui.painter().text(
        rect.center_top() + Vec2::new(0., 20.),
        egui::Align2::CENTER_CENTER,
        value,
        FontId::new(24., FontFamily::Name("icons".into())),
        color,
    );
    ui.painter().text(
        rect.center_bottom() - Vec2::new(0., 13.),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.),
        color,
    );
    response.on_hover_text(label)
}
