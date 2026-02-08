use crate::api::submissions::helpers::sanitized_filename;

#[test]
fn sanitized_filename_filters_disallowed_chars() {
    let input = "report (final)!.png";
    let sanitized = sanitized_filename(input);
    assert_eq!(sanitized, "reportfinal.png");
}

#[test]
fn sanitized_filename_falls_back_on_empty() {
    let input = "###";
    let sanitized = sanitized_filename(input);
    assert_eq!(sanitized, "upload");
}
