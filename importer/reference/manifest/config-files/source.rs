// Copyright the Cargo contributors. MIT OR Apache-2.0.
// Unchanged source excerpts, review reference only; see README.md.
    fn _load_file(
        &self,
        path: &Path,
        seen: &mut HashSet<PathBuf>,
        includes: bool,
        why_load: WhyLoad,
    ) -> CargoResult<ConfigValue> {
        if !seen.insert(path.to_path_buf()) {
            bail!(
                "config `include` cycle detected with path `{}`",
                path.display()
            );
        }
        tracing::debug!(?path, ?why_load, includes, "load config from file");

        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read configuration file `{}`", path.display()))?;
        let toml = parse_document(&contents, path, self).with_context(|| {
            format!("could not parse TOML configuration in `{}`", path.display())
        })?;
        let def = match why_load {
            WhyLoad::Cli => Definition::Cli(Some(path.into())),
            WhyLoad::FileDiscovery => Definition::Path(path.into()),
        };
        let value = CV::from_toml(def, toml::Value::Table(toml)).with_context(|| {
            format!(
                "failed to load TOML configuration from `{}`",
                path.display()
            )
        })?;
        if includes {
            self.load_includes(value, seen, why_load)
        } else {
            Ok(value)
        }
    }

    /// Load any `include` files listed in the given `value`.
    ///
    /// Returns `value` with the given include files merged into it.
    ///
    /// * `seen` is used to check for cyclic includes.
    /// * `why_load` tells why a config is being loaded.
    fn load_includes(
        &self,
        mut value: CV,
        seen: &mut HashSet<PathBuf>,
        why_load: WhyLoad,
    ) -> CargoResult<CV> {
        // Get the list of files to load.
        let includes = self.include_paths(&mut value, true)?;

        // Accumulate all values here.
        let mut root = CV::Table(HashMap::default(), value.definition().clone());
        for include in includes {
            let Some(abs_path) = include.resolve_path(self) else {
                continue;
            };

            self._load_file(&abs_path, seen, true, why_load)
                .and_then(|include| root.merge(include, true))
                .with_context(|| {
                    format!(
                        "failed to load config include `{}` from `{}`",
                        include.path.display(),
                        include.def
                    )
                })?;
        }
        root.merge(value, true)?;
        Ok(root)
    }
