//! Minimal standard-library command-line parsing for the application's scene modes.

use std::{error::Error, fmt};

/// Seed of the procedural island when `--procedural-island` is given without `--seed`. Chosen
/// because its initial view shows coal, iron, and diamond ore on the island's lit rock face.
pub const DEFAULT_ISLAND_SEED: u64 = 42;

const VOXEL_DIAGNOSTIC_FLAG: &str = "--voxel-diagnostic";
const PROCEDURAL_ISLAND_FLAG: &str = "--procedural-island";
const SEED_FLAG: &str = "--seed";

pub const USAGE: &str = "usage: Minecraft_Diorama [--voxel-diagnostic | --procedural-island \
                         [--seed <u64>] | --seed <u64>]";

/// Which scene the application renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneMode {
    /// The default five-block AABB showcase (no arguments).
    Showcase,
    /// `--voxel-diagnostic`: the small hybrid voxel diagnostic scene.
    VoxelDiagnostic,
    /// `--procedural-island`, `--seed <u64>`, or both: one generated floating island.
    ProceduralIsland { seed: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliError {
    /// `--seed` is the last argument or is followed by another option.
    MissingSeedValue,
    /// The seed value starts with `-`: seeds are unsigned.
    NegativeSeed(String),
    /// The seed value is not a plain decimal number.
    InvalidSeed(String),
    /// The seed value is decimal but larger than `u64::MAX`.
    SeedOutOfRange(String),
    /// The same option appears more than once.
    Repeated(String),
    /// `--voxel-diagnostic` combined with the procedural island options.
    ConflictingModes,
    UnknownArgument(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSeedValue => write!(formatter, "{SEED_FLAG} requires a value"),
            Self::NegativeSeed(value) => {
                write!(formatter, "seed `{value}` is negative; seeds are unsigned")
            }
            Self::InvalidSeed(value) => {
                write!(
                    formatter,
                    "seed `{value}` is not an unsigned decimal integer"
                )
            }
            Self::SeedOutOfRange(value) => {
                write!(formatter, "seed `{value}` exceeds the maximum {}", u64::MAX)
            }
            Self::Repeated(option) => write!(formatter, "{option} was given more than once"),
            Self::ConflictingModes => write!(
                formatter,
                "{VOXEL_DIAGNOSTIC_FLAG} cannot be combined with {PROCEDURAL_ISLAND_FLAG} or \
                 {SEED_FLAG}"
            ),
            Self::UnknownArgument(argument) => write!(formatter, "unknown argument `{argument}`"),
        }
    }
}

impl Error for CliError {}

/// Parses the arguments after the program name.
///
/// No arguments selects the showcase. `--seed` alone implies `--procedural-island`. Every
/// malformed or unknown argument is an error: an explicitly supplied seed is never silently
/// replaced by the default.
pub fn parse_args<I>(arguments: I) -> Result<SceneMode, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut voxel_diagnostic = false;
    let mut procedural_island = false;
    let mut seed = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            VOXEL_DIAGNOSTIC_FLAG => set_once(&mut voxel_diagnostic, &argument)?,
            PROCEDURAL_ISLAND_FLAG => set_once(&mut procedural_island, &argument)?,
            SEED_FLAG => {
                if seed.is_some() {
                    return Err(CliError::Repeated(argument));
                }
                let value = arguments
                    .next()
                    .filter(|value| !value.starts_with("--"))
                    .ok_or(CliError::MissingSeedValue)?;
                seed = Some(parse_seed(value)?);
            }
            _ => return Err(CliError::UnknownArgument(argument)),
        }
    }

    match (voxel_diagnostic, procedural_island || seed.is_some()) {
        (true, true) => Err(CliError::ConflictingModes),
        (true, false) => Ok(SceneMode::VoxelDiagnostic),
        (false, true) => Ok(SceneMode::ProceduralIsland {
            seed: seed.unwrap_or(DEFAULT_ISLAND_SEED),
        }),
        (false, false) => Ok(SceneMode::Showcase),
    }
}

fn set_once(flag: &mut bool, argument: &str) -> Result<(), CliError> {
    if *flag {
        return Err(CliError::Repeated(argument.to_owned()));
    }
    *flag = true;
    Ok(())
}

/// Accepts ASCII decimal digits only: no sign, whitespace, or radix prefix.
fn parse_seed(value: String) -> Result<u64, CliError> {
    if value.starts_with('-') {
        return Err(CliError::NegativeSeed(value));
    }
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CliError::InvalidSeed(value));
    }
    // Digits only, so the one remaining failure is a value above `u64::MAX`.
    value.parse().map_err(|_| CliError::SeedOutOfRange(value))
}

#[cfg(test)]
mod tests {
    use super::{CliError, DEFAULT_ISLAND_SEED, SceneMode, parse_args};

    fn parse(arguments: &[&str]) -> Result<SceneMode, CliError> {
        parse_args(arguments.iter().map(|argument| argument.to_string()))
    }

    fn island(seed: u64) -> Result<SceneMode, CliError> {
        Ok(SceneMode::ProceduralIsland { seed })
    }

    #[test]
    fn existing_modes_are_preserved() {
        assert_eq!(parse(&[]), Ok(SceneMode::Showcase));
        assert_eq!(
            parse(&["--voxel-diagnostic"]),
            Ok(SceneMode::VoxelDiagnostic)
        );
    }

    #[test]
    fn procedural_island_uses_the_documented_default_seed() {
        assert_eq!(parse(&["--procedural-island"]), island(DEFAULT_ISLAND_SEED));
    }

    #[test]
    fn explicit_seeds_are_parsed_exactly() {
        assert_eq!(
            parse(&["--procedural-island", "--seed", "1847"]),
            island(1847)
        );
        assert_eq!(
            parse(&["--seed", "1847", "--procedural-island"]),
            island(1847)
        );
        assert_eq!(parse(&["--seed", "1847"]), island(1847));
        assert_eq!(parse(&["--seed", "0"]), island(0));
        assert_eq!(parse(&["--seed", "007"]), island(7));
        assert_eq!(parse(&["--seed", "18446744073709551615"]), island(u64::MAX));
    }

    #[test]
    fn missing_seed_value_is_rejected() {
        assert_eq!(parse(&["--seed"]), Err(CliError::MissingSeedValue));
        assert_eq!(
            parse(&["--procedural-island", "--seed"]),
            Err(CliError::MissingSeedValue)
        );
        assert_eq!(
            parse(&["--seed", "--procedural-island"]),
            Err(CliError::MissingSeedValue)
        );
    }

    #[test]
    fn malformed_seeds_are_rejected_without_a_fallback() {
        assert_eq!(
            parse(&["--seed", "-5"]),
            Err(CliError::NegativeSeed("-5".into()))
        );
        assert_eq!(
            parse(&["--seed", "-0"]),
            Err(CliError::NegativeSeed("-0".into()))
        );
        for invalid in [
            "abc", "", "12a", "+5", " 5", "5 ", "1e3", "0x10", "1_000", "1.0",
        ] {
            assert_eq!(
                parse(&["--procedural-island", "--seed", invalid]),
                Err(CliError::InvalidSeed(invalid.into())),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn overflowing_seeds_are_rejected() {
        for overflow in ["18446744073709551616", "99999999999999999999999"] {
            assert_eq!(
                parse(&["--seed", overflow]),
                Err(CliError::SeedOutOfRange(overflow.into()))
            );
        }
    }

    #[test]
    fn unknown_and_repeated_arguments_are_rejected() {
        assert_eq!(
            parse(&["--island"]),
            Err(CliError::UnknownArgument("--island".into()))
        );
        assert_eq!(
            parse(&["1847"]),
            Err(CliError::UnknownArgument("1847".into()))
        );
        assert_eq!(
            parse(&["--seed=1847"]),
            Err(CliError::UnknownArgument("--seed=1847".into()))
        );
        assert_eq!(
            parse(&["--seed", "1", "--seed", "2"]),
            Err(CliError::Repeated("--seed".into()))
        );
        assert_eq!(
            parse(&["--voxel-diagnostic", "--voxel-diagnostic"]),
            Err(CliError::Repeated("--voxel-diagnostic".into()))
        );
    }

    #[test]
    fn voxel_diagnostic_does_not_combine_with_the_island() {
        assert_eq!(
            parse(&["--voxel-diagnostic", "--procedural-island"]),
            Err(CliError::ConflictingModes)
        );
        assert_eq!(
            parse(&["--voxel-diagnostic", "--seed", "3"]),
            Err(CliError::ConflictingModes)
        );
    }

    #[test]
    fn errors_have_readable_messages() {
        assert_eq!(
            CliError::NegativeSeed("-5".into()).to_string(),
            "seed `-5` is negative; seeds are unsigned"
        );
        assert!(
            CliError::SeedOutOfRange("18446744073709551616".into())
                .to_string()
                .contains("18446744073709551615")
        );
    }
}
