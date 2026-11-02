//! The page's right-button menu: the operators in a submenu per group of the table,
//! then Duplicate and Delete for the block under the pointer.

use std::cell::Cell;
use std::rc::Rc;

use mscent_texgen::ops;
use xui::prelude::*;
use xui::widget::Menu;

use super::TexMsg;
use crate::ui::Msg;

/// Commands of the menu; adding an operator is its table index past `ADD_BASE`.
pub(super) const MENU_DELETE: MenuId = MenuId::new(1000);
pub(super) const MENU_DUPLICATE: MenuId = MenuId::new(1001);
const GROUP_BASE: usize = 1100;
const ADD_BASE: usize = 1200;

/// The operators' menu, by their group in the table.
pub(super) fn page_menu(ui: &Ui<Msg>, cell: Rc<Cell<(u16, u16)>>) -> Menu<Msg> {
    let mut groups: Vec<&'static str> = Vec::new();
    for op in ops::all() {
        if !groups.contains(&op.group) {
            groups.push(op.group);
        }
    }
    let fill = move |m: &mut MenuScope<'_>| {
        for (g, group) in groups.iter().enumerate() {
            m.submenu(MenuId::new(GROUP_BASE + g), group, |sub| {
                for (i, op) in ops::all().enumerate().filter(|(_, o)| o.group == *group) {
                    sub.item(MenuId::new(ADD_BASE + i), op.name);
                }
            });
        }
        m.separator();
        m.item(MENU_DUPLICATE, "Duplicate");
        m.item(MENU_DELETE, "Delete   Del");
    };
    Menu::context(ui).build(fill).on_select(move |id| {
        if id == MENU_DELETE {
            return Some(Msg::Tex(TexMsg::Delete));
        }
        if id == MENU_DUPLICATE {
            return Some(Msg::Tex(TexMsg::Duplicate));
        }
        let op = (0..ops::all().count()).find(|i| MenuId::new(ADD_BASE + i) == id)?;
        Some(Msg::Tex(TexMsg::Add {
            op,
            cell: cell.get(),
        }))
    })
}
