//! Implementations of the low-level parser combinators.

use crate::format_description::modifier::Padding;
use core::{
    num::{NonZeroU128, NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU8, NonZeroUsize},
    str::FromStr,
};

/// Marker trait for integers.
pub(crate) trait Integer: FromStr {}
impl Integer for u8 {}
impl Integer for u16 {}
impl Integer for u32 {}
impl Integer for u64 {}
impl Integer for u128 {}
impl Integer for usize {}
impl Integer for NonZeroU8 {}
impl Integer for NonZeroU16 {}
impl Integer for NonZeroU32 {}
impl Integer for NonZeroU64 {}
impl Integer for NonZeroU128 {}
impl Integer for NonZeroUsize {}

/// Call the provided parser, only mutating the original input if the final value is successful.
///
/// This is helpful when there may be multiple steps in parsing, as wrapping it with this will
/// ensure the input is never partially mutated.
pub(crate) fn lazy_mut<'a, T>(
    parser: impl Fn(&mut &'a str) -> Option<T>,
) -> impl Fn(&mut &'a str) -> Option<T> {
    move |orig_input| {
        let mut input = *orig_input;
        let value = parser(&mut input)?;
        *orig_input = input;
        Some(value)
    }
}

/// Parse a string.
pub(crate) fn string<'a>(expected: &'a str) -> impl Fn(&mut &'a str) -> Option<&'a str> {
    move |input| {
        let remaining = input.strip_prefix(expected)?;
        *input = remaining;
        Some(expected)
    }
}

/// Parse one of the provided strings.
pub(crate) fn first_string_of<'a>(
    expected_one_of: &'a [&str],
) -> impl Fn(&mut &'a str) -> Option<&'a str> {
    move |input| {
        expected_one_of
            .iter()
            .find_map(|expected| string(expected)(input))
    }
}

/// Consume the first matching string, returning its associated value.
pub(crate) fn first_match<'a, T: Copy + 'a>(
    mut options: impl Iterator<Item = &'a (&'a str, T)>,
) -> impl FnMut(&mut &'a str) -> Option<T> {
    move |input| {
        options.find_map(|&(expected, t)| {
            string(expected)(input)?;
            Some(t)
        })
    }
}

/// Map the resulting value to a new value (that may or may not be the same type).
pub(crate) fn flat_map<'a, T, U>(
    parser: impl Fn(&mut &'a str) -> Option<T>,
    map_fn: impl Fn(T) -> Option<U>,
) -> impl Fn(&mut &'a str) -> Option<U> {
    lazy_mut(move |input| parser(input).and_then(|v| map_fn(v)))
}

/// Consume between `n` and `m` instances of the provided parser.
pub(crate) fn n_to_m<'a, T>(
    n: u8,
    m: u8,
    parser: impl Fn(&mut &'a str) -> Option<T>,
) -> impl Fn(&mut &'a str) -> Option<&'a str> {
    debug_assert!(m >= n);
    lazy_mut(move |input| {
        // We need to keep this to determine the total length eventually consumed.
        let orig_input = *input;

        // Mandatory
        for _ in 0..n {
            parser(input)?;
        }

        // Optional
        for _ in n..m {
            if parser(input).is_none() {
                break;
            };
        }

        Some(&orig_input[..(orig_input.len() - input.len())])
    })
}

/// Consume exactly `n` instances of the provided parser.
pub(crate) fn exactly_n<'a, T>(
    n: u8,
    parser: impl Fn(&mut &'a str) -> Option<T>,
) -> impl Fn(&mut &'a str) -> Option<&'a str> {
    n_to_m(n, n, parser)
}

/// Consume between `n` and `m` digits, returning the numerical value.
pub(crate) fn n_to_m_digits<'a, T: Integer>(n: u8, m: u8) -> impl Fn(&mut &'a str) -> Option<T> {
    debug_assert!(m >= n);
    flat_map(n_to_m(n, m, any_digit), |value| value.parse().ok())
}

/// Consume exactly `n` digits, returning the numerical value.
pub(crate) fn exactly_n_digits<'a, T: Integer>(n: u8) -> impl Fn(&mut &'a str) -> Option<T> {
    n_to_m_digits(n, n)
}

/// Consume exactly `n` digits, returning the numerical value.
pub(crate) fn exactly_n_digits_padded<'a, T: Integer>(
    n: u8,
    padding: Padding,
) -> impl Fn(&mut &'a str) -> Option<T> {
    lazy_mut(move |input| {
        if padding == Padding::None {
            n_to_m_digits(1, n)(input)
        } else if padding == Padding::Space {
            let pad_width = n_to_m(0, n, ascii_char(b' '))(input).map_or(0, |s| s.len() as u8);
            exactly_n_digits(n - pad_width)(input)
        } else {
            let pad_width = n_to_m(0, n, ascii_char(b'0'))(input).map_or(0, |s| s.len() as u8);
            exactly_n_digits(n - pad_width)(input)
        }
    })
}

/// Consume exactly one digit.
pub(crate) fn any_digit(input: &mut &str) -> Option<u8> {
    if !input.is_empty() && input.as_bytes()[0].is_ascii_digit() {
        let ret_val = input.as_bytes()[0];
        *input = &input[1..];
        Some(ret_val)
    } else {
        None
    }
}

/// Consume exactly one of the provided ASCII characters.
pub(crate) fn ascii_char(char: u8) -> impl Fn(&mut &str) -> Option<()> {
    move |input| {
        if !input.is_empty() && input.as_bytes()[0] == char {
            *input = &input[1..];
            Some(())
        } else {
            None
        }
    }
}
