// Copyright the Cargo contributors. MIT OR Apache-2.0.
// Unchanged source sections; see README.md for test-only adapters.
use std::{mem, collections::hash_map::Entry};
use rustc_hash::FxHashMap as HashMap;
use super::{Definition, ConfigKey};
type CargoResult<T> = Result<T, String>;
#[allow(dead_code)]
enum KeyOrIdx { Key(String), Idx(usize) }
macro_rules! bail { ($($arg:tt)*) => { return Err(format!($($arg)*)) }; }
macro_rules! anyhow { ($($arg:tt)*) => { format!($($arg)*) }; }
trait Context<T> { fn with_context(self, message: impl FnOnce() -> String) -> Result<T,String>; }
impl<T> Context<T> for Result<T,String> {
    fn with_context(self, message: impl FnOnce()->String) -> Result<T,String> {
        self.map_err(|error| format!("{}: {error}", message()))
    }
}
const NON_MERGEABLE_LISTS: &[&str] = &[
    "credential-alias.*",
    "doc.browser",
    "host.runner",
    "registries.*.credential-provider",
    "registry.credential-provider",
    "target.*.runner",
];
#[derive(Eq, PartialEq, Clone)]
pub enum ConfigValue {
    Integer(i64, Definition),
    String(String, Definition),
    List(Vec<ConfigValue>, Definition),
    Table(HashMap<String, ConfigValue>, Definition),
    Boolean(bool, Definition),
}
use ConfigValue as CV;
impl ConfigValue {
    fn from_toml_inner(
        def: Definition,
        toml: toml::Value,
        path: &mut Vec<KeyOrIdx>,
    ) -> CargoResult<ConfigValue> {
        match toml {
            toml::Value::String(val) => Ok(CV::String(val, def)),
            toml::Value::Boolean(b) => Ok(CV::Boolean(b, def)),
            toml::Value::Integer(i) => Ok(CV::Integer(i, def)),
            toml::Value::Array(val) => Ok(CV::List(
                val.into_iter()
                    .enumerate()
                    .map(|(i, toml)| {
                        CV::from_toml_inner(def.clone(), toml, path)
                            .inspect_err(|_| path.push(KeyOrIdx::Idx(i)))
                    })
                    .collect::<CargoResult<_>>()?,
                def,
            )),
            toml::Value::Table(val) => Ok(CV::Table(
                val.into_iter()
                    .map(
                        |(key, value)| match CV::from_toml_inner(def.clone(), value, path) {
                            Ok(value) => Ok((key, value)),
                            Err(e) => {
                                path.push(KeyOrIdx::Key(key));
                                Err(e)
                            }
                        },
                    )
                    .collect::<CargoResult<_>>()?,
                def,
            )),
            v => bail!("unsupported TOML configuration type `{}`", v.type_str()),
        }
    }
    pub(super) fn merge(&mut self, from: ConfigValue, force: bool) -> CargoResult<()> {
        self.merge_helper(from, force, &mut ConfigKey::new())
    }

    fn merge_helper(
        &mut self,
        from: ConfigValue,
        force: bool,
        parts: &mut ConfigKey,
    ) -> CargoResult<()> {
        let is_higher_priority = from.definition().is_higher_priority(self.definition());
        match (self, from) {
            (&mut CV::List(ref mut old, _), CV::List(ref mut new, _)) => {
                if is_nonmergeable_list(&parts) {
                    // Use whichever list is higher priority.
                    if force || is_higher_priority {
                        mem::swap(new, old);
                    }
                } else {
                    // Merge the lists together.
                    if force {
                        old.append(new);
                    } else {
                        new.append(old);
                        mem::swap(new, old);
                    }
                }
                old.sort_by(|a, b| a.definition().cmp(b.definition()));
            }
            (&mut CV::Table(ref mut old, _), CV::Table(ref mut new, _)) => {
                for (key, value) in mem::take(new) {
                    match old.entry(key.clone()) {
                        Entry::Occupied(mut entry) => {
                            let new_def = value.definition().clone();
                            let entry = entry.get_mut();
                            parts.push(&key);
                            entry.merge_helper(value, force, parts).with_context(|| {
                                format!(
                                    "failed to merge key `{}` between \
                                     {} and {}",
                                    key,
                                    entry.definition(),
                                    new_def,
                                )
                            })?;
                        }
                        Entry::Vacant(entry) => {
                            entry.insert(value);
                        }
                    };
                }
            }
            // Allow switching types except for tables or arrays.
            (expected @ &mut CV::List(_, _), found)
            | (expected @ &mut CV::Table(_, _), found)
            | (expected, found @ CV::List(_, _))
            | (expected, found @ CV::Table(_, _)) => {
                return Err(anyhow!(
                    "failed to merge config value from `{}` into `{}`: expected {}, but found {}",
                    found.definition(),
                    expected.definition(),
                    expected.desc(),
                    found.desc()
                ));
            }
            (old, mut new) => {
                if force || is_higher_priority {
                    mem::swap(old, &mut new);
                }
            }
        }

        Ok(())
    }
    pub fn desc(&self) -> &'static str {
        match *self {
            CV::Table(..) => "table",
            CV::List(..) => "array",
            CV::String(..) => "string",
            CV::Boolean(..) => "boolean",
            CV::Integer(..) => "integer",
        }
    }

    pub fn definition(&self) -> &Definition {
        match self {
            CV::Boolean(_, def)
            | CV::Integer(_, def)
            | CV::String(_, def)
            | CV::List(_, def)
            | CV::Table(_, def) => def,
        }
    }
    pub(super) fn parse(source: &str, definition: Definition) -> CargoResult<Self> {
        let table: toml::Table = source.parse().map_err(|e: toml::de::Error|e.to_string())?;
        Self::from_toml_inner(definition, toml::Value::Table(table), &mut Vec::new())
    }
}
pub(super) fn is_nonmergeable_list(key: &ConfigKey) -> bool {
    NON_MERGEABLE_LISTS.iter().any(|l| key.matches(l))
}
