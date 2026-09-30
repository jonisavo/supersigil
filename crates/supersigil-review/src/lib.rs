//! Builds the review model: the reviewed diff with each changed line's attribution.
//!
//! Pure over its inputs: it diffs blob bytes, maps attribution onto the diff
//! lines through ordered outcome rules, and produces the JSON model and the
//! terminal summary that `supersigil review` and `supersigil why` print.
