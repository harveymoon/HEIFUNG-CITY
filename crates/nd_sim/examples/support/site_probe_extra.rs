// [r14-builders] The builders' own books (branch only; on main this file
// is `fn extra(_: &World) -> String { String::new() }`).
fn extra(w: &World) -> String {
    let Some(d) = w.builders.yesterday() else { return String::new() };
    let merchants: Vec<String> = w.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::BuildersMerchant)).map(|(_, b)| format!("{}{}", b.name, if b.construction.is_some() { " (building)" } else { "" })).collect();
    let yards: Vec<String> = w.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::Contractor)).map(|(id, b)| format!("{} [{}{} staff {} crews {} jobs {} money {:.0}]", b.name, if b.construction.is_some() { "building, " } else { "" }, if b.business.as_ref().unwrap().closed { "BUST, " } else { "" }, b.workers.len(), w.contractor_crews(id), w.contractor_sites(id).len(), b.business.as_ref().unwrap().money)).collect();
    format!(
        "  builders: let local {} out-of-town {} | finished {} (local {}) mean {:.2}d | loads yard {} merchant {} maker {} scrap {} import {} (collected {}) | paid local {:.0} out-of-town fees {:.0} | merchant trade {:.0} takings {:.0} | contractors {} money {:.0} crews {} busts {} | merchants {}\n  yards: {}\n  merchants: {}",
        d.let_local, d.let_outside, d.finished, d.finished_local, d.mean_build_days(), d.yard_loads, d.merchant_loads, d.maker_loads, d.scrap_loads, d.import_loads, d.collected, d.contracts_paid, d.outside_paid, d.trade_sales, d.merchant_takings, d.contractors, d.contractor_money, d.crews, d.busts, d.merchants, yards.join("; "), merchants.join("; ")
    )
}
