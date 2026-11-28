//! Small dense normal equations for the calibration fit.

pub(crate) struct NormalEquations<const N: usize> {
    matrix: [[f64; N]; N],
    rhs: [f64; N],
}

impl<const N: usize> NormalEquations<N> {
    pub(crate) fn new() -> Self {
        Self {
            matrix: [[0.0; N]; N],
            rhs: [0.0; N],
        }
    }

    pub(crate) fn add_row(&mut self, row: [f64; N], target: f64) {
        for (index, &coefficient) in row.iter().enumerate() {
            for (entry, other) in self.matrix[index].iter_mut().zip(row) {
                *entry += coefficient * other;
            }
            self.rhs[index] += coefficient * target;
        }
    }

    /// Eliminates with partial pivoting and rejects a small relative pivot.
    pub(crate) fn solve(self, min_pivot_ratio: f64) -> Option<[f64; N]> {
        let Self {
            mut matrix,
            mut rhs,
        } = self;
        if N == 0
            || !(0.0..=1.0).contains(&min_pivot_ratio)
            || !matrix.iter().flatten().chain(&rhs).all(|x| x.is_finite())
        {
            return None;
        }

        let mut largest_pivot: f64 = 0.0;
        let mut smallest_pivot = f64::INFINITY;
        for column in 0..N {
            let pivot_row = (column..N)
                .max_by(|&a, &b| matrix[a][column].abs().total_cmp(&matrix[b][column].abs()))?;
            matrix.swap(column, pivot_row);
            rhs.swap(column, pivot_row);
            let pivot = matrix[column][column];
            if !pivot.is_finite() || pivot == 0.0 {
                return None;
            }
            largest_pivot = largest_pivot.max(pivot.abs());
            smallest_pivot = smallest_pivot.min(pivot.abs());

            let pivot_row = matrix[column];
            let pivot_target = rhs[column];
            for (row, target) in matrix.iter_mut().zip(&mut rhs).skip(column + 1) {
                let factor = row[column] / pivot;
                row[column] = 0.0;
                for (entry, &other) in row.iter_mut().zip(&pivot_row).skip(column + 1) {
                    *entry -= factor * other;
                }
                *target -= factor * pivot_target;
            }
        }
        if smallest_pivot < largest_pivot * min_pivot_ratio {
            return None;
        }

        let mut solution = [0.0; N];
        for index in (0..N).rev() {
            let known_sum: f64 = matrix[index]
                .iter()
                .zip(&solution)
                .skip(index + 1)
                .map(|(coefficient, value)| coefficient * value)
                .sum();
            solution[index] = (rhs[index] - known_sum) / matrix[index][index];
        }
        solution
            .iter()
            .all(|value| value.is_finite())
            .then_some(solution)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_pivoting_solves_with_off_diagonal_dominance() {
        let mut fit = NormalEquations::<2>::new();
        fit.add_row([1.0, 4.0], 11.0);
        fit.add_row([1.0, 1.0], 5.0);
        let actual = fit.solve(1e-6).unwrap();
        assert!((actual[0] - 3.0).abs() < 1e-10);
        assert!((actual[1] - 2.0).abs() < 1e-10);
    }

    #[test]
    fn rejects_rank_deficiency_and_a_small_relative_pivot() {
        let mut fit = NormalEquations::<2>::new();
        fit.add_row([1.0, 1.0], 2.0);
        fit.add_row([2.0, 2.0], 4.0);
        assert!(fit.solve(1e-6).is_none());
        let mut fit = NormalEquations::<2>::new();
        fit.add_row([0.0001, 0.0], 1.0);
        fit.add_row([0.0, 1.0], 1.0);
        assert!(fit.solve(1e-6).is_none());
    }
}
