use chumsky::prelude::*;

use super::token::Token;

/// Build a chumsky lexer that converts source text to a token stream.
pub fn lexer<'src>()
-> impl Parser<'src, &'src str, Vec<(Token<'src>, SimpleSpan)>, extra::Err<Rich<'src, char>>> {
    let int = {
        // Hex literals: 0x[0-9a-fA-F_]+
        let hex = just("0x").ignore_then(
            any()
                .filter(|c: &char| c.is_ascii_hexdigit() || *c == '_')
                .repeated()
                .at_least(1)
                .to_slice()
                .map(|s: &str| {
                    let stripped: String = s.chars().filter(|c| *c != '_').collect();
                    u64::from_str_radix(&stripped, 16).unwrap_or(0)
                }),
        );

        // Decimal literals: [0-9][0-9_]*
        let dec = any()
            .filter(|c: &char| c.is_ascii_digit())
            .then(
                any()
                    .filter(|c: &char| c.is_ascii_digit() || *c == '_')
                    .repeated(),
            )
            .to_slice()
            .map(|s: &str| {
                let stripped: String = s.chars().filter(|c| *c != '_').collect();
                stripped.parse::<u64>().unwrap_or(0)
            });

        hex.or(dec).map(Token::Int)
    };

    // Identifiers and keywords: [a-zA-Z_][a-zA-Z0-9_']*
    let ident = any()
        .filter(|c: &char| c.is_ascii_alphabetic() || *c == '_')
        .then(
            any()
                .filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_' || *c == '\'')
                .repeated(),
        )
        .to_slice()
        .map(|s: &str| match s {
            "process" => Token::Process,
            "pipe" => Token::Pipe,
            "type" => Token::Type,
            "record" => Token::Record,
            "enum" => Token::Enum,
            "rule" => Token::Rule,
            "let" => Token::Let,
            "match" => Token::Match,
            "if" => Token::If,
            "then" => Token::Then,
            "else" => Token::Else,
            "consumes" => Token::Consumes,
            "produces" => Token::Produces,
            "state" => Token::State,
            "peeks" => Token::Peeks,
            "external" => Token::External,
            "Queue" => Token::Queue,
            "Cell" => Token::Cell,
            "Memory" => Token::Memory,
            "Some" => Token::SomeKw,
            "None" => Token::NoneKw,
            "true" => Token::True,
            "false" => Token::False,
            "init" => Token::Init,
            "depth" => Token::Depth,
            "_" => Token::Underscore,
            _ => Token::Ident(s),
        });

    // Multi-char operators (must be tried before single-char)
    let op = choice((
        just("=>").to(Token::Arrow),
        just(":=").to(Token::ColonEq),
        just("==").to(Token::Eq),
        just("!=").to(Token::Neq),
        just("<=").to(Token::Le),
        just(">=").to(Token::Ge),
        just("<<").to(Token::Shl),
        just(">>").to(Token::Shr),
        just("&&").to(Token::LogicalAnd),
        just("||").to(Token::LogicalOr),
    ));

    // Single-char operators and punctuation
    let single = choice((
        just('+').to(Token::Plus),
        just('-').to(Token::Minus),
        just('*').to(Token::Star),
        just('&').to(Token::Ampersand),
        just('|').to(Token::Pipe_),
        just('^').to(Token::Caret),
        just('!').to(Token::Bang),
        just('<').to(Token::Lt),
        just('>').to(Token::Gt),
        just('.').to(Token::Dot),
        just(':').to(Token::Colon),
        just('=').to(Token::Assign),
        just(',').to(Token::Comma),
        just('(').to(Token::LParen),
        just(')').to(Token::RParen),
        just('[').to(Token::LBrack),
        just(']').to(Token::RBrack),
        just('{').to(Token::LBrace),
        just('}').to(Token::RBrace),
    ));

    // × (Unicode MULTIPLICATION SIGN U+00D7)
    let times = just('\u{00D7}').to(Token::Times);

    // Comments: -- to end of line (ignored)
    let comment = just("--")
        .then(any().and_is(just('\n').not()).repeated())
        .ignored();

    // Whitespace
    let whitespace = any().filter(|c: &char| c.is_whitespace()).ignored();

    let token = choice((int, op, times, single, ident));

    // Padding: any mix of whitespace and comments
    let padding = whitespace.or(comment).repeated();

    token
        .map_with(|tok, e| (tok, e.span()))
        .padded_by(padding)
        .repeated()
        .collect::<Vec<_>>()
        .padded_by(padding)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<Token<'_>> {
        let (tokens, errs) = lexer().parse(src).into_output_errors();
        assert!(errs.is_empty(), "lex errors: {errs:?}");
        tokens.unwrap().into_iter().map(|(tok, _)| tok).collect()
    }

    #[test]
    fn keywords() {
        let tokens = lex("process pipe type record enum rule let match if then else");
        assert_eq!(
            tokens,
            vec![
                Token::Process,
                Token::Pipe,
                Token::Type,
                Token::Record,
                Token::Enum,
                Token::Rule,
                Token::Let,
                Token::Match,
                Token::If,
                Token::Then,
                Token::Else,
            ]
        );
    }

    #[test]
    fn integers() {
        let tokens = lex("42 0xFF 0x8000_0000 1_000");
        assert_eq!(
            tokens,
            vec![
                Token::Int(42),
                Token::Int(0xFF),
                Token::Int(0x8000_0000),
                Token::Int(1000),
            ]
        );
    }

    #[test]
    fn operators_and_punctuation() {
        let tokens = lex("=> := == != <= >= << >> && || + - * . : = , ( ) [ ] { }");
        assert_eq!(
            tokens,
            vec![
                Token::Arrow,
                Token::ColonEq,
                Token::Eq,
                Token::Neq,
                Token::Le,
                Token::Ge,
                Token::Shl,
                Token::Shr,
                Token::LogicalAnd,
                Token::LogicalOr,
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Dot,
                Token::Colon,
                Token::Assign,
                Token::Comma,
                Token::LParen,
                Token::RParen,
                Token::LBrack,
                Token::RBrack,
                Token::LBrace,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn ident_with_prime() {
        let tokens = lex("regs'");
        assert_eq!(tokens, vec![Token::Ident("regs'")]);
    }

    #[test]
    fn comments_stripped() {
        let tokens = lex("let x -- this is a comment\nlet y");
        assert_eq!(
            tokens,
            vec![Token::Let, Token::Ident("x"), Token::Let, Token::Ident("y")]
        );
    }

    #[test]
    fn times_unicode() {
        let tokens = lex("Addr × Word");
        assert_eq!(
            tokens,
            vec![Token::Ident("Addr"), Token::Times, Token::Ident("Word")]
        );
    }

    #[test]
    fn underscore_is_token() {
        let tokens = lex("_ foo _bar");
        assert_eq!(
            tokens,
            vec![Token::Underscore, Token::Ident("foo"), Token::Ident("_bar")]
        );
    }
}
