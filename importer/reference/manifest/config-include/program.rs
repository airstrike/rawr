// Copyright the Cargo contributors. MIT OR Apache-2.0.
struct ConfigInclude {
    /// Path to a config-include configuration file.
    /// Could be either relative or absolute.
    path: PathBuf,
    def: Definition,
    /// Whether this include is optional (missing files are silently ignored)
    optional: bool,
}

impl ConfigInclude {
    fn new(p: impl Into<PathBuf>, def: Definition) -> Self {
        Self {
            path: p.into(),
            def,
            optional: false,
        }
    }

}
struct GlobalContext;
impl GlobalContext {
    fn include_paths(&self, cv: &mut CV, remove: bool) -> CargoResult<Vec<ConfigInclude>> {
        let CV::Table(table, _def) = cv else {
            unreachable!()
        };
        let include = if remove {
            table.remove("include").map(Cow::Owned)
        } else {
            table.get("include").map(Cow::Borrowed)
        };
        let includes = match include.map(|c| c.into_owned()) {
            Some(CV::List(list, _def)) => list
                .into_iter()
                .enumerate()
                .map(|(idx, cv)| match cv {
                    CV::String(s, def) => Ok(ConfigInclude::new(s, def)),
                    CV::Table(mut table, def) => {
                        // Extract `include.path`
                        let s = match table.remove("path") {
                            Some(CV::String(s, _)) => s,
                            Some(other) => bail!(
                                "expected a string, but found {} at `include[{idx}].path` in `{def}`",
                                other.desc()
                            ),
                            None => bail!("missing field `path` at `include[{idx}]` in `{def}`"),
                        };

                        // Extract optional `include.optional` field
                        let optional = match table.remove("optional") {
                            Some(CV::Boolean(b, _)) => b,
                            Some(other) => bail!(
                                "expected a boolean, but found {} at `include[{idx}].optional` in `{def}`",
                                other.desc()
                            ),
                            None => false,
                        };

                        let mut include = ConfigInclude::new(s, def);
                        include.optional = optional;
                        Ok(include)
                    }
                    other => bail!(
                        "expected a string or table, but found {} at `include[{idx}]` in {}",
                        other.desc(),
                        other.definition(),
                    ),
                })
                .collect::<CargoResult<Vec<_>>>()?,
            Some(other) => bail!(
                "expected a list of strings or a list of tables, but found {} at `include` in `{}",
                other.desc(),
                other.definition()
            ),
            None => {
                return Ok(Vec::new());
            }
        };

        for include in &includes {
            if include.path.extension() != Some(OsStr::new("toml")) {
                bail!(
                    "expected a config include path ending with `.toml`, \
                     but found `{}` from `{}`",
                    include.path.display(),
                    include.def,
                )
            }

            if let Some(path) = include.path.to_str() {
                // Ignore non UTF-8 bytes as glob and template syntax are for textual config.
                if is_glob_pattern(path) {
                    bail!(
                        "expected a config include path without glob patterns, \
                         but found `{}` from `{}`",
                        include.path.display(),
                        include.def,
                    )
                }
                if path.contains(&['{', '}']) {
                    bail!(
                        "expected a config include path without template braces, \
                         but found `{}` from `{}`",
                        include.path.display(),
                        include.def,
                    )
                }
            }
        }

        Ok(includes)
    }

}
