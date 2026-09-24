//! Decimal string to fixed point conversion.
//!
//! Status: signatures and tests only. The bodies are deliberately
//! unimplemented.
//!
//! Exchange payloads carry prices and quantities as decimal strings
//! (`"104.48000000"`). Rules work on `i64` scaled by the instrument's
//! exponent, so `"104.48000000"` with a price exponent of 2 becomes `10448`.
//! The exponent belongs to the instrument, never to the value: two values of
//! the same instrument share a scale and therefore compare and subtract
//! directly. See `design.md`, D1, for why floats are not an option.
//!
//! The conversion is total and explicit. A digit that would be lost is an
//! error, not a rounding: Binance publishes prices on the tick grid, so a
//! digit past the instrument's precision means the exponent table and the
//! stream disagree, and that is a data quality bug worth surfacing rather
//! than absorbing.

use thiserror::Error;

/// Largest exponent [`parse_fixed`] accepts.
///
/// The scale factor is `10^exp`, and `10^18` is the last power of ten that
/// fits in `i64` (`i64::MAX` is roughly `9.22e18`). Real instruments sit far
/// below this: the reference corpus uses 2 for prices and at most 5 for
/// quantities.
pub const MAX_EXP: u8 = 18;

/// Why a decimal string could not be converted.
///
/// Carries the offending byte offset where there is one, so the caller can
/// report the record it came from without re-scanning the input.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FixedError {
    /// The input was empty.
    #[error("empty input")]
    Empty,

    /// A byte that is not a digit appeared where a digit was required.
    ///
    /// Covers exponent notation (`"1e5"`), surrounding whitespace, an explicit
    /// `+` sign, and anything else Binance does not emit. `index` is a byte
    /// offset into the original string.
    #[error("invalid byte {byte:?} at offset {index}")]
    InvalidByte { index: usize, byte: char },

    /// More than one decimal separator, as in `"1.2.3"`.
    #[error("more than one decimal point")]
    MultipleDots,

    /// A sign and/or a dot, but no digit at all: `"-"`, `"."`, `".5"`, `"104."`.
    ///
    /// Both sides of the dot are required. Accepting `".5"` or `"104."` would
    /// mean guessing at an input shape the exchange never produces.
    #[error("no digits on both sides of the decimal point")]
    NoDigits,

    /// A non-zero digit beyond the instrument's precision.
    ///
    /// Never rounded. `"104.485"` at exponent 2 is an error; `"104.48000000"`
    /// is not, because everything past the exponent is zero.
    #[error("significant digit beyond exponent {exp} at offset {index}")]
    PrecisionLoss { index: usize, exp: u8 },

    /// The scaled value does not fit in `i64`.
    #[error("value does not fit in i64")]
    Overflow,

    /// `exp` is above [`MAX_EXP`], so `10^exp` is not representable.
    #[error("exponent {exp} above the maximum of {MAX_EXP}")]
    ExponentOutOfRange { exp: u8 },
}

/// Parses a decimal string into fixed point at the given exponent.
///
/// Accepts an optional leading `-`, one or more digits, and optionally a `.`
/// followed by one or more digits. Nothing else: no `+`, no exponent
/// notation, no surrounding whitespace, no thousands separator.
///
/// Fewer fractional digits than `exp` are padded (`"104.4"` at exponent 2 is
/// `10440`), more are accepted only while they are zero.
///
/// ```ignore
/// assert_eq!(parse_fixed("104.48000000", 2), Ok(10448));
/// assert_eq!(parse_fixed("104.4", 2), Ok(10440));
/// assert_eq!(parse_fixed("104", 2), Ok(10400));
/// assert!(parse_fixed("104.485", 2).is_err());
/// ```
pub fn parse_fixed(_s: &str, _exp: u8) -> Result<i64, FixedError> {
    // Validate exp against MAX_EXP first: an out of range exponent makes
    // every other check meaningless.
    todo!("parse_fixed")
}

/// Renders a fixed point value back to a decimal string.
///
/// The inverse of [`parse_fixed`], in the sense that parsing the output at the
/// same exponent yields the input value. Trailing fractional zeros are
/// dropped, and so is the decimal point when nothing is left after it:
/// `10440` at exponent 2 is `"104.4"`, `10400` is `"104"`, `0` is `"0"`.
///
/// Total: `i64::MIN` and exponents above [`MAX_EXP`] all render. Note that an
/// exponent above `MAX_EXP` renders correctly but will not parse back, since
/// no `i64` scale factor exists for it.
pub fn format_fixed(_v: i64, _exp: u8) -> String {
    todo!("format_fixed")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse: the shapes the corpus actually contains --------------------

    #[test]
    fn exp_zero_is_a_plain_integer() {
        assert_eq!(parse_fixed("104", 0), Ok(104));
        assert_eq!(parse_fixed("0", 0), Ok(0));
        assert_eq!(parse_fixed("1", 0), Ok(1));
        // A fractional part is still allowed at exponent 0, as long as it is
        // all zeros: Binance pads quantities to eight decimals regardless of
        // stepSize, so "5.00000000" at exponent 0 is a real input.
        assert_eq!(parse_fixed("5.00000000", 0), Ok(5));
    }

    #[test]
    fn trailing_zeros_beyond_exp_are_tolerated() {
        assert_eq!(parse_fixed("104.48000000", 2), Ok(10448));
        assert_eq!(parse_fixed("0.00008000", 5), Ok(8));
        assert_eq!(parse_fixed("80968.90000000", 2), Ok(8096890));
    }

    #[test]
    fn non_zero_digit_beyond_exp_is_an_error() {
        // Never rounded, in either direction.
        assert!(matches!(
            parse_fixed("104.485", 2),
            Err(FixedError::PrecisionLoss { exp: 2, .. })
        ));
        assert!(matches!(
            parse_fixed("104.484", 2),
            Err(FixedError::PrecisionLoss { exp: 2, .. })
        ));
        // The offending digit can sit arbitrarily far past the exponent,
        // behind any number of zeros.
        assert!(matches!(
            parse_fixed("104.48000001", 2),
            Err(FixedError::PrecisionLoss { exp: 2, .. })
        ));
        // Including at exponent 0.
        assert!(matches!(
            parse_fixed("104.1", 0),
            Err(FixedError::PrecisionLoss { exp: 0, .. })
        ));
    }

    #[test]
    fn fewer_decimals_than_exp_are_padded() {
        assert_eq!(parse_fixed("104.4", 2), Ok(10440));
        assert_eq!(parse_fixed("104.48", 2), Ok(10448));
        assert_eq!(parse_fixed("0.1", 5), Ok(10000));
    }

    #[test]
    fn missing_fractional_part_is_padded() {
        assert_eq!(parse_fixed("104", 2), Ok(10400));
        assert_eq!(parse_fixed("0", 5), Ok(0));
    }

    #[test]
    fn negative_values() {
        assert_eq!(parse_fixed("-104.48", 2), Ok(-10448));
        assert_eq!(parse_fixed("-104", 2), Ok(-10400));
        assert_eq!(parse_fixed("-0.01", 2), Ok(-1));
        assert_eq!(parse_fixed("-104.4", 2), Ok(-10440));
    }

    #[test]
    fn negative_zero_is_zero() {
        // i64 has no signed zero, so the sign is simply dropped. Worth
        // pinning: an implementation that negates only a non-zero accumulator,
        // or one that carries the sign separately, can get this wrong.
        assert_eq!(parse_fixed("-0.00", 2), Ok(0));
        assert_eq!(parse_fixed("-0", 0), Ok(0));
        assert_eq!(parse_fixed("-0.00000000", 5), Ok(0));
    }

    // --- parse: rejection --------------------------------------------------

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(parse_fixed("", 2), Err(FixedError::Empty));
    }

    #[test]
    fn non_numeric_input_is_rejected() {
        assert!(matches!(
            parse_fixed("abc", 2),
            Err(FixedError::InvalidByte { index: 0, .. })
        ));
        // Exponent notation is valid JSON number syntax but Binance never
        // emits it, and accepting it would mean a second parsing path.
        assert!(matches!(
            parse_fixed("1e5", 2),
            Err(FixedError::InvalidByte { index: 1, .. })
        ));
        // No trimming: whitespace means the caller sliced the payload wrong.
        assert!(matches!(
            parse_fixed(" 104.48", 2),
            Err(FixedError::InvalidByte { index: 0, .. })
        ));
        assert!(matches!(
            parse_fixed("104.48 ", 2),
            Err(FixedError::InvalidByte { index: 6, .. })
        ));
        // An explicit plus is not accepted; the exchange does not produce it.
        assert!(matches!(
            parse_fixed("+104.48", 2),
            Err(FixedError::InvalidByte { index: 0, .. })
        ));
    }

    #[test]
    fn multiple_decimal_points_are_rejected() {
        assert_eq!(parse_fixed("1.2.3", 2), Err(FixedError::MultipleDots));
    }

    #[test]
    fn digits_are_required_on_both_sides_of_the_dot() {
        assert_eq!(parse_fixed("-", 2), Err(FixedError::NoDigits));
        assert_eq!(parse_fixed(".", 2), Err(FixedError::NoDigits));
        assert_eq!(parse_fixed(".5", 2), Err(FixedError::NoDigits));
        assert_eq!(parse_fixed("104.", 2), Err(FixedError::NoDigits));
        assert_eq!(parse_fixed("-.5", 2), Err(FixedError::NoDigits));
    }

    // --- parse: numeric limits ---------------------------------------------

    #[test]
    fn overflow_is_reported_not_wrapped() {
        // One past i64::MAX.
        assert_eq!(
            parse_fixed("9223372036854775808", 0),
            Err(FixedError::Overflow)
        );
        // Far past it, and long enough that an accumulator must stop early
        // rather than wrap silently on the way.
        assert_eq!(parse_fixed(&"9".repeat(40), 0), Err(FixedError::Overflow));
        assert_eq!(
            parse_fixed(&format!("-{}", "9".repeat(40)), 0),
            Err(FixedError::Overflow)
        );
        // Overflow can also come from the scaling, not from the digits: this
        // one fits in i64 on its own but not once shifted by two decimals.
        assert_eq!(
            parse_fixed("92233720368547758", 2),
            Err(FixedError::Overflow)
        );
    }

    #[test]
    fn i64_bounds_are_representable() {
        assert_eq!(parse_fixed("9223372036854775807", 0), Ok(i64::MAX));
        // i64::MIN has no positive counterpart, so an implementation that
        // accumulates a positive magnitude and negates at the end overflows
        // here. Accumulate in the negative direction, or in a wider type.
        assert_eq!(parse_fixed("-9223372036854775808", 0), Ok(i64::MIN));
    }

    #[test]
    fn exponent_bounds() {
        // Exponent 0: no scaling at all.
        assert_eq!(parse_fixed("7", 0), Ok(7));
        // Exponent 18: the largest scale factor that fits in i64.
        assert_eq!(parse_fixed("1", MAX_EXP), Ok(1_000_000_000_000_000_000));
        assert_eq!(parse_fixed("0.000000000000000001", MAX_EXP), Ok(1));
        // 9.223372036854775807 scaled by 10^18 is exactly i64::MAX.
        assert_eq!(parse_fixed("9.223372036854775807", MAX_EXP), Ok(i64::MAX));
        // Two units of scale too far.
        assert_eq!(parse_fixed("10", MAX_EXP), Err(FixedError::Overflow));
        assert_eq!(
            parse_fixed("1", MAX_EXP + 1),
            Err(FixedError::ExponentOutOfRange { exp: 19 })
        );
        // The exponent is checked before the string: a bad exponent is a host
        // side bug and should not be masked by whatever the payload holds.
        assert_eq!(
            parse_fixed("not a number", 255),
            Err(FixedError::ExponentOutOfRange { exp: 255 })
        );
    }

    // --- format ------------------------------------------------------------

    #[test]
    fn format_drops_superfluous_trailing_zeros() {
        assert_eq!(format_fixed(10448, 2), "104.48");
        assert_eq!(format_fixed(10440, 2), "104.4");
        assert_eq!(format_fixed(10400, 2), "104");
        assert_eq!(format_fixed(0, 2), "0");
        assert_eq!(format_fixed(0, 0), "0");
    }

    #[test]
    fn format_pads_the_integer_part_when_needed() {
        // The value is smaller than one unit, so the integer part is an
        // explicit zero and the fraction is zero padded on the left.
        assert_eq!(format_fixed(1, 2), "0.01");
        assert_eq!(format_fixed(8, 5), "0.00008");
        assert_eq!(format_fixed(35152, 5), "0.35152");
    }

    #[test]
    fn format_negative_values() {
        assert_eq!(format_fixed(-10448, 2), "-104.48");
        assert_eq!(format_fixed(-10400, 2), "-104");
        assert_eq!(format_fixed(-1, 2), "-0.01");
        // i64::MIN: its magnitude is not representable as a positive i64, the
        // same trap as on the parse side.
        assert_eq!(format_fixed(i64::MIN, 0), "-9223372036854775808");
        assert_eq!(format_fixed(i64::MIN, MAX_EXP), "-9.223372036854775808");
    }

    #[test]
    fn format_at_exp_zero_is_a_plain_integer() {
        assert_eq!(format_fixed(104, 0), "104");
        assert_eq!(format_fixed(i64::MAX, 0), "9223372036854775807");
    }

    // --- round trip --------------------------------------------------------

    #[test]
    fn round_trip_on_corpus_values() {
        // Taken from fixtures/reference.msr.zst. The corpus quotes prices at
        // tickSize 0.01 (exponent 2) and quantities at stepSize 0.00001,
        // 0.0001, 0.001 or 1 (exponents 5, 4, 3 and 0).
        const CASES: &[(&str, u8, i64)] = &[
            ("104.52000000", 2, 10452),
            ("80968.90000000", 2, 8096890),
            ("2495.03000000", 2, 249503),
            ("720.24000000", 2, 72024),
            ("0.00008000", 5, 8),
            ("0.35152000", 5, 35152),
            ("0.00123000", 5, 123),
            ("76.23600000", 3, 76236),
            ("58.92000000", 3, 58920),
            ("0.04200000", 4, 420),
        ];

        for &(input, exp, expected) in CASES {
            assert_eq!(parse_fixed(input, exp), Ok(expected), "parsing {input:?}");
            // Formatting is not string identity: the exchange pads to eight
            // decimals, we do not. Round tripping goes through the value.
            let rendered = format_fixed(expected, exp);
            assert_eq!(
                parse_fixed(&rendered, exp),
                Ok(expected),
                "round tripping {input:?} via {rendered:?}"
            );
        }
    }

    #[test]
    fn round_trip_is_stable_across_the_i64_range() {
        const VALUES: &[i64] = &[0, 1, -1, 10, -10, 999, -999, 1_000_000, i64::MAX, i64::MIN];

        for &exp in &[0u8, 1, 2, 5, 8, MAX_EXP] {
            for &v in VALUES {
                let rendered = format_fixed(v, exp);
                assert_eq!(
                    parse_fixed(&rendered, exp),
                    Ok(v),
                    "value {v} at exponent {exp} rendered as {rendered:?}"
                );
            }
        }
    }
}
