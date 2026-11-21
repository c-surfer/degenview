//! The sidebar's menus: their widths, the section and row menus, and the watchlist membership
//! submenu.

use super::header::flag_row;
use super::{PromptKind, WatchlistAction, WatchlistSidebarInput, WatchlistSidebarState};

use crate::model::watchlist::{EntryId, Watchlist, WatchlistFlag, WatchlistInstrument};
use crate::ui::icons::Icon;
use crate::ui::kit;
use eframe::egui;

/// The header "…" menu and the row menus, and their submenus. Measured against their rows by
/// `menu_widths_fit_their_rows`, which fails if either drifts too narrow or wide. (The list menu
/// is sized from its list names when it opens.)
///
/// The bundled UI font (Inter) sets the width, the same on every platform: the rows need 195.4 and
/// 187.8, so these are a point or so over.
pub(super) const MENU_WIDTH: f32 = 196.0;
pub(super) const SUBMENU_WIDTH: f32 = 189.0;
/// What the menu frame's margins take from its width.
pub(super) const MENU_FRAME: f32 = 10.0;

pub(super) fn section_menu(
    ui: &mut egui::Ui,
    state: &mut WatchlistSidebarState,
    list: &Watchlist,
    section: EntryId,
    title: &str,
    collapsed: bool,
    actions: &mut Vec<WatchlistAction>,
) {
    kit::menu_scope(ui, SUBMENU_WIDTH - MENU_FRAME);
    let (icon, label) = if collapsed {
        (Icon::ChevronDown, crate::ui::tr("Expand Section", "Развернуть секцию"))
    } else {
        (Icon::ChevronUp, crate::ui::tr("Collapse Section", "Свернуть секцию"))
    };
    if kit::menu_item(ui, icon, label, true, false) {
        actions.push(WatchlistAction::SetSectionCollapsed {
            list: list.id,
            section,
            collapsed: !collapsed,
        });
        ui.close();
    }
    if kit::menu_item(ui, Icon::Pencil, crate::ui::tr("Rename Section…", "Переименовать секцию…"), true, false) {
        state.open_prompt(
            PromptKind::RenameSection {
                list: list.id,
                section,
            },
            title.to_owned(),
        );
        ui.close();
    }
    if kit::menu_item(ui, Icon::Plus, crate::ui::tr("Add Section…", "Добавить секцию…"), true, false) {
        state.open_prompt(PromptKind::NewSection(list.id), String::new());
        ui.close();
    }
    kit::menu_separator(ui);
    if kit::menu_item(ui, Icon::Trash, crate::ui::tr("Delete Section", "Удалить секцию"), true, true) {
        actions.push(WatchlistAction::DeleteSection {
            list: list.id,
            section,
        });
        ui.close();
    }
}

/// The actions for one market, used by its context menu.
pub(super) fn instrument_menu(
    ui: &mut egui::Ui,
    state: &mut WatchlistSidebarState,
    input: &WatchlistSidebarInput,
    list: &Watchlist,
    item: &WatchlistInstrument,
    actions: &mut Vec<WatchlistAction>,
) {
    kit::menu_scope(ui, MENU_WIDTH - MENU_FRAME);
    if kit::menu_item(ui, Icon::ChartLine, crate::ui::tr("Open in Focused Chart", "Открыть на активном графике"), true, false) {
        actions.push(WatchlistAction::Open(item.clone()));
        ui.close();
    }
    if kit::menu_item(ui, Icon::Plus, crate::ui::tr("Add as New Chart", "Добавить как новый график"), true, false) {
        actions.push(WatchlistAction::AddChart(item.clone()));
        ui.close();
    }
    if kit::menu_item(ui, Icon::ArrowUpRightSquare, crate::ui::tr("Open in New Tab", "Открыть в новой вкладке"), true, false) {
        actions.push(WatchlistAction::OpenInNewTab(item.clone()));
        ui.close();
    }
    kit::menu_separator(ui);

    membership_menu(ui, input.lists, item, input.load_failed, actions, |item| {
        state.prompt_new_list_with(item);
    });

    if list.sections().next().is_some() {
        let owner = list.owning_section(item.id).map(|section| section.id);
        let row = kit::MenuRow {
            icon: Some(Icon::Layers),
            ..kit::MenuRow::new(crate::ui::tr("Move to Section", "Переместить в секцию"))
        };
        kit::menu_submenu(ui, row, SUBMENU_WIDTH, |ui| {
            let none = kit::menu_row(
                ui,
                kit::MenuRow {
                    checked: owner.is_none(),
                    ..kit::MenuRow::new(crate::ui::tr("No Section", "Без секции"))
                },
            );
            if none.clicked() {
                actions.push(WatchlistAction::MoveToSection {
                    list: list.id,
                    entry: item.id,
                    section: None,
                });
                ui.close();
            }
            for section in list.sections() {
                let target = kit::menu_row(
                    ui,
                    kit::MenuRow {
                        checked: owner == Some(section.id),
                        ..kit::MenuRow::new(&section.title)
                    },
                );
                if target.clicked() {
                    actions.push(WatchlistAction::MoveToSection {
                        list: list.id,
                        entry: item.id,
                        section: Some(section.id),
                    });
                    ui.close();
                }
            }
        });
    }

    let current = input.flags.get(&item.instrument.key()).copied();
    let row = kit::MenuRow {
        icon: Some(Icon::Tag),
        ..kit::MenuRow::new(crate::ui::tr("Set Flag", "Установить флаг"))
    };
    kit::menu_submenu(ui, row, SUBMENU_WIDTH, |ui| {
        for flag in WatchlistFlag::ALL {
            if flag_row(ui, flag, current == Some(flag)).clicked() {
                actions.push(WatchlistAction::SetFlag {
                    instrument: item.instrument.clone(),
                    flag: (current != Some(flag)).then_some(flag),
                });
                ui.close();
            }
        }
        kit::menu_separator(ui);
        if kit::menu_item(ui, Icon::Xmark, crate::ui::tr("Clear Flag", "Снять флаг"), current.is_some(), false) {
            actions.push(WatchlistAction::SetFlag {
                instrument: item.instrument.clone(),
                flag: None,
            });
            ui.close();
        }
    });

    if !item.instrument.source.is_prediction_market()
        && item.instrument.source != crate::model::DataSource::CoinMarketCap
        && kit::menu_item(ui, Icon::Bell, crate::ui::tr("Create Price Alert…", "Создать оповещение…"), true, false)
    {
        actions.push(WatchlistAction::CreateAlert(item.clone()));
        ui.close();
    }

    kit::menu_separator(ui);
    if kit::menu_item(ui, Icon::CopyDoc, crate::ui::tr("Copy Symbol", "Скопировать символ"), true, false) {
        actions.push(WatchlistAction::CopyText(item.instrument.symbol.clone()));
        ui.close();
    }
    if kit::menu_item(ui, Icon::Clipboard, crate::ui::tr("Copy Qualified Symbol", "Скопировать полный символ"), true, false) {
        actions.push(WatchlistAction::CopyText(format!(
            "{}:{}",
            item.instrument.source.watchlist_slug(),
            item.instrument.symbol
        )));
        ui.close();
    }
    kit::menu_separator(ui);
    if kit::menu_item(ui, Icon::Trash, crate::ui::tr("Remove from Watchlist", "Удалить из списка"), true, true) {
        actions.push(WatchlistAction::RemoveEntry {
            list: list.id,
            entry: item.id,
        });
        ui.close();
    }
}

/// "Add to Watchlist": every list with a check where the market already is, then New
/// Watchlist…. Shared by the sidebar, chart cards and search results so membership
/// reads the same everywhere. `on_new_list` is called for "New Watchlist…"; the caller
/// decides where the name prompt lives. Ticking a list leaves the menu open, so a market can
/// join several in one visit.
pub fn membership_menu(
    ui: &mut egui::Ui,
    lists: &[Watchlist],
    item: &WatchlistInstrument,
    disabled: bool,
    actions: &mut Vec<WatchlistAction>,
    on_new_list: impl FnOnce(WatchlistInstrument),
) {
    let row = kit::MenuRow {
        icon: Some(Icon::Star),
        enabled: !disabled,
        ..kit::MenuRow::new(crate::ui::tr("Add to Watchlist", "Добавить в список"))
    };
    let response = kit::menu_submenu(ui, row, SUBMENU_WIDTH, |ui| {
        for list in lists {
            let member = list.contains(&item.instrument);
            let toggle = kit::menu_row(
                ui,
                kit::MenuRow {
                    checked: member,
                    ..kit::MenuRow::new(crate::ui::t(&list.name))
                },
            );
            if toggle.clicked() {
                actions.push(if member {
                    WatchlistAction::RemoveFromList {
                        list: list.id,
                        instrument: item.instrument.clone(),
                    }
                } else {
                    WatchlistAction::AddToList {
                        list: list.id,
                        item: item.clone(),
                    }
                });
            }
        }
        kit::menu_separator(ui);
        if kit::menu_item(ui, Icon::Plus, crate::ui::tr("New Watchlist…", "Новый список…"), true, false) {
            on_new_list(item.clone());
            ui.close();
        }
    });
    if disabled {
        response.on_hover_text(crate::ui::tr("Watchlists are unavailable", "Списки наблюдения недоступны"));
    }
}
