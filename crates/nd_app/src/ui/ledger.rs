//! [ledger] The city's money as accounts in the UI: an account's balance
//! and its last moves (the household, business and owner inspectors), and
//! Economy, Money flow: where rent, wages, profits and taxes go, drawn as
//! bands from payers to payees (a Sankey in rows) with the table behind it.

use super::*;
use nd_sim::bank::{dollars, Account, Cell, Class, Kind, CLASSES};

/// One account, folded: its balance in the header and its last moves
/// inside, newest first, each with who was on the other side.
pub fn account_section(ui: &mut egui::Ui, w: &nd_sim::World, a: Account, title: &str) {
    let bank = &w.economy.bank;
    let balance = bank.balance(a);
    let recent = bank.recent(a);
    let head = format!("{title}  ·  {}  ·  {} recent moves", super::fmt::money(balance), recent.len());
    egui::CollapsingHeader::new(egui::RichText::new(head).small()).id_salt(("ledger_account", a)).show(ui, |ui| {
        if recent.is_empty() {
            hint(ui, "Nothing has moved through this account since the city was loaded.");
            return;
        }
        egui::Grid::new(("ledger_rows", a)).num_columns(4).spacing([8.0, 1.0]).striped(true).show(ui, |ui| {
            for t in recent.iter().rev().take(20) {
                ui.label(egui::RichText::new(super::fmt::day_time(t.tick)).small().weak());
                ui.label(egui::RichText::new(t.kind.label()).small());
                let other = crate::remote::ledger::account_name(w, t.other);
                ui.label(egui::RichText::new(if t.units >= 0 { format!("from {other}") } else { format!("to {other}") }).small());
                let col = if t.units >= 0 { theme::GOOD } else { WARN };
                ui.label(egui::RichText::new(format!("{:+.2}", t.dollars())).small().color(col));
                ui.end_row();
            }
        });
    });
}

/// Rows of the page: (kind group, from class, to class) → units, biggest
/// first; groups in a fixed order.
const GROUPS: [(&str, &str); 6] = [
    ("property", "Rent and property: tenants to owners, owners to upkeep, tax and the bank"),
    ("wages", "Wages: earnings in from outside, payrolls out, the purse and the pockets"),
    ("profits", "Profits and investment: dividends, stakes, deposits"),
    ("city", "The city's books: taxes, fares and fees in, upkeep and building out"),
    ("trade", "Trade: shopping, supplies, imports, exports, visitors"),
    ("other", "Everything else: cars, fares, fees, gifts, theft, arrivals and departures"),
];

fn class_color(c: Class) -> egui::Color32 {
    match c {
        Class::Outside => egui::Color32::from_rgb(150, 150, 170),
        Class::City => GOLD,
        Class::Household => egui::Color32::from_rgb(120, 190, 255),
        Class::Person => egui::Color32::from_rgb(160, 210, 255),
        Class::Business => egui::Color32::from_rgb(255, 150, 90),
        Class::Company => egui::Color32::from_rgb(230, 120, 220),
        Class::Corp => egui::Color32::from_rgb(200, 90, 255),
        Class::University => egui::Color32::from_rgb(120, 230, 200),
        Class::Landlords => egui::Color32::from_rgb(220, 180, 120),
        Class::Tourist => egui::Color32::from_rgb(140, 240, 140),
        Class::Airline => egui::Color32::from_rgb(140, 220, 240),
    }
}

/// Economy, Money flow.
pub fn page(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let bank = &w.economy.bank;
    let id = egui::Id::new("money_flow_day");
    let mut yesterday = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(id, || true));
    let have_yesterday = bank.yesterday().is_some();
    ui.horizontal(|ui| {
        ui.label("Show");
        if ui.selectable_label(yesterday && have_yesterday, "yesterday").clicked() {
            yesterday = true;
        }
        if ui.selectable_label(!yesterday || !have_yesterday, "today so far").clicked() {
            yesterday = false;
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, yesterday));
    let cells: Vec<Cell> = if yesterday && have_yesterday { bank.yesterday().map(|d| d.cells.clone()).unwrap_or_default() } else { bank.today_cells() };
    let inn: i64 = cells.iter().filter(|c| c.from == Class::Outside).map(|c| c.units).sum();
    let out: i64 = cells.iter().filter(|c| c.to == Class::Outside).map(|c| c.units).sum();
    let rent = |k: Kind| dollars(cells.iter().filter(|c| c.kind == k).map(|c| c.units).sum());
    pane::cards(ui, |ui| {
        pane::kpi(ui, "rent paid", egui::RichText::new(super::fmt::money(rent(Kind::Rent) + rent(Kind::ShopRent))).color(GOLD));
        pane::kpi(ui, "in from outside", egui::RichText::new(super::fmt::money(dollars(inn))).color(theme::GOOD));
        pane::kpi(ui, "out of town", egui::RichText::new(super::fmt::money(dollars(out))).color(WARN));
        pane::kpi(ui, "books balance", egui::RichText::new(if w.ledger_sum() == 0 { "to the cent".to_string() } else { format!("off by {}", w.ledger_sum()) }));
        if bank.untracked_moves > 0 {
            pane::kpi(ui, "untracked", egui::RichText::new(format!("{} ({}×)", super::fmt::money(dollars(bank.untracked_units)), bank.untracked_moves)).color(WARN));
        }
    });
    hint(ui, "Every dollar moves from one account to another. Bands run from who paid (left) to who was paid (right), as wide as the money; the outside world is everything beyond the city limits.");
    for (g, blurb) in GROUPS {
        let mut rows: Vec<Cell> = cells.iter().filter(|c| c.kind.group() == g && c.units != 0).copied().collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by(|a, b| b.units.abs().cmp(&a.units.abs()));
        let total = dollars(rows.iter().map(|c| c.units).sum());
        section(ui, &format!("{}  ·  {}", g.to_uppercase(), super::fmt::money(total)));
        hint(ui, blurb);
        sankey(ui, &rows);
        egui::CollapsingHeader::new(egui::RichText::new(format!("{} flows", rows.len())).small()).id_salt(("money_flow_rows", g)).show(ui, |ui| {
            egui::Grid::new(("money_flow_grid", g)).num_columns(5).spacing([10.0, 1.0]).striped(true).show(ui, |ui| {
                for c in &rows {
                    ui.label(egui::RichText::new(c.kind.label()).small());
                    ui.label(egui::RichText::new(c.from.label()).small().color(class_color(c.from)));
                    ui.label(egui::RichText::new("→").small().weak());
                    ui.label(egui::RichText::new(c.to.label()).small().color(class_color(c.to)));
                    ui.label(egui::RichText::new(format!("{}  ({}×)", super::fmt::money(c.dollars()), c.n)).small());
                    ui.end_row();
                }
            });
        });
    }
}

/// Payers on the left, payees on the right, a band between for each flow,
/// as thick as its share of the group's money.
fn sankey(ui: &mut egui::Ui, rows: &[Cell]) {
    let mut left = [0i64; CLASSES];
    let mut right = [0i64; CLASSES];
    for c in rows {
        left[c.from.index()] += c.units.abs();
        right[c.to.index()] += c.units.abs();
    }
    let total: i64 = left.iter().sum::<i64>().max(1);
    let width = ui.available_width().clamp(300.0, 900.0);
    let used_l = left.iter().filter(|x| **x > 0).count();
    let used_r = right.iter().filter(|x| **x > 0).count();
    let gap = 6.0;
    let height = 120.0f32.max(14.0 * used_l.max(used_r) as f32 + gap * used_l.max(used_r) as f32);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let bar_w = 10.0;
    let span = |n: usize| (height - gap * (n.saturating_sub(1)) as f32).max(10.0);
    // Where each class's node sits on a side: (top, scale).
    let place = |side: &[i64; CLASSES], n: usize| {
        let mut tops = [0.0f32; CLASSES];
        let mut y = rect.top();
        let h = span(n);
        for c in Class::ALL {
            let v = side[c.index()];
            if v > 0 {
                tops[c.index()] = y;
                y += v as f32 / total as f32 * h + gap;
            }
        }
        (tops, h)
    };
    let (ltop, lh) = place(&left, used_l);
    let (rtop, rh) = place(&right, used_r);
    // Room for the labels either side ("independent landlords $12k").
    let (lx, rx) = (rect.left() + 170.0, rect.right() - 180.0);
    let mut lcur = ltop;
    let mut rcur = rtop;
    for c in rows {
        let v = c.units.abs() as f32 / total as f32;
        let (a0, a1) = (lcur[c.from.index()], lcur[c.from.index()] + v * lh);
        let (b0, b1) = (rcur[c.to.index()], rcur[c.to.index()] + v * rh);
        lcur[c.from.index()] = a1;
        rcur[c.to.index()] = b1;
        // A band of quads along a smooth step from left to right.
        let col = class_color(c.from).gamma_multiply(0.35);
        let steps = 24;
        for i in 0..steps {
            let t0 = i as f32 / steps as f32;
            let t1 = (i + 1) as f32 / steps as f32;
            let s = |t: f32| t * t * (3.0 - 2.0 * t);
            let x0 = lx + bar_w + (rx - lx - bar_w) * t0;
            let x1 = lx + bar_w + (rx - lx - bar_w) * t1;
            let top0 = a0 + (b0 - a0) * s(t0);
            let bot0 = a1 + (b1 - a1) * s(t0);
            let top1 = a0 + (b0 - a0) * s(t1);
            let bot1 = a1 + (b1 - a1) * s(t1);
            p.add(egui::Shape::convex_polygon(vec![egui::pos2(x0, top0), egui::pos2(x1, top1), egui::pos2(x1, bot1.max(top1 + 0.5)), egui::pos2(x0, bot0.max(top0 + 0.5))], col, egui::Stroke::NONE));
        }
    }
    let font = egui::FontId::proportional(11.0);
    for c in Class::ALL {
        let v = left[c.index()];
        if v > 0 {
            let h = v as f32 / total as f32 * lh;
            let r = egui::Rect::from_min_size(egui::pos2(lx, ltop[c.index()]), egui::vec2(bar_w, h.max(1.0)));
            p.rect_filled(r, 1.0, class_color(c));
            p.text(egui::pos2(lx - 4.0, r.center().y), egui::Align2::RIGHT_CENTER, format!("{} {}", c.label(), super::fmt::money(dollars(v))), font.clone(), IVORY);
        }
        let v = right[c.index()];
        if v > 0 {
            let h = v as f32 / total as f32 * rh;
            let r = egui::Rect::from_min_size(egui::pos2(rx, rtop[c.index()]), egui::vec2(bar_w, h.max(1.0)));
            p.rect_filled(r, 1.0, class_color(c));
            p.text(egui::pos2(rx + bar_w + 4.0, r.center().y), egui::Align2::LEFT_CENTER, format!("{} {}", c.label(), super::fmt::money(dollars(v))), font.clone(), IVORY);
        }
    }
}
