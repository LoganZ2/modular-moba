//! Stat computation (level + items + buffs) and status timers.

use super::combat::{self, Hit};
use super::*;
use crate::items::item;
use crate::skills::BuffStat;
use crate::stats::StatBlock;

/// Decrement status durations, apply regeneration and damage over time,
/// tick cooldowns.
pub(crate) fn tick_statuses(w: &mut World) {
    let mut burns: Vec<(EntityId, f32, EntityId)> = Vec::new();
    for e in &mut w.entities {
        // Cooldowns keep running while a champion is dead (as in LoL).
        if let Some(h) = e.hero_mut() {
            for cd in h.cooldowns.iter_mut().chain(h.summoner_cd.iter_mut()) {
                *cd = (*cd - DT).max(0.0);
            }
        }
        if !e.alive {
            e.statuses.clear();
            continue;
        }
        let mut regen = 0.0;
        for s in &mut e.statuses {
            s.remaining -= DT;
            match s.kind {
                StatusKind::Regen(r) => regen += r,
                StatusKind::Burn { dps, source } => burns.push((e.id, dps * DT, source)),
                _ => {}
            }
        }
        e.statuses.retain(|s| s.remaining > 0.0 && !matches!(s.kind, StatusKind::Shield(v) if v <= 0.0));
        e.hp = (e.hp + (regen + e.stats.hp_regen) * DT).min(e.stats.max_hp);
        e.mana = (e.mana + e.stats.mana_regen * DT).min(e.stats.max_mana);
        e.attack_cd -= DT;
        e.repath -= DT;
        if let Some(h) = e.hero_mut() {
            h.damagers.retain(|(_, t)| w.time - *t <= ASSIST_WINDOW);
        }
    }
    for (target, dmg, source) in burns {
        combat::deal_damage(w, source, target, dmg, DamageKind::True, Hit::QUIET);
    }
}

pub(crate) fn update_all(w: &mut World) {
    for e in &mut w.entities {
        recompute(e);
    }
}

fn recompute(e: &mut Entity) {
    let (mut st, base_as, mut bonus_as) = match &e.kind {
        UnitKind::Hero(h) => hero_stats(h),
        _ => (e.base, e.base.attack_speed, 0.0),
    };
    let mut ms_pct = 0.0;
    let mut slow: f32 = 0.0;
    for s in &e.statuses {
        match s.kind {
            StatusKind::Slow(p) => slow = slow.max(p),
            StatusKind::Buff(stat, amount) => match stat {
                BuffStat::MoveSpeed => ms_pct += amount,
                BuffStat::AttackSpeed => bonus_as += amount,
                BuffStat::Armor => st.armor += amount,
                BuffStat::MagicResist => st.mr += amount,
                BuffStat::AttackDamage => st.ad += amount,
                BuffStat::AbilityPower => st.ap += amount,
            },
            StatusKind::Ghost => ms_pct += 0.35,
            StatusKind::BlueBuff => {
                st.haste += 15.0;
                st.mana_regen += 4.0;
            }
            StatusKind::RedBuff => st.hp_regen += 3.0,
            StatusKind::BaronBuff => {
                st.ad += 30.0;
                st.ap += 50.0;
            }
            _ => {}
        }
    }
    st.attack_speed = (base_as * (1.0 + bonus_as)).min(2.5);
    st.move_speed = (st.move_speed * (1.0 + ms_pct) * (1.0 - slow.min(0.9))).max(25.0);

    if e.alive && e.stats.max_hp > 0.0 {
        if st.max_hp > e.stats.max_hp {
            e.hp += st.max_hp - e.stats.max_hp;
        }
        if st.max_mana > e.stats.max_mana {
            e.mana += st.max_mana - e.stats.max_mana;
        }
    }
    e.hp = e.hp.min(st.max_hp);
    e.mana = e.mana.min(st.max_mana);
    e.stats = st;
}

/// (stats without temporary buffs, base attack speed, bonus attack speed fraction)
pub(crate) fn hero_stats(h: &HeroState) -> (Stats, f32, f32) {
    let b = &h.def().base;
    let l = (h.level.max(1) - 1) as f32;
    let mut items = StatBlock::ZERO;
    for (id, _) in h.items.iter().flatten() {
        let it = item(*id);
        if it.stack == 1 {
            items += it.stats;
        }
    }
    let base_ad = b.ad + b.ad_lvl * l;
    let st = Stats {
        max_hp: b.hp + b.hp_lvl * l + items.hp,
        max_mana: b.mana + b.mana_lvl * l + items.mana,
        ad: base_ad + items.ad,
        base_ad,
        ap: items.ap,
        armor: b.armor + b.armor_lvl * l + items.armor,
        mr: b.mr + b.mr_lvl * l + items.mr,
        attack_speed: b.attack_speed,
        range: b.range,
        move_speed: b.move_speed + items.move_speed,
        crit: items.crit.min(1.0),
        lifesteal: items.lifesteal,
        haste: items.haste,
        hp_regen: b.hp_regen + 0.1 * l + items.hp_regen,
        mana_regen: b.mana_regen + 0.08 * l + items.mana_regen,
    };
    (st, b.attack_speed, b.as_lvl * l + items.attack_speed)
}
