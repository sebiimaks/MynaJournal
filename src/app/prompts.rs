use std::sync::LazyLock;

use backend::{JournalPrompt, Mood};
use rand::seq::IteratorRandom;
use serde::Deserialize;

#[derive(Deserialize)]
struct CatalogPrompt {
    id: String,
    mood: Mood,
    topic: String,
    prompt: String,
}

static CATALOG: LazyLock<Vec<CatalogPrompt>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/mood_journaling_prompts.json"))
        .expect("bundled journal prompts must be valid")
});

pub(super) fn choose(mood: Mood) -> Option<JournalPrompt> {
    CATALOG
        .iter()
        .filter(|prompt| prompt.mood == mood)
        .choose(&mut rand::thread_rng())
        .map(|prompt| JournalPrompt {
            id: prompt.id.clone(),
            mood,
            topic: prompt.topic.clone(),
            text: prompt.prompt.clone(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_unique_ids_and_forty_prompts_for_each_mood() {
        let mut ids = std::collections::HashSet::new();
        for prompt in CATALOG.iter() {
            assert!(ids.insert(&prompt.id));
            assert!(!prompt.prompt.trim().is_empty());
            assert!(!prompt.prompt.chars().any(char::is_control));
        }
        for mood in Mood::ALL {
            assert_eq!(CATALOG.iter().filter(|p| p.mood == mood).count(), 40);
            for _ in 0..100 {
                let chosen = choose(mood).unwrap();
                assert_eq!(chosen.mood, mood);
                assert!(
                    CATALOG
                        .iter()
                        .any(|p| p.id == chosen.id && p.prompt == chosen.text)
                );
            }
        }
    }
}
