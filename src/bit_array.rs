use crate::{BitOption, Expr, Expression};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SegmentKind {
    Int,
    Float,
    Bits,
    Bytes,
    Text(u8),
    Codepoint(u8),
}

impl SegmentKind {
    pub(crate) fn c_name(self) -> &'static str {
        match self {
            Self::Int => "G_BIT_INT",
            Self::Float => "G_BIT_FLOAT",
            Self::Bits => "G_BIT_BITS",
            Self::Bytes => "G_BIT_BYTES",
            Self::Text(8) => "G_BIT_UTF8",
            Self::Text(16) => "G_BIT_UTF16",
            Self::Text(_) => "G_BIT_UTF32",
            Self::Codepoint(8) => "G_BIT_CODEPOINT8",
            Self::Codepoint(16) => "G_BIT_CODEPOINT16",
            Self::Codepoint(_) => "G_BIT_CODEPOINT32",
        }
    }

    pub(crate) fn uses_remainder(self) -> bool {
        matches!(self, Self::Bits | Self::Bytes)
    }
}

pub(crate) struct SegmentOptions<'a> {
    pub kind: SegmentKind,
    pub endian: &'static str,
    pub signed: bool,
    pub size: Option<&'a Expression>,
    pub unit: u64,
}

impl<'a> SegmentOptions<'a> {
    pub(crate) fn parse(
        options: &'a [BitOption],
        string_literal: bool,
    ) -> Result<Self, &'static str> {
        let mut kind = None;
        let mut endian = None;
        let mut signed = None;
        let mut size = None;
        let mut unit = None;
        for option in options {
            let duplicate = match option {
                BitOption::Int => kind.replace(SegmentKind::Int).is_some(),
                BitOption::Float => kind.replace(SegmentKind::Float).is_some(),
                BitOption::Bits => kind.replace(SegmentKind::Bits).is_some(),
                BitOption::Bytes => kind.replace(SegmentKind::Bytes).is_some(),
                BitOption::Utf8 => kind.replace(SegmentKind::Text(8)).is_some(),
                BitOption::Utf16 => kind.replace(SegmentKind::Text(16)).is_some(),
                BitOption::Utf32 => kind.replace(SegmentKind::Text(32)).is_some(),
                BitOption::Utf8Codepoint => kind.replace(SegmentKind::Codepoint(8)).is_some(),
                BitOption::Utf16Codepoint => kind.replace(SegmentKind::Codepoint(16)).is_some(),
                BitOption::Utf32Codepoint => kind.replace(SegmentKind::Codepoint(32)).is_some(),
                BitOption::Big => endian.replace("G_BIT_BIG").is_some(),
                BitOption::Little => endian.replace("G_BIT_LITTLE").is_some(),
                BitOption::Native => endian.replace("G_BIT_NATIVE").is_some(),
                BitOption::Signed => signed.replace(true).is_some(),
                BitOption::Unsigned => signed.replace(false).is_some(),
                BitOption::Size(value) => size.replace(value).is_some(),
                BitOption::Unit(value) => unit.replace(value).is_some(),
            };
            if duplicate {
                return Err("conflicting bit array segment options");
            }
        }
        let kind = kind.unwrap_or(if string_literal {
            SegmentKind::Text(8)
        } else {
            SegmentKind::Int
        });
        if matches!(kind, SegmentKind::Text(_) | SegmentKind::Codepoint(_))
            && (size.is_some() || unit.is_some())
        {
            return Err("UTF bit array segments cannot specify a size or unit");
        }
        if signed.is_some() && kind != SegmentKind::Int {
            return Err("only integer bit array segments can specify signedness");
        }
        if endian.is_some()
            && matches!(
                kind,
                SegmentKind::Bits
                    | SegmentKind::Bytes
                    | SegmentKind::Text(8)
                    | SegmentKind::Codepoint(8)
            )
        {
            return Err("this bit array segment does not support an endianness option");
        }
        let unit = match unit {
            Some(Expression {
                expr: Expr::Int(n), ..
            }) => n.as_u64().filter(|n| (1..=256).contains(n)),
            Some(_) => None,
            None if kind == SegmentKind::Bytes => Some(8),
            None => Some(1),
        }
        .ok_or("bit array unit must be an integer literal from 1 to 256")?;
        Ok(Self {
            kind,
            endian: endian.unwrap_or("G_BIT_BIG"),
            signed: signed.unwrap_or(false),
            size,
            unit,
        })
    }
}
