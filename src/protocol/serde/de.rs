/// Implements the de-serialization of the TERA network protocol using serde.
use std::str;

use byteorder::{ByteOrder, LittleEndian};
use serde::de::IntoDeserializer;
use serde::{self, Deserialize};

use super::error::{Error, Result};

/// A Deserializer that reads bytes from a vector.
#[derive(Clone, Debug)]
pub struct Deserializer {
    data: Vec<u8>,
    pos: usize,
}

// TODO we are currently too trustworthy with the client data and need to fet it more (we sometimes can get out of a slice boundary!)

/// Parses the given `Vec<u8>`
pub fn from_vec<'a, T>(v: Vec<u8>) -> Result<T>
where
    T: Deserialize<'a>,
{
    let mut deserializer = Deserializer::from_vec(v);
    let t = T::deserialize(&mut deserializer)?;
    Ok(t)
}

impl<'de> Deserializer {
    /// Creates a new Deserializer with a given `Vec<u8>`.
    pub fn from_vec(r: Vec<u8>) -> Self {
        Deserializer { data: r, pos: 0 }
    }

    fn abs_offset(&self, offset: usize) -> usize {
        // The array we have doesn't include the leading opcode / length u16, so -4 bytes
        if offset == 0 {
            offset
        } else {
            offset - 4
        }
    }
}

macro_rules! impl_nums {
    ($ty:ty, $dser_method:ident, $visitor_method:ident, $reader_method:ident, $size:literal) => {
        #[inline]
        fn $dser_method<V>(self, visitor: V) -> Result<V::Value>
        where
            V: serde::de::Visitor<'de>,
        {
            let d = LittleEndian::$reader_method(&self.data[self.pos..self.pos + $size]);
            self.pos += $size;
            visitor.$visitor_method(d)
        }
    };
}

impl<'de, 'a> serde::Deserializer<'de> for &'a mut Deserializer {
    type Error = Error;

    #[inline]
    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(Error::DeserializeAnyNotSupported(self.pos))
    }

    #[inline]
    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let pos = self.pos;
        let value: u8 = serde::Deserialize::deserialize(self)?;
        match value {
            1 => visitor.visit_bool(true),
            0 => visitor.visit_bool(false),
            v => Err(Error::InvalidBoolEncoding(v, pos)),
        }
    }

    #[inline]
    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        self.pos += 1;
        visitor.visit_i8(self.data[self.pos - 1] as i8)
    }

    #[inline]
    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        self.pos += 1;
        visitor.visit_u8(self.data[self.pos - 1])
    }

    impl_nums!(u16, deserialize_u16, visit_u16, read_u16, 2);
    impl_nums!(u32, deserialize_u32, visit_u32, read_u32, 4);
    impl_nums!(u64, deserialize_u64, visit_u64, read_u64, 8);
    impl_nums!(i16, deserialize_i16, visit_i16, read_i16, 2);
    impl_nums!(i32, deserialize_i32, visit_i32, read_i32, 4);
    impl_nums!(i64, deserialize_i64, visit_i64, read_i64, 8);
    impl_nums!(f32, deserialize_f32, visit_f32, read_f32, 4);
    impl_nums!(f64, deserialize_f64, visit_f64, read_f64, 8);

    #[inline]
    fn deserialize_char<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(Error::DeserializeCharNotSupported(self.pos))
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let tmp_offset = LittleEndian::read_u16(&self.data[self.pos..self.pos + 2]) as usize;
        let abs_pos = self.abs_offset(tmp_offset as usize);
        self.pos += 2;

        if abs_pos >= self.data.len() {
            return Err(Error::OffsetOutsideData(self.pos, abs_pos));
        }

        for i in (abs_pos..self.data.len()).step_by(2) {
            // Look for null terminator
            if self.data[i] == 0 && self.data[i + 1] == 0 {
                let mut aligned = vec![0u16; (i - abs_pos) / 2];
                for (j, el) in aligned.iter_mut().enumerate() {
                    *el = LittleEndian::read_u16(&self.data[abs_pos + j * 2..abs_pos + j * 2 + 2]);
                }
                let mut utf8 = vec![0u8; aligned.len() * 3];
                let size = ucs2::decode(&aligned, &mut utf8).unwrap();
                let s: &str;

                unsafe {
                    s = str::from_utf8_unchecked(&utf8[..size]);
                }

                return visitor.visit_string(s.to_string());
            }
        }
        Err(Error::StringNotNullTerminated(self.pos))
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        // Use byte_buf. Could be useful for arrays though.
        Err(Error::DeserializeBytesNotSupported(self.pos))
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let tmp_offset = LittleEndian::read_u16(&self.data[self.pos..self.pos + 2]) as usize;
        let abs_offset = self.abs_offset(tmp_offset as usize);
        self.pos += 2;

        let len = LittleEndian::read_u16(&self.data[self.pos..self.pos + 2]) as usize;
        self.pos += 2;

        if (abs_offset + len as usize) > self.data.len() {
            return Err(Error::BytesTooBig(self.pos));
        };

        let b = &self.data[abs_offset..abs_offset + len as usize];
        visitor.visit_byte_buf(b.to_vec())
    }

    fn deserialize_option<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(Error::DeserializeOptionNotSupported(self.pos))
    }

    #[inline]
    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V>(self, _name: &str, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        struct Access<'a> {
            deserializer: &'a mut Deserializer,
            count: usize,
            data_len: usize,
            next_offset: usize,
            old_pos: usize,
        }

        impl<'de, 'a, 'b: 'a> serde::de::SeqAccess<'de> for Access<'a> {
            type Error = Error;

            fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
            where
                T: serde::de::DeserializeSeed<'de>,
            {
                if self.count > 0 {
                    self.count -= 1;

                    // The array is a linked list
                    if self.next_offset >= self.data_len {
                        return Err(Error::OffsetOutsideData(
                            self.deserializer.pos,
                            self.next_offset,
                        ));
                    }
                    self.deserializer.pos = self.next_offset;

                    let tmp_offset: usize = LittleEndian::read_u16(
                        &self.deserializer.data[self.deserializer.pos..self.deserializer.pos + 2],
                    ) as usize;
                    let abs_offset: usize = self.deserializer.abs_offset(tmp_offset);
                    self.deserializer.pos += 2;

                    if abs_offset != self.next_offset {
                        return Err(Error::InvalidSeqEntry(abs_offset));
                    }

                    let tmp_offset: usize = LittleEndian::read_u16(
                        &self.deserializer.data[self.deserializer.pos..self.deserializer.pos + 2],
                    ) as usize;
                    let abs_offset: usize = self.deserializer.abs_offset(tmp_offset);
                    self.next_offset = abs_offset;
                    self.deserializer.pos += 2;

                    let value =
                        serde::de::DeserializeSeed::deserialize(seed, &mut *self.deserializer)?;
                    Ok(Some(value))
                } else {
                    // Return to the end of the array header
                    self.deserializer.pos = self.old_pos;
                    Ok(None)
                }
            }

            fn size_hint(&self) -> Option<usize> {
                Some(self.count)
            }
        }

        let count: usize = LittleEndian::read_u16(&self.data[self.pos..self.pos + 2]) as usize;
        self.pos += 2;
        let tmp_offset: usize = LittleEndian::read_u16(&self.data[self.pos..self.pos + 2]) as usize;
        let next_offset: usize = self.abs_offset(tmp_offset);
        self.pos += 2;

        let old_pos = self.pos;
        let data_len = self.data.len();

        visitor.visit_seq(Access {
            deserializer: self,
            count,
            data_len,
            next_offset,
            old_pos,
        })
    }

    fn deserialize_tuple<V>(self, count: usize, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        struct Access<'a> {
            deserializer: &'a mut Deserializer,
            count: usize,
        }

        impl<'de, 'a, 'b: 'a> serde::de::SeqAccess<'de> for Access<'a> {
            type Error = Error;

            fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
            where
                T: serde::de::DeserializeSeed<'de>,
            {
                if self.count > 0 {
                    self.count -= 1;
                    let value =
                        serde::de::DeserializeSeed::deserialize(seed, &mut *self.deserializer)?;
                    Ok(Some(value))
                } else {
                    Ok(None)
                }
            }

            fn size_hint(&self) -> Option<usize> {
                Some(self.count)
            }
        }

        visitor.visit_seq(Access {
            deserializer: self,
            count,
        })
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(Error::DeserializeMapNotSupported(self.pos))
    }

    fn deserialize_struct<V>(
        self,
        _name: &str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_tuple(fields.len(), visitor)
    }

    fn deserialize_enum<V>(
        self,
        _enum: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        impl<'de, 'a> serde::de::EnumAccess<'de> for &'a mut Deserializer {
            type Error = Error;
            type Variant = Self;

            fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
            where
                V: serde::de::DeserializeSeed<'de>,
            {
                // Enums in packets have to be a u32!
                let idx: u32 = serde::de::Deserialize::deserialize(&mut *self)?;
                let val: Result<_> = seed.deserialize(idx.into_deserializer());
                Ok((val?, self))
            }
        }

        visitor.visit_enum(self)
    }

    fn deserialize_identifier<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(Error::DeserializeIdentifierNotSupported(self.pos))
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(Error::DeserializeIgnoredAnyNotSupported(self.pos))
    }
}

impl<'de, 'a> serde::de::VariantAccess<'de> for &'a mut Deserializer {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: serde::de::DeserializeSeed<'de>,
    {
        serde::de::DeserializeSeed::deserialize(seed, self)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        serde::de::Deserializer::deserialize_tuple(self, len, visitor)
    }

    fn struct_variant<V>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        serde::de::Deserializer::deserialize_tuple(self, fields.len(), visitor)
    }
}

// The serializer and deserializer are tested in the packet definition with real world data.
#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[test]
    fn test_primitive_struct() -> Result<()> {
        #[derive(Deserialize, PartialEq, Debug)]
        struct SimpleStruct {
            a: u8,
            b: i8,
            c: f32,
            d: f64,
        }

        let data = vec![
            0x12, 0xf3, 0xCD, 0xCC, 0x0C, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f,
        ];
        let expected = SimpleStruct {
            a: 18,
            b: -13,
            c: 2.2,
            d: 1.0,
        };

        let str = from_vec::<SimpleStruct>(data)?;
        assert_eq!(str, expected);
        Ok(())
    }
}
