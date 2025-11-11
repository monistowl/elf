#[cfg(feature = "polars")]
pub mod polars_io {
    use anyhow::Result;
    use polars::prelude::*;

    /// Load a single-column CSV as f64 vector. Assumes header with column name.
    pub fn load_column(path: &str, col: &str) -> Result<Vec<f64>> {
        let df = CsvReadOptions::default()
            .try_into_reader_with_file_path(Some(path.into()))?
            .finish()?;
        let s = df.column(col)?;
        Ok(s.f64()?.into_no_null_iter().collect())
    }
}
