#![allow(clippy::type_complexity)]

mod integer;
use std::{any::Any, borrow::Cow, error::Error, sync::Arc};

pub use integer::*;
mod float;
pub use float::*;
mod literal;
pub use literal::*;
mod or;
pub use or::*;
mod then;
pub use then::*;
mod string;
pub use string::*;
mod repeat;
pub use repeat::*;
mod separated;
pub use separated::*;
mod has_parser;
pub use has_parser::*;
mod word;
pub use word::*;
mod sentence;
pub use sentence::*;
mod stop_on;
pub use stop_on::*;
mod map;
pub use map::*;
mod regex;
pub use regex::*;

/// A trait for a parser with a default state.
pub trait CreateParserState: Parser {
    /// Create the default state of the parser.
    fn create_parser_state(&self) -> <Self as Parser>::PartialState;
}

impl<P: ?Sized + CreateParserState> CreateParserState for &P {
    fn create_parser_state(&self) -> <Self as Parser>::PartialState {
        (*self).create_parser_state()
    }
}

impl<P: ?Sized + CreateParserState> CreateParserState for Box<P> {
    fn create_parser_state(&self) -> <Self as Parser>::PartialState {
        (**self).create_parser_state()
    }
}

impl<P: ?Sized + CreateParserState> CreateParserState for Arc<P> {
    fn create_parser_state(&self) -> <Self as Parser>::PartialState {
        (**self).create_parser_state()
    }
}

impl CreateParserState for ArcParser {
    fn create_parser_state(&self) -> <Self as Parser>::PartialState {
        self.0.create_parser_state()
    }
}

/// An incremental parser for a structured input.
pub trait Parser {
    /// The error type of the parser.
    type Error;
    /// The output of the parser.
    type Output;
    /// The state of the parser.
    type PartialState;

    /// Parse the given input.
    fn parse<'a>(
        &self,
        state: &Self::PartialState,
        input: &'a [u8],
    ) -> Result<ParseResult<'a, Self::PartialState, Self::Output>, Self::Error>;
}

impl Parser for () {
    type Error = ();
    type Output = ();
    type PartialState = ();

    fn parse<'a>(
        &self,
        _state: &Self::PartialState,
        input: &'a [u8],
    ) -> Result<ParseResult<'a, Self::PartialState, Self::Output>, Self::Error> {
        Ok(ParseResult::Finished {
            result: (),
            remaining: input,
        })
    }
}

impl<P: ?Sized + Parser> Parser for &P {
    type Error = P::Error;
    type Output = P::Output;
    type PartialState = P::PartialState;

    fn parse<'a>(
        &self,
        state: &Self::PartialState,
        input: &'a [u8],
    ) -> Result<ParseResult<'a, Self::PartialState, Self::Output>, Self::Error> {
        (*self).parse(state, input)
    }
}

impl<P: ?Sized + Parser> Parser for Box<P> {
    type Error = P::Error;
    type Output = P::Output;
    type PartialState = P::PartialState;

    fn parse<'a>(
        &self,
        state: &Self::PartialState,
        input: &'a [u8],
    ) -> Result<ParseResult<'a, Self::PartialState, Self::Output>, Self::Error> {
        let _self: &P = self;
        _self.parse(state, input)
    }
}

impl<P: ?Sized + Parser> Parser for Arc<P> {
    type Error = P::Error;
    type Output = P::Output;
    type PartialState = P::PartialState;

    fn parse<'a>(
        &self,
        state: &Self::PartialState,
        input: &'a [u8],
    ) -> Result<ParseResult<'a, Self::PartialState, Self::Output>, Self::Error> {
        let _self: &P = self;
        _self.parse(state, input)
    }
}

trait AnyCreateParserState:
    Parser<
        Error = Arc<dyn Error + Send + Sync>,
        Output = Arc<dyn Any + Send + Sync>,
        PartialState = Arc<dyn Any + Send + Sync>,
    > + CreateParserState
    + Send
    + Sync
{
}

impl<
        P: Parser<
                Error = Arc<dyn Error + Send + Sync>,
                Output = Arc<dyn Any + Send + Sync>,
                PartialState = Arc<dyn Any + Send + Sync>,
            > + CreateParserState
            + Send
            + Sync,
    > AnyCreateParserState for P
{
}

/// A boxed parser.
pub struct ArcParser(Arc<dyn AnyCreateParserState + Send + Sync>);

impl ArcParser {
    fn new<P>(parser: P) -> Self
    where
        P: Parser<
                Error = Arc<dyn Error + Send + Sync>,
                Output = Arc<dyn Any + Send + Sync>,
                PartialState = Arc<dyn Any + Send + Sync>,
            > + CreateParserState
            + Send
            + Sync
            + 'static,
    {
        ArcParser(Arc::new(parser))
    }
}

impl Parser for ArcParser {
    type Error = Arc<dyn Error + Send + Sync>;
    type Output = Arc<dyn Any + Send + Sync>;
    type PartialState = Arc<dyn Any + Send + Sync>;

    fn parse<'a>(
        &self,
        state: &Self::PartialState,
        input: &'a [u8],
    ) -> Result<ParseResult<'a, Self::PartialState, Self::Output>, Self::Error> {
        let _self: &dyn Parser<
            Error = Arc<dyn Error + Send + Sync>,
            Output = Arc<dyn Any + Send + Sync>,
            PartialState = Arc<dyn Any + Send + Sync>,
        > = &self.0;
        _self.parse(state, input)
    }
}

/// A wrapper for a parser that implements an easily boxable version of Parser.
struct AnyParser<P>(P);

impl<P> Parser for AnyParser<P>
where
    P: Parser,
    P::Error: Error + Send + Sync + 'static,
    P::Output: Send + Sync + 'static,
    P::PartialState: Send + Sync + 'static,
{
    type Error = Arc<dyn Error + Send + Sync>;
    type Output = Arc<dyn Any + Sync + Send>;
    type PartialState = Arc<dyn Any + Sync + Send>;

    fn parse<'a>(
        &self,
        state: &Self::PartialState,
        input: &'a [u8],
    ) -> Result<ParseResult<'a, Self::PartialState, Self::Output>, Self::Error> {
        let state = state.downcast_ref::<P::PartialState>().ok_or_else(|| {
            struct StateIsNotOfTheCorrectType;
            impl std::fmt::Display for StateIsNotOfTheCorrectType {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "State is not of the correct type")
                }
            }
            impl std::fmt::Debug for StateIsNotOfTheCorrectType {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "State is not of the correct type")
                }
            }
            impl Error for StateIsNotOfTheCorrectType {}
            Arc::new(StateIsNotOfTheCorrectType) as Arc<dyn Error + Send + Sync>
        })?;
        match self.0.parse(state, input) {
            Ok(result) => Ok(result
                .map_state(|state| Arc::new(state) as Arc<dyn Any + Sync + Send>)
                .map(|output| Arc::new(output) as Arc<dyn Any + Sync + Send>)),
            Err(err) => Err(Arc::new(err)),
        }
    }
}

impl<P: CreateParserState> CreateParserState for AnyParser<P>
where
    P: Parser,
    P::Error: Error + Send + Sync + 'static,
    P::Output: Send + Sync + 'static,
    P::PartialState: Send + Sync + 'static,
{
    fn create_parser_state(&self) -> <Self as Parser>::PartialState {
        Arc::new(self.0.create_parser_state())
    }
}

/// An extension trait for parsers.
pub trait ParserExt: Parser {
    /// Parse this parser, or another other parser.
    fn or<V: Parser<Error = E, Output = O, PartialState = PA>, E, O, PA>(
        self,
        other: V,
    ) -> ChoiceParser<Self, V>
    where
        Self: Sized,
    {
        ChoiceParser {
            parser1: self,
            parser2: other,
        }
    }

    /// Parse this parser, then the other parser.
    fn then<V: Parser<Error = E, Output = O, PartialState = PA>, E, O, PA>(
        self,
        other: V,
    ) -> SequenceParser<Self, V>
    where
        Self: Sized,
    {
        SequenceParser::new(self, other)
    }

    /// Repeat this parser a number of times.
    fn repeat(self, length_range: std::ops::RangeInclusive<usize>) -> RepeatParser<Self>
    where
        Self: Sized,
    {
        RepeatParser::new(self, length_range)
    }

    /// Map the output of this parser.
    fn map_output<F: Fn(Self::Output) -> O, O>(self, f: F) -> MapOutputParser<Self, F, O>
    where
        Self: Sized,
    {
        MapOutputParser {
            parser: self,
            map: f,
            _output: std::marker::PhantomData,
        }
    }

    /// Get a boxed version of this parser.
    fn boxed(self) -> ArcParser
    where
        Self: CreateParserState + Sized + Send + Sync + 'static,
        Self::Error: Error + Send + Sync + 'static,
        Self::Output: Send + Sync + 'static,
        Self::PartialState: Send + Sync + 'static,
    {
        ArcParser::new(AnyParser(self))
    }
}

impl<P: Parser> ParserExt for P {}

/// A parser for a choice between two parsers.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum OwnedParseResult<P, R> {
    /// The parser is incomplete.
    Incomplete {
        /// The new state of the parser.
        new_state: P,
        /// The text that is required next.
        required_next: Cow<'static, str>,
    },
    /// The parser is finished.
    Finished {
        /// The result of the parser.
        result: R,
        /// The remaining input.
        remaining: Vec<u8>,
    },
}

impl<P, R> From<ParseResult<'_, P, R>> for OwnedParseResult<P, R> {
    fn from(result: ParseResult<P, R>) -> Self {
        match result {
            ParseResult::Incomplete {
                new_state,
                required_next,
            } => OwnedParseResult::Incomplete {
                new_state,
                required_next,
            },
            ParseResult::Finished { result, remaining } => OwnedParseResult::Finished {
                result,
                remaining: remaining.to_vec(),
            },
        }
    }
}

/// The state of a parser.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ParseResult<'a, P, R> {
    /// The parser is incomplete.
    Incomplete {
        /// The new state of the parser.
        new_state: P,
        /// The text that is required next.
        required_next: Cow<'static, str>,
    },
    /// The parser is finished.
    Finished {
        /// The result of the parser.
        result: R,
        /// The remaining input.
        remaining: &'a [u8],
    },
}

impl<'a, P, R> ParseResult<'a, P, R> {
    /// Take the remaining bytes from the parser.
    pub fn without_remaining(self) -> ParseResult<'static, P, R> {
        match self {
            ParseResult::Finished { result, .. } => ParseResult::Finished {
                result,
                remaining: &[],
            },
            ParseResult::Incomplete {
                new_state,
                required_next,
            } => ParseResult::Incomplete {
                new_state,
                required_next,
            },
        }
    }

    /// Unwrap the parser to a finished result.
    pub fn unwrap_finished(self) -> R {
        match self {
            ParseResult::Finished { result, .. } => result,
            ParseResult::Incomplete { .. } => {
                panic!("called `ParseResult::unwrap_finished()` on an `Incomplete` value")
            }
        }
    }

    /// Unwrap the parser to an incomplete result.
    pub fn unwrap_incomplete(self) -> (P, Cow<'static, str>) {
        match self {
            ParseResult::Finished { .. } => {
                panic!("called `ParseResult::unwrap_incomplete()` on a `Finished` value")
            }
            ParseResult::Incomplete {
                new_state,
                required_next,
            } => (new_state, required_next),
        }
    }

    /// Map the result of the parser.
    pub fn map<F, O>(self, f: F) -> ParseResult<'a, P, O>
    where
        F: FnOnce(R) -> O,
    {
        match self {
            ParseResult::Finished { result, remaining } => ParseResult::Finished {
                result: f(result),
                remaining,
            },
            ParseResult::Incomplete {
                new_state,
                required_next,
            } => ParseResult::Incomplete {
                new_state,
                required_next,
            },
        }
    }

    /// Map the state of the parser.
    pub fn map_state<F, O>(self, f: F) -> ParseResult<'a, O, R>
    where
        F: FnOnce(P) -> O,
    {
        match self {
            ParseResult::Finished { result, remaining } => {
                ParseResult::Finished { result, remaining }
            }
            ParseResult::Incomplete {
                new_state,
                required_next,
            } => ParseResult::Incomplete {
                new_state: f(new_state),
                required_next,
            },
        }
    }
}

/// A validator for a string
#[derive(Debug, Clone)]
pub enum StructureParser {
    /// A literal string
    Literal(String),
    /// A number
    Num {
        /// The minimum value of the number
        min: f64,
        /// The maximum value of the number
        max: f64,
        /// If the number must be an integer
        integer: bool,
    },
    /// Either the first or the second parser
    Either {
        /// The first parser
        first: Box<StructureParser>,
        /// The second parser
        second: Box<StructureParser>,
    },
    /// The first parser, then the second parser
    Then {
        /// The first parser
        first: Box<StructureParser>,
        /// The second parser
        second: Box<StructureParser>,
    },
}

/// The state of a structure parser.
#[allow(missing_docs)]
#[derive(Debug, PartialEq, Clone)]
pub enum StructureParserState {
    Literal(LiteralParserOffset),
    NumInt(IntegerParserState),
    Num(FloatParserState),
    Either(ChoiceParserState<Box<StructureParserState>, Box<StructureParserState>, (), ()>),
    Then(SequenceParserState<Box<StructureParserState>, Box<StructureParserState>, ()>),
}

impl CreateParserState for StructureParser {
    fn create_parser_state(&self) -> <Self as Parser>::PartialState {
        match self {
            StructureParser::Literal(literal) => {
                StructureParserState::Literal(LiteralParser::from(literal).create_parser_state())
            }
            StructureParser::Num { min, max, integer } => {
                if *integer {
                    StructureParserState::NumInt(
                        IntegerParser::new(*min as i128..=*max as i128).create_parser_state(),
                    )
                } else {
                    StructureParserState::Num(FloatParser::new(*min..=*max).create_parser_state())
                }
            }
            StructureParser::Either { first, second } => {
                StructureParserState::Either(ChoiceParserState::new(
                    Box::new(first.create_parser_state()),
                    Box::new(second.create_parser_state()),
                ))
            }
            StructureParser::Then { first, .. } => StructureParserState::Then(
                SequenceParserState::FirstParser(Box::new(first.create_parser_state())),
            ),
        }
    }
}

impl Parser for StructureParser {
    type Error = ();
    type Output = ();
    type PartialState = StructureParserState;

    fn parse<'a>(
        &self,
        state: &Self::PartialState,
        input: &'a [u8],
    ) -> Result<ParseResult<'a, Self::PartialState, Self::Output>, Self::Error> {
        match (self, state) {
            (StructureParser::Literal(lit_parser), StructureParserState::Literal(state)) => {
                LiteralParser::from(lit_parser)
                    .parse(state, input)
                    .map(|result| result.map(|_| ()).map_state(StructureParserState::Literal))
                    .map_err(|_| ())
            }
            (
                StructureParser::Num {
                    min,
                    max,
                    integer: false,
                },
                StructureParserState::Num(state),
            ) => FloatParser::new(*min..=*max)
                .parse(state, input)
                .map(|result| result.map(|_| ()).map_state(StructureParserState::Num)),
            (
                StructureParser::Num {
                    min,
                    max,
                    integer: true,
                },
                StructureParserState::NumInt(int),
            ) => IntegerParser::new(*min as i128..=*max as i128)
                .parse(int, input)
                .map(|result| result.map(|_| ()).map_state(StructureParserState::NumInt)),
            (StructureParser::Either { first, second }, StructureParserState::Either(state)) => {
                let state = ChoiceParserState {
                    state1: match &state.state1 {
                        Ok(state) => Ok((**state).clone()),
                        Err(()) => Err(()),
                    },
                    state2: match &state.state2 {
                        Ok(state) => Ok((**state).clone()),
                        Err(()) => Err(()),
                    },
                };
                let parser = ChoiceParser::new(first.clone(), second.clone());
                match parser.parse(&state, input) {
                    Ok(ParseResult::Incomplete { required_next, .. }) => {
                        Ok(ParseResult::Incomplete {
                            new_state: StructureParserState::Either(ChoiceParserState {
                                state1: state.state1.map(Box::new),
                                state2: state.state2.map(Box::new),
                            }),
                            required_next,
                        })
                    }
                    Ok(ParseResult::Finished { remaining, .. }) => Ok(ParseResult::Finished {
                        result: (),
                        remaining,
                    }),
                    Err(_) => Err(()),
                }
            }
            (StructureParser::Then { first, second }, StructureParserState::Then(state)) => {
                let state = SequenceParserState::FirstParser(match &state {
                    SequenceParserState::FirstParser(state) => (**state).clone(),
                    SequenceParserState::SecondParser(state, _) => (**state).clone(),
                });
                let parser = SequenceParser::new(first.clone(), second.clone());
                match parser.parse(&state, input) {
                    Ok(ParseResult::Incomplete { required_next, .. }) => {
                        Ok(ParseResult::Incomplete {
                            new_state: StructureParserState::Then(match state {
                                SequenceParserState::FirstParser(state) => {
                                    SequenceParserState::FirstParser(Box::new(state))
                                }
                                SequenceParserState::SecondParser(state, _) => {
                                    SequenceParserState::SecondParser(Box::new(state), ())
                                }
                            }),
                            required_next,
                        })
                    }
                    Ok(ParseResult::Finished { remaining, .. }) => Ok(ParseResult::Finished {
                        result: (),
                        remaining,
                    }),
                    Err(_) => Err(()),
                }
            }
            _ => unreachable!(),
        }
    }
}
