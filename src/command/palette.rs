//! Fuzzy matching for the command palette.
//!
//! The matcher is a subsequence scorer: every character of the query must
//! appear in the candidate in order, and the score rewards matches that a
//! human would consider a better fit. Typing `sov` should find "Step over"
//! ahead of anything that merely contains those letters scattered about.
//!
//! Four things raise a score:
//!
//! - matching at the start of a word, which is how people abbreviate;
//! - matching consecutive characters, so `step` beats `s…t…e…p`;
//! - matching case exactly, breaking ties between otherwise equal candidates;
//! - matching a larger fraction of a short candidate, so an exact hit on a
//!   short name outranks a partial hit on a long one.
//!
//! It is written here rather than pulled in as a dependency because the rules
//! above are the whole requirement, and having them visible means they can be
//! tuned against real command names.

/// A candidate that matched a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    /// The score; higher is a better match.
    pub score: i32,
    /// Byte offsets of the matched characters, for highlighting.
    pub positions: Vec<usize>,
}

/// Bonus for matching the first character of a word.
const WORD_START_BONUS: i32 = 12;
/// Bonus for matching immediately after the previous match.
const CONSECUTIVE_BONUS: i32 = 8;
/// Bonus for matching the very first character of the candidate.
const FIRST_CHARACTER_BONUS: i32 = 10;
/// Bonus for matching the case exactly.
const EXACT_CASE_BONUS: i32 = 2;
/// Penalty for each candidate character skipped before a match.
const SKIP_PENALTY: i32 = 1;
/// Base value of any matched character.
const MATCH_SCORE: i32 = 6;

/// Scores `candidate` against `query`, or returns `None` when it does not match.
///
/// An empty query matches everything with a score of zero, which is what the
/// palette wants before the user types: the full list in its natural order.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    if query.trim().is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }

    let query_chars: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
    if query_chars.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }

    let candidate_chars: Vec<(usize, char)> = candidate.char_indices().collect();
    if query_chars.len() > candidate_chars.len() {
        return None;
    }

    let mut positions = Vec::with_capacity(query_chars.len());
    let mut score = 0i32;
    let mut candidate_index = 0usize;
    let mut previous_match: Option<usize> = None;

    for wanted in query_chars {
        let mut found = None;

        for index in candidate_index..candidate_chars.len() {
            let (offset, actual) = candidate_chars[index];
            if !actual.eq_ignore_ascii_case(&wanted) {
                continue;
            }

            let mut character_score = MATCH_SCORE;

            if index == 0 {
                character_score += FIRST_CHARACTER_BONUS;
            } else {
                let (_, previous) = candidate_chars[index - 1];
                // A word start is either after a separator, or a capital
                // following a lowercase letter as in "StepOver".
                let after_separator = !previous.is_alphanumeric();
                let camel_boundary = previous.is_lowercase() && actual.is_uppercase();
                if after_separator || camel_boundary {
                    character_score += WORD_START_BONUS;
                }
            }

            if previous_match.is_some_and(|previous| previous + 1 == index) {
                character_score += CONSECUTIVE_BONUS;
            }
            if actual == wanted {
                character_score += EXACT_CASE_BONUS;
            }

            // Skipping over candidate characters costs a little, so an early
            // match is preferred to a late one.
            let skipped = i32::try_from(index - candidate_index).unwrap_or(i32::MAX);
            character_score -= skipped.saturating_mul(SKIP_PENALTY);

            found = Some((index, offset, character_score));
            break;
        }

        let (index, offset, character_score) = found?;
        score += character_score;
        positions.push(offset);
        previous_match = Some(index);
        candidate_index = index + 1;
    }

    // Reward covering more of a short candidate: an exact hit on "Run" should
    // beat a scattered hit inside a much longer name.
    let coverage = (positions.len() * 100 / candidate_chars.len().max(1)) as i32;
    score += coverage / 10;

    Some(FuzzyMatch { score, positions })
}

/// Filters and ranks `candidates` against `query`.
///
/// Returns the index of each matching candidate with its match, best first.
/// Ties are broken by the original order, so an empty query leaves the list
/// exactly as it was given.
pub fn rank<'a, I>(query: &str, candidates: I) -> Vec<(usize, FuzzyMatch)>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut matches: Vec<(usize, FuzzyMatch)> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, candidate)| fuzzy_match(query, candidate).map(|found| (index, found)))
        .collect();

    matches.sort_by(|left, right| {
        right
            .1
            .score
            .cmp(&left.1.score)
            .then_with(|| left.0.cmp(&right.0))
    });
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;

    fn score(query: &str, candidate: &str) -> Option<i32> {
        fuzzy_match(query, candidate).map(|found| found.score)
    }

    #[test]
    fn an_empty_query_matches_everything_equally() {
        assert_eq!(score("", "anything"), Some(0));
        assert_eq!(score("   ", "anything"), Some(0));
    }

    #[test]
    fn an_exact_match_scores() {
        assert!(score("run", "Run").is_some());
        assert!(score("Run", "Run").unwrap() >= score("run", "Run").unwrap());
    }

    #[test]
    fn a_subsequence_matches_and_a_non_subsequence_does_not() {
        assert!(score("sov", "Step over").is_some());
        assert!(score("vos", "Step over").is_none(), "order matters");
        assert!(score("xyz", "Step over").is_none());
    }

    #[test]
    fn a_query_longer_than_the_candidate_cannot_match() {
        assert_eq!(score("stepping", "step"), None);
    }

    #[test]
    fn word_starts_beat_letters_buried_mid_word() {
        // "sa" as the initials of "Save as" should beat the same letters
        // appearing inside a longer unrelated phrase.
        let initials = score("sa", "Save as").expect("matches");
        let buried = score("sa", "Disassembly syntax").expect("matches");
        assert!(
            initials > buried,
            "initials {initials} should beat buried {buried}"
        );
    }

    #[test]
    fn consecutive_characters_beat_scattered_ones() {
        let consecutive = score("step", "Step over").expect("matches");
        let scattered = score("step", "Set the pace").expect("matches");
        assert!(
            consecutive > scattered,
            "consecutive {consecutive} should beat scattered {scattered}"
        );
    }

    #[test]
    fn matching_is_case_insensitive_but_exact_case_ranks_higher() {
        assert!(score("STEP", "Step over").is_some());
        let exact = score("Step", "Step over").expect("matches");
        let inexact = score("step", "Step over").expect("matches");
        assert!(exact > inexact);
    }

    #[test]
    fn positions_point_at_the_matched_characters() {
        let found = fuzzy_match("so", "Step over").expect("matches");
        assert_eq!(found.positions.len(), 2);
        let text = "Step over";
        let matched: String = found
            .positions
            .iter()
            .map(|offset| text[*offset..].chars().next().unwrap_or('?'))
            .collect();
        assert_eq!(matched.to_lowercase(), "so");
    }

    #[test]
    fn positions_are_byte_offsets_valid_for_slicing() {
        // Non-ASCII candidates must not produce offsets that split a character.
        let candidate = "Ölçüm göster";
        let found = fuzzy_match("gs", candidate).expect("matches");
        for offset in found.positions {
            assert!(
                candidate.is_char_boundary(offset),
                "offset {offset} is not a character boundary"
            );
        }
    }

    #[test]
    fn ranking_puts_the_best_match_first() {
        let candidates = ["Step out", "Step over", "Stop the program", "Save"];
        let ranked = rank("sov", candidates);
        assert_eq!(
            ranked.first().map(|(index, _)| candidates[*index]),
            Some("Step over")
        );
    }

    #[test]
    fn ranking_an_empty_query_preserves_the_original_order() {
        let candidates = ["one", "two", "three"];
        let ranked = rank("", candidates);
        let order: Vec<usize> = ranked.iter().map(|(index, _)| *index).collect();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn ranking_drops_candidates_that_do_not_match() {
        let ranked = rank("zzz", ["Step over", "Run", "Build"]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn typing_a_command_identifier_finds_its_command() {
        // A user who knows the id from their config should be able to type it.
        let commands = Command::all();
        let texts: Vec<String> = commands.iter().map(Command::search_text).collect();
        let ranked = rank("debug.step-over", texts.iter().map(String::as_str));
        assert_eq!(
            ranked.first().map(|(index, _)| commands[*index].id()),
            Some("debug.step-over".to_owned())
        );
    }

    #[test]
    fn common_abbreviations_find_the_expected_command() {
        let commands = Command::all();
        let texts: Vec<String> = commands.iter().map(Command::search_text).collect();

        for (query, expected) in [
            ("toggle break", "debug.toggle-breakpoint"),
            ("save as", "file.save-as"),
            ("syscall", "app.syscalls"),
            ("go to address", "navigate.go-to-address"),
            ("theme", "view.cycle-theme"),
        ] {
            let ranked = rank(query, texts.iter().map(String::as_str));
            let best = ranked
                .first()
                .map(|(index, _)| commands[*index].id())
                .unwrap_or_default();
            assert_eq!(best, expected, "query {query:?} found {best}");
        }
    }

    #[test]
    fn every_command_is_findable_by_its_own_title() {
        // A command nothing can reach from the palette may as well not exist.
        let commands = Command::all();
        let texts: Vec<String> = commands.iter().map(Command::search_text).collect();

        for command in &commands {
            let ranked = rank(&command.title(), texts.iter().map(String::as_str));
            assert!(
                ranked
                    .first()
                    .is_some_and(|(index, _)| commands[*index] == *command),
                "{} was not the top hit for its own title",
                command.id()
            );
        }
    }

    #[test]
    fn scoring_a_long_query_against_a_long_candidate_terminates() {
        // Guards against pathological input from a paste.
        let query = "a".repeat(500);
        let candidate = "ab".repeat(500);
        assert!(fuzzy_match(&query, &candidate).is_some());
        assert!(fuzzy_match(&query, "short").is_none());
    }
}
