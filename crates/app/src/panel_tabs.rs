//! The device panel's tabs: which catalog topic goes on which tab. The overview holds the
//! panel's own controls (group, colour, presets, trend) and, again, the device name.

use neumann_control_core::catalog::{Entry, Group};

use crate::paths;
use crate::texts::{self, Text};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PanelTab {
    #[default]
    Overview,
    Outputs,
    Inputs,
    Standby,
    Device,
}

/// In the order the tab bar shows them.
pub const PANEL_TABS: [PanelTab; 5] = [
    PanelTab::Overview,
    PanelTab::Outputs,
    PanelTab::Inputs,
    PanelTab::Standby,
    PanelTab::Device,
];

impl PanelTab {
    pub fn title(self) -> Text {
        match self {
            PanelTab::Overview => texts::TAB_OVERVIEW,
            PanelTab::Outputs => texts::TAB_OUTPUTS,
            PanelTab::Inputs => texts::TAB_INPUTS,
            PanelTab::Standby => texts::TAB_STANDBY,
            PanelTab::Device => texts::TAB_DEVICE,
        }
    }
}

/// The tab a catalog topic is listed on; metering and EQ leftovers go with the device.
pub fn tab_of(group: Group) -> PanelTab {
    match group {
        Group::Output | Group::Output1 | Group::Output2 => PanelTab::Outputs,
        Group::Input | Group::RearPanel => PanelTab::Inputs,
        Group::Standby => PanelTab::Standby,
        Group::Device | Group::Metering | Group::Eq => PanelTab::Device,
    }
}

/// Entries the overview shows as well as their own tab: the name, the first thing one sets.
pub fn also_in_overview(entry: &Entry) -> bool {
    entry.path.to_string() == paths::DEVICE_NAME
}

/// The tabs a speaker with entries of `groups` gets: the overview always, the others only
/// with something on them.
pub fn tabs_for(groups: impl IntoIterator<Item = Group>) -> Vec<PanelTab> {
    let used: Vec<PanelTab> = groups.into_iter().map(tab_of).collect();
    PANEL_TABS
        .into_iter()
        .filter(|tab| *tab == PanelTab::Overview || used.contains(tab))
        .collect()
}

/// The tab to show: the one last chosen in this session, where the speaker has it.
pub fn shown_tab(chosen: PanelTab, available: &[PanelTab]) -> PanelTab {
    if available.contains(&chosen) {
        chosen
    } else {
        PanelTab::Overview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_output_lands_on_the_outputs_tab() {
        for group in [Group::Output, Group::Output1, Group::Output2] {
            assert_eq!(tab_of(group), PanelTab::Outputs, "{group:?}");
        }
        assert_eq!(tab_of(Group::RearPanel), PanelTab::Inputs);
    }

    /// A speaker that has not told its model yet has nothing but the overview.
    #[test]
    fn a_tab_without_entries_is_left_out() {
        assert_eq!(tabs_for([]), vec![PanelTab::Overview]);
        assert_eq!(
            tabs_for([Group::Device, Group::Output]),
            vec![PanelTab::Overview, PanelTab::Outputs, PanelTab::Device]
        );
    }

    #[test]
    fn the_last_tab_stays_unless_the_speaker_lacks_it() {
        let tabs = [PanelTab::Overview, PanelTab::Outputs];
        assert_eq!(shown_tab(PanelTab::Outputs, &tabs), PanelTab::Outputs);
        assert_eq!(shown_tab(PanelTab::Standby, &tabs), PanelTab::Overview);
    }
}
