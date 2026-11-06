use gpui::{App, Global, Hsla, WindowAppearance, rgb, rgba};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg: Hsla,
    pub surface: Hsla,
    pub raised: Hsla,
    pub folder: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub hover: Hsla,
    pub map: Hsla,
    pub error: Hsla,
    pub danger: Hsla,
    pub danger_text: Hsla,
    pub warning: Hsla,
    pub growth: Hsla,
    pub shrink: Hsla,
    pub selection: Hsla,
    pub backdrop: Hsla,
    pub accent_hover: Hsla,
    pub danger_hover: Hsla,
}
impl Global for Palette {}

pub fn palette(cx: &App) -> Palette {
    *cx.global::<Palette>()
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            bg: rgb(0x18212c).into(),
            surface: rgb(0x202c39).into(),
            raised: rgb(0x293747).into(),
            folder: rgb(0x354557).into(),
            text: rgb(0xeaf0f6).into(),
            muted: rgb(0xa5b4c5).into(),
            line: rgb(0x364454).into(),
            accent: rgb(0x86b9ef).into(),
            accent_text: rgb(0x172a40).into(),
            hover: rgb(0x304156).into(),
            map: rgb(0x131c26).into(),
            error: rgb(0xf4b3b3).into(),
            danger: rgb(0xb64148).into(),
            danger_text: rgb(0xffffff).into(),
            warning: rgb(0xf2c97d).into(),
            growth: rgb(0xe3ba91).into(),
            shrink: rgb(0x8acbb6).into(),
            selection: rgba(0x86b9ef55).into(),
            backdrop: rgba(0x00000077).into(),
            accent_hover: rgb(0x91c8ff).into(),
            danger_hover: rgb(0xc5464e).into(),
        }
    }
    pub fn light() -> Self {
        Self {
            bg: rgb(0xf3f6fa).into(),
            surface: rgb(0xffffff).into(),
            raised: rgb(0xe9eff6).into(),
            folder: rgb(0xd4dfeb).into(),
            text: rgb(0x203348).into(),
            muted: rgb(0x576b80).into(),
            line: rgb(0xced9e5).into(),
            accent: rgb(0x2969ab).into(),
            accent_text: rgb(0xffffff).into(),
            hover: rgb(0xdee9f5).into(),
            map: rgb(0xe2eaf3).into(),
            error: rgb(0xa33038).into(),
            danger: rgb(0xb52f3a).into(),
            danger_text: rgb(0xffffff).into(),
            warning: rgb(0x8a5a00).into(),
            growth: rgb(0x925a26).into(),
            shrink: rgb(0x29765e).into(),
            selection: rgba(0x2969ab55).into(),
            backdrop: rgba(0x00000077).into(),
            accent_hover: rgb(0x2c71b9).into(),
            danger_hover: rgb(0xc3333f).into(),
        }
    }
    pub fn from_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            _ => Self::dark(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Rgba;

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        fn luminance(color: Hsla) -> f32 {
            let color: Rgba = color.into();
            let linear = |v: f32| {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn appearance_selects_css_palette() {
        for appearance in [WindowAppearance::Light, WindowAppearance::VibrantLight] {
            assert_eq!(Palette::from_appearance(appearance), Palette::light());
        }
        for appearance in [WindowAppearance::Dark, WindowAppearance::VibrantDark] {
            assert_eq!(Palette::from_appearance(appearance), Palette::dark());
        }
        assert_ne!(Palette::dark(), Palette::light());
    }

    #[test]
    fn css_values_and_readable_contrast() {
        let dark = Palette::dark();
        let light = Palette::light();
        assert_eq!(dark.bg, Hsla::from(rgb(0x18212c)));
        assert_eq!(dark.folder, Hsla::from(rgb(0x354557)));
        assert_eq!(dark.warning, Hsla::from(rgb(0xf2c97d)));
        assert_eq!(light.bg, Hsla::from(rgb(0xf3f6fa)));
        assert_eq!(light.folder, Hsla::from(rgb(0xd4dfeb)));
        assert_eq!(light.hover, Hsla::from(rgb(0xdee9f5)));
        assert_eq!(light.error, Hsla::from(rgb(0xa33038)));
        for palette in [dark, light] {
            assert!(contrast(palette.text, palette.bg) >= 4.5);
            assert!(contrast(palette.muted, palette.surface) >= 4.5);
        }
    }
}
