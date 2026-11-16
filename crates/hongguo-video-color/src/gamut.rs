use crate::{Matrix, Primaries};

pub fn from_cicp(value: u8) -> Result<Primaries, String> {
    match value {
        1 => Ok(Primaries::BT709),
        9 => Ok(Primaries::BT2020),
        12 => Ok(Primaries::P3_D65),
        5 => Ok(Primaries {
            red: [0.64, 0.33],
            green: [0.29, 0.60],
            blue: [0.15, 0.06],
            white: [0.3127, 0.3290],
        }),
        6 | 7 => Ok(Primaries {
            red: [0.630, 0.340],
            green: [0.310, 0.595],
            blue: [0.155, 0.070],
            white: [0.3127, 0.3290],
        }),
        _ => Err(format!("unsupported CICP primaries {value}")),
    }
}

fn xyz(xy: [f64; 2]) -> Result<[f64; 3], String> {
    if xy.iter().any(|value| !value.is_finite())
        || xy[1] <= 0.0
        || xy[0] < 0.0
        || xy[0] + xy[1] > 1.0 + 1e-6
    {
        return Err("invalid display chromaticity".into());
    }
    Ok([xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]])
}

fn inverse(m: Matrix) -> Result<Matrix, String> {
    let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if determinant.abs() < 1e-12 {
        return Err("singular display gamut".into());
    }
    Ok(std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            let rows = [(column + 1) % 3, (column + 2) % 3];
            let columns = [(row + 1) % 3, (row + 2) % 3];
            (m[rows[0]][columns[0]] * m[rows[1]][columns[1]]
                - m[rows[0]][columns[1]] * m[rows[1]][columns[0]])
                / determinant
        })
    }))
}

fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| (0..3).map(|index| a[row][index] * b[index][column]).sum())
    })
}

fn vector(matrix: Matrix, value: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row.into_iter().zip(value).map(|(a, b)| a * b).sum())
}

fn rgb_to_xyz(primaries: Primaries) -> Result<Matrix, String> {
    let colors = [
        xyz(primaries.red)?,
        xyz(primaries.green)?,
        xyz(primaries.blue)?,
    ];
    let matrix = std::array::from_fn(|row| std::array::from_fn(|column| colors[column][row]));
    let scale = vector(inverse(matrix)?, xyz(primaries.white)?);
    Ok(std::array::from_fn(|row| {
        std::array::from_fn(|column| matrix[row][column] * scale[column])
    }))
}

pub fn luminance(primaries: Primaries) -> Result<[f64; 3], String> {
    Ok(rgb_to_xyz(primaries)?[1])
}

pub fn conversion(source: Primaries, target: Primaries) -> Result<Matrix, String> {
    const BRADFORD: Matrix = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let source_white = vector(BRADFORD, xyz(source.white)?);
    let target_white = vector(BRADFORD, xyz(target.white)?);
    let adaptation = product(
        inverse(BRADFORD)?,
        std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                BRADFORD[row][column] * target_white[row] / source_white[row]
            })
        }),
    );
    Ok(product(
        inverse(rgb_to_xyz(target)?)?,
        product(adaptation, rgb_to_xyz(source)?),
    ))
}
