//! Subject-led "Whenever one or more / two or more <subject> attack" triggers
//! watch the attacking players their subject names — not only the trigger's
//! controller.
//!
//! CR 506.2 + CR 508.1a: the active player is the attacking player and
//! declares only creatures they control, so the controller scope written on the
//! subject ("creatures", "creatures you control", "creatures your opponents
//! control") IS the scope of attacking players the trigger watches. CR 603.2:
//! whenever a game event matches the trigger event, the ability triggers.
//! CR 603.2c: a batched "one or more" trigger fires once per declaration.
//!
//! Cards (verbatim Oracle text) and rulings:
//! - Duelist's Heritage — "Whenever one or more creatures attack, you may have
//!   target attacking creature gain double strike until end of turn." Ruling:
//!   triggers whenever any player attacks with one or more creatures, not just
//!   when you do.
//! - Argent Dais — "Whenever two or more creatures attack, put an oil counter on
//!   this artifact." Ruling: triggers whenever two or more creatures attack, not
//!   just when you attack with two or more.
//! - Flummoxed Cyclops — "Whenever two or more creatures your opponents control
//!   attack, this creature can't block this combat."
//!
//! Before the fix the parser left the attacking-player gate at the
//! controller-scoped default for every subject without an attachment clause, so
//! the unscoped cards fired only on their controller's attacks and Flummoxed
//! Cyclops (opponent-scoped subject, controller-scoped gate) never fired.

use engine::game::combat::can_block_pair;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

use super::rules::AttackTarget;

const DUELISTS_HERITAGE: &str = "Whenever one or more creatures attack, you may have target attacking creature gain double strike until end of turn.";
const ARGENT_DAIS: &str = "This artifact enters with two oil counters on it.\nWhenever two or more creatures attack, put an oil counter on this artifact.\n{2}, {T}, Remove two oil counters from this artifact: Exile another target nonland permanent. Its controller draws two cards.";
const FLUMMOXED_CYCLOPS: &str = "Reach\nWhenever two or more creatures your opponents control attack, this creature can't block this combat.";
const CONTROL_SCOPED_TWO_OR_MORE: &str =
    "Whenever two or more creatures you control attack, draw a card.";

const P2: PlayerId = PlayerId(2);

/// Drive the declared-attackers trigger window until the stack is empty.
/// Answers trigger ordering with identity, trigger targeting with `target`, and
/// every optional-effect prompt with `accept`. Returns how many optional-effect
/// prompts were answered (a reach-guard for decline paths).
fn drive_attack_triggers(runner: &mut GameRunner, target: Option<ObjectId>, accept: bool) -> usize {
    let mut optional_prompts = 0;
    for _ in 0..60 {
        let action = match &runner.state().waiting_for {
            WaitingFor::OrderTriggers { triggers, .. } => GameAction::OrderTriggers {
                order: (0..triggers.len()).collect(),
            },
            WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                GameAction::ChooseTarget {
                    target: Some(TargetRef::Object(
                        target.expect("a targeted trigger needs a chosen target"),
                    )),
                }
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                optional_prompts += 1;
                GameAction::DecideOptionalEffect { accept }
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    return optional_prompts;
                }
                GameAction::PassPriority
            }
            other => panic!("unexpected window: {other:?}"),
        };
        runner.act(action).expect("drive attack-trigger window");
    }
    panic!("the stack did not empty within the window budget");
}

/// CR 506.2: hand the turn to `attacker` in a 3-player game and pass priority
/// until the declare-attackers step (priority advances only after every player
/// passes, so `pass_both_players` is not enough with three players).
fn hand_turn_to(runner: &mut GameRunner, attacker: PlayerId) {
    runner.state_mut().active_player = attacker;
    runner.state_mut().priority_player = attacker;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: attacker };

    for _ in 0..16 {
        if runner.waiting_for_kind() == "DeclareAttackers" {
            return;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("priority pass should advance toward declare attackers");
    }
    panic!("expected DeclareAttackers");
}

/// Two-player hand-off: make `attacker` the active player and move from
/// precombat main to the declare-attackers step.
fn reach_declare_attackers(runner: &mut GameRunner, attacker: PlayerId) {
    runner.state_mut().active_player = attacker;
    runner.pass_both_players();
}

fn declare(runner: &mut GameRunner, attackers: &[ObjectId], defender: PlayerId) {
    let attacks: Vec<_> = attackers
        .iter()
        .map(|&id| (id, AttackTarget::Player(defender)))
        .collect();
    runner
        .declare_attackers(&attacks)
        .expect("attackers are declared");
}

/// Number of creatures declared as attackers this combat (reach-guard proving
/// the declaration event happened).
fn declared_attackers(runner: &GameRunner) -> usize {
    runner
        .state()
        .combat
        .as_ref()
        .map_or(0, |combat| combat.attackers.len())
}

fn has_double_strike(runner: &GameRunner, id: ObjectId) -> bool {
    runner.state().objects[&id].has_keyword(&Keyword::DoubleStrike)
}

fn oil_counters(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Generic("oil".to_string()))
        .copied()
        .unwrap_or(0)
}

fn hand_size(runner: &GameRunner, player: PlayerId) -> usize {
    runner
        .state()
        .players
        .iter()
        .find(|p| p.id == player)
        .map_or(0, |p| p.hand.len())
}

// --- Duelist's Heritage (unscoped, one or more) ---

/// R1: an opponent attacks; P0's Duelist's Heritage triggers and grants double
/// strike to the opponent's attacker (any attacking creature is a legal target).
#[test]
fn duelists_heritage_fires_on_opponent_attack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_enchantment_from_oracle(P0, "Duelist's Heritage", DUELISTS_HERITAGE);
    let opp_attacker = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();

    reach_declare_attackers(&mut runner, P1);
    declare(&mut runner, &[opp_attacker], P0);
    assert!(!has_double_strike(&runner, opp_attacker));
    drive_attack_triggers(&mut runner, Some(opp_attacker), true);

    // Revert-failing: at BASE the controller-scoped gate rejects P1's attack.
    assert!(
        has_double_strike(&runner, opp_attacker),
        "Duelist's Heritage must trigger on an opponent's attack (CR 603.2)"
    );
}

/// R1-decline: the trigger reaches its optional prompt; declining grants nothing.
#[test]
fn duelists_heritage_decline_grants_nothing() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_enchantment_from_oracle(P0, "Duelist's Heritage", DUELISTS_HERITAGE);
    let opp_attacker = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();

    reach_declare_attackers(&mut runner, P1);
    declare(&mut runner, &[opp_attacker], P0);
    let prompts = drive_attack_triggers(&mut runner, Some(opp_attacker), false);

    // Reach-guard: the trigger fired and offered its optional effect.
    assert!(prompts >= 1, "the optional double-strike grant was offered");
    assert!(!has_double_strike(&runner, opp_attacker));
}

/// R1-own: the controller's own attack still triggers.
#[test]
fn duelists_heritage_fires_on_own_attack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_enchantment_from_oracle(P0, "Duelist's Heritage", DUELISTS_HERITAGE);
    let own_attacker = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();

    reach_declare_attackers(&mut runner, P0);
    declare(&mut runner, &[own_attacker], P1);
    drive_attack_triggers(&mut runner, Some(own_attacker), true);

    assert!(has_double_strike(&runner, own_attacker));
}

/// R1-3p: the attacking player (P1), the defending player (P2), and the source
/// controller (P0) are all distinct — the trigger still fires.
#[test]
fn duelists_heritage_fires_when_third_party_attacks() {
    let mut scenario = GameScenario::new_n_player(3, 42);
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_enchantment_from_oracle(P0, "Duelist's Heritage", DUELISTS_HERITAGE);
    let attacker = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();

    hand_turn_to(&mut runner, P1);
    declare(&mut runner, &[attacker], P2);
    drive_attack_triggers(&mut runner, Some(attacker), true);

    assert!(
        has_double_strike(&runner, attacker),
        "Duelist's Heritage must trigger when P1 attacks P2"
    );
}

// --- Argent Dais (unscoped, two or more) ---

/// R2: an opponent attacks with two creatures → one oil counter.
#[test]
fn argent_dais_counts_opponent_attack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dais = scenario
        .add_artifact_from_oracle(P0, "Argent Dais", ARGENT_DAIS)
        .id();
    let a = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let b = scenario.add_creature(P1, "Runeclaw Bear", 2, 2).id();
    let mut runner = scenario.build();
    let before = oil_counters(&runner, dais);

    reach_declare_attackers(&mut runner, P1);
    declare(&mut runner, &[a, b], P0);
    drive_attack_triggers(&mut runner, None, true);

    // Revert-failing: at BASE the gate rejects P1's attack and the count reads P0.
    assert_eq!(oil_counters(&runner, dais), before + 1);
}

/// R2-one: a single opponent attacker does not meet "two or more".
#[test]
fn argent_dais_ignores_single_opponent_attacker() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dais = scenario
        .add_artifact_from_oracle(P0, "Argent Dais", ARGENT_DAIS)
        .id();
    let a = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();
    let before = oil_counters(&runner, dais);

    reach_declare_attackers(&mut runner, P1);
    declare(&mut runner, &[a], P0);
    drive_attack_triggers(&mut runner, None, true);

    // Reach-guard: the declaration happened (paired positive: R2).
    assert_eq!(declared_attackers(&runner), 1);
    assert_eq!(oil_counters(&runner, dais), before);
}

/// R2-own: the controller's own two-creature attack still counts.
#[test]
fn argent_dais_counts_own_attack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dais = scenario
        .add_artifact_from_oracle(P0, "Argent Dais", ARGENT_DAIS)
        .id();
    let a = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let b = scenario.add_creature(P0, "Runeclaw Bear", 2, 2).id();
    let mut runner = scenario.build();
    let before = oil_counters(&runner, dais);

    reach_declare_attackers(&mut runner, P0);
    declare(&mut runner, &[a, b], P1);
    drive_attack_triggers(&mut runner, None, true);

    assert_eq!(oil_counters(&runner, dais), before + 1);
}

// --- Flummoxed Cyclops (opponent-scoped, two or more) ---

/// R3: two opponent attackers → Cyclops can't block this combat.
#[test]
fn flummoxed_cyclops_cant_block_after_two_opponent_attackers() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let cyclops = scenario
        .add_creature_from_oracle(P0, "Flummoxed Cyclops", 4, 4, FLUMMOXED_CYCLOPS)
        .id();
    let a = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let b = scenario.add_creature(P1, "Runeclaw Bear", 2, 2).id();
    let mut runner = scenario.build();

    reach_declare_attackers(&mut runner, P1);
    declare(&mut runner, &[a, b], P0);
    drive_attack_triggers(&mut runner, None, true);

    // Revert-failing: at BASE the trigger never fires (gate scoped to P0, count
    // reads P0's attackers), so Cyclops could still block.
    assert!(
        !can_block_pair(runner.state(), cyclops, a),
        "Flummoxed Cyclops can't block after two opponent creatures attack"
    );
}

/// R3-one: a single opponent attacker does not meet "two or more" — Cyclops
/// can still block it (positive reach-guard on the same blocking check).
#[test]
fn flummoxed_cyclops_can_block_single_opponent_attacker() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let cyclops = scenario
        .add_creature_from_oracle(P0, "Flummoxed Cyclops", 4, 4, FLUMMOXED_CYCLOPS)
        .id();
    let a = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();

    reach_declare_attackers(&mut runner, P1);
    declare(&mut runner, &[a], P0);
    drive_attack_triggers(&mut runner, None, true);

    assert_eq!(declared_attackers(&runner), 1);
    assert!(can_block_pair(runner.state(), cyclops, a));
}

// --- "you control" subject (preservation) ---

/// R4: "creatures you control" stays controller-scoped — an opponent's
/// two-creature attack does not draw.
///
/// Sibling negative, NOT revert-failing: at BASE this also stays silent
/// (and `valid_card`'s "you control" would reject the opponent's attackers
/// regardless of the gate). The real "you control" regression guards are the
/// parser preservation tests and R4+.
#[test]
fn control_scoped_two_or_more_silent_on_opponent_attack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Mountain"]);
    scenario.add_creature_from_oracle(P0, "Test Warleader", 2, 2, CONTROL_SCOPED_TWO_OR_MORE);
    let a = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let b = scenario.add_creature(P1, "Runeclaw Bear", 2, 2).id();
    let mut runner = scenario.build();
    let before = hand_size(&runner, P0);

    reach_declare_attackers(&mut runner, P1);
    declare(&mut runner, &[a, b], P0);
    drive_attack_triggers(&mut runner, None, true);

    // Reach-guard: the two-creature declaration happened (paired positive: R4+).
    assert_eq!(declared_attackers(&runner), 2);
    assert_eq!(hand_size(&runner, P0), before);
}

/// R4+: the controller's own two-creature attack draws a card.
#[test]
fn control_scoped_two_or_more_fires_on_own_attack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Mountain"]);
    let a = scenario
        .add_creature_from_oracle(P0, "Test Warleader", 2, 2, CONTROL_SCOPED_TWO_OR_MORE)
        .id();
    let b = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();
    let before = hand_size(&runner, P0);

    reach_declare_attackers(&mut runner, P0);
    declare(&mut runner, &[a, b], P1);
    drive_attack_triggers(&mut runner, None, true);

    assert_eq!(hand_size(&runner, P0), before + 1);
}
