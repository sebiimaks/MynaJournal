use std::collections::HashMap;

use super::ui::themes::TagColors;

#[derive(Debug, Clone)]
/// Manages assigning colors to the tags, keeping track on the assigned colors and providing
/// functions to updating them.
pub struct ColoredTagsManager {
    tag_colors_map: HashMap<String, TagColors>,
    available_colors: Vec<TagColors>,
}

impl ColoredTagsManager {
    /// Creates a manager assigning the colors of the given palette to new tags in order.
    pub fn new(palette: &[TagColors]) -> Self {
        // Colors are popped from the end of the stack, so the palette is reversed to keep its order.
        let available_colors = palette.iter().rev().copied().collect();

        Self {
            tag_colors_map: HashMap::new(),
            available_colors,
        }
    }

    /// Updates the tag_color map with the provided tags, removing the not existing tags and
    /// assigning colors to the newly added ones.
    pub fn update_tags(&mut self, current_tags: Vec<String>) {
        // First: Clear the non-existing anymore tags.
        let tags_to_remove: Vec<_> = self
            .tag_colors_map
            .keys()
            .filter(|t| !current_tags.contains(t))
            .cloned()
            .collect();

        for tag in tags_to_remove {
            let color = self.tag_colors_map.remove(&tag).unwrap();
            self.available_colors.push(color)
        }

        // Second: Add the new tags to the map
        for tag in current_tags {
            match self.tag_colors_map.entry(tag) {
                std::collections::hash_map::Entry::Occupied(_) => {}
                std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                    let color = self.available_colors.pop().unwrap_or_default();
                    vacant_entry.insert(color);
                }
            }
        }
    }

    /// Gets the matching color for the giving tag if tag exists.
    pub fn get_tag_color(&self, tag: &str) -> Option<TagColors> {
        self.tag_colors_map.get(tag).copied()
    }
}

#[cfg(test)]
mod tests {

    use ratatui::style::Color;

    use super::*;
    use crate::app::ui::themes::TagsColors;

    #[test]
    fn test_colored_tags() {
        const TAG_ONE: &str = "Tag 1";
        const TAG_TWO: &str = "Tag 2";
        const ADDED_TAG: &str = "Added Tag";

        let mut tags = vec![
            String::from(TAG_ONE),
            String::from(TAG_TWO),
            String::from("Tag 3"),
            String::from("Tag 4"),
        ];

        let mut manager = ColoredTagsManager::new(&TagsColors::default().palette);
        manager.update_tags(tags.clone());

        // Ensure all tags have colors.
        for tag in tags.iter() {
            assert!(manager.get_tag_color(tag).is_some());
        }

        // Ensure non existing tags are none
        assert!(manager.get_tag_color("Non Existing Tag").is_none());

        // Keep track on colors before updating.
        let tag_one_color = manager.get_tag_color(TAG_ONE).unwrap();
        let tag_two_color = manager.get_tag_color(TAG_TWO).unwrap();

        // Remove Tag one with changing the order of the tags.
        assert_eq!(tags.swap_remove(0), TAG_ONE);

        tags.push(ADDED_TAG.into());

        manager.update_tags(tags.clone());

        // Ensure all current tags have colors.
        for tag in tags.iter() {
            assert!(manager.get_tag_color(tag).is_some());
        }

        // Tag one should have no color after remove.
        assert!(manager.get_tag_color(TAG_ONE).is_none());

        // Tag two color must remain the same after update.
        assert_eq!(manager.get_tag_color(TAG_TWO).unwrap(), tag_two_color);

        // Added tag should take the color of tag one because we removed it then added the new tag.
        assert_eq!(manager.get_tag_color(ADDED_TAG).unwrap(), tag_one_color);
    }

    #[test]
    fn custom_palette_is_assigned_to_tags_in_order() {
        let palette = [
            TagColors::new(Color::White, Color::Blue),
            TagColors::new(Color::Black, Color::Rgb(250, 240, 200)),
            TagColors::new(Color::Indexed(236), Color::LightYellow),
        ];
        let tags: Vec<String> = ["alpha", "beta", "gamma", "delta"].map(String::from).into();

        let mut manager = ColoredTagsManager::new(&palette);
        manager.update_tags(tags.clone());

        for (tag, colors) in tags.iter().zip(palette) {
            assert_eq!(manager.get_tag_color(tag), Some(colors));
        }
        assert_eq!(manager.get_tag_color("delta"), Some(TagColors::default()));
    }

    #[test]
    fn default_palette_assigns_the_previous_builtin_colors_in_order() {
        let tags: Vec<String> = (1..=10).map(|number| format!("Tag {number:02}")).collect();
        let expected = [
            (Color::Black, Color::LightCyan),
            (Color::Black, Color::LightRed),
            (Color::Black, Color::LightGreen),
            (Color::Reset, Color::DarkGray),
            (Color::Black, Color::LightYellow),
            (Color::Reset, Color::Red),
            (Color::Yellow, Color::Blue),
            (Color::Red, Color::Cyan),
            (Color::Black, Color::LightMagenta),
            (Color::Reset, Color::Reset),
        ];

        let mut manager = ColoredTagsManager::new(&TagsColors::default().palette);
        manager.update_tags(tags.clone());

        for (tag, (foreground, background)) in tags.iter().zip(expected) {
            assert_eq!(
                manager.get_tag_color(tag),
                Some(TagColors::new(foreground, background))
            );
        }
    }
}
