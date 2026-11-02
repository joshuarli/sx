use std::collections::BTreeMap;

use sx_kernel::diagnostics::Diagnostic;
use sx_kernel::syntax::Span;

use crate::ast::*;
use crate::diag;
use crate::lexer::{Lexer, Tok, Token};

pub struct Program {
    pub body: Vec<Stmt>,
    pub types: BTreeMap<String, TypeAnn>,
    pub diagnostics: Vec<Diagnostic>,
}

struct Stop;

type Parse<T> = Result<T, Stop>;

pub fn parse(source: &str) -> Result<Program, Vec<Diagnostic>> {
    let tokens = Lexer::new(source).tokenize().map_err(|d| vec![*d])?;
    let mut parser = Parser { tokens, at: 0, diagnostics: Vec::new(), types: BTreeMap::new() };
    let mut body = Vec::new();
    let finished = loop {
        if matches!(parser.peek().tok, Tok::Eof) {
            break true;
        }
        match parser.statement() {
            Ok(statement) => body.push(statement),
            Err(Stop) => break false,
        }
    };
    if !finished {
        parser.diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.column));
        return Err(parser.diagnostics);
    }
    Ok(Program { body, types: parser.types, diagnostics: parser.diagnostics })
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    diagnostics: Vec<Diagnostic>,
    types: BTreeMap<String, TypeAnn>,
}

const ASSIGNMENT_REJECTED: [&str; 8] = ["**=", "&=", "|=", "^=", "<<=", ">>=", ">>>=", "&&="];
const MODULE_WORDS: [&str; 2] = ["import", "export"];
const DECLARE_WORDS: [&str; 4] = ["enum", "namespace", "declare", "module"];

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.at.min(self.tokens.len() - 1)]
    }

    fn peek_at(&self, ahead: usize) -> &Token {
        &self.tokens[(self.at + ahead).min(self.tokens.len() - 1)]
    }

    fn next(&mut self) -> Token {
        let token = self.peek().clone();
        if self.at < self.tokens.len() - 1 {
            self.at += 1;
        }
        token
    }

    fn span(&self) -> Span {
        self.peek().span
    }

    fn previous_end(&self) -> sx_kernel::syntax::Position {
        self.tokens[self.at.saturating_sub(1)].span.end
    }

    fn since(&self, start: Span) -> Span {
        Span::new(start.start, self.previous_end())
    }

    fn is_punct(&self, p: &str) -> bool {
        matches!(&self.peek().tok, Tok::Punct(q) if *q == p)
    }

    fn is_punct_at(&self, ahead: usize, p: &str) -> bool {
        matches!(&self.peek_at(ahead).tok, Tok::Punct(q) if *q == p)
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(&self.peek().tok, Tok::Ident(name) if name == w)
    }

    fn is_word_at(&self, ahead: usize, w: &str) -> bool {
        matches!(&self.peek_at(ahead).tok, Tok::Ident(name) if name == w)
    }

    fn eat(&mut self, p: &str) -> bool {
        let found = self.is_punct(p);
        if found {
            self.next();
        }
        found
    }

    fn eat_word(&mut self, w: &str) -> bool {
        let found = self.is_word(w);
        if found {
            self.next();
        }
        found
    }

    fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn fail<T>(&mut self, span: Span, message: impl Into<String>) -> Parse<T> {
        self.report(diag("ts-syntax", span, message));
        Err(Stop)
    }

    fn describe(token: &Token) -> String {
        match &token.tok {
            Tok::Ident(name) => format!("`{name}`"),
            Tok::Punct(p) => format!("`{p}`"),
            Tok::Eof => "the end of the program".to_string(),
            Tok::Str(_) => "a string".to_string(),
            Tok::Int(_) | Tok::Float(_) | Tok::BigInt => "a number".to_string(),
            Tok::Template(..) => "a template".to_string(),
            Tok::Regex => "a regular expression".to_string(),
        }
    }

    fn expect(&mut self, p: &str) -> Parse<Token> {
        if self.is_punct(p) {
            return Ok(self.next());
        }
        let found = Self::describe(self.peek());
        let span = self.span();
        self.fail(span, format!("Expected `{p}`, found {found}"))
    }

    fn identifier(&mut self) -> Parse<Ident> {
        if let Tok::Ident(name) = &self.peek().tok {
            let name = name.clone();
            let span = self.span();
            self.next();
            return Ok(Ident { name, span });
        }
        let found = Self::describe(self.peek());
        let span = self.span();
        self.fail(span, format!("Expected a name, found {found}"))
    }

    fn terminator(&mut self) -> Parse<()> {
        if self.eat(";") || self.is_punct("}") || matches!(self.peek().tok, Tok::Eof) || self.peek().newline_before {
            return Ok(());
        }
        let found = Self::describe(self.peek());
        let span = self.span();
        self.fail(span, format!("Expected `;`, found {found}"))
    }

    fn skip_statement(&mut self) {
        let mut depth = 0usize;
        let mut first = true;
        loop {
            if matches!(self.peek().tok, Tok::Eof) {
                return;
            }
            if depth == 0 && !first && self.peek().newline_before && !self.is_punct("{") && !self.is_word("else") {
                return;
            }
            first = false;
            let token = self.next();
            match token.tok {
                Tok::Punct("(" | "[" | "{") => depth += 1,
                Tok::Punct(")" | "]") => depth = depth.saturating_sub(1),
                Tok::Punct("}") => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && !self.is_punct(")") && !self.is_punct(",") && !self.is_word("else") && !self.is_word("catch") {
                        return;
                    }
                }
                Tok::Punct(";") if depth == 0 => return,
                _ => {}
            }
        }
    }

    fn reject_statement(&mut self, diagnostic: Diagnostic) -> Parse<Stmt> {
        let span = diagnostic.span;
        self.report(diagnostic);
        self.skip_statement();
        Ok(Stmt { kind: StmtKind::Rejected(Vec::new()), span })
    }

    fn statement(&mut self) -> Parse<Stmt> {
        let start = self.span();
        let Tok::Ident(word) = self.peek().tok.clone() else {
            return self.simple_statement(start);
        };
        let next_is_colon = self.is_punct_at(1, ":") && !matches!(word.as_str(), "default");
        if next_is_colon && !word.is_empty() {
            self.report(diag("ts-label", start, "Labels are not supported").fix("remove the label"));
            self.next();
            self.next();
            let inner = self.statement()?;
            return Ok(Stmt { kind: StmtKind::Rejected(vec![inner]), span: start });
        }
        match word.as_str() {
            "const" | "let" => {
                let kind = if word == "const" { DeclKind::Const } else { DeclKind::Let };
                self.next();
                let decls = self.declarators()?;
                self.terminator()?;
                Ok(Stmt { kind: StmtKind::Decl(kind, decls), span: self.since(start) })
            }
            "var" => {
                let d = diag("ts-unsupported", start, "var is not supported in checkable TypeScript").fix("use const or let");
                self.reject_statement(d)
            }
            "function" => {
                let function = self.function_declaration(false)?;
                Ok(Stmt { span: function.span, kind: StmtKind::Function(function) })
            }
            "async" if self.is_word_at(1, "function") => {
                self.next();
                let function = self.function_declaration(true)?;
                Ok(Stmt { span: function.span, kind: StmtKind::Function(function) })
            }
            "if" => self.if_statement(start),
            "for" => self.for_statement(start),
            "while" | "do" => {
                self.report(
                    diag("ts-while", start, "Loops must be bounded")
                        .fix("retry({ ... }, () => call), waitUntil, loop({ max: N }, state, fn), or for (const x of xs)"),
                );
                self.next();
                let body = if word == "while" {
                    self.expect("(")?;
                    self.expression()?;
                    self.expect(")")?;
                    self.statement()?
                } else {
                    let body = self.statement()?;
                    if self.eat_word("while") {
                        self.expect("(")?;
                        self.expression()?;
                        self.expect(")")?;
                    }
                    self.eat(";");
                    body
                };
                Ok(Stmt { kind: StmtKind::Rejected(vec![body]), span: start })
            }
            "switch" => {
                self.report(diag("ts-switch", start, "switch is not supported").fix("use an if chain"));
                self.next();
                self.expect("(")?;
                self.expression()?;
                self.expect(")")?;
                self.expect("{")?;
                let mut children = Vec::new();
                while !self.is_punct("}") {
                    if matches!(self.peek().tok, Tok::Eof) {
                        let span = self.span();
                        return self.fail(span, "Expected `}`, found the end of the program");
                    }
                    if self.eat_word("case") {
                        self.expression()?;
                        self.expect(":")?;
                    } else if self.eat_word("default") {
                        self.expect(":")?;
                    } else {
                        children.push(self.statement()?);
                    }
                }
                self.next();
                Ok(Stmt { kind: StmtKind::Rejected(children), span: start })
            }
            "try" => self.try_statement(start),
            "throw" => {
                self.next();
                if self.peek().newline_before {
                    return self.fail(start, "A throw needs a value on the same line");
                }
                let value = self.expression()?;
                self.terminator()?;
                Ok(Stmt { kind: StmtKind::Throw(value), span: self.since(start) })
            }
            "return" => {
                self.next();
                let value = if self.is_punct(";") || self.is_punct("}") || self.peek().newline_before || matches!(self.peek().tok, Tok::Eof) {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.terminator()?;
                Ok(Stmt { kind: StmtKind::Return(value), span: self.since(start) })
            }
            "break" | "continue" => {
                self.next();
                if matches!(self.peek().tok, Tok::Ident(_)) && !self.peek().newline_before {
                    let d = diag("ts-label", self.span(), "Labels are not supported");
                    self.report(d);
                    self.next();
                }
                self.terminator()?;
                let kind = if word == "break" { StmtKind::Break } else { StmtKind::Continue };
                Ok(Stmt { kind, span: self.since(start) })
            }
            "class" => {
                let d = diag("ts-class", start, "Classes are not supported").fix("use object literals");
                self.reject_statement(d)
            }
            "with" | "debugger" => {
                let d = diag("ts-label", start, format!("{word} is not supported"));
                self.reject_statement(d)
            }
            w if MODULE_WORDS.contains(&w) && !self.is_punct_at(1, "(") && !self.is_punct_at(1, ".") => {
                let d = diag("ts-import-export", start, "Programs have no modules").fix("use the tools directly");
                self.reject_statement(d)
            }
            w if DECLARE_WORDS.contains(&w) && matches!(self.peek_at(1).tok, Tok::Ident(_)) => {
                let d = diag("ts-enum", start, format!("{w} is not supported")).fix("use a union of string literals");
                self.reject_statement(d)
            }
            "type" if matches!(self.peek_at(1).tok, Tok::Ident(_)) && (self.is_punct_at(2, "=") || self.is_punct_at(2, "<")) => {
                self.next();
                let name = self.identifier()?;
                if self.is_punct("<") {
                    let d = diag("ts-annotation", self.span(), "Generic type declarations are not supported");
                    self.report(d);
                    return Err(Stop);
                }
                self.expect("=")?;
                let value = self.type_annotation()?;
                self.terminator()?;
                self.types.insert(name.name.clone(), value.clone());
                Ok(Stmt { kind: StmtKind::TypeDecl(name.name, value), span: self.since(start) })
            }
            "interface" if matches!(self.peek_at(1).tok, Tok::Ident(_)) => {
                self.next();
                let name = self.identifier()?;
                if !self.is_punct("{") {
                    let d = diag("ts-annotation", self.span(), "interface extends and generics are not supported");
                    self.report(d);
                    return Err(Stop);
                }
                let value = self.type_annotation()?;
                self.types.insert(name.name.clone(), value.clone());
                Ok(Stmt { kind: StmtKind::TypeDecl(name.name, value), span: self.since(start) })
            }
            _ => self.simple_statement(start),
        }
    }

    fn simple_statement(&mut self, start: Span) -> Parse<Stmt> {
        if self.eat(";") {
            return Ok(Stmt { kind: StmtKind::Empty, span: start });
        }
        if self.is_punct("{") {
            let block = self.block()?;
            return Ok(Stmt { kind: StmtKind::Block(block), span: self.since(start) });
        }
        let expression = self.expression()?;
        self.terminator()?;
        Ok(Stmt { kind: StmtKind::Expr(expression), span: self.since(start) })
    }

    fn block(&mut self) -> Parse<Vec<Stmt>> {
        self.expect("{")?;
        let mut body = Vec::new();
        while !self.is_punct("}") {
            if matches!(self.peek().tok, Tok::Eof) {
                let span = self.span();
                return self.fail(span, "Expected `}`, found the end of the program");
            }
            body.push(self.statement()?);
        }
        self.next();
        Ok(body)
    }

    fn declarators(&mut self) -> Parse<Vec<Declarator>> {
        let mut decls = Vec::new();
        loop {
            let pattern = self.pattern()?;
            decls.push(self.declarator_rest(pattern)?);
            if !self.eat(",") {
                return Ok(decls);
            }
        }
    }

    fn declarator_rest(&mut self, pattern: Pattern) -> Parse<Declarator> {
        let annotation = if self.eat(":") { Some(self.type_annotation()?) } else { None };
        let init = if self.eat("=") { Some(self.assignment()?) } else { None };
        Ok(Declarator { pattern, annotation, init })
    }

    fn pattern(&mut self) -> Parse<Pattern> {
        let start = self.span();
        if self.eat("{") {
            let mut props = Vec::new();
            while !self.is_punct("}") {
                let key_span = self.span();
                if self.is_punct("...") {
                    self.report(diag("ts-param", key_span, "Defaults and rest are not supported"));
                    self.next();
                    self.pattern()?;
                } else if self.is_punct("[") {
                    self.report(diag("ts-key", key_span, "This key cannot be a record field"));
                    return Err(Stop);
                } else {
                    let key = match self.next().tok {
                        Tok::Ident(name) => name,
                        Tok::Str(text) => text,
                        _ => return self.fail(key_span, "Expected a property name in the pattern"),
                    };
                    let value = if self.eat(":") {
                        self.pattern()?
                    } else {
                        Pattern { kind: PatternKind::Ident(key.clone()), span: key_span }
                    };
                    if self.is_punct("=") {
                        self.report(diag("ts-param", self.span(), "Defaults and rest are not supported"));
                        self.next();
                        self.assignment()?;
                    }
                    props.push(PatternProp { key, value });
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("}")?;
            return Ok(Pattern { kind: PatternKind::Object(props), span: self.since(start) });
        }
        if self.eat("[") {
            let mut elements = Vec::new();
            while !self.is_punct("]") {
                if self.is_punct(",") {
                    let d = diag("ts-unsupported", self.span(), "An array pattern hole is not supported in checkable TypeScript");
                    self.report(d);
                    self.next();
                    continue;
                }
                if self.is_punct("...") {
                    self.report(diag("ts-param", self.span(), "Defaults and rest are not supported"));
                    self.next();
                }
                elements.push(self.pattern()?);
                if self.is_punct("=") {
                    self.report(diag("ts-param", self.span(), "Defaults and rest are not supported"));
                    self.next();
                    self.assignment()?;
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("]")?;
            return Ok(Pattern { kind: PatternKind::Array(elements), span: self.since(start) });
        }
        let name = self.identifier()?;
        Ok(Pattern { kind: PatternKind::Ident(name.name), span: name.span })
    }

    fn parameters(&mut self) -> Parse<Vec<Pattern>> {
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.is_punct(")") {
            let start = self.span();
            if self.is_punct("...") {
                self.report(diag("ts-param", start, "Defaults and rest are not supported"));
                self.next();
            }
            if self.is_word("this") {
                self.report(diag("ts-param", start, "Defaults and rest are not supported").fix("remove the this parameter"));
                self.next();
                if self.eat(":") {
                    self.type_annotation()?;
                }
                if !self.eat(",") {
                    break;
                }
                continue;
            }
            let pattern = self.pattern()?;
            if self.is_punct("?") {
                self.report(diag("ts-param", self.span(), "Defaults and rest are not supported"));
                self.next();
            }
            if self.eat(":") {
                self.type_annotation()?;
            }
            if self.is_punct("=") {
                self.report(diag("ts-param", self.span(), "Defaults and rest are not supported"));
                self.next();
                self.assignment()?;
            }
            params.push(pattern);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(params)
    }

    fn function_declaration(&mut self, is_async: bool) -> Parse<Function> {
        let start = self.span();
        self.next();
        if self.is_punct("*") {
            let d = diag("ts-generator", self.span(), "Generators are not supported").fix("use for…of");
            self.report(d);
            return Err(Stop);
        }
        let name = self.identifier()?;
        if self.is_punct("<") {
            let d = diag("ts-annotation", self.span(), "Generic parameters are not supported");
            self.report(d);
            return Err(Stop);
        }
        let params = self.parameters()?;
        if self.eat(":") {
            self.type_annotation()?;
        }
        let body = self.block()?;
        Ok(Function { name: Some(name), params, body: FunctionBody::Block(body), is_async, span: self.since(start) })
    }

    fn if_statement(&mut self, start: Span) -> Parse<Stmt> {
        self.next();
        self.expect("(")?;
        let condition = self.expression()?;
        self.expect(")")?;
        let then = Box::new(self.statement()?);
        let otherwise = if self.eat_word("else") { Some(Box::new(self.statement()?)) } else { None };
        Ok(Stmt { kind: StmtKind::If(condition, then, otherwise), span: self.since(start) })
    }

    fn for_statement(&mut self, start: Span) -> Parse<Stmt> {
        self.next();
        if self.is_word("await") {
            let d = diag("ts-generator", self.span(), "Generators are not supported").fix("use for…of");
            return self.reject_statement(d);
        }
        self.expect("(")?;
        let init: Option<Box<Stmt>> = if self.is_punct(";") {
            None
        } else if self.is_word("const") || self.is_word("let") || self.is_word("var") {
            let init_start = self.span();
            if self.is_word("var") {
                self.report(diag("ts-unsupported", init_start, "var is not supported in checkable TypeScript").fix("use const or let"));
            }
            let kind = if self.is_word("const") { DeclKind::Const } else { DeclKind::Let };
            self.next();
            let pattern = self.pattern()?;
            if self.eat_word("of") {
                let source = self.assignment()?;
                self.expect(")")?;
                let body = Box::new(self.statement()?);
                return Ok(Stmt { kind: StmtKind::ForOf { pattern, source, body }, span: self.since(start) });
            }
            if self.is_word("in") {
                self.report(diag("ts-for-in", start, "for…in is not supported").fix("Object.keys(m).sort()"));
                self.next();
                self.expression()?;
                self.expect(")")?;
                let body = self.statement()?;
                return Ok(Stmt { kind: StmtKind::Rejected(vec![body]), span: start });
            }
            let mut decls = vec![self.declarator_rest(pattern)?];
            while self.eat(",") {
                let next_pattern = self.pattern()?;
                decls.push(self.declarator_rest(next_pattern)?);
            }
            Some(Box::new(Stmt { kind: StmtKind::Decl(kind, decls), span: self.since(init_start) }))
        } else {
            let init_start = self.span();
            let expression = self.expression()?;
            if self.is_word("of") || self.is_word("in") {
                let d = diag("ts-unsupported", start, "for…of without a declaration is not supported in checkable TypeScript")
                    .fix("for (const x of xs)");
                return self.reject_statement(d);
            }
            Some(Box::new(Stmt { kind: StmtKind::Expr(expression), span: self.since(init_start) }))
        };
        self.expect(";")?;
        let test = if self.is_punct(";") { None } else { Some(self.expression()?) };
        self.expect(";")?;
        let update = if self.is_punct(")") { None } else { Some(self.expression()?) };
        self.expect(")")?;
        let body = Box::new(self.statement()?);
        Ok(Stmt { kind: StmtKind::For { init, test, update, body }, span: self.since(start) })
    }

    fn try_statement(&mut self, start: Span) -> Parse<Stmt> {
        self.next();
        let block = self.block()?;
        let mut param = None;
        let mut handler = None;
        if self.eat_word("catch") {
            if self.eat("(") {
                param = Some(self.pattern()?);
                if self.eat(":") {
                    self.type_annotation()?;
                }
                self.expect(")")?;
            }
            handler = Some(self.block()?);
        }
        if self.is_word("finally") {
            let span = self.span();
            self.next();
            self.block()?;
            self.report(diag("ts-finally", span, "finally is not supported").fix("write the cleanup in both branches"));
        }
        let Some(handler) = handler else {
            return self.fail(start, "A try needs a catch");
        };
        Ok(Stmt { kind: StmtKind::Try { block, param, handler }, span: self.since(start) })
    }

    pub fn expression(&mut self) -> Parse<Expr> {
        let expression = self.assignment()?;
        if self.is_punct(",") {
            let span = self.span();
            self.report(diag("ts-operator", span, "the comma operator is not supported"));
            while self.eat(",") {
                self.assignment()?;
            }
        }
        Ok(expression)
    }

    fn try_arrow(&mut self) -> Parse<Option<Expr>> {
        let start = self.span();
        let save = (self.at, self.diagnostics.len());
        let is_async = self.is_word("async") && (self.is_punct_at(1, "(") || matches!(self.peek_at(1).tok, Tok::Ident(_)));
        if is_async {
            self.next();
        }
        let params = if let Tok::Ident(name) = &self.peek().tok {
            if !self.is_punct_at(1, "=>") || matches!(name.as_str(), "async") && is_async {
                self.at = save.0;
                return Ok(None);
            }
            let ident = self.identifier()?;
            vec![Pattern { kind: PatternKind::Ident(ident.name), span: ident.span }]
        } else if self.is_punct("(") {
            let mut depth = 0usize;
            let mut at = 0usize;
            loop {
                match &self.peek_at(at).tok {
                    Tok::Punct("(" | "[" | "{") => depth += 1,
                    Tok::Punct(")" | "]" | "}") => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Tok::Eof => {
                        self.at = save.0;
                        return Ok(None);
                    }
                    _ => {}
                }
                at += 1;
            }
            if !(self.is_punct_at(at + 1, "=>") || self.is_punct_at(at + 1, ":") && self.arrow_after_annotation(at + 1)) {
                self.at = save.0;
                return Ok(None);
            }
            self.parameters()?
        } else {
            self.at = save.0;
            return Ok(None);
        };
        if self.eat(":") {
            self.type_annotation()?;
        }
        if !self.eat("=>") {
            self.at = save.0;
            self.diagnostics.truncate(save.1);
            return Ok(None);
        }
        let body = if self.is_punct("{") {
            FunctionBody::Block(self.block()?)
        } else {
            FunctionBody::Expr(Box::new(self.assignment()?))
        };
        let span = self.since(start);
        Ok(Some(Expr { kind: ExprKind::Arrow(Box::new(Function { name: None, params, body, is_async, span })), span }))
    }

    fn arrow_after_annotation(&self, colon: usize) -> bool {
        let mut depth = 0usize;
        let mut at = colon + 1;
        loop {
            match &self.peek_at(at).tok {
                Tok::Punct("(" | "[" | "{" | "<") => depth += 1,
                Tok::Punct(")" | "]" | "}" | ">") => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                Tok::Punct("=>") if depth == 0 => return true,
                Tok::Punct(";" | ",") | Tok::Eof => return false,
                _ => {}
            }
            at += 1;
        }
    }

    fn assignment(&mut self) -> Parse<Expr> {
        if let Some(arrow) = self.try_arrow()? {
            return Ok(arrow);
        }
        let start = self.span();
        if self.is_word("yield") {
            let d = diag("ts-generator", start, "Generators are not supported").fix("use for…of");
            self.report(d);
            return Err(Stop);
        }
        let target = self.conditional()?;
        let op = match &self.peek().tok {
            Tok::Punct("=") => Some(AssignOp::Set),
            Tok::Punct("+=") => Some(AssignOp::Add),
            Tok::Punct("-=") => Some(AssignOp::Sub),
            Tok::Punct("*=") => Some(AssignOp::Mul),
            Tok::Punct("/=") => Some(AssignOp::Div),
            Tok::Punct("%=") => Some(AssignOp::Rem),
            Tok::Punct(p) if ASSIGNMENT_REJECTED.contains(p) => {
                let span = self.span();
                self.report(diag("ts-operator", span, format!("{p} is not supported")));
                self.next();
                self.assignment()?;
                return Ok(Expr { kind: ExprKind::Rejected, span: self.since(start) });
            }
            Tok::Punct(p @ ("||=" | "??=")) => {
                let span = self.span();
                self.report(diag("ts-operator", span, format!("{p} is not supported")));
                self.next();
                self.assignment()?;
                return Ok(Expr { kind: ExprKind::Rejected, span: self.since(start) });
            }
            _ => None,
        };
        let Some(op) = op else {
            return Ok(target);
        };
        self.next();
        let value = self.assignment()?;
        Ok(Expr { kind: ExprKind::Assign(op, Box::new(target), Box::new(value)), span: self.since(start) })
    }

    fn conditional(&mut self) -> Parse<Expr> {
        let start = self.span();
        let condition = self.binary(0)?;
        if !self.eat("?") {
            return Ok(condition);
        }
        let then = self.assignment()?;
        self.expect(":")?;
        let otherwise = self.assignment()?;
        Ok(Expr {
            kind: ExprKind::Conditional(Box::new(condition), Box::new(then), Box::new(otherwise)),
            span: self.since(start),
        })
    }

    fn binary_operator(&self) -> Option<(&'static str, u8)> {
        let precedence = |p: &'static str| -> Option<u8> {
            Some(match p {
                "??" => 1,
                "||" => 2,
                "&&" => 3,
                "|" => 4,
                "^" => 5,
                "&" => 6,
                "==" | "!=" | "===" | "!==" => 7,
                "<" | ">" | "<=" | ">=" => 8,
                "<<" | ">>" | ">>>" => 9,
                "+" | "-" => 10,
                "*" | "/" | "%" => 11,
                "**" => 12,
                _ => return None,
            })
        };
        match &self.peek().tok {
            Tok::Punct(p) => precedence(p).map(|level| (*p, level)),
            Tok::Ident(w) if w == "instanceof" => Some(("instanceof", 8)),
            Tok::Ident(w) if w == "in" => Some(("in", 8)),
            Tok::Ident(w) if (w == "as" || w == "satisfies") && !self.peek().newline_before => Some((if w == "as" { "as" } else { "satisfies" }, 8)),
            _ => None,
        }
    }

    fn binary(&mut self, minimum: u8) -> Parse<Expr> {
        let start = self.span();
        let mut left = self.unary()?;
        while let Some((op, level)) = self.binary_operator() {
            if level < minimum || level == 0 {
                break;
            }
            let op_span = self.span();
            self.next();
            match op {
                "as" => {
                    let annotation = self.type_annotation()?;
                    left = Expr { kind: ExprKind::Cast(Box::new(left), annotation), span: self.since(start) };
                    continue;
                }
                "satisfies" => {
                    self.report(diag("ts-cast", op_span, "A cast cannot be checked"));
                    self.type_annotation()?;
                    continue;
                }
                _ => {}
            }
            let right = if op == "**" { self.binary(level)? } else { self.binary(level + 1)? };
            let span = self.since(start);
            let rejected = |parser: &mut Parser, code: &'static str, message: String| {
                parser.report(diag(code, op_span, message));
                Expr { kind: ExprKind::Rejected, span }
            };
            left = match op {
                "instanceof" => rejected(self, "ts-instanceof", "Types are static; instanceof is not supported".to_string()),
                "in" | "**" | "|" | "^" | "&" | "<<" | ">>" | ">>>" => {
                    rejected(self, "ts-operator", format!("{op} is not supported"))
                }
                "==" | "!=" => {
                    let null_side = |e: &Expr| matches!(e.kind, ExprKind::Null);
                    if null_side(&right) || null_side(&left) {
                        let operand = if null_side(&right) { left } else { right };
                        let kind = if op == "==" { BinaryOp::LooseEqNull } else { BinaryOp::LooseNeNull };
                        Expr { kind: ExprKind::Binary(kind, Box::new(operand.clone()), Box::new(operand)), span }
                    } else {
                        let d = diag("ts-loose-equality", op_span, "Use === and !==");
                        self.report(d.fix("=== or !=="));
                        Expr { kind: ExprKind::Rejected, span }
                    }
                }
                _ => {
                    let kind = match op {
                        "+" => BinaryOp::Add,
                        "-" => BinaryOp::Sub,
                        "*" => BinaryOp::Mul,
                        "/" => BinaryOp::Div,
                        "%" => BinaryOp::Rem,
                        "<" => BinaryOp::Lt,
                        "<=" => BinaryOp::Le,
                        ">" => BinaryOp::Gt,
                        ">=" => BinaryOp::Ge,
                        "===" => BinaryOp::StrictEq,
                        "!==" => BinaryOp::StrictNe,
                        "&&" => BinaryOp::And,
                        "||" => BinaryOp::Or,
                        _ => BinaryOp::Coalesce,
                    };
                    Expr { kind: ExprKind::Binary(kind, Box::new(left), Box::new(right)), span }
                }
            };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Parse<Expr> {
        let start = self.span();
        let operator = match &self.peek().tok {
            Tok::Punct(p) => Some(*p),
            Tok::Ident(w) if matches!(w.as_str(), "typeof" | "await" | "void" | "delete") => {
                Some(match w.as_str() {
                    "typeof" => "typeof",
                    "await" => "await",
                    "void" => "void",
                    _ => "delete",
                })
            }
            _ => None,
        };
        let wrap = |kind: ExprKind, parser: &Parser| Expr { kind, span: parser.since(start) };
        match operator {
            Some("!") => {
                self.next();
                let operand = self.unary()?;
                Ok(wrap(ExprKind::Unary(UnaryOp::Not, Box::new(operand)), self))
            }
            Some("-") => {
                self.next();
                let operand = self.unary()?;
                Ok(wrap(ExprKind::Unary(UnaryOp::Neg, Box::new(operand)), self))
            }
            Some("+") => {
                self.next();
                let operand = self.unary()?;
                Ok(wrap(ExprKind::Unary(UnaryOp::Plus, Box::new(operand)), self))
            }
            Some("typeof") => {
                self.next();
                let operand = self.unary()?;
                Ok(wrap(ExprKind::Unary(UnaryOp::TypeOf, Box::new(operand)), self))
            }
            Some("await") => {
                self.next();
                let operand = self.unary()?;
                Ok(wrap(ExprKind::Await(Box::new(operand)), self))
            }
            Some("++") | Some("--") => {
                let increment = operator == Some("++");
                self.next();
                let target = self.unary()?;
                Ok(wrap(ExprKind::Update { increment, target: Box::new(target) }, self))
            }
            Some("void") => {
                self.next();
                let operand = self.unary()?;
                let is_zero = matches!(operand.kind, ExprKind::Number(Number::Int(0)));
                let (code, message) = if is_zero {
                    ("ts-undefined", "undefined does not exist; absent values are null")
                } else {
                    ("ts-operator", "void is not supported")
                };
                self.report(diag(code, start, message));
                Ok(wrap(ExprKind::Rejected, self))
            }
            Some("delete") | Some("~") => {
                self.next();
                self.unary()?;
                self.report(diag("ts-operator", start, format!("{} is not supported", operator.unwrap_or(""))));
                Ok(wrap(ExprKind::Rejected, self))
            }
            Some("<") => {
                self.report(diag("ts-cast", start, "A cast cannot be checked"));
                Err(Stop)
            }
            _ => self.postfix(),
        }
    }

    fn arguments(&mut self) -> Parse<Vec<Expr>> {
        self.expect("(")?;
        let mut args = Vec::new();
        while !self.is_punct(")") {
            if self.is_punct("...") {
                let span = self.span();
                self.next();
                self.assignment()?;
                let d = diag("ts-spread", span, "Only records with declared fields can be spread into an object");
                self.report(d.fix("write the arguments out"));
                if !self.eat(",") {
                    break;
                }
                continue;
            }
            args.push(self.assignment()?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(args)
    }

    fn property_name(&mut self) -> Parse<String> {
        let token = self.next();
        match token.tok {
            Tok::Ident(name) => Ok(name),
            _ => self.fail(token.span, "Expected a property name"),
        }
    }

    fn postfix(&mut self) -> Parse<Expr> {
        let start = self.span();
        let mut expression = self.primary()?;
        let mut links: Vec<Link> = Vec::new();
        loop {
            let link_start = self.span();
            if self.is_punct("?.") {
                self.next();
                let link = if self.is_punct("(") {
                    Link::Call { args: self.arguments()?, optional: true, span: self.since(link_start) }
                } else if self.eat("[") {
                    let index = self.expression()?;
                    self.expect("]")?;
                    Link::Access { access: Access::Index(Box::new(index)), optional: true, span: self.since(link_start) }
                } else {
                    let name = self.property_name()?;
                    Link::Access { access: Access::Field(name), optional: true, span: self.since(link_start) }
                };
                links.push(link);
            } else if self.is_punct(".") {
                self.next();
                let name = self.property_name()?;
                if links.is_empty() {
                    expression = Expr { kind: ExprKind::Field(Box::new(expression), name), span: self.since(start) };
                } else {
                    links.push(Link::Access { access: Access::Field(name), optional: false, span: self.since(link_start) });
                }
            } else if self.is_punct("[") && !self.peek().newline_before {
                self.next();
                let index = self.expression()?;
                self.expect("]")?;
                if links.is_empty() {
                    expression = Expr { kind: ExprKind::Index(Box::new(expression), Box::new(index)), span: self.since(start) };
                } else {
                    links.push(Link::Access {
                        access: Access::Index(Box::new(index)),
                        optional: false,
                        span: self.since(link_start),
                    });
                }
            } else if self.is_punct("(") && !self.peek().newline_before {
                let args = self.arguments()?;
                if links.is_empty() {
                    expression = Expr { kind: ExprKind::Call(Box::new(expression), args), span: self.since(start) };
                } else {
                    links.push(Link::Call { args, optional: false, span: self.since(link_start) });
                }
            } else if self.is_punct("!") && !self.peek().newline_before && !self.is_punct_at(1, "=") {
                self.report(diag("ts-cast", self.span(), "A cast cannot be checked"));
                self.next();
            } else if matches!(self.peek().tok, Tok::Template(..)) && !self.peek().newline_before {
                self.report(diag("ts-unsupported", self.span(), "tagged templates are not supported in checkable TypeScript"));
                self.next();
            } else if (self.is_punct("++") || self.is_punct("--")) && !self.peek().newline_before {
                let increment = self.is_punct("++");
                self.next();
                let target = if links.is_empty() { expression } else { Expr { kind: ExprKind::Chain(Box::new(expression), std::mem::take(&mut links)), span: self.since(start) } };
                expression = Expr { kind: ExprKind::Update { increment, target: Box::new(target) }, span: self.since(start) };
            } else {
                break;
            }
        }
        if links.is_empty() {
            Ok(expression)
        } else {
            Ok(Expr { kind: ExprKind::Chain(Box::new(expression), links), span: self.since(start) })
        }
    }

    fn primary(&mut self) -> Parse<Expr> {
        let token = self.next();
        let span = token.span;
        let at = |kind: ExprKind| Expr { kind, span };
        match token.tok {
            Tok::Int(value) => Ok(at(ExprKind::Number(Number::Int(value)))),
            Tok::Float(value) => {
                if value.is_infinite() {
                    self.report(diag("ts-unsafe-integer", span, "This literal is not a safe integer"));
                }
                Ok(at(ExprKind::Number(Number::Float(value))))
            }
            Tok::BigInt => {
                self.report(diag("ts-unsupported-class", span, "BigInt is not supported").fix("use arrays and records"));
                Ok(at(ExprKind::Rejected))
            }
            Tok::Str(text) => Ok(at(ExprKind::Str(text))),
            Tok::Regex => {
                self.report(diag("ts-regexp", span, "Regular expressions are not supported").fix("includes, startsWith, split"));
                Ok(at(ExprKind::Rejected))
            }
            Tok::Template(quasis, parts) => {
                let mut expressions = Vec::new();
                for part in parts {
                    let saved_tokens = std::mem::replace(&mut self.tokens, part);
                    let saved_at = std::mem::replace(&mut self.at, 0);
                    let parsed = self.expression();
                    let finished = matches!(self.peek().tok, Tok::Eof);
                    let end = self.span();
                    self.tokens = saved_tokens;
                    self.at = saved_at;
                    let expression = parsed?;
                    if !finished {
                        return self.fail(end, "Unexpected token in a template literal");
                    }
                    expressions.push(expression);
                }
                Ok(at(ExprKind::Template(quasis, expressions)))
            }
            Tok::Punct("(") => {
                let inner = self.expression()?;
                self.expect(")")?;
                Ok(Expr { kind: inner.kind, span: inner.span })
            }
            Tok::Punct("[") => {
                let mut items = Vec::new();
                while !self.is_punct("]") {
                    if self.is_punct(",") {
                        let d = diag("ts-unsupported", self.span(), "An array hole is not supported in checkable TypeScript");
                        self.report(d);
                        self.next();
                        continue;
                    }
                    if self.eat("...") {
                        items.push(ArrayItem::Spread(self.assignment()?));
                    } else {
                        items.push(ArrayItem::Item(self.assignment()?));
                    }
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect("]")?;
                Ok(Expr { kind: ExprKind::Array(items), span: self.since(span) })
            }
            Tok::Punct("{") => self.object_literal(span),
            Tok::Ident(word) => self.word(word, span),
            other => {
                let found = Self::describe(&Token { tok: other, span, newline_before: false });
                self.fail(span, format!("Unexpected {found}"))
            }
        }
    }

    fn word(&mut self, word: String, span: Span) -> Parse<Expr> {
        match word.as_str() {
            "true" => Ok(Expr { kind: ExprKind::Bool(true), span }),
            "false" => Ok(Expr { kind: ExprKind::Bool(false), span }),
            "null" => Ok(Expr { kind: ExprKind::Null, span }),
            "function" => {
                let d = diag("ts-unsupported", span, "function expressions are not supported in checkable TypeScript")
                    .fix("use an arrow function");
                self.report(d);
                Err(Stop)
            }
            "class" => {
                self.report(diag("ts-class", span, "Classes are not supported").fix("use object literals"));
                Err(Stop)
            }
            "async" if self.is_word("function") => {
                let d = diag("ts-unsupported", span, "function expressions are not supported in checkable TypeScript")
                    .fix("use an arrow function");
                self.report(d);
                Err(Stop)
            }
            "new" => {
                let callee = self.identifier()?;
                let args = if self.is_punct("(") { self.arguments()? } else { Vec::new() };
                Ok(Expr { kind: ExprKind::New(callee.name, args), span: self.since(span) })
            }
            "import" if self.is_punct("(") => {
                self.arguments()?;
                self.report(diag("ts-eval", span, "Code cannot be built at run time"));
                Ok(Expr { kind: ExprKind::Rejected, span: self.since(span) })
            }
            _ => Ok(Expr { kind: ExprKind::Ident(word), span }),
        }
    }

    fn object_literal(&mut self, start: Span) -> Parse<Expr> {
        let mut items: Vec<ObjectItem> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        while !self.is_punct("}") {
            let key_span = self.span();
            if self.eat("...") {
                items.push(ObjectItem::Spread(self.assignment()?));
            } else if self.is_punct("[") {
                self.report(diag("ts-key", key_span, "This key cannot be a record field"));
                return Err(Stop);
            } else {
                let token = self.next();
                let (key, is_name) = match token.tok {
                    Tok::Ident(name) => (name, true),
                    Tok::Str(text) => (text, false),
                    Tok::Int(_) | Tok::Float(_) => {
                        self.report(diag("ts-key", key_span, "This key cannot be a record field"));
                        return Err(Stop);
                    }
                    _ => return self.fail(key_span, "Expected a property name"),
                };
                if is_integer_like(&key) {
                    self.report(diag("ts-key", key_span, "This key cannot be a record field"));
                }
                if (key == "get" || key == "set") && is_name && matches!(self.peek().tok, Tok::Ident(_)) {
                    self.report(diag("ts-class", key_span, "Classes are not supported").fix("use object literals"));
                    return Err(Stop);
                }
                if self.is_punct("(") {
                    self.report(diag("ts-unsupported", key_span, "methods are not supported in checkable TypeScript").fix("use an arrow function"));
                    return Err(Stop);
                }
                if seen.contains(&key) {
                    self.report(diag("ts-key", key_span, "This key cannot be a record field"));
                }
                seen.push(key.clone());
                let value = if self.eat(":") {
                    self.assignment()?
                } else if is_name {
                    if self.is_punct("=") {
                        self.report(diag("ts-param", self.span(), "Defaults and rest are not supported"));
                        return Err(Stop);
                    }
                    Expr { kind: ExprKind::Ident(key.clone()), span: key_span }
                } else {
                    return self.fail(key_span, "Expected `:` after the key");
                };
                items.push(ObjectItem::Field { key, key_span, value });
            }
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        Ok(Expr { kind: ExprKind::Object(items), span: self.since(start) })
    }

    fn type_error<T>(&mut self, span: Span, what: &str) -> Parse<T> {
        self.report(diag("ts-annotation", span, format!("{what} is not in the annotation grammar")));
        Err(Stop)
    }

    pub fn type_annotation(&mut self) -> Parse<TypeAnn> {
        self.eat("|");
        let mut members = vec![self.type_postfix()?];
        while self.eat("|") {
            members.push(self.type_postfix()?);
        }
        Ok(if members.len() == 1 { members.remove(0) } else { TypeAnn::Union(members) })
    }

    fn type_postfix(&mut self) -> Parse<TypeAnn> {
        let readonly = self.eat_word("readonly");
        let mut base = self.type_atom()?;
        while self.is_punct("[") && self.is_punct_at(1, "]") && !self.peek().newline_before {
            self.next();
            self.next();
            base = TypeAnn::List(Box::new(base));
        }
        if readonly && !matches!(base, TypeAnn::List(_) | TypeAnn::Tuple(_)) {
            return self.type_error(self.span(), "readonly on this type");
        }
        Ok(base)
    }

    fn type_atom(&mut self) -> Parse<TypeAnn> {
        let token = self.next();
        let span = token.span;
        match token.tok {
            Tok::Str(text) => Ok(TypeAnn::Literal(text)),
            Tok::Punct("(") => {
                let inner = self.type_annotation()?;
                if !self.is_punct(")") {
                    return self.type_error(self.span(), "a function type");
                }
                self.next();
                Ok(inner)
            }
            Tok::Punct("[") => {
                let mut elements = Vec::new();
                while !self.is_punct("]") {
                    elements.push(self.type_annotation()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect("]")?;
                Ok(TypeAnn::Tuple(elements))
            }
            Tok::Punct("{") => {
                let mut fields = Vec::new();
                while !self.is_punct("}") {
                    let key_token = self.next();
                    let key = match key_token.tok {
                        Tok::Ident(name) => name,
                        Tok::Str(text) => text,
                        _ => return self.type_error(key_token.span, "this member"),
                    };
                    let optional = self.eat("?");
                    if !self.eat(":") {
                        return self.type_error(self.span(), "a method or index signature");
                    }
                    fields.push((key, optional, self.type_annotation()?));
                    if !(self.eat(";") || self.eat(",")) && !self.peek().newline_before && !self.is_punct("}") {
                        return self.type_error(self.span(), "this member");
                    }
                }
                self.expect("}")?;
                Ok(TypeAnn::Object(fields))
            }
            Tok::Ident(name) if matches!(name.as_str(), "keyof" | "typeof" | "infer" | "unique" | "new" | "abstract") => {
                self.type_error(span, &format!("the type operator {name}"))
            }
            Tok::Ident(name) => {
                let generic = self.is_punct("<");
                let primitive = |name: &str| -> Option<TypeAnn> {
                    Some(match name {
                        "string" => TypeAnn::Primitive("string"),
                        "number" => TypeAnn::Primitive("number"),
                        "boolean" => TypeAnn::Primitive("boolean"),
                        "null" => TypeAnn::Primitive("null"),
                        "Int" => TypeAnn::Primitive("Int"),
                        "Float" => TypeAnn::Primitive("Float"),
                        "unknown" => TypeAnn::Unknown,
                        "any" => TypeAnn::Any,
                        "void" => TypeAnn::Void,
                        _ => return None,
                    })
                };
                if !generic {
                    if self.is_punct(".") || self.is_punct("[") && !self.is_punct_at(1, "]") {
                        return self.type_error(span, "a qualified or indexed type");
                    }
                    return Ok(primitive(&name).unwrap_or(TypeAnn::Name(name)));
                }
                self.next();
                let mut arguments = vec![self.type_annotation()?];
                while self.eat(",") {
                    arguments.push(self.type_annotation()?);
                }
                self.expect(">")?;
                match (name.as_str(), arguments.len()) {
                    ("Array", 1) => Ok(TypeAnn::List(Box::new(arguments.remove(0)))),
                    ("Promise", 1) => Ok(TypeAnn::Promise(Box::new(arguments.remove(0)))),
                    ("Record", 2) if arguments[0] == TypeAnn::Primitive("string") => Ok(TypeAnn::Map(Box::new(arguments.remove(1)))),
                    _ => self.type_error(span, &format!("the generic type {name}")),
                }
            }
            other => {
                let found = Self::describe(&Token { tok: other, span, newline_before: false });
                self.type_error(span, &found)
            }
        }
    }
}

pub fn is_integer_like(key: &str) -> bool {
    key == "0" || (!key.is_empty() && !key.starts_with('0') && key.bytes().all(|b| b.is_ascii_digit()))
}
