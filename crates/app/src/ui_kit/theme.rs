//! The look of the app in Windows' light and dark mode. egui follows the system theme on its
//! own, so each of its two themes gets its own style here.

use std::sync::Arc;

use eframe::egui::style::{Selection, WidgetVisuals};
use eframe::egui::{
    vec2, Color32, Context, CornerRadius, FontId, Stroke, Style, TextStyle, Theme, ThemePreference,
    Visuals,
};

use neumann_control_core::settings::ThemeChoice;

use super::fonts::semibold;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub chrome: Color32,
    pub card: Color32,
    pub border: Color32,
    pub field: Color32,
    pub hover: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub danger: Color32,
    pub connected: Color32,
    pub connecting: Color32,
    pub unreachable: Color32,
    pub meter_track: Color32,
    pub meter_fill: Color32,
    pub clip_off: Color32,
}

const RADIUS_SMALL: u8 = 3;
const RADIUS_LARGE: u8 = 6;
const SIZE_SMALL: f32 = 10.5;
const SIZE_BODY: f32 = 12.0;
const SIZE_HEADING: f32 = 13.5;

impl Tokens {
    pub fn dark() -> Tokens {
        Tokens {
            chrome: Color32::from_gray(32),
            card: Color32::from_gray(44),
            border: Color32::from_gray(64),
            field: Color32::from_gray(56),
            hover: Color32::from_gray(72),
            text: Color32::from_gray(222),
            text_dim: Color32::from_gray(160),
            accent: Color32::from_rgb(34, 110, 220),
            accent_text: Color32::WHITE,
            danger: Color32::from_rgb(248, 113, 120),
            connected: Color32::from_rgb(74, 200, 110),
            connecting: Color32::from_rgb(230, 180, 60),
            unreachable: Color32::from_gray(110),
            meter_track: Color32::from_gray(24),
            meter_fill: Color32::from_rgb(74, 200, 110),
            clip_off: Color32::from_gray(70),
        }
    }

    pub fn light() -> Tokens {
        Tokens {
            chrome: Color32::from_rgb(243, 243, 246),
            card: Color32::WHITE,
            border: Color32::from_rgb(210, 210, 218),
            field: Color32::from_rgb(236, 236, 240),
            hover: Color32::from_rgb(224, 224, 230),
            text: Color32::from_rgb(24, 24, 28),
            text_dim: Color32::from_rgb(92, 92, 102),
            accent: Color32::from_rgb(29, 98, 216),
            accent_text: Color32::WHITE,
            danger: Color32::from_rgb(200, 40, 50),
            connected: Color32::from_rgb(30, 150, 70),
            connecting: Color32::from_rgb(200, 140, 20),
            unreachable: Color32::from_gray(150),
            meter_track: Color32::from_rgb(222, 222, 228),
            meter_fill: Color32::from_rgb(30, 160, 80),
            clip_off: Color32::from_rgb(200, 200, 206),
        }
    }

    /// The tokens of the theme egui is showing right now.
    pub fn current(context: &Context) -> Tokens {
        match context.theme() {
            Theme::Dark => Tokens::dark(),
            Theme::Light => Tokens::light(),
        }
    }
}

fn widget_visuals(fill: Color32, border: Color32, foreground: Color32) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, border),
        corner_radius: CornerRadius::same(RADIUS_SMALL),
        fg_stroke: Stroke::new(1.0, foreground),
        expansion: 0.0,
    }
}

fn visuals(tokens: &Tokens, theme: Theme) -> Visuals {
    let mut visuals = theme.default_visuals();
    visuals.panel_fill = tokens.chrome;
    visuals.window_fill = tokens.card;
    visuals.window_stroke = Stroke::new(1.0, tokens.border);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_LARGE);
    visuals.extreme_bg_color = tokens.field;
    visuals.override_text_color = Some(tokens.text);
    visuals.weak_text_color = Some(tokens.text_dim);
    visuals.error_fg_color = tokens.danger;
    visuals.selection = Selection {
        bg_fill: tokens.accent,
        stroke: Stroke::new(1.0, tokens.accent_text),
    };
    visuals.widgets.noninteractive = widget_visuals(tokens.card, tokens.border, tokens.text_dim);
    visuals.widgets.inactive = widget_visuals(tokens.field, tokens.border, tokens.text);
    visuals.widgets.hovered = widget_visuals(tokens.hover, tokens.border, tokens.text);
    visuals.widgets.active = widget_visuals(tokens.hover, tokens.accent, tokens.text);
    visuals.widgets.open = widget_visuals(tokens.field, tokens.border, tokens.text);
    visuals.slider_trailing_fill = true;
    visuals
}

fn style(theme: Theme) -> Style {
    let tokens = match theme {
        Theme::Dark => Tokens::dark(),
        Theme::Light => Tokens::light(),
    };
    let mut style = Style {
        visuals: visuals(&tokens, theme),
        ..Style::default()
    };
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(SIZE_SMALL)),
        (TextStyle::Body, FontId::proportional(SIZE_BODY)),
        (TextStyle::Button, FontId::proportional(SIZE_BODY)),
        (TextStyle::Heading, semibold(SIZE_HEADING)),
        (TextStyle::Monospace, FontId::monospace(SIZE_BODY)),
    ]
    .into();
    style.spacing.item_spacing = vec2(8.0, 6.0);
    style.spacing.button_padding = vec2(10.0, 4.0);
    style.spacing.interact_size = vec2(24.0, 24.0);
    style
}

pub fn apply(context: &Context) {
    for theme in [Theme::Light, Theme::Dark] {
        context.set_style_of(theme, Arc::new(style(theme)));
    }
}

/// What the settings dialog's theme choice means for egui.
pub fn preference(choice: ThemeChoice) -> ThemePreference {
    match choice {
        ThemeChoice::System => ThemePreference::System,
        ThemeChoice::Light => ThemePreference::Light,
        ThemeChoice::Dark => ThemePreference::Dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_egui_theme_gets_its_own_tokens() {
        let context = Context::default();
        apply(&context);
        assert_eq!(
            context.style_of(Theme::Light).visuals.panel_fill,
            Tokens::light().chrome
        );
        assert_eq!(
            context.style_of(Theme::Dark).visuals.panel_fill,
            Tokens::dark().chrome
        );
        assert!(!context.style_of(Theme::Light).visuals.dark_mode);
    }
}
