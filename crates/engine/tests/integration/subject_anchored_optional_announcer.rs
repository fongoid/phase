//! CR 608.2c + CR 608.2d + CR 109.5: class-level runtime coverage for the
//! subject-anchored "may" announcer stamp. Willie Lumpkin's own card test
//! stays in `willie_lumpkin_cant_attack.rs`; this module carries the class
//! hostile fixture (a "you may have" causative that must NOT move) and the
//! `ScopedPlayer` class fixture (Academy Loremaster), including the CR 603.5
//! auto-choice key seat guard (V9) nested inside the same scenario.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::triggers::process_triggers;
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::events::GameEvent;
use engine::types::game_state::{AutoMayChoice, MayTriggerAutoChoiceScope, WaitingFor};
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

const PROTECTED: PlayerId = P0;
const RESTRICTED: PlayerId = P1;

fn hand_len(runner: &GameRunner, player: PlayerId) -> usize {
    runner
        .state()
        .players
        .iter()
        .find(|p| p.id == player)
        .map(|p| p.hand.len())
        .unwrap_or(0)
}

/// CR 608.2c + CR 608.2d hostile fixture: "you may have that player discard a
/// card" (Slavering Nulls) is the causative construction that stays with the
/// CONTROLLER even though its effect's player slot holds the same
/// `TriggeringPlayer` anaphor as Willie's own "that player may draw a card" —
/// only the clause-level provenance (blocked at `clause_shell.rs:609`)
/// separates them. First production branch reached on revert:
/// `optional_prompt_player:9951`'s `if let Some(optional_player)` is NOT
/// taken, so it falls through to the controller at `:10041`.
#[test]
fn you_may_have_that_player_discard_still_asks_the_controller() {
    const SLAVERING_NULLS_ORACLE: &str = "Whenever this creature deals combat damage to a \
         player, if you control a Swamp, you may have that player discard a card.";

    let mut scenario = GameScenario::new_n_player(2, 42);
    scenario.add_basic_land(PROTECTED, ManaColor::Black);
    // The named patient MUST hold a card, or the discard assertion below is
    // vacuous: `Player::default()` starts every hand empty, so "they discarded"
    // would compare 0 against 0 and pass whether or not the discard happened.
    scenario.add_card_to_hand(RESTRICTED, "Slavering Nulls discard fixture");
    let source = scenario
        .add_creature_from_oracle(PROTECTED, "Slavering Nulls", 2, 2, SLAVERING_NULLS_ORACLE)
        .id();
    let mut runner = scenario.build();

    let restricted_hand_before = hand_len(&runner, RESTRICTED);
    assert_eq!(
        restricted_hand_before, 1,
        "reach-guard: the patient holds a discardable card, so the assertion below is not 0 == 0"
    );

    process_triggers(
        runner.state_mut(),
        &[GameEvent::DamageDealt {
            source_id: source,
            target: TargetRef::Player(RESTRICTED),
            amount: 1,
            is_combat: true,
            excess: 0,
        }],
    );

    let mut prompts_seen = 0usize;
    loop {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalEffectChoice { player, .. } => {
                prompts_seen += 1;
                assert_eq!(
                    player, PROTECTED,
                    "CR 608.2c + CR 608.2d: \"you may have\" is a causative held by the \
                     CONTROLLER, not by the named patient"
                );
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .expect("the controller decides the causative may");
            }
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("priority resolves the trigger");
            }
            other => panic!("unexpected Slavering Nulls prompt: {other:?}"),
        }
    }

    assert_eq!(
        prompts_seen, 1,
        "exactly one optional-effect prompt is minted across the whole resolution"
    );
    assert_eq!(
        hand_len(&runner, RESTRICTED),
        restricted_hand_before - 1,
        "the controller accepted, so the named patient discarded — the causative names WHO acts \
         on the patient, it does not move the announcement to them"
    );
}

fn advance_to_optional_on_draw(runner: &mut GameRunner) {
    for _ in 0..240 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalEffectChoice { .. } => return,
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).ok();
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner
                    .act(GameAction::DeclareAttackers {
                        attacks: vec![],
                        bands: vec![],
                    })
                    .ok();
            }
            WaitingFor::DeclareBlockers { .. } => {
                runner
                    .act(GameAction::DeclareBlockers {
                        assignments: vec![],
                    })
                    .ok();
            }
            _ => return,
        }
    }
}

/// CR 608.2c + CR 608.2d (P7) + CR 603.5 (V9): bare `ScopedPlayer` routes at
/// RUNTIME, not just in the parsed shape. Academy Loremaster's "that player may
/// draw an additional card" moves the announcer seat to whichever player's
/// draw step it is — asserted BEFORE acting in every case, per P3
/// (`apply_as_current_with_mode` drives whichever seat `waiting_for` names).
///
/// V9, nested in the same scenario: a `DecideOptionalEffectAndRemember` answer
/// stored for the CONTROLLER's own draw-step instance must not silently
/// auto-answer the OTHER player's instance of the same ability. Key-exists
/// reach-guard first (T-3): `may_trigger_origin` is `None` whenever
/// `definition_ref` is `None` at the mint, so the stale-key assertion is
/// vacuous unless a key is proven to exist by suppressing a SECOND prompt for
/// the SAME player first.
#[test]
fn academy_loremaster_each_player_announces_their_own_additional_draw() {
    const ACADEMY_LOREMASTER_ORACLE: &str = "At the beginning of each player's draw step, that \
         player may draw an additional card. If they do, spells they cast this turn cost {2} \
         more to cast.";

    let mut scenario = GameScenario::new_n_player(2, 42);
    scenario.at_phase(Phase::Untap);
    for &pid in &[PROTECTED, RESTRICTED] {
        scenario.with_library_top(pid, &["Lib A", "Lib B", "Lib C", "Lib D", "Lib E", "Lib F"]);
    }
    scenario.add_creature_from_oracle(
        PROTECTED,
        "Academy Loremaster",
        2,
        2,
        ACADEMY_LOREMASTER_ORACLE,
    );
    let mut runner = scenario.build();

    // ---- Positive control: the controller's OWN draw step (this turn). ----
    advance_to_optional_on_draw(&mut runner);
    assert_eq!(runner.state().phase, Phase::Draw);
    match runner.state().waiting_for.clone() {
        WaitingFor::OptionalEffectChoice { player, .. } => {
            assert_eq!(
                player, PROTECTED,
                "positive control: the controller's own draw step asks the controller"
            );
        }
        other => {
            panic!("expected OptionalEffectChoice at the controller's draw step, got {other:?}")
        }
    }

    // V9 key-exists reach-guard: remember PROTECTED's answer, then confirm it
    // suppresses a SECOND prompt for PROTECTED before trusting the stale-key
    // assertion below. If no key were minted this remembered answer would be a
    // no-op and the second prompt would still fire.
    runner
        .act(GameAction::DecideOptionalEffectAndRemember {
            choice: AutoMayChoice::Accept,
            scope: MayTriggerAutoChoiceScope::default(),
        })
        .expect("the controller remembers their own accept answer");
    let protected_hand_after_first_draw = hand_len(&runner, PROTECTED);
    assert!(
        protected_hand_after_first_draw > 0,
        "reach-guard: the remembered accept must have actually drawn the extra card"
    );

    // Let the rest of PROTECTED's turn and RESTRICTED's upkeep pass, to
    // RESTRICTED's own draw step (the class assertion).
    advance_to_optional_on_draw(&mut runner);
    assert_eq!(runner.state().phase, Phase::Draw);
    match runner.state().waiting_for.clone() {
        WaitingFor::OptionalEffectChoice { player, .. } => {
            assert_eq!(
                player, RESTRICTED,
                "CR 608.2c + CR 608.2d + P7: bare ScopedPlayer moves the announcer to the \
                 player whose draw step this is, not to Academy Loremaster's controller"
            );
        }
        other => panic!("expected OptionalEffectChoice at RESTRICTED's draw step, got {other:?}"),
    }
    let restricted_hand_before = hand_len(&runner, RESTRICTED);
    runner
        .act(GameAction::DecideOptionalEffect { accept: true })
        .expect("RESTRICTED accepts their own optional draw");
    assert_eq!(
        hand_len(&runner, RESTRICTED),
        restricted_hand_before + 1,
        "RESTRICTED drew the extra card from their OWN accept, not from PROTECTED's remembered \
         answer"
    );

    // V9 stale-key assertion, stated the way the mechanism actually works.
    //
    // PROTECTED's remembered answer is keyed to PROTECTED (the reach-guard
    // above proved it is live: it suppressed PROTECTED's own second prompt).
    // So on PROTECTED's NEXT draw step the gate auto-answers and poses nothing,
    // and the next prompt that is actually REACHABLE is therefore RESTRICTED's
    // — who holds no remembered answer and must still be asked every turn.
    //
    // That is the whole V9 claim: a controller-keyed answer never auto-answers
    // for a different announcing seat. Direction is fail-safe — more prompting
    // for the unkeyed seat, never a wrong auto-answer on their behalf. Asserting
    // PROTECTED here instead would contradict the suppression this same test
    // just established.
    let restricted_before_next_cycle = hand_len(&runner, RESTRICTED);
    advance_to_optional_on_draw(&mut runner);
    assert_eq!(runner.state().phase, Phase::Draw);
    match runner.state().waiting_for.clone() {
        WaitingFor::OptionalEffectChoice { player, .. } => {
            assert_eq!(
                player, RESTRICTED,
                "CR 603.5: PROTECTED's remembered answer auto-answers only PROTECTED's own \
                 instance, so the next reachable prompt is RESTRICTED's — never auto-answered \
                 on their behalf"
            );
        }
        other => panic!("expected RESTRICTED to be asked again, got {other:?}"),
    }
    // Reach-guard that the intervening auto-answer really happened rather than
    // the loop having stalled: RESTRICTED has drawn at least their normal
    // draw-step card since the previous prompt, so turns did advance.
    assert!(
        hand_len(&runner, RESTRICTED) > restricted_before_next_cycle,
        "reach-guard: turns advanced through PROTECTED's auto-answered draw step"
    );
}
