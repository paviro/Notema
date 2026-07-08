pub const FEELINGS: &[&str] = &[
    "calm",
    "content",
    "grateful",
    "hopeful",
    "joyful",
    "excited",
    "energized",
    "focused",
    "proud",
    "relieved",
    "curious",
    "okay",
    "mixed",
    "tired",
    "bored",
    "sad",
    "lonely",
    "anxious",
    "stressed",
    "overwhelmed",
    "frustrated",
    "angry",
    "guilty",
    "numb",
];

/// Emotional valence of a feeling. The [`FEELINGS`] list is ordered
/// positive → neutral → negative, so a value's slot in it is its sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Valence {
    Positive,
    Neutral,
    Negative,
}

/// Classify a feeling by valence, or `None` if it is not a known feeling. The
/// two neutral values (`okay`, `mixed`) split the ordered list; everything
/// before them is positive, everything after is negative.
pub fn feeling_valence(feeling: &str) -> Option<Valence> {
    let feeling = feeling.trim().to_lowercase();
    let index = FEELINGS.iter().position(|f| *f == feeling)?;
    let okay = FEELINGS.iter().position(|f| *f == "okay").unwrap();
    let mixed = FEELINGS.iter().position(|f| *f == "mixed").unwrap();
    Some(if index < okay {
        Valence::Positive
    } else if index <= mixed {
        Valence::Neutral
    } else {
        Valence::Negative
    })
}

pub fn normalize_feeling(feeling: &str) -> Option<String> {
    let feeling = feeling.trim().to_lowercase();
    FEELINGS.contains(&feeling.as_str()).then_some(feeling)
}

pub fn normalize_feelings<'a>(feelings: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut normalized = Vec::new();
    for feeling in feelings {
        let Some(feeling) = normalize_feeling(feeling) else {
            continue;
        };
        if !normalized.contains(&feeling) {
            normalized.push(feeling);
        }
    }
    normalized
}

pub fn validate_feelings<'a>(
    feelings: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for feeling in feelings {
        let trimmed = feeling.trim();
        let Some(feeling) = normalize_feeling(trimmed) else {
            return Err(format!(
                "unknown feeling '{trimmed}'; valid feelings: {}",
                FEELINGS.join(", ")
            ));
        };
        if !normalized.contains(&feeling) {
            normalized.push(feeling);
        }
    }
    Ok(normalized)
}
