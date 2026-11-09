use crate::baselines::{PHASE2_DEPTH, phase2_config};
use othello::agents::{Agent, greedy_agent, random_agent, search_agent};
use othello::search::{NO_DEADLINE, SearchConfig, SearchLimits, probcut_at};

fn unknown(name: &str) -> String {
    format!("unknown agent: {name}")
}

pub fn parse_name(name: &str) -> Result<(&str, u32, Option<u64>), String> {
    let mut parts = name.split(':');
    let kind = parts.next().unwrap();
    let time = |parts: &mut std::str::Split<char>| match parts.next() {
        None => Ok(None),
        Some(text) => text.parse().map(Some).map_err(|_| unknown(name)),
    };
    let (depth, time) = match kind {
        "random" | "greedy" => (0, None),
        "phase2" => (PHASE2_DEPTH, time(&mut parts)?),
        "pc" => {
            let parameter = parts.next().ok_or_else(|| unknown(name))?;
            if parameter != "off" {
                probcut_at(parameter.parse().map_err(|_| unknown(name))?)
                    .map_err(|_| unknown(name))?;
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
    let config = match kind {
        "phase2" => SearchConfig {
            ordering,
            ..phase2_config()
        },
        "pc" => {
            let parameter = name.split(':').nth(1).ok_or_else(|| unknown(name))?;
            let probcut = if parameter == "off" {
                None
            } else {
                Some(probcut_at(parameter.parse().map_err(|_| unknown(name))?)?)
            };
            SearchConfig {
                ordering,
                probcut,
                ..SearchConfig::standard()
            }
        }
        _ => SearchConfig {
            ordering,
            ..SearchConfig::standard()
        },
    };
    Ok((config, limits))
}

pub fn build(name: &str, seed: u64, nodes: u64, ordering: bool) -> Result<Box<dyn Agent>, String> {
    let (kind, _, _) = parse_name(name)?;
    match kind {
        "random" => Ok(Box::new(random_agent(seed))),
        "greedy" => Ok(Box::new(greedy_agent())),
        _ => Ok(Box::new(search_agent(
            config(name, nodes, ordering)?.0,
            config(name, nodes, ordering)?.1,
        ))),
    }
}
