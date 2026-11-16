//! Kleine Formelsprache für die Zeichen-Op `shader`: ein Ausdruck, der für
//! jeden Pixel eine Zahl ergibt.
//!
//! Es gibt nur Zahlen, feste Variablen, Rechenzeichen und eine feste Liste
//! von Funktionen. Keine Schleifen, keine Zuweisungen, kein Zugriff auf
//! Dateien oder das System: ein Ausdruck kann nichts außer rechnen, und seine
//! Größe ist begrenzt.

use crate::{Error, Result};

/// Längster erlaubter Ausdruck in Zeichen.
pub const MAX_SOURCE: usize = 2000;
/// Meiste erlaubte Rechenschritte in einem Ausdruck.
pub const MAX_NODES: usize = 1000;
const MAX_DEPTH: usize = 64;

/// Die Variablen, in dieser Reihenfolge in [`Env::vars`].
pub const VARIABLES: [&str; 10] = ["x", "y", "w", "h", "frame", "frames", "t", "cx", "cy", "pi"];
const X: usize = 0;
const Y: usize = 1;
const CX: usize = 7;
const CY: usize = 8;

/// Werte, mit denen ein Ausdruck für einen Pixel gerechnet wird.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    pub vars: [f64; VARIABLES.len()],
    pub seed: u64,
}

impl Env {
    /// Umgebung für ein Bild von `w × h` Pixeln und Frame `frame` von `frames`.
    pub fn new(w: u32, h: u32, frame: usize, frames: usize, seed: u64) -> Self {
        let frames = frames.max(1);
        let mut vars = [0.0; VARIABLES.len()];
        vars[2] = w as f64;
        vars[3] = h as f64;
        vars[4] = frame as f64;
        vars[5] = frames as f64;
        vars[6] = frame as f64 / frames as f64;
        vars[CX] = (w as f64 - 1.0) / 2.0;
        vars[CY] = (h as f64 - 1.0) / 2.0;
        vars[9] = std::f64::consts::PI;
        Self { vars, seed }
    }

    pub fn at(mut self, x: u32, y: u32) -> Self {
        self.vars[X] = x as f64;
        self.vars[Y] = y as f64;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Func {
    Abs,
    Floor,
    Ceil,
    Round,
    Sqrt,
    Sin,
    Cos,
    Sign,
    Min,
    Max,
    Pow,
    Mod,
    Clamp,
    Mix,
    Step,
    If,
    Dist,
    Angle,
    Rand,
    Noise,
    Bayer,
}

/// Name, Funktion, wenigste und meiste Argumente.
const FUNCTIONS: [(&str, Func, usize, usize); 21] = [
    ("abs", Func::Abs, 1, 1),
    ("floor", Func::Floor, 1, 1),
    ("ceil", Func::Ceil, 1, 1),
    ("round", Func::Round, 1, 1),
    ("sqrt", Func::Sqrt, 1, 1),
    ("sin", Func::Sin, 1, 1),
    ("cos", Func::Cos, 1, 1),
    ("sign", Func::Sign, 1, 1),
    ("min", Func::Min, 2, 8),
    ("max", Func::Max, 2, 8),
    ("pow", Func::Pow, 2, 2),
    ("mod", Func::Mod, 2, 2),
    ("clamp", Func::Clamp, 3, 3),
    ("mix", Func::Mix, 3, 3),
    ("step", Func::Step, 2, 2),
    ("if", Func::If, 3, 3),
    ("dist", Func::Dist, 0, 2),
    ("angle", Func::Angle, 0, 2),
    ("rand", Func::Rand, 0, 1),
    ("noise", Func::Noise, 1, 2),
    ("bayer", Func::Bayer, 0, 1),
];

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Num(f64),
    Var(usize),
    Neg(Box<Node>),
    Not(Box<Node>),
    Bin(Bin, Box<Node>, Box<Node>),
    Cond(Box<Node>, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

/// Ein geprüfter Ausdruck, bereit zum Ausrechnen.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    root: Node,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(f64),
    Name(String),
    Sym(&'static str),
}

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Formula(message.into()))
}

fn tokens(source: &str) -> Result<Vec<Token>> {
    const SYMBOLS: [&str; 22] = [
        "<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "^", "<", ">", "!", "(", ")",
        ",", "?", ":", "=", "&",
    ];
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            match text.parse::<f64>() {
                Ok(n) => out.push(Token::Num(n)),
                Err(_) => return fail(format!("'{text}' is not a number")),
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            match SYMBOLS.iter().find(|s| rest.starts_with(**s)) {
                Some(&"=") => return fail("use == to compare"),
                Some(&"&") => return fail("use && for 'and'"),
                Some(sym) => {
                    out.push(Token::Sym(sym));
                    i += sym.len();
                }
                None => return fail(format!("unexpected character '{c}'")),
            }
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    nodes: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn eat(&mut self, sym: &str) -> bool {
        let hit = matches!(self.peek(), Some(Token::Sym(s)) if *s == sym);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn expect(&mut self, sym: &str) -> Result<()> {
        if self.eat(sym) {
            Ok(())
        } else {
            fail(format!("expected '{sym}'{}", self.found()))
        }
    }

    fn found(&self) -> String {
        match self.peek() {
            Some(Token::Num(n)) => format!(", found {n}"),
            Some(Token::Name(n)) => format!(", found '{n}'"),
            Some(Token::Sym(s)) => format!(", found '{s}'"),
            None => ", but the formula ends".into(),
        }
    }

    fn node(&mut self, node: Node) -> Result<Node> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return fail(format!(
                "the formula is too large (more than {MAX_NODES} steps)"
            ));
        }
        Ok(node)
    }

    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return fail("the formula is nested too deeply");
        }
        let out = f(self);
        self.depth -= 1;
        out
    }

    fn ternary(&mut self) -> Result<Node> {
        self.nested(|p| {
            let cond = p.binary(0)?;
            if !p.eat("?") {
                return Ok(cond);
            }
            let yes = p.ternary()?;
            p.expect(":")?;
            let no = p.ternary()?;
            p.node(Node::Cond(Box::new(cond), Box::new(yes), Box::new(no)))
        })
    }

    /// Zweistellige Rechenzeichen, von schwacher zu starker Bindung.
    fn binary(&mut self, level: usize) -> Result<Node> {
        const LEVELS: [&[(&str, Bin)]; 5] = [
            &[("||", Bin::Or)],
            &[("&&", Bin::And)],
            &[
                ("<=", Bin::Le),
                (">=", Bin::Ge),
                ("==", Bin::Eq),
                ("!=", Bin::Ne),
                ("<", Bin::Lt),
                (">", Bin::Gt),
            ],
            &[("+", Bin::Add), ("-", Bin::Sub)],
            &[("*", Bin::Mul), ("/", Bin::Div), ("%", Bin::Rem)],
        ];
        let Some(ops) = LEVELS.get(level) else {
            return self.unary();
        };
        let mut left = self.binary(level + 1)?;
        while let Some(&(_, op)) = ops.iter().find(|(sym, _)| self.eat(sym)) {
            let right = self.binary(level + 1)?;
            left = self.node(Node::Bin(op, Box::new(left), Box::new(right)))?;
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node> {
        self.nested(|p| {
            if p.eat("-") {
                let inner = p.unary()?;
                return p.node(Node::Neg(Box::new(inner)));
            }
            if p.eat("!") {
                let inner = p.unary()?;
                return p.node(Node::Not(Box::new(inner)));
            }
            let base = p.atom()?;
            if p.eat("^") {
                let exponent = p.unary()?;
                return p.node(Node::Bin(Bin::Pow, Box::new(base), Box::new(exponent)));
            }
            Ok(base)
        })
    }

    fn atom(&mut self) -> Result<Node> {
        let token = self.peek().cloned();
        match token {
            Some(Token::Num(n)) => {
                self.pos += 1;
                self.node(Node::Num(n))
            }
            Some(Token::Sym("(")) => {
                self.pos += 1;
                let inner = self.ternary()?;
                self.expect(")")?;
                Ok(inner)
            }
            Some(Token::Name(name)) => {
                self.pos += 1;
                if !self.eat("(") {
                    return match VARIABLES.iter().position(|v| *v == name) {
                        Some(i) => self.node(Node::Var(i)),
                        None => fail(format!(
                            "unknown variable '{name}'. Variables: {}",
                            VARIABLES.join(", ")
                        )),
                    };
                }
                let Some(&(_, func, min, max)) = FUNCTIONS.iter().find(|f| f.0 == name) else {
                    let names: Vec<&str> = FUNCTIONS.iter().map(|f| f.0).collect();
                    return fail(format!(
                        "unknown function '{name}'. Functions: {}",
                        names.join(", ")
                    ));
                };
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(self.ternary()?);
                        if self.eat(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                if args.len() < min || args.len() > max {
                    return fail(if min == max {
                        format!("{name}() takes {min} argument(s), got {}", args.len())
                    } else {
                        format!(
                            "{name}() takes {min} to {max} arguments, got {}",
                            args.len()
                        )
                    });
                }
                self.node(Node::Call(func, args))
            }
            _ => fail(format!("expected a value{}", self.found())),
        }
    }
}

impl Expr {
    pub fn parse(source: &str) -> Result<Self> {
        if source.chars().count() > MAX_SOURCE {
            return fail(format!(
                "the formula is longer than {MAX_SOURCE} characters"
            ));
        }
        let mut parser = Parser {
            tokens: tokens(source)?,
            pos: 0,
            nodes: 0,
            depth: 0,
        };
        if parser.tokens.is_empty() {
            return fail("the formula is empty");
        }
        let root = parser.ternary()?;
        if parser.pos < parser.tokens.len() {
            return fail(format!(
                "unexpected text after the formula{}",
                parser.found()
            ));
        }
        Ok(Self { root })
    }

    pub fn eval(&self, env: &Env) -> f64 {
        eval(&self.root, env)
    }
}

fn truth(v: f64) -> bool {
    v != 0.0 && !v.is_nan()
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn eval(node: &Node, env: &Env) -> f64 {
    match node {
        Node::Num(n) => *n,
        Node::Var(i) => env.vars[*i],
        Node::Neg(a) => -eval(a, env),
        Node::Not(a) => flag(!truth(eval(a, env))),
        Node::Cond(c, a, b) => {
            if truth(eval(c, env)) {
                eval(a, env)
            } else {
                eval(b, env)
            }
        }
        Node::Bin(op, a, b) => {
            let a = eval(a, env);
            // „und“ und „oder“ rechnen die rechte Seite nur aus, wenn sie zählt.
            match op {
                Bin::And => return flag(truth(a) && truth(eval(b, env))),
                Bin::Or => return flag(truth(a) || truth(eval(b, env))),
                _ => {}
            }
            let b = eval(b, env);
            match op {
                Bin::Add => a + b,
                Bin::Sub => a - b,
                Bin::Mul => a * b,
                Bin::Div => a / b,
                Bin::Rem => a.rem_euclid(b),
                Bin::Pow => a.powf(b),
                Bin::Lt => flag(a < b),
                Bin::Le => flag(a <= b),
                Bin::Gt => flag(a > b),
                Bin::Ge => flag(a >= b),
                Bin::Eq => flag(a == b),
                Bin::Ne => flag(a != b),
                Bin::And | Bin::Or => unreachable!("handled above"),
            }
        }
        Node::Call(func, args) => {
            let arg = |i: usize| eval(&args[i], env);
            let (x, y) = (env.vars[X], env.vars[Y]);
            // Ohne Angabe ist der Bezugspunkt die Bildmitte.
            let point = || match args.len() {
                2 => (arg(0), arg(1)),
                _ => (env.vars[CX], env.vars[CY]),
            };
            match func {
                Func::Abs => arg(0).abs(),
                Func::Floor => arg(0).floor(),
                Func::Ceil => arg(0).ceil(),
                Func::Round => arg(0).round(),
                Func::Sqrt => arg(0).sqrt(),
                Func::Sin => arg(0).sin(),
                Func::Cos => arg(0).cos(),
                Func::Sign => {
                    let v = arg(0);
                    flag(v > 0.0) - flag(v < 0.0)
                }
                Func::Min => (0..args.len()).map(arg).fold(f64::INFINITY, f64::min),
                Func::Max => (0..args.len()).map(arg).fold(f64::NEG_INFINITY, f64::max),
                Func::Pow => arg(0).powf(arg(1)),
                Func::Mod => arg(0).rem_euclid(arg(1)),
                Func::Clamp => arg(0).max(arg(1)).min(arg(2)),
                Func::Mix => {
                    let (a, b, t) = (arg(0), arg(1), arg(2));
                    a + (b - a) * t
                }
                Func::Step => flag(arg(1) >= arg(0)),
                Func::If => {
                    if truth(arg(0)) {
                        arg(1)
                    } else {
                        arg(2)
                    }
                }
                Func::Dist => {
                    let (px, py) = point();
                    ((x - px).powi(2) + (y - py).powi(2)).sqrt()
                }
                Func::Angle => {
                    let (px, py) = point();
                    (y - py).atan2(x - px)
                }
                Func::Rand => {
                    let k = if args.is_empty() { 0.0 } else { arg(0) };
                    unit(hash(env.seed, x as i64, y as i64, k.to_bits()))
                }
                Func::Noise => {
                    let scale = arg(0).abs().max(1.0);
                    let k = if args.len() > 1 { arg(1) } else { 0.0 };
                    value_noise(env.seed, x / scale, y / scale, k.to_bits())
                }
                Func::Bayer => {
                    let size = if args.is_empty() { 4.0 } else { arg(0) };
                    let bits = if size >= 8.0 {
                        3
                    } else if size >= 4.0 {
                        2
                    } else {
                        1
                    };
                    bayer_threshold(x as i64, y as i64, bits)
                }
            }
        }
    }
}

fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Fester Zufallswert für einen Pixel: gleiche Eingaben, gleiches Ergebnis,
/// auf jedem Rechner.
pub fn hash(seed: u64, x: i64, y: i64, k: u64) -> u64 {
    mix64(mix64(mix64(mix64(seed) ^ x as u64) ^ y as u64) ^ k)
}

/// Bringt einen Zufallswert in den Bereich 0 bis unter 1.
pub fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Weiches Rauschen zwischen 0 und 1: Zufallswerte auf einem Gitter, dazwischen
/// geglättet. Gut für Glut, Wolken und fleckigen Stein.
fn value_noise(seed: u64, x: f64, y: f64, k: u64) -> f64 {
    if !x.is_finite() || !y.is_finite() {
        return 0.0;
    }
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(fx), smooth(fy));
    let corner = |dx: i64, dy: i64| unit(hash(seed, x0 as i64 + dx, y0 as i64 + dy, k));
    let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * sx;
    let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Schwelle des Bayer-Musters mit Kantenlänge `2^bits`, zwischen 0 und 1.
pub fn bayer_threshold(x: i64, y: i64, bits: u32) -> f64 {
    const CELL: [[u64; 2]; 2] = [[0, 2], [3, 1]];
    let mut value = 0u64;
    for bit in 0..bits {
        let (bx, by) = ((x >> bit) & 1, (y >> bit) & 1);
        value += CELL[by as usize][bx as usize] << (2 * (bits - 1 - bit));
    }
    (value as f64 + 0.5) / (1u64 << (2 * bits)) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(source: &str) -> f64 {
        let env = Env::new(8, 4, 1, 4, 7).at(3, 2);
        Expr::parse(source).unwrap().eval(&env)
    }

    #[test]
    fn arithmetic_follows_the_usual_rules() {
        assert_eq!(calc("1 + 2 * 3"), 7.0);
        assert_eq!(calc("(1 + 2) * 3"), 9.0);
        assert_eq!(calc("-2 ^ 2"), -4.0);
        assert_eq!(calc("2 ^ 3 ^ 2"), 512.0);
        assert_eq!(calc("7 % 4 + -1 % 4"), 6.0);
        assert_eq!(calc("x + y * 10"), 23.0);
        assert_eq!(calc("w * h + frame + frames"), 37.0);
        assert_eq!(calc("t"), 0.25);
        assert_eq!(calc("x > 2 && y < 3 ? 5 : 6"), 5.0);
        assert_eq!(calc("x > 5 || !(y == 2) ? 5 : 6"), 6.0);
        assert_eq!(calc("if(x != 3, 1, clamp(9, 0, 4))"), 4.0);
        assert_eq!(calc("min(4, 2, 8) + max(1, 3) + abs(-2) + floor(1.9)"), 8.0);
        assert_eq!(calc("dist(0, 2)"), 3.0);
        assert_eq!(calc("round(dist() * dist() * 2)"), 1.0);
        assert_eq!(calc("step(3, x) + step(4, x)"), 1.0);
    }

    #[test]
    fn random_values_are_fixed_per_pixel_and_seed() {
        let a = calc("rand()");
        assert_eq!(a, calc("rand()"));
        assert!((0.0..1.0).contains(&a));
        assert_ne!(a, calc("rand(1)"));
        let other = Expr::parse("rand()")
            .unwrap()
            .eval(&Env::new(8, 4, 1, 4, 8).at(3, 2));
        assert_ne!(a, other);
        let n = calc("noise(4)");
        assert!((0.0..=1.0).contains(&n));
        assert_eq!(calc("bayer(2)"), 0.625);
    }

    #[test]
    fn mistakes_are_explained() {
        let err = |source: &str| Expr::parse(source).unwrap_err().to_string();
        assert!(err("z + 1").contains("unknown variable 'z'"));
        assert!(err("open(1)").contains("unknown function 'open'"));
        assert!(err("min(1)").contains("2 to 8"));
        assert!(err("1 +").contains("formula ends"));
        assert!(err("(1").contains("expected ')'"));
        assert!(err("x = 1").contains("=="));
        assert!(err("x; y").contains("unexpected character"));
        assert!(err("").contains("empty"));
        assert!(err(&"(".repeat(200)).contains("nested"));
        assert!(err(&"1+".repeat(1200)).contains("longer"));
        assert!(err(&format!("{}1", "1+".repeat(900))).contains("too large"));
    }
}
