//! Counts and means for the check and the site.

use std::collections::BTreeMap;

pub fn count<'a>(values: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
}

/// A running mean, rounded half away from zero
#[derive(Clone, Copy, Debug, Default)]
pub struct Mean {
    sum: i64,
    n: i64,
}

impl Mean {
    pub fn get(self) -> Option<i64> {
        (self.n > 0).then(|| (self.sum + self.sum.signum() * self.n / 2) / self.n)
    }
}

impl Extend<i64> for Mean {
    fn extend<I: IntoIterator<Item = i64>>(&mut self, values: I) {
        for value in values {
            self.sum += value;
            self.n += 1;
        }
    }
}

impl FromIterator<i64> for Mean {
    fn from_iter<I: IntoIterator<Item = i64>>(values: I) -> Self {
        let mut mean = Self::default();
        mean.extend(values);
        mean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn means_round_half_away_from_zero() {
        assert_eq!(Mean::default().get(), None);
        assert_eq!([-80, -81].into_iter().collect::<Mean>().get(), Some(-81));
        assert_eq!([1, 2].into_iter().collect::<Mean>().get(), Some(2));
        assert_eq!(
            [-70, -71, -73].into_iter().collect::<Mean>().get(),
            Some(-71)
        );
    }
}
