//! One group's settings as the groups window and the speaker panel's group tab show them: name,
//! colour, whether it has a strip, what it links and who belongs to it.

use eframe::egui::{Color32, TextEdit, Ui};
use neumann_control_core::settings::{GroupSettings, GroupStrip};

use super::color_choice;
use crate::groups::{linkable_label, toggle_link, LINKABLE};
use crate::texts;

const NAME_ID: &str = "group_name";

/// A speaker the group could have, as the settings list it.
pub struct MemberChoice {
    pub name: String,
    /// The other group it is in; checking it moves it from there.
    pub elsewhere: Option<String>,
    pub checked: bool,
}

/// The group as edited, the palette colour an own one starts from, and every speaker.
pub struct GroupEdit<'a> {
    pub settings: &'a mut GroupSettings,
    pub palette: Color32,
    pub members: &'a mut [MemberChoice],
}

/// True when anything changed; the members' checks are the caller's to apply.
pub fn edit(ui: &mut Ui, edit: GroupEdit) -> bool {
    let GroupEdit {
        settings,
        palette,
        members,
    } = edit;
    let mut changed = name(ui, &mut settings.name);
    changed |= color_choice::edit(
        ui,
        texts::COLOR_DEFAULT.get(),
        &mut settings.color_choice,
        palette,
    );
    changed |= strip_check(ui, &mut settings.strip);
    changed |= link_checks(ui, settings);
    changed |= member_checks(ui, members);
    changed
}

/// Typed into a draft: an emptied field is no name, the group keeps the one it had, and once
/// the field is left it shows that name again.
fn name(ui: &mut Ui, name: &mut String) -> bool {
    let draft_id = ui.scope_id().with(NAME_ID);
    let mut draft = ui
        .data(|data| data.get_temp::<String>(draft_id))
        .unwrap_or_else(|| name.clone());
    let label = ui.label(texts::GROUP_NAME);
    let response = ui
        .add(TextEdit::singleline(&mut draft))
        .labelled_by(label.id);
    let typed = draft.trim().to_owned();
    // Only what was typed: a draft left from another group must not rename this one.
    let changed = response.changed() && !typed.is_empty() && typed != *name;
    if changed {
        *name = typed;
    }
    // Not being typed in, it follows the group (another one chosen, renamed elsewhere).
    if !response.has_focus() {
        draft = name.clone();
    }
    ui.data_mut(|data| data.insert_temp(draft_id, draft));
    changed
}

fn strip_check(ui: &mut Ui, strip: &mut GroupStrip) -> bool {
    let mut shown = *strip == GroupStrip::Shown;
    if !ui.checkbox(&mut shown, texts::SHOW_GROUP_STRIP).changed() {
        return false;
    }
    *strip = if shown {
        GroupStrip::Shown
    } else {
        GroupStrip::Hidden
    };
    true
}

fn link_checks(ui: &mut Ui, settings: &mut GroupSettings) -> bool {
    ui.label(texts::LINKED);
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for linkable in LINKABLE {
            let mut linked = settings.linked.contains(&linkable);
            if ui.checkbox(&mut linked, linkable_label(linkable)).changed() {
                toggle_link(settings, linkable);
                changed = true;
            }
        }
    });
    changed
}

fn member_checks(ui: &mut Ui, members: &mut [MemberChoice]) -> bool {
    if members.is_empty() {
        return false;
    }
    ui.label(texts::GROUP_MEMBERS);
    let mut changed = false;
    for member in members {
        let response = ui.checkbox(&mut member.checked, &member.name);
        changed |= response.changed();
        if let Some(group) = &member.elsewhere {
            response.on_hover_text(texts::in_other_group(group));
        }
    }
    changed
}
