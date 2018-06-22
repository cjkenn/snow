#[derive(Debug, PartialEq, Clone)]
pub enum TknTy {
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Semicolon,
    Eq,
    Lt,
    Gt,
    Period,
    Comma,
    Bang,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Tilde,

    // Multi character tokens
    EqEq,
    LtEq,
    GtEq,
    BangEq,
    AmpAmp,
    PipePipe,

    // Identifiers/literals
    Ident(String),
    Str(String),
    Val(f64),

    // Keywords
    Let,
    Imm,
    Func,
    Return,
    Class,
    This,
    If,
    Elif,
    Then,
    Else,
    While,
    In,
    For,
    Num,
    String,
    Bool,
    True,
    False,
    Or,
    And,
    Null,

    Eof
}

impl TknTy {
    pub fn is_bin_op(&self) -> bool {
        match self {
            TknTy::Plus |
            TknTy::Minus |
            TknTy::Star |
            TknTy::Slash |
            TknTy::Percent |
            TknTy::EqEq |
            TknTy::BangEq |
            TknTy::Gt |
            TknTy::Lt |
            TknTy::GtEq |
            TknTy::LtEq => true,
            _ => false
        }
    }

    pub fn is_numerical_op(&self) -> bool {
        match self {
            TknTy::Plus |
            TknTy::Minus |
            TknTy::Star |
            TknTy::Slash |
            TknTy::Percent => true,
            _ => false
        }
    }

    pub fn is_cmp_op(&self) -> bool {
        match self {
            TknTy::EqEq |
            TknTy::BangEq |
            TknTy::Gt |
            TknTy::Lt |
            TknTy::GtEq |
            TknTy::LtEq => true,
            _ => false
        }
    }

    pub fn is_logical_op(&self) -> bool {
        match self {
            TknTy::AmpAmp | TknTy::PipePipe | TknTy::Or | TknTy::And => true,
             _ => false
        }
    }

    pub fn is_unary_op(&self) -> bool {
        match self {
            TknTy::Minus | TknTy::Bang => true,
            _ => false
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Token {
    pub ty: TknTy,
    pub line: usize,
    pub pos: usize
}

impl Token {
    pub fn new(ty: TknTy, line: usize, pos: usize) -> Token {
        Token {
            ty: ty,
            line: line,
            pos: pos
        }
    }

    pub fn is_ty(&self) -> bool {
        self.ty == TknTy::Num ||
            self.ty == TknTy::String ||
            self.ty == TknTy::Bool
    }

    pub fn get_name(&self) -> String {
        match self.ty {
            TknTy::Ident(ref name) => name.to_string(),
            _ => "".to_string()
        }
    }

    pub fn is_ident(&self) -> bool {
        match self.ty {
            TknTy::Ident(_) => true,
            _ => false
        }
    }
}
