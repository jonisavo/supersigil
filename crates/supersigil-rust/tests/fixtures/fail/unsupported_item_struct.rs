use supersigil_rust::verifies;

#[verifies("req/auth#crit-1")]
struct NotAFunction;

fn main() {}
