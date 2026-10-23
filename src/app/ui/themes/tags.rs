use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TagsColors {
    /// Colors assigned to the tags in alphabetical order, one pair per tag.
    #[serde(default = "palette")]
    pub palette: Vec<TagColors>,
}

impl Default for TagsColors {
    fn default() -> Self {
        Self { palette: palette() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
/// Represents the needed colors for a colored tag
pub struct TagColors {
    #[serde(rename = "fg")]
    pub foreground: Color,
    #[serde(rename = "bg")]
    pub background: Color,
}

impl TagColors {
    pub const fn new(foreground: Color, background: Color) -> Self {
        Self {
            foreground,
            background,
        }
    }
}

#[inline]
fn palette() -> Vec<TagColors> {
    vec![
        TagColors::new(Color::Black, Color::LightCyan),
        TagColors::new(Color::Black, Color::LightRed),
        TagColors::new(Color::Black, Color::LightGreen),
        TagColors::new(Color::Reset, Color::DarkGray),
        TagColors::new(Color::Black, Color::LightYellow),
        TagColors::new(Color::Reset, Color::Red),
        TagColors::new(Color::Yellow, Color::Blue),
        TagColors::new(Color::Red, Color::Cyan),
        TagColors::new(Color::Black, Color::LightMagenta),
    ]
}
