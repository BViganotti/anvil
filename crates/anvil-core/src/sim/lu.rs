//! Dense LU factorisation with partial pivoting.
//!
//! Circuit matrices are small (a few hundred nodes at most) and dense assembly
//! is predictable and robust. The solver never panics: non-finite input,
//! singular matrices and size violations all return errors.

#[derive(Clone, Debug)]
pub struct Lu {
    n: usize,
    a: Vec<f64>,
    perm: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuError {
    TooLarge,
    Empty,
    NonFinite,
    Singular,
}

impl Lu {
    /// Factor an `n x n` row-major matrix. `a` is not modified.
    pub fn factor(matrix: &[f64], n: usize) -> Result<Lu, LuError> {
        if n == 0 {
            return Err(LuError::Empty);
        }
        if n > 2048 {
            return Err(LuError::TooLarge);
        }
        if matrix.len() != n.saturating_mul(n) {
            return Err(LuError::NonFinite);
        }
        let mut a = matrix.to_vec();
        if a.iter().any(|v| !v.is_finite()) {
            return Err(LuError::NonFinite);
        }
        let mut perm: Vec<usize> = (0..n).collect();
        for k in 0..n {
            let mut pivot = k;
            let mut best = a[k * n + k].abs();
            for i in (k + 1)..n {
                let v = a[i * n + k].abs();
                if v > best {
                    best = v;
                    pivot = i;
                }
            }
            if !(best > 1e-18) || !best.is_finite() {
                return Err(LuError::Singular);
            }
            if pivot != k {
                for j in 0..n {
                    a.swap(k * n + j, pivot * n + j);
                }
                perm.swap(k, pivot);
            }
            let diag = a[k * n + k];
            for i in (k + 1)..n {
                let factor = a[i * n + k] / diag;
                a[i * n + k] = factor;
                if factor == 0.0 {
                    continue;
                }
                for j in (k + 1)..n {
                    a[i * n + j] -= factor * a[k * n + j];
                }
            }
        }
        Ok(Lu { n, a, perm })
    }

    /// Solve `A x = b`.
    pub fn solve(&self, b: &[f64]) -> Result<Vec<f64>, LuError> {
        if b.len() != self.n {
            return Err(LuError::NonFinite);
        }
        if b.iter().any(|v| !v.is_finite()) {
            return Err(LuError::NonFinite);
        }
        let n = self.n;
        let mut y = vec![0.0; n];
        for i in 0..n {
            let mut s = b[self.perm[i]];
            for j in 0..i {
                s -= self.a[i * n + j] * y[j];
            }
            y[i] = s;
        }
        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let mut s = y[i];
            for j in (i + 1)..n {
                s -= self.a[i * n + j] * x[j];
            }
            let d = self.a[i * n + i];
            if !(d.abs() > 1e-18) {
                return Err(LuError::Singular);
            }
            x[i] = s / d;
            if !x[i].is_finite() {
                return Err(LuError::NonFinite);
            }
        }
        Ok(x)
    }

    pub fn size(&self) -> usize {
        self.n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_two_by_two() {
        // [[2,1],[1,3]] x = [5,10] => x = [1,3]
        let lu = Lu::factor(&[2.0, 1.0, 1.0, 3.0], 2).unwrap();
        let x = lu.solve(&[5.0, 10.0]).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-12);
        assert!((x[1] - 3.0).abs() < 1e-12);
    }

    #[test]
    fn pivots_when_needed() {
        // The first pivot is zero.
        let lu = Lu::factor(&[0.0, 1.0, 1.0, 1.0], 2).unwrap();
        let x = lu.solve(&[1.0, 2.0]).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-12);
        assert!((x[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn singular_matrix_errors() {
        assert_eq!(
            Lu::factor(&[1.0, 2.0, 2.0, 4.0], 2).err(),
            Some(LuError::Singular)
        );
    }

    #[test]
    fn rejects_non_finite() {
        assert_eq!(Lu::factor(&[f64::NAN], 1).err(), Some(LuError::NonFinite));
        assert_eq!(Lu::factor(&[], 0).err(), Some(LuError::Empty));
    }

    #[test]
    fn solves_three_by_three() {
        // Hilbert-like but well conditioned.
        let a = [4.0, -1.0, 0.0, -1.0, 4.0, -1.0, 0.0, -1.0, 3.0];
        let b = [15.0, 10.0, 10.0];
        let lu = Lu::factor(&a, 3).unwrap();
        let x = lu.solve(&b).unwrap();
        // Verify residual.
        for i in 0..3 {
            let mut sum = 0.0;
            for j in 0..3 {
                sum += a[i * 3 + j] * x[j];
            }
            assert!((sum - b[i]).abs() < 1e-10);
        }
    }
}
