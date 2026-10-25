//! Whose strip and compact layout a speaker shows. By default a model's layout is shared by
//! every speaker of that model; with **own layouts** a speaker keeps its own, which starts as a
//! copy of its model's and goes back to it on a reset.

use std::collections::BTreeMap;

/// The speaker a layout is shown for or changed on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutOwner {
    /// The product name the speaker reports.
    pub product: String,
    /// The speaker's identity in the settings.
    pub member: String,
}

impl LayoutOwner {
    /// What the layout it shows is kept under: its identity with own layouts, else its model.
    /// Strips with the same key show the same layout.
    pub fn key(&self, per_speaker: bool) -> &str {
        if per_speaker {
            &self.member
        } else {
            &self.product
        }
    }
}

/// Where a kind of layout is kept: by product name and by speaker identity.
pub struct LayoutMaps<'a, T> {
    pub by_model: &'a mut BTreeMap<String, T>,
    pub by_speaker: &'a mut BTreeMap<String, T>,
    pub per_speaker: bool,
}

/// The stored layout `owner` shows; `None` for the default one.
pub fn stored<'a, T>(
    by_model: &'a BTreeMap<String, T>,
    by_speaker: &'a BTreeMap<String, T>,
    per_speaker: bool,
    owner: &LayoutOwner,
) -> Option<&'a T> {
    let own = by_speaker.get(&owner.member).filter(|_| per_speaker);
    own.or_else(|| by_model.get(&owner.product))
}

/// Keeps `layout` as what `owner` shows from now on; `None` (a reset) drops the entry.
pub fn store<T>(maps: LayoutMaps<T>, owner: &LayoutOwner, layout: Option<T>) {
    let map = if maps.per_speaker {
        maps.by_speaker
    } else {
        maps.by_model
    };
    let key = owner.key(maps.per_speaker);
    match layout {
        Some(layout) => {
            map.insert(key.to_owned(), layout);
        }
        None => {
            map.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(member: &str) -> LayoutOwner {
        LayoutOwner {
            product: "KH 120 II".to_owned(),
            member: member.to_owned(),
        }
    }

    fn maps() -> (BTreeMap<String, u8>, BTreeMap<String, u8>) {
        let by_model = BTreeMap::from([("KH 120 II".to_owned(), 1)]);
        let by_speaker = BTreeMap::from([("serial:a".to_owned(), 2)]);
        (by_model, by_speaker)
    }

    #[test]
    fn shared_layouts_ignore_a_speakers_own() {
        let (by_model, by_speaker) = maps();
        assert_eq!(
            stored(&by_model, &by_speaker, false, &owner("serial:a")),
            Some(&1)
        );
    }

    #[test]
    fn an_own_layout_starts_as_the_models() {
        let (by_model, by_speaker) = maps();
        assert_eq!(
            stored(&by_model, &by_speaker, true, &owner("serial:a")),
            Some(&2)
        );
        assert_eq!(
            stored(&by_model, &by_speaker, true, &owner("serial:b")),
            Some(&1)
        );
    }

    #[test]
    fn a_change_with_own_layouts_leaves_the_model_alone() {
        let (mut by_model, mut by_speaker) = maps();
        let speaker_maps = LayoutMaps {
            by_model: &mut by_model,
            by_speaker: &mut by_speaker,
            per_speaker: true,
        };
        store(speaker_maps, &owner("serial:b"), Some(3));
        assert_eq!(by_model["KH 120 II"], 1);
        assert_eq!(by_speaker["serial:b"], 3);
    }

    #[test]
    fn a_reset_with_own_layouts_goes_back_to_the_models() {
        let (mut by_model, mut by_speaker) = maps();
        let speaker_maps = LayoutMaps {
            by_model: &mut by_model,
            by_speaker: &mut by_speaker,
            per_speaker: true,
        };
        store(speaker_maps, &owner("serial:a"), None);
        assert_eq!(
            stored(&by_model, &by_speaker, true, &owner("serial:a")),
            Some(&1)
        );
    }

    #[test]
    fn a_shared_change_goes_to_the_model() {
        let (mut by_model, mut by_speaker) = maps();
        let model_maps = LayoutMaps {
            by_model: &mut by_model,
            by_speaker: &mut by_speaker,
            per_speaker: false,
        };
        store(model_maps, &owner("serial:a"), Some(4));
        assert_eq!(by_model["KH 120 II"], 4);
        assert_eq!(by_speaker["serial:a"], 2);
    }
}
