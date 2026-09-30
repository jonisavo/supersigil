//! Attributes the lines of a reviewed file to the recorded edits that produced them.
//!
//! Pure over its inputs: given a base state, a target state, and the recorded
//! edits of one path, it walks backward from the target, accepting a step only
//! when executing the edit forward reproduces the current bytes and every
//! recorded hash matches, and reports per line which edits explain it, which
//! readings disagree, and where the evidence stops.
