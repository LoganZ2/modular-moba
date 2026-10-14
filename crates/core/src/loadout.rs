//! Loadouts: hero + three basic skills (Q, W, E) + two summoner spells.
//!
//! The rule that makes the game "modular": **every skill in Q/W/E must share
//! at least one role with the hero**. R is always the hero's own ultimate.

use crate::heroes::{try_hero, HeroDef, HeroId, HEROES};
use crate::roles::shares_role;
use crate::skills::{basic_skills, try_skill, SkillDef, SkillId};
use crate::summoners::SummonerId;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loadout {
    pub hero: HeroId,
    /// Q, W, E.
    pub skills: [SkillId; 3],
    /// D, F.
    pub summoners: [SummonerId; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadoutError {
    UnknownHero(HeroId),
    UnknownSkill(SkillId),
    UltimateInBasicSlot(SkillId),
    DuplicateSkill(SkillId),
    NoSharedRole { skill: SkillId, hero: HeroId },
    DuplicateSummoner(SummonerId),
}

impl fmt::Display for LoadoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadoutError::UnknownHero(h) => write!(f, "unknown hero #{}", h.0),
            LoadoutError::UnknownSkill(s) => write!(f, "unknown skill #{}", s.0),
            LoadoutError::UltimateInBasicSlot(s) => {
                write!(f, "{} is an ultimate and can't go in Q/W/E", skill_name(*s))
            }
            LoadoutError::DuplicateSkill(s) => write!(f, "{} is equipped twice", skill_name(*s)),
            LoadoutError::NoSharedRole { skill, hero } => write!(
                f,
                "{} shares no role with {}",
                skill_name(*skill),
                try_hero(*hero).map(|h| h.name).unwrap_or("?")
            ),
            LoadoutError::DuplicateSummoner(s) => write!(f, "{} is picked twice", s.name()),
        }
    }
}

fn skill_name(id: SkillId) -> &'static str {
    try_skill(id).map(|s| s.name).unwrap_or("?")
}

/// Can `hero` put `skill` in Q/W/E?
pub fn can_equip(hero: &HeroDef, skill: &SkillDef) -> bool {
    !skill.ultimate && shares_role(&hero.roles, &skill.roles)
}

/// All basic skills `hero` may equip.
pub fn eligible_skills(hero: &HeroDef) -> Vec<&'static SkillDef> {
    basic_skills().filter(|s| can_equip(hero, s)).collect()
}

impl Loadout {
    /// The hero's recommended build.
    pub fn suggested(hero: HeroId) -> Loadout {
        let def = try_hero(hero).unwrap_or(&HEROES[0]);
        Loadout { hero: def.id, skills: def.suggested, summoners: [SummonerId::Flash, default_second_summoner(def)] }
    }

    pub fn validate(&self) -> Result<(), LoadoutError> {
        let hero = try_hero(self.hero).ok_or(LoadoutError::UnknownHero(self.hero))?;
        for (i, id) in self.skills.iter().enumerate() {
            let sk = try_skill(*id).ok_or(LoadoutError::UnknownSkill(*id))?;
            if sk.ultimate {
                return Err(LoadoutError::UltimateInBasicSlot(*id));
            }
            if self.skills[..i].contains(id) {
                return Err(LoadoutError::DuplicateSkill(*id));
            }
            if !can_equip(hero, sk) {
                return Err(LoadoutError::NoSharedRole { skill: *id, hero: self.hero });
            }
        }
        if self.summoners[0] == self.summoners[1] {
            return Err(LoadoutError::DuplicateSummoner(self.summoners[0]));
        }
        Ok(())
    }

    /// Fix up an arbitrary loadout into a valid one (used for bots and as a
    /// guard for stale client state): invalid skills are replaced by the
    /// hero's suggestions or other eligible skills.
    pub fn sanitized(&self) -> Loadout {
        if self.validate().is_ok() {
            return *self;
        }
        let hero = try_hero(self.hero).unwrap_or(&HEROES[0]);
        let mut out = Loadout::suggested(hero.id);
        let mut chosen: Vec<SkillId> = Vec::new();
        for id in self.skills {
            if let Some(sk) = try_skill(id) {
                if can_equip(hero, sk) && !chosen.contains(&id) {
                    chosen.push(id);
                }
            }
        }
        for id in hero.suggested.iter().copied().chain(eligible_skills(hero).iter().map(|s| s.id)) {
            if chosen.len() >= 3 {
                break;
            }
            if !chosen.contains(&id) {
                chosen.push(id);
            }
        }
        out.skills = [chosen[0], chosen[1], chosen[2]];
        if self.summoners[0] != self.summoners[1] {
            out.summoners = self.summoners;
        }
        out
    }
}

fn default_second_summoner(hero: &HeroDef) -> SummonerId {
    use crate::roles::Role::*;
    match hero.roles[0] {
        Tank => SummonerId::Ignite,
        Fighter | Assassin => SummonerId::Ignite,
        Mage => SummonerId::Barrier,
        Marksman => SummonerId::Heal,
        Support => SummonerId::Exhaust,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heroes::HEROES;

    #[test]
    fn suggested_loadouts_are_valid() {
        for h in HEROES {
            let l = Loadout::suggested(h.id);
            assert_eq!(l.validate(), Ok(()), "{}", h.name);
        }
    }

    #[test]
    fn every_hero_has_plenty_of_choices() {
        for h in HEROES {
            let n = eligible_skills(h).len();
            assert!(n >= 10, "{} only has {} eligible skills", h.name, n);
        }
    }

    #[test]
    fn rejects_skill_without_shared_role() {
        // Ironclad is Tank/Fighter; Healing Wave is Mage/Support.
        let mut l = Loadout::suggested(HeroId(0));
        l.skills[0] = SkillId(20);
        assert_eq!(l.validate(), Err(LoadoutError::NoSharedRole { skill: SkillId(20), hero: HeroId(0) }));
        let fixed = l.sanitized();
        assert_eq!(fixed.validate(), Ok(()));
    }

    #[test]
    fn rejects_duplicates_and_ultimates() {
        let mut l = Loadout::suggested(HeroId(0));
        l.skills[1] = l.skills[0];
        assert!(matches!(l.validate(), Err(LoadoutError::DuplicateSkill(_))));
        let mut l = Loadout::suggested(HeroId(0));
        l.skills[2] = SkillId(101);
        assert!(matches!(l.validate(), Err(LoadoutError::UltimateInBasicSlot(_))));
        let mut l = Loadout::suggested(HeroId(0));
        l.summoners = [SummonerId::Flash, SummonerId::Flash];
        assert!(matches!(l.validate(), Err(LoadoutError::DuplicateSummoner(_))));
    }
}
