//! The genetic search converges on a hand-written strategy, with a fixed
//! seed and a bounded generation count.

use pod_sidecar::{Date, Primitive, Search, SearchError, Simulator, Step, Strategy, Trigger};

fn date() -> Date {
    Date::parse("2026-11-12").unwrap()
}

fn target() -> Strategy {
    Strategy::new(date(), "lab-a", "hand", vec![Step::new(Trigger::FirstOutboundData, Primitive::Drop)])
}

#[test]
fn the_search_converges_on_a_hand_written_strategy() {
    let target = target();
    let seed = 7;
    let target_run = Simulator::new(seed).run(&target);
    assert!(!target_run.divergence.is_empty());
    let search = Search::new(seed, date(), "lab-a").population(32).generations(40);
    let outcome = search.towards(&target).unwrap();
    assert_eq!(outcome.run.divergence, target_run.divergence, "{}", outcome.run.divergence);
    assert!(outcome.best.steps.len() <= target.steps.len());
    assert!(outcome.generation < 40);
    assert!(outcome.history.len() <= 40);
    assert_eq!(outcome.history.last(), Some(&outcome.history.iter().copied().max().unwrap()));
}

#[test]
fn the_same_seed_gives_the_same_search() {
    let target = target();
    let one = Search::new(11, date(), "lab-a").population(24).generations(20).towards(&target).unwrap();
    let two = Search::new(11, date(), "lab-a").population(24).generations(20).towards(&target).unwrap();
    assert_eq!(one.best, two.best);
    assert_eq!(one.score, two.score);
    assert_eq!(one.run.wire(), two.run.wire());
}

#[test]
fn the_generation_count_bounds_the_search() {
    let target = target();
    let outcome = Search::new(3, date(), "lab-a").population(20).generations(5).towards(&target).unwrap();
    assert!(outcome.history.len() <= 5);
    assert!(outcome.generation < 5);
}

#[test]
fn a_target_with_no_divergence_is_refused() {
    let silent = Strategy::new(date(), "lab-a", "hand", vec![Step::new(Trigger::FirstInboundRst, Primitive::Drop)]);
    assert_eq!(Search::new(1, date(), "lab-a").towards(&silent).unwrap_err(), SearchError::TargetSilent);
}

#[test]
fn the_search_finds_a_disagreement_with_no_target() {
    let outcome = Search::new(5, date(), "lab-a").population(24).generations(8).find().unwrap();
    assert!(!outcome.run.divergence.is_empty(), "{}", outcome.run.divergence);
    assert!(outcome.best.steps.len() <= 2, "{}", outcome.best.steps.len());
    assert_eq!(outcome.best.date, date());
    assert_eq!(outcome.best.found_by, "genetic search");
}

#[test]
fn a_bad_setting_is_refused() {
    assert_eq!(Search::new(1, date(), "lab-a").population(0).towards(&target()).unwrap_err(), SearchError::BadSettings);
    assert_eq!(
        Search::new(1, date(), "lab-a").generations(0).towards(&target()).unwrap_err(),
        SearchError::BadSettings
    );
}
