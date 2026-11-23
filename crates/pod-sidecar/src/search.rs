//! A genetic search over strategies (rule SYN-3). The search runs against
//! the local [`Simulator`] alone: it never opens a socket and never touches
//! a live path.
//!
//! A genome is an ordered list of steps, at most four. The initial
//! population holds every one-step strategy and then random genomes. Each
//! generation keeps the best half and breeds the rest by crossover and
//! mutation. The fitness of [`Search::find`] is the simulated disagreement
//! with the fewest changes; [`Search::towards`] scores a run against a
//! hand-written strategy, so a test can require convergence on it.

use std::fmt;

use crate::primitive::{Date, Params, Primitive, Step, Strategy, Trigger};
use crate::rng::Rng;
use crate::simulator::{Divergence, Run, Simulator};

/// The most steps of one genome.
pub const MAX_STEPS: usize = 4;

/// A failure of the search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchError {
    /// The target strategy changed nothing, so there is no disagreement to
    /// reproduce.
    TargetSilent,
    /// The population or the generation count is zero.
    BadSettings,
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::TargetSilent => f.write_str("the target produced no divergence"),
            SearchError::BadSettings => f.write_str("the population and the generations must be at least 1"),
        }
    }
}

impl std::error::Error for SearchError {}

/// The result of a search.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub best: Strategy,
    pub run: Run,
    pub score: i64,
    /// The generation where the best score first appeared.
    pub generation: usize,
    /// The best score of each generation.
    pub history: Vec<i64>,
}

/// The search.
#[derive(Clone, Debug)]
pub struct Search {
    seed: u64,
    population: usize,
    generations: usize,
    date: Date,
    network: String,
    found_by: String,
}

impl Search {
    pub fn new(seed: u64, date: Date, network: impl Into<String>) -> Search {
        Search {
            seed,
            population: 32,
            generations: 40,
            date,
            network: network.into(),
            found_by: "genetic search".into(),
        }
    }

    pub fn population(mut self, population: usize) -> Search {
        self.population = population;
        self
    }

    pub fn generations(mut self, generations: usize) -> Search {
        self.generations = generations;
        self
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The best disagreement with the fewest steps.
    pub fn find(&self) -> Result<Outcome, SearchError> {
        self.search(|run, steps| score_find(&run.divergence, steps))
    }

    /// The fewest steps that reproduce the divergence of `target`.
    pub fn towards(&self, target: &Strategy) -> Result<Outcome, SearchError> {
        let sim = Simulator::new(self.seed);
        let target_run = sim.run(target);
        if target_run.divergence.is_empty() {
            return Err(SearchError::TargetSilent);
        }
        self.search(|run, steps| score_towards(&run.divergence, &target_run.divergence, steps))
    }

    fn search(&self, score: impl Fn(&Run, usize) -> i64) -> Result<Outcome, SearchError> {
        if self.population == 0 || self.generations == 0 {
            return Err(SearchError::BadSettings);
        }
        let sim = Simulator::new(self.seed);
        let mut rng = Rng::new(self.seed ^ 0x5eed);
        let mut population = self.initial(&mut rng);
        let mut best: Option<Outcome> = None;
        let mut history = Vec::with_capacity(self.generations);
        for generation in 0..self.generations {
            let mut evaluated: Vec<(i64, Vec<Step>, Run)> = population
                .iter()
                .map(|genome| {
                    let strategy = self.strategy(genome);
                    let run = sim.run(&strategy);
                    (score(&run, genome.len()), genome.clone(), run)
                })
                .collect();
            evaluated.sort_by_key(|a| std::cmp::Reverse(a.0));
            let (top, genome, run) = evaluated.first().cloned().ok_or(SearchError::BadSettings)?;
            history.push(top);
            let better = best.as_ref().is_none_or(|outcome| top > outcome.score);
            if better {
                best = Some(Outcome { best: self.strategy(&genome), run, score: top, generation, history: Vec::new() });
            }
            if generation + 1 == self.generations {
                break;
            }
            population = self.breed(&evaluated, &mut rng);
        }
        let mut outcome = best.ok_or(SearchError::BadSettings)?;
        outcome.history = history;
        Ok(outcome)
    }

    fn initial(&self, rng: &mut Rng) -> Vec<Vec<Step>> {
        let mut population = Vec::with_capacity(self.population);
        'outer: for primitive in Primitive::ALL {
            for trigger in Trigger::ALL {
                population.push(vec![Step::new(trigger, primitive)]);
                if population.len() == self.population {
                    break 'outer;
                }
            }
        }
        while population.len() < self.population {
            let length = 1 + rng.below(3);
            population.push((0..length).map(|_| random_step(rng)).collect());
        }
        population
    }

    fn breed(&self, evaluated: &[(i64, Vec<Step>, Run)], rng: &mut Rng) -> Vec<Vec<Step>> {
        let keep = (self.population / 2).max(1);
        let mut next: Vec<Vec<Step>> = evaluated.iter().take(keep).map(|(_, genome, _)| genome.clone()).collect();
        while next.len() < self.population {
            let a = &evaluated[rng.below(keep)].1;
            let b = &evaluated[rng.below(keep)].1;
            let mut child = crossover(a, b, rng);
            if rng.chance(3, 4) {
                mutate(&mut child, rng);
            }
            next.push(child);
        }
        next
    }

    fn strategy(&self, genome: &[Step]) -> Strategy {
        Strategy::new(self.date, self.network.clone(), self.found_by.clone(), genome.to_vec())
    }
}

fn random_step(rng: &mut Rng) -> Step {
    let primitive = Primitive::ALL[rng.below(Primitive::ALL.len())];
    let trigger = Trigger::ALL[rng.below(Trigger::ALL.len())];
    Step { trigger, primitive, params: random_params(rng) }
}

fn random_params(rng: &mut Rng) -> Params {
    Params { offset: Some(rng.below(16)), byte: Some(rng.byte()), count: Some(1 + rng.below(3) as u32) }
}

fn crossover(a: &[Step], b: &[Step], rng: &mut Rng) -> Vec<Step> {
    let left = 1 + rng.below(a.len());
    let right = 1 + rng.below(b.len());
    let mut child = a[..left].to_vec();
    child.extend_from_slice(&b[right..]);
    child.truncate(MAX_STEPS);
    child
}

fn mutate(genome: &mut Vec<Step>, rng: &mut Rng) {
    if genome.is_empty() {
        genome.push(random_step(rng));
        return;
    }
    match rng.below(6) {
        0 => {
            let index = rng.below(genome.len());
            genome[index].trigger = Trigger::ALL[rng.below(Trigger::ALL.len())];
        }
        1 => {
            let index = rng.below(genome.len());
            genome[index].primitive = Primitive::ALL[rng.below(Primitive::ALL.len())];
        }
        2 => {
            let index = rng.below(genome.len());
            genome[index].params.offset = Some(rng.below(16));
        }
        3 => {
            let index = rng.below(genome.len());
            genome[index].params.byte = Some(rng.byte());
        }
        4 => {
            if genome.len() < MAX_STEPS {
                genome.push(random_step(rng));
            } else {
                genome.remove(rng.below(genome.len()));
            }
        }
        _ => {
            let index = rng.below(genome.len());
            genome[index].params.count = Some(1 + rng.below(3) as u32);
        }
    }
    if genome.is_empty() {
        genome.push(random_step(rng));
    }
}

fn score_find(divergence: &Divergence, steps: usize) -> i64 {
    divergence.magnitude() - steps as i64 * 5
}

fn score_towards(got: &Divergence, want: &Divergence, steps: usize) -> i64 {
    let mut score = 0;
    if got.states == want.states {
        score += 1000;
    } else {
        score -= 100;
    }
    if got.bytes == want.bytes {
        score += 600;
    } else if got.bytes.len() == want.bytes.len()
        && got.bytes.iter().zip(&want.bytes).all(|(a, b)| a.side == b.side && a.offset == b.offset)
    {
        score += 300;
    }
    if got.segments == want.segments {
        score += 300;
    }
    if got.is_empty() {
        score -= 1000;
    }
    score - steps as i64 * 20
}
