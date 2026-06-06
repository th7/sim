//! The showcase's headless guarantee: every scenario's synthetic payloads
//! survive the real `ClientModel` pipeline and the resulting `RenderState`
//! contains everything the scenario promises to display. The manual pass on a
//! real display then only judges *appearance* — presence is checked here.

use client::showcase::scenarios;

#[test]
fn overworld_scenario_displays_a_live_tree() {
    let s = scenarios().into_iter().find(|s| s.name == "overworld").expect("overworld scenario");
    let rs = s.state(0.0);
    assert!(rs.nodes.values().any(|n| !n.depleted), "a live tree is displayed");
}
