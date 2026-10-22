enum ValueSource<'gctx, 'err> {
    /// The deserializer used to actually deserialize a Value struct.
    Deserializer {
        de: Deserializer<'gctx>,
        definition: Definition,
    },
    /// A [`ConfigValue`](CV).
    ///
    /// This is used for situations where you can't address type via a TOML key,
    /// such as a value inside an array.
    /// The [`ConfigSeqAccess`] doesn't know what type it should deserialize to
    /// so [`ArrayItemDeserializer`] needs to be able to handle all of them.
    ConfigValue {
        cv: CV,
        key_path: &'err mut ArrayItemKeyPath,
    },
}

impl<'gctx, 'err> ValueSource<'gctx, 'err> {
    fn with_deserializer(de: Deserializer<'gctx>) -> Result<ValueSource<'gctx, 'err>, ConfigError> {
        // Figure out where this key is defined.
        let definition = {
            let env = de.key.as_env_key();
            let env_def = Definition::Environment(env.to_string());
            match (de.gctx.env.contains_key(env), de.gctx.get_cv(&de.key)?) {
                (true, Some(cv)) => {
                    // Both, pick highest priority.
                    if env_def.is_higher_priority(cv.definition()) {
                        env_def
                    } else {
                        cv.definition().clone()
                    }
                }
                (false, Some(cv)) => cv.definition().clone(),
                // Assume it is an environment, even if the key is not set.
                // This can happen for intermediate tables, like
                // CARGO_FOO_BAR_* where `CARGO_FOO_BAR` is not set.
                (_, None) => env_def,
            }
        };

        Ok(Self::Deserializer { de, definition })
    }

    fn with_cv(cv: CV, key_path: &'err mut ArrayItemKeyPath) -> ValueSource<'gctx, 'err> {
        ValueSource::ConfigValue { cv, key_path }
    }
}

/// This is a deserializer that deserializes into a `Value<T>` for
/// configuration.
///
/// This is a special deserializer because it deserializes one of its struct
/// fields into the location that this configuration value was defined in.
///
/// See more comments in `value.rs` for the protocol used here.
struct ValueDeserializer<'gctx, 'err> {
    hits: u32,
    source: ValueSource<'gctx, 'err>,
}

impl<'gctx, 'err> ValueDeserializer<'gctx, 'err> {
    fn new(source: ValueSource<'gctx, 'err>) -> ValueDeserializer<'gctx, 'err> {
        Self { hits: 0, source }
    }

    fn definition(&self) -> &Definition {
        match &self.source {
            ValueSource::Deserializer { definition, .. } => definition,
            ValueSource::ConfigValue { cv, .. } => cv.definition(),
        }
    }
}

impl<'de, 'gctx, 'err> de::MapAccess<'de> for ValueDeserializer<'gctx, 'err> {
    type Error = ConfigError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        self.hits += 1;
        match self.hits {
            1 => seed
                .deserialize(value::VALUE_FIELD.into_deserializer())
                .map(Some),
            2 => seed
                .deserialize(value::DEFINITION_FIELD.into_deserializer())
                .map(Some),
            _ => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        // If this is the first time around we deserialize the `value` field
        // which is the actual deserializer
        if self.hits == 1 {
            return match &mut self.source {
                ValueSource::Deserializer { de, definition } => seed
                    .deserialize(de.clone())
                    .map_err(|e| e.with_key_context(&de.key, Some(definition.clone()))),
                ValueSource::ConfigValue { cv, key_path } => {
                    let de = ArrayItemDeserializer {
                        cv: cv.clone(),
                        key_path,
                    };
                    seed.deserialize(de)
                }
            };
        }

        // ... otherwise we're deserializing the `definition` field, so we need
        // to figure out where the field we just deserialized was defined at.
        match self.definition() {
            Definition::BuiltIn => seed.deserialize(0.into_deserializer()),
            Definition::Path(path) => {
                seed.deserialize(Tuple2Deserializer(1i32, path.to_string_lossy()))
            }
            Definition::Environment(env) => {
                seed.deserialize(Tuple2Deserializer(2i32, env.as_str()))
            }
            Definition::Cli(path) => {
                let s = path
                    .as_ref()
                    .map(|p| p.to_string_lossy())
                    .unwrap_or_default();
                seed.deserialize(Tuple2Deserializer(3i32, s))
            }
        }
    }
}

struct Tuple2Deserializer<T, U>(T, U);

impl<'de, T, U> de::Deserializer<'de> for Tuple2Deserializer<T, U>
where
    T: IntoDeserializer<'de, ConfigError>,
    U: IntoDeserializer<'de, ConfigError>,
{
    type Error = ConfigError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, ConfigError>
    where
        V: de::Visitor<'de>,
    {
        struct SeqVisitor<T, U> {
            first: Option<T>,
            second: Option<U>,
        }
        impl<'de, T, U> de::SeqAccess<'de> for SeqVisitor<T, U>
        where
            T: IntoDeserializer<'de, ConfigError>,
            U: IntoDeserializer<'de, ConfigError>,
        {
            type Error = ConfigError;
            fn next_element_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
            where
                K: de::DeserializeSeed<'de>,
            {
                if let Some(first) = self.first.take() {
                    return seed.deserialize(first.into_deserializer()).map(Some);
                }
                if let Some(second) = self.second.take() {
                    return seed.deserialize(second.into_deserializer()).map(Some);
                }
                Ok(None)
            }
        }

        visitor.visit_seq(SeqVisitor {
            first: Some(self.0),
            second: Some(self.1),
        })
    }

    serde::forward_to_deserialize_any! {
        bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string seq
        bytes byte_buf map struct option unit newtype_struct
        ignored_any unit_struct tuple_struct tuple enum identifier
    }
}
