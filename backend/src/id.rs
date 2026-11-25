//! Row ids. Every key in FileHub is a CUID2: 24 characters, a lowercase letter
//! then lowercase letters and digits. [`Id`] holds one inline and is `Copy`,
//! so it moves around like a number; it is stored as text.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueRef};
use sqlx::{Decode, Encode, Postgres, Type};

/// Longest id accepted (CUID2 allows up to 32).
const MAX: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id {
    len: u8,
    buf: [u8; MAX],
}

impl Id {
    /// A new random id.
    pub fn new() -> Self {
        Self::parse(&cuid2::create_id()).expect("cuid2 makes valid ids")
    }

    /// `s` as an id, if it has the right shape.
    pub fn parse(s: &str) -> Option<Self> {
        if !is_id(s) {
            return None;
        }
        let mut buf = [0u8; MAX];
        buf[..s.len()].copy_from_slice(s.as_bytes());
        Some(Self { len: s.len() as u8, buf })
    }

    pub fn as_str(&self) -> &str {
        // Only ASCII is ever stored.
        std::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
}

impl Default for Id {
    fn default() -> Self { Self::new() }
}

/// True when `s` has the shape of a FileHub id.
pub fn is_id(s: &str) -> bool {
    (2..=MAX).contains(&s.len())
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// A new id as a `String`, for tables keyed by text.
pub fn new_id() -> String { cuid2::create_id() }

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) }
}
impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Id({})", self.as_str()) }
}
impl FromStr for Id {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> { Self::parse(s).ok_or("not a valid id") }
}
impl AsRef<str> for Id {
    fn as_ref(&self) -> &str { self.as_str() }
}
impl PartialEq<str> for Id {
    fn eq(&self, o: &str) -> bool { self.as_str() == o }
}
impl PartialEq<String> for Id {
    fn eq(&self, o: &String) -> bool { self.as_str() == o }
}
impl From<Id> for String {
    fn from(i: Id) -> String { i.as_str().to_string() }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(self.as_str()) }
}
impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = std::borrow::Cow::<'de, str>::deserialize(d)?;
        Id::parse(&s).ok_or_else(|| serde::de::Error::custom("not a valid id"))
    }
}

impl Type<Postgres> for Id {
    fn type_info() -> PgTypeInfo { <String as Type<Postgres>>::type_info() }
    fn compatible(ty: &PgTypeInfo) -> bool { <String as Type<Postgres>>::compatible(ty) }
}
impl PgHasArrayType for Id {
    fn array_type_info() -> PgTypeInfo { <String as PgHasArrayType>::array_type_info() }
    fn array_compatible(ty: &PgTypeInfo) -> bool { <String as PgHasArrayType>::array_compatible(ty) }
}
impl<'q> Encode<'q, Postgres> for Id {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        <&str as Encode<Postgres>>::encode(self.as_str(), buf)
    }
}
impl<'r> Decode<'r, Postgres> for Id {
    fn decode(v: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let s = <&str as Decode<Postgres>>::decode(v)?;
        Id::parse(s).ok_or_else(|| format!("not a valid id: {s}").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shape() {
        let a = Id::new();
        assert_eq!(a.as_str().len(), 24);
        assert!(is_id(a.as_str()));
        assert!(Id::parse("Sys_neb").is_none());
        assert!(Id::parse("1abc").is_none());
        assert!(Id::parse("00000000-0000-7000-8000-000000002001").is_none());
        assert_eq!(serde_json::to_string(&a).unwrap(), format!("\"{a}\""));
    }
}

impl Id {
    /// Like `Uuid::parse_str`: an error when `s` is not an id.
    pub fn parse_str(s: &str) -> Result<Self, &'static str> { s.parse() }
}
