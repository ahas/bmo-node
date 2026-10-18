//! Numbers read and written as JavaScript reads and writes them, since the readings are handed to the
//! console as JSON and the probe's fixtures hold the readings written that way: a figure is taken from the
//! longest leading run of text that is one, rounded half up, and written without a point where it is
//! whole.

/// The longest leading run of the text that is a number, after its leading spaces; none where it holds
/// none or what it holds is not finite.
pub fn parse_float(text: &str) -> Option<f64> {
  let text = text.trim_start();
  let bytes = text.as_bytes();
  let mut end = 0;

  if matches!(bytes.first(), Some(b'+' | b'-')) {
    end += 1;
  }

  let digits = |from: usize| bytes[from..].iter().take_while(|byte| byte.is_ascii_digit()).count();
  let whole = digits(end);

  end += whole;

  let mut fraction = 0;

  if bytes.get(end) == Some(&b'.') {
    fraction = digits(end + 1);

    if whole > 0 || fraction > 0 {
      end += 1 + fraction;
    }
  }

  if whole == 0 && fraction == 0 {
    return None;
  }

  if matches!(bytes.get(end), Some(b'e' | b'E')) {
    let mut at = end + 1;

    if matches!(bytes.get(at), Some(b'+' | b'-')) {
      at += 1;
    }

    let exponent = digits(at);

    if exponent > 0 {
      end = at + exponent;
    }
  }

  text[..end].parse::<f64>().ok().filter(|value| value.is_finite())
}

/// A whole text read as a number the strict way: empty or blank is nought, anything else must be one
/// number entire.
pub fn number(text: &str) -> Option<f64> {
  let text = text.trim();

  if text.is_empty() {
    return Some(0.0);
  }

  if text.bytes().any(|byte| byte.is_ascii_alphabetic() && !matches!(byte, b'e' | b'E')) {
    return None;
  }

  text.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Rounded to the nearest whole, a half going up.
pub fn round(value: f64) -> f64 {
  let down = value.floor();

  if value - down >= 0.5 { down + 1.0 } else { down }
}

/// Rounded to so many places, as the record keeps a figure.
pub fn round_to(value: f64, places: f64) -> f64 {
  round(value * places) / places
}

/// A number as text, without a point where it is whole.
pub fn shown(value: f64) -> String {
  format!("{value}")
}
