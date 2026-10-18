//! What the "Gefundene Lautsprecher" dialog offers after "Suchen": the speakers a scan found,
//! which of them are already shown, and which the user checked.

use std::net::SocketAddrV6;

use neumann_control_core::discovery::Found;
use neumann_control_core::mask::mask_host;
use neumann_control_core::settings::ManualSpeaker;

use crate::state::SpeakerKey;
use crate::targets::{entry_for, is_shown};

#[derive(Clone, Debug, PartialEq)]
pub enum ScanChoice {
    Scanning,
    Failed,
    Offered(Vec<Offer>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub found: Found,
    /// Already shown, so it cannot be chosen again.
    pub known: bool,
    pub checked: bool,
}

impl ScanChoice {
    /// `shown` are the key and address of every speaker in the window.
    pub fn offer(found: &[Found], shown: &[(SpeakerKey, SocketAddrV6)]) -> ScanChoice {
        let offers = found
            .iter()
            .map(|found| Offer {
                found: found.clone(),
                known: is_shown(found, shown),
                checked: false,
            })
            .collect();
        ScanChoice::Offered(offers)
    }

    /// Flips the check of one row; rows of speakers already shown stay unchecked.
    pub fn toggle(&mut self, index: usize) {
        let ScanChoice::Offered(offers) = self else {
            return;
        };
        let Some(offer) = offers.get_mut(index) else {
            return;
        };
        if !offer.known {
            offer.checked = !offer.checked;
        }
    }

    pub fn can_add(&self) -> bool {
        self.offers().iter().any(|offer| offer.checked)
    }

    /// The settings entries for the checked speakers.
    pub fn chosen(&self) -> Vec<ManualSpeaker> {
        self.offers()
            .iter()
            .filter(|offer| offer.checked)
            .map(|offer| entry_for(&offer.found))
            .collect()
    }

    fn offers(&self) -> &[Offer] {
        match self {
            ScanChoice::Offered(offers) => offers,
            ScanChoice::Scanning | ScanChoice::Failed => &[],
        }
    }
}

impl Offer {
    /// The announced name, else the model, else the masked address; announced names can be
    /// empty.
    pub fn name(&self) -> String {
        [
            Some(self.found.name.as_str()),
            self.found.product.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| self.masked_address())
    }

    /// The announced model, unless the name already is the model.
    pub fn model(&self) -> Option<&str> {
        let name = self.name();
        self.found.product.as_deref().filter(|model| *model != name)
    }

    pub fn masked_address(&self) -> String {
        mask_host(&self.found.address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::address::parse_address;

    const KNOWN_SERIAL: &str = "SIM0001234";
    const NEW_SERIAL: &str = "SIM0007500";
    const KNOWN_ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3456%7";
    const NEW_ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3457%7";
    const PORT: u16 = 45;

    fn address(text: &str) -> SocketAddrV6 {
        parse_address(text, PORT).unwrap()
    }

    fn found(name: &str, serial: &str, text: &str) -> Found {
        Found {
            name: name.to_owned(),
            product: Some("KH 750".to_owned()),
            serial: Some(serial.to_owned()),
            address: address(text),
        }
    }

    fn choice() -> ScanChoice {
        let shown = [(
            SpeakerKey::Serial(KNOWN_SERIAL.to_owned()),
            address(KNOWN_ADDRESS_TEXT),
        )];
        ScanChoice::offer(
            &[
                found("Desk", KNOWN_SERIAL, KNOWN_ADDRESS_TEXT),
                found("Sub", NEW_SERIAL, NEW_ADDRESS_TEXT),
            ],
            &shown,
        )
    }

    #[test]
    fn speakers_already_shown_are_offered_as_known_and_nothing_is_checked() {
        let ScanChoice::Offered(offers) = choice() else {
            panic!("a finished scan offers its speakers");
        };
        let known: Vec<bool> = offers.iter().map(|offer| offer.known).collect();
        assert_eq!(known, vec![true, false]);
        assert!(offers.iter().all(|offer| !offer.checked));
        assert!(!choice().can_add());
    }

    #[test]
    fn only_new_speakers_can_be_checked() {
        let mut choice = choice();
        choice.toggle(0);
        assert!(!choice.can_add());
        choice.toggle(1);
        assert!(choice.can_add());
        choice.toggle(1);
        assert!(!choice.can_add());
        choice.toggle(2);
        assert!(!choice.can_add());
    }

    #[test]
    fn the_checked_speakers_become_settings_entries() {
        let mut choice = choice();
        choice.toggle(1);
        assert_eq!(
            choice.chosen(),
            vec![ManualSpeaker {
                address: "fe80::2a36:38ff:fe12:3457%7".to_owned(),
                port: PORT,
                serial: Some(NEW_SERIAL.to_owned()),
            }]
        );
    }

    #[test]
    fn a_running_or_failed_scan_offers_nothing() {
        for mut choice in [ScanChoice::Scanning, ScanChoice::Failed] {
            choice.toggle(0);
            assert!(!choice.can_add());
            assert!(choice.chosen().is_empty());
        }
    }

    #[test]
    fn an_unnamed_speaker_is_named_by_model_then_masked_address() {
        let mut offer = Offer {
            found: found("", NEW_SERIAL, NEW_ADDRESS_TEXT),
            known: false,
            checked: false,
        };
        assert_eq!(offer.name(), "KH 750");
        assert_eq!(offer.model(), None);
        offer.found.product = None;
        assert_eq!(offer.name(), mask_host(&address(NEW_ADDRESS_TEXT)));
        offer.found.name = "Sub".to_owned();
        offer.found.product = Some("KH 750".to_owned());
        assert_eq!(offer.model(), Some("KH 750"));
    }
}
