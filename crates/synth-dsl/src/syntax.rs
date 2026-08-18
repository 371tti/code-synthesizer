use super::CompileError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LiteralUnit {
    Plain,
    Seconds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Literal {
    pub value: f32,
    pub unit: LiteralUnit,
    pub integral_without_suffix: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) struct ProgramAst {
    pub note_output_layout: ChannelLayout,
    pub effect_input_layout: Option<ChannelLayout>,
    pub effect_output_layout: Option<ChannelLayout>,
    pub parameters: Vec<ParameterDecl>,
    pub functions: Vec<Function>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChannelLayout {
    Mono,
    Stereo,
}

#[derive(Clone, Debug)]
pub(crate) struct ParameterDecl {
    pub name: String,
    pub arguments: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) struct Function {
    pub name: String,
    pub parameters: Vec<String>,
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) enum Statement {
    ScalarStorage {
        domain: String,
        name: String,
        initializer: Expr,
        span: Span,
    },
    RingStorage {
        domain: String,
        name: String,
        size: Literal,
        span: Span,
    },
    Assignment {
        targets: Vec<String>,
        value: Expr,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Literal(Literal),
    Name(String, Span),
    Call {
        name: String,
        arguments: Vec<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        value: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    pub(crate) fn span(&self) -> Span {
        match self {
            Self::Literal(value) => value.span,
            Self::Name(_, span)
            | Self::Call { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. } => *span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Positive,
    Negative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
}

impl BinaryOp {
    pub(crate) fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Less
                | Self::LessEqual
                | Self::Greater
                | Self::GreaterEqual
                | Self::Equal
                | Self::NotEqual
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(Literal),
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    EqualEqual,
    NotEqual,
    Comma,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Arrow,
    Newline,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    span: Span,
}

pub(crate) fn parse(source: &str) -> Result<ProgramAst, CompileError> {
    Parser::new(lex(source)?).parse_program()
}

fn lex(source: &str) -> Result<Vec<Token>, CompileError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;
    let mut column = 1;
    while index < chars.len() {
        let span = Span { line, column };
        match chars[index] {
            ' ' | '\t' | '\r' => {
                index += 1;
                column += 1;
            }
            '\n' => {
                tokens.push(Token {
                    kind: TokenKind::Newline,
                    span,
                });
                index += 1;
                line += 1;
                column = 1;
            }
            '#' => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                    column += 1;
                }
            }
            '/' if chars.get(index + 1) == Some(&'/') => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                    column += 1;
                }
            }
            '(' => push_simple(
                &mut tokens,
                TokenKind::LeftParen,
                span,
                &mut index,
                &mut column,
            ),
            ')' => push_simple(
                &mut tokens,
                TokenKind::RightParen,
                span,
                &mut index,
                &mut column,
            ),
            '{' => push_simple(
                &mut tokens,
                TokenKind::LeftBrace,
                span,
                &mut index,
                &mut column,
            ),
            '}' => push_simple(
                &mut tokens,
                TokenKind::RightBrace,
                span,
                &mut index,
                &mut column,
            ),
            ',' => push_simple(&mut tokens, TokenKind::Comma, span, &mut index, &mut column),
            '.' if !chars
                .get(index + 1)
                .is_some_and(|value| value.is_ascii_digit()) =>
            {
                push_simple(&mut tokens, TokenKind::Dot, span, &mut index, &mut column);
            }
            '+' => push_simple(&mut tokens, TokenKind::Plus, span, &mut index, &mut column),
            '*' => push_simple(&mut tokens, TokenKind::Star, span, &mut index, &mut column),
            '%' => push_simple(
                &mut tokens,
                TokenKind::Percent,
                span,
                &mut index,
                &mut column,
            ),
            '^' => push_simple(&mut tokens, TokenKind::Caret, span, &mut index, &mut column),
            '-' if chars.get(index + 1) == Some(&'>') => {
                tokens.push(Token {
                    kind: TokenKind::Arrow,
                    span,
                });
                index += 2;
                column += 2;
            }
            '-' => push_simple(&mut tokens, TokenKind::Minus, span, &mut index, &mut column),
            '/' => push_simple(&mut tokens, TokenKind::Slash, span, &mut index, &mut column),
            '<' if chars.get(index + 1) == Some(&'=') => {
                tokens.push(Token {
                    kind: TokenKind::LessEqual,
                    span,
                });
                index += 2;
                column += 2;
            }
            '<' => push_simple(&mut tokens, TokenKind::Less, span, &mut index, &mut column),
            '>' if chars.get(index + 1) == Some(&'=') => {
                tokens.push(Token {
                    kind: TokenKind::GreaterEqual,
                    span,
                });
                index += 2;
                column += 2;
            }
            '>' => push_simple(
                &mut tokens,
                TokenKind::Greater,
                span,
                &mut index,
                &mut column,
            ),
            '=' if chars.get(index + 1) == Some(&'=') => {
                tokens.push(Token {
                    kind: TokenKind::EqualEqual,
                    span,
                });
                index += 2;
                column += 2;
            }
            '=' => push_simple(&mut tokens, TokenKind::Equal, span, &mut index, &mut column),
            '!' if chars.get(index + 1) == Some(&'=') => {
                tokens.push(Token {
                    kind: TokenKind::NotEqual,
                    span,
                });
                index += 2;
                column += 2;
            }
            value
                if value.is_ascii_digit()
                    || (value == '.'
                        && chars
                            .get(index + 1)
                            .is_some_and(|next| next.is_ascii_digit())) =>
            {
                let start = index;
                let start_column = column;
                if value == '.' {
                    index += 1;
                    column += 1;
                }
                while chars.get(index).is_some_and(|value| value.is_ascii_digit()) {
                    index += 1;
                    column += 1;
                }
                if chars.get(index) == Some(&'.') {
                    index += 1;
                    column += 1;
                    while chars.get(index).is_some_and(|value| value.is_ascii_digit()) {
                        index += 1;
                        column += 1;
                    }
                }
                if chars
                    .get(index)
                    .is_some_and(|value| matches!(value, 'e' | 'E'))
                {
                    index += 1;
                    column += 1;
                    if chars
                        .get(index)
                        .is_some_and(|value| matches!(value, '+' | '-'))
                    {
                        index += 1;
                        column += 1;
                    }
                    let exponent_start = index;
                    while chars.get(index).is_some_and(|value| value.is_ascii_digit()) {
                        index += 1;
                        column += 1;
                    }
                    if exponent_start == index {
                        return Err(CompileError::new("指数部に数字が必要です", line, column));
                    }
                }
                let number_end = index;
                while chars
                    .get(index)
                    .is_some_and(|value| value.is_ascii_alphabetic())
                {
                    index += 1;
                    column += 1;
                }
                let raw: String = chars[start..number_end].iter().collect();
                let suffix: String = chars[number_end..index].iter().collect();
                let parsed = raw.parse::<f32>().map_err(|_| {
                    CompileError::new("数値リテラルを解釈できません", line, start_column)
                })?;
                let (scale, unit) = match suffix.as_str() {
                    "" => (1.0, LiteralUnit::Plain),
                    "s" => (1.0, LiteralUnit::Seconds),
                    "ms" => (1.0e-3, LiteralUnit::Seconds),
                    "us" => (1.0e-6, LiteralUnit::Seconds),
                    "k" => (1.0e3, LiteralUnit::Plain),
                    "m" => (1.0e-3, LiteralUnit::Plain),
                    "u" => (1.0e-6, LiteralUnit::Plain),
                    "g" => (1.0e9, LiteralUnit::Plain),
                    _ => {
                        return Err(CompileError::new(
                            format!("未対応の数値suffixです: {suffix}"),
                            line,
                            start_column,
                        )
                        .with_hint("使用できるsuffixは s, ms, us, k, m, u, g です"));
                    }
                };
                let value = parsed * scale;
                if !value.is_finite() {
                    return Err(CompileError::new(
                        "数値リテラルがf32の有限範囲を超えています",
                        line,
                        start_column,
                    ));
                }
                tokens.push(Token {
                    kind: TokenKind::Number(Literal {
                        value,
                        unit,
                        integral_without_suffix: suffix.is_empty()
                            && !raw.contains('.')
                            && !raw.contains('e')
                            && !raw.contains('E'),
                        span,
                    }),
                    span,
                });
            }
            value if is_ident_start(value) => {
                let start = index;
                while chars
                    .get(index)
                    .is_some_and(|value| is_ident_continue(*value))
                {
                    index += 1;
                    column += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Ident(chars[start..index].iter().collect()),
                    span,
                });
            }
            value => {
                return Err(CompileError::new(
                    format!("使用できない文字です: {value}"),
                    line,
                    column,
                ));
            }
        }
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span { line, column },
    });
    Ok(tokens)
}

fn push_simple(
    tokens: &mut Vec<Token>,
    kind: TokenKind,
    span: Span,
    index: &mut usize,
    column: &mut usize,
) {
    tokens.push(Token { kind, span });
    *index += 1;
    *column += 1;
}

fn is_ident_start(value: char) -> bool {
    value == '_' || value.is_ascii_alphabetic()
}

fn is_ident_continue(value: char) -> bool {
    value == '_' || value.is_ascii_alphanumeric()
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse_program(mut self) -> Result<ProgramAst, CompileError> {
        let mut note_output_layout = None;
        let mut effect_input_layout = None;
        let mut effect_output_layout = None;
        let mut parameters = Vec::new();
        let mut functions: Vec<Function> = Vec::new();

        self.skip_newlines();

        while !self.at_eof() {
            if self.at_ident("mode") {
                return Err(self.error_here(
                "`mode mono/stereo` は廃止されました。note.out.layout = mono|stereo を指定してください",
            ));
            } else if self.at_ident("note") || self.at_ident("effect") {
                let statement = self.parse_assignment()?;

                let Statement::Assignment {
                    targets,
                    value,
                    span,
                } = statement
                else {
                    unreachable!();
                };

                if targets.len() != 1 {
                    return Err(CompileError::new(
                        "layout宣言は1つのfieldへ代入してください",
                        span.line,
                        span.column,
                    ));
                }

                let Expr::Name(value, _) = value else {
                    return Err(CompileError::new(
                        "layoutには mono または stereo を指定してください",
                        span.line,
                        span.column,
                    ));
                };

                let layout = match value.as_str() {
                    "mono" => ChannelLayout::Mono,
                    "stereo" => ChannelLayout::Stereo,
                    _ => {
                        return Err(CompileError::new(
                            "layoutには mono または stereo を指定してください",
                            span.line,
                            span.column,
                        ));
                    }
                };

                let target = &targets[0];

                let destination = match target.as_str() {
                    "note.out.layout"
                        if !functions.iter().any(|function| function.name == "note") =>
                    {
                        &mut note_output_layout
                    }

                    "effect.in.layout" | "effect.out.layout"
                        if !functions.iter().any(|function| function.name == "effect") =>
                    {
                        if target == "effect.in.layout" {
                            &mut effect_input_layout
                        } else {
                            &mut effect_output_layout
                        }
                    }

                    "note.out.layout" => {
                        return Err(CompileError::new(
                            "note.out.layout は fn note より前に置く必要があります",
                            span.line,
                            span.column,
                        ));
                    }

                    "effect.in.layout" | "effect.out.layout" => {
                        return Err(CompileError::new(
                            "effect layout は fn effect より前に置く必要があります",
                            span.line,
                            span.column,
                        ));
                    }

                    _ => {
                        return Err(CompileError::new(
                            "layout宣言は note.out.layout / effect.in.layout / effect.out.layout のいずれかです",
                            span.line,
                            span.column,
                        ));
                    }
                };

                if destination.replace(layout).is_some() {
                    return Err(CompileError::new(
                        format!("{target} は1つだけ宣言できます"),
                        span.line,
                        span.column,
                    ));
                }
            } else if self.at_ident("fn") {
                functions.push(self.parse_function()?);
            } else {
                // p.* = param(...) はトップレベルなら位置を問わない。
                let statement = self.parse_assignment()?;

                let Statement::Assignment {
                    targets,
                    value,
                    span,
                } = statement
                else {
                    unreachable!();
                };

                if targets.len() != 1 || !targets[0].starts_with("p.") {
                    return Err(CompileError::new(
                        "トップレベルには p.name = param(...) だけを宣言できます",
                        span.line,
                        span.column,
                    ));
                }

                let Expr::Call {
                    name, arguments, ..
                } = value
                else {
                    return Err(CompileError::new(
                        "parameter宣言の右辺には param(...) が必要です",
                        span.line,
                        span.column,
                    ));
                };

                if name != "param" {
                    return Err(CompileError::new(
                        "parameter宣言の右辺には param(...) が必要です",
                        span.line,
                        span.column,
                    ));
                }

                parameters.push(ParameterDecl {
                    name: targets.into_iter().next().unwrap(),
                    arguments,
                    span,
                });
            }

            self.skip_newlines();
        }

        let note_output_layout = if functions.iter().any(|f| f.name == "note") {
            note_output_layout.ok_or_else(|| {
                CompileError::new(
                    "最初のfnより前に `note.out.layout = mono` または `stereo` を宣言してください",
                    1,
                    1,
                )
            })?
        } else {
            // If there's no `fn note`, the note output layout is irrelevant; default to mono.
            note_output_layout.unwrap_or(ChannelLayout::Mono)
        };

        Ok(ProgramAst {
            note_output_layout,
            effect_input_layout,
            effect_output_layout,
            parameters,
            functions,
        })
    }

    fn parse_function(&mut self) -> Result<Function, CompileError> {
        let span = self.current().span;
        self.expect_ident("fn")?;
        let name = self.parse_qualified_name()?;
        self.expect_simple(TokenKind::LeftParen, "関数名の後に '(' が必要です")?;
        self.skip_newlines();
        let mut parameters = Vec::new();
        if !self.at_simple(&TokenKind::RightParen) {
            loop {
                parameters.push(self.take_ident("引数名が必要です")?);
                self.skip_newlines();
                if !self.consume_simple(&TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
            }
        }
        self.expect_simple(TokenKind::RightParen, "引数リストを ')' で閉じてください")?;
        self.skip_newlines();
        self.expect_simple(TokenKind::Arrow, "関数には '-> out' が必要です")?;
        self.skip_newlines();
        self.expect_ident("out")?;
        self.skip_newlines();
        self.expect_simple(TokenKind::LeftBrace, "関数bodyを '{' で開始してください")?;
        self.skip_newlines();
        let mut statements = Vec::new();
        while !self.at_simple(&TokenKind::RightBrace) {
            if self.at_eof() {
                return Err(self.error_here("関数bodyを閉じる '}' がありません"));
            }
            statements.push(self.parse_statement()?);
            self.skip_newlines();
        }
        self.index += 1;
        Ok(Function {
            name,
            parameters,
            statements,
            span,
        })
    }

    fn parse_statement(&mut self) -> Result<Statement, CompileError> {
        if self.at_ident("f32") {
            return self.parse_scalar_storage();
        }
        if self.at_ident("RingBuf") {
            return self.parse_ring_storage();
        }
        self.parse_assignment()
    }

    fn parse_scalar_storage(&mut self) -> Result<Statement, CompileError> {
        let span = self.current().span;
        self.expect_ident("f32")?;
        let domain = self.take_ident("storage domainが必要です")?;
        let name = self.parse_qualified_name()?;
        if !self.consume_simple(&TokenKind::Equal) {
            return Err(CompileError::new(
                "scalar storageには初期値が必要です",
                span.line,
                span.column,
            )
            .with_hint("例: f32 voice phase = 0"));
        }
        let initializer = self.parse_expression(0)?;
        Ok(Statement::ScalarStorage {
            domain,
            name,
            initializer,
            span,
        })
    }

    fn parse_ring_storage(&mut self) -> Result<Statement, CompileError> {
        let span = self.current().span;
        self.expect_ident("RingBuf")?;
        self.expect_simple(TokenKind::Less, "RingBufの型引数に '<' が必要です")?;
        self.expect_ident("f32")?;
        self.expect_simple(TokenKind::Comma, "RingBuf<f32, Size> の ',' が必要です")?;
        self.skip_newlines();
        let size = match self.current().kind {
            TokenKind::Number(value) => {
                self.index += 1;
                value
            }
            _ => return Err(self.error_here("RingBuf容量には正の数値が必要です")),
        };
        self.skip_newlines();
        self.expect_simple(TokenKind::Greater, "RingBuf型を '>' で閉じてください")?;
        let domain = self.take_ident("storage domainが必要です")?;
        let name = self.parse_qualified_name()?;
        Ok(Statement::RingStorage {
            domain,
            name,
            size,
            span,
        })
    }

    fn parse_assignment(&mut self) -> Result<Statement, CompileError> {
        let span = self.current().span;
        let mut targets = vec![self.parse_qualified_name()?];
        self.expect_simple(TokenKind::Equal, "代入には '=' が必要です")?;
        self.skip_newlines();
        while self.qualified_name_followed_by_equal() {
            targets.push(self.parse_qualified_name()?);
            self.expect_simple(TokenKind::Equal, "代入には '=' が必要です")?;
            self.skip_newlines();
        }
        let value = self.parse_expression(0)?;
        Ok(Statement::Assignment {
            targets,
            value,
            span,
        })
    }

    fn parse_expression(&mut self, minimum_binding: u8) -> Result<Expr, CompileError> {
        self.skip_newlines();
        let mut left = match self.current().kind {
            TokenKind::Plus | TokenKind::Minus => {
                let span = self.current().span;
                let op = if matches!(self.current().kind, TokenKind::Plus) {
                    UnaryOp::Positive
                } else {
                    UnaryOp::Negative
                };
                self.index += 1;
                Expr::Unary {
                    op,
                    value: Box::new(self.parse_expression(13)?),
                    span,
                }
            }
            TokenKind::Number(value) => {
                self.index += 1;
                Expr::Literal(value)
            }
            TokenKind::LeftParen => {
                self.index += 1;
                let value = self.parse_expression(0)?;
                self.expect_simple(TokenKind::RightParen, "式を ')' で閉じてください")?;
                value
            }
            TokenKind::Ident(_) => {
                let span = self.current().span;
                let name = self.parse_qualified_name()?;
                self.skip_newlines();
                if self.consume_simple(&TokenKind::LeftParen) {
                    let mut arguments = Vec::new();
                    self.skip_newlines();
                    if !self.at_simple(&TokenKind::RightParen) {
                        loop {
                            arguments.push(self.parse_expression(0)?);
                            self.skip_newlines();
                            if !self.consume_simple(&TokenKind::Comma) {
                                break;
                            }
                            self.skip_newlines();
                        }
                    }
                    self.expect_simple(
                        TokenKind::RightParen,
                        "関数呼び出しを ')' で閉じてください",
                    )?;
                    Expr::Call {
                        name,
                        arguments,
                        span,
                    }
                } else {
                    Expr::Name(name, span)
                }
            }
            _ => return Err(self.error_here("式が必要です")),
        };

        loop {
            self.skip_newlines();
            let Some((op, left_binding, right_binding)) = self.binary_operator() else {
                break;
            };
            if left_binding < minimum_binding {
                break;
            }
            let span = self.current().span;
            self.index += 1;
            let right = self.parse_expression(right_binding)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn binary_operator(&self) -> Option<(BinaryOp, u8, u8)> {
        Some(match self.current().kind {
            TokenKind::EqualEqual => (BinaryOp::Equal, 2, 3),
            TokenKind::NotEqual => (BinaryOp::NotEqual, 2, 3),
            TokenKind::Less => (BinaryOp::Less, 4, 5),
            TokenKind::LessEqual => (BinaryOp::LessEqual, 4, 5),
            TokenKind::Greater => (BinaryOp::Greater, 4, 5),
            TokenKind::GreaterEqual => (BinaryOp::GreaterEqual, 4, 5),
            TokenKind::Plus => (BinaryOp::Add, 6, 7),
            TokenKind::Minus => (BinaryOp::Subtract, 6, 7),
            TokenKind::Star => (BinaryOp::Multiply, 8, 9),
            TokenKind::Slash => (BinaryOp::Divide, 8, 9),
            TokenKind::Percent => (BinaryOp::Modulo, 8, 9),
            TokenKind::Caret => (BinaryOp::Power, 12, 11),
            _ => return None,
        })
    }

    fn qualified_name_followed_by_equal(&self) -> bool {
        let mut index = self.index;
        if !matches!(self.tokens[index].kind, TokenKind::Ident(_)) {
            return false;
        }
        index += 1;
        while matches!(self.tokens[index].kind, TokenKind::Dot) {
            index += 1;
            if !matches!(self.tokens[index].kind, TokenKind::Ident(_)) {
                return false;
            }
            index += 1;
        }
        while matches!(self.tokens[index].kind, TokenKind::Newline) {
            index += 1;
        }
        matches!(self.tokens[index].kind, TokenKind::Equal)
    }

    fn parse_qualified_name(&mut self) -> Result<String, CompileError> {
        self.skip_newlines();
        let mut name = self.take_ident("名前が必要です")?;
        while self.consume_simple(&TokenKind::Dot) {
            name.push('.');
            name.push_str(&self.take_ident("'.' の後に名前が必要です")?);
        }
        Ok(name)
    }

    fn expect_ident(&mut self, expected: &str) -> Result<(), CompileError> {
        self.skip_newlines();
        if self.at_ident(expected) {
            self.index += 1;
            Ok(())
        } else {
            Err(self.error_here(format!("'{expected}' が必要です")))
        }
    }

    fn take_ident(&mut self, message: &str) -> Result<String, CompileError> {
        self.skip_newlines();
        match &self.current().kind {
            TokenKind::Ident(value) => {
                let value = value.clone();
                self.index += 1;
                Ok(value)
            }
            _ => Err(self.error_here(message)),
        }
    }

    fn expect_simple(&mut self, expected: TokenKind, message: &str) -> Result<(), CompileError> {
        self.skip_newlines();
        if self.at_simple(&expected) {
            self.index += 1;
            Ok(())
        } else {
            Err(self.error_here(message))
        }
    }

    fn consume_simple(&mut self, expected: &TokenKind) -> bool {
        self.skip_newlines();
        if self.at_simple(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn at_simple(&self, expected: &TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(expected)
    }

    fn at_ident(&self, expected: &str) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(value) if value == expected)
    }

    fn at_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.current().kind, TokenKind::Newline) {
            self.index += 1;
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn error_here(&self, message: impl Into<String>) -> CompileError {
        CompileError::new(
            message,
            self.current().span.line,
            self.current().span.column,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_parameters_functions_storage_and_chain_assignment() {
        let ast = parse(
            r#"
note.out.layout = stereo

p.cutoff = param(2k, 20, 20k, 1, 74)

fn note(in, p) -> out {
    f32 voice phase = 0
    RingBuf<f32, 180ms> voice delay
    out.wave_l = out.wave_r =
        sin(TAU * phase)
    out.l_limit = 1s
}
"#,
        )
        .unwrap();
        assert_eq!(ast.parameters.len(), 1);
        assert_eq!(ast.functions.len(), 1);
        assert_eq!(ast.functions[0].statements.len(), 4);
    }
}
