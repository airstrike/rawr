//! Bounded diagnostic attribution, never a source of work authority.

const SITES: usize = 256;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Counts {
    pub admitted: u128,
    /// Requested increments refused by the original account; not charged usage.
    pub rejected: u128,
    pub saturated: bool,
}

impl Counts {
    fn record(&mut self, amount: usize, admitted: bool) {
        let counter = if admitted {
            &mut self.admitted
        } else {
            &mut self.rejected
        };
        match counter.checked_add(amount as u128) {
            Some(value) => *counter = value,
            None => {
                *counter = u128::MAX;
                self.saturated = true;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Row {
    pub file: &'static str,
    pub line: u32,
    pub counts: Counts,
}

/// Fixed diagnostic storage is part of the graph object, with no separate heap
/// allocation, globals, fresh account, or bookkeeping in the authority vector.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Paths {
    pub enabled: bool,
    pub rows: [Option<Row>; SITES],
    /// Attribution beyond the fixed site capacity remains included here.
    pub overflow: Counts,
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            enabled: option_env!("RAWR_SCOPE_CENSUS") == Some("1"),
            rows: [None; SITES],
            overflow: Counts::default(),
        }
    }
}

impl Paths {
    pub(super) fn record(
        &mut self,
        caller: &'static std::panic::Location<'static>,
        amount: usize,
        admitted: bool,
    ) {
        self.record_site(caller.file(), caller.line(), amount, admitted);
    }

    fn record_site(&mut self, file: &'static str, line: u32, amount: usize, admitted: bool) {
        if !self.enabled {
            return;
        }
        for row in &mut self.rows {
            match row {
                Some(row) if row.file == file && row.line == line => {
                    row.counts.record(amount, admitted);
                    return;
                }
                None => {
                    let mut counts = Counts::default();
                    counts.record(amount, admitted);
                    *row = Some(Row { file, line, counts });
                    return;
                }
                _ => {}
            }
        }
        self.overflow.record(amount, admitted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_attribution_overflow_preserves_admitted_and_rejected_amounts() {
        let mut paths = Paths {
            enabled: true,
            ..Paths::default()
        };
        for line in 0..SITES as u32 + 2 {
            paths.record_site("fixture", line, 3, true);
            paths.record_site("fixture", line, 5, false);
        }
        assert_eq!(
            paths
                .rows
                .iter()
                .flatten()
                .map(|row| row.counts.admitted)
                .sum::<u128>()
                + paths.overflow.admitted,
            (SITES as u128 + 2) * 3
        );
        assert_eq!(paths.overflow.admitted, 6);
        assert_eq!(paths.overflow.rejected, 10);
        assert!(!paths.overflow.saturated);
    }

    #[test]
    fn diagnostic_counts_reconcile_without_changing_exceedance_or_usage() {
        let graph = super::super::GraphBudget::new();
        let line = line!() + 1;
        graph.paths(7).unwrap();
        let paths = graph.census_paths().unwrap();
        assert_eq!(
            paths.enabled,
            option_env!("RAWR_SCOPE_CENSUS") == Some("1")
        );
        if paths.enabled {
            let row = paths
                .rows
                .iter()
                .flatten()
                .find(|row| row.line == line && row.file == file!())
                .unwrap();
            assert_eq!(row.counts.admitted, 7);
        }
        graph
            .paths(super::super::MAX_PATH_CHECKS_PER_GRAPH - 7)
            .unwrap();
        // The path-check target is accounting; one more check is
        // admitted and recorded with its truthful high water.
        graph.paths(1).unwrap();
        assert_eq!(
            graph.census_usage().unwrap().1,
            super::super::MAX_PATH_CHECKS_PER_GRAPH + 1
        );
        let axis = *graph
            .exceedances()
            .axis(super::super::PATHS_RESOURCE)
            .unwrap();
        assert_eq!(
            axis.high_water,
            super::super::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
        );
        let paths = graph.census_paths().unwrap();
        let admitted = paths
            .rows
            .iter()
            .flatten()
            .map(|row| row.counts.admitted)
            .sum::<u128>()
            + paths.overflow.admitted;
        let rejected = paths
            .rows
            .iter()
            .flatten()
            .map(|row| row.counts.rejected)
            .sum::<u128>()
            + paths.overflow.rejected;
        assert_eq!(
            admitted,
            if paths.enabled {
                super::super::MAX_PATH_CHECKS_PER_GRAPH as u128 + 1
            } else {
                0
            }
        );
        assert_eq!(rejected, 0);
    }
}
