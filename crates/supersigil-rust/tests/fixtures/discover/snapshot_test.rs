use supersigil_rust::verifies;

#[verifies("req/output#crit-1")]
#[test]
fn test_render_output() {
    let result = render();
    insta::assert_snapshot!("render_output", result);
}
