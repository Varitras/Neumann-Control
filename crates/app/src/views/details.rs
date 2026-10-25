//! The device panel's settings: the catalog entries for the speaker's model on one tab, by
//! topic; the outputs side by side, one column each. Entries the model may write get a control
//! (numbers also a knob), the rest a greyed value; what the strip or the panel header shows and
//! what the speaker does not have are left out.

use eframe::egui::{self, Button, RichText, Ui};
use neumann_control_core::catalog::{Catalog, Entry, Group, Kind};
use neumann_control_core::settings::StripElement;
use neumann_control_core::speaker::Command;

use super::details_fields::Fields;
use super::knob;
use super::layout_menu;
use crate::display::shown_value;
use crate::panel_tabs::{also_in_overview, tab_of, tabs_for, PanelTab};
use crate::paths;
use crate::state::{FullScanState, SpeakerState, Status};
use crate::strip_layout::{self, LayoutChange, StripLayout};
use crate::texts;

/// The panel's topics in the order they are listed.
const GROUP_ORDER: [Group; 9] = [
    Group::Output,
    Group::Output1,
    Group::Output2,
    Group::Input,
    Group::Standby,
    Group::Device,
    Group::RearPanel,
    Group::Metering,
    Group::Eq,
];
const OUTPUTS: [Group; 3] = [Group::Output, Group::Output1, Group::Output2];
const GROUP_GAP: f32 = 8.0;

/// Drawn by the strip's fixed part (level, meters) or the panel header (model, firmware,
/// serial). Mute and identify stay listed: a layout can take them off the strip.
const SHOWN_ELSEWHERE: &[&str] = &[
    paths::OUTPUT_LEVEL,
    paths::INPUT_METER,
    paths::INPUT_CLIP,
    paths::PRODUCT,
    paths::FIRMWARE_VERSION,
    paths::SERIAL,
];

pub struct DetailsResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    /// A row put on the model's strips or taken off them.
    pub layout: Option<LayoutChange>,
}

/// The tabs the speaker's panel has.
pub fn tabs(speaker: &SpeakerState, catalog: &Catalog) -> Vec<PanelTab> {
    let groups = groups(catalog.entries_for(speaker.product()), speaker);
    tabs_for(groups.into_iter().map(|(group, _)| group))
}

/// Draws the entries of `tab`; `layout` is the model's strip layout once the speaker reported
/// its model.
pub fn show(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    layout: Option<&StripLayout>,
    tab: PanelTab,
) -> DetailsResponse {
    ui.vertical(|ui| rows(ui, speaker, catalog, layout, tab))
        .inner
}

/// The columns of `tab`, each a list of topics: the outputs one each, the overview one without
/// headings, the others split in two.
fn columns_of(groups: Groups<'_>, tab: PanelTab) -> Vec<Groups<'_>> {
    match tab {
        PanelTab::Overview => {
            let entries: Vec<&Entry> = groups
                .into_iter()
                .flat_map(|(_, entries)| entries)
                .filter(|entry| also_in_overview(entry))
                .collect();
            vec![entries
                .into_iter()
                .map(|entry| (entry.group, vec![entry]))
                .collect()]
        }
        PanelTab::Outputs => groups
            .into_iter()
            .filter(|(group, _)| tab_of(*group) == tab)
            .map(|group| vec![group])
            .collect(),
        _ => {
            let on_tab = groups
                .into_iter()
                .filter(|(group, _)| tab_of(*group) == tab)
                .collect();
            let (left, right) = split_in_columns(on_tab);
            vec![left, right]
        }
    }
}

fn rows(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    layout: Option<&StripLayout>,
    tab: PanelTab,
) -> DetailsResponse {
    let product = speaker.product().to_owned();
    let groups = groups(catalog.entries_for(&product), speaker);
    let addable = layout.map(|layout| strip_layout::addable(layout, &product, catalog));
    let mut change = None;
    let mut commands = Vec::new();
    let mut fields = Fields {
        speaker,
        catalog,
        commands: &mut commands,
    };
    let lists = columns_of(groups, tab);
    let headed = tab != PanelTab::Overview;
    ui.columns(lists.len().max(1), |columns| {
        for (column, groups) in columns.iter_mut().zip(lists) {
            for (group, entries) in groups {
                column.push_id(group.title(texts::language()), |ui| {
                    if headed {
                        ui.label(RichText::new(group.title(texts::language())).strong());
                    }
                    for entry in entries {
                        let rect = ui.scope(|ui| row(ui, &mut fields, entry)).response.rect;
                        let (Some(layout), Some(addable)) = (layout, &addable) else {
                            continue;
                        };
                        let element = StripElement::Path(entry.path.clone());
                        let on_strip = strip_layout::on_strip(layout, &element);
                        if !on_strip && !addable.contains(&element) {
                            continue;
                        }
                        if let Some(wish) = layout_menu::row_menu(ui, rect, &element, on_strip) {
                            change = Some(wish);
                        }
                    }
                    ui.add_space(GROUP_GAP);
                });
            }
        }
    });
    if tab == PanelTab::Device {
        ui.separator();
        ui.horizontal(|ui| {
            all_values_button(ui, fields.speaker, fields.commands);
            full_scan_button(ui, fields.speaker, fields.commands);
        });
        full_scan_outcome(ui, fields.speaker);
    }
    DetailsResponse {
        commands,
        layout: change,
    }
}

type Groups<'a> = Vec<(Group, Vec<&'a Entry>)>;

/// The first groups left until about half the rows are placed; the outputs stay together in
/// the column they start in.
fn split_in_columns(groups: Groups<'_>) -> (Groups<'_>, Groups<'_>) {
    let total: usize = groups.iter().map(|(_, entries)| entries.len()).sum();
    let mut placed = 0;
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (group, entries) in groups {
        let continues_outputs = OUTPUTS.contains(&group)
            && left
                .last()
                .is_some_and(|(previous, _): &(Group, _)| OUTPUTS.contains(previous));
        let fits_left = right.is_empty() && (2 * placed < total || continues_outputs);
        placed += entries.len();
        if fits_left {
            left.push((group, entries));
        } else {
            right.push((group, entries));
        }
    }
    (left, right)
}

/// The entries to show, grouped by topic (`arranged`).
fn groups<'a>(entries: Vec<&'a Entry>, speaker: &SpeakerState) -> Groups<'a> {
    let shown = entries.into_iter().filter(|entry| is_shown(entry, speaker));
    arranged(grouped(shown.collect()), speaker.product())
}

/// In topic order, and in each group what can be set before what is only shown; otherwise the
/// catalog's order.
fn arranged<'a>(mut groups: Groups<'a>, product: &str) -> Groups<'a> {
    let place = |group: &Group| GROUP_ORDER.iter().position(|known| known == group);
    groups.sort_by_key(|(group, _)| place(group));
    for (_, entries) in &mut groups {
        entries.sort_by_key(|entry| !entry.is_writable(product));
    }
    groups
}

fn grouped(entries: Vec<&Entry>) -> Groups<'_> {
    let mut groups: Groups = Vec::new();
    for entry in entries {
        match groups.iter_mut().find(|(group, _)| *group == entry.group) {
            Some((_, members)) => members.push(entry),
            None => groups.push((entry.group, vec![entry])),
        }
    }
    groups
}

/// Meters sit on the strip, clip lists light their lamps and EQ bands have their own window,
/// so none of them gets a row here.
fn is_shown(entry: &Entry, speaker: &SpeakerState) -> bool {
    let path = entry.path.to_string();
    let on_strip = SHOWN_ELSEWHERE.contains(&path.as_str());
    let absent = speaker.absent.contains(&entry.path);
    let drawable = !matches!(entry.kind, Kind::Clip | Kind::Meter) && !entry.bands;
    drawable && !on_strip && !absent
}

fn row(ui: &mut Ui, fields: &mut Fields, entry: &Entry) {
    let product = fields.speaker.product();
    if !entry.is_writable(product) {
        display(ui, fields.speaker, entry);
        return;
    }
    let is_list = !entry.element_labels(texts::language()).is_empty();
    match entry.kind {
        Kind::Number if is_list => fields.number_list(ui, entry),
        Kind::Number => {
            ui.horizontal(|ui| {
                let catalog = fields.catalog;
                let label = texts::knob_label(entry.label(texts::language()));
                let speaker = &mut *fields.speaker;
                let sent = knob::for_entry(
                    ui,
                    speaker,
                    catalog,
                    entry,
                    &label,
                    false,
                    knob::KNOB_DIAMETER,
                );
                fields.commands.extend(sent);
                fields.number(ui, entry);
            });
        }
        Kind::Switch => fields.switch(ui, entry),
        Kind::Select => fields.select(ui, entry),
        Kind::Text => fields.text(ui, entry),
        Kind::Display | Kind::Meter | Kind::Clip => display(ui, fields.speaker, entry),
    }
}

fn display(ui: &mut Ui, speaker: &SpeakerState, entry: &Entry) {
    let shown = speaker
        .values
        .get(&entry.path)
        .map(|value| shown_value(entry, value))
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.weak(entry.label(texts::language()));
        ui.weak(shown);
    });
}

fn full_scan_button(ui: &mut Ui, speaker: &mut SpeakerState, commands: &mut Vec<Command>) {
    let running = speaker.full_scan == FullScanState::Running;
    let enabled = speaker.status == Status::Connected && !running;
    if ui
        .add_enabled(enabled, Button::new(texts::FULL_SCAN))
        .clicked()
    {
        commands.push(speaker.start_full_scan());
    }
}

fn full_scan_outcome(ui: &mut Ui, speaker: &SpeakerState) {
    match &speaker.full_scan {
        FullScanState::Idle | FullScanState::Read(_) => {}
        FullScanState::Running => {
            ui.weak(texts::FULL_SCAN_RUNNING);
        }
        FullScanState::Written(path) => {
            ui.add(egui::Label::new(texts::scan_written(path)).wrap());
        }
        FullScanState::Failed(failure) => {
            ui.colored_label(ui.visuals().error_fg_color, texts::scan_failed(*failure));
        }
    }
}

fn all_values_button(ui: &mut Ui, speaker: &mut SpeakerState, commands: &mut Vec<Command>) {
    if ui
        .add_enabled(
            speaker.can_read_all_values(),
            Button::new(texts::ALL_VALUES),
        )
        .clicked()
    {
        commands.push(speaker.read_all_values());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SpeakerKey;
    use neumann_control_core::speaker::Identity;

    const SUBWOOFER: &str = "KH 750";
    const MONITOR: &str = "KH 120 II";

    /// What the panel of a connected speaker of `product` lists, in its order.
    fn arranged_for(product: &str) -> Groups<'static> {
        let address = "[fe80::1%7]:45".parse().unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        speaker.identity = Some(Identity {
            product: product.to_owned(),
            version: "1_0_0".to_owned(),
            serial: "SIM1".to_owned(),
            name: String::new(),
        });
        groups(Catalog::embedded().entries_for(product), &speaker)
    }

    fn titles(groups: &Groups) -> Vec<Group> {
        groups.iter().map(|(group, _)| *group).collect()
    }

    #[test]
    fn groups_follow_the_topics_with_the_outputs_first() {
        assert_eq!(
            titles(&arranged_for(SUBWOOFER))[..7],
            [
                Group::Output,
                Group::Output1,
                Group::Output2,
                Group::Input,
                Group::Standby,
                Group::Device,
                Group::RearPanel,
            ]
        );
    }

    /// Output A and B used to land in different columns.
    #[test]
    fn the_outputs_stay_together_in_one_column() {
        for product in [SUBWOOFER, MONITOR] {
            let (left, right) = split_in_columns(arranged_for(product));
            let outputs = [Group::Output, Group::Output1, Group::Output2];
            assert!(
                titles(&right).iter().all(|group| !outputs.contains(group)),
                "{product}: {:?} | {:?}",
                titles(&left),
                titles(&right)
            );
        }
    }

    #[test]
    fn in_a_group_the_settings_come_before_what_is_only_shown() {
        for (group, entries) in arranged_for(SUBWOOFER) {
            let shown_only: Vec<bool> = entries
                .iter()
                .map(|entry| !entry.is_writable(SUBWOOFER))
                .collect();
            let mut sorted = shown_only.clone();
            sorted.sort();
            assert_eq!(shown_only, sorted, "{group:?}");
        }
    }
}
