pub(super) const THEMES: &[(&str, &str)] = &[
    (
        "dark_abyssal",
        include_str!("../../../../assets/themes/theme_dark_abyssal.toml"),
    ),
    (
        "dark_borealis",
        include_str!("../../../../assets/themes/theme_dark_borealis.toml"),
    ),
    (
        "dark_fernwood",
        include_str!("../../../../assets/themes/theme_dark_fernwood.toml"),
    ),
    (
        "dark_kiln",
        include_str!("../../../../assets/themes/theme_dark_kiln.toml"),
    ),
    (
        "dark_night_market",
        include_str!("../../../../assets/themes/theme_dark_night_market.toml"),
    ),
    (
        "light_cyanotype",
        include_str!("../../../../assets/themes/theme_light_cyanotype.toml"),
    ),
    (
        "light_lido",
        include_str!("../../../../assets/themes/theme_light_lido.toml"),
    ),
    (
        "light_matcha",
        include_str!("../../../../assets/themes/theme_light_matcha.toml"),
    ),
    (
        "light_riso",
        include_str!("../../../../assets/themes/theme_light_riso.toml"),
    ),
    (
        "light_sorbet",
        include_str!("../../../../assets/themes/theme_light_sorbet.toml"),
    ),
];

pub(super) fn source(id: &str) -> Option<&'static str> {
    THEMES
        .iter()
        .find_map(|&(name, source)| (name == id).then_some(source))
}
