//! Runtime coverage for the LIFETIME of `ChosenAttribute::Color` on one source
//! (CR 607.2d + CR 400.7 + CR 608.2d).
//!
//! Two seams, one file, because they only make sense together:
//!
//! * **U1 — replace-on-rechoose.** `apply_choice_attributes`
//!   (`game/effects/choose.rs`) used to clear only `Keyword` / `Counter` /
//!   `Direction`, so a source that resolved a colour choice twice ACCUMULATED
//!   both answers while every reader (`GameObject::chosen_color`, both
//!   `FilterProp::IsChosenColor` arms in `game/filter.rs`,
//!   `DevotionColors::ChosenColor` in `game/quantity.rs`) takes the FIRST match.
//!   The second resolution therefore silently used the first resolution's
//!   answer. CR 607.2d links "choose a [value]" to "the chosen [value]" per
//!   choice; CR 400.7 makes a recast spell a new object that nonetheless keeps
//!   the storage object's attributes, because `chosen_attributes` is cleared
//!   only by `reset_for_battlefield_entry`, which a spell never reaches.
//!
//! * **U0 — the CR 607.2d linked colour choice.** Floating Shield makes its
//!   choice on its as-enters replacement and READS IT BACK on its
//!   "Sacrifice this Aura:" ability. The keyword-grant injector used to hand
//!   that ability a chooser of its own, so the Aura ended up holding two
//!   answers — and today's first-match read accidentally preserved the
//!   as-enters one. U1 alone would flip that read to the SECOND (spurious)
//!   answer, so the document relation that removes the spurious chooser has to
//!   land with it. This file pins both directions.
//!
//! Built via the `/card-test` recipe: `GameScenario` +
//! `GameRunner::cast(..)/activate(..).resolve()` + `CastOutcome`/`Outcome` zone
//! deltas, on VERBATIM Oracle text. Every negative assertion is paired with a
//! positive reach-guard in the same test.
//!
//! REVERT DISCRIMINATORS:
//! * `wash_out_recast_on_the_same_object_uses_its_own_color` — revert the
//!   `ChoiceType::Color` arm in `apply_choice_attributes` and the second cast
//!   bounces the FIRST cast's colour.
//! * `floating_shield_sacrifice_grant_reads_the_as_enters_color` — revert the
//!   `LinkedColorChoice` relation and the activation raises a second colour
//!   prompt (and, with U1 in place, the grant then reads that second answer).

use engine::game::scenario::{GameScenario, P0};
use engine::game::zones::move_to_zone;
use engine::types::ability::{ChoiceType, ChosenAttribute};
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::{Keyword, ProtectionTarget};
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

/// Wash Out {3}{U} Sorcery — verbatim Oracle text (MTGJSON `AtomicCards.json`).
const WASH_OUT: &str = "Return all permanents of the color of your choice to their owners' hands.";

/// Knight of Dawn {1}{W}{W} Creature — Human Knight 2/2 — verbatim.
const KNIGHT_OF_DAWN: &str =
    "First strike\n{W}{W}: This creature gains protection from the color of your choice until end \
     of turn.";

/// Hall of Triumph {3} Legendary Artifact — verbatim.
const HALL_OF_TRIUMPH: &str =
    "As Hall of Triumph enters, choose a color.\nCreatures you control of the chosen color get \
     +1/+1.";

/// Floating Shield {2}{W} Enchantment — Aura — verbatim.
const FLOATING_SHIELD: &str =
    "Enchant creature\nAs this Aura enters, choose a color.\nEnchanted creature has protection \
     from the chosen color. This effect doesn't remove this Aura.\nSacrifice this Aura: Target \
     creature gains protection from the chosen color until end of turn.";

/// A one-colour mana cost, so `with_mana_cost` derives exactly that colour
/// (CR 202.2 / CR 105.2).
fn one_color(shard: ManaCostShard) -> ManaCost {
    ManaCost::Cost {
        shards: vec![shard],
        generic: 1,
    }
}

/// `n` units of white mana with no producing source and no spend restrictions —
/// the plainest pool contents that can pay a printed activation cost.
fn white_mana(n: usize) -> Vec<engine::types::mana::ManaUnit> {
    vec![
        engine::types::mana::ManaUnit::new(
            engine::types::mana::ManaType::White,
            ObjectId(0),
            false,
            vec![]
        );
        n
    ]
}

/// Whether an object carries protection from EXACTLY this colour.
///
/// Deliberately not `game::keywords::has_keyword`: that helper matches on the
/// `Keyword` discriminant alone and so answers `true` for protection from ANY
/// colour, which would make every assertion below vacuous. The layer applier
/// bakes `Protection(ChosenColor)` into `Protection(Color(c))` on the recipient
/// (CR 702.16 + CR 613.1), so the concrete colour is what these tests read.
fn has_protection_from(obj: &engine::game::game_object::GameObject, color: ManaColor) -> bool {
    obj.keywords
        .iter()
        .any(|keyword| keyword == &Keyword::Protection(ProtectionTarget::Color(color)))
}

/// Every `ChosenAttribute::Color` currently recorded on an object, in order.
/// The whole point of U1 is that this is at most one element long.
fn chosen_colors(runner: &engine::game::scenario::GameRunner, id: ObjectId) -> Vec<ManaColor> {
    runner.state().objects[&id]
        .chosen_attributes
        .iter()
        .filter_map(|attribute| match attribute {
            ChosenAttribute::Color(color) => Some(*color),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// T1 — the maintainer's explicit ask: the SAME object resolving twice.
// ---------------------------------------------------------------------------

/// T1 (RUNTIME) — CR 607.2d + CR 400.7 + CR 608.2d. A source that resolves a
/// colour choice TWICE binds its OWN answer the second time.
///
/// Wash Out is a sorcery, so CR 608.2n puts it into the graveyard on
/// resolution. The recast therefore has to reuse the SAME storage object —
/// `move_to_zone(.., Zone::Hand, ..)` on the same `ObjectId`, the shape a
/// Regrowth / Yawgmoth's Will recursion produces — rather than casting a second
/// copy, which would be two objects and would not reach this seam at all.
///
/// THE ASSERTION THAT FLIPS on revert: `red_bear` in `Zone::Hand` after the
/// second cast. Before the fix the source held `[Color(Blue), Color(Red)]`, both
/// `FilterProp::IsChosenColor` readers took the FIRST, and the second Wash Out
/// bounced BLUE permanents again while red stayed put.
#[test]
fn wash_out_recast_on_the_same_object_uses_its_own_color() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let blue_bear = scenario
        .add_creature(P0, "Blue Bear", 2, 2)
        .with_mana_cost(one_color(ManaCostShard::Blue))
        .id();
    let red_bear = scenario
        .add_creature(P0, "Red Bear", 2, 2)
        .with_mana_cost(one_color(ManaCostShard::Red))
        .id();
    let green_bear = scenario
        .add_creature(P0, "Green Bear", 2, 2)
        .with_mana_cost(one_color(ManaCostShard::Green))
        .id();
    // Multi-authority decoy: a DIFFERENT object that already chose Green. The
    // read must stay bound to the resolving source, not to "any chosen colour
    // on the board".
    let decoy = scenario
        .add_creature(P0, "Decoy Bear", 1, 1)
        .with_mana_cost(one_color(ManaCostShard::White))
        .id();

    let wash_out = scenario
        .add_spell_to_hand_from_oracle(P0, "Wash Out", false, WASH_OUT)
        .with_mana_cost(ManaCost::zero())
        .id();

    let mut runner = scenario.build();
    runner
        .state_mut()
        .objects
        .get_mut(&decoy)
        .expect("decoy exists")
        .chosen_attributes
        .push(ChosenAttribute::Color(ManaColor::Green));

    // CAST 1 — choose Blue.
    let first = runner.cast(wash_out).choose_option("Blue").resolve();
    // POSITIVE REACH-GUARD (a): the first resolution genuinely ran.
    first.assert_zone(&[blue_bear], Zone::Hand);
    first.assert_zone(&[red_bear, green_bear, decoy], Zone::Battlefield);
    assert_eq!(
        chosen_colors(&runner, wash_out),
        vec![ManaColor::Blue],
        "the first choice must be recorded on the source"
    );
    // CR 608.2n: the sorcery is in the graveyard, which is why the recast has to
    // move the SAME object back to hand.
    assert_eq!(runner.state().objects[&wash_out].zone, Zone::Graveyard);

    // CR 400.7: recursion returns the SAME storage object to hand. Nothing
    // clears `chosen_attributes` on this move — only `reset_for_battlefield_entry`
    // does, and a spell never reaches it.
    let mut events = Vec::new();
    move_to_zone(runner.state_mut(), wash_out, Zone::Hand, &mut events);
    // POSITIVE REACH-GUARD (b): the object really is back in hand and really is
    // still carrying the first answer, so the second cast reaches the seam under
    // test rather than a freshly-cleared object.
    assert_eq!(runner.state().objects[&wash_out].zone, Zone::Hand);
    assert_eq!(
        chosen_colors(&runner, wash_out),
        vec![ManaColor::Blue],
        "the move to hand must not clear the prior answer — otherwise this test \
         would pass without the fix"
    );

    // POSITIVE REACH-GUARD (c) on the RUNTIME CHOICE PATH: the second cast
    // raises its OWN prompt. `drive_resolution` answers a `NamedChoice` window
    // only when a choice was declared and otherwise breaks, so a declared
    // `choose_option` that is never consumed would be silent.
    let halted = runner.cast(wash_out).resolve();
    assert!(
        matches!(
            halted.final_waiting_for(),
            WaitingFor::NamedChoice {
                choice_type: ChoiceType::Color { .. },
                ..
            }
        ),
        "the SECOND resolution must raise its own colour prompt, got {:?}",
        halted.final_waiting_for()
    );
    runner
        .act(engine::types::actions::GameAction::ChooseOption {
            choice: "Red".to_string(),
        })
        .expect("answer the second colour prompt");
    for _ in 0..40 {
        if runner.state().stack.is_empty() {
            break;
        }
        if !matches!(runner.state().waiting_for, WaitingFor::Priority { .. }) {
            break;
        }
        runner
            .act(engine::types::actions::GameAction::PassPriority)
            .expect("pass priority toward resolution");
    }

    // THE ASSERTIONS THAT FLIP if the `Color` arm is reverted.
    assert_eq!(
        chosen_colors(&runner, wash_out),
        vec![ManaColor::Red],
        "the second choice must REPLACE the first, not accumulate behind it"
    );
    assert_eq!(
        runner.state().objects[&red_bear].zone,
        Zone::Hand,
        "the second resolution must bounce its OWN colour"
    );
    // NEGATIVE, with the positive above as its guard: the untouched colours stay.
    assert_eq!(runner.state().objects[&green_bear].zone, Zone::Battlefield);
    assert_eq!(
        runner.state().objects[&decoy].zone,
        Zone::Battlefield,
        "the decoy's own chosen colour must never govern this resolution"
    );
    // REACH-GUARD: the second cast finished rather than hanging on a prompt.
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "no further prompt may be raised, got {:?}",
        runner.state().waiting_for
    );
}

/// T1 sibling — CR 105.4. The second resolution choosing a colour NOTHING has
/// moves nothing, and in particular does not fall back to the first answer.
///
/// This is the vacuity guard for T1: if the second resolution silently reused
/// the first colour, this run would bounce `blue_bear` again.
#[test]
fn wash_out_recast_choosing_an_absent_color_moves_nothing() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let blue_bear = scenario
        .add_creature(P0, "Blue Bear", 2, 2)
        .with_mana_cost(one_color(ManaCostShard::Blue))
        .id();
    let red_bear = scenario
        .add_creature(P0, "Red Bear", 2, 2)
        .with_mana_cost(one_color(ManaCostShard::Red))
        .id();

    let wash_out = scenario
        .add_spell_to_hand_from_oracle(P0, "Wash Out", false, WASH_OUT)
        .with_mana_cost(ManaCost::zero())
        .id();

    let mut runner = scenario.build();
    let first = runner.cast(wash_out).choose_option("Blue").resolve();
    // POSITIVE REACH-GUARD: the first cast really bounced its colour.
    first.assert_zone(&[blue_bear], Zone::Hand);

    let mut events = Vec::new();
    move_to_zone(runner.state_mut(), wash_out, Zone::Hand, &mut events);
    // Put the blue bear back so a first-answer reuse would be observable.
    move_to_zone(
        runner.state_mut(),
        blue_bear,
        Zone::Battlefield,
        &mut events,
    );
    assert_eq!(runner.state().objects[&blue_bear].zone, Zone::Battlefield);

    let second = runner.cast(wash_out).choose_option("White").resolve();

    // THE NEGATIVE: nothing moved, because nothing is white.
    second.assert_zone(&[blue_bear, red_bear], Zone::Battlefield);
    assert_eq!(
        chosen_colors(&runner, wash_out),
        vec![ManaColor::White],
        "the source holds only its CURRENT answer"
    );
    // REACH-GUARD: the spell resolved (CR 608.2n) rather than hanging.
    assert_eq!(runner.state().objects[&wash_out].zone, Zone::Graveyard);
    assert!(
        matches!(second.final_waiting_for(), WaitingFor::Priority { .. }),
        "no further prompt may be raised, got {:?}",
        second.final_waiting_for()
    );
}

// ---------------------------------------------------------------------------
// T2 — a permanent that re-chooses without ever leaving the battlefield.
// ---------------------------------------------------------------------------

/// T2 (RUNTIME) — CR 607.2d + CR 514.2. A permanent that activates its colour
/// choice on two different turns grants protection from its SECOND answer.
///
/// The two activations are on DIFFERENT turns on purpose: the first grant has
/// expired (CR 514.2) before the second is created, so this asserts only the
/// replace-on-rechoose seam and not the CR 611.2c / CR 613.7b per-effect
/// latching residual (two SIMULTANEOUSLY live grants from one source both bake
/// from the source's current colour — filed as F10, deliberately unasserted).
#[test]
fn knight_of_dawn_second_activation_uses_its_own_color() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let knight = scenario
        .add_creature_from_oracle(P0, "Knight of Dawn", 2, 2, KNIGHT_OF_DAWN)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::White, ManaCostShard::White],
            generic: 1,
        })
        .id();
    // Exactly the `{W}{W}` the first activation costs — an unbounded pool would
    // let an unaffordable activation pass unnoticed.
    scenario.with_mana_pool(P0, white_mana(2));

    let mut runner = scenario.build();

    // ACTIVATION 1 — Red. POSITIVE REACH-GUARD: the grant genuinely lands.
    let first = runner.activate(knight, 0).choose_option("Red").resolve();
    assert!(
        matches!(first.final_waiting_for(), WaitingFor::Priority { .. }),
        "activation 1 must resolve, got {:?}",
        first.final_waiting_for()
    );
    assert_eq!(
        chosen_colors(&runner, knight),
        vec![ManaColor::Red],
        "activation 1 records its own answer"
    );
    assert!(
        has_protection_from(&runner.state().objects[&knight], ManaColor::Red),
        "activation 1 must actually grant protection from RED: {:?}",
        runner.state().objects[&knight].keywords
    );

    // CR 514.2: cross the turn boundary (through cleanup) so the first grant
    // expires. The ability has no timing restriction, so the next turn's upkeep
    // is a legal activation window (CR 602.2).
    runner.advance_to_combat();
    runner
        .declare_attackers(&[])
        .expect("declare no attackers (CR 508.1)");
    runner.advance_to_upkeep();
    // REACH-GUARD: the turn really turned over and the first grant really
    // expired, so the "protection from red is gone" assertion at the end is
    // about the SECOND activation's answer and not about a leftover first one.
    assert_eq!(
        runner.state().phase,
        Phase::Upkeep,
        "the run must cross the turn boundary to activate again, got {:?}",
        runner.state().phase
    );
    assert!(
        !has_protection_from(&runner.state().objects[&knight], ManaColor::Red),
        "CR 514.2: the first grant must have expired at cleanup: {:?}",
        runner.state().objects[&knight].keywords
    );
    // CR 117.3a: the new turn's active player receives priority first; pass it
    // to the Knight's controller.
    runner
        .act(engine::types::actions::GameAction::PassPriority)
        .expect("pass priority to the Knight's controller");
    // CR 500.4: the pool emptied at each step/phase boundary — refill exactly
    // the second activation's cost.
    for unit in white_mana(2) {
        let _ = runner.state_mut().add_mana_to_pool(P0, unit);
    }

    // ACTIVATION 2 — Blue.
    let second = runner.activate(knight, 0).choose_option("Blue").resolve();
    assert!(
        matches!(second.final_waiting_for(), WaitingFor::Priority { .. }),
        "activation 2 must resolve, got {:?}",
        second.final_waiting_for()
    );

    // THE ASSERTIONS THAT FLIP if the `Color` arm is reverted: without
    // replace-on-rechoose the source holds `[Red, Blue]`, the layer applier's
    // first-match `chosen_color()` pre-read bakes RED, and the second grant is
    // protection from red.
    assert_eq!(
        chosen_colors(&runner, knight),
        vec![ManaColor::Blue],
        "the second activation must REPLACE the first answer"
    );
    assert!(
        has_protection_from(&runner.state().objects[&knight], ManaColor::Blue),
        "activation 2 must grant protection from its OWN colour: {:?}",
        runner.state().objects[&knight].keywords
    );
    assert!(
        !has_protection_from(&runner.state().objects[&knight], ManaColor::Red),
        "the expired first grant must not survive into turn 2: {:?}",
        runner.state().objects[&knight].keywords
    );
}

// ---------------------------------------------------------------------------
// T3 — the 59-card as-enters bucket is a no-op.
// ---------------------------------------------------------------------------

/// T3 (RUNTIME) — CR 400.7. The as-enters chooser class is untouched: the
/// permanent holds exactly one `Color` before and after, and its dependent
/// static still applies.
///
/// `reset_for_battlefield_entry` clears `chosen_attributes` before each
/// battlefield entry, so this bucket could never accumulate; the row exists so a
/// `retain` that fired on the wrong key would be caught here rather than in the
/// pool.
#[test]
fn as_enters_color_chooser_still_holds_exactly_one_color() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let blue_bear = scenario
        .add_creature(P0, "Blue Bear", 2, 2)
        .with_mana_cost(one_color(ManaCostShard::Blue))
        .id();
    let red_bear = scenario
        .add_creature(P0, "Red Bear", 2, 2)
        .with_mana_cost(one_color(ManaCostShard::Red))
        .id();

    let hall = scenario
        .add_artifact_to_hand_from_oracle(P0, "Hall of Triumph", HALL_OF_TRIUMPH)
        .with_mana_cost(ManaCost::zero())
        .as_legendary()
        .id();

    let mut runner = scenario.build();
    let outcome = runner.cast(hall).choose_option("Blue").resolve();

    // POSITIVE REACH-GUARD (a): the artifact entered.
    outcome.assert_zone(&[hall], Zone::Battlefield);
    // POSITIVE REACH-GUARD (b): the dependent static really applies, so the
    // choice was read back and the assertion below is not about a dead value.
    assert_eq!(
        runner.state().objects[&blue_bear].power,
        Some(3),
        "the chosen-colour anthem must apply to the blue creature"
    );
    assert_eq!(
        runner.state().objects[&red_bear].power,
        Some(2),
        "the anthem must not apply to another colour"
    );

    // THE INVARIANT: exactly one chosen colour on the source.
    assert_eq!(
        chosen_colors(&runner, hall),
        vec![ManaColor::Blue],
        "an as-enters chooser holds exactly one colour"
    );
}

// ---------------------------------------------------------------------------
// T0b — the CR 607.2d linked colour choice, at runtime.
// ---------------------------------------------------------------------------

/// T0b (RUNTIME) — CR 607.2d. Floating Shield's "Sacrifice this Aura:" grant
/// reads the colour chosen by its LINKED as-enters replacement, and makes no
/// choice of its own.
///
/// This is the card the replace-on-rechoose fix would otherwise break. Before
/// the `LinkedColorChoice` relation the injector handed the sacrifice ability a
/// chooser, so the Aura accumulated a SECOND answer; today's first-match read
/// accidentally preserved the as-enters one, and U1 alone would have flipped it
/// to the spurious second.
///
/// THE NEGATIVE THAT MATTERS, and the assertion that flips if U0 is reverted:
/// the activation is resolved with NO colour declared and must run to
/// completion. `drive_resolution` halts AT any `NamedChoice` window it has no
/// answer for, so a second prompt would leave the run parked there.
#[test]
fn floating_shield_sacrifice_grant_reads_the_as_enters_color() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let host = scenario.add_creature(P0, "Host Bear", 2, 2).id();
    let recipient = scenario.add_creature(P0, "Recipient Bear", 2, 2).id();

    let shield = scenario
        .add_enchantment_from_oracle(P0, "Floating Shield", FLOATING_SHIELD)
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::White],
            generic: 2,
        })
        .id();

    let mut runner = scenario.build();
    // Attach the Aura and record the as-enters answer, the state the replacement
    // produces on entry (the same construction `issue_6499_flickering_ward_…`
    // uses for this card class).
    {
        let state = runner.state_mut();
        let aura = state.objects.get_mut(&shield).expect("aura exists");
        aura.attached_to = Some(host.into());
        aura.chosen_attributes
            .push(ChosenAttribute::Color(ManaColor::Blue));
        state
            .objects
            .get_mut(&host)
            .expect("host exists")
            .attachments
            .push(shield);
        state.layers_dirty.mark_full();
    }
    engine::game::layers::evaluate_layers(runner.state_mut());

    // POSITIVE REACH-GUARD (a): the linked STATIC reads the same choice, so the
    // suppression did not strand the enchanted creature's protection.
    assert!(
        has_protection_from(&runner.state().objects[&host], ManaColor::Blue),
        "the enchanted creature must have protection from the as-enters colour: {:?}",
        runner.state().objects[&host].keywords
    );

    // Resolve the activation with NO colour declared.
    let outcome = runner
        .activate(shield, 0)
        .target_object(recipient)
        .resolve();

    // THE ASSERTION THAT FLIPS if U0 is reverted: no second prompt was raised.
    assert!(
        matches!(outcome.final_waiting_for(), WaitingFor::Priority { .. }),
        "the linked grant must raise NO colour prompt of its own, got {:?}",
        outcome.final_waiting_for()
    );
    // POSITIVE REACH-GUARD (b): the sacrifice cost was actually paid
    // (CR 400.7j — the effect still finds the source in its public zone).
    outcome.assert_zone(&[shield], Zone::Graveyard);
    // THE POSITIVE DELTA: the target really gained protection, from the
    // as-enters colour. With U1 and without U0 the Aura would hold the
    // activation's own second answer instead.
    assert!(
        has_protection_from(&runner.state().objects[&recipient], ManaColor::Blue),
        "the sacrifice grant must bake the as-enters colour: {:?}",
        runner.state().objects[&recipient].keywords
    );
    assert_eq!(
        chosen_colors(&runner, shield),
        vec![ManaColor::Blue],
        "the Aura must still hold exactly its as-enters answer"
    );
}
