//! AST for parsing format descriptions.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::iter;
use core::iter::Peekable;

use super::{lexer, Error, Location, Span, Spanned, SpannedValue};

/// One part of a complete format description.
#[allow(variant_size_differences)]
pub(super) enum Item<'a> {
    /// A literal string, formatted and parsed as-is.
    Literal(Spanned<&'a [u8]>),
    /// A sequence of brackets. The first acts as the escape character.
    EscapedBracket {
        /// The first bracket.
        _first: Location,
        /// The second bracket.
        _second: Location,
    },
    /// Part of a type, along with its modifiers.
    Component {
        /// Where the opening bracket was in the format string.
        _opening_bracket: Location,
        /// Whitespace between the opening bracket and name.
        _leading_whitespace: Option<Spanned<&'a [u8]>>,
        /// The name of the component.
        name: Spanned<&'a [u8]>,
        /// The modifiers for the component.
        modifiers: Box<[Modifier<'a>]>,
        /// Whitespace between the modifiers and closing bracket.
        _trailing_whitespace: Option<Spanned<&'a [u8]>>,
        /// Where the closing bracket was in the format string.
        _closing_bracket: Location,
    },
    /// An optional sequence of items.
    Optional {
        /// Where the opening bracket was in the format string.
        opening_bracket: Location,
        /// Whitespace between the opening bracket and "optional".
        _leading_whitespace: Option<Spanned<&'a [u8]>>,
        /// The "optional" keyword.
        _optional_kw: Spanned<&'a [u8]>,
        /// Whitespace between the "optional" keyword and the opening bracket.
        _whitespace: Spanned<&'a [u8]>,
        /// The items within the optional sequence.
        nested_format_description: NestedFormatDescription<'a>,
        /// Where the closing bracket was in the format string.
        closing_bracket: Location,
    },
}

/// A format description that is nested within another format description.
pub(super) struct NestedFormatDescription<'a> {
    /// Where the opening bracket was in the format string.
    pub(super) _opening_bracket: Location,
    /// The items within the nested format description.
    pub(super) items: Box<[Item<'a>]>,
    /// Where the closing bracket was in the format string.
    pub(super) _closing_bracket: Location,
    /// Whitespace between the closing bracket and the next item.
    pub(super) _trailing_whitespace: Option<Spanned<&'a [u8]>>,
}

/// A modifier for a component.
pub(super) struct Modifier<'a> {
    /// Whitespace preceding the modifier.
    pub(super) _leading_whitespace: Spanned<&'a [u8]>,
    /// The key of the modifier.
    pub(super) key: Spanned<&'a [u8]>,
    /// Where the colon of the modifier was in the format string.
    pub(super) _colon: Location,
    /// The value of the modifier.
    pub(super) value: Spanned<&'a [u8]>,
}

/// Parse the provided tokens into an AST.
pub(super) fn parse<'item: 'iter, 'iter>(
    tokens: &'iter mut Peekable<impl Iterator<Item = lexer::Token<'item>>>,
) -> impl Iterator<Item = Result<Item<'item>, Error>> + 'iter {
    parse_inner::<_, false>(tokens)
}

/// Parse the provided tokens into an AST. The const generic indicates whether the resulting
/// [`Item`] will be used directly or as part of a [`NestedFormatDescription`].
fn parse_inner<'item, I: Iterator<Item = lexer::Token<'item>>, const NESTED: bool>(
    tokens: &mut Peekable<I>,
) -> impl Iterator<Item = Result<Item<'item>, Error>> + '_ {
    iter::from_fn(move || {
        if NESTED
            && matches!(
                tokens.peek(),
                Some(lexer::Token::Bracket {
                    kind: lexer::BracketKind::Closing,
                    location: _,
                })
            )
        {
            return None;
        }

        Some(match tokens.next()? {
            lexer::Token::Literal(Spanned { value: _, span: _ }) if NESTED => {
                unreachable!("internal error: literal should not be present in nested description")
            }
            lexer::Token::Literal(Spanned { value, span }) => {
                Ok(Item::Literal(value.spanned(span)))
            }
            lexer::Token::Bracket {
                kind: lexer::BracketKind::Opening,
                location,
            } => {
                if let Some(&lexer::Token::Bracket {
                    // escaped bracket
                    kind: lexer::BracketKind::Opening,
                    location: second_location,
                }) = tokens.peek()
                {
                    tokens.next(); // consume
                    Ok(Item::EscapedBracket {
                        _first: location,
                        _second: second_location,
                    })
                } else {
                    // component
                    parse_component(location, tokens)
                }
            }
            lexer::Token::Bracket {
                kind: lexer::BracketKind::Closing,
                location: _,
            } if NESTED => {
                unreachable!(
                    "internal error: closing bracket should be caught by the `if` statement"
                )
            }
            lexer::Token::Bracket {
                kind: lexer::BracketKind::Closing,
                location: _,
            } => {
                unreachable!(
                    "internal error: closing bracket should have been consumed by \
                     `parse_component`"
                )
            }
            lexer::Token::ComponentPart {
                kind: _, // whitespace is significant in nested components
                value,
            } if NESTED => Ok(Item::Literal(value)),
            lexer::Token::ComponentPart { kind: _, value: _ } => unreachable!(
                "internal error: component part should have been consumed by `parse_component`"
            ),
        })
    })
}

/// Parse a component. This assumes that the opening bracket has already been consumed.
fn parse_component<'a>(
    opening_bracket: Location,
    tokens: &mut Peekable<impl Iterator<Item = lexer::Token<'a>>>,
) -> Result<Item<'a>, Error> {
    let leading_whitespace = if let Some(&lexer::Token::ComponentPart {
        kind: lexer::ComponentKind::Whitespace,
        value,
    }) = tokens.peek()
    {
        tokens.next(); // consume
        Some(value)
    } else {
        None
    };

    let name = if let Some(&lexer::Token::ComponentPart {
        kind: lexer::ComponentKind::NotWhitespace,
        value,
    }) = tokens.peek()
    {
        tokens.next(); // consume
        value
    } else {
        let span = match leading_whitespace {
            Some(Spanned { value: _, span }) => span,
            None => Span {
                start: opening_bracket,
                end: opening_bracket,
            },
        };
        return Err(Error {
            _inner: span.error("expected component name"),
            public: crate::error::InvalidFormatDescription::MissingComponentName {
                index: span.start.byte as _,
            },
        });
    };

    if *name == b"optional" {
        if let Some(&lexer::Token::ComponentPart {
            kind: lexer::ComponentKind::Whitespace,
            value: whitespace,
        }) = tokens.peek()
        {
            tokens.next(); // consume

            let nested = parse_nested(whitespace.span.end, tokens)?;

            let closing_bracket = if let Some(&lexer::Token::Bracket {
                kind: lexer::BracketKind::Closing,
                location,
            }) = tokens.peek()
            {
                tokens.next(); // consume
                location
            } else {
                return Err(Error {
                    _inner: opening_bracket.error("unclosed bracket"),
                    public: crate::error::InvalidFormatDescription::UnclosedOpeningBracket {
                        index: opening_bracket.byte as _,
                    },
                });
            };

            return Ok(Item::Optional {
                opening_bracket,
                _leading_whitespace: leading_whitespace,
                _optional_kw: name,
                _whitespace: whitespace,
                nested_format_description: nested,
                closing_bracket,
            });
        } else {
            return Err(Error {
                _inner: name.span.error("expected whitespace after `optional`"),
                public: crate::error::InvalidFormatDescription::Expected {
                    what: "whitespace after `optional`",
                    index: name.span.end.byte as _,
                },
            });
        }
    }

    let mut modifiers = Vec::new();
    let trailing_whitespace = loop {
        let whitespace = if let Some(&lexer::Token::ComponentPart {
            kind: lexer::ComponentKind::Whitespace,
            value,
        }) = tokens.peek()
        {
            tokens.next(); // consume
            value
        } else {
            break None;
        };

        // This is not necessary for proper parsing, but provides a much better error when a nested
        // description is used where it's not allowed.
        if let Some(&lexer::Token::Bracket {
            kind: lexer::BracketKind::Opening,
            location,
        }) = tokens.peek()
        {
            return Err(Error {
                _inner: location
                    .to(location)
                    .error("modifier must be of the form `key:value`"),
                public: crate::error::InvalidFormatDescription::InvalidModifier {
                    value: String::from("["),
                    index: location.byte as _,
                },
            });
        }

        if let Some(&lexer::Token::ComponentPart {
            kind: lexer::ComponentKind::NotWhitespace,
            value: Spanned { value, span },
        }) = tokens.peek()
        {
            tokens.next(); // consume

            let colon_index = match value.iter().position(|&b| b == b':') {
                Some(index) => index,
                None => {
                    return Err(Error {
                        _inner: span.error("modifier must be of the form `key:value`"),
                        public: crate::error::InvalidFormatDescription::InvalidModifier {
                            value: String::from_utf8_lossy(value).into_owned(),
                            index: span.start.byte as _,
                        },
                    });
                }
            };
            let key = &value[..colon_index];
            let value = &value[colon_index + 1..];

            if key.is_empty() {
                return Err(Error {
                    _inner: span.shrink_to_start().error("expected modifier key"),
                    public: crate::error::InvalidFormatDescription::InvalidModifier {
                        value: String::new(),
                        index: span.start.byte as _,
                    },
                });
            }
            if value.is_empty() {
                return Err(Error {
                    _inner: span.shrink_to_end().error("expected modifier value"),
                    public: crate::error::InvalidFormatDescription::InvalidModifier {
                        value: String::new(),
                        index: span.shrink_to_end().start.byte as _,
                    },
                });
            }

            modifiers.push(Modifier {
                _leading_whitespace: whitespace,
                key: key.spanned(span.shrink_to_before(colon_index as _)),
                _colon: span.start.offset(colon_index as _),
                value: value.spanned(span.shrink_to_after(colon_index as _)),
            });
        } else {
            break Some(whitespace);
        }
    };

    let closing_bracket = if let Some(&lexer::Token::Bracket {
        kind: lexer::BracketKind::Closing,
        location,
    }) = tokens.peek()
    {
        tokens.next(); // consume
        location
    } else {
        return Err(Error {
            _inner: opening_bracket.error("unclosed bracket"),
            public: crate::error::InvalidFormatDescription::UnclosedOpeningBracket {
                index: opening_bracket.byte as _,
            },
        });
    };

    Ok(Item::Component {
        _opening_bracket: opening_bracket,
        _leading_whitespace: leading_whitespace,
        name,
        modifiers: modifiers.into_boxed_slice(),
        _trailing_whitespace: trailing_whitespace,
        _closing_bracket: closing_bracket,
    })
}

/// Parse a nested format description. The location provided is the the most recent one consumed.
fn parse_nested<'a>(
    last_location: Location,
    tokens: &mut Peekable<impl Iterator<Item = lexer::Token<'a>>>,
) -> Result<NestedFormatDescription<'a>, Error> {
    let opening_bracket = if let Some(&lexer::Token::Bracket {
        kind: lexer::BracketKind::Opening,
        location,
    }) = tokens.peek()
    {
        tokens.next(); // consume
        location
    } else {
        return Err(Error {
            _inner: last_location.error("expected opening bracket"),
            public: crate::error::InvalidFormatDescription::Expected {
                what: "opening bracket",
                index: last_location.byte as _,
            },
        });
    };

    let items = parse_inner::<_, true>(tokens).collect::<Result<_, _>>()?;

    let closing_bracket = if let Some(&lexer::Token::Bracket {
        kind: lexer::BracketKind::Closing,
        location,
    }) = tokens.peek()
    {
        tokens.next(); // consume
        location
    } else {
        return Err(Error {
            _inner: opening_bracket.error("unclosed bracket"),
            public: crate::error::InvalidFormatDescription::UnclosedOpeningBracket {
                index: opening_bracket.byte as _,
            },
        });
    };

    let trailing_whitespace = if let Some(&lexer::Token::ComponentPart {
        kind: lexer::ComponentKind::Whitespace,
        value,
    }) = tokens.peek()
    {
        tokens.next(); // consume
        Some(value)
    } else {
        None
    };

    Ok(NestedFormatDescription {
        _opening_bracket: opening_bracket,
        items,
        _closing_bracket: closing_bracket,
        _trailing_whitespace: trailing_whitespace,
    })
}
