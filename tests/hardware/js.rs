//! Numbers read and written as JavaScript reads and writes them.

use hardware::js::{parse_float, round, round_to, shown};

#[test]
fn a_figure_is_the_longest_leading_number_and_nothing_else() {
  assert_eq!(parse_float("  117358.75 1800000.00"), Some(117_358.75));
  assert_eq!(parse_float("812"), Some(812.0));
  assert_eq!(parse_float("-128000"), Some(-128_000.0));
  assert_eq!(parse_float("3.5GHz"), Some(3.5));
  assert_eq!(parse_float(".5"), Some(0.5));
  assert_eq!(parse_float("1e3x"), Some(1000.0));
  assert_eq!(parse_float("[N/A]"), None);
  assert_eq!(parse_float(""), None);
  assert_eq!(parse_float("-"), None);
}

#[test]
fn a_half_rounds_up_and_a_whole_number_is_shown_without_a_point() {
  assert_eq!(round(2.5), 3.0);
  assert_eq!(round(-2.5), -2.0);
  assert_eq!(round(-2.6), -3.0);
  assert_eq!(round(44_850.0 / 100.0) / 10.0, 44.9);
  assert_eq!(round_to(0.123_456, 100.0), 0.12);
  assert_eq!(shown(50.0), "50");
  assert_eq!(shown(0.25), "0.25");
}
