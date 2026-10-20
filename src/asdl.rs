macro_rules! emit {
    ($w:ident, "") => {
        $w.push(String::new())
    };
    ($w:ident, $lit:literal) => {
        $w.push(format!($lit))
    };
    ($w:ident, $fmt:literal, $($arg:tt)*) => {
        $w.push(format!($fmt, $($arg)*))
    };
}

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::Path;

pub(crate) const BUILTIN_TYPES: &[&str] = &["identifier", "string", "int", "constant"];
pub(crate) const KNOWN_ANNOTATIONS: &[&str] = &["stable"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Module {
    pub(crate) name: String,
    pub(crate) version: u32,
    pub(crate) dfns: Vec<Type>,
    pub(crate) types: HashMap<String, TypeKind>,
}

impl Module {
    pub(crate) fn new(name: String, version: u32, dfns: Vec<Type>) -> Self {
        let types = dfns
            .iter()
            .map(|t| (t.name.clone(), t.value.clone()))
            .collect();
        Self {
            name,
            version,
            dfns,
            types,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Type {
    pub(crate) name: String,
    pub(crate) value: TypeKind,
    pub(crate) annotations: Vec<String>,
}

impl Type {
    pub(crate) fn new(name: String, value: TypeKind, annotations: Vec<String>) -> Self {
        Self {
            name,
            value,
            annotations,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeKind {
    Sum(Sum),
    Product(Product),
}

impl TypeKind {
    pub(crate) fn attributes(&self) -> &[Field] {
        match self {
            TypeKind::Sum(s) => &s.attributes,
            TypeKind::Product(p) => &p.attributes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Constructor {
    pub(crate) name: String,
    pub(crate) fields: Vec<Field>,
}

impl Constructor {
    pub(crate) fn new(name: String, fields: Vec<Field>) -> Self {
        Self { name, fields }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quantifier {
    Optional,
    Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    pub(crate) ty: String,
    pub(crate) name: Option<String>,
    pub(crate) seq: bool,
    pub(crate) opt: bool,
    pub(crate) quantifiers: Vec<Quantifier>,
}

impl Field {
    pub(crate) fn new(ty: String, name: Option<String>, quantifiers: Vec<Quantifier>) -> Self {
        let seq = quantifiers.last() == Some(&Quantifier::Sequence);
        let opt = quantifiers.last() == Some(&Quantifier::Optional);
        Self {
            ty,
            name,
            seq,
            opt,
            quantifiers,
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ty)?;
        for q in &self.quantifiers {
            match q {
                Quantifier::Sequence => write!(f, "*")?,
                Quantifier::Optional => write!(f, "?")?,
            }
        }
        match &self.name {
            Some(name) => write!(f, " {name}"),
            None => write!(f, " None"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sum {
    pub(crate) types: Vec<Constructor>,
    pub(crate) attributes: Vec<Field>,
}

impl Sum {
    pub(crate) fn new(types: Vec<Constructor>, attributes: Vec<Field>) -> Self {
        Self { types, attributes }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Product {
    pub(crate) fields: Vec<Field>,
    pub(crate) attributes: Vec<Field>,
}

impl Product {
    pub(crate) fn new(fields: Vec<Field>, attributes: Vec<Field>) -> Self {
        Self { fields, attributes }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ASDLSyntaxError {
    pub(crate) msg: String,
    pub(crate) lineno: Option<usize>,
}

impl ASDLSyntaxError {
    pub(crate) fn new(msg: impl Into<String>, lineno: Option<usize>) -> Self {
        Self {
            msg: msg.into(),
            lineno,
        }
    }
}

impl fmt::Display for ASDLSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.lineno {
            Some(line) => write!(f, "Syntax error on line {line}: {}", self.msg),
            None => write!(f, "Syntax error on line <unknown>: {}", self.msg),
        }
    }
}

impl std::error::Error for ASDLSyntaxError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    ConstructorId,
    TypeId,
    Int,
    Equals,
    Comma,
    Question,
    Pipe,
    Asterisk,
    LParen,
    RParen,
    LBrace,
    RBrace,
    At,
}

impl TokenKind {
    fn from_operator(op: &str) -> Option<Self> {
        match op {
            "=" => Some(Self::Equals),
            "," => Some(Self::Comma),
            "?" => Some(Self::Question),
            "|" => Some(Self::Pipe),
            "(" => Some(Self::LParen),
            ")" => Some(Self::RParen),
            "*" => Some(Self::Asterisk),
            "{" => Some(Self::LBrace),
            "}" => Some(Self::RBrace),
            "@" => Some(Self::At),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) value: String,
    pub(crate) lineno: usize,
}

pub(crate) fn tokenize_asdl(buf: &str) -> Result<Vec<Token>, ASDLSyntaxError> {
    let mut tokens = Vec::new();
    for (idx, raw_line) in buf.lines().enumerate() {
        let lineno = idx + 1;
        let line = raw_line.trim();
        let mut chars = line.char_indices().peekable();
        while let Some(&(start, ch)) = chars.peek() {
            if ch.is_whitespace() {
                chars.next();
                continue;
            }
            if line[start..].starts_with("--") {
                break;
            }
            if is_word_char(ch) {
                let mut end = start + ch.len_utf8();
                chars.next();
                while let Some(&(i, c)) = chars.peek() {
                    if is_word_char(c) {
                        end = i + c.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let word = &line[start..end];
                let first = word.chars().next().unwrap();
                if first.is_alphabetic() {
                    let kind = if first.is_uppercase() {
                        TokenKind::ConstructorId
                    } else {
                        TokenKind::TypeId
                    };
                    tokens.push(Token {
                        kind,
                        value: word.to_string(),
                        lineno,
                    });
                } else if word.chars().all(|c| c.is_ascii_digit()) {
                    tokens.push(Token {
                        kind: TokenKind::Int,
                        value: word.to_string(),
                        lineno,
                    });
                } else {
                    return Err(ASDLSyntaxError::new(
                        format!("Invalid operator {word}"),
                        Some(lineno),
                    ));
                }
            } else {
                chars.next();
                let op = &line[start..start + ch.len_utf8()];
                let Some(op_kind) = TokenKind::from_operator(op) else {
                    return Err(ASDLSyntaxError::new(
                        format!("Invalid operator {op}"),
                        Some(lineno),
                    ));
                };
                tokens.push(Token {
                    kind: op_kind,
                    value: op.to_string(),
                    lineno,
                });
            }
        }
    }
    Ok(tokens)
}

fn is_word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

pub(crate) struct ASDLParser {
    tokens: Vec<Token>,
    pos: usize,
}

impl ASDLParser {
    pub(crate) fn new() -> Self {
        Self {
            tokens: Vec::new(),
            pos: 0,
        }
    }

    pub(crate) fn parse(&mut self, buf: &str) -> Result<Module, ASDLSyntaxError> {
        self.tokens = tokenize_asdl(buf)?;
        self.pos = 0;
        self.parse_module()
    }

    fn cur_token(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn parse_module(&mut self) -> Result<Module, ASDLSyntaxError> {
        if self.at_keyword("module") {
            self.advance();
        } else {
            let (found, lineno) = match self.cur_token() {
                Some(tok) => (tok.value.as_str(), Some(tok.lineno)),
                None => ("EOF", None),
            };
            return Err(ASDLSyntaxError::new(
                format!("Expected \"module\" (found {found})"),
                lineno,
            ));
        }
        let name = self.match_kinds(&[TokenKind::ConstructorId, TokenKind::TypeId])?;
        let version = if self.at_keyword("version") {
            let lineno = self.cur_token().map(|t| t.lineno);
            self.advance();
            let ver_str = self.match_kind(TokenKind::Int)?;
            let ver: u32 = ver_str.parse().map_err(|_| {
                ASDLSyntaxError::new(format!("Invalid version number {ver_str}"), lineno)
            })?;
            if ver == 0 {
                return Err(ASDLSyntaxError::new("Module version must be >= 1", lineno));
            }
            ver
        } else {
            1
        };
        self.match_kind(TokenKind::LBrace)?;
        let defs = self.parse_definitions()?;
        self.match_kind(TokenKind::RBrace)?;
        Ok(Module::new(name, version, defs))
    }

    fn parse_definitions(&mut self) -> Result<Vec<Type>, ASDLSyntaxError> {
        let mut defs = Vec::new();
        while let Some(tok) = self.cur_token() {
            if tok.kind != TokenKind::TypeId && tok.kind != TokenKind::At {
                break;
            }
            let annotations = self.parse_annotations()?;
            let typename = self.match_kind(TokenKind::TypeId)?;
            self.match_kind(TokenKind::Equals)?;
            let ty = self.parse_type()?;
            defs.push(Type::new(typename, ty, annotations));
        }
        Ok(defs)
    }

    fn parse_annotations(&mut self) -> Result<Vec<String>, ASDLSyntaxError> {
        let mut annotations = Vec::new();
        while let Some(tok) = self.cur_token() {
            if tok.kind != TokenKind::At {
                break;
            }
            let lineno = tok.lineno;
            self.advance();
            let Some(next_tok) = self.cur_token() else {
                return Err(ASDLSyntaxError::new(
                    "Expected an annotation name after @",
                    Some(lineno),
                ));
            };
            if next_tok.kind != TokenKind::TypeId {
                return Err(ASDLSyntaxError::new(
                    "Expected an annotation name after @",
                    Some(lineno),
                ));
            }
            let name = next_tok.value.clone();
            if !KNOWN_ANNOTATIONS.contains(&name.as_str()) {
                let mut known: Vec<String> =
                    KNOWN_ANNOTATIONS.iter().map(|a| format!("@{a}")).collect();
                known.sort();
                return Err(ASDLSyntaxError::new(
                    format!("Unknown annotation @{name} (known: {})", known.join(", ")),
                    Some(lineno),
                ));
            }
            if annotations.contains(&name) {
                return Err(ASDLSyntaxError::new(
                    format!("Duplicate annotation @{name}"),
                    Some(lineno),
                ));
            }
            self.advance();
            annotations.push(name);
        }
        Ok(annotations)
    }

    fn parse_type(&mut self) -> Result<TypeKind, ASDLSyntaxError> {
        if self.cur_token().map(|t| t.kind) == Some(TokenKind::LParen) {
            self.parse_product().map(TypeKind::Product)
        } else {
            let first_name = self.match_kind(TokenKind::ConstructorId)?;
            let first_fields = self.parse_optional_fields()?;
            let mut sumlist = vec![Constructor::new(first_name, first_fields)];
            while self.cur_token().map(|t| t.kind) == Some(TokenKind::Pipe) {
                self.advance();
                let cname = self.match_kind(TokenKind::ConstructorId)?;
                let cfields = self.parse_optional_fields()?;
                sumlist.push(Constructor::new(cname, cfields));
            }
            let attributes = self.parse_optional_attributes()?;
            Ok(TypeKind::Sum(Sum::new(sumlist, attributes)))
        }
    }

    fn parse_product(&mut self) -> Result<Product, ASDLSyntaxError> {
        let fields = self.parse_fields()?;
        let attributes = self.parse_optional_attributes()?;
        Ok(Product::new(fields, attributes))
    }

    fn parse_fields(&mut self) -> Result<Vec<Field>, ASDLSyntaxError> {
        let mut fields = Vec::new();
        self.match_kind(TokenKind::LParen)?;
        while self.cur_token().map(|t| t.kind) == Some(TokenKind::TypeId) {
            let typename = self.advance().unwrap();
            let quantifiers = self.parse_optional_field_quantifier();
            let id = if matches!(
                self.cur_token().map(|t| t.kind),
                Some(TokenKind::ConstructorId | TokenKind::TypeId)
            ) {
                self.advance()
            } else {
                None
            };
            fields.push(Field::new(typename, id, quantifiers));
            match self.cur_token().map(|t| t.kind) {
                Some(TokenKind::RParen) => break,
                Some(TokenKind::Comma) => {
                    self.advance();
                }
                _ => {}
            }
        }
        self.match_kind(TokenKind::RParen)?;
        Ok(fields)
    }

    fn parse_optional_fields(&mut self) -> Result<Vec<Field>, ASDLSyntaxError> {
        if self.cur_token().map(|t| t.kind) == Some(TokenKind::LParen) {
            self.parse_fields()
        } else {
            Ok(Vec::new())
        }
    }

    fn parse_optional_attributes(&mut self) -> Result<Vec<Field>, ASDLSyntaxError> {
        if self.at_keyword("attributes") {
            self.advance();
            self.parse_fields()
        } else {
            Ok(Vec::new())
        }
    }

    fn parse_optional_field_quantifier(&mut self) -> Vec<Quantifier> {
        let mut quantifiers = Vec::new();
        while let Some(tok) = self.cur_token() {
            match tok.kind {
                TokenKind::Asterisk => quantifiers.push(Quantifier::Sequence),
                TokenKind::Question => quantifiers.push(Quantifier::Optional),
                _ => break,
            }
            self.advance();
        }
        quantifiers
    }

    fn advance(&mut self) -> Option<String> {
        let val = self.cur_token().map(|t| t.value.clone());
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        val
    }

    fn match_kind(&mut self, kind: TokenKind) -> Result<String, ASDLSyntaxError> {
        match self.cur_token() {
            Some(tok) if tok.kind == kind => {
                let val = tok.value.clone();
                self.advance();
                Ok(val)
            }
            Some(tok) => Err(ASDLSyntaxError::new(
                format!("Unmatched {kind:?} (found {:?})", tok.kind),
                Some(tok.lineno),
            )),
            None => Err(ASDLSyntaxError::new(format!("Unmatched {kind:?} (found EOF)"), None)),
        }
    }

    fn match_kinds(&mut self, kinds: &[TokenKind]) -> Result<String, ASDLSyntaxError> {
        match self.cur_token() {
            Some(tok) if kinds.contains(&tok.kind) => {
                let val = tok.value.clone();
                self.advance();
                Ok(val)
            }
            Some(tok) => Err(ASDLSyntaxError::new(
                format!("Unmatched {kinds:?} (found {:?})", tok.kind),
                Some(tok.lineno),
            )),
            None => Err(ASDLSyntaxError::new(format!("Unmatched {kinds:?} (found EOF)"), None)),
        }
    }

    fn at_keyword(&self, keyword: &str) -> bool {
        matches!(
            self.cur_token(),
            Some(tok) if tok.kind == TokenKind::TypeId && tok.value == keyword
        )
    }
}

impl Default for ASDLParser {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn parse_str(buf: &str) -> Result<Module, ASDLSyntaxError> {
    ASDLParser::new().parse(buf)
}

pub(crate) fn parse(filename: impl AsRef<Path>) -> Result<Module, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(filename)?;
    Ok(parse_str(&content)?)
}

#[derive(Default)]
pub(crate) struct Check {
    pub(crate) cons: HashMap<String, String>,
    pub(crate) errors: usize,
    pub(crate) types: Vec<(String, Vec<String>)>,
    type_indices: HashMap<String, usize>,
    pub(crate) messages: Vec<String>,
}

impl Check {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn visit_module(&mut self, mod_node: &Module) {
        for dfn in &mod_node.dfns {
            self.visit_type(dfn);
        }
    }

    pub(crate) fn visit_type(&mut self, ty: &Type) {
        match &ty.value {
            TypeKind::Sum(sum) => self.visit_sum(sum, &ty.name),
            TypeKind::Product(prod) => self.visit_product(prod, &ty.name),
        }
    }

    pub(crate) fn visit_sum(&mut self, sum: &Sum, name: &str) {
        for t in &sum.types {
            self.visit_constructor(t, name);
        }
    }

    pub(crate) fn visit_constructor(&mut self, cons: &Constructor, name: &str) {
        let key = &cons.name;
        if let Some(conflict) = self.cons.get(key) {
            self.messages
                .push(format!("Redefinition of constructor {key}"));
            self.messages
                .push(format!("Defined in {conflict} and {name}"));
            self.errors += 1;
        } else {
            self.cons.insert(key.clone(), name.to_string());
        }
        for f in &cons.fields {
            self.visit_field(f, key);
        }
    }

    pub(crate) fn visit_field(&mut self, field: &Field, name: &str) {
        if let Some(&idx) = self.type_indices.get(&field.ty) {
            self.types[idx].1.push(name.to_string());
        } else {
            let idx = self.types.len();
            self.type_indices.insert(field.ty.clone(), idx);
            self.types.push((field.ty.clone(), vec![name.to_string()]));
        }
    }

    pub(crate) fn visit_product(&mut self, prod: &Product, name: &str) {
        for f in &prod.fields {
            self.visit_field(f, name);
        }
    }
}

pub(crate) fn check_with_builtins(mod_node: &Module, builtins: &[&str]) -> Result<(), Vec<String>> {
    let builtin_set: HashSet<&str> = builtins.iter().copied().collect();
    let mut v = Check::new();
    v.visit_module(mod_node);

    for (t, uses) in &v.types {
        if !mod_node.types.contains_key(t) && !builtin_set.contains(t.as_str()) {
            v.errors += 1;
            let uses_str = uses.join(", ");
            v.messages
                .push(format!("Undefined type {t}, used in {uses_str}"));
        }
    }
    if v.errors == 0 {
        Ok(())
    } else {
        Err(v.messages)
    }
}
