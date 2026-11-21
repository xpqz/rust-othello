use crate::baselines::{PHASE2_DEPTH, classic_config, phase2_config};
use othello::agents::{Agent, SearchAgent, clock_agent, greedy_agent, random_agent, search_agent};
use othello::search::{
    CLOCK_POLICIES, ClockPolicy, MAX_CLOCK_MS, NO_DEADLINE, ProbCut, SearchConfig, SearchLimits,
};

fn unknown(name: &str) -> String {
    format!("unknown agent: {name}")
}

/// A `c<ms>[v<k>]` field: a game clock in 1..=`MAX_CLOCK_MS` and a
/// registered clock policy variant, 0 by default.
fn clock_field(text: &str) -> Option<(u64, usize)> {
    let clock = text.strip_prefix('c')?;
    let (ms, k) = clock.split_once('v').unwrap_or((clock, "0"));
    let ms = ms
        .parse()
        .ok()
        .filter(|ms| (1..=MAX_CLOCK_MS).contains(ms))?;
    Some((
        ms,
        k.parse()
            .ok()
            .filter(|&k| ClockPolicy::variant(k).is_some())?,
    ))
}

/// A time field: per-move ms, or a clock field.
fn time_field(name: &str, text: &str) -> Result<u64, String> {
    if text.starts_with('c') {
        clock_field(text)
            .map(|(ms, _)| ms)
            .ok_or_else(|| unknown(name))
    } else {
        text.parse().map_err(|_| unknown(name))
    }
}

pub fn parse_name(name: &str) -> Result<(&str, u32, Option<u64>), String> {
    let mut parts = name.split(':');
    let kind = parts.next().unwrap();
    let time =
        |parts: &mut std::str::Split<char>| parts.next().map(|t| time_field(name, t)).transpose();
    let (depth, time) = match kind {
        "random" | "greedy" => (0, None),
        "phase2" => (PHASE2_DEPTH, time(&mut parts)?),
        "classic" => {
            let mut depth = 64;
            let mut time = None;
            if let Some(field) = parts.next() {
                if let Some(value) = field.strip_prefix('d') {
                    depth = value.parse().map_err(|_| unknown(name))?;
                    if depth == 0 || depth > 64 {
                        return Err(unknown(name));
                    }
                } else {
                    time = Some(time_field(name, field)?);
                }
            }
            (depth, time)
        }
        "pc" => {
            let t = parts.next().ok_or_else(|| unknown(name))?;
            if t != "off" && !t.parse().is_ok_and(|t: u32| (50..=400).contains(&t)) {
                return Err(unknown(name));
            }
            (64, time(&mut parts)?)
        }
        "ab" => {
            let depth = parts
                .next()
                .ok_or_else(|| unknown(name))?
                .parse()
                .map_err(|_| unknown(name))?;
            if depth == 0 || depth > 10 {
                return Err(unknown(name));
            }
            (depth, time(&mut parts)?)
        }
        _ => return Err(unknown(name)),
    };
    if parts.next().is_some() {
        return Err(unknown(name));
    }
    Ok((kind, depth, time))
}

/// The game clock in ms of a `c<ms>` time field, if the name has one.
pub fn clock(name: &str) -> Result<Option<u64>, String> {
    Ok(clock_spec(name)?.map(|(ms, _)| ms))
}

fn clock_spec(name: &str) -> Result<Option<(u64, usize)>, String> {
    parse_name(name)?;
    Ok(name.split(':').skip(1).last().and_then(clock_field))
}

/// The clock policy variant of a `c<ms>v<k>` field; 0 without one.
pub fn policy(name: &str) -> Result<usize, String> {
    Ok(clock_spec(name)?.map_or(0, |(_, k)| k))
}

pub fn effective(depth: u32, time: Option<u64>, nodes: u64) -> Result<(u32, u64), String> {
    if nodes == 0 && time.is_none() {
        return Ok((depth, NO_DEADLINE));
    }
    if nodes > 0 && time.is_some() {
        return Err("time control conflicts with --nodes".into());
    }
    Ok((64, time.unwrap_or(NO_DEADLINE)))
}

pub fn config(
    name: &str,
    nodes: u64,
    ordering: bool,
) -> Result<(SearchConfig, SearchLimits), String> {
    let (kind, depth, time) = parse_name(name)?;
    let (depth, time) = effective(depth, time, nodes)?;
    let limits = SearchLimits::new(depth, time, nodes);
    // `pc:<t>`: the fitted model with margin `t`; `pc:off`: none.
    let margin = name.split(':').nth(1).and_then(|t| t.parse().ok());
    let base = match kind {
        "phase2" => phase2_config(),
        "classic" => classic_config(),
        "pc" => SearchConfig {
            probcut: margin.map(|t| ProbCut {
                t,
                ..ProbCut::FITTED
            }),
            ..SearchConfig::standard()
        },
        _ => SearchConfig::standard(),
    };
    let config = SearchConfig { ordering, ..base };
    Ok((config, limits))
}

/// The search agent for a search kind's name: a `clock_agent` when the name
/// has a `c<ms>` field.
pub fn search(name: &str, nodes: u64, ordering: bool) -> Result<SearchAgent, String> {
    let (config, limits) = config(name, nodes, ordering)?;
    Ok(match clock(name)? {
        Some(_) => clock_agent(config, limits, &CLOCK_POLICIES[policy(name)?]),
        None => search_agent(config, limits),
    })
}

pub fn build(name: &str, seed: u64, nodes: u64, ordering: bool) -> Result<Box<dyn Agent>, String> {
    let (kind, _, _) = parse_name(name)?;
    match kind {
        "random" => Ok(Box::new(random_agent(seed))),
        "greedy" => Ok(Box::new(greedy_agent())),
        _ => Ok(Box::new(search(name, nodes, ordering)?)),
    }
}
