#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Matrix {
    pub(crate) fn luma_weights(self) -> (f64, f64) {
        match self {
            Self::Bt601 => (0.299, 0.114),
            Self::Bt709 => (0.2126, 0.0722),
            Self::Bt2020 => (0.2627, 0.0593),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameColor {
    pub matrix: Matrix,
    pub full_range: bool,
}

impl FrameColor {
    pub const BT601_FULL: Self = Self {
        matrix: Matrix::Bt601,
        full_range: true,
    };

    pub fn assumed(height: usize, full_range: bool) -> Self {
        Self {
            matrix: if height >= 720 {
                Matrix::Bt709
            } else {
                Matrix::Bt601
            },
            full_range,
        }
    }

    pub fn from_matrix_coeffs(matrix_coeffs: u8, full_range: bool, height: usize) -> Self {
        let matrix = match matrix_coeffs {
            1 => Matrix::Bt709,
            5 | 6 => Matrix::Bt601,
            9 | 10 => Matrix::Bt2020,
            _ => return Self::assumed(height, full_range),
        };
        Self { matrix, full_range }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unspecified_matrix_uses_resolution_while_explicit_metadata_wins() {
        assert_eq!(
            FrameColor::from_matrix_coeffs(2, false, 1080).matrix,
            Matrix::Bt709
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(0, false, 720).matrix,
            Matrix::Bt709
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(2, false, 480).matrix,
            Matrix::Bt601
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(1, false, 480).matrix,
            Matrix::Bt709
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(6, true, 1080),
            FrameColor::BT601_FULL
        );
        assert_eq!(
            FrameColor::from_matrix_coeffs(9, false, 2160).matrix,
            Matrix::Bt2020
        );
    }
}
