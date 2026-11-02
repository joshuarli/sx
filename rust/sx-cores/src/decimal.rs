use std::cmp::Ordering;
use std::fmt;

pub const MAX_DIGITS: usize = 38;
pub const MAX_SCALE: u32 = 38;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecimalError {
    Syntax(&'static str),
    TooPrecise,
    DivisionByZero,
    Places(i64),
}

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecimalError::Syntax(reason) => f.write_str(reason),
            DecimalError::TooPrecise => {
                write!(f, "the value needs more than {MAX_DIGITS} significant digits or {MAX_SCALE} fractional digits")
            }
            DecimalError::DivisionByZero => f.write_str("division by zero"),
            DecimalError::Places(places) => {
                write!(f, "{places} places is outside 0 to {MAX_SCALE}")
            }
        }
    }
}

impl std::error::Error for DecimalError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    HalfEven,
    HalfUp,
    Down,
    Up,
    Floor,
    Ceiling,
}

impl Rounding {
    pub const ALL: [Rounding; 6] =
        [Rounding::HalfEven, Rounding::HalfUp, Rounding::Down, Rounding::Up, Rounding::Floor, Rounding::Ceiling];

    pub fn variant(self) -> &'static str {
        match self {
            Rounding::HalfEven => "HalfEven",
            Rounding::HalfUp => "HalfUp",
            Rounding::Down => "Down",
            Rounding::Up => "Up",
            Rounding::Floor => "Floor",
            Rounding::Ceiling => "Ceiling",
        }
    }

    pub fn from_variant(name: &str) -> Option<Rounding> {
        Rounding::ALL.into_iter().find(|mode| mode.variant() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Magnitude(Vec<u8>);

impl Magnitude {
    fn zero() -> Magnitude {
        Magnitude(Vec::new())
    }

    fn one() -> Magnitude {
        Magnitude(vec![1])
    }

    fn power_of_ten(exponent: u32) -> Magnitude {
        Magnitude::one().shifted(exponent)
    }

    fn normalized(mut digits: Vec<u8>) -> Magnitude {
        while digits.last() == Some(&0) {
            digits.pop();
        }
        Magnitude(digits)
    }

    fn from_u128(mut n: u128) -> Magnitude {
        let mut digits = Vec::new();
        while n > 0 {
            digits.push((n % 10) as u8);
            n /= 10;
        }
        Magnitude(digits)
    }

    fn to_u128(&self) -> Option<u128> {
        if self.0.len() > MAX_DIGITS {
            return None;
        }
        self.0.iter().rev().try_fold(0u128, |n, digit| n.checked_mul(10)?.checked_add(u128::from(*digit)))
    }

    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    fn is_odd(&self) -> bool {
        self.0.first().is_some_and(|digit| digit % 2 == 1)
    }

    fn shifted(&self, places: u32) -> Magnitude {
        if self.is_zero() {
            return Magnitude::zero();
        }
        let mut digits = vec![0u8; places as usize];
        digits.extend_from_slice(&self.0);
        Magnitude(digits)
    }

    fn compare(&self, other: &Magnitude) -> Ordering {
        self.0.len().cmp(&other.0.len()).then_with(|| self.0.iter().rev().cmp(other.0.iter().rev()))
    }

    fn add(&self, other: &Magnitude) -> Magnitude {
        let mut digits = Vec::with_capacity(self.0.len().max(other.0.len()) + 1);
        let (mut left, mut right) = (self.0.iter(), other.0.iter());
        let mut carry = 0u8;
        loop {
            let (a, b) = (left.next(), right.next());
            if a.is_none() && b.is_none() && carry == 0 {
                break;
            }
            let sum = a.copied().unwrap_or(0) + b.copied().unwrap_or(0) + carry;
            digits.push(sum % 10);
            carry = sum / 10;
        }
        Magnitude(digits)
    }

    fn subtract(&self, smaller: &Magnitude) -> Magnitude {
        let mut digits = Vec::with_capacity(self.0.len());
        let mut right = smaller.0.iter();
        let mut borrow = 0u8;
        for digit in &self.0 {
            let taken = right.next().copied().unwrap_or(0) + borrow;
            if *digit >= taken {
                digits.push(digit - taken);
                borrow = 0;
            } else {
                digits.push(digit + 10 - taken);
                borrow = 1;
            }
        }
        Magnitude::normalized(digits)
    }

    fn multiply(&self, other: &Magnitude) -> Magnitude {
        let mut cells = vec![0u32; self.0.len() + other.0.len()];
        for (offset, left) in self.0.iter().enumerate() {
            let mut carry = 0u32;
            for (inner, right) in other.0.iter().enumerate() {
                if let Some(cell) = cells.get_mut(offset + inner) {
                    let total = *cell + u32::from(*left) * u32::from(*right) + carry;
                    *cell = total % 10;
                    carry = total / 10;
                }
            }
            if let Some(cell) = cells.get_mut(offset + other.0.len()) {
                *cell += carry;
            }
        }
        Magnitude::normalized(cells.into_iter().map(|cell| cell as u8).collect())
    }

    fn divide(&self, divisor: &Magnitude) -> (Magnitude, Magnitude) {
        let mut quotient = Vec::with_capacity(self.0.len());
        let mut remainder = Magnitude::zero();
        for digit in self.0.iter().rev() {
            let mut shifted = Vec::with_capacity(remainder.0.len() + 1);
            shifted.push(*digit);
            shifted.extend_from_slice(&remainder.0);
            remainder = Magnitude::normalized(shifted);
            let mut count = 0u8;
            while remainder.compare(divisor) != Ordering::Less {
                remainder = remainder.subtract(divisor);
                count += 1;
            }
            quotient.push(count);
        }
        quotient.reverse();
        (Magnitude::normalized(quotient), remainder)
    }

    fn digits_text(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        self.0.iter().rev().map(|digit| char::from(b'0' + digit)).collect()
    }
}

struct Exact {
    negative: bool,
    magnitude: Magnitude,
    scale: u32,
}

impl Exact {
    fn aligned(&self, scale: u32) -> Magnitude {
        self.magnitude.shifted(scale.saturating_sub(self.scale))
    }

    fn canonical(mut self) -> Result<Decimal, DecimalError> {
        while self.scale > 0 && self.magnitude.0.first() == Some(&0) {
            self.magnitude.0.remove(0);
            self.scale -= 1;
        }
        if self.magnitude.is_zero() {
            self.scale = 0;
            self.negative = false;
        }
        let magnitude = self.magnitude.to_u128().ok_or(DecimalError::TooPrecise)?;
        if self.scale > MAX_SCALE {
            return Err(DecimalError::TooPrecise);
        }
        let mantissa = i128::try_from(magnitude).map_err(|_| DecimalError::TooPrecise)?;
        Ok(Decimal { mantissa: if self.negative { -mantissa } else { mantissa }, scale: self.scale })
    }
}

fn rounded_quotient(
    quotient: Magnitude,
    remainder: &Magnitude,
    divisor: &Magnitude,
    negative: bool,
    mode: Rounding,
) -> Magnitude {
    let increment = if remainder.is_zero() {
        false
    } else {
        match mode {
            Rounding::Down => false,
            Rounding::Up => true,
            Rounding::Floor => negative,
            Rounding::Ceiling => !negative,
            Rounding::HalfUp | Rounding::HalfEven => match remainder.add(remainder).compare(divisor) {
                Ordering::Less => false,
                Ordering::Greater => true,
                Ordering::Equal => mode == Rounding::HalfUp || quotient.is_odd(),
            },
        }
    };
    if increment { quotient.add(&Magnitude::one()) } else { quotient }
}

fn places_of(places: i64) -> Result<u32, DecimalError> {
    u32::try_from(places).ok().filter(|places| *places <= MAX_SCALE).ok_or(DecimalError::Places(places))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decimal {
    mantissa: i128,
    scale: u32,
}

impl Decimal {
    pub fn from_int(n: i64) -> Decimal {
        Decimal { mantissa: i128::from(n), scale: 0 }
    }

    pub fn parse(text: &str) -> Result<Decimal, DecimalError> {
        let unsigned = text.strip_prefix('-');
        let negative = unsigned.is_some();
        let body = unsigned.unwrap_or(text);
        let (whole, fraction) = match body.split_once('.') {
            Some((whole, fraction)) => (whole, fraction),
            None => (body, ""),
        };
        let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        if body.is_empty() || whole.is_empty() || !digits(whole) || !digits(fraction) {
            return Err(DecimalError::Syntax("write an optional minus sign, digits, and optionally a point and digits"));
        }
        if body.contains('.') && fraction.is_empty() {
            return Err(DecimalError::Syntax("a point needs digits after it"));
        }
        let all: Vec<u8> = whole.bytes().chain(fraction.bytes()).rev().map(|byte| byte - b'0').collect();
        let scale = u32::try_from(fraction.len()).map_err(|_| DecimalError::TooPrecise)?;
        Exact { negative, magnitude: Magnitude::normalized(all), scale }.canonical()
    }

    fn exact(self) -> Exact {
        Exact { negative: self.mantissa < 0, magnitude: Magnitude::from_u128(self.mantissa.unsigned_abs()), scale: self.scale }
    }

    pub fn show(self) -> String {
        self.fixed(self.scale)
    }

    fn fixed(self, places: u32) -> String {
        let exact = self.exact();
        let mut digits = exact.magnitude.digits_text();
        let padding = (exact.scale as usize + 1).saturating_sub(digits.len());
        digits.insert_str(0, &"0".repeat(padding));
        let point = digits.len() - exact.scale as usize;
        let (whole, fraction) = digits.split_at(point);
        let sign = if exact.negative { "-" } else { "" };
        let zeros = "0".repeat(places.saturating_sub(exact.scale) as usize);
        if places == 0 { format!("{sign}{whole}") } else { format!("{sign}{whole}.{fraction}{zeros}") }
    }

    pub fn format(self, places: i64) -> Result<String, DecimalError> {
        let places = places_of(places)?;
        let rounded = self.round_to(places, Rounding::HalfEven)?;
        Ok(rounded.fixed(places))
    }

    pub fn compare(self, other: Decimal) -> Ordering {
        let (left, right) = (self.exact(), other.exact());
        match (left.negative, right.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (negative, _) => {
                let scale = left.scale.max(right.scale);
                let order = left.aligned(scale).compare(&right.aligned(scale));
                if negative { order.reverse() } else { order }
            }
        }
    }

    fn signed_sum(left: &Exact, right: &Exact) -> Result<Decimal, DecimalError> {
        let scale = left.scale.max(right.scale);
        let (a, b) = (left.aligned(scale), right.aligned(scale));
        let (negative, magnitude) = if left.negative == right.negative {
            (left.negative, a.add(&b))
        } else {
            match a.compare(&b) {
                Ordering::Equal => (false, Magnitude::zero()),
                Ordering::Greater => (left.negative, a.subtract(&b)),
                Ordering::Less => (right.negative, b.subtract(&a)),
            }
        };
        Exact { negative, magnitude, scale }.canonical()
    }

    pub fn sum(self, other: Decimal) -> Result<Decimal, DecimalError> {
        Decimal::signed_sum(&self.exact(), &other.exact())
    }

    pub fn subtract(self, other: Decimal) -> Result<Decimal, DecimalError> {
        let mut negated = other.exact();
        negated.negative = !negated.negative;
        Decimal::signed_sum(&self.exact(), &negated)
    }

    pub fn multiply(self, other: Decimal) -> Result<Decimal, DecimalError> {
        let (left, right) = (self.exact(), other.exact());
        Exact {
            negative: left.negative != right.negative,
            magnitude: left.magnitude.multiply(&right.magnitude),
            scale: left.scale + right.scale,
        }
        .canonical()
    }

    pub fn divide(self, other: Decimal, places: i64, mode: Rounding) -> Result<Decimal, DecimalError> {
        let places = places_of(places)?;
        let (left, right) = (self.exact(), other.exact());
        if right.magnitude.is_zero() {
            return Err(DecimalError::DivisionByZero);
        }
        let dividend = left.magnitude.shifted(right.scale + places);
        let divisor = right.magnitude.shifted(left.scale);
        let (quotient, remainder) = dividend.divide(&divisor);
        let negative = left.negative != right.negative;
        let magnitude = rounded_quotient(quotient, &remainder, &divisor, negative, mode);
        Exact { negative, magnitude, scale: places }.canonical()
    }

    fn round_to(self, places: u32, mode: Rounding) -> Result<Decimal, DecimalError> {
        if places >= self.scale {
            return Ok(self);
        }
        let exact = self.exact();
        let divisor = Magnitude::power_of_ten(self.scale - places);
        let (quotient, remainder) = exact.magnitude.divide(&divisor);
        let magnitude = rounded_quotient(quotient, &remainder, &divisor, exact.negative, mode);
        Exact { negative: exact.negative, magnitude, scale: places }.canonical()
    }

    pub fn round(self, places: i64, mode: Rounding) -> Result<Decimal, DecimalError> {
        self.round_to(places_of(places)?, mode)
    }

    pub fn to_float(self) -> Option<f64> {
        self.show().parse::<f64>().ok().filter(|value| value.is_finite())
    }
}
